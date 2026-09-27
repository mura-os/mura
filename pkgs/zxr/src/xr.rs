//! The runtime boundary (specs/zxr-core.md §2, §3 `xr`): openxrs instance/system/session on a
//! Vulkan device the runtime creates (`XR_KHR_vulkan_enable2`), the two per-view swapchains, the
//! reference space, `xrLocateViews`, and the **wait thread** — `xrWaitFrame` blocks there and
//! posts each `FrameState` into the state loop (ADR 0006 amendment 2026-09-26). The state loop
//! owns begin/end; the two threads exchange exactly one frame in flight.

use ash::vk;
use ash::vk::Handle as _;
use openxr as xr;
use std::ffi::{c_void, CStr, CString};
use smithay::reexports::calloop::channel as cchannel;
use crate::input::actions::{self, Act, Ext, On, ProfileKind};
use crate::input::bridge;
use crate::input::{AxisSource, Flags, Quality, Sample, Side, SourceKind};
use crate::journal::{timed, Lat};
use std::sync::{Arc, Condvar, Mutex};


pub const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

/// What the wait thread hands the state loop per frame.
#[derive(Debug, Clone, Copy)]
pub struct FrameTick {
    pub frame_id: u64,
    pub predicted_display_time: xr::Time,
    pub predicted_display_period: xr::Duration,
    pub should_render: bool,
    /// When `xrWaitFrame` returned (monotonic ns), for the frame journal.
    pub woke_at_ns: u64,
    /// how long `xrWaitFrame` blocked (ns) — the runtime-side throttle, not our cost
    pub wait_ns: u64,
}

/// The Vulkan objects the runtime created for us; the renderer borrows them.
#[allow(dead_code)]
pub struct VkCore {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue_family: u32,
    pub queue: vk::Queue,
}

#[allow(dead_code)]
pub struct XrCore {
    pub instance: xr::Instance,
    pub system: xr::SystemId,
    pub session: xr::Session<xr::Vulkan>,
    pub stream: xr::FrameStream<xr::Vulkan>,
    pub space: xr::Space,
    pub views: Vec<xr::ViewConfigurationView>,
    pub swapchains: Vec<Swapchain>,
    pub color_format: vk::Format,
    pub environment_blend: xr::EnvironmentBlendMode,
    /// `XrSystemGraphicsProperties::maxLayerCount` — bounds the quad layers (spec §7 rev 3)
    pub max_layer_count: u32,
    /// the instance enabled `XR_KHR_composition_layer_color_scale_bias` (emphasis; else none)
    pub color_scale_bias: bool,
    pub session_running: bool,
    pub exit_requested: bool,
    /// `XR_EXT_user_presence` (`XrEventDataUserPresenceChangedEXT`): the last value the runtime
    /// reported, taken by `input::tick`; `None` = no event yet (or the extension is absent)
    pub presence_event: Option<bool>,
    /// per-call latencies (research/63 Phase 0); the loop merges them into the journal
    pub calls: crate::journal::Calls,
    /// The action set — the XR source seam (spatial-input §1a; `input/actions.rs` documents the
    /// flow). `None` only if the runtime refused the set (logged); the head ray still works.
    pub actions: Option<Actions>,
    /// the head pose of the last `locate_views` (LOCAL), for the joint bridge
    head: xr::Posef,
    events: xr::EventDataBuffer,
    /// The wait thread's handshake: it may call `xrWaitFrame` again only once the previous frame
    /// has been begun (rendering.adoc:792-794) and only while the session is running.
    handshake: Arc<(Mutex<Handshake>, Condvar)>,
    ticks: Option<cchannel::Channel<FrameTick>>,
}

/// One of the action-space poses zxr caches per tick (spatial-input §2 sources): the spaces are
/// scene frames (`Space::Xr`) so the batched `xrLocateSpacesKHR` the loop already makes locates
/// them (spec §5a); `locate_spaces` writes each located action space back here by handle, and
/// `sync_samples` reads the cache when it builds the tick's samples.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoseTag {
    Aim(Side),
    Grip(Side),
    Poke(Side),
    Gaze,
}

impl PoseTag {
    pub const ALL: [PoseTag; 7] = [PoseTag::Aim(Side::Left), PoseTag::Aim(Side::Right), PoseTag::Grip(Side::Left), PoseTag::Grip(Side::Right), PoseTag::Poke(Side::Left), PoseTag::Poke(Side::Right), PoseTag::Gaze];

    fn index(self) -> usize {
        match self {
            PoseTag::Aim(Side::Left) => 0,
            PoseTag::Aim(Side::Right) => 1,
            PoseTag::Grip(Side::Left) => 2,
            PoseTag::Grip(Side::Right) => 3,
            PoseTag::Poke(Side::Left) => 4,
            PoseTag::Poke(Side::Right) => 5,
            PoseTag::Gaze => 6,
        }
    }
}

fn side_index(s: Side) -> usize {
    match s {
        Side::Left => 0,
        Side::Right => 1,
    }
}

/// Per-hand intake state between ticks: the bound profile (refreshed on
/// `XrEventDataInteractionProfileChanged`, `input.adoc:478-491`), the last button levels for
/// edge emission, whether the stick was live (so one zero sample closes an axis run), and the
/// joint bridge's hysteresis/hold state (§10).
struct HandState {
    profile: xr::Path,
    kind: Option<SourceKind>,
    buttons: [bool; actions::BUTTONS.len()],
    stick_live: bool,
    /// the bridge produced a sample last tick (so a lost hand gets one closing sample)
    bridge_live: bool,
    bridge: bridge::State,
}

impl Default for HandState {
    fn default() -> Self {
        HandState { profile: xr::Path::NULL, kind: None, buttons: [false; actions::BUTTONS.len()], stick_live: false, bridge_live: false, bridge: bridge::State::default() }
    }
}

/// The `mura` action set and everything read through it (spatial-input §1a "The XR source seam
/// is the OpenXR action set"; research/68 §5.3). One action per semantic input, suggested
/// bindings per profile from `actions::PROFILES`, one space per pose action and hand.
#[allow(dead_code)]
pub struct Actions {
    set: xr::ActionSet,
    aim_pose: xr::Action<xr::Posef>,
    grip_pose: xr::Action<xr::Posef>,
    poke_pose: xr::Action<xr::Posef>,
    gaze_pose: xr::Action<xr::Posef>,
    select: xr::Action<bool>,
    menu: xr::Action<bool>,
    system: xr::Action<bool>,
    secondary: xr::Action<bool>,
    ready: xr::Action<bool>,
    stick: xr::Action<xr::Vector2f>,
    pinch: xr::Action<f32>,
    aim_activate: xr::Action<f32>,
    grasp: xr::Action<f32>,
    hand_paths: [xr::Path; 2],
    /// the profiles bindings were suggested for, by path, with the kind each maps to
    profiles: Vec<(xr::Path, ProfileKind)>,
    /// the action spaces until `actions::register_frames` moves them into the scene
    spaces: Vec<(PoseTag, xr::Space)>,
    /// the same spaces' raw handles, for the write-back in `locate_spaces`
    handles: Vec<(PoseTag, xr::sys::Space)>,
    /// this tick's located action poses, by `PoseTag::index`
    poses: [(xr::Posef, xr::SpaceLocationFlags); 7],
    /// `XR_EXT_hand_tracking` trackers for the bridge (§10), when the runtime has hands
    trackers: [Option<xr::HandTracker>; 2],
    hands: [HandState; 2],
    profile_dirty: bool,
    /// `XR_EXT_eye_gaze_interaction` enabled: a gaze sample is produced every tick
    eye_gaze: bool,
    sync_failed: bool,
    /// local counters until `journal::Calls` grows the fields (see the lane report): one
    /// `xrSyncActions` per tick = N_devices `update_inputs` RPCs on Monado
    /// (`oxr_input.c:2045-2050`, `ipc_client_xdev.c:37-70`); `xrGetActionState*` and
    /// `xrGetCurrentInteractionProfile` are client-side there; `xrLocateHandJointsEXT` is one RPC
    pub sync_actions_lat: Lat,
    pub get_action_state_lat: Lat,
    /// the joint bridge's configuration (settings.rs `Prefs::bridge_cfg`; `input.hand.dominant`,
    /// `input.body.*`, `system.gesture.hold_ms`, the pinch metre ladder)
    pub bridge_cfg: bridge::BridgeCfg,
    pub hand_joints_lat: Lat,
}

