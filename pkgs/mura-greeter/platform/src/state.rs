//! The sctk application state: one surface in one of two roles, `wl_shm` presentation on the
//! frame-callback cadence, the seat's keyboard/pointer/touch into Slint's `WindowEvent`s, and
//! `zwp_text_input_v3` for the on-screen keyboard (`text_input.rs`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use calloop::ping::{make_ping, Ping};
use calloop::LoopHandle;
use i_slint_core::window::InputMethodRequest;
use slint::platform::{PlatformError, PointerEventButton, WindowAdapter as _, WindowEvent};
use slint::{LogicalPosition, PhysicalSize, SharedString};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::touch::TouchHandler;
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::session_lock::{SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure};
use smithay_client_toolkit::shell::wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_session_lock,
    delegate_shm, delegate_touch, registry_handlers,
};
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface, wl_touch};
use wayland_client::{Connection, QueueHandle};

use crate::adapter::MuraWindowAdapter;
#[cfg(feature = "input-method")]
use crate::input_method::InputMethod;
use crate::text_input::TextInput;
use crate::{Config, LockEvent, Role};

pub(crate) enum LockRequest {
    Lock,
    Unlock,
}

/// State the adapter and the sctk handlers both touch.
pub(crate) struct Shared {
    pub adapter: Option<Rc<MuraWindowAdapter>>,
    pub im_requests: Vec<InputMethodRequest>,
    pub visible: bool,
    pub keyboard_focus: bool,
    pub lock_requests: Vec<LockRequest>,
    pub lock_events: Vec<LockEvent>,
    pub lock_callback: Option<Box<dyn Fn(LockEvent)>>,
    pub locked: bool,
    pub start: Instant,
    /// `Handle::set_visible`: a layer surface unmapped (null buffer) and remapped on demand
    pub visible_request: Option<bool>,
    /// the layer surface is mapped, or will be at the next configure (`Handle::is_visible`)
    pub mapped: bool,
    #[cfg(feature = "input-method")]
    pub im_callback: Option<Box<dyn Fn(crate::ImEvent)>>,
    #[cfg(feature = "input-method")]
    pub im_actions: Vec<crate::ImAction>,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            adapter: None,
            im_requests: Vec::new(),
            visible: true,
            keyboard_focus: false,
            lock_requests: Vec::new(),
            lock_events: Vec::new(),
            lock_callback: None,
            locked: false,
            start: Instant::now(),
            visible_request: None,
            mapped: true,
            #[cfg(feature = "input-method")]
            im_callback: None,
            #[cfg(feature = "input-method")]
            im_actions: Vec::new(),
        }
    }
}

/// `slint::invoke_from_event_loop` / `quit_event_loop` from any thread: queue + calloop ping.
#[derive(Clone)]
pub(crate) struct Proxy {
    queue: Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>,
    quit: Arc<AtomicBool>,
    ping: Ping,
}

impl Proxy {
    pub fn quit_requested(&self) -> bool {
        self.quit.load(Ordering::Relaxed)
    }
    /// A `Send + Sync` waker for other threads (AccessKit's).
    pub fn waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let ping = self.ping.clone();
        Arc::new(move || ping.ping())
    }
}

impl slint::platform::EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.quit.store(true, Ordering::Relaxed);
        self.ping.ping();
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: Box<dyn FnOnce() + Send>) -> Result<(), slint::EventLoopError> {
        self.queue.lock().unwrap().push(event);
        self.ping.ping();
        Ok(())
    }
}

enum Surface {
    Layer(LayerSurface),
    Lock(SessionLockSurface),
    None,
}

impl Surface {
    fn wl_surface(&self) -> Option<&wl_surface::WlSurface> {
        match self {
            Surface::Layer(l) => Some(l.wl_surface()),
            Surface::Lock(l) => Some(l.wl_surface()),
            Surface::None => None,
        }
    }
}

