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
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::utils::{Rectangle, Serial, Transform, SERIAL_COUNTER};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
};
use smithay::wayland::dmabuf::{get_dmabuf, DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::drm_syncobj::{supports_syncobj_eventfd, DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState};
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::selection::data_device::{set_data_device_focus, DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState, XdgToplevelSurfaceData};
use smithay::wayland::shm::{with_buffer_contents, ShmHandler, ShmState};
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::viewporter::ViewporterState;

use crate::journal::Journal;
use crate::render::{Renderer, Texture};
use crate::scene::{self, Flags, MemberId, Scene, Shape, M_PER_PX};
use crate::xr::math;
use crate::xr::XrCore;

/// One client's compositor bookkeeping (smithay's `ClientData`).
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}
impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

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
    pub popups: PopupManager,
    pub seat: Seat<Zxr>,
    pub output: Output,

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
    pub children: Vec<Child>,
    /// xwayland-satellite's pid when spawned: its toplevels are the X11 ones (gate 4)
    pub satellite_pid: Option<u32>,
    pub pointer_focus: Option<WlSurface>,
    pub last_head_pose: Option<openxr::Posef>,
    /// research/63 Phase 1b: how panels reach the runtime (projection pass / quad layers / both)
    /// `--debug-panels`: force every plane into the projection layer (the R0 path) for measurement
    pub debug_panels: DebugPanels,
    /// native-openxr-apps.md §4 quiet mode: a native app is primary — submit no layers, run no
    /// panel or projection pass, hold no client buffers; planes get fallback callbacks only.
    /// Set by the primary-client observer (libmonado, M1); the control socket toggles it for
    /// measurement.
    pub quiet: bool,
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

/// The frontend's member payload (spec §5a `M`): the smithay window, its panel, and the
/// per-member state the tick reads. `scene` never sees these types.
pub struct Payload {
    pub window: Window,
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
        self.window.toplevel().map(|t| t.wl_surface().clone())
    }
}

/// The plane extents in metres of a window's current geometry (`M_PER_PX`).
pub fn plane_size_of(window: &Window) -> [f32; 2] {
    let g = window.geometry().size;
    [g.w.max(1) as f32 * M_PER_PX, g.h.max(1) as f32 * M_PER_PX]
}

impl Zxr {
    pub fn new(
        display: Display<Zxr>,
        loop_handle: LoopHandle<'static, Zxr>,
        loop_signal: LoopSignal,
        xr: XrCore,
        renderer: Renderer,
        socket: Option<&str>,
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
        let popups = PopupManager::default();

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
        seat.add_keyboard(Default::default(), 200, 25).map_err(|e| e.to_string())?;
        seat.add_pointer();

        // ---- the one wl_output: a virtual panel (§10 notes; sized to the view)
        let output = Output::new("XR-1".into(), PhysicalProperties { size: (600, 340).into(), subpixel: Subpixel::Unknown, make: "Mura".into(), model: "zxr virtual".into(), serial_number: "0".into() });
        let _global = output.create_global::<Self>(&dh);
        let mode = Mode { size: (1920, 1080).into(), refresh: 60_000 };
        output.change_current_state(Some(mode), Some(Transform::Normal), Some(Scale::Integer(1)), Some((0, 0).into()));
        output.set_preferred(mode);

        // ---- listening socket + display source
        let listening = match socket {
            Some(name) => ListeningSocketSource::with_name(name),
            None => ListeningSocketSource::new_auto(),
        }
        .map_err(|e| format!("wayland socket: {e}"))?;
        let socket_name = listening.socket_name().to_os_string();
        loop_handle
            .insert_source(listening, move |stream, _, state| {
                if let Err(e) = state.dh.insert_client(stream, Arc::new(ClientState::default())) {
                    tracing::warn!("insert_client: {e}");
                }
            })
            .map_err(|e| e.to_string())?;
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
            dh,
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
            popups,
            seat,
            output,
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
            children: Vec::new(),
            satellite_pid: None,
            pointer_focus: None,
            last_head_pose: None,
            debug_panels: DebugPanels::default(),
            quiet: false,
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
        use smithay::backend::input::KeyState;
        use smithay::input::keyboard::{FilterResult, Keycode};
        let kb = self.seat.get_keyboard().unwrap();
        let serial = SERIAL_COUNTER.next_serial();
        let time = smithay::backend::input::InputTime::now();
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        kb.input::<(), _>(self, Keycode::new(evdev + 8), state, serial, time, |_, _, _| FilterResult::Forward);
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
        self.scene.find(|p| p.window.toplevel().map(|t| t.wl_surface() == root).unwrap_or(false))
    }