#[derive(Default)]
struct Handshake {
    /// frames begun by the loop
    begun: u64,
    /// `xrBeginSession` done and `xrEndSession` not yet
    running: bool,
}

pub struct Swapchain {
    pub handle: xr::Swapchain<xr::Vulkan>,
    pub images: Vec<vk::Image>,
    pub extent: vk::Extent2D,
}

/// One panel handed to the runtime as an `XrCompositionLayerQuad` (spec §4 rev 3).
pub struct QuadLayer<'a> {
    pub swapchain: &'a Swapchain,
    pub pose: xr::Posef,
    /// metres
    pub size: [f32; 2],
    /// the part of the image the panel occupies (grow-only swapchains, spec §5a): top-left
    /// `image_extent` pixels
    pub image_extent: [u32; 2],
    /// touch-class hover emphasis ∈ [0,1] (spatial-input §4) — applied as `XR_KHR_composition_layer_color_scale_bias`
    /// when the runtime has it (Monado: `oxr_extension_support.py:47`); no fallback pass without it
    pub emphasis: f32,
}

/// `xrGetSystemProperties` with the eye-gaze and hand-tracking property structs chained (the
/// openxr crate's `system_properties` takes no chain): `(supports_eye_gaze, supports_hand_tracking)`,
/// each `false` unless its extension is enabled and the system says yes.
fn system_supports(instance: &xr::Instance, system: xr::SystemId, eye_gaze_enabled: bool, hand_tracking_enabled: bool) -> (bool, bool) {
    let mut eyes = xr::sys::SystemEyeGazeInteractionPropertiesEXT { ty: xr::sys::SystemEyeGazeInteractionPropertiesEXT::TYPE, next: std::ptr::null_mut(), supports_eye_gaze_interaction: false.into() };
    let mut hands = xr::sys::SystemHandTrackingPropertiesEXT { ty: xr::sys::SystemHandTrackingPropertiesEXT::TYPE, next: std::ptr::null_mut(), supports_hand_tracking: false.into() };
    let mut chain: *mut c_void = std::ptr::null_mut();
    if eye_gaze_enabled {
        eyes.next = chain;
        chain = &mut eyes as *mut _ as *mut c_void;
    }
    if hand_tracking_enabled {
        hands.next = chain;
        chain = &mut hands as *mut _ as *mut c_void;
    }
    if chain.is_null() {
        return (false, false);
    }
    // SAFETY: the chained structs outlive the call; the runtime writes only the chained members.
    let r = unsafe {
        let mut p = xr::sys::SystemProperties { ty: xr::sys::SystemProperties::TYPE, next: chain, ..std::mem::zeroed() };
        (instance.fp().get_system_properties)(instance.as_raw(), system, &mut p)
    };
    if r.into_raw() < 0 {
        tracing::warn!("xrGetSystemProperties (eye gaze / hand tracking): {r:?}");
        return (false, false);
    }
    (eye_gaze_enabled && eyes.supports_eye_gaze_interaction.into(), hand_tracking_enabled && hands.supports_hand_tracking.into())
}

fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: plain clock_gettime into a stack struct.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

impl XrCore {
    /// Instance → system → Vulkan instance/device from the runtime → session → swapchains →
    /// reference space, then the wait thread. `loader`: the OpenXR loader path (baked by Nix).
    /// `overlay`: create an `XR_EXTX_overlay` session at this layer placement (native-openxr-apps.md
    /// §2: zxr is always an overlay session in production; `None` keeps the plain main session
    /// for measurement).
    pub fn new(loader: &str, app_name: &str, overlay: Option<u32>) -> Result<(XrCore, VkCore), String> {
        // SAFETY: loading the OpenXR loader shared object by path.
        let entry = unsafe { xr::Entry::load_from(std::path::Path::new(loader), &()) }.map_err(|e| format!("openxr loader {loader}: {e}"))?;
        let available = entry.enumerate_extensions().map_err(|e| e.to_string())?;
        if !available.khr_vulkan_enable2 {
            return Err("runtime lacks XR_KHR_vulkan_enable2".into());
        }
        let mut exts = xr::ExtensionSet::default();
        exts.khr_vulkan_enable2 = true;
        // one round trip for every frame the views do not give (spec §5a); Monado always has it
        exts.khr_locate_spaces = available.khr_locate_spaces;
        // headset on/off as a session event (spatial-input §1a; Monado reports it from the head
        // device's HEAD_DETECT input — not on the simulated HMD)
        exts.ext_user_presence = available.ext_user_presence;
        // spatial-input §4: plane-level emphasis of the targeted member on its quad layer
        exts.khr_composition_layer_color_scale_bias = available.khr_composition_layer_color_scale_bias;
        // the XR sources (spatial-input §2), each only when advertised: the hand-interaction and
        // eye-gaze profiles bind through the action set; hand tracking feeds the §10 bridge
        // while Monado has no `EXT_hand_interaction` device (research/63 §1 "Monado status")
        exts.ext_hand_interaction = available.ext_hand_interaction;
        exts.ext_eye_gaze_interaction = available.ext_eye_gaze_interaction;
        exts.ext_hand_tracking = available.ext_hand_tracking;
        // `XR_MNDX_system_buttons` (Monado's preview extension exposing controller system buttons
        // through `/virtual_profiles/mndx/*_system_button`, `auxiliary/bindings/bindings.json:82-140`)
        // is not in openxr 0.22's `ExtensionSet` (no `mndx_system_buttons` in `generated.rs`);
        // enabled by name when advertised so the `system` action can bind on those profiles
        let mndx_system_buttons = available.other.iter().any(|n| n.as_slice() == actions::MNDX_SYSTEM_BUTTONS.as_bytes());
        if mndx_system_buttons {
            exts.other.push(actions::MNDX_SYSTEM_BUTTONS.as_bytes().to_vec());
        }
        if overlay.is_some() {
            if !available.extx_overlay {
                return Err("runtime lacks XR_EXTX_overlay (needed for --overlay)".into());
            }
            exts.extx_overlay = true;
        }
        let instance = entry
            .create_instance(&xr::ApplicationInfo { application_name: app_name, application_version: 1, engine_name: "zxr", engine_version: 1, api_version: xr::Version::new(1, 0, 0) }, &exts, &[], &())
            .map_err(|e| format!("xrCreateInstance: {e}"))?;
        let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY).map_err(|e| format!("no HMD system: {e}"))?;
        let reqs = instance.graphics_requirements::<xr::Vulkan>(system).map_err(|e| e.to_string())?;
        tracing::info!(min = %reqs.min_api_version_supported, max = %reqs.max_api_version_supported, "runtime Vulkan requirements");

