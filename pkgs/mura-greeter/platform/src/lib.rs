//! Slint on smithay-client-toolkit for Mura's shell components (shell-plane.md §4 rev 0.3;
//! research/78 §9). One platform, two surface roles:
//!
//! - **layer-shell** (`zwlr_layer_surface_v1`) — the greeter in `zxr --greeter`, panels, the OSK;
//! - **session lock** (`ext_session_lock_surface_v1`) — the lock program on the public socket.
//!
//! Rendering is Slint's `SoftwareRenderer` into `wl_shm` buffers (two, swapped), presented on the
//! compositor's `wl_surface.frame` cadence: a redraw is requested by Slint, drawn once, and the
//! next only after the frame callback — a still scene costs nothing. Input is the seat's:
//! keyboard through xkbcommon (sctk), pointer, touch; text entry additionally through
//! `zwp_text_input_v3` so an on-screen keyboard's `commit_string` lands in the focused field
//! (squeekboard types this way, research/75 §3.2). Accessibility is `mura-slint-accesskit`, the
//! translation extracted from Slint's winit backend, on `accesskit_unix` (feature `accessibility`).
//!
//! Why not Slint's winit backend: it creates xdg toplevels only; neither role exists there
//! (research/78 §9; the route libcosmic/iced took for the same reason). Slint's own non-winit
//! `linuxkms` backend is the template for the adapter (`references/slint/internal/backends/linuxkms/`).
//!
//! Budget: one thread, one Wayland connection, one calloop loop; no bus, no GPU; the scene is
//! redrawn only on Slint's request and only the dirty region is rendered.

mod adapter;
#[cfg(feature = "input-method")]
pub mod input_method;
mod pixel;
mod state;
mod text_input;

#[cfg(feature = "input-method")]
pub use input_method::ImEvent;

/// What the program asks the input method to do (feature `input-method`; the OSK's two verbs).
#[cfg(feature = "input-method")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImAction {
    /// `zwp_input_method_v2.commit_string` + `commit`: text into the focused field
    CommitString(String),
    /// one press-and-release of an evdev key through `zwp_virtual_keyboard_v1`
    Key(u32),
}

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use slint::platform::{Platform, PlatformError};
use wayland_client::globals::registry_queue_init;
use wayland_client::Connection;

pub use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

/// The surface role the platform gives Slint's window.
#[derive(Clone, Debug)]
pub enum Role {
    /// A `zwlr_layer_surface_v1` with these properties (the greeter: `Overlay`, all anchors,
    /// `Exclusive`, zone −1 — cosmic-greeter's and gtkgreet `-l`'s).
    Layer { layer: Layer, anchor: Anchor, exclusive_zone: i32, keyboard: KeyboardInteractivity, namespace: String, size: (u32, u32) },
    /// `ext_session_lock_v1`: the window is the lock surface of the first output; [`Handle::lock`]
    /// starts the lock, [`Handle::unlock`] ends it.
    SessionLock,
}

/// What the platform is told at start.
#[derive(Clone, Debug)]
pub struct Config {
    pub role: Role,
    /// Log/trace name only (layer-shell has the namespace; lock surfaces have none).
    pub app_id: String,
}

/// Lock-mode events the program acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockEvent {
    /// The compositor sent `locked`: the session is locked (I2 — the program reports `SetLockedHint`).
    Locked,
    /// The compositor sent `finished`: another locker holds the lock, or it refused.
    Finished,
}

/// The program's handle to the platform: lock control and event delivery.
#[derive(Clone)]
pub struct Handle {
    shared: Rc<RefCell<state::Shared>>,
}

