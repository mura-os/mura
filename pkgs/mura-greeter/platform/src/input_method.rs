//! The on-screen keyboard's two protocols, as a client (shell-plane §3.2; research/75 §3.2 —
//! squeekboard's shape): **`zwp_input_method_v2`** tells the keyboard when a text field wants it
//! (`activate` … `done`) and carries what it types back as `commit_string`; **`zwp_virtual_keyboard_v1`**
//! carries the keys that are not text — Backspace, Return, Tab, the arrows — because the
//! input-method protocol has no key event of its own (squeekboard's Erase is a virtual-keyboard
//! Backspace, `squeekboard/src/submission.rs:116-150`; `delete_surrounding_text` is never used).
//! No preedit: the keyboard commits whole characters.
//!
//! sctk 0.20 has neither client, so both are plain `Dispatch`es on the generated protocols, like
//! `text_input.rs`. The virtual keyboard needs a keymap before its first key: the `us` map from
//! xkbcommon, compiled once and handed over as a memfd (the protocol's `keymap(format, fd, size)`;
//! wvkbd and squeekboard do the same).
//!
//! Budget: two globals, one keymap compile at start (~100 KB text, freed after the fd is sent),
//! no per-key allocation.

use std::io::Write;
use std::os::fd::AsFd;

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_seat;
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::{self, ZwpInputMethodV2};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

use crate::state::AppState;

/// What the compositor's text field asked of the keyboard (one per `done`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImEvent {
    /// A field is focused: its `zwp_text_input_v3` content type and surrounding text.
    Activate { hint: u32, purpose: u32, text: String, cursor: u32 },
    /// No field is focused (squeekboard hides 200 ms later, `animation.rs:15`).
    Deactivate,
    /// Another input method holds the seat; this one will never be activated.
    Unavailable,
}

/// `zwp_text_input_v3.content_purpose` values (the protocol's enum; `wayland-protocols`
/// `text-input-unstable-v3.xml`), for the layout choice.
pub mod purpose {
    pub const NORMAL: u32 = 0;
    pub const ALPHA: u32 = 1;
    pub const DIGITS: u32 = 2;
    pub const NUMBER: u32 = 3;
    pub const PHONE: u32 = 4;
    pub const URL: u32 = 5;
    pub const EMAIL: u32 = 6;
    pub const NAME: u32 = 7;
    pub const PASSWORD: u32 = 8;
    pub const PIN: u32 = 9;
    pub const DATE: u32 = 10;
    pub const TIME: u32 = 11;
    pub const DATETIME: u32 = 12;
    pub const TERMINAL: u32 = 13;
}

/// Linux evdev key codes the keyboard sends through the virtual keyboard (`input-event-codes.h`).
pub mod key {
    pub const BACKSPACE: u32 = 14;
    pub const TAB: u32 = 15;
    pub const ENTER: u32 = 28;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const UP: u32 = 103;
    pub const DOWN: u32 = 108;
    pub const ESC: u32 = 1;
}

#[derive(Default)]
struct Pending {
    active: bool,
    hint: u32,
    purpose: u32,
    text: String,
    cursor: u32,
}

pub(crate) struct InputMethod {
    im_manager: Option<ZwpInputMethodManagerV2>,
    vk_manager: Option<ZwpVirtualKeyboardManagerV1>,
    im: Option<ZwpInputMethodV2>,
    vk: Option<ZwpVirtualKeyboardV1>,
    /// the protocol's commit serial: the number of `done` events received
    serial: u32,
    pending: Pending,
    /// the state the last `done` applied — a `done` that changes nothing produces no event
    active: bool,
    events: Vec<ImEvent>,
    start: std::time::Instant,
}