        // ---- Vulkan instance, created by the runtime (research/59 §3) ----
        // SAFETY: ash's loader; the pointers we hand to the runtime outlive the calls.
        let vk_entry = unsafe { ash::Entry::load() }.map_err(|e| format!("vulkan loader: {e}"))?;
        let get_ipa: xr::sys::platform::VkGetInstanceProcAddr = unsafe {
            std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr as *const c_void)
        };
        let app_name_c = CString::new(app_name).unwrap();
        let app_info = vk::ApplicationInfo::default().application_name(&app_name_c).application_version(1).engine_name(c"zxr").engine_version(1).api_version(vk::make_api_version(0, 1, 2, 0));
        let inst_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let vk_instance_raw = unsafe {
            instance
                .create_vulkan_instance(system, get_ipa, &inst_info as *const _ as *const _)
                .map_err(|e| format!("xrCreateVulkanInstanceKHR: {e}"))?
                .map_err(|r| format!("xrCreateVulkanInstanceKHR: vk {r}"))?
        };
        let vk_instance = unsafe { ash::Instance::load(vk_entry.static_fn(), vk::Instance::from_raw(vk_instance_raw as u64)) };
        let physical_raw = unsafe { instance.vulkan_graphics_device(system, vk_instance_raw).map_err(|e| format!("xrGetVulkanGraphicsDevice2KHR: {e}"))? };
        let physical = vk::PhysicalDevice::from_raw(physical_raw as u64);
        let props = unsafe { vk_instance.get_physical_device_properties(physical) };
        tracing::info!(device = %unsafe { CStr::from_ptr(props.device_name.as_ptr()) }.to_string_lossy(), "runtime-selected Vulkan device");

        // ---- Vulkan device, created by the runtime, with our import/sync extensions ----
        let qfams = unsafe { vk_instance.get_physical_device_queue_family_properties(physical) };
        let queue_family = qfams.iter().position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS)).ok_or("no graphics queue family")? as u32;
        let prio = [1.0f32];
        let qinfo = [vk::DeviceQueueCreateInfo::default().queue_family_index(queue_family).queue_priorities(&prio)];
        let device_exts: Vec<&CStr> = vec![
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::ext::image_drm_format_modifier::NAME,
            ash::khr::external_semaphore_fd::NAME,
            ash::khr::external_fence_fd::NAME,
            ash::ext::queue_family_foreign::NAME,
        ];
        let ext_ptrs: Vec<*const i8> = device_exts.iter().map(|e| e.as_ptr()).collect();
        // Timeline semaphores via the KHR feature struct, not Vulkan12Features: Monado inserts a
        // VkPhysicalDeviceTimelineSemaphoreFeatures itself unless one is already in the chain
        // (oxr_vulkan.c:491-508), and Vulkan12Features + that struct together violate
        // VUID-VkDeviceCreateInfo-pNext-02830 (found by the validation layer, research/65 §0.2).
        let mut features12 = vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
        let dev_info = vk::DeviceCreateInfo::default().queue_create_infos(&qinfo).enabled_extension_names(&ext_ptrs).push_next(&mut features12);
        let device_raw = unsafe {
            instance
                .create_vulkan_device(system, get_ipa, physical_raw, &dev_info as *const _ as *const _)
                .map_err(|e| format!("xrCreateVulkanDeviceKHR: {e}"))?
                .map_err(|r| format!("xrCreateVulkanDeviceKHR: vk {r}"))?
        };
        let device = unsafe { ash::Device::load(vk_instance.fp_v1_0(), vk::Device::from_raw(device_raw as u64)) };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        // ---- session ----
        let (session, waiter, stream) = unsafe {
            match overlay {
                None => instance
                    .create_session::<xr::Vulkan>(system, &xr::vulkan::SessionCreateInfo { instance: vk_instance_raw, physical_device: physical_raw, device: device_raw, queue_family_index: queue_family, queue_index: 0 })
                    .map_err(|e| format!("xrCreateSession: {e}"))?,
                Some(placement) => {
                    // openxrs has no builder for the overlay struct: chain it by hand under the Vulkan binding
                    let overlay_info = xr::sys::SessionCreateInfoOverlayEXTX { ty: xr::sys::SessionCreateInfoOverlayEXTX::TYPE, next: std::ptr::null(), create_flags: Default::default(), session_layers_placement: placement };
                    let binding = xr::sys::GraphicsBindingVulkanKHR { ty: xr::sys::GraphicsBindingVulkanKHR::TYPE, next: &overlay_info as *const _ as *const c_void, instance: vk_instance_raw, physical_device: physical_raw, device: device_raw, queue_family_index: queue_family, queue_index: 0 };
                    let info = xr::sys::SessionCreateInfo { ty: xr::sys::SessionCreateInfo::TYPE, next: &binding as *const _ as *const c_void, create_flags: Default::default(), system_id: system };
                    let mut handle = <xr::sys::Session as xr::sys::Handle>::NULL;
                    let r = (instance.fp().create_session)(instance.as_raw(), &info, &mut handle);
                    if r.into_raw() < 0 {
                        return Err(format!("xrCreateSession(overlay): {r}"));
                    }
                    tracing::info!(placement, "XR_EXTX_overlay session");
                    xr::Session::<xr::Vulkan>::from_raw(instance.clone(), handle, Box::new(()))
                }
            }
        };
        // ---- the action set (spatial-input §1a; input/actions.rs) — attached once, here ----
        // The extension being advertised is not the system having the tracker: Monado offers
        // `EXT_eye_gaze_interaction` on every system and answers with the system property
        // (`ext_eye_gaze_interaction.adoc` `supportsEyeGazeInteraction`; the hand-tracking twin).
        // Without the property there is no gaze space, no per-tick gaze sample, no hand tracker.
        let (eyes, hands_tracked) = system_supports(&instance, system, exts.ext_eye_gaze_interaction, exts.ext_hand_tracking);
        let enabled = actions::Enabled { hand_interaction: exts.ext_hand_interaction, eye_gaze: eyes, hand_tracking: hands_tracked, mndx_system_buttons };
        let actions = match Actions::create(&instance, &session, &enabled) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!("action set: {e}; XR intake is the head ray only");
                None
            }
        };

        let views = instance.enumerate_view_configuration_views(system, VIEW_TYPE).map_err(|e| e.to_string())?;
        let blend = instance.enumerate_environment_blend_modes(system, VIEW_TYPE).map_err(|e| e.to_string())?[0];
        let max_layer_count = instance.system_properties(system).map(|p| p.graphics_properties.max_layer_count).unwrap_or(16);
        let exts_enabled_color_scale_bias = instance.exts().khr_composition_layer_color_scale_bias.is_some();
        tracing::info!(max_layer_count, "runtime layer cap");
        let formats = session.enumerate_swapchain_formats().map_err(|e| e.to_string())?;
        let want = [vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB, vk::Format::B8G8R8A8_UNORM, vk::Format::R8G8B8A8_UNORM];
        let color_format = want.iter().copied().find(|f| formats.contains(&(f.as_raw() as u32))).ok_or("no usable swapchain format")?;
        let mut swapchains = Vec::new();
        for v in &views {
            let handle = session
                .create_swapchain(&xr::SwapchainCreateInfo {
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::SAMPLED,
                    format: color_format.as_raw() as u32,
                    sample_count: 1,
                    width: v.recommended_image_rect_width,
                    height: v.recommended_image_rect_height,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                })
                .map_err(|e| format!("xrCreateSwapchain: {e}"))?;
            let images = handle.enumerate_images().map_err(|e| e.to_string())?.into_iter().map(|i| vk::Image::from_raw(i)).collect();
            swapchains.push(Swapchain { handle, images, extent: vk::Extent2D { width: v.recommended_image_rect_width, height: v.recommended_image_rect_height } });
        }
        let space = session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY).map_err(|e| e.to_string())?;
        tracing::info!(views = views.len(), w = views[0].recommended_image_rect_width, h = views[0].recommended_image_rect_height, format = ?color_format, "session created");

        // ---- the wait thread ----
        let handshake = Arc::new((Mutex::new(Handshake::default()), Condvar::new()));
        let (tx, ticks) = cchannel::channel();
        {
            let handshake = handshake.clone();
            let mut waiter = waiter;
            std::thread::Builder::new()
                .name("zxr-xrwait".into())
                .spawn(move || {
                    let mut frame_id: u64 = 0;
                    loop {
                        // wait until the session runs and frame `frame_id` (the previous one) has
                        // been begun by the loop
                        {
                            let (m, cv) = &*handshake;
                            let mut g = m.lock().unwrap();
                            while !g.running || g.begun < frame_id {
                                g = cv.wait(g).unwrap();
                            }
                        }
                        let wait_t0 = monotonic_ns();
                        let st = match waiter.wait() {
                            Ok(s) => s,
                            Err(xr::sys::Result::ERROR_SESSION_NOT_RUNNING) => continue,
                            Err(e) => {
                                tracing::error!("xrWaitFrame: {e}");
                                return;
                            }
                        };
                        frame_id += 1;
                        let tick = FrameTick { frame_id, predicted_display_time: st.predicted_display_time, predicted_display_period: st.predicted_display_period, should_render: st.should_render, woke_at_ns: monotonic_ns(), wait_ns: monotonic_ns().saturating_sub(wait_t0) };
                        if tx.send(tick).is_err() {
                            return;
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }

        Ok((
            XrCore { instance, system, session, stream, space, views, swapchains, color_format, environment_blend: blend, max_layer_count, session_running: false, exit_requested: false, presence_event: None, color_scale_bias: exts_enabled_color_scale_bias, calls: Default::default(), actions, head: xr::Posef::IDENTITY, events: xr::EventDataBuffer::new(), handshake, ticks: Some(ticks) },
            VkCore { entry: vk_entry, instance: vk_instance, physical, device, queue_family, queue },
        ))
    }

    /// The calloop source carrying frame ticks; the state loop inserts it once.
    pub fn take_ticks(&mut self) -> Option<cchannel::Channel<FrameTick>> {
        self.ticks.take()
    }

    fn set_running(&mut self, running: bool) {
        self.session_running = running;
        let (m, cv) = &*self.handshake;
        m.lock().unwrap().running = running;
        cv.notify_one();
    }

    /// Drain runtime events; begin/end the session on state changes.
    pub fn poll_events(&mut self) -> Result<(), String> {
        loop {
            let ev = timed(&mut self.calls.poll_event, || self.instance.poll_event(&mut self.events)).map_err(|e| e.to_string())?;
            let Some(ev) = ev else { break };
            if let xr::Event::SessionStateChanged(e) = ev {
                tracing::info!(state = ?e.state(), "session state");
                match e.state() {
                    xr::SessionState::READY => {
                        self.session.begin(VIEW_TYPE).map_err(|e| e.to_string())?;
                        self.set_running(true);
                    }
                    xr::SessionState::STOPPING => {
                        self.set_running(false);
                        self.session.end().map_err(|e| e.to_string())?;
                    }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                        self.exit_requested = true;
                    }
                    _ => {}
                }
            } else if let xr::Event::InstanceLossPending(_) = ev {
                self.exit_requested = true;
            } else if let xr::Event::UserPresenceChangedEXT(e) = ev {
                // headset on/off (spatial-input §1a; ADR 0007 doff → blank + grace): an event,
                // not a source — `input::tick` hands it to the mode stage
                tracing::info!(present = e.is_user_present(), "user presence");
                self.presence_event = Some(e.is_user_present());
            } else if let xr::Event::InteractionProfileChanged(_) = ev {
                // the runtime rebound (input.adoc:478-491, 499-505): re-read each hand's profile
                // at the next sync, and only then — the query is not per tick
                if let Some(a) = &mut self.actions {
                    a.profile_dirty = true;
                }
            }
        }
        Ok(())
    }

    /// The XR side of intake (spatial-input §1a): one `xrSyncActions`, then one `Sample` per
    /// source kind that has state this tick — controllers and hands from the action states and
    /// the pose cache `locate_spaces` filled, the joint bridge where the runtime lacks the
    /// hand-interaction device (§10), gaze when the extension is enabled. The flow is documented
    /// in `input/actions.rs`.
    pub fn sync_samples(&mut self, time: xr::Time, now_ns: u64, out: &mut Vec<Sample>) {
        let Some(a) = &mut self.actions else { return };
        let r = timed(&mut a.sync_actions_lat, || self.session.sync_actions(&[xr::ActiveActionSet::new(&a.set)]));
        if let Err(e) = r {
            if !a.sync_failed {
                tracing::warn!("xrSyncActions: {e}");
                a.sync_failed = true;
            }
            return;
        }
        a.sync_failed = false;
        if a.profile_dirty {
            let before = [a.hands[0].kind, a.hands[1].kind];
            a.refresh_profiles(&self.session);
            // a kind that went away (controller unbound) says so once, as the injector's `off`
            // does: a pose-less, untracked sample of the old kind, so no stage waits on a timeout
            for (i, old) in before.into_iter().enumerate() {
                if let Some(k) = old {
                    if a.hands[i].kind != Some(k) {
                        let mut s = Sample::new(k, now_ns);
                        s.xr_time = Some(time);
                        out.push(s);
                    }
                }
            }
        }
        for side in [Side::Left, Side::Right] {
            match a.hands[side_index(side)].kind {
                Some(SourceKind::Controller(_)) => a.controller_samples(&self.session, side, time, now_ns, out),
                Some(SourceKind::Hand(_)) => a.hand_samples(&self.session, side, time, now_ns, out),
                _ => {}
            }
            // the bridge (§10): only while this hand is *not* bound to `hand_interaction_ext`
            // and the runtime tracks joints — one `xrLocateHandJointsEXT` RPC per hand per tick
            if !matches!(a.hands[side_index(side)].kind, Some(SourceKind::Hand(_))) {
                a.bridge_samples(&self.space, self.head, side, time, now_ns, out);
            }
        }
        if a.eye_gaze {
            a.gaze_sample(&self.session, time, now_ns, out);
        }
        // the census (spec §11): the action set's calls beside the frame loop's
        self.calls.sync_actions = a.sync_actions_lat.clone();
        self.calls.get_action_state = a.get_action_state_lat.clone();
        self.calls.hand_joints = a.hand_joints_lat.clone();
    }

    /// The action spaces, once, for `actions::register_frames` to move into the scene as
    /// `Space::Xr` frames. Empty after the first call.
    pub fn take_action_spaces(&mut self) -> Vec<(PoseTag, xr::Space)> {
        self.actions.as_mut().map(|a| std::mem::take(&mut a.spaces)).unwrap_or_default()
    }

    /// Orderly session exit: `xrRequestExitSession`, then drive the state machine until the
    /// runtime has taken the session out of the running state (STOPPING → `xrEndSession`), so
    /// the wait thread is parked on the handshake, not inside the runtime, when handles drop.
    pub fn shutdown(&mut self) {
        if !self.session_running {
            return;
        }
        if let Err(e) = self.session.request_exit() {
            tracing::warn!("xrRequestExitSession: {e}");
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while self.session_running && std::time::Instant::now() < deadline {
            if self.poll_events().is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if self.session_running {
            tracing::warn!("session still running after exit request; tearing down anyway");
        }
        if let Some(a) = &self.actions {
            // the census lines `journal::Calls` will carry once it grows the fields (lane report)
            tracing::info!(sync_actions_n = a.sync_actions_lat.n, sync_actions_us_mean = if a.sync_actions_lat.n > 0 { a.sync_actions_lat.sum_ns / a.sync_actions_lat.n / 1000 } else { 0 }, sync_actions_us_max = a.sync_actions_lat.max_ns / 1000, get_action_state_n = a.get_action_state_lat.n, hand_joints_n = a.hand_joints_lat.n, "action set calls");
        }
    }

    /// `xrBeginFrame`, then release the wait thread for the next `xrWaitFrame`.
    pub fn begin_frame(&mut self, tick: &FrameTick) -> Result<(), String> {
        timed(&mut self.calls.begin_frame, || self.stream.begin()).map_err(|e| format!("xrBeginFrame: {e}"))?;
        let (m, cv) = &*self.handshake;
        m.lock().unwrap().begun = tick.frame_id;
        cv.notify_one();
        Ok(())
    }

    pub fn locate_views(&mut self, time: xr::Time) -> Result<Vec<xr::View>, String> {
        let (_flags, views) = timed(&mut self.calls.locate_views, || self.session.locate_views(VIEW_TYPE, time, &self.space)).map_err(|e| format!("xrLocateViews: {e}"))?;
        // the head this tick for the joint bridge (§10: palm-toward-head, the shoulder pivot):
        // the same midpoint the loop gives the scene's head frame
        if let (Some(a), Some(b)) = (views.first(), views.get(1).or(views.first())) {
            self.head = xr::Posef { orientation: a.pose.orientation, position: xr::Vector3f { x: (a.pose.position.x + b.pose.position.x) * 0.5, y: (a.pose.position.y + b.pose.position.y) * 0.5, z: (a.pose.position.z + b.pose.position.z) * 0.5 } };
        }
        Ok(views)
    }

    /// Locate every space in one round trip (`xrLocateSpacesKHR`; Monado batches the array into
    /// one IPC exchange, `ipc_client_space_overseer.c:161-195`). Called only when the scene has a
    /// frame the views do not give (spec §5a). Returns `(pose, valid)` per space, in order;
    /// `valid` = position and orientation both valid.
    pub fn locate_spaces(&mut self, spaces: &[&xr::Space], time: xr::Time, out: &mut Vec<(xr::Posef, bool)>) -> Result<(), String> {
        out.clear();
        if spaces.is_empty() {
            return Ok(());
        }
        let fp = self.instance.exts().khr_locate_spaces.as_ref().ok_or("runtime lacks XR_KHR_locate_spaces")?;
        let raw: Vec<xr::sys::Space> = spaces.iter().map(|s| s.as_raw()).collect();
        let mut data = vec![xr::sys::SpaceLocationData::default(); raw.len()];
        let info = xr::sys::SpacesLocateInfo { ty: xr::sys::SpacesLocateInfo::TYPE, next: std::ptr::null(), base_space: self.space.as_raw(), time, space_count: raw.len() as u32, spaces: raw.as_ptr() };
        let mut locations = xr::sys::SpaceLocations { ty: xr::sys::SpaceLocations::TYPE, next: std::ptr::null_mut(), location_count: data.len() as u32, locations: data.as_mut_ptr() };
        let session = self.session.as_raw();
        // SAFETY: the arrays outlive the call; the runtime writes `location_count` entries.
        let r = timed(&mut self.calls.locate_spaces, || unsafe { (fp.locate_spaces)(session, &info, &mut locations) });
        if r.into_raw() < 0 {
            return Err(format!("xrLocateSpacesKHR: {r:?}"));
        }
        let both = xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
        out.extend(data.iter().map(|d| (d.pose, d.location_flags.contains(both))));
        // action spaces among the located frames: cache pose *and* flags — the scene frame keeps
        // only `valid`, and gaze quality needs the TRACKED bits (`ext_eye_gaze_interaction.adoc:140-158`)
        if let Some(a) = &mut self.actions {
            for (i, h) in raw.iter().enumerate() {
                if let Some((tag, _)) = a.handles.iter().find(|(_, ah)| ah == h) {
                    a.poses[tag.index()] = (data[i].pose, data[i].location_flags);
                }
            }
        }
        Ok(())
    }

    /// Acquire + wait one image per view; returns the image indices.
    pub fn acquire_images(&mut self) -> Result<Vec<u32>, String> {
        let mut out = Vec::with_capacity(self.swapchains.len());
        let calls = &mut self.calls;
        for sc in &mut self.swapchains {
            let idx = timed(&mut calls.acquire_image, || sc.handle.acquire_image()).map_err(|e| e.to_string())?;
            timed(&mut calls.wait_image, || sc.handle.wait_image(xr::Duration::from_nanos(100_000_000))).map_err(|e| e.to_string())?;
            out.push(idx);
        }
        Ok(out)
    }

    pub fn release_images(&mut self) -> Result<(), String> {
        let calls = &mut self.calls;
        for sc in &mut self.swapchains {
            timed(&mut calls.release_image, || sc.handle.release_image()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// A runtime-owned swapchain for one panel (research/63 Phase 1b, `--panels=quad`): the
    /// client's buffer is blitted into it and the runtime composites it as a quad layer.
    pub fn create_panel_swapchain(&self, width: u32, height: u32) -> Result<Swapchain, String> {
        let handle = self
            .session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::SAMPLED | xr::SwapchainUsageFlags::TRANSFER_DST,
                format: self.color_format.as_raw() as u32,
                sample_count: 1,
                width,
                height,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            })
            .map_err(|e| format!("xrCreateSwapchain(panel): {e}"))?;
        let images = handle.enumerate_images().map_err(|e| e.to_string())?.into_iter().map(vk::Image::from_raw).collect();
        Ok(Swapchain { handle, images, extent: vk::Extent2D { width, height } })
    }

    /// Acquire (+ wait: Monado's Vulkan path folds the wait into acquire, `oxr_swapchain_vk.c:22-57`)
    /// one image of a panel swapchain; returns its index.
    pub fn acquire_panel_image(&mut self, sc: &mut Swapchain) -> Result<u32, String> {
        let calls = &mut self.calls;
        let idx = timed(&mut calls.acquire_image, || sc.handle.acquire_image()).map_err(|e| e.to_string())?;
        timed(&mut calls.wait_image, || sc.handle.wait_image(xr::Duration::from_nanos(100_000_000))).map_err(|e| e.to_string())?;
        Ok(idx)
    }

    /// Release a panel image after its pass has been queued.
    pub fn release_panel_image(&mut self, sc: &mut Swapchain) -> Result<(), String> {
        timed(&mut self.calls.release_image, || sc.handle.release_image()).map_err(|e| e.to_string())
    }

    /// `xrEndFrame` with an optional projection layer plus quad layers (spec §7 rev 3).
    /// Quad layers are submitted in the given order (painter's order, `rendering.adoc:1143-1147`).
    pub fn end_frame_with_quads(&mut self, time: xr::Time, views: Option<&[xr::View]>, quads: &[QuadLayer<'_>], emphasis_strength: f32) -> Result<(), String> {
        let mut pv: Vec<xr::CompositionLayerProjectionView<xr::Vulkan>> = Vec::new();
        if let Some(views) = views {
            for (i, v) in views.iter().enumerate() {
                let sc = &self.swapchains[i];
                pv.push(
                    xr::CompositionLayerProjectionView::new()
                        .pose(v.pose)
                        .fov(v.fov)
                        .sub_image(xr::SwapchainSubImage::new().swapchain(&sc.handle).image_array_index(0).image_rect(xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: sc.extent.width as i32, height: sc.extent.height as i32 } })),
                );
            }
        }
        let projection = views.map(|_| xr::CompositionLayerProjection::new().space(&self.space).views(&pv));
        // one colour scale/bias struct per quad, chained on `next` for the emphasised ones only
        // (spatial-input §4; scale 1 + strength·e, `input.emphasis.strength`, default 0.15). `biases` outlives `quad_layers`
        // and the `end` call below, so the raw pointer stays valid.
        let biases: Vec<xr::sys::CompositionLayerColorScaleBiasKHR> = quads
            .iter()
            .map(|q| {
                let s = 1.0 + emphasis_strength.max(0.0) * q.emphasis.clamp(0.0, 1.0);
                xr::sys::CompositionLayerColorScaleBiasKHR { ty: xr::sys::CompositionLayerColorScaleBiasKHR::TYPE, next: std::ptr::null(), color_scale: xr::Color4f { r: s, g: s, b: s, a: 1.0 }, color_bias: xr::Color4f { r: 0.0, g: 0.0, b: 0.0, a: 0.0 } }
            })
            .collect();
        let quad_layers: Vec<xr::CompositionLayerQuad<xr::Vulkan>> = quads
            .iter()
            .zip(biases.iter())
            .map(|(q, b)| {
                let layer = xr::CompositionLayerQuad::new()
                    .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                    .space(&self.space)
                    .eye_visibility(xr::EyeVisibility::BOTH)
                    .sub_image(xr::SwapchainSubImage::new().swapchain(&q.swapchain.handle).image_array_index(0).image_rect(xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: q.image_extent[0].min(q.swapchain.extent.width) as i32, height: q.image_extent[1].min(q.swapchain.extent.height) as i32 } }))
                    .pose(q.pose)
                    .size(xr::Extent2Df { width: q.size[0], height: q.size[1] });
                if self.color_scale_bias && q.emphasis > 0.0 {
                    let mut raw = layer.into_raw();
                    raw.next = (b as *const xr::sys::CompositionLayerColorScaleBiasKHR).cast();
                    // SAFETY: `biases` outlives this vector and the `end` call; the struct is a valid chain element.
                    unsafe { xr::CompositionLayerQuad::from_raw(raw) }
                } else {
                    layer
                }
            })
            .collect();
        let mut layers: Vec<&xr::CompositionLayerBase<xr::Vulkan>> = Vec::with_capacity(1 + quad_layers.len());
        if let Some(p) = &projection {
            layers.push(p);
        }
        for q in &quad_layers {
            layers.push(q);
        }
        let blend = self.environment_blend;
        timed(&mut self.calls.end_frame, || self.stream.end(time, blend, &layers)).map_err(|e| format!("xrEndFrame: {e}"))
    }

    /// `xrEndFrame` with one projection layer (the model), or an empty frame.
    pub fn end_frame(&mut self, time: xr::Time, views: Option<&[xr::View]>) -> Result<(), String> {
        match views {
            Some(views) => {
                let pv: Vec<xr::CompositionLayerProjectionView<xr::Vulkan>> = views
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let sc = &self.swapchains[i];
                        xr::CompositionLayerProjectionView::new()
                            .pose(v.pose)
                            .fov(v.fov)
                            .sub_image(xr::SwapchainSubImage::new().swapchain(&sc.handle).image_array_index(0).image_rect(xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: sc.extent.width as i32, height: sc.extent.height as i32 } }))
                    })
                    .collect();
                let layer = xr::CompositionLayerProjection::new().space(&self.space).views(&pv);
                let blend = self.environment_blend;
                timed(&mut self.calls.end_frame, || self.stream.end(time, blend, &[&layer])).map_err(|e| format!("xrEndFrame: {e}"))
            }
            None => {
                let blend = self.environment_blend;
                timed(&mut self.calls.end_frame, || self.stream.end(time, blend, &[])).map_err(|e| format!("xrEndFrame: {e}"))
            }
        }
    }
}

