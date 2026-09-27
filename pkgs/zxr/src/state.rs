//! The compositor state (specs/zxr-core.md §3 `frontend`, `scene`, `input`): smithay's
//! delegate states, the seat, the one `wl_output`, the texture cache, and the handlers that map
//! xdg-shell lifecycle onto planes. The frame procedure itself lives in `main.rs`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::sync::Arc;
use std::time::Instant;

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::{Buffer as _, Format, Fourcc, Modifier};
use smithay::backend::drm::DrmDeviceFd;
use smithay::backend::renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state, Buffer as HeldWlBuffer, CommitCounter};
use smithay::desktop::{find_popup_root_surface, get_popup_toplevel_coords, PopupKind, PopupManager, Window};
use smithay::input::dnd::{DnDGrab, DndGrabHandler, GrabType, Source};
use smithay::input::pointer::{CursorImageStatus, Focus};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, LoopSignal, Mode as CMode, PostAction};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::utils::{Rectangle, Serial, Size, Transform, SERIAL_COUNTER};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
};
use smithay::wayland::dmabuf::{get_dmabuf, DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::drm_syncobj::{supports_syncobj_eventfd, DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState};
use smithay::wayland::input_method::{InputMethodHandler, InputMethodManagerState, PopupSurface as ImPopupSurface};
use smithay::wayland::keyboard_shortcuts_inhibit::{KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor};
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::selection::data_device::{set_data_device_focus, DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState, XdgToplevelSurfaceData};
use smithay::wayland::shm::{with_buffer_contents, ShmHandler, ShmState};
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::text_input::TextInputManagerState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState;
use smithay::wayland::xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData};

use crate::input::{ei, focus, libinput, text};
use crate::journal::Journal;
use crate::render::{Renderer, Texture};
use crate::scene::{self, Flags, MemberId, Scene, Shape};
use crate::shell::Surface;
use crate::xr::math;
use crate::xr::XrCore;

/// One client's compositor bookkeeping and admission bits (`shell::filter`, spec §10 rev 3.12).
pub use crate::shell::filter::ClientState;

/// A surface's texture: shm textures belong to the surface and are re-uploaded per commit;
/// dmabuf textures belong to the `wl_buffer` (cached in `dmabuf_textures`) and are only
/// referenced here.
pub struct SurfaceTex {
    pub texture: Option<Texture>,
    /// dmabuf: the buffer whose cached texture is current
    pub dmabuf_buffer: Option<ObjectId>,
    pub commit: Option<CommitCounter>,
    pub last_used_frame: u64,
}

/// A client buffer held for a frame that sampled it (§6.5). Dropping the clone is what
/// lets smithay send `wl_buffer.release` / signal the release point.
pub struct HeldBuffer {
    pub buffer: HeldWlBuffer,
    pub since_frame: u64,
}

pub struct Zxr {
    pub dh: DisplayHandle,
    pub loop_handle: LoopHandle<'static, Zxr>,
    pub loop_signal: LoopSignal,
    pub socket_name: OsString,
    pub start: Instant,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub _output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Zxr>,
    pub data_device_state: DataDeviceState,
    pub dmabuf_state: DmabufState,
    pub _dmabuf_global: DmabufGlobal,
    pub syncobj_state: Option<DrmSyncobjState>,
    pub _viewporter: ViewporterState,
    pub _presentation: PresentationState,
    pub _single_pixel: SinglePixelBufferState,
    /// cursor-shape-v1 (spatial-input §7: names rendered from one theme at one scale)
    pub _cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState,
    /// ADR 0007: `ext-idle-notify-v1` + `zwp_idle_inhibit_v1`
    pub idle_notifier: smithay::wayland::idle_notify::IdleNotifierState<Zxr>,
    pub _idle_inhibit: smithay::wayland::idle_inhibit::IdleInhibitManagerState,
    pub idle_inhibitors: Vec<WlSurface>,
    /// the one cursor panel (spatial-input §7; research/70 §9; input/cursor.rs): a fixed
    /// `CURSOR_PX`² swapchain (grown only around a larger client image, never shrunk), drawn into
    /// only when the layer's content changes — the ring once, a `set_cursor` surface on its
    /// commits, a `cursor-shape-v1` name when the name changes — and positioned per tick as the
    /// one band-5 cursor quad: the cursor-plane shape (one fixed-size plane, one image) every
    /// desktop compositor prefers over compositing the cursor into the window
    pub cursor_panel: Option<PanelSwapchain>,
    /// the grab bar's one fixed swapchain (wm §4a; drawn once, shown under the hovered or
    /// grabbed plane — the cursor's one-layer shape)
    pub bar_panel: Option<PanelSwapchain>,
    /// what the cursor panel currently holds; a different key redraws it
    pub cursor_key: Option<CursorKey>,
    /// the ring texture (16 KiB; lives for the session)
    pub ring_tex: Option<crate::render::Texture>,
    /// the bar's strip texture
    pub bar_tex: Option<crate::render::Texture>,
    /// the cursor theme for `cursor-shape-v1` names, and the texture of the current name
    pub cursor_theme: crate::input::theme::Theme,
    pub cursor_named: Option<(smithay::input::pointer::CursorIcon, crate::render::Texture)>,
    /// spatial-input §6: `xdg_activation_v1` — tokens carry the commit's serial (`focus.rs`)
    pub activation_state: XdgActivationState,
    /// spatial-input §12: the text-entry seam — `text-input-v3`, `input-method-v2`,
    /// `virtual-keyboard-v1`; smithay routes focus and activate/deactivate (`text.rs`)
    pub _text_input_state: TextInputManagerState,
    pub _input_method_state: InputMethodManagerState,
    pub _virtual_keyboard_state: VirtualKeyboardManagerState,
    /// spatial-input §8: `keyboard-shortcuts-inhibit`, recorded per surface in `text`
    pub shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    // ---- the shell-layer half (spec §4/§9/§10 rev 3.12; shell/)
    pub layer_shell_state: smithay::wayland::shell::wlr_layer::WlrLayerShellState,
    pub _layer_anchoring_global: smithay::reexports::wayland_server::backend::GlobalId,
    pub shell_anchoring_managers: Vec<crate::shell::anchoring::zxr_layer_anchoring_v1::ZxrLayerAnchoringV1>,
    pub session_lock_state: smithay::wayland::session_lock::SessionLockManagerState,
    pub _security_context_state: smithay::wayland::security_context::SecurityContextState,
    pub shell: crate::shell::Shell,
    /// the trusted connections' data, read each tick for `disconnected`
    pub trusted_clients: Vec<Arc<ClientState>>,
    pub popups: PopupManager,
    pub seat: Seat<Zxr>,
    pub output: Output,
    /// spatial-input §6: the focus stack, the last commit serial, activation counters
    pub focus: focus::Focus,
    /// spatial-input §12: OSK suppression, shortcuts inhibitors
    pub text: text::Text,
    /// spatial-input §8: libinput intake state and the `hmdButtons` roles
    pub peripherals: libinput::Peripherals,
    /// spatial-input §1a: the EIS server
    pub ei: ei::EiServer,

    /// spec §5a: the three arenas; the member payload is `Payload` (this module's)
    pub scene: Scene<Payload>,
    /// panel swapchains taken from removed members, destroyed once both slots' fences have
    /// passed (a pass recorded this tick may still reference the target)
    pub retired_panels: Vec<(u64, PanelSwapchain)>,
    pub surface_tex: HashMap<ObjectId, SurfaceTex>,
    pub dmabuf_textures: HashMap<ObjectId, (Texture, wl_buffer::WlBuffer)>,
    pub pending_dmabufs: HashMap<ObjectId, Dmabuf>,