    pub fn window_for_root(&self, root: &WlSurface) -> Option<&Window> {
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
        for (i, m) in self.scene.iter() {
            if let Some(t) = m.m.window.toplevel() {
                t.with_pending_state(|s| {
                    if Some(i) == id {
                        s.states.set(xdg_toplevel::State::Activated);
                    } else {
                        s.states.unset(xdg_toplevel::State::Activated);
                    }
                });
                t.send_pending_configure();
            }
        }
        let surf = self.scene.focused().and_then(|m| m.m.root());
        self.set_keyboard_focus(surf);
    }

    /// Spec §5a hit test, then smithay's 2D hit within the plane: the member, the surface under
    /// the ray and the surface-local point.
    pub fn hit_surface(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<(MemberId, WlSurface, smithay::utils::Point<f64, smithay::utils::Logical>)> {
        let (id, local, _) = self.scene.hit(origin, dir, |p| p.mapped())?;
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
                pointer.motion(self, Some((surface.clone(), loc)), &smithay::input::pointer::MotionEvent { location: loc, serial, time });
                pointer.frame(self);
                self.pointer_focus = Some(surface);
            }
            None => {
                if self.pointer_focus.take().is_some() {
                    pointer.motion(self, None, &smithay::input::pointer::MotionEvent { location: (0.0, 0.0).into(), serial, time });
                    pointer.frame(self);
                }
            }
        }
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else { return };
        let Some(window) = self.window_for_root(&root) else { return };
        let geo = window.geometry();
        let mut target = Rectangle::new((0, 0).into(), (1920, 1080).into());
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
                    let size = plane_size_of(&window);
                    self.scene.set_shape(id, Shape::Plane { size });
                    if newly_mapped {
                        if let Some(m) = self.scene.get_mut(id) {
                            m.m.mapped_at = self.frame_id.max(1);
                        }
                        self.journal.toplevels_mapped += 1;
                        if self.client_is_satellite(surface) {
                            self.journal.xwayland_toplevels += 1;
                        }
                        self.focus_window(Some(id));
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
        let window = Window::new_wayland_window(surface);
        // mapped_at 0 = not yet mapped; commit() flips it on the first buffer. Placement is the
        // stand-in fan in the default place (spec §5a) until M1's `free` engine.
        let size = plane_size_of(&window);
        let payload = Payload { window, panel: None, dirty: false, mapped_at: 0, last_frame_callback: 0, hidden: false };
        let id = self.scene.add_fanned(Shape::Plane { size }, Flags::WINDOW, payload);
        // a new, unmapped toplevel must not steal focus from a mapped one
        if let Some(prev) = self.scene.iter().filter(|(i, m)| *i != id && m.m.mapped()).map(|(i, _)| i).last() {
            self.scene.focus(Some(prev));
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.member_for_root(surface.wl_surface()) {
            if let Some(member) = self.scene.remove(id) {
                if member.m.mapped() {
                    self.journal.toplevels_unmapped += 1;
                }
                if let Some(panel) = member.m.panel {
                    self.retire_panel(panel);
                }
            }
        }
        let focus = self.scene.focused;
        self.focus_window(focus);
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

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // R0: planes move through the control socket; interactive grabs arrive with hands (M1)
    }
}

impl SeatHandler for Zxr {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Zxr> {
        &mut self.seat_state
    }
    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
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
impl smithay::wayland::pointer_constraints::PointerConstraintsHandler for Zxr {}

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