impl Actions {
    /// Create the set, the actions, the suggested bindings for every profile the runtime can
    /// take, the action spaces, and attach — once, before the session runs.
    fn create(instance: &xr::Instance, session: &xr::Session<xr::Vulkan>, enabled: &actions::Enabled) -> Result<Actions, String> {
        let p = |s: &str| instance.string_to_path(s).map_err(|e| format!("xrStringToPath {s}: {e}"));
        let hand_paths = [p("/user/hand/left")?, p("/user/hand/right")?];
        let set = instance.create_action_set(actions::SET_NAME, "Mura", 0).map_err(|e| format!("xrCreateActionSet: {e}"))?;
        let hands: &[xr::Path] = &hand_paths;
        let none: &[xr::Path] = &[];
        macro_rules! act {
            ($t:ty, $a:expr, $sub:expr) => {
                set.create_action::<$t>(actions::name(*$a), actions::localized(*$a), $sub).map_err(|e| format!("xrCreateAction {}: {e}", actions::name(*$a)))?
            };
        }
        let aim_pose = act!(xr::Posef, &Act::AimPose, hands);
        let grip_pose = act!(xr::Posef, &Act::GripPose, hands);
        let poke_pose = act!(xr::Posef, &Act::PokePose, hands);
        let gaze_pose = act!(xr::Posef, &Act::GazePose, none);
        let select = act!(bool, &Act::Select, hands);
        let menu = act!(bool, &Act::Menu, hands);
        let system = act!(bool, &Act::System, hands);
        let secondary = act!(bool, &Act::Secondary, hands);
        let ready = act!(bool, &Act::Ready, hands);
        let stick = act!(xr::Vector2f, &Act::Stick, hands);
        let pinch = act!(f32, &Act::Pinch, hands);
        let aim_activate = act!(f32, &Act::AimActivate, hands);
        let grasp = act!(f32, &Act::Grasp, hands);

        // suggested bindings, one call per profile the runtime can accept: an unknown profile or
        // one whose extension is not enabled fails the whole call with PATH_UNSUPPORTED
        // (Monado: `oxr_api_action.c:278-297`), so profiles are filtered by `Enabled` first
        let mut profiles = Vec::with_capacity(actions::PROFILES.len());
        for prof in actions::PROFILES {
            let ok = match prof.ext {
                Ext::Core => true,
                Ext::HandInteraction => enabled.hand_interaction,
                Ext::EyeGaze => enabled.eye_gaze,
                Ext::MndxSystemButtons => enabled.mndx_system_buttons,
            };
            if !ok {
                continue;
            }
            let profile = p(prof.path)?;
            let mut paths: Vec<(Act, xr::Path)> = Vec::with_capacity(prof.binds.len() * 2);
            for b in prof.binds {
                let users: &[&str] = match b.on {
                    On::Both => &["/user/hand/left", "/user/hand/right"],
                    On::Left => &["/user/hand/left"],
                    On::Right => &["/user/hand/right"],
                    On::Eyes => &["/user/eyes_ext"],
                };
                for u in users {
                    paths.push((b.act, p(&format!("{u}/{}", b.sub))?));
                }
            }
            let bindings: Vec<xr::Binding<'_>> = paths
                .iter()
                .map(|(act, path)| match act {
                    Act::AimPose => xr::Binding::new(&aim_pose, *path),
                    Act::GripPose => xr::Binding::new(&grip_pose, *path),
                    Act::PokePose => xr::Binding::new(&poke_pose, *path),
                    Act::GazePose => xr::Binding::new(&gaze_pose, *path),
                    Act::Select => xr::Binding::new(&select, *path),
                    Act::Menu => xr::Binding::new(&menu, *path),
                    Act::System => xr::Binding::new(&system, *path),
                    Act::Secondary => xr::Binding::new(&secondary, *path),
                    Act::Ready => xr::Binding::new(&ready, *path),
                    Act::Stick => xr::Binding::new(&stick, *path),
                    Act::Pinch => xr::Binding::new(&pinch, *path),
                    Act::AimActivate => xr::Binding::new(&aim_activate, *path),
                    Act::Grasp => xr::Binding::new(&grasp, *path),
                })
                .collect();
            instance.suggest_interaction_profile_bindings(profile, &bindings).map_err(|e| format!("xrSuggestInteractionProfileBindings {}: {e}", prof.path))?;
            profiles.push((profile, prof.kind));
        }

        // action spaces (created before attach, as hello_xr does); the scene owns them after
        // `actions::register_frames`
        let mut spaces = Vec::with_capacity(PoseTag::ALL.len());
        for tag in PoseTag::ALL {
            let (action, sub) = match tag {
                PoseTag::Aim(s) => (&aim_pose, hand_paths[side_index(s)]),
                PoseTag::Grip(s) => (&grip_pose, hand_paths[side_index(s)]),
                PoseTag::Poke(s) => (&poke_pose, hand_paths[side_index(s)]),
                PoseTag::Gaze => (&gaze_pose, xr::Path::NULL),
            };
            if tag == PoseTag::Gaze && !enabled.eye_gaze {
                continue;
            }
            let space = action.create_space(session, sub, xr::Posef::IDENTITY).map_err(|e| format!("xrCreateActionSpace {tag:?}: {e}"))?;
            spaces.push((tag, space));
        }
        let handles: Vec<(PoseTag, xr::sys::Space)> = spaces.iter().map(|(t, s)| (*t, s.as_raw())).collect();
        session.attach_action_sets(&[&set]).map_err(|e| format!("xrAttachSessionActionSets: {e}"))?;

        // the bridge's trackers (§10): the extension may be enabled while the system has no hand
        // device — the runtime says so at creation, and the bridge is then simply absent
        let mut trackers = [None, None];
        if enabled.hand_tracking {
            for (i, hand) in [xr::Hand::LEFT, xr::Hand::RIGHT].into_iter().enumerate() {
                match session.create_hand_tracker(hand) {
                    Ok(t) => trackers[i] = Some(t),
                    Err(e) => tracing::info!(?hand, "no hand tracker ({e}); the joint bridge is off"),
                }
            }
        }
        tracing::info!(profiles = profiles.len(), spaces = spaces.len(), hand_trackers = trackers.iter().filter(|t| t.is_some()).count(), eye_gaze = enabled.eye_gaze, hand_interaction = enabled.hand_interaction, mndx_system_buttons = enabled.mndx_system_buttons, "action set attached");
        Ok(Actions {
            set,
            aim_pose,
            grip_pose,
            poke_pose,
            gaze_pose,
            select,
            menu,
            system,
            secondary,
            ready,
            stick,
            pinch,
            aim_activate,
            grasp,
            hand_paths,
            profiles,
            spaces,
            handles,
            poses: [(xr::Posef::IDENTITY, xr::SpaceLocationFlags::EMPTY); 7],
            trackers,
            hands: [HandState::default(), HandState::default()],
            profile_dirty: true,
            eye_gaze: enabled.eye_gaze,
            sync_failed: false,
            sync_actions_lat: Lat::default(),
            get_action_state_lat: Lat::default(),
            bridge_cfg: bridge::BridgeCfg::default(),
            hand_joints_lat: Lat::default(),
        })
    }

