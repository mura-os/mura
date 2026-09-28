//! mura-osk — Mura's on-screen keyboard (shell-plane.md §3.2; research/75 §3.2; ADR 0012).
//!
//! **Shape: squeekboard's, in Slint on Mura's sctk platform.** A layer-shell client — layer `top`,
//! anchored bottom|left|right, namespace `osk`, exclusive zone = its height
//! (`squeekboard/src/panel.c:64,84`) — that binds `zwp_input_method_v2` to learn when a text field
//! wants it and `zwp_virtual_keyboard_v1` for the keys that are not text. It types with
//! `commit_string` only, erases with a virtual-keyboard Backspace, sends no preedit and never
//! `delete_surrounding_text` (`squeekboard/src/submission.rs:116-150`). It is **zxr's child** in
//! every mode (`--osk`, KWin's input-method shape: spawned over `WAYLAND_SOCKET`, restarted on
//! crash within a bound) — so it exists on the greeter and the lock scene too, where the
//! compositor's trusted-connection bit is the only thing that lets a keyboard map.
//!
//! **Show/hide** is the input method's: `activate` shows, `deactivate` hides after 200 ms
//! (squeekboard's `HIDING_TIMEOUT`, `animation.rs:15` — no flicker between fields). The
//! **layout** follows the field's content purpose (`data/loading.rs:122-126`): digits, number,
//! phone, pin, date, time → the digit pad; everything else the letters, with symbols one key
//! away. A password field is the letters with shift off.
//!
//! **`sm.puri.OSK0`** (`/sm/puri/OSK0`, `SetVisible(b)`, property `Visible`) is served on the
//! session bus when there is one, so a shell control that knows squeekboard's name works
//! unchanged; without a bus (greeter mode) the keyboard simply follows the input method.
//!
//! Every key is an accessible button with its name (research/37): the AT-SPI tree is the keyboard.
//!
//! Budget: one thread for the scene (plus zbus's executor when a bus exists), one Wayland
//! connection, wl_shm and the software renderer; a hidden keyboard is an unmapped surface and
//! costs the compositor nothing; keys are large so the ray's error budget is the wearer's, not
//! the layout's.

use std::rc::Rc;
use std::time::Duration;

use mura_slint_platform::input_method::{key, purpose};
use mura_slint_platform::{Anchor, Handle, ImEvent, KeyboardInteractivity, Layer, Role};

slint::include_modules!();

/// The keyboard's height in the frame's pixels (the layer surface's requested size and its
/// exclusive zone). Four rows of ≥ 72 px keys with spacing: large enough for a ray at arm's
/// length (spatial-input §13: targets ≥ 2°; at 21.3 px/° on the greeter frame a key is ≈ 3.4° tall).
const HEIGHT_PX: u32 = 360;
/// squeekboard's hide delay (`animation.rs:15`): a field change is not a flicker.
const HIDE_DELAY: Duration = Duration::from_millis(200);

const USAGE: &str = "mura-osk [--height PX]";

fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into())).with_writer(std::io::stderr).init();
    let mut height = HEIGHT_PX;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--height" => height = args.next().and_then(|v| v.parse().ok()).unwrap_or(HEIGHT_PX),
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("mura-osk: unknown argument {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    if let Err(e) = run(height) {
        eprintln!("mura-osk: {e}");
        std::process::exit(1);
    }
}

fn layout_for(purpose_: u32) -> Layout {
    match purpose_ {
        purpose::DIGITS | purpose::NUMBER | purpose::PHONE | purpose::PIN | purpose::DATE | purpose::TIME | purpose::DATETIME => Layout::Digits,
        _ => Layout::Letters,
    }
}

