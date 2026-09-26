//! The text-entry seam (spatial-input §12 lines 404-416; §8 lines 337-342 "Keyboard"; spec §8
//! lines 450-452; research/63 §9) and the `Im` stage of the chain (§1a line 129).
//!
//! **The chain is Wayland's** (§12): a client's `zwp_text_input_v3.enable` + `commit` after the
//! keyboard `enter` tells the compositor a field has focus; smithay's text-input dispatch then
//! sends `activate` on the bound `zwp_input_method_v2` (`references/smithay/src/wayland/text_input/mod.rs:85`
//! `TextInputManagerState`; `input_method/mod.rs:129` `InputMethodManagerState`;
//! `virtual_keyboard/mod.rs:70` `VirtualKeyboardManagerState`). Keyboard focus reaches the
//! text-input side inside smithay's `WlSurface` keyboard target (`references/smithay/src/wayland/seat/keyboard.rs:245-259`:
//! `text_input.set_focus` + `enter` when an IM instance exists; `:280-291` `leave` on focus
//! leave) — zxr only serves the globals and implements the handlers (`state.rs`).
//!
//! **The IM is a separate client, never spawned here.** KWin spawns and restarts its input
//! method process (`references/kwin/src/inputmethod.cpp:864-925`: `startInputMethod` creates a
//! socketpair connection, `QProcess::start`, restarts on `CrashExit` up to five times) — the
//! reason transfers (third-party code must not take the compositor down, §1a line 79), the
//! mechanism does not: Mura's keyboard component is a session unit (ADR 0012) that binds
//! `zwp_input_method_manager_v2` like any client; the unit restarts it.
//!
//! **Placement hint.** The IM popup's parent is the focused text-input's surface; smithay asks
//! the compositor for the parent geometry (`InputMethodHandler::parent_geometry`) and positions
//! the popup at `set_cursor_rectangle` within it (`input_method_handle.rs:103-124`) — the popup
//! is drawn *on the committed member's plane* like any popup of that toplevel (spec §5a's tree
//! walk). The keyboard component itself (research/36 §7's floating, focus-bound keyboard) is a
//! shell client summoned into a head-anchored place near the committed member — its placement is
//! the shell's, not this module's; the hint it needs is [`Zxr::focus`]'s `last_commit_member`.
//!
//! **Physical keys suppress the OSK** (§8 lines 338-341; §12 line 411): a key from a real keyboard
//! (`SourceKind::Keyboard` without `Flags::EMULATED` / `Flags::SYNTHETIC`) records
//! `Text::osk_suppressed_until_ns = now + OSK_SUPPRESS_NS`. Meta minimizes the keyboard while a
//! physical one is used [external]; StereoKit's fallback keyboard refuses to open for five
//! minutes after a physical key (`references/stereokit/StereoKitC/platforms/platform.cpp:253-263`,
//! `physical_interact_timeout = 60 * 5`) — the stand-in duration. What smithay lets the
//! compositor *do* with it: `InputMethodHandle::deactivate_input_method` is `pub(crate)`
//! (`references/smithay/src/wayland/input_method/input_method_handle.rs:149`), so zxr cannot send
//! `deactivate` to the IM without a text-input focus change; `TextInputHandle::leave()`
//! (`text_input_handle.rs:107`) *is* public but lies to the client (it still has focus). The
//! suppression is therefore a **state the shell reads** ([`Zxr::osk_suppressed`]) until either
//! smithay exposes deactivation (upstream item) or the keyboard component learns it over the
//! shell protocol — flagged in the lane report.
//!
//! **The `Im` stage** ([`ImStage`]): the IM sees only what nothing above consumed (§1a). In
//! smithay the IM's keyboard grab (`InputMethodKeyboardGrab`, `input_method/mod.rs:77`) is
//! honoured *inside* `KeyboardHandle::input` — when the IM has grabbed, the key goes to the IM's
//! virtual keyboard instead of the focus; when not, to the focus. So routing and emission are
//! one call, and this stage's own work is the suppression bookkeeping. Key emission to the seat
//! is the `Seat` slot's (lane C); until that stage lands, [`IM_EMITS_KEYS`] makes this stage
//! perform the call so keys reach clients — a labelled stand-in, one constant to flip.
//!
//! **`keyboard-shortcuts-inhibit`** (§8 line 337): `KeyboardShortcutsInhibitState`
//! (`references/smithay/src/wayland/keyboard_shortcuts_inhibit/mod.rs:65`); inhibitors are
//! activated on creation (niri `references/niri/src/handlers/mod.rs:281-283` — the confirmation
//! dialog is a FIXME there too) and recorded per surface; [`Zxr::shortcuts_inhibited`] answers
//! for the *focused* surface. The compositor's key chords (none yet — the reserved input is not
//! a key) check it after the `Reserved` stage, so the escape chord stays (the protocol: "under no
//! obligation to disable all of its shortcuts").
//!
//! Budget (invariant 9): per key one flag test and one `u64` store; no per-tick work except a
//! mutex probe for the IM-bound log line, once.