    /// `xrGetCurrentInteractionProfile` per hand → the source kind each hand is this session
    /// (`actions::kind_for_profile`). Client-side on Monado; called only when the event says so.
    fn refresh_profiles(&mut self, session: &xr::Session<xr::Vulkan>) {
        self.profile_dirty = false;
        for side in [Side::Left, Side::Right] {
            let i = side_index(side);
            let profile = session.current_interaction_profile(self.hand_paths[i]).unwrap_or(xr::Path::NULL);
            let kind = actions::kind_for_profile(profile, &self.profiles, side);
            if profile != self.hands[i].profile || kind != self.hands[i].kind {
                tracing::info!(?side, ?kind, "interaction profile");
                self.hands[i].profile = profile;
                self.hands[i].kind = kind;
                // a rebinding drops any half-pressed edge state
                self.hands[i].buttons = [false; actions::BUTTONS.len()];
                self.hands[i].stick_live = false;
            }
        }
    }

    fn cached(&self, tag: PoseTag) -> (xr::Posef, xr::SpaceLocationFlags) {
        self.poses[tag.index()]
    }

    /// A sample skeleton for one hand from the cached aim pose.
    fn base(&self, kind: SourceKind, side: Side, time: xr::Time, now_ns: u64) -> Sample {
        let (aim, flags) = self.cached(PoseTag::Aim(side));
        let mut s = Sample::new(kind, now_ns);
        s.xr_time = Some(time);
        s.pose = if flags.contains(actions::valid()) { Some(aim) } else { None };
        s.tracked = flags.contains(actions::tracked());
        s.quality = if s.tracked { Quality::Nominal } else { Quality::Lost };
        s
    }