    /// declared before `xr`: fields drop in order, and the renderer's views of the swapchain
    /// images must go before the runtime frees the images (teardown, `Drop for Zxr`)
    pub renderer: Renderer,
    pub xr: XrCore,
    pub frame_id: u64,
    pub slot: usize,
    /// buffers each frame slot's last submission sampled; released after that slot's fence
    pub held: [Vec<HeldBuffer>; 2],
    pub journal: Journal,
    pub frames_limit: Option<u64>,
    pub journal_path: Option<std::path::PathBuf>,
    /// the wearer's preferences and the input calibrations, resolved in-process (settings.rs;
    /// research/73 §6 option b); `prefs.generation` moves on every reload
    pub prefs: crate::settings::Prefs,
    /// the window-management floor (policy/, window-workspace-management §2–§8)
    pub policy: crate::policy::Policy,
    /// the window-management seam (policy/seam.rs, wm §11)
    pub seam: crate::policy::seam::Seam,
    /// the open settings engine and its watch state, when an artifact exists
    pub settings: Option<crate::settings::Settings>,
    pub children: Vec<Child>,
    /// xwayland-satellite's pid when spawned: its toplevels are the X11 ones (gate 4)
    pub satellite_pid: Option<u32>,
    pub pointer_focus: Option<WlSurface>,
    /// the R0 head-floor path's last sent (surface, wl_fixed point) — the still-pointer rule
    pub last_gaze_sent: Option<(ObjectId, (f64, f64))>,
    pub last_head_pose: Option<openxr::Posef>,
    /// research/63 Phase 1b: how panels reach the runtime (projection pass / quad layers / both)
    /// `--debug-panels`: force every plane into the projection layer (the R0 path) for measurement
    pub debug_panels: DebugPanels,
    /// native-openxr-apps.md §4 quiet mode: a native app is primary — submit no layers, run no
    /// panel or projection pass, hold no client buffers; planes get fallback callbacks only.
    /// Set by the primary-client observer (libmonado, M1); the control socket toggles it for
    /// measurement.
    pub quiet: bool,
    /// the input module (spatial-input §1a): the stage chain, the intake queue, presence
    pub input: crate::input::Input,
    /// research/69: release moment for buffers of non-sampled surfaces (`--debug-hold`)
    pub hold: HoldPolicy,
    /// `HoldPolicy::Tick`: released at the top of the next tick
    pub held_tick: Vec<HeldBuffer>,
    /// `HoldPolicy::Callback`: released when the member's frame callback is sent
    pub held_callback: Vec<(MemberId, HeldBuffer)>,
    /// depth-content hooks (spec §7 rev 3): counts of what needs zxr's projection layer. All zero
    /// until M2 (volumes) and the passthrough rung (environment, cutout sources).
    pub volumes_mapped: u32,
    pub environment_source: bool,
    pub cutout_source: bool,
    /// the runtime's `XrSystemGraphicsProperties::maxLayerCount`
    pub max_layer_count: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DebugPanels {
    /// the rule: quads always, projection only with depth content
    #[default]
    Auto,
    /// measurement override: every plane drawn in the projection layer, no quads
    Projection,
}

/// research/69: when the buffer of a surface zxr is *not* sampling (quiet mode, hidden or
/// unmapped member) is released back to the client. The axis is the release moment; a composed
/// member's buffers are always held to the fence of the frame that sampled them (§6.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HoldPolicy {
    /// released by the replacing commit (smithay's default; nothing held)
    #[default]
    Replacement,
    /// released at the next tick
    Tick,
    /// released when the member next receives a frame callback (the fallback cadence)
    Callback,
    /// held like a sampled buffer: released when this slot's fence completes (two ticks later)
    Fence,
}

/// A 2D plane's runtime-owned panel swapchain and the render target over its images. Grow-only:
/// the image may be larger than the panel (spec §5a); `bounds` is what the panel occupies now.
pub struct PanelSwapchain {
    pub sc: crate::xr::Swapchain,
    pub target: crate::render::ViewTarget,
    /// a pass has written the current bounds into the image (a fresh swapchain has nothing to show)
    pub has_image: bool,
    /// panel bounds in logical px relative to the toplevel geometry origin (geometry union popups)
    pub bounds: smithay::utils::Rectangle<i32, smithay::utils::Logical>,
    /// the tick the bounds first fell below the image extent — the lazy-shrink debounce
    pub shrink_since: Option<u64>,
    pub passes: u64,
}

/// Ticks the bounds must stay smaller than the image before the swapchain is recreated smaller
/// (spec §5a stand-in: 60 ≈ 1 s at 60 Hz; fixed by measurement).
pub const PANEL_SHRINK_TICKS: u64 = 60;

/// Which client image the cursor panel holds (spatial-input §7): a theme image by its
/// `cursor-shape-v1` name, or a `set_cursor` surface at one of its commits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CursorImageKey {
    Named(smithay::input::pointer::CursorIcon),
    Surface(smithay::reexports::wayland_server::backend::ObjectId, smithay::backend::renderer::utils::CommitCounter),
}

/// What the cursor panel holds: the content shape and the image, if any. The frame procedure
/// redraws the panel only when this changes — never per pointer motion (research/70 §9).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CursorKey {
    pub content: crate::input::cursor::Content,
    pub image: Option<CursorImageKey>,
}

/// The frontend's member payload (spec §5a `M`): the smithay window, its panel, and the
/// per-member state the tick reads. `scene` never sees these types.
pub struct Payload {
    /// the plane's surface role: an xdg toplevel, a layer surface or a lock surface (`shell::Surface`)
    pub window: Surface,
    pub panel: Option<PanelSwapchain>,
    /// set by the commit handler when any surface of this member's tree committed; cleared by
    /// the panel pass. Nothing is hashed per tick.
    pub dirty: bool,
    /// the tick the first buffer arrived (0 = not yet mapped)
    pub mapped_at: u64,
    /// the last tick this member received `wl_surface.frame` (research/65 §4.2)
    pub last_frame_callback: u64,
    /// window-workspace-management.md: hidden — not rendered, keeps place and pose. Excluded
    /// from the flatten and the dirty walk; frame callbacks on the fallback cadence (research/69
    /// A/B; the design's stricter "no frame callbacks" is river's `hide`).
    pub hidden: bool,
    /// spatial-input §6: demands attention — a refused activation, a new window a commit
    /// pre-empted. The shell presents it; the compositor never moves or raises for it.
    pub urgent: bool,
    /// the seat's `last_commit_serial` when this toplevel was requested (`new_toplevel`); the
    /// new-window rule compares it at map time (mutter `intervening_user_event_occurred`)
    pub requested_at_commit: Option<Serial>,
    /// an `xdg_activation_v1.activate` arrived before the first buffer (niri keeps the token on
    /// the unmapped window, `handlers/mod.rs:836-838`): its serial, applied at map
    pub pending_activation: Option<Option<Serial>>,
    /// the member belongs to a trusted (socketpair) connection: composed and routed while the
    /// mode gate is closed (spec §9 rev 3.12)
    pub trusted: bool,
}

impl Payload {
    pub fn mapped(&self) -> bool {
        self.mapped_at != 0
    }
    /// composed this tick when zxr presents: mapped and not hidden
    pub fn presentable(&self) -> bool {
        self.mapped() && !self.hidden
    }
    pub fn root(&self) -> Option<WlSurface> {
        self.window.wl_surface()
    }
}

/// The plane extents in metres of a window's current geometry at the scene's density
/// (`wm.density_px_per_cm`, `Layout::m_per_px`).
pub fn plane_size_of(window: &Surface, layout: &scene::Layout) -> [f32; 2] {
    let g = window.geometry().size;
    layout.plane_size(g.w, g.h)
}

