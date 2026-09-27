//! Focus and activation (spatial-input §6 "Focus and activation (ruled)", lines 280-307;
//! specs/zxr-core.md §8 lines 434-439 "Focus follows the commit, never hover"; ADR 0013
//! amendment item 5; window-workspace-management.md §11 lines 264-272).
//!
//! The rules, and the comparable each is ported from:
//!
//! - **A commit sets keyboard focus** ([`commit_focus`]): a `down` / `button` press on a member
//!   makes it the keyboard focus and the *committed* member; hover, gaze and motion never call
//!   this. The member goes to the front of the [`FocusStack`] — cosmic-comp's `FocusStack`
//!   (`references/cosmic-comp/src/shell/focus/mod.rs:123-126`: an `IndexSet` whose *last* entry is
//!   the most recent; `last()` skips dead and minimized entries) — and its serial becomes
//!   `last_commit_serial`.
//! - **New windows take focus unless a user commit intervened** ([`new_window_takes_focus`]):
//!   mutter's `intervening_user_event_occurred` (`references/mutter/src/core/window.c:2013` — the
//!   rule; `:2125` `window_state_on_map`: `*takes_focus = !intervening_events`) compares the
//!   window's user-time against the focus window's; niri's `ActivateWindow::Smart`
//!   (`references/niri/src/layout/mod.rs:516, 629`) is the same shape behind a closure. zxr's
//!   "user time" is the commit serial: the serial current when the toplevel was requested is
//!   compared with the seat's latest commit at map time.
//! - **Activation is serial-validated** ([`serial_ok`]): niri's check
//!   (`references/niri/src/handlers/mod.rs:790-802`) accepts a token whose serial is no older than
//!   the keyboard's **or** the pointer's `last_enter` (both, "since layer-shell surfaces with no
//!   keyboard interactivity won't have any keyboard focus"); a token with no serial is urgency-only
//!   (`:766-773`, `UrgentOnlyMarker`). mutter (`references/mutter/src/wayland/meta-wayland-activation.c:74-82`
//!   stores the serial + seat; `:295-312` validates it against the keyboard's grab serial then the
//!   seat's grab info) and KWin (`references/kwin/src/wayland/xdgactivation_v1.cpp:49-69` stores it;
//!   the policy lives in `activation.cpp`) converge on "the token carries an input serial the seat
//!   recognises". smithay supplies both sides: `KeyboardHandle::last_enter`
//!   (`references/smithay/src/input/keyboard/mod.rs:1341`) and `PointerHandle::last_enter`
//!   (`references/smithay/src/input/pointer/mod.rs:472`).
//! - **Refusal is urgency-only**: `Payload.urgent` is set, nothing moves or raises (§6: "the
//!   compositor never moves or raises for it"; the shell presents it).
//! - **Focus restore** on close is the stack's most recent still-mapped member
//!   ([`FocusStack::restore`]), cosmic's `FocusStack::last`.
//! - **A manager's focus request** ([`manager_focus_request`]) is the same rule with the manager's
//!   delivered `interaction` serial (window-workspace-management §11: "an application's standing
//!   under `xdg-activation`, no more"); no protocol carries it yet.
//! - **Layer-shell keyboard interactivity** sits above member focus (§6 rev 0.5, niri's
//!   `update_keyboard_focus`; research/77 §4.2): [`layer_focus_override`] is the topmost mapped
//!   `exclusive` layer surface (`shell::exclusive_override`) and `focus_window` defers to it;
//!   an `on_demand` surface is an ordinary stack member; a `none` surface never takes the
//!   keyboard (`shell::layer_accepts_focus`). The fallback member on a `none` commit is the
//!   stack's most recent mapped *window*.
//!
//! Budget (invariant 9): the stack is a `Vec<MemberId>` of ≤ N members, touched once per commit
//! and once per close; the serial rule is two `u32` comparisons; nothing per tick.

use smithay::utils::Serial;

use crate::scene::MemberId;
use crate::state::Zxr;

/// Most-recent-first list of committed members (cosmic's `IndexSet`, reversed so the front is
/// the restore candidate). Members are unique; a re-commit moves one to the front.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct FocusStack {
    order: Vec<MemberId>,
}

impl FocusStack {
    /// A commit on `id`: move it to the front (insert if absent).
    pub fn touch(&mut self, id: MemberId) {
        if let Some(i) = self.order.iter().position(|m| *m == id) {
            if i == 0 {
                return;
            }
            self.order.remove(i);
        }
        self.order.insert(0, id);
    }