    /// Controller intake (spatial-input §2 "controller", §8): the aim ray every tick; one sample
    /// per button edge; the stick as a continuous axis while live plus one zero when it stops.
    fn controller_samples(&mut self, session: &xr::Session<xr::Vulkan>, side: Side, time: xr::Time, now_ns: u64, out: &mut Vec<Sample>) {
        let i = side_index(side);
        let sub = self.hand_paths[i];
        let mut s = self.base(SourceKind::Controller(side), side, time, now_ns);
        let lat = &mut self.get_action_state_lat;
        // `ready` for a controller = the aim pose action is active (the device is bound and
        // this session is focused, input.adoc:839-843)
        s.ready = timed(lat, || self.aim_pose.is_active(session, sub)).unwrap_or(false);
        s.values.grasp = timed(lat, || self.grasp.state(session, sub)).map(|st| if st.is_active { st.current_state } else { 0.0 }).unwrap_or(0.0);
        let stick = timed(lat, || self.stick.state(session, sub)).ok().filter(|st| st.is_active).map(|st| st.current_state).unwrap_or(xr::Vector2f { x: 0.0, y: 0.0 });
        let live = stick.x != 0.0 || stick.y != 0.0;
        if live || self.hands[i].stick_live {
            s.axis = Some((stick.x as f64, stick.y as f64));
            s.axis_source = Some(AxisSource::Continuous);
        }
        self.hands[i].stick_live = live;
        out.push(s);
        // button edges: level state read every tick, a sample only on change
        let mut cur = [false; actions::BUTTONS.len()];
        for (bi, (act, _)) in actions::BUTTONS.iter().enumerate() {
            let action = match act {
                Act::Select => &self.select,
                Act::Menu => &self.menu,
                Act::System => &self.system,
                _ => &self.secondary,
            };
            cur[bi] = timed(lat, || action.state(session, sub)).map(|st| st.is_active && st.current_state).unwrap_or(false);
        }
        for (button, pressed) in actions::button_edges(&mut self.hands[i].buttons, cur).into_iter().flatten() {
            let mut e = s;
            e.axis = None;
            e.axis_source = None;
            e.button = Some((button, pressed));
            out.push(e);
        }
    }

