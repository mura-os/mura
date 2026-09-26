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
    pub session_running: bool,
    pub exit_requested: bool,
    events: xr::EventDataBuffer,
    /// The wait thread's handshake: it may call `xrWaitFrame` again only once the previous frame
    /// has been begun (rendering.adoc:792-794) and only while the session is running.
    handshake: Arc<(Mutex<Handshake>, Condvar)>,
    ticks: Option<cchannel::Channel<FrameTick>>,
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

fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: plain clock_gettime into a stack struct.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

impl XrCore {
    /// Instance → system → Vulkan instance/device from the runtime → session → swapchains →
    /// reference space, then the wait thread. `loader`: the OpenXR loader path (baked by Nix).
    pub fn new(loader: &str, app_name: &str) -> Result<(XrCore, VkCore), String> {
        // SAFETY: loading the OpenXR loader shared object by path.
        let entry = unsafe { xr::Entry::load_from(std::path::Path::new(loader), &()) }.map_err(|e| format!("openxr loader {loader}: {e}"))?;
        let available = entry.enumerate_extensions().map_err(|e| e.to_string())?;
        if !available.khr_vulkan_enable2 {
            return Err("runtime lacks XR_KHR_vulkan_enable2".into());
        }
        let mut exts = xr::ExtensionSet::default();
        exts.khr_vulkan_enable2 = true;
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
        let mut timeline = vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
        let mut features12 = vk::PhysicalDeviceVulkan12Features::default().timeline_semaphore(true);
        let dev_info = vk::DeviceCreateInfo::default().queue_create_infos(&qinfo).enabled_extension_names(&ext_ptrs).push_next(&mut features12).push_next(&mut timeline);
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
            instance
                .create_session::<xr::Vulkan>(system, &xr::vulkan::SessionCreateInfo { instance: vk_instance_raw, physical_device: physical_raw, device: device_raw, queue_family_index: queue_family, queue_index: 0 })
                .map_err(|e| format!("xrCreateSession: {e}"))?
        };
        let views = instance.enumerate_view_configuration_views(system, VIEW_TYPE).map_err(|e| e.to_string())?;
        let blend = instance.enumerate_environment_blend_modes(system, VIEW_TYPE).map_err(|e| e.to_string())?[0];
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
                        let st = match waiter.wait() {
                            Ok(s) => s,
                            Err(xr::sys::Result::ERROR_SESSION_NOT_RUNNING) => continue,
                            Err(e) => {
                                tracing::error!("xrWaitFrame: {e}");
                                return;
                            }
                        };
                        frame_id += 1;
                        let tick = FrameTick { frame_id, predicted_display_time: st.predicted_display_time, predicted_display_period: st.predicted_display_period, should_render: st.should_render, woke_at_ns: monotonic_ns() };
                        if tx.send(tick).is_err() {
                            return;
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }

        Ok((
            XrCore { instance, system, session, stream, space, views, swapchains, color_format, environment_blend: blend, session_running: false, exit_requested: false, events: xr::EventDataBuffer::new(), handshake, ticks: Some(ticks) },
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
        while let Some(ev) = self.instance.poll_event(&mut self.events).map_err(|e| e.to_string())? {
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
            }
        }
        Ok(())
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
    }

    /// `xrBeginFrame`, then release the wait thread for the next `xrWaitFrame`.
    pub fn begin_frame(&mut self, tick: &FrameTick) -> Result<(), String> {
        self.stream.begin().map_err(|e| format!("xrBeginFrame: {e}"))?;
        let (m, cv) = &*self.handshake;
        m.lock().unwrap().begun = tick.frame_id;
        cv.notify_one();
        Ok(())
    }

    pub fn locate_views(&self, time: xr::Time) -> Result<Vec<xr::View>, String> {
        let (_flags, views) = self.session.locate_views(VIEW_TYPE, time, &self.space).map_err(|e| format!("xrLocateViews: {e}"))?;
        Ok(views)
    }

    /// Acquire + wait one image per view; returns the image indices.
    pub fn acquire_images(&mut self) -> Result<Vec<u32>, String> {
        let mut out = Vec::with_capacity(self.swapchains.len());
        for sc in &mut self.swapchains {
            let idx = sc.handle.acquire_image().map_err(|e| e.to_string())?;
            sc.handle.wait_image(xr::Duration::from_nanos(100_000_000)).map_err(|e| e.to_string())?;
            out.push(idx);
        }
        Ok(out)
    }

    pub fn release_images(&mut self) -> Result<(), String> {
        for sc in &mut self.swapchains {
            sc.handle.release_image().map_err(|e| e.to_string())?;
        }
        Ok(())
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
                self.stream.end(time, self.environment_blend, &[&layer]).map_err(|e| format!("xrEndFrame: {e}"))
            }
            None => self.stream.end(time, self.environment_blend, &[]).map_err(|e| format!("xrEndFrame: {e}")),
        }
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