    /// The member is gone (closed).
    pub fn remove(&mut self, id: MemberId) {
        self.order.retain(|m| *m != id);
    }

    /// The most recently committed member that `alive` still accepts — cosmic's
    /// `FocusStack::last` ("the last unminimized window … that is still alive").
    pub fn restore(&self, alive: impl Fn(MemberId) -> bool) -> Option<MemberId> {
        self.order.iter().copied().find(|m| alive(*m))
    }

    pub fn top(&self) -> Option<MemberId> {
        self.order.first().copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = MemberId> + '_ {
        self.order.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// The seat's focus bookkeeping on `Zxr`.
#[derive(Default, Debug)]
pub struct Focus {
    pub stack: FocusStack,
    /// the serial of the last user commit delivered to a client (`down` / `button` press)
    pub last_commit_serial: Option<Serial>,
    /// the member that commit landed on
    pub last_commit_member: Option<MemberId>,
    /// counters (rendered by the journal once `journal.rs` takes them — patch in the lane report)
    pub commits: u64,
    pub activations_focused: u64,
    pub activations_urgent: u64,
    pub new_windows_focused: u64,
    pub new_windows_urgent: u64,
    pub urgency_marks: u64,
    pub tokens_created: u64,
    pub tokens_without_serial: u64,
    pub activations_expired: u64,
}

/// niri's activation check (`references/niri/src/handlers/mod.rs:790-802`): the token's serial is
/// accepted when it is no older than the keyboard's *or* the pointer's `last_enter`. No serial
/// (`:766-773`) or no live enter on either device → refused (urgency-only).
pub fn serial_ok(token: Option<Serial>, kb_last_enter: Option<Serial>, ptr_last_enter: Option<Serial>) -> bool {
    let Some(serial) = token else { return false };
    if kb_last_enter.is_some_and(|e| serial.is_no_older_than(&e)) {
        return true;
    }
    ptr_last_enter.is_some_and(|e| serial.is_no_older_than(&e))
}

/// mutter's `intervening_user_event_occurred`, inverted (`references/mutter/src/core/window.c:2013,
/// 2125`): a new window takes focus unless a user commit happened after the request that created
/// it. `at_request` is the seat's `last_commit_serial` when the toplevel was requested; `now` is
/// it at map time. Equal (or both absent) means no commit intervened.
pub fn new_window_takes_focus(at_request: Option<Serial>, now: Option<Serial>) -> bool {
    match (at_request, now) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(_), None) => true,
        (Some(a), Some(b)) => a == b || a.is_no_older_than(&b),
    }
}

/// `wm.focus.new_windows` (window-workspace-management §12): `smart` is the rule above (GNOME
/// `focus-new-windows = smart`, niri `Smart`); `strict` never focuses a new window — it is marked
/// urgent instead (GNOME `strict`). Anything else reads as `smart`.
pub fn new_window_rule(mode: &str, at_request: Option<Serial>, now: Option<Serial>) -> bool {
    match mode {
        "strict" => false,
        _ => new_window_takes_focus(at_request, now),
    }
}

/// Layer-shell keyboard-interactivity precedence (§6 rev 0.5; spec §8 rev 3.12; research/77
/// §4.2): the topmost mapped `exclusive` layer surface on overlay/top — the greeter/lock program's
/// mode — owns the keyboard above every member, and on bottom/background only while no window is
/// mapped (niri `update_keyboard_focus`, `references/niri/src/niri.rs:1354-1366`; cosmic-comp
/// `focus/mod.rs:648-672`; sway `layer_shell.c:103-138`). `focus_window` defers to it. An
/// `on_demand` surface is not an override: it is a member of the stack (`commit_focus`, the
/// new-window rule at map — `shell::layer::commit`).
pub fn layer_focus_override(st: &Zxr) -> Option<MemberId> {
    crate::shell::exclusive_override(st)
}

/// A layer surface mapped, unmapped, committed or died: recompute the override and re-apply the
/// focus (a changed override is counted — spec §11 `layer_focus_overrides`).
pub fn layer_focus_changed(st: &mut Zxr) {
    let now = layer_focus_override(st);
    if now != st.shell.last_override {
        st.shell.last_override = now;
        st.shell.focus_overrides += 1;
        st.journal.layer_focus_overrides += 1;
    }
    let f = st.scene.focused;
    st.focus_window(f);
}