pub(crate) struct AppState {
    registry_state: RegistryState,
    compositor: CompositorState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    layer_shell: Option<LayerShell>,
    session_lock_state: SessionLockState,
    session_lock: Option<SessionLock>,
    surface: Surface,
    pool: SlotPool,
    buffers: [Option<Buffer>; 2],
    buffer_index: usize,
    configured: Option<(u32, u32)>,
    scale: i32,
    frame_pending: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    touch: Option<wl_touch::WlTouch>,
    touch_points: Vec<(i32, (f64, f64))>,
    modifiers: Modifiers,
    pub(crate) text_input: TextInput,
    #[cfg(feature = "input-method")]
    pub(crate) input_method: InputMethod,
    /// the layer surface is mapped (or will be at the next configure); `false` after `set_visible(false)`
    mapped: bool,
    shared: Rc<RefCell<Shared>>,
    proxy: Proxy,
    config: Config,
    loop_handle: LoopHandle<'static, AppState>,
    pending_events: Vec<WindowEvent>,
    closed: bool,
    first_frame: bool,
}

impl AppState {
    pub fn new(
        _conn: &Connection,
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        config: &Config,
        shared: Rc<RefCell<Shared>>,
        loop_handle: LoopHandle<'static, AppState>,
    ) -> Result<Self, PlatformError> {
        let compositor = CompositorState::bind(globals, qh).map_err(|e| PlatformError::Other(format!("wl_compositor: {e}")))?;
        let shm = Shm::bind(globals, qh).map_err(|e| PlatformError::Other(format!("wl_shm: {e}")))?;
        let pool = SlotPool::new(4 * 1024 * 1024, &shm).map_err(|e| PlatformError::Other(format!("shm pool: {e}")))?;
        let layer_shell = LayerShell::bind(globals, qh).ok();
        let (ping, ping_source) = make_ping().map_err(|e| PlatformError::Other(format!("ping: {e}")))?;
        loop_handle.insert_source(ping_source, |_, _, _| {}).map_err(|e| PlatformError::Other(format!("ping source: {e}")))?;
        let proxy = Proxy { queue: Default::default(), quit: Default::default(), ping };
        let text_input = TextInput::bind(globals, qh);
        #[cfg(feature = "input-method")]
        let input_method = InputMethod::bind(globals, qh);
        Ok(AppState {
            registry_state: RegistryState::new(globals),
            compositor,
            output_state: OutputState::new(globals, qh),
            seat_state: SeatState::new(globals, qh),
            shm,
            layer_shell,
            session_lock_state: SessionLockState::new(globals, qh),
            session_lock: None,
            surface: Surface::None,
            pool,
            buffers: [None, None],
            buffer_index: 0,
            configured: None,
            scale: 1,
            frame_pending: false,
            keyboard: None,
            pointer: None,
            touch: None,
            touch_points: Vec::new(),
            modifiers: Modifiers::default(),
            text_input,
            #[cfg(feature = "input-method")]
            input_method,
            mapped: true,
            shared,
            proxy,
            config: config.clone(),
            loop_handle,
            pending_events: Vec::new(),
            closed: false,
            first_frame: false,
        })
    }

    pub fn proxy(&self) -> Proxy {
        self.proxy.clone()
    }

    /// Create the role surface. Layer: now. Lock: on `Handle::lock()`.
    pub fn start(&mut self, qh: &QueueHandle<Self>) {
        #[cfg(feature = "accessibility")]
        if let Some(adapter) = self.shared.borrow().adapter.clone() {
            adapter.init_accesskit(self.proxy.waker());
        }
        if let Role::Layer { layer, anchor, exclusive_zone, keyboard, namespace, size } = &self.config.role {
            let Some(shell) = &self.layer_shell else {
                tracing::error!("zwlr_layer_shell_v1 not offered; the layer role cannot map");
                self.closed = true;
                return;
            };
            let wl = self.compositor.create_surface(qh);
            let surface = shell.create_layer_surface(qh, wl, *layer, Some(namespace.clone()), None);
            surface.set_anchor(*anchor);
            surface.set_exclusive_zone(*exclusive_zone);
            surface.set_keyboard_interactivity(*keyboard);
            surface.set_size(size.0, size.1);
            surface.commit();
            self.surface = Surface::Layer(surface);
        }
    }

    pub fn needs_tick(&self) -> bool {
        !self.pending_events.is_empty()
    }