impl Handle {
    /// Session-lock role: request the lock (`ext_session_lock_manager_v1.lock`).
    pub fn lock(&self) {
        self.shared.borrow_mut().lock_requests.push(state::LockRequest::Lock);
    }
    /// Session-lock role: `unlock_and_destroy` — the program's word, after its own PAM success.
    pub fn unlock(&self) {
        self.shared.borrow_mut().lock_requests.push(state::LockRequest::Unlock);
    }
    /// Lock events since the last call.
    pub fn take_lock_events(&self) -> Vec<LockEvent> {
        std::mem::take(&mut self.shared.borrow_mut().lock_events)
    }
    /// Register a callback for lock events (called on the Slint thread).
    pub fn on_lock_event(&self, f: impl Fn(LockEvent) + 'static) {
        self.shared.borrow_mut().lock_callback = Some(Box::new(f));
    }
    /// Whether the compositor has the lock (`locked` received and not yet unlocked).
    pub fn is_locked(&self) -> bool {
        self.shared.borrow().locked
    }
    /// Layer role: map or unmap the surface (an on-screen keyboard's show/hide).
    pub fn set_visible(&self, visible: bool) {
        self.shared.borrow_mut().visible_request = Some(visible);
    }
    /// Whether the surface is mapped (or about to be).
    pub fn is_visible(&self) -> bool {
        self.shared.borrow().mapped
    }
    /// Feature `input-method`: register the callback for the input method's events (on the Slint thread).
    #[cfg(feature = "input-method")]
    pub fn on_im_event(&self, f: impl Fn(ImEvent) + 'static) {
        self.shared.borrow_mut().im_callback = Some(Box::new(f));
    }
    /// Feature `input-method`: type text into the focused field.
    #[cfg(feature = "input-method")]
    pub fn commit_string(&self, text: &str) {
        self.shared.borrow_mut().im_actions.push(ImAction::CommitString(text.to_string()));
    }
    /// Feature `input-method`: one evdev key press-and-release (`input_method::key`).
    #[cfg(feature = "input-method")]
    pub fn key(&self, keycode: u32) {
        self.shared.borrow_mut().im_actions.push(ImAction::Key(keycode));
    }
}

/// Install the platform. Call before creating any Slint component; the Wayland connection is
/// `WAYLAND_DISPLAY`/`WAYLAND_SOCKET` (libwayland's rule — the socketpair child needs no name).
pub fn init(config: Config) -> Result<Handle, PlatformError> {
    let conn = Connection::connect_to_env().map_err(|e| PlatformError::Other(format!("wayland: {e}")))?;
    let (globals, event_queue) = registry_queue_init::<state::AppState>(&conn).map_err(|e| PlatformError::Other(format!("registry: {e}")))?;
    let qh = event_queue.handle();
    let event_loop: EventLoop<'static, state::AppState> = EventLoop::try_new().map_err(|e| PlatformError::Other(format!("calloop: {e}")))?;
    let shared = Rc::new(RefCell::new(state::Shared::default()));
    let app = state::AppState::new(&conn, &globals, &qh, &config, shared.clone(), event_loop.handle())?;
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle()).map_err(|e| PlatformError::Other(format!("wayland source: {e}")))?;
    let proxy = app.proxy();
    let platform = MuraPlatform { inner: RefCell::new(Some((event_loop, app))), qh, shared: shared.clone(), conn, config, proxy };
    slint::platform::set_platform(Box::new(platform)).map_err(|e| PlatformError::Other(format!("set_platform: {e:?}")))?;
    Ok(Handle { shared })
}

struct MuraPlatform {
    inner: RefCell<Option<(EventLoop<'static, state::AppState>, state::AppState)>>,
    qh: wayland_client::QueueHandle<state::AppState>,
    shared: Rc<RefCell<state::Shared>>,
    conn: Connection,
    config: Config,
    proxy: state::Proxy,
}

impl Platform for MuraPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, PlatformError> {
        let adapter = adapter::MuraWindowAdapter::new(self.shared.clone());
        self.shared.borrow_mut().adapter = Some(adapter.clone());
        Ok(adapter)
    }

    fn run_event_loop(&self) -> Result<(), PlatformError> {
        let Some((mut event_loop, mut app)) = self.inner.borrow_mut().take() else {
            return Err(PlatformError::Other("event loop already running".into()));
        };
        tracing::info!(app = %self.config.app_id, role = ?self.config.role, "mura-slint-platform: running");
        app.start(&self.qh);
        loop {
            slint::platform::update_timers_and_animations();
            app.process(&self.qh);
            // a dead connection (the compositor gone, or a protocol error it posted) ends the
            // program: a readable-at-EOF fd would otherwise spin the loop forever
            match self.conn.flush() {
                Ok(()) => {}
                Err(wayland_client::backend::WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(PlatformError::Other(format!("wayland connection: {e}"))),
            }
            if let Some(e) = self.conn.protocol_error() {
                return Err(PlatformError::Other(format!("wayland protocol error: {} on {}@{}: {}", e.code, e.object_interface, e.object_id, e.message)));
            }
            if self.proxy.quit_requested() {
                break;
            }
            let timeout = if app.needs_tick() { Some(Duration::ZERO) } else { slint::platform::duration_until_next_timer_update() };
            event_loop.dispatch(timeout, &mut app).map_err(|e| PlatformError::Other(format!("dispatch: {e}")))?;
        }
        app.shutdown();
        let _ = self.conn.flush();
        Ok(())
    }

    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> {
        Some(Box::new(self.proxy.clone()))
    }

    fn duration_since_start(&self) -> Duration {
        self.shared.borrow().start.elapsed()
    }
}