    /// Hand intake through `hand_interaction_ext` (spatial-input §2 "hand"; adoc:284-302): aim +
    /// poke poses, the three values, `ready` = any of the profile's `ready_ext` gates (boolean
    /// OR across bindings, input.adoc:878-880).
    fn hand_samples(&mut self, session: &xr::Session<xr::Vulkan>, side: Side, time: xr::Time, now_ns: u64, out: &mut Vec<Sample>) {
        let i = side_index(side);
        let sub = self.hand_paths[i];
        let mut s = self.base(SourceKind::Hand(side), side, time, now_ns);
        let (poke, pflags) = self.cached(PoseTag::Poke(side));
        s.poke_pose = if pflags.contains(actions::valid()) { Some(poke) } else { None };
        let lat = &mut self.get_action_state_lat;
        s.ready = timed(lat, || self.ready.state(session, sub)).map(|st| st.is_active && st.current_state).unwrap_or(false);
        let f = |a: &xr::Action<f32>, lat: &mut Lat| timed(lat, || a.state(session, sub)).map(|st| if st.is_active { st.current_state } else { 0.0 }).unwrap_or(0.0);
        s.values.pinch = f(&self.pinch, lat);
        s.values.aim_activate = f(&self.aim_activate, lat);
        s.values.grasp = f(&self.grasp, lat);
        if side == self.bridge_cfg.dominant {
            s.flags.insert(Flags::DOMINANT);
        }
        out.push(s);
    }

    /// The joint bridge (spatial-input §10): `xrLocateHandJointsEXT`, then `bridge::derive`
    /// behind the same `Sample` shape, `Flags::BRIDGED`. Nothing when the hand is not tracked.
    fn bridge_samples(&mut self, base_space: &xr::Space, head: xr::Posef, side: Side, time: xr::Time, now_ns: u64, out: &mut Vec<Sample>) {
        let i = side_index(side);
        let Some(tracker) = &self.trackers[i] else { return };
        let mut poses = [xr::Posef::IDENTITY; xr::HAND_JOINT_COUNT];
        let mut tracked = false;
        match timed(&mut self.hand_joints_lat, || base_space.locate_hand_joints(tracker, time)) {
            Ok(Some(joints)) => {
                tracked = true;
                for (k, j) in joints.iter().enumerate() {
                    poses[k] = j.pose;
                    if bridge::REQUIRED_JOINTS.contains(&k) && !j.location_flags.contains(actions::tracked()) {
                        tracked = false;
                    }
                }
                self.hands[i].bridge_live = true;
            }
            // not tracked this tick: one closing sample if the hand was live, else nothing
            _ => {
                if !self.hands[i].bridge_live {
                    return;
                }
                self.hands[i].bridge_live = false;
            }
        }
        let d = bridge::derive_with(&self.bridge_cfg, side, &poses, tracked, head, now_ns, &mut self.hands[i].bridge);
        let mut s = Sample::new(SourceKind::Hand(side), now_ns);
        s.xr_time = Some(time);
        out.push(bridge::fill(s, &d));
    }