    /// One pass of the platform's own work; called each loop iteration after dispatch.
    pub fn process(&mut self, qh: &QueueHandle<Self>) {
        for f in std::mem::take(&mut *self.proxy.queue.lock().unwrap()) {
            f();
        }
        let adapter = self.shared.borrow().adapter.clone();
        let Some(adapter) = adapter else { return };

        #[cfg(feature = "accessibility")]
        adapter.pump_accesskit();

        for ev in std::mem::take(&mut self.pending_events) {
            adapter.window().dispatch_event(ev);
        }

        let requests = std::mem::take(&mut self.shared.borrow_mut().lock_requests);
        for r in requests {
            match r {
                LockRequest::Lock => self.lock(qh),
                LockRequest::Unlock => self.unlock(),
            }
        }
        let events = std::mem::take(&mut self.shared.borrow_mut().lock_events);
        if !events.is_empty() {
            let cb = self.shared.borrow_mut().lock_callback.take();
            if let Some(cb) = cb {
                for e in &events {
                    cb(*e);
                }
                let mut s = self.shared.borrow_mut();
                if s.lock_callback.is_none() {
                    s.lock_callback = Some(cb);
                }
            } else {
                self.shared.borrow_mut().lock_events = events;
            }
        }

        let im = std::mem::take(&mut self.shared.borrow_mut().im_requests);
        for r in im {
            self.text_input.handle_request(r, self.scale);
        }
        let te = self.text_input.take_events();
        for ev in te {
            adapter.window().dispatch_event(ev);
        }

        #[cfg(feature = "input-method")]
        {
            let actions = std::mem::take(&mut self.shared.borrow_mut().im_actions);
            for a in actions {
                match a {
                    crate::ImAction::CommitString(text) => self.input_method.commit_string(&text),
                    crate::ImAction::Key(code) => self.input_method.key(code),
                }
            }
            let events = self.input_method.take_events();
            if !events.is_empty() {
                let cb = self.shared.borrow_mut().im_callback.take();
                if let Some(cb) = cb {
                    for e in events {
                        cb(e);
                    }
                    let mut s = self.shared.borrow_mut();
                    if s.im_callback.is_none() {
                        s.im_callback = Some(cb);
                    }
                }
            }
        }

        let visible_request = self.shared.borrow_mut().visible_request.take();
        if let Some(v) = visible_request {
            self.set_visible(v);
        }

        if self.closed {
            let _ = slint::quit_event_loop();
            return;
        }

        if self.mapped && self.configured.is_some() && !self.frame_pending && adapter.take_redraw_request() {
            self.draw(qh, &adapter);
        }
    }

    /// Map or unmap the layer surface (the OSK's show/hide; squeekboard hides its panel the same
    /// way). Hide: a null buffer commit — the protocol's unmap, after which the compositor treats
    /// the next commit as the initial one again (`zwlr_layer_shell_v1.xml` `configure`/unmap;
    /// smithay resets the role on unmap). Show: that initial commit, whose configure redraws.
    fn set_visible(&mut self, visible: bool) {
        if visible == self.mapped {
            return;
        }
        let Surface::Layer(layer) = &self.surface else { return };
        let layer = layer.clone();
        self.mapped = visible;
        if visible {
            // unmapping reset the layer state to what `get_layer_surface` left (the protocol;
            // smithay `wlr_layer/mod.rs` resets the pending state), so the role's properties are
            // sent again before the initial commit — a width of 0 without left|right anchors is a
            // protocol error otherwise
            if let Role::Layer { layer: l, anchor, exclusive_zone, keyboard, size, .. } = &self.config.role {
                layer.set_layer(*l);
                layer.set_anchor(*anchor);
                layer.set_exclusive_zone(*exclusive_zone);
                layer.set_keyboard_interactivity(*keyboard);
                layer.set_size(size.0, size.1);
            }
            layer.commit();
            if let Some(adapter) = self.shared.borrow().adapter.clone() {
                adapter.window().request_redraw();
            }
        } else {
            let wl = layer.wl_surface();
            wl.attach(None, 0, 0);
            wl.commit();
            self.frame_pending = false;
            self.buffers = [None, None];
            // no buffer before the next configure (the protocol's rule for the initial commit)
            self.configured = None;
        }
        self.shared.borrow_mut().mapped = visible;
    }