impl Zxr {
    pub fn new(
        display: Display<Zxr>,
        loop_handle: LoopHandle<'static, Zxr>,
        loop_signal: LoopSignal,
        xr: XrCore,
        renderer: Renderer,
        socket: Option<&str>,
        listen: bool,
        drm_node: Option<&str>,
    ) -> Result<Zxr, String> {
        let dh = display.handle();
        let start = Instant::now();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let viewporter = ViewporterState::new::<Self>(&dh);
        let presentation = PresentationState::new::<Self>(&dh, libc::CLOCK_MONOTONIC as u32);
        let single_pixel = SinglePixelBufferState::new::<Self>(&dh);
        let cursor_shape = smithay::wayland::cursor_shape::CursorShapeManagerState::new::<Self>(&dh);
        let idle_notifier = smithay::wayland::idle_notify::IdleNotifierState::<Self>::new(&dh, loop_handle.clone());
        let idle_inhibit = smithay::wayland::idle_inhibit::IdleInhibitManagerState::new::<Self>(&dh);
        let popups = PopupManager::default();
        // ---- focus/activation and the text-entry seam (spatial-input §6, §8, §12)
        let activation_state = XdgActivationState::new::<Self>(&dh);
        let text_input_state = TextInputManagerState::new::<Self>(&dh);
        // The IM and virtual-keyboard globals are privileged (a client that binds them reads
        // and writes every keystroke). niri gates them on the security context
        // (`niri.rs:2462-2466`, `client_is_unrestricted`); zxr's gate — the session's keyboard
        // component only — is the filter closure here. Open to every client until the
        // component and its unit exist (flagged in the lane report).
        // rev 3.12: the predicate is the connection's `restricted` bit (shell/filter.rs) — the
        // privileged set is hidden from security-context clients (research/30's rule; niri,
        // cosmic-comp, Hyprland)
        let input_method_state = InputMethodManagerState::new::<Self, _>(&dh, crate::shell::filter::unrestricted);
        let virtual_keyboard_state = VirtualKeyboardManagerState::new::<Self, _>(&dh, crate::shell::filter::unrestricted);
        let shortcuts_inhibit_state = KeyboardShortcutsInhibitState::new::<Self>(&dh);
        tracing::info!("text-input-v3, input-method-v2, virtual-keyboard-v1, keyboard-shortcuts-inhibit, xdg-activation globals created (spatial-input §6, §12)");
        // ---- the shell-layer half (spec §4/§9/§10 rev 3.12): layer-shell + anchoring, the
        // desktop profile's session lock, security-context — all behind the filter
        let layer_shell_state = smithay::wayland::shell::wlr_layer::WlrLayerShellState::new_with_filter::<Self, _>(&dh, crate::shell::filter::unrestricted);
        let layer_anchoring_global = dh.create_global::<Self, crate::shell::anchoring::zxr_layer_anchoring_v1::ZxrLayerAnchoringV1, ()>(1, ());
        let session_lock_state = smithay::wayland::session_lock::SessionLockManagerState::new::<Self, _>(&dh, crate::shell::filter::unrestricted);
        let security_context_state = smithay::wayland::security_context::SecurityContextState::new::<Self, _>(&dh, crate::shell::filter::no_security_context);
        tracing::info!("wlr-layer-shell v5, zxr-layer-anchoring-v1, ext-session-lock-v1, security-context-v1 globals created behind the per-connection filter (spec §10 rev 3.12)");

        // ---- dmabuf v4 with feedback: the render node + the modifiers the device samples (§6.1)
        let node = drm_node.map(String::from).or_else(find_render_node);
        let mut formats: Vec<Format> = Vec::new();
        for (fourcc, vkfmt) in [(Fourcc::Argb8888, ash::vk::Format::B8G8R8A8_SRGB), (Fourcc::Xrgb8888, ash::vk::Format::B8G8R8A8_SRGB), (Fourcc::Abgr8888, ash::vk::Format::R8G8B8A8_SRGB), (Fourcc::Xbgr8888, ash::vk::Format::R8G8B8A8_SRGB)] {
            for m in renderer.sampled_modifiers(vkfmt) {
                formats.push(Format { code: fourcc, modifier: Modifier::from(m) });
            }
        }
        tracing::info!(node = ?node, formats = formats.len(), "dmabuf feedback table");
        let mut dmabuf_state = DmabufState::new();
        let (dmabuf_global, syncobj_state) = match &node {
            Some(path) => {
                let fd = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
                let dev_fd = DrmDeviceFd::new(OwnedFd::from(fd).into());
                let dev_t = dev_fd.dev_id().map_err(|e| e.to_string())?;
                let feedback = DmabufFeedbackBuilder::new(dev_t, formats.clone()).build().map_err(|e| e.to_string())?;
                let global = dmabuf_state.create_global_with_default_feedback::<Self>(&dh, &feedback);
                let syncobj = if supports_syncobj_eventfd(&dev_fd) {
                    tracing::info!("linux-drm-syncobj-v1: eventfd supported, global created");
                    Some(DrmSyncobjState::new::<Self>(&dh, dev_fd))
                } else {
                    tracing::warn!("linux-drm-syncobj-v1: eventfd unsupported, implicit sync only");
                    None
                };
                (global, syncobj)
            }
            None => {
                tracing::warn!("no render node found; dmabuf v3 without feedback, no syncobj");
                (dmabuf_state.create_global::<Self>(&dh, formats.clone()), None)
            }
        };

        // ---- seat: keyboard + pointer (the gaze ray drives the pointer at R0, §8)
        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        // the repeat the settings declare as default (`input.keyboard.repeat.{delay_ms,rate_hz}`
        // 600 / 25 — Hyprland's and COSMIC's, as preferences.nix records; GNOME's are 500 / 33); the
        // wearer's values arrive through settings.rs `apply` at start and live
        seat.add_keyboard(Default::default(), 600, 25).map_err(|e| e.to_string())?;
        seat.add_pointer();

        // ---- the one wl_output: a virtual panel (§10 notes; sized to the view)
        let output = Output::new("XR-1".into(), PhysicalProperties { size: (600, 340).into(), subpixel: Subpixel::Unknown, make: "Mura".into(), model: "zxr virtual".into(), serial_number: "0".into() });
        let _global = output.create_global::<Self>(&dh);
        // the output *is* the head rectangle (research/77 §3.3): its mode follows `shell.head.*`,
        // 1920 wide, 1493 tall at the 90×70° default; `shell::take_prefs` re-derives it live
        let head_size = crate::shell::head_mode_size(&crate::shell::HeadCfg::default());
        let mode = Mode { size: (head_size.w, head_size.h).into(), refresh: 60_000 };
        output.change_current_state(Some(mode), Some(Transform::Normal), Some(Scale::Integer(1)), Some((0, 0).into()));
        output.set_preferred(mode);

        // ---- listening socket + display source. `--greeter` binds none (session-auth §5: "no
        // client Wayland listening socket"); its clients arrive over socketpairs (shell/filter.rs).
        let socket_name = if listen {
            let listening = match socket {
                Some(name) => ListeningSocketSource::with_name(name),
                None => ListeningSocketSource::new_auto(),
            }
            .map_err(|e| format!("wayland socket: {e}"))?;
            let socket_name = listening.socket_name().to_os_string();
            loop_handle
                .insert_source(listening, move |stream, _, state| {
                    // the public socket: neither restricted nor trusted (spec §10 rev 3.12)
                    if let Err(e) = state.dh.insert_client(stream, Arc::new(ClientState::default())) {
                        tracing::warn!("insert_client: {e}");
                    }
                })
                .map_err(|e| e.to_string())?;
            socket_name
        } else {
            tracing::info!("no listening socket (restricted mode): clients are admitted over socketpairs only");
            OsString::new()
        };
        loop_handle
            .insert_source(Generic::new(display, Interest::READ, CMode::Level), |_, display, state| {
                // SAFETY: the display is never dropped while the source lives
                unsafe {
                    display.get_mut().dispatch_clients(state).unwrap();
                }
                Ok(PostAction::Continue)
            })
            .map_err(|e| e.to_string())?;

        let xr_max_layer_count = xr.max_layer_count;
        let mut journal = Journal::default();
        journal.started_at_ns = now_ns();

        Ok(Zxr {
            dh: dh.clone(),
            loop_handle,
            loop_signal,
            socket_name,
            start,
            compositor_state,
            xdg_shell_state,
            shm_state,
            _output_manager_state: output_manager_state,
            seat_state,
            data_device_state,
            dmabuf_state,
            _dmabuf_global: dmabuf_global,
            syncobj_state,
            _viewporter: viewporter,
            _presentation: presentation,
            _single_pixel: single_pixel,
            _cursor_shape: cursor_shape,
            idle_notifier,
            _idle_inhibit: idle_inhibit,
            idle_inhibitors: Vec::new(),
            cursor_panel: None,
            bar_panel: None,
            cursor_key: None,
            ring_tex: None,
            bar_tex: None,
            cursor_theme: crate::input::theme::Theme::from_env(),
            cursor_named: None,
            activation_state,
            _text_input_state: text_input_state,
            _input_method_state: input_method_state,
            _virtual_keyboard_state: virtual_keyboard_state,
            shortcuts_inhibit_state,
            layer_shell_state,
            _layer_anchoring_global: layer_anchoring_global,
            shell_anchoring_managers: Vec::new(),
            session_lock_state,
            _security_context_state: security_context_state,
            shell: crate::shell::Shell::new(),
            trusted_clients: Vec::new(),
            popups,
            seat,
            output,
            focus: focus::Focus::default(),
            text: text::Text::default(),
            peripherals: libinput::Peripherals::default(),
            ei: ei::EiServer::default(),
            scene: Scene::new(),
            retired_panels: Vec::new(),
            surface_tex: HashMap::new(),
            dmabuf_textures: HashMap::new(),
            pending_dmabufs: HashMap::new(),
            xr,
            renderer,
            frame_id: 0,
            slot: 0,
            held: [Vec::new(), Vec::new()],
            journal,
            frames_limit: None,
            journal_path: None,
            prefs: crate::settings::Prefs::default(),
            policy: crate::policy::Policy::default(),
            seam: crate::policy::seam::serve(&dh),
            settings: None,
            children: Vec::new(),
            satellite_pid: None,
            pointer_focus: None,
            last_gaze_sent: None,
            last_head_pose: None,
            debug_panels: DebugPanels::default(),
            quiet: false,
            input: crate::input::Input::default(),
            hold: HoldPolicy::default(),
            held_tick: Vec::new(),
            held_callback: Vec::new(),
            volumes_mapped: 0,
            environment_source: false,
            cutout_source: false,
            max_layer_count: xr_max_layer_count,
        })
    }