impl InputMethod {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<AppState>) -> Self {
        let im_manager = globals.bind::<ZwpInputMethodManagerV2, AppState, ()>(qh, 1..=1, ()).ok();
        let vk_manager = globals.bind::<ZwpVirtualKeyboardManagerV1, AppState, ()>(qh, 1..=1, ()).ok();
        if im_manager.is_none() {
            tracing::warn!("zwp_input_method_manager_v2 not offered; the keyboard will never be activated");
        }
        if vk_manager.is_none() {
            tracing::warn!("zwp_virtual_keyboard_manager_v1 not offered; Backspace/Return will not type");
        }
        InputMethod { im_manager, vk_manager, im: None, vk: None, serial: 0, pending: Pending::default(), active: false, events: Vec::new(), start: std::time::Instant::now() }
    }

    pub fn seat_ready(&mut self, qh: &QueueHandle<AppState>, seat: &wl_seat::WlSeat) {
        if self.im.is_none() {
            if let Some(m) = &self.im_manager {
                self.im = Some(m.get_input_method(seat, qh, ()));
            }
        }
        if self.vk.is_none() {
            if let Some(m) = &self.vk_manager {
                let vk = m.create_virtual_keyboard(seat, qh, ());
                match keymap_fd() {
                    Ok((fd, size)) => vk.keymap(1, fd.as_fd(), size),
                    Err(e) => tracing::error!("virtual keyboard keymap: {e}"),
                }
                self.vk = Some(vk);
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<ImEvent> {
        std::mem::take(&mut self.events)
    }

    /// Type text into the focused field (`commit_string` + `commit`; squeekboard's only text path).
    pub fn commit_string(&mut self, text: &str) {
        let Some(im) = &self.im else { return };
        if !self.active {
            return;
        }
        im.commit_string(text.to_string());
        im.commit(self.serial);
    }

    /// One press-and-release of an evdev key through the virtual keyboard.
    pub fn key(&mut self, keycode: u32) {
        let Some(vk) = &self.vk else { return };
        let t = self.start.elapsed().as_millis() as u32;
        vk.key(t, keycode, 1);
        vk.key(t, keycode, 0);
    }

    fn event(&mut self, event: zwp_input_method_v2::Event) {
        match event {
            zwp_input_method_v2::Event::Activate => self.pending.active = true,
            zwp_input_method_v2::Event::Deactivate => self.pending.active = false,
            zwp_input_method_v2::Event::SurroundingText { text, cursor, anchor: _ } => {
                self.pending.text = text;
                self.pending.cursor = cursor;
            }
            zwp_input_method_v2::Event::TextChangeCause { .. } => {}
            zwp_input_method_v2::Event::ContentType { hint, purpose } => {
                self.pending.hint = hint.into();
                self.pending.purpose = purpose.into();
            }
            zwp_input_method_v2::Event::Done => {
                self.serial = self.serial.wrapping_add(1);
                let p = &self.pending;
                if p.active {
                    self.active = true;
                    self.events.push(ImEvent::Activate { hint: p.hint, purpose: p.purpose, text: p.text.clone(), cursor: p.cursor });
                } else if self.active {
                    self.active = false;
                    self.events.push(ImEvent::Deactivate);
                }
                // the protocol: state is per-`done`; a new activation starts from defaults
                if !p.active {
                    self.pending = Pending::default();
                }
            }
            zwp_input_method_v2::Event::Unavailable => {
                tracing::warn!("zwp_input_method_v2: unavailable (another input method holds the seat)");
                self.events.push(ImEvent::Unavailable);
            }
            _ => {}
        }
    }
}

/// The `us` keymap as a memfd for `zwp_virtual_keyboard_v1.keymap` (sealed against growth, as
/// wlroots' keymap fds are).
fn keymap_fd() -> Result<(std::os::fd::OwnedFd, u32), String> {
    use std::os::fd::FromRawFd;
    let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
    let keymap = xkbcommon::xkb::Keymap::new_from_names(&ctx, "", "", "us", "", None, xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS).ok_or("xkb: no `us` keymap")?;
    let text = keymap.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
    // SAFETY: memfd_create with a static NUL-terminated name; the fd is owned from here on
    let raw = unsafe { libc::memfd_create(c"mura-osk-keymap".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw < 0 {
        return Err(format!("memfd_create: {}", std::io::Error::last_os_error()));
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(raw) };
    file.write_all(text.as_bytes()).and_then(|_| file.write_all(b"\0")).map_err(|e| format!("keymap write: {e}"))?;
    let size = text.len() as u32 + 1;
    // SAFETY: a valid memfd; sealing failures are not fatal (the compositor maps read-only)
    unsafe {
        libc::fcntl(raw, libc::F_ADD_SEALS, libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE | libc::F_SEAL_SEAL);
    }
    Ok((file.into(), size))
}

impl Dispatch<ZwpInputMethodV2, ()> for AppState {
    fn event(state: &mut Self, _: &ZwpInputMethodV2, event: zwp_input_method_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        state.input_method.event(event);
    }
}

delegate_noop!(AppState: ignore ZwpInputMethodManagerV2);
delegate_noop!(AppState: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(AppState: ignore ZwpVirtualKeyboardV1);