    pub fn shutdown(&mut self) {
        self.surface = Surface::None;
        self.session_lock = None;
    }

    fn lock(&mut self, qh: &QueueHandle<Self>) {
        if self.session_lock.is_some() {
            return;
        }
        match self.session_lock_state.lock(qh) {
            Ok(lock) => {
                // One lock surface per output; Slint's window is the first output's (I1: zxr has
                // one output — the HMD). A second output's surface is a plain black buffer.
                let outputs: Vec<_> = self.output_state.outputs().collect();
                for (i, output) in outputs.iter().enumerate() {
                    let wl = self.compositor.create_surface(qh);
                    let ls = lock.create_lock_surface(wl, output, qh);
                    if i == 0 {
                        self.surface = Surface::Lock(ls);
                    } else {
                        std::mem::forget(ls);
                    }
                }
                if outputs.is_empty() {
                    tracing::warn!("no outputs yet; lock surface created when one appears");
                }
                self.session_lock = Some(lock);
            }
            Err(e) => {
                tracing::error!("ext_session_lock_v1 unavailable: {e}");
                self.shared.borrow_mut().lock_events.push(LockEvent::Finished);
            }
        }
    }

    fn unlock(&mut self) {
        if let Some(lock) = self.session_lock.take() {
            lock.unlock();
            self.drop_surface();
            self.shared.borrow_mut().locked = false;
        }
    }

    /// The surface is gone (unlock, `finished`): nothing of the old one may gate the next — a
    /// frame callback that will never arrive would otherwise hold `frame_pending` and the next
    /// lock surface would never draw (found at G3: the session's second lock stayed blank).
    fn drop_surface(&mut self) {
        self.surface = Surface::None;
        self.configured = None;
        self.frame_pending = false;
        self.buffers = [None, None];
    }

    fn draw(&mut self, qh: &QueueHandle<Self>, adapter: &Rc<MuraWindowAdapter>) {
        let Some((w, h)) = self.configured else { return };
        let Some(surface) = self.surface.wl_surface().cloned() else { return };
        let (pw, ph) = (w * self.scale as u32, h * self.scale as u32);
        let stride = pw as i32 * 4;
        let idx = self.buffer_index;
        let need_new = match &self.buffers[idx] {
            Some(b) => {
                let (bw, bh) = self.pool.canvas(b).map(|c| (c.len() as u32 / 4 / ph.max(1), ph)).unwrap_or((0, 0));
                bw != pw || bh != ph || b.slot().has_active_buffers()
            }
            None => true,
        };
        if need_new {
            match self.pool.create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888) {
                Ok((b, _)) => self.buffers[idx] = Some(b),
                Err(e) => {
                    tracing::error!("shm buffer: {e}");
                    return;
                }
            }
        }
        let buffer = self.buffers[idx].as_ref().unwrap();
        let canvas = self.pool.canvas(buffer).expect("canvas");
        let pixels = crate::pixel::as_pixels(canvas);
        let region = adapter.software_renderer().render(pixels, pw as usize);
        for (pos, size) in region.iter() {
            surface.damage_buffer(pos.x, pos.y, size.width as i32, size.height as i32);
        }
        surface.frame(qh, surface.clone());
        if let Err(e) = buffer.attach_to(&surface) {
            tracing::error!("attach: {e}");
        }
        surface.commit();
        if !self.first_frame {
            self.first_frame = true;
            tracing::info!(w = pw, h = ph, "first frame committed");
        }
        self.frame_pending = true;
        self.buffer_index ^= 1;
    }

    fn configure_size(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        self.configured = Some((w, h));
        if let Some(adapter) = self.shared.borrow().adapter.clone() {
            adapter.set_physical_size(PhysicalSize::new(w * self.scale as u32, h * self.scale as u32));
            // a configure is a fresh surface (the first, a remap, a second lock): its buffer must be
            // drawn even when the size did not change — `set_physical_size` asks for a redraw only
            // on a size change (found at G3: the second lock of a session never mapped, the
            // adapter still held the first lock's 1920×1493, so nothing was drawn or focused)
            adapter.window().request_redraw();
        }
    }

    fn set_scale(&mut self, scale: i32) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        if let Some(s) = self.surface.wl_surface() {
            s.set_buffer_scale(scale);
        }
        self.pending_events.push(WindowEvent::ScaleFactorChanged { scale_factor: scale as f32 });
        if let Some((w, h)) = self.configured {
            self.configure_size(w, h);
        }
    }

    pub(crate) fn our_surface(&self) -> Option<&wl_surface::WlSurface> {
        self.surface.wl_surface()
    }

    fn is_ours(&self, surface: &wl_surface::WlSurface) -> bool {
        self.surface.wl_surface() == Some(surface)
    }

    fn push(&mut self, ev: WindowEvent) {
        self.pending_events.push(ev);
    }

    fn key_text(event: &KeyEvent) -> Option<SharedString> {
        map_key_sym(event.keysym).or_else(|| event.utf8.as_deref().map(SharedString::from))
    }
}