    /// Synthesised keyboard input through the seat (the harness's `key`/`type`): evdev code →
    /// xkb keycode (+8), forwarded to the focused surface like any device key.
    pub fn send_key(&mut self, evdev: u32, pressed: bool) {
        let time = smithay::backend::input::InputTime::now();
        self.send_key_at(evdev, pressed, time.micros() * 1000);
    }

    /// A key edge with the device's own CLOCK_MONOTONIC time (libinput / EI samples). smithay
    /// routes it inside `KeyboardHandle::input`: to the input method's grab when one holds the
    /// keyboard (`InputMethodKeyboardGrab`), else to the focused surface (spatial-input §12).
    pub fn send_key_at(&mut self, evdev: u32, pressed: bool, time_ns: u64) {
        use smithay::backend::input::KeyState;
        use smithay::input::keyboard::{FilterResult, Keycode};
        let kb = self.seat.get_keyboard().unwrap();
        let serial = SERIAL_COUNTER.next_serial();
        let time = smithay::backend::input::InputTime::from_micros(time_ns / 1000);
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        kb.input::<(), _>(self, Keycode::new(evdev + 8), state, serial, time, |_, _, _| FilterResult::Forward);
    }

    /// spatial-input §8: does the keyboard-focused surface hold an active
    /// `keyboard-shortcuts-inhibit` inhibitor? The compositor's own key chords (none yet) check
    /// this after the `Reserved` stage — the reserved input is never inhibited.
    #[allow(dead_code)] // no compositor key chord exists yet to ask
    pub fn shortcuts_inhibited(&self) -> bool {
        let Some(kb) = self.seat.get_keyboard() else { return false };
        let Some(focus) = kb.current_focus() else { return false };
        self.text.inhibitor_for(&focus).map(|i| i.is_active()).unwrap_or(false)
    }

    /// spatial-input §8 / §12: a physical key was pressed recently — the shell keeps the
    /// on-screen keyboard down (`text.rs` for what smithay lets the compositor do about it).
    #[allow(dead_code)] // read by the shell protocol / `zxr ctl` once they carry it
    pub fn osk_suppressed(&self) -> bool {
        // `input.osk.enabled = false` is a permanent suppression: the wearer has said no OSK
        !self.prefs.osk_enabled || self.text.suppressed(now_ns())
    }

    fn client_is_satellite(&self, surface: &WlSurface) -> bool {
        match (self.satellite_pid, surface.client()) {
            (Some(pid), Some(client)) => client.get_credentials(&self.dh).map(|c| c.pid as u32 == pid).unwrap_or(false),
            _ => false,
        }
    }

    /// Spec ?7 rev 3: does this tick need zxr's projection layer? A mapped 3D volume, an
    /// environment or cutout source, panel overflow past the layer cap ? or the debug override.
    pub fn depth_content_present(&self, overflow: bool) -> bool {
        self.debug_panels == DebugPanels::Projection || overflow || self.volumes_mapped > 0 || self.environment_source || self.cutout_source
    }

    /// How many planes may be quad layers this tick: the cap minus one reserved for the projection
    /// layer (spec ?7 rev 3); the rest are drawn in the projection layer.
    pub fn quad_budget(&self) -> usize {
        self.max_layer_count.saturating_sub(1).max(1) as usize
    }

    /// The member whose toplevel surface is `root` (linear over N ≈ 50 members).
    pub fn member_for_root(&self, root: &WlSurface) -> Option<MemberId> {
        self.scene.find(|p| p.window.wl_surface().as_ref() == Some(root))
    }

    /// A member's window geometry in logical pixels.
    pub fn logical_size(&self, id: MemberId) -> Option<(i32, i32)> {
        let m = self.scene.get(id)?;
        let g = m.m.window.geometry().size;
        Some((g.w.max(1), g.h.max(1)))
    }

    /// Ask a member's client for a new logical size (a resize grab's step, window-workspace-
    /// management §4a: resize changes pixels). The plane's extents follow the client's commit.
    pub fn request_size(&mut self, id: MemberId, w: i32, h: i32) {
        let Some(t) = self.scene.get(id).and_then(|m| m.m.window.toplevel().cloned()) else { return };
        t.with_pending_state(|s| s.size = Some((w.max(1), h.max(1)).into()));
        t.send_pending_configure();
    }

    /// The seat keyboard's `logo` (Super) modifier is down — the desktops' move modifier
    /// (GNOME `mouse-button-modifier`, KWin `CommandAllKey`).
    pub fn modifier_logo(&self) -> bool {
        self.seat.get_keyboard().map(|k| k.modifier_state().logo).unwrap_or(false)
    }

    pub fn window_for_root(&self, root: &WlSurface) -> Option<&Surface> {
        self.member_for_root(root).and_then(|id| self.scene.get(id)).map(|m| &m.m.window)
    }

    /// Commit → member (spec §5a): the subsurface parent chain to its root, then — if the root
    /// is a popup — the popup's toplevel root; `None` for surfaces no panel shows (cursors, drag
    /// icons, not-yet-tracked popups).
    pub fn member_for_committed(&self, surface: &WlSurface) -> Result<Option<MemberId>, ()> {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if let Some(id) = self.member_for_root(&root) {
            return Ok(Some(id));
        }
        if let Some(kind) = self.popups.find_popup(&root) {
            return match find_popup_root_surface(&kind) {
                Ok(toplevel) => Ok(self.member_for_root(&toplevel)),
                // a popup whose parent chain is broken: the conservative fallback
                Err(_) => Err(()),
            };
        }
        Ok(None)
    }

    /// Mark a member's panel for a pass this tick.
    pub fn mark_dirty(&mut self, id: MemberId) {
        if let Some(m) = self.scene.get_mut(id) {
            m.m.dirty = true;
        }
    }

    pub fn mark_all_dirty(&mut self) {
        for (_, m) in self.scene.iter_mut() {
            if m.m.mapped() {
                m.m.dirty = true;
            }
        }
    }

    /// Take a member's panel out of service; its Vulkan target dies after both slots' fences.
    pub fn retire_panel(&mut self, panel: PanelSwapchain) {
        self.journal.panel_swapchains_destroyed += 1;
        self.retired_panels.push((self.frame_id, panel));
    }

    pub fn flush_retired_panels(&mut self, frame: u64) {
        let mut i = 0;
        while i < self.retired_panels.len() {
            if frame.saturating_sub(self.retired_panels[i].0) >= 2 {
                let (_, p) = self.retired_panels.swap_remove(i);
                self.renderer.destroy_target(p.target);
            } else {
                i += 1;
            }
        }
    }