/// spatial-input §6 line 282: a `down` / `button` press on `member`'s surface. Called by the
/// transports (lane C) on the press edge only — never on hover, gaze or motion. Sets the keyboard
/// focus (and `xdg_toplevel` Activated), records the member at the front of the stack, records the
/// serial the client received, and clears any urgency on it.
pub fn commit_focus(st: &mut Zxr, member: MemberId, serial: Serial) {
    if st.scene.get(member).is_none() {
        return;
    }
    // a `none` layer surface (panel, OSD, notification, OSK) never takes the keyboard: the commit
    // changes no focus (spec §8 rev 3.12; the OSK types through the IM into the focused member)
    if crate::shell::layer_accepts_focus(st, member) == Some(false) {
        return;
    }
    st.focus.last_commit_serial = Some(serial);
    st.focus.last_commit_member = Some(member);
    // `wm.focus.raise_on_commit` (GNOME `raise-on-click`, KWin `ClickRaise`): the commit
    // focuses either way; raising to the front of the stack is the preference
    if st.prefs.wm_focus_raise_on_commit {
        st.focus.stack.touch(member);
    }
    st.focus.commits += 1;
    set_urgent(st, member, false);
    st.focus_window(Some(member));
    // the manager hears every commit's serial (`interaction`) and may `focus` with it — the
    // serial rule stays the compositor's (window-workspace-management §11, Q5-focus)
    crate::policy::seam::interaction(st, member, serial);
}

/// Mark / clear urgency on a member (§6: "a state on the member the shell presents"). Nothing
/// moves or raises here.
pub fn set_urgent(st: &mut Zxr, member: MemberId, on: bool) {
    if let Some(m) = st.scene.get_mut(member) {
        if m.m.urgent != on {
            m.m.urgent = on;
            if on {
                st.focus.urgency_marks += 1;
            }
        }
    }
}

/// The activation decision shared by `xdg_activation_v1` and the manager hook: `token_serial`
/// against both devices' `last_enter`. On success the member is focused (and its urgency
/// cleared); on refusal it is marked urgent. Returns whether it focused.
pub fn activate(st: &mut Zxr, member: MemberId, token_serial: Option<Serial>) -> bool {
    let kb = st.seat.get_keyboard().and_then(|k| k.last_enter());
    let ptr = st.seat.get_pointer().and_then(|p| p.last_enter());
    let ok = serial_ok(token_serial, kb, ptr);
    if ok {
        st.focus.activations_focused += 1;
        set_urgent(st, member, false);
        st.focus.stack.touch(member);
        st.focus_window(Some(member));
    } else {
        st.focus.activations_urgent += 1;
        set_urgent(st, member, true);
    }
    ok
}

/// window-workspace-management §11 (lines 264-272): a manager's `focus(window, serial)` with a
/// serial the compositor delivered to it as `interaction`. Same rule as activation; refusal is
/// urgency-only. No protocol carries this yet — the seam is the function.
pub fn manager_focus_request(st: &mut Zxr, member: MemberId, serial: Serial) -> bool {
    if st.scene.get(member).is_none() {
        return false;
    }
    activate(st, member, Some(serial))
}

/// Focus restore after `member` closed (§6 line 297): the stack's most recent still-mapped member.
pub fn restore_after_close(st: &mut Zxr, closed: MemberId) {
    st.focus.stack.remove(closed);
    if st.focus.last_commit_member == Some(closed) {
        st.focus.last_commit_member = None;
    }
    let next = {
        let scene = &st.scene;
        st.focus.stack.restore(|m| scene.get(m).map(|x| x.m.mapped()).unwrap_or(false))
    };
    // fall back to any mapped member only when nothing was ever committed (fresh session)
    let next = next.or_else(|| st.scene.iter().filter(|(_, m)| m.m.mapped()).map(|(id, _)| id).last());
    st.focus_window(next);
}