use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::input_method::InputMethodSeat;
use smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitor;

use super::{Flags, Flow, Sample, SourceKind, Stage};
use crate::state::Zxr;

/// StereoKit's five minutes (`platform.cpp:258`), the design's stand-in (§8 line 341).
pub const OSK_SUPPRESS_NS: u64 = 5 * 60 * 1_000_000_000;

/// Stand-in: the `Im` stage performs the seat's `KeyboardHandle::input` for key samples and
/// consumes them, until lane C's `Seat` stage emits keys. Flip to `false` when it does.
pub const IM_EMITS_KEYS: bool = true;

/// The text-entry state on `Zxr`.
#[derive(Default)]
pub struct Text {
    /// the OSK is suppressed until this CLOCK_MONOTONIC time (0 = not suppressed)
    pub osk_suppressed_until_ns: u64,
    /// keyboard-shortcuts inhibitors by surface (niri's `keyboard_shortcuts_inhibiting_surfaces`)
    pub inhibitors: Vec<(ObjectId, KeyboardShortcutsInhibitor)>,
    /// the IM bound / unbound transition was logged
    pub im_bound: bool,
    /// counters (journal patch in the lane report)
    pub physical_keys: u64,
    pub emulated_keys: u64,
    pub keys_emitted: u64,
}

impl Text {
    pub fn suppressed(&self, now_ns: u64) -> bool {
        now_ns < self.osk_suppressed_until_ns
    }

    pub fn inhibitor_for(&self, surface: &WlSurface) -> Option<&KeyboardShortcutsInhibitor> {
        let id = surface.id();
        self.inhibitors.iter().find(|(s, _)| *s == id).map(|(_, i)| i)
    }

    pub fn add_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        let id = inhibitor.wl_surface().id();
        self.inhibitors.retain(|(s, _)| *s != id);
        self.inhibitors.push((id, inhibitor));
    }

    pub fn remove_inhibitor(&mut self, inhibitor: &KeyboardShortcutsInhibitor) {
        let id = inhibitor.wl_surface().id();
        self.inhibitors.retain(|(s, _)| *s != id);
    }
}

/// A key edge from a real keyboard: `Keyboard` kind, a key, and neither the EI nor the injector
/// flag (libei's "distinction", `references/libei/README.md:55-60`).
pub fn is_physical_key(s: &Sample) -> bool {
    s.kind == SourceKind::Keyboard && s.key.is_some() && !s.flags.contains(Flags::EMULATED) && !s.flags.contains(Flags::SYNTHETIC)
}

/// The `Im` slot.
pub struct ImStage;

impl Stage for ImStage {
    fn name(&self) -> &'static str {
        "im:text"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        if s.kind != SourceKind::Keyboard {
            return Flow::Continue;
        }
        let Some((code, pressed)) = s.key else { return Flow::Continue };
        if is_physical_key(s) {
            st.text.physical_keys += 1;
            if pressed {
                st.text.osk_suppressed_until_ns = s.time_ns.saturating_add(OSK_SUPPRESS_NS);
            }
        } else {
            st.text.emulated_keys += 1;
        }
        if IM_EMITS_KEYS {
            // smithay routes to the IM's grab or the focus inside this call (see module doc)
            st.send_key_at(code, pressed, s.time_ns);
            st.text.keys_emitted += 1;
            return Flow::Consumed;
        }
        Flow::Continue
    }

    fn tick(&mut self, st: &mut Zxr, _now_ns: u64) {
        // one log line per bind/unbind transition of an input-method client (a mutex probe)
        let bound = st.seat.input_method().has_instance();
        if bound != st.text.im_bound {
            st.text.im_bound = bound;
            if bound {
                tracing::info!("input-method bound (zwp_input_method_v2); text-input activate/deactivate now reach it");
            } else {
                tracing::info!("input-method unbound");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_vs_emulated_keys() {
        let mut k = Sample::new(SourceKind::Keyboard, 1);
        k.key = Some((30, true));
        assert!(is_physical_key(&k));
        let mut e = k;
        e.flags.insert(Flags::EMULATED);
        assert!(!is_physical_key(&e));
        let mut sy = k;
        sy.flags.insert(Flags::SYNTHETIC);
        assert!(!is_physical_key(&sy));
        // a pointer button is not a key; a keyboard sample without a key edge is not one either
        let p = Sample::new(SourceKind::Pointer, 1).with_button(super::super::Button::Select, true);
        assert!(!is_physical_key(&p));
        assert!(!is_physical_key(&Sample::new(SourceKind::Keyboard, 1)));
    }

    #[test]
    fn suppression_window() {
        let mut t = Text::default();
        assert!(!t.suppressed(0));
        t.osk_suppressed_until_ns = 1_000 + OSK_SUPPRESS_NS;
        assert!(t.suppressed(1_000));
        assert!(t.suppressed(1_000 + OSK_SUPPRESS_NS - 1));
        assert!(!t.suppressed(1_000 + OSK_SUPPRESS_NS));
        assert_eq!(OSK_SUPPRESS_NS, 300_000_000_000);
    }
}