    /// Bring a surface's texture up to date with its committed buffer. Returns the texture
    /// to draw (borrowed from the caches) and records the held buffer for this frame.
    pub fn update_surface_texture(&mut self, surface: &WlSurface, frame: u64) -> Option<TexRef> {
        let id = surface.id();
        let (buffer, commit, size) = with_renderer_surface_state(surface, |st| (st.buffer().cloned(), st.current_commit(), st.buffer_size()))?;
        let buffer = buffer?;
        let size = size?;
        let entry = self.surface_tex.entry(id.clone()).or_insert(SurfaceTex { texture: None, dmabuf_buffer: None, commit: None, last_used_frame: frame });
        entry.last_used_frame = frame;
        let changed = entry.commit != Some(commit);
        let wl_buffer: &wl_buffer::WlBuffer = &buffer;
        let result;
        if let Ok(dmabuf) = get_dmabuf(wl_buffer) {
            let bid = wl_buffer.id();
            if !self.dmabuf_textures.contains_key(&bid) {
                let dmabuf = dmabuf.clone();
                let fd = dmabuf.handles().next()?.try_clone_to_owned().ok()?;
                let tex = match self.renderer.import_dmabuf(&fd, dmabuf.width(), dmabuf.height(), dmabuf.format().code as u32, u64::from(dmabuf.format().modifier), dmabuf.offsets().next()?, dmabuf.strides().next()?) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("dmabuf import at commit: {e}");
                        return None;
                    }
                };
                self.journal.dmabuf_imports += 1;
                self.dmabuf_textures.insert(bid.clone(), (tex, wl_buffer.clone()));
            }
            let entry = self.surface_tex.get_mut(&id).unwrap();
            if let Some(old) = entry.texture.take() {
                self.renderer.destroy_texture(old);
            }
            entry.dmabuf_buffer = Some(bid.clone());
            entry.commit = Some(commit);
            result = TexRef::Dmabuf(bid);
        } else {
            // shm: (re)create on size change, upload on commit change
            let (w, h) = (size.w.max(1) as u32, size.h.max(1) as u32);
            let entry = self.surface_tex.get_mut(&id).unwrap();
            entry.dmabuf_buffer = None;
            let need_new = entry.texture.as_ref().map(|t| t.width != w || t.height != h).unwrap_or(true);
            if need_new {
                if let Some(old) = entry.texture.take() {
                    self.renderer.destroy_texture(old);
                }
                match self.renderer.create_shm_texture(w, h) {
                    Ok(t) => entry.texture = Some(t),
                    Err(e) => {
                        tracing::warn!("shm texture: {e}");
                        return None;
                    }
                }
            }
            if changed || need_new {
                let tex = entry.texture.as_mut().unwrap();
                let renderer = &mut self.renderer;
                let up = with_buffer_contents(wl_buffer, |ptr, len, data| {
                    // SAFETY: smithay hands us the mapped pool for the closure's duration
                    let bytes = unsafe { std::slice::from_raw_parts(ptr.add(data.offset as usize), len - data.offset as usize) };
                    renderer.upload_shm(tex, bytes, data.stride as u32)
                });
                match up {
                    Ok(Ok(())) => self.journal.shm_uploads += 1,
                    Ok(Err(e)) => tracing::warn!("shm upload: {e}"),
                    Err(e) => tracing::warn!("shm access: {e:?}"),
                }
                entry.commit = Some(commit);
            }
            result = TexRef::Surface(id);
        }
        // hold the client buffer until this frame's GPU work is done (§6.5)
        self.held[self.slot].push(HeldBuffer { buffer, since_frame: frame });
        Some(result)
    }

    /// Drop textures of surfaces not sampled for a while. A surface is sampled only by a panel
    /// pass (spec §5a), so a static surface's shm texture goes too — its content lives in the
    /// panel image; the next commit re-creates and uploads it (10 s at 60 Hz).
    pub fn gc_textures(&mut self, frame: u64) {
        let stale: Vec<ObjectId> = self.surface_tex.iter().filter(|(_, e)| frame.saturating_sub(e.last_used_frame) > 600).map(|(k, _)| k.clone()).collect();
        for id in stale {
            if let Some(e) = self.surface_tex.remove(&id) {
                if let Some(t) = e.texture {
                    self.renderer.destroy_texture(t);
                }
            }
        }
    }

    /// Release the buffers held by the slot whose fence just completed.
    pub fn release_held(&mut self, slot: usize, frame: u64) {
        let held = std::mem::take(&mut self.held[slot]);
        for h in held {
            self.journal.record_release(frame.saturating_sub(h.since_frame));
            drop(h.buffer);
        }
    }

    /// research/69: a commit to a surface zxr will not sample this tick (quiet, or the member is
    /// hidden / unmapped). Under `Replacement` nothing is held and smithay releases the previous
    /// buffer now; under the other policies the committed buffer is held until the policy's
    /// release moment, which is the only back-pressure a client that ignores frame callbacks
    /// feels. The commit handler is the one place a non-sampled buffer is ever held.
    pub fn hold_if_not_sampled(&mut self, id: MemberId, surface: &WlSurface) {
        if self.hold == HoldPolicy::Replacement {
            return;
        }
        let sampled = !self.quiet && self.scene.get(id).map(|m| m.m.presentable()).unwrap_or(false);
        if sampled {
            return;
        }
        let Some(Some(buffer)) = with_renderer_surface_state(surface, |st| st.buffer().cloned()) else { return };
        let frame = self.frame_id;
        self.journal.held_unsampled += 1;
        let h = HeldBuffer { buffer, since_frame: frame };
        match self.hold {
            HoldPolicy::Replacement => unreachable!(),
            HoldPolicy::Tick => self.held_tick.push(h),
            HoldPolicy::Callback => self.held_callback.push((id, h)),
            HoldPolicy::Fence => self.held[self.slot].push(h),
        }
        let outstanding = self.held_tick.len() + self.held_callback.len() + self.held[0].len() + self.held[1].len();
        self.journal.held_outstanding_max = self.journal.held_outstanding_max.max(outstanding as u64);
    }

    /// research/69 §3: `xdg_toplevel.suspended` (xdg-shell v6: "the surface is currently not
    /// ordinarily being repainted") is the protocol's word for a plane zxr is not composing —
    /// quiet mode or a hidden member. KWin sets it on the visibility change
    /// (`windowitem.cpp:195-203`), mutter 3 s after the window hides (`window.c:110, 2286-2335`);
    /// zxr sets it immediately (the hysteresis is a flagged judgment, research/69 §3).
    pub fn set_suspended(&mut self, id: MemberId, on: bool) {
        let Some(m) = self.scene.get(id) else { return };
        let Some(t) = m.m.window.toplevel().cloned() else { return };
        let changed = t.with_pending_state(|s| {
            let had = s.states.contains(xdg_toplevel::State::Suspended);
            if on {
                s.states.set(xdg_toplevel::State::Suspended);
            } else {
                s.states.unset(xdg_toplevel::State::Suspended);
            }
            had != on
        });
        if changed {
            t.send_pending_configure();
            self.journal.suspended_configures += 1;
        }
    }

    /// `wm.density_px_per_cm` changed: every window plane is re-derived from its geometry at
    /// the new scale (the placement stays; only the extents move).
    pub fn rescale_planes(&mut self) {
        let layout = self.scene.layout;
        let ids: Vec<(MemberId, [f32; 2])> = self.scene.iter().filter(|(_, m)| m.m.window.toplevel().is_some()).map(|(id, m)| (id, plane_size_of(&m.m.window, &layout))).collect();
        for (id, size) in ids {
            self.scene.set_shape(id, Shape::Plane { size });
        }
    }

    /// A native app became (or stopped being) primary (native-openxr-apps §4; the libmonado
    /// observer, M1): quiet mode follows, unless `games.keep_planes` keeps the planes composed
    /// over the game.
    pub fn primary_changed(&mut self, native_primary: bool) {
        let quiet = native_primary && !self.prefs.games_keep_planes;
        if quiet != self.quiet {
            self.set_quiet(quiet);
        }
    }

    /// Quiet mode on/off: every mapped plane is (un)suspended (spec §7 rev 3.3).
    pub fn set_quiet(&mut self, on: bool) {
        self.quiet = on;
        let ids: Vec<MemberId> = self.scene.iter_mut().filter(|(_, m)| m.m.mapped()).map(|(id, _)| id).collect();
        for id in ids {
            let hidden = self.scene.get(id).map(|m| m.m.hidden).unwrap_or(false);
            self.set_suspended(id, on || hidden);
        }
    }

    /// `HoldPolicy::Tick`: the top of a tick releases everything held since the last one.
    pub fn release_held_tick(&mut self, frame: u64) {
        let held = std::mem::take(&mut self.held_tick);
        for h in held {
            self.journal.record_release(frame.saturating_sub(h.since_frame));
            drop(h.buffer);
        }
    }

    /// `HoldPolicy::Callback`: a member that just received `wl_surface.frame` gets its buffers back.
    pub fn release_held_callback(&mut self, id: MemberId, frame: u64) {
        if self.held_callback.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.held_callback.len() {
            if self.held_callback[i].0 == id {
                let (_, h) = self.held_callback.swap_remove(i);
                self.journal.record_release(frame.saturating_sub(h.since_frame));
                drop(h.buffer);
            } else {
                i += 1;
            }
        }
    }

    pub fn set_keyboard_focus(&mut self, surface: Option<WlSurface>) {
        let kb = self.seat.get_keyboard().unwrap();
        let serial = SERIAL_COUNTER.next_serial();
        if kb.current_focus() != surface {
            self.journal.focus_changes += 1;
        }
        kb.set_focus(self, surface, serial);
    }

    pub fn focus_window(&mut self, id: Option<MemberId>) {
        if !self.scene.focus(id) {
            return;
        }
        // the policy's most-recently-used order (tidy places by it, wm §4)
        if let Some(m) = id {
            self.policy.touch(m);
        }
        // spec §8 rev 3.12: an `exclusive` layer surface owns the keyboard above the stack and no
        // toplevel is Activated while it exists (cosmic-comp's rule; focus::layer_focus_override)
        let override_member = focus::layer_focus_override(self);
        // a `none` layer surface never takes the keyboard: the stack's member keeps it
        let id_accepts = id.map(|m| crate::shell::layer_accepts_focus(self, m).unwrap_or(true)).unwrap_or(true);
        let effective = override_member.or(if id_accepts { id } else { self.focus.stack.restore(|m| self.scene.get(m).map(|x| x.m.mapped() && x.m.window.is_window()).unwrap_or(false)) });
        for (i, m) in self.scene.iter() {
            if let Some(t) = m.m.window.toplevel() {
                t.with_pending_state(|s| {
                    if Some(i) == effective {
                        s.states.set(xdg_toplevel::State::Activated);
                    } else {
                        s.states.unset(xdg_toplevel::State::Activated);
                    }
                });
                t.send_pending_configure();
            }
        }
        let surf = effective.and_then(|m| self.scene.get(m)).and_then(|m| m.m.root());
        self.set_keyboard_focus(surf);
    }

    /// Spec §5a hit test, then smithay's 2D hit within the plane: the member, the surface under
    /// the ray and the surface-local point.
    pub fn hit_surface(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<(MemberId, WlSurface, smithay::utils::Point<f64, smithay::utils::Logical>)> {
        let gated = self.input.mode != crate::input::Mode::Normal;
        let (id, local, _) = self.scene.hit(origin, dir, |p| p.mapped() && (!gated || p.trusted))?;
        self.hit_surface_at(id, local)
    }

    /// The surface under a plane-local point of a member (the second half of the hit test:
    /// smithay's surface tree). Used by the input transports over the Hit stage's `input::Hit`.
    pub fn hit_surface_at(&self, id: MemberId, local: [f32; 2]) -> Option<(MemberId, WlSurface, smithay::utils::Point<f64, smithay::utils::Logical>)> {
        let m = self.scene.get(id)?;
        let Shape::Plane { size } = m.shape else { return None };
        let g = m.m.window.geometry();
        let (x, y) = scene::local_to_logical(local, size, (g.loc.x, g.loc.y, g.size.w, g.size.h));
        let logical = smithay::utils::Point::<f64, smithay::utils::Logical>::from((x, y));
        let (surface, loc) = m.m.window.surface_under(logical, smithay::desktop::WindowSurfaceType::ALL)?;
        Some((id, surface, logical - loc.to_f64()))
    }

    /// Gaze pointer (§8 R0): cast the head ray, move the pointer to the surface under it.
    pub fn update_gaze_pointer(&mut self, pose: openxr::Posef) {
        self.last_head_pose = Some(pose);
        let pointer = self.seat.get_pointer().unwrap();
        let origin = [pose.position.x, pose.position.y, pose.position.z];
        let dir = math::rotate(pose.orientation, [0.0, 0.0, -1.0]);
        let hit = self.hit_surface(origin, dir);
        let serial = SERIAL_COUNTER.next_serial();
        let time = smithay::backend::input::InputTime::now();
        match hit {
            Some((_, surface, loc)) => {
                // the still-pointer rule (spec §8 rev 3.12): the same surface at the same logical pixel
                // sends nothing (wlroots `wlr_seat_pointer_send_motion`, at the pixel — pointer.rs)
                let key = (surface.id(), (loc.x, loc.y));
                let same = self.last_gaze_sent.as_ref().map(|(id, (lx, ly))| *id == key.0 && (lx - loc.x).abs() < 1.0 && (ly - loc.y).abs() < 1.0).unwrap_or(false);
                if self.pointer_focus.as_ref() == Some(&surface) && same {
                    self.journal.pointer_motion_deduped += 1;
                    return;
                }
                pointer.motion(self, Some((surface.clone(), loc)), &smithay::input::pointer::MotionEvent { location: loc, serial, time });
                pointer.frame(self);
                self.last_gaze_sent = Some(key);
                self.pointer_focus = Some(surface);
            }
            None => {
                if self.pointer_focus.take().is_some() {
                    pointer.motion(self, None, &smithay::input::pointer::MotionEvent { location: (0.0, 0.0).into(), serial, time });
                    pointer.frame(self);
                    self.last_gaze_sent = None;
                }
            }
        }
    }

    pub(crate) fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else { return };
        let Some(window) = self.window_for_root(&root) else { return };
        let geo = window.geometry();
        // the unconstrain box: the frame rectangle for a layer popup (sway's full-output rule on
        // the frame, research/77 §2.5), the output for a window's
        let (size, origin) = match self.shell.entry_for_surface(&root) {
            Some(e) => (self.shell.rect(e.frame).map(|r| r.size).unwrap_or_else(|| crate::shell::head_mode_size(&self.shell.head)), e.box_px.map(|b| b.loc).unwrap_or_default()),
            None => (self.output.current_mode().map(|m| Size::from((m.size.w, m.size.h))).unwrap_or_else(|| crate::shell::head_mode_size(&self.shell.head)), (0, 0).into()),
        };
        let mut target = Rectangle::new((0, 0).into(), size);
        target.loc -= origin;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    pub fn now_ms(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }
}