fn run(height: u32) -> Result<(), Box<dyn std::error::Error>> {
    let role = Role::Layer {
        layer: Layer::Top,
        anchor: Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        exclusive_zone: height as i32,
        // the keyboard never takes keyboard focus: its keys are the ray's (squeekboard: none)
        keyboard: KeyboardInteractivity::None,
        namespace: "osk".into(),
        size: (0, height),
    };
    let handle = mura_slint_platform::init(mura_slint_platform::Config { role, app_id: "mura-osk".into() })?;
    let ui = Osk::new()?;

    // hidden until a field asks (squeekboard maps on the first `activate`); the surface exists so
    // the compositor knows the OSK's namespace and band from the start
    handle.set_visible(false);

    let hide_timer = Rc::new(slint::Timer::default());
    let bus_visible = osk0::serve(handle.clone());
    // squeekboard's visibility override (`state.rs:292-318`): the wearer's "hide" holds while the
    // same activation goes on updating (a field's cursor moved, the scene re-laid out around the
    // keyboard's own band); any change of active state — a new field, no field — clears it
    let vis = Rc::new(std::cell::Cell::new(Visibility { im_active: false, forced_hidden: false }));

    // keys → the input method
    {
        let handle = handle.clone();
        let ui_weak = ui.as_weak();
        let vis = vis.clone();
        let bus_visible = bus_visible.clone();
        ui.on_key_pressed(move |action, text| {
            let Some(ui) = ui_weak.upgrade() else { return };
            match action.as_str() {
                "text" => {
                    handle.commit_string(text.as_str());
                    // one-shot shift, as every soft keyboard does
                    if ui.get_shift() {
                        ui.set_shift(false);
                    }
                }
                "backspace" => handle.key(key::BACKSPACE),
                "enter" => handle.key(key::ENTER),
                "tab" => handle.key(key::TAB),
                "left" => handle.key(key::LEFT),
                "right" => handle.key(key::RIGHT),
                "shift" => ui.set_shift(!ui.get_shift()),
                "symbols" => ui.set_layout(Layout::Symbols),
                "letters" => ui.set_layout(Layout::Letters),
                "digits" => ui.set_layout(Layout::Digits),
                "hide" => {
                    vis.set(Visibility { forced_hidden: true, ..vis.get() });
                    handle.set_visible(false);
                    bus_visible.borrow_mut().set(false);
                }
                other => tracing::warn!(other, "unknown key action"),
            }
        });
    }

    // the input method → show/hide and the layout
    {
        let handle_ = handle.clone();
        let ui_weak = ui.as_weak();
        let hide_timer = hide_timer.clone();
        let bus_visible = bus_visible.clone();
        let vis = vis.clone();
        handle.on_im_event(move |ev| {
            let Some(ui) = ui_weak.upgrade() else { return };
            match ev {
                ImEvent::Activate { purpose: p, hint: _, .. } => {
                    hide_timer.stop();
                    let v = vis.get();
                    // an update of the same activation keeps the wearer's hide; a new one clears it
                    let forced_hidden = v.im_active && v.forced_hidden;
                    vis.set(Visibility { im_active: true, forced_hidden });
                    ui.set_layout(layout_for(p));
                    if !v.im_active {
                        ui.set_shift(false);
                    }
                    if !forced_hidden && !handle_.is_visible() {
                        handle_.set_visible(true);
                        bus_visible.borrow_mut().set(true);
                        tracing::info!(purpose = p, "activated: keyboard shown");
                    }
                }
                ImEvent::Deactivate => {
                    vis.set(Visibility { im_active: false, forced_hidden: false });
                    let h = handle_.clone();
                    let bv = bus_visible.clone();
                    hide_timer.start(slint::TimerMode::SingleShot, HIDE_DELAY, move || {
                        h.set_visible(false);
                        bv.borrow_mut().set(false);
                        tracing::info!("deactivated: keyboard hidden");
                    });
                }
                ImEvent::Unavailable => tracing::warn!("another input method holds the seat; this keyboard stays hidden"),
            }
        });
    }

    ui.run()?;
    Ok(())
}

/// `sm.puri.OSK0` — squeekboard's D-Bus name, served when a session bus exists.
mod osk0 {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::Handle;

    /// The `Visible` property's backing value, written from the Slint thread, read by the bus thread.
    pub struct Visible(Arc<AtomicBool>);

    impl Visible {
        pub fn set(&mut self, v: bool) {
            self.0.store(v, Ordering::Relaxed);
        }
    }

    thread_local! {
        /// The platform handle, on the Slint thread only; `SetVisible` reaches it through
        /// `invoke_from_event_loop` (`Handle` is an `Rc` and never crosses threads).
        static HANDLE: RefCell<Option<Handle>> = const { RefCell::new(None) };
    }

    struct Osk0 {
        visible: Arc<AtomicBool>,
    }

    #[zbus::interface(name = "sm.puri.OSK0")]
    impl Osk0 {
        /// squeekboard's `SetVisible`: a shell's keyboard button.
        fn set_visible(&mut self, visible: bool) {
            let _ = slint::invoke_from_event_loop(move || {
                HANDLE.with(|h| {
                    if let Some(h) = &*h.borrow() {
                        h.set_visible(visible);
                    }
                })
            });
            self.visible.store(visible, Ordering::Relaxed);
        }

        #[zbus(property)]
        fn visible(&self) -> bool {
            self.visible.load(Ordering::Relaxed)
        }
    }

    /// Serve the name on the session bus if one is reachable; otherwise log and go without (the
    /// greeter mode has no session bus — the keyboard follows the input method alone).
    pub fn serve(handle: Handle) -> Rc<RefCell<Visible>> {
        let flag = Arc::new(AtomicBool::new(false));
        let visible = Rc::new(RefCell::new(Visible(flag.clone())));
        HANDLE.with(|h| *h.borrow_mut() = Some(handle));
        std::thread::Builder::new()
            .name("osk0-bus".into())
            .spawn(move || {
                let iface = Osk0 { visible: flag };
                match zbus::blocking::connection::Builder::session().and_then(|b| b.name("sm.puri.OSK0")).and_then(|b| b.serve_at("/sm/puri/OSK0", iface)).and_then(|b| b.build()) {
                    Ok(_conn) => {
                        tracing::info!("sm.puri.OSK0 served on the session bus");
                        // the connection lives as long as this thread; the executor is zbus's
                        loop {
                            std::thread::park();
                        }
                    }
                    Err(e) => tracing::info!("no session bus for sm.puri.OSK0 ({e}); the keyboard follows the input method"),
                }
            })
            .expect("spawn osk0 thread");
        visible
    }
}

/// The input method's active state and the wearer's hide, squeekboard's `visibility_override`.
#[derive(Clone, Copy, Debug)]
struct Visibility {
    im_active: bool,
    forced_hidden: bool,
}