/// Lane F's counters as journal lines (`key=value`), for `zxr ctl journal` and the end-of-run
/// dump once `main.rs` appends them (patch in the lane report; `journal.rs` is not lane-owned).
pub fn render_counters(st: &Zxr) -> String {
    let f = &st.focus;
    let t = &st.text;
    let p = &st.peripherals;
    let e = &st.ei;
    format!(
        "focus_commits={}\nfocus_stack_len={}\nactivations_focused={}\nactivations_urgent={}\nactivations_expired={}\ntokens_created={}\ntokens_without_serial={}\nnew_windows_focused={}\nnew_windows_urgent={}\nurgency_marks={}\nkeys_physical={}\nkeys_emulated={}\nkeys_emitted_im={}\nshortcut_inhibitors={}\nosk_suppressed={}\nlibinput_active={}\nlibinput_devices={}\nlibinput_events={}\nlibinput_events_unmapped={}\nhmd_button_samples={}\nei_clients={}\nei_connections_total={}\nei_events={}\nei_events_unmapped={}\nei_text_events={}",
        f.commits,
        f.stack.len(),
        f.activations_focused,
        f.activations_urgent,
        f.activations_expired,
        f.tokens_created,
        f.tokens_without_serial,
        f.new_windows_focused,
        f.new_windows_urgent,
        f.urgency_marks,
        t.physical_keys,
        t.emulated_keys,
        t.keys_emitted,
        t.inhibitors.len(),
        st.osk_suppressed() as u8,
        p.active as u8,
        p.devices,
        p.events,
        p.events_unmapped,
        p.hmd_buttons,
        e.clients,
        e.connections_total,
        e.events,
        e.events_unmapped,
        e.text_events
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Arena;

    /// Distinct `MemberId`s from a throwaway arena (handles are opaque).
    fn ids(n: usize) -> Vec<MemberId> {
        let mut a: Arena<()> = Arena::default();
        (0..n).map(|_| MemberId(a.insert(()))).collect()
    }
    fn s(n: u32) -> Serial {
        Serial::from(n)
    }

    #[test]
    fn serial_rule_table() {
        // (token, kb last_enter, ptr last_enter) -> focus?
        let table: [(Option<u32>, Option<u32>, Option<u32>, bool); 9] = [
            (Some(10), Some(10), None, true),   // equal to keyboard enter
            (Some(11), Some(10), None, true),   // newer than keyboard enter
            (Some(9), Some(10), None, false),   // older than keyboard enter → urgency
            (Some(9), Some(10), Some(8), true), // older than keyboard, newer than pointer (niri: both devices)
            (Some(9), None, Some(9), true),     // no keyboard focus (layer surface); pointer enter equal
            (Some(9), None, None, false),       // no live enter anywhere
            (None, Some(10), Some(10), false),  // no serial → urgency-only
            (None, None, None, false),
            (Some(1), Some(u32::MAX), None, true), // wrap-around: 1 is *after* u32::MAX (modular compare)
        ];
        // and the converse of the wrap case: a token from before the wrap is older
        assert!(!serial_ok(Some(s(u32::MAX)), Some(s(1)), None));
        for (tok, kb, ptr, want) in table {
            assert_eq!(serial_ok(tok.map(s), kb.map(s), ptr.map(s)), want, "token={tok:?} kb={kb:?} ptr={ptr:?}");
        }
    }

    #[test]
    fn new_window_rule() {
        // no commit ever: takes focus
        assert!(new_window_takes_focus(None, None));
        // requested with commit 5 current, still 5 at map: no intervening commit
        assert!(new_window_takes_focus(Some(s(5)), Some(s(5))));
        // a commit (7) happened after the request (5): urgent instead
        assert!(!new_window_takes_focus(Some(s(5)), Some(s(7))));
        // requested before any commit, one happened since: urgent
        assert!(!new_window_takes_focus(None, Some(s(3))));
        // `wm.focus.new_windows`
        assert!(super::new_window_rule("smart", Some(s(5)), Some(s(5))));
        assert!(!super::new_window_rule("strict", Some(s(5)), Some(s(5))), "strict: never");
        assert!(super::new_window_rule("whatever", None, None), "unknown reads as smart");
    }

    #[test]
    fn focus_stack_restore_after_close() {
        let v = ids(3);
        let (a, b, c) = (v[0], v[1], v[2]);
        let mut st = FocusStack::default();
        st.touch(a);
        st.touch(b);
        st.touch(c);
        st.touch(b); // re-commit moves to front
        assert_eq!(st.iter().collect::<Vec<_>>(), vec![b, c, a]);
        assert_eq!(st.top(), Some(b));
        // close b: restore goes to c (most recent still alive), not a ("last live member")
        st.remove(b);
        assert_eq!(st.restore(|_| true), Some(c));
        // c unmapped: skip to a
        assert_eq!(st.restore(|m| m != c), Some(a));
        assert_eq!(st.restore(|_| false), None);
        st.touch(a);
        assert_eq!(st.len(), 2);
        assert_eq!(st.top(), Some(a));
    }
}