pub enum TexRef {
    Dmabuf(ObjectId),
    Surface(ObjectId),
}

impl Drop for Zxr {
    /// Teardown order (spec §9 restart contract): GPU idle → held client buffers released →
    /// textures destroyed on the still-live device → then the fields drop: renderer (its views
    /// of the swapchain images) before `xr` (the session that owns those images).
    fn drop(&mut self) {
        // SAFETY: idle the queue before destroying anything the frames referenced
        unsafe {
            let _ = self.renderer.device.device_wait_idle();
        }
        for slot in 0..self.held.len() {
            self.held[slot].clear();
        }
        for (_, e) in self.surface_tex.drain() {
            if let Some(t) = e.texture {
                self.renderer.destroy_texture(t);
            }
        }
        for (_, (t, _)) in self.dmabuf_textures.drain() {
            self.renderer.destroy_texture(t);
        }
        for (_, m) in self.scene.iter_mut() {
            if let Some(p) = m.m.panel.take() {
                self.retired_panels.push((0, p));
            }
        }
        for (_, p) in self.retired_panels.drain(..) {
            self.renderer.destroy_target(p.target);
        }
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
        // the EIS listener lives in the loop, which outlives `Zxr`; unlink its socket here
        if let Some(p) = &self.ei.socket_path {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn now_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: plain clock_gettime into a stack struct.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

fn find_render_node() -> Option<String> {
    if let Ok(n) = std::env::var("ZXR_DRM_NODE") {
        return Some(n);
    }
    let mut nodes: Vec<String> = std::fs::read_dir("/dev/dri").ok()?.filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().into_owned()).filter(|p| p.contains("renderD")).collect();
    nodes.sort();
    nodes.into_iter().next()
}

// ---------------------------------------------------------------------------------------------
// smithay handlers
// ---------------------------------------------------------------------------------------------

impl CompositorHandler for Zxr {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        // Acquire (§6.3): explicit-sync acquire point → eventfd blocker; else the dmabuf's
        // implicit fence via the readable-fd blocker (cosmic-comp `compositor.rs:177-232`).
        add_pre_commit_hook::<Self, _>(surface, move |state, _dh, surface| {
            let mut acquire_point = None;
            let maybe_dmabuf = with_states(surface, |sd| {
                acquire_point = sd.cached_state.get::<DrmSyncobjCachedState>().pending().acquire_point.clone();
                sd.cached_state.get::<SurfaceAttributes>().pending().buffer.as_ref().and_then(|a| match a {
                    BufferAssignment::NewBuffer(b) => get_dmabuf(b).ok().cloned(),
                    _ => None,
                })
            });
            let Some(dmabuf) = maybe_dmabuf else { return };
            let client = surface.client().unwrap();
            if let Some(ap) = acquire_point {
                if let Ok((blocker, source)) = ap.generate_blocker() {
                    let c = client.clone();
                    if state
                        .loop_handle
                        .insert_source(source, move |_, _, state| {
                            let dh = state.dh.clone();
                            state.client_compositor_state(&c).blocker_cleared(state, &dh);
                            Ok(())
                        })
                        .is_ok()
                    {
                        add_blocker(surface, blocker);
                        state.journal.acquire_syncobj += 1;
                        return;
                    }
                }
            }
            state.journal.acquire_implicit += 1;
            if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) {
                if state
                    .loop_handle
                    .insert_source(source, move |_, _, state| {
                        let dh = state.dh.clone();
                        state.client_compositor_state(&client).blocker_cleared(state, &dh);
                        Ok(())
                    })
                    .is_ok()
                {
                    add_blocker(surface, blocker);
                }
            }
        });
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.journal.commits += 1;
        // popups first so a popup's first commit is tracked before root resolution
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(xdg)) = self.popups.find_popup(surface) {
            if !xdg.is_initial_configure_sent() {
                let _ = xdg.send_configure();
            }
        }
        // spec §5a: the commit sets the member's dirty flag — the only per-commit scene work
        let member = if is_sync_subsurface(surface) {
            // a sync subsurface's state applies with its parent's commit; that commit marks
            None
        } else {
            match self.member_for_committed(surface) {
                Ok(m) => m,
                Err(()) => {
                    self.journal.dirty_fallbacks += 1;
                    self.mark_all_dirty();
                    None
                }
            }
        };
        if let Some(id) = member {
            let (window, root) = {
                let m = self.scene.get(id).unwrap();
                (m.m.window.clone(), m.m.root())
            };
            window.on_commit();
            self.mark_dirty(id);
            self.hold_if_not_sampled(id, surface);
            // the shell roles have their own commit paths (shell/layer.rs, shell/lock.rs)
            match &window {
                Surface::Layer(_) => {
                    crate::shell::layer::commit(self, id, surface);
                    return;
                }
                Surface::Lock(_) => {
                    crate::shell::lock::commit(self, id, surface);
                    return;
                }
                Surface::Window(_) => {}
            }
            // xdg toplevel: initial configure, then map on first buffer
            if root.as_ref() == Some(surface) {
                let initial_sent = with_states(surface, |states| states.data_map.get::<XdgToplevelSurfaceData>().unwrap().lock().unwrap().initial_configure_sent);
                if !initial_sent {
                    if let Some(t) = window.toplevel() {
                        t.with_pending_state(|s| {
                            s.size = None;
                            s.states.set(xdg_toplevel::State::Activated);
                        });
                        t.send_configure();
                    }
                } else {
                    let has_buffer = with_renderer_surface_state(surface, |st| st.buffer().is_some()).unwrap_or(false);
                    let newly_mapped = has_buffer && !self.scene.get(id).map(|m| m.m.mapped()).unwrap_or(true);
                    // the plane's extents follow the window geometry
                    let size = plane_size_of(&window, &self.scene.layout);
                    self.scene.set_shape(id, Shape::Plane { size });
                    if newly_mapped {
                        // the size is known now: the free engine places the plane for real
                        // (window-workspace-management §3; policy::placed_at_map)
                        let parent = window.toplevel().and_then(|t| t.parent()).and_then(|p| self.member_for_root(&p));
                        crate::policy::placed_at_map(self, id, parent);
                        crate::policy::seam::window_mapped(self, id);
                        let (at_request, pending) = match self.scene.get_mut(id) {
                            Some(m) => {
                                m.m.mapped_at = self.frame_id.max(1);
                                (m.m.requested_at_commit, m.m.pending_activation.take())
                            }
                            None => (None, None),
                        };
                        self.journal.toplevels_mapped += 1;
                        if self.client_is_satellite(surface) {
                            self.journal.xwayland_toplevels += 1;
                        }
                        // spatial-input §6 lines 288-290: a new window takes focus unless a user
                        // commit intervened since the request that mapped it (mutter
                        // `window.c:2013, 2125`; niri `ActivateWindow::Smart`); an activation
                        // token delivered before the map is judged by the serial rule instead.
                        match pending {
                            Some(token_serial) => {
                                focus::activate(self, id, token_serial);
                            }
                            None if focus::new_window_rule(&self.prefs.wm_focus_new_windows, at_request, self.focus.last_commit_serial) => {
                                self.focus.new_windows_focused += 1;
                                self.focus.stack.touch(id);
                                self.focus_window(Some(id));
                            }
                            None => {
                                self.focus.new_windows_urgent += 1;
                                focus::set_urgent(self, id, true);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl BufferHandler for Zxr {
    fn buffer_destroyed(&mut self, buffer: &wl_buffer::WlBuffer) {
        let id = buffer.id();
        self.pending_dmabufs.remove(&id);
        if let Some((tex, _)) = self.dmabuf_textures.remove(&id) {
            // the frame that last sampled it holds its Buffer clone; its fence completed before
            // the client could destroy it (release precedes destroy), so the image is idle.
            self.renderer.destroy_texture(tex);
        }
        for e in self.surface_tex.values_mut() {
            if e.dmabuf_buffer.as_ref() == Some(&id) {
                e.dmabuf_buffer = None;
                e.commit = None;
            }
        }
    }
}

impl ShmHandler for Zxr {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl DmabufHandler for Zxr {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }
    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        // Import at attach time is what the protocol expects; we validate the plane count and
        // format here and do the Vulkan import lazily at first draw (one import per wl_buffer).
        let ok = dmabuf.num_planes() == 1 && matches!(dmabuf.format().code, Fourcc::Argb8888 | Fourcc::Xrgb8888 | Fourcc::Abgr8888 | Fourcc::Xbgr8888);
        if ok {
            let _ = notifier.successful::<Zxr>();
        } else {
            tracing::warn!(planes = dmabuf.num_planes(), format = ?dmabuf.format(), "dmabuf rejected");
            notifier.failed();
        }
    }
}

impl DrmSyncobjHandler for Zxr {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj_state.as_mut()
    }
}

impl XdgShellHandler for Zxr {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // a transient's parent (set before the first commit) places it beside the parent (wm §3)
        let parent = surface.parent().and_then(|p| self.member_for_root(&p));
        let window = Surface::Window(Window::new_wayland_window(surface));
        // mapped_at 0 = not yet mapped; commit() flips it on the first buffer. Placement is the
        // `free` engine's (policy/free.rs, window-workspace-management §3): the current place,
        // head-relative spawn into a free angular slot, or beside the parent.
        let size = plane_size_of(&window, &self.scene.layout);
        // the new-window rule's "request time" (spatial-input §6): the seat's last commit now
        // the connection's trusted bit is the member's (spec §9 rev 3.12)
        let trusted = window.wl_surface().and_then(|s| s.client()).map(|c| crate::shell::filter::is_trusted(&c)).unwrap_or(false);
        let payload = Payload { window, panel: None, dirty: false, mapped_at: 0, last_frame_callback: 0, hidden: false, urgent: false, requested_at_commit: self.focus.last_commit_serial, pending_activation: None, trusted };
        let id = crate::policy::spawn(self, Shape::Plane { size }, Flags::WINDOW, payload, parent);
        // a new, unmapped toplevel must not steal focus from a mapped one
        if let Some(prev) = self.scene.iter().filter(|(i, m)| *i != id && m.m.mapped()).map(|(i, _)| i).last() {
            self.scene.focus(Some(prev));
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(id) = self.member_for_root(surface.wl_surface()) else {
            let focus = self.scene.focused;
            self.focus_window(focus);
            return;
        };
        let had_focus = self.scene.focused == Some(id);
        // a fullscreen member closing brings its hidden siblings back (wm §5)
        if matches!(self.policy.state(id).life, crate::policy::Life::Fullscreen(_)) {
            crate::policy::lifecycle::fullscreen(self, id, false);
        }
        crate::policy::seam::window_closed(self, id);
        crate::policy::removed(self, id);
        if let Some(member) = self.scene.remove(id) {
            if member.m.mapped() {
                self.journal.toplevels_unmapped += 1;
            }
            if let Some(panel) = member.m.panel {
                self.retire_panel(panel);
            }
        }
        // spatial-input §6 line 297: focus restore = the most recently committed still-mapped
        // member (the stack; cosmic `FocusStack::last`), not `Scene::remove`'s "last live member"
        if had_focus {
            focus::restore_after_close(self, id);
        } else {
            self.focus.stack.remove(id);
            let focus = self.scene.focused;
            self.focus_window(focus);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
        self.journal.popups += 1;
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    /// `set_maximized` / `unset_maximized`: the floor decides (wm §5 — the manager on request;
    /// with no external manager the floor is the manager).
    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.member_for_root(surface.wl_surface()) {
            // a connected manager decides (the request is re-emitted to it, wm §11); the floor
            // decides when none is
            if !crate::policy::seam::client_request(self, id, crate::policy::seam::ClientRequest::Maximize(true)) {
                crate::policy::lifecycle::maximize(self, id, true);
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.member_for_root(surface.wl_surface()) {
            if !crate::policy::seam::client_request(self, id, crate::policy::seam::ClientRequest::Maximize(false)) {
                crate::policy::lifecycle::maximize(self, id, false);
            }
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>) {
        if let Some(id) = self.member_for_root(surface.wl_surface()) {
            if !crate::policy::seam::client_request(self, id, crate::policy::seam::ClientRequest::Fullscreen(true)) {
                crate::policy::lifecycle::fullscreen(self, id, true);
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.member_for_root(surface.wl_surface()) {
            if !crate::policy::seam::client_request(self, id, crate::policy::seam::ClientRequest::Fullscreen(false)) {
                crate::policy::lifecycle::fullscreen(self, id, false);
            }
        }
    }

    /// `set_minimized`: never applied as a compositor state (wm §5); the floor's minimize verb
    /// decides — dock indicator when a dock client exists, else close (Q1 ruled) — or the
    /// connected manager's.
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.member_for_root(surface.wl_surface()) {
            if !crate::policy::seam::client_request(self, id, crate::policy::seam::ClientRequest::Minimize) {
                crate::policy::lifecycle::minimize(self, id);
            }
        }
    }

    /// A client's `xdg_toplevel.move`: the compositor's own grab (window-workspace-management §4a,
    /// research/76 convergence 5 — kwin-vr on KWin's move, wxrd/wxrc/river on the request). The
    /// `Grabs` stage takes the request on the next sample of a kind whose commit is held.
    fn move_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        if let Some(member) = self.member_for_root(surface.wl_surface()) {
            self.input.grab_request = Some(crate::input::grabs::GrabRequest { member, op: crate::input::grabs::Op::Move, serial, at_ns: now_ns() });
            self.journal.grab_requests += 1;
            // the manager hears of it too (wm §11); the grab is the compositor's either way
            crate::policy::seam::client_request(self, member, crate::policy::seam::ClientRequest::Move(serial));
        }
    }

    /// A client's `xdg_toplevel.resize(edges)`: the same grab, resizing by those edges.
    fn resize_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, serial: Serial, edges: xdg_toplevel::ResizeEdge) {
        use crate::input::grabs::{Edges, GrabRequest, Op};
        let Some(member) = self.member_for_root(surface.wl_surface()) else { return };
        let e = match edges {
            xdg_toplevel::ResizeEdge::Top => Edges { top: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::Bottom => Edges { bottom: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::Left => Edges { left: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::Right => Edges { right: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::TopLeft => Edges { top: true, left: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::TopRight => Edges { top: true, right: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::BottomLeft => Edges { bottom: true, left: true, ..Edges::default() },
            xdg_toplevel::ResizeEdge::BottomRight => Edges { bottom: true, right: true, ..Edges::default() },
            _ => Edges::default(),
        };
        if !e.any() {
            return;
        }
        let start_px = self.logical_size(member).unwrap_or((800, 600));
        self.input.grab_request = Some(GrabRequest { member, op: Op::Resize { edges: e, start_px }, serial, at_ns: now_ns() });
        self.journal.grab_requests += 1;
        let bits = (e.top as u32) | ((e.bottom as u32) << 1) | ((e.left as u32) << 2) | ((e.right as u32) << 3);
        crate::policy::seam::client_request(self, member, crate::policy::seam::ClientRequest::Resize(serial, bits));
    }
}

impl SeatHandler for Zxr {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Zxr> {
        &mut self.seat_state
    }
    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        // cursor-shape names and set_cursor surfaces alike; the seat stage takes it each tick
        // (spatial-input §7)
        self.input.cursor_image = Some(image);
    }
    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.dh;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

impl SelectionHandler for Zxr {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Zxr {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl DndGrabHandler for Zxr {}
impl WaylandDndGrabHandler for Zxr {
    fn dnd_requested<S: Source>(&mut self, source: S, _icon: Option<WlSurface>, seat: Seat<Self>, serial: Serial, type_: GrabType) {
        match type_ {
            GrabType::Pointer => {
                let ptr = seat.get_pointer().unwrap();
                if let Some(start_data) = ptr.grab_start_data() {
                    let grab = DnDGrab::new_pointer(&self.dh, start_data, source, seat);
                    ptr.set_grab(self, grab, serial, Focus::Keep);
                } else {
                    source.cancel();
                }
            }
            GrabType::Touch => source.cancel(),
        }
    }
}

impl OutputHandler for Zxr {}

// cursor-shape-v1 needs the tablet seat handler bound (smithay `cursor_shape.rs:252-258`); zxr
// serves no tablet tools yet.
impl smithay::input::tablet::TabletSeatHandler for Zxr {
    type ToolFocus = WlSurface;
}

// ADR 0007: serve `ext-idle-notify-v1`, honour `zwp_idle_inhibit_v1`; user activity reaches the
// notifier from `input::activity::notify` (KWin's spy shape, `input.cpp:3169-3172`).
impl smithay::wayland::idle_notify::IdleNotifierHandler for Zxr {
    fn idle_notifier_state(&mut self) -> &mut smithay::wayland::idle_notify::IdleNotifierState<Self> {
        &mut self.idle_notifier
    }
}

impl smithay::wayland::idle_inhibit::IdleInhibitHandler for Zxr {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.push(surface);
        self.idle_notifier.set_is_inhibited(true);
    }
    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|s| *s != surface);
        let on = !self.idle_inhibitors.is_empty();
        self.idle_notifier.set_is_inhibited(on);
    }
}
impl smithay::wayland::pointer_constraints::PointerConstraintsHandler for Zxr {}

/// spatial-input §6 lines 291-297 (`xdg_activation_v1`): the token's serial decides; refusal is
/// urgency-only. Ported from niri (`references/niri/src/handlers/mod.rs:761-838`): `token_created`
/// keeps every token (a token without a serial is urgency-only — niri's `UrgentOnlyMarker`,
/// `:766-773`; here the absent serial itself says so at request time); `request_activation`
/// validates against both devices' `last_enter` (`:790-802`), keeps the token on an unmapped
/// window (`:836-838`) and otherwise focuses or marks urgent. Tokens older than niri's
/// `XDG_ACTIVATION_TOKEN_TIMEOUT` (10 s, `:88`) are ignored — a stand-in from the comparable.
const ACTIVATION_TOKEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl XdgActivationHandler for Zxr {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation_state
    }

    fn token_created(&mut self, _token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        // the serial the client attached is the commit's (§6 line 291: "granted with the serial
        // of the commit that produced it"); nothing to record beyond what smithay keeps
        self.focus.tokens_created += 1;
        if data.serial.is_none() {
            self.focus.tokens_without_serial += 1;
        }
        true
    }

    fn request_activation(&mut self, _token: XdgActivationToken, token_data: XdgActivationTokenData, surface: WlSurface) {
        if token_data.timestamp.elapsed() >= ACTIVATION_TOKEN_TIMEOUT {
            self.focus.activations_expired += 1;
            return;
        }
        let serial = token_data.serial.as_ref().map(|(s, _)| *s);
        let Some(id) = self.member_for_root(&surface) else { return };
        let mapped = self.scene.get(id).map(|m| m.m.mapped()).unwrap_or(false);
        if mapped {
            focus::activate(self, id, serial);
        } else if let Some(m) = self.scene.get_mut(id) {
            m.m.pending_activation = Some(serial);
        }
    }
}

/// spatial-input §12: the input-method popup is a popup of the focused text-input's toplevel,
/// drawn on that member's plane (niri `references/niri/src/handlers/mod.rs:237-274` — the same
/// four hooks over its `PopupManager`).
impl InputMethodHandler for Zxr {
    fn new_popup(&mut self, surface: ImPopupSurface) {
        if let Err(e) = self.popups.track_popup(PopupKind::InputMethod(surface)) {
            tracing::warn!("input-method popup: {e:?}");
        }
        self.journal.popups += 1;
    }

    fn dismiss_popup(&mut self, surface: ImPopupSurface) {
        if let Some(parent) = surface.get_parent().map(|p| p.surface.clone()) {
            let _ = PopupManager::dismiss_popup(&parent, &PopupKind::from(surface));
        }
    }

    fn popup_repositioned(&mut self, _surface: ImPopupSurface) {}

    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, smithay::utils::Logical> {
        self.window_for_root(parent).map(|w| w.geometry()).unwrap_or_default()
    }
}

/// spatial-input §8 line 337: inhibitors are activated on creation (niri
/// `references/niri/src/handlers/mod.rs:281-287`, the confirmation dialog a FIXME there too) and
/// recorded per surface; `Zxr::shortcuts_inhibited` answers for the focused one.
impl KeyboardShortcutsInhibitHandler for Zxr {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        inhibitor.activate();
        self.text.add_inhibitor(inhibitor);
        tracing::info!(inhibitors = self.text.inhibitors.len(), "keyboard-shortcuts-inhibit: inhibitor activated");
    }

    fn inhibitor_destroyed(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        self.text.remove_inhibitor(&inhibitor);
    }
}

smithay::delegate_dispatch2!(Zxr);

/// Spawn a client with `WAYLAND_DISPLAY` (and `DISPLAY` if given) set.
pub fn spawn_client(cmd: &str, wayland_display: &OsString, x_display: Option<&str>) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c").arg(cmd).env("WAYLAND_DISPLAY", wayland_display).env_remove("WAYLAND_SOCKET");
    // SAFETY: async-signal-safe calls only; undoes the loop's signal block for the child.
    unsafe {
        c.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for s in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
                libc::sigaddset(&mut set, s);
            }
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
            Ok(())
        });
    }
    match x_display {
        Some(d) => {
            c.env("DISPLAY", d);
        }
        None => {
            c.env_remove("DISPLAY");
        }
    }
    c.spawn()
}

#[allow(dead_code)]
pub fn socket_pair() -> std::io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}