/// Keysym → Slint key text, via Slint's own key table (`i_slint_common::for_each_keys!`, the
/// mapping its linuxkms backend uses: `internal/backends/linuxkms/calloop_backend/input.rs:419`).
fn map_key_sym(sym: Keysym) -> Option<SharedString> {
    mod xkb {
        pub use smithay_client_toolkit::seat::keyboard::Keysym;
    }
    macro_rules! keysym_to_string {
        ($($char:literal # $name:ident # $($shifted:ident)? $(=> $($_muda:ident)? # $($_qt:ident)|* # $($_winit:ident $(($_pos:ident))?)|* # $($xkb:ident)|* )? ;)*) => {
            match sym {
                $($($(xkb::Keysym::$xkb => $char,)*)?)*
                _ => sym.key_char()?,
            }
        };
    }
    let char = i_slint_common::for_each_keys!(keysym_to_string);
    Some(char.into())
}

fn button(code: u32) -> PointerEventButton {
    match code {
        0x110 => PointerEventButton::Left,
        0x111 => PointerEventButton::Right,
        0x112 => PointerEventButton::Middle,
        0x113 => PointerEventButton::Back,
        0x114 => PointerEventButton::Forward,
        _ => PointerEventButton::Other,
    }
}

impl CompositorHandler for AppState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, new_factor: i32) {
        if self.is_ours(surface) {
            self.set_scale(new_factor);
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _time: u32) {
        if self.is_ours(surface) {
            self.frame_pending = false;
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        // A lock held before any output was known: give the new output its surface.
        if let (Some(lock), Surface::None) = (&self.session_lock, &self.surface) {
            let wl = self.compositor.create_surface(qh);
            self.surface = Surface::Lock(lock.create_lock_surface(wl, &output, qh));
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for AppState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        tracing::info!("layer surface closed by the compositor");
        self.closed = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _serial: u32) {
        let (w, h) = configure.new_size;
        let fallback = if let Role::Layer { size, .. } = &self.config.role { *size } else { (1, 1) };
        self.configure_size(if w == 0 { fallback.0 } else { w }, if h == 0 { fallback.1 } else { h });
    }
}

impl SessionLockHandler for AppState {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        tracing::info!("ext_session_lock_v1: locked");
        let mut s = self.shared.borrow_mut();
        s.locked = true;
        s.lock_events.push(LockEvent::Locked);
    }
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        tracing::warn!("ext_session_lock_v1: finished (lock refused or lost)");
        self.session_lock = None;
        self.drop_surface();
        let mut s = self.shared.borrow_mut();
        s.locked = false;
        s.lock_events.push(LockEvent::Finished);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLockSurface, configure: SessionLockSurfaceConfigure, _serial: u32) {
        let (w, h) = configure.new_size;
        self.configure_size(w, h);
    }
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                let lh = self.loop_handle.clone();
                match self.seat_state.get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    lh,
                    Box::new(|state: &mut AppState, _kbd, event: KeyEvent| {
                        if let Some(text) = AppState::key_text(&event) {
                            state.push(WindowEvent::KeyPressRepeated { text });
                        }
                    }),
                ) {
                    Ok(k) => self.keyboard = Some(k),
                    Err(e) => tracing::error!("keyboard: {e}"),
                }
                self.text_input.seat_ready(qh, &seat);
                #[cfg(feature = "input-method")]
                self.input_method.seat_ready(qh, &seat);
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
            }
            Capability::Touch if self.touch.is_none() => {
                self.touch = self.seat_state.get_touch(qh, &seat).ok();
            }
            _ => {}
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
            }
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.release();
                }
            }
            Capability::Touch => {
                if let Some(t) = self.touch.take() {
                    t.release();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for AppState {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        if self.is_ours(surface) {
            self.shared.borrow_mut().keyboard_focus = true;
            self.push(WindowEvent::WindowActiveChanged(true));
            #[cfg(feature = "accessibility")]
            if let Some(a) = self.shared.borrow().adapter.clone() {
                a.accesskit_window_focused(true);
            }
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if self.is_ours(surface) {
            self.shared.borrow_mut().keyboard_focus = false;
            self.push(WindowEvent::WindowActiveChanged(false));
            #[cfg(feature = "accessibility")]
            if let Some(a) = self.shared.borrow().adapter.clone() {
                a.accesskit_window_focused(false);
            }
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if let Some(text) = Self::key_text(&event) {
            self.push(WindowEvent::KeyPressed { text });
        }
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if let Some(text) = Self::key_text(&event) {
            self.push(WindowEvent::KeyPressRepeated { text });
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if let Some(text) = Self::key_text(&event) {
            self.push(WindowEvent::KeyReleased { text });
        }
    }
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: RawModifiers, _: u32) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for AppState {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if !self.is_ours(&event.surface) {
                continue;
            }
            let position = LogicalPosition::new(event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.push(WindowEvent::PointerMoved { position }),
                PointerEventKind::Leave { .. } => self.push(WindowEvent::PointerExited),
                PointerEventKind::Press { button: b, .. } => {
                    tracing::debug!(x = position.x, y = position.y, button = b, "pointer press");
                    self.push(WindowEvent::PointerPressed { position, button: button(b) })
                }
                PointerEventKind::Release { button: b, .. } => self.push(WindowEvent::PointerReleased { position, button: button(b) }),
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    self.push(WindowEvent::PointerScrolled { position, delta_x: -horizontal.absolute as f32, delta_y: -vertical.absolute as f32 })
                }
            }
        }
    }
}

impl TouchHandler for AppState {
    fn down(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch, _: u32, _: u32, surface: wl_surface::WlSurface, id: i32, position: (f64, f64)) {
        if !self.is_ours(&surface) {
            return;
        }
        self.touch_points.retain(|(i, _)| *i != id);
        self.touch_points.push((id, position));
        self.push(touch_event(id, position, i_slint_core::input::TouchPhase::Started));
    }
    fn up(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch, _: u32, _: u32, id: i32) {
        if let Some(pos) = self.touch_points.iter().position(|(i, _)| *i == id) {
            let (_, p) = self.touch_points.remove(pos);
            self.push(touch_event(id, p, i_slint_core::input::TouchPhase::Ended));
        }
    }
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch, _: u32, id: i32, position: (f64, f64)) {
        if let Some((_, p)) = self.touch_points.iter_mut().find(|(i, _)| *i == id) {
            *p = position;
            self.push(touch_event(id, position, i_slint_core::input::TouchPhase::Moved));
        }
    }
    fn shape(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch, _: i32, _: f64, _: f64) {}
    fn orientation(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch, _: i32, _: f64) {}
    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch) {
        for (id, p) in std::mem::take(&mut self.touch_points) {
            self.push(touch_event(id, p, i_slint_core::input::TouchPhase::Cancelled));
        }
    }
}

fn touch_event(id: i32, position: (f64, f64), phase: i_slint_core::input::TouchPhase) -> WindowEvent {
    WindowEvent::internal(i_slint_core::platform::InternalEvent::Touch {
        id,
        position: i_slint_core::lengths::LogicalPoint::new(position.0 as f32, position.1 as f32),
        phase,
    })
}

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(AppState);
delegate_output!(AppState);
delegate_shm!(AppState);
delegate_seat!(AppState);
delegate_keyboard!(AppState);
delegate_pointer!(AppState);
delegate_touch!(AppState);
delegate_layer!(AppState);
delegate_session_lock!(AppState);
delegate_registry!(AppState);
