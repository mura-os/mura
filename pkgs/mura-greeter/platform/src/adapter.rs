//! The `WindowAdapter`: Slint's window over the platform's one Wayland surface. Modelled on
//! Slint's `linuxkms` `FullscreenWindowAdapter` (`references/slint/internal/backends/linuxkms/fullscreenwindowadapter.rs`):
//! it owns the `SoftwareRenderer`, remembers "redraw requested", and forwards the two things a
//! Wayland client must relay — the input-method requests of a focused text field and the mouse
//! cursor shape — to the state through `Shared`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use i_slint_core::window::{InputMethodRequest, WindowAdapterInternal};
use slint::platform::software_renderer::{RepaintBufferType, SoftwareRenderer};
use i_slint_core::renderer::RendererSealed as _;
use slint::platform::{PlatformError, WindowAdapter};
use slint::{PhysicalSize, Window};

use crate::state::Shared;

#[cfg(feature = "accessibility")]
use std::cell::OnceCell;

pub struct MuraWindowAdapter {
    window: Window,
    renderer: SoftwareRenderer,
    size: Cell<PhysicalSize>,
    redraw_requested: Cell<bool>,
    shared: Rc<RefCell<Shared>>,
    #[cfg(feature = "accessibility")]
    accesskit: OnceCell<RefCell<mura_slint_accesskit::AccessKitAdapter>>,
    self_weak: Weak<Self>,
}

impl MuraWindowAdapter {
    pub fn new(shared: Rc<RefCell<Shared>>) -> Rc<Self> {
        let adapter = Rc::<Self>::new_cyclic(|self_weak| MuraWindowAdapter {
            window: Window::new(self_weak.clone()),
            renderer: SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::SwappedBuffers),
            size: Cell::new(PhysicalSize::new(0, 0)),
            redraw_requested: Cell::new(true),
            shared,
            #[cfg(feature = "accessibility")]
            accesskit: OnceCell::new(),
            self_weak: self_weak.clone(),
        });
        adapter.renderer.set_window_adapter(&(adapter.clone() as Rc<dyn WindowAdapter>));
        adapter
    }

    /// Start the AT-SPI adapter (`accesskit_unix`); `wake` is called from AccessKit's thread and
    /// must make the loop call [`Self::pump_accesskit`].
    #[cfg(feature = "accessibility")]
    pub fn init_accesskit(self: &Rc<Self>, wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
        let host: Weak<dyn mura_slint_accesskit::Host> = self.self_weak.clone();
        let _ = self.accesskit.set(RefCell::new(mura_slint_accesskit::AccessKitAdapter::new(host, wake)));
    }

    /// Drain AccessKit's posted events and apply the resulting actions to the window.
    #[cfg(feature = "accessibility")]
    pub fn pump_accesskit(&self) {
        let Some(cell) = self.accesskit.get() else { return };
        let actions = cell.borrow_mut().process_pending();
        for a in actions {
            a.invoke(&self.window);
        }
    }

    /// Keyboard focus entered or left the surface.
    #[cfg(feature = "accessibility")]
    pub fn accesskit_window_focused(&self, focused: bool) {
        if let Some(cell) = self.accesskit.get() {
            cell.borrow_mut().set_window_focused(focused);
        }
    }

    pub fn software_renderer(&self) -> &SoftwareRenderer {
        &self.renderer
    }

    /// The compositor configured a size (layer-shell / lock configure).
    pub fn set_physical_size(&self, size: PhysicalSize) {
        if self.size.get() != size {
            self.size.set(size);
            self.window.dispatch_event(slint::platform::WindowEvent::Resized { size: size.to_logical(self.window.scale_factor()) });
            self.redraw_requested.set(true);
        }
    }

    pub fn take_redraw_request(&self) -> bool {
        self.redraw_requested.replace(false)
    }
}

impl WindowAdapter for MuraWindowAdapter {
    fn window(&self) -> &Window {
        &self.window
    }

    fn size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.renderer
    }

    fn request_redraw(&self) {
        self.redraw_requested.set(true);
    }

    fn set_visible(&self, visible: bool) -> Result<(), PlatformError> {
        self.shared.borrow_mut().visible = visible;
        Ok(())
    }

    fn internal(&self, _: i_slint_core::InternalToken) -> Option<&dyn WindowAdapterInternal> {
        Some(self)
    }
}

impl WindowAdapterInternal for MuraWindowAdapter {
    fn input_method_request(&self, request: InputMethodRequest) {
        self.shared.borrow_mut().im_requests.push(request);
    }

    fn handle_focus_change(&self, _old: Option<i_slint_core::items::ItemRc>, _new: Option<i_slint_core::items::ItemRc>) {
        #[cfg(feature = "accessibility")]
        if let Some(cell) = self.accesskit.get() {
            cell.borrow_mut().handle_focus_item_change();
        }
    }

    fn register_item_tree(&self, _: i_slint_core::item_tree::ItemTreeRefPin) {
        #[cfg(feature = "accessibility")]
        if let Some(cell) = self.accesskit.get() {
            cell.borrow_mut().reload_tree();
        }
    }

    fn unregister_item_tree(&self, component: i_slint_core::item_tree::ItemTreeRef, _items: &mut dyn Iterator<Item = std::pin::Pin<i_slint_core::items::ItemRef<'_>>>) {
        #[cfg(feature = "accessibility")]
        if let Some(cell) = self.accesskit.get() {
            cell.borrow_mut().unregister_item_tree(component);
        }
        #[cfg(not(feature = "accessibility"))]
        let _ = component;
    }
}

#[cfg(feature = "accessibility")]
impl mura_slint_accesskit::Host for MuraWindowAdapter {
    fn window_adapter(&self) -> Option<Rc<dyn WindowAdapter>> {
        self.self_weak.upgrade().map(|a| a as Rc<dyn WindowAdapter>)
    }
    fn has_focus(&self) -> bool {
        self.shared.borrow().keyboard_focus
    }
    fn with_accesskit(&self, f: Box<dyn FnOnce(&RefCell<mura_slint_accesskit::AccessKitAdapter>) + '_>) {
        if let Some(cell) = self.accesskit.get() {
            f(cell);
        }
    }
}