    /// Gaze (spatial-input §2 "gaze"; §9): one sample per tick while the extension is enabled,
    /// quality from the located flags per `ext_eye_gaze_interaction.adoc:140-158`.
    fn gaze_sample(&mut self, session: &xr::Session<xr::Vulkan>, time: xr::Time, now_ns: u64, out: &mut Vec<Sample>) {
        let (pose, flags) = self.cached(PoseTag::Gaze);
        let active = timed(&mut self.get_action_state_lat, || self.gaze_pose.is_active(session, xr::Path::NULL)).unwrap_or(false);
        let mut s = Sample::new(SourceKind::Gaze, now_ns);
        s.xr_time = Some(time);
        s.quality = actions::gaze_quality(flags, active);
        s.tracked = s.quality == Quality::Nominal;
        s.ready = s.quality == Quality::Nominal;
        s.pose = if active && flags.contains(actions::valid()) { Some(pose) } else { None };
        out.push(s);
    }
}

/// Column-major 4×4 helpers for the renderer (view/projection from `xrLocateViews`).
pub mod math {
    use openxr as xr;

    pub type Mat4 = [f32; 16];

    pub fn identity() -> Mat4 {
        let mut m = [0.0; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m
    }

    pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
        let mut r = [0.0; 16];
        for c in 0..4 {
            for row in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += a[k * 4 + row] * b[c * 4 + k];
                }
                r[c * 4 + row] = s;
            }
        }
        r
    }

    /// OpenXR asymmetric fov → Vulkan projection (depth 0..1, y down handled by the viewport).
    pub fn projection(fov: xr::Fovf, near: f32, far: f32) -> Mat4 {
        let l = fov.angle_left.tan();
        let r = fov.angle_right.tan();
        let d = fov.angle_down.tan();
        let u = fov.angle_up.tan();
        let w = r - l;
        let h = u - d;
        let mut m = [0.0; 16];
        m[0] = 2.0 / w;
        m[5] = 2.0 / h;
        m[8] = (r + l) / w;
        m[9] = (u + d) / h;
        m[10] = -far / (far - near);
        m[11] = -1.0;
        m[14] = -(far * near) / (far - near);
        m
    }

    /// Inverse of a rigid pose (rotation quaternion + translation) as a view matrix.
    pub fn view(pose: xr::Posef) -> Mat4 {
        let q = pose.orientation;
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        // rotation matrix of q (column-major)
        let rot = [
            1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w), 0.0,
            2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w), 0.0,
            2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y), 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        // inverse: transpose rotation, negate translation
        let mut inv = [0.0; 16];
        for c in 0..3 {
            for r in 0..3 {
                inv[c * 4 + r] = rot[r * 4 + c];
            }
        }
        let t = pose.position;
        inv[12] = -(inv[0] * t.x + inv[4] * t.y + inv[8] * t.z);
        inv[13] = -(inv[1] * t.x + inv[5] * t.y + inv[9] * t.z);
        inv[14] = -(inv[2] * t.x + inv[6] * t.y + inv[10] * t.z);
        inv[15] = 1.0;
        inv
    }

    /// A plane's model matrix: translation + yaw (radians) about Y.
    pub fn model(pos: [f32; 3], yaw: f32) -> Mat4 {
        let (s, c) = yaw.sin_cos();
        let mut m = identity();
        m[0] = c;
        m[2] = -s;
        m[8] = s;
        m[10] = c;
        m[12] = pos[0];
        m[13] = pos[1];
        m[14] = pos[2];
        m
    }

    /// Orthographic pixels → Vulkan NDC for a panel pass: x ∈ [0,w] → [−1,1], y ∈ [0,h] → [−1,1]
    /// (framebuffer y is down, so no flip; textures are drawn with `flip_v`), z ∈ [−1,0] → [0,1].
    pub fn ortho_px(w: f32, h: f32) -> Mat4 {
        let mut m = identity();
        m[0] = 2.0 / w;
        m[5] = 2.0 / h;
        m[10] = 1.0;
        m[12] = -1.0;
        m[13] = -1.0;
        m[14] = 1.0;
        m
    }

    /// Vulkan's clip space has y down relative to OpenGL's: flip y.
    pub fn flip_y() -> Mat4 {
        let mut m = identity();
        m[5] = -1.0;
        m
    }

    #[allow(dead_code)]
    pub fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    }

    // ---- rigid poses (spec §5a: poses, not matrices, are the stored form) ----

    pub fn pose_identity() -> xr::Posef {
        xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } }
    }

    /// A pose from a position and a yaw (radians) about +Y — the fan's shape.
    pub fn pose_yaw(pos: [f32; 3], yaw: f32) -> xr::Posef {
        let (s, c) = (yaw * 0.5).sin_cos();
        xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: s, z: 0.0, w: c }, position: xr::Vector3f { x: pos[0], y: pos[1], z: pos[2] } }
    }

    /// Hamilton product `a ⊗ b` (apply `b`, then `a`).
    pub fn quat_mul(a: xr::Quaternionf, b: xr::Quaternionf) -> xr::Quaternionf {
        xr::Quaternionf {
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        }
    }

    pub fn quat_conj(q: xr::Quaternionf) -> xr::Quaternionf {
        xr::Quaternionf { x: -q.x, y: -q.y, z: -q.z, w: q.w }
    }

    /// Compose rigid poses: `a ∘ b` maps a point in `b`'s frame into `a`'s parent frame
    /// (`frame.pose ∘ place.local ∘ member.local` is the member's world pose).
    pub fn pose_mul(a: xr::Posef, b: xr::Posef) -> xr::Posef {
        let p = rotate(a.orientation, [b.position.x, b.position.y, b.position.z]);
        xr::Posef {
            orientation: quat_mul(a.orientation, b.orientation),
            position: xr::Vector3f { x: a.position.x + p[0], y: a.position.y + p[1], z: a.position.z + p[2] },
        }
    }

    /// The inverse rigid pose (a unit quaternion's inverse is its conjugate).
    pub fn pose_inverse(p: xr::Posef) -> xr::Posef {
        let q = quat_conj(p.orientation);
        let t = rotate(q, [-p.position.x, -p.position.y, -p.position.z]);
        xr::Posef { orientation: q, position: xr::Vector3f { x: t[0], y: t[1], z: t[2] } }
    }

    /// Map a point through a pose (rotate, then translate).
    pub fn pose_apply(p: xr::Posef, v: [f32; 3]) -> [f32; 3] {
        let r = rotate(p.orientation, v);
        [r[0] + p.position.x, r[1] + p.position.y, r[2] + p.position.z]
    }

    /// A pose as a column-major model matrix — built only where a pass needs one.
    pub fn pose_to_mat(p: xr::Posef) -> Mat4 {
        let q = p.orientation;
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        [
            1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w), 0.0,
            2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w), 0.0,
            2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y), 0.0,
            p.position.x, p.position.y, p.position.z, 1.0,
        ]
    }

    /// Rotate a direction by a quaternion.
    pub fn rotate(q: xr::Quaternionf, v: [f32; 3]) -> [f32; 3] {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let rot = [
            1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w),
            2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w),
            2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y),
        ];
        [
            rot[0] * v[0] + rot[3] * v[1] + rot[6] * v[2],
            rot[1] * v[0] + rot[4] * v[1] + rot[7] * v[2],
            rot[2] * v[0] + rot[5] * v[1] + rot[8] * v[2],
        ]
    }
}
