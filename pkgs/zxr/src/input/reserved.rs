//! `Slot::Reserved` — the reserved system input (native-openxr-apps.md §6, ruled Q-B/Q-C/Q-E in
//! §10 lines 280-295; spatial-input.md §2 lines 172-188 "The reserved system input"; §1a line 113
//! "reserved — the system input: consumed here, never forwarded").
//!
//! **The rule** (research/66 §12 verdict 4, lines 408-412): one control per device tier is the
//! *system* control, consumed before any application and delivered to none — the 2D compositor's
//! non-maskable chord (mutter `restore-shortcuts`, niri's hardcoded binds: "the user has no way
//! to unlock the compositor… 'jailing' the user"), OpenXR's `system` semantics, Valve's, Meta's,
//! Microsoft's. So this stage returns [`Flow::Consumed`] for every sample that carries the role
//! and never anything else.
//!
//! **Where the role comes from** (three sources, one press map):
//! - `Button::System` on any kind — a controller's `/input/system/click` or the contract's
//!   `hmdButtons.systemRole` through libinput (native-openxr-apps §6 lines 186-199; the evdev
//!   code → `Button::System` mapping is the libinput backend's, lane F).
//! - `Flags::SYSTEM_GESTURE` on a hand sample — the posture-gated held palm gesture (§6 lines
//!   200-209, ruled Q-C). Every platform recognises it system-side and tells the app to stand
//!   down through a flag on the hand data (research/68 §3.5 lines 251-279: Horizon's
//!   `SystemGestureProcessing`, Android XR's `AimFlags.SystemGesture`, HoloLens's shell,
//!   SteamVR's `IsInputAvailable`); under OpenXR that is `XR_FB_hand_tracking_aim`'s
//!   `SYSTEM_GESTURE_BIT_FB`. Monado has neither the extension nor a recogniser, so the flag
//!   arrives from zxr's own bridge until it does (spatial-input §1a line 76).
//! - `Flags::MENU_PRESSED` — the non-dominant hand's completed gesture, "to signal a menu button
//!   press" (research/68 §3.5 lines 255-256).
//!
//! **Position in the chain.** KWin puts its own non-maskable filters — `VirtualTerminal`,
//! `LockScreen` — *after* the accessibility transforms (`references/kwin/src/input.h:366-393`:
//! `SlowKeys, BounceKeys, StickyKeys, MouseKeys, DwellClicker, EisInput, VirtualTerminal,
//! LockScreen, …`). zxr puts the reserved input **first, ahead of a11y** (spatial-input §1a lines
//! 113-130), because an a11y transform must not be able to eat the wearer's only escape from a
//! native app. That ordering difference from the ported order is a **flagged judgment** (it is
//! already the design's, §1a; recorded here because it is a deliberate divergence from KWin's).
//!
//! **Inhibitors.** `keyboard-shortcuts-inhibit` is honoured only *below* this stage: the reserved
//! input is not a key, so no inhibitor can ever name it and this stage never consults one. Lane
//! F's keyboard path checks the inhibitor after `Reserved` has had the sample.

use super::activity;
use super::{Button, Flags, Flow, Sample, SourceKind, Stage};
use crate::state::Zxr;

// -------------------------------------------------------------------------------------------
// The press map (native-openxr-apps §6 lines 227-236, ruled Q-B) — pure, driven by a clock
// the caller supplies, so the boundaries are testable without a runtime.
// -------------------------------------------------------------------------------------------

/// short press — summon/dismiss. **Stand-in:** the design says "< ~500 ms" (§6 line 231) and
/// Horizon's own boundary is 500 ms; 400 ms is this lane's number, chosen so the short/long
/// window has a dead band rather than a shared edge. Flagged: not measured, not a comparable's.
pub const SHORT_MAX_NS: u64 = 400_000_000;
/// long press — recenter (§6 line 233: "long = recenter"). **Stand-in:** between Meta's
/// > 500 ms and PICO's 1 s (research/66 §11); 800 ms is this lane's pick. Flagged.
pub const LONG_NS: u64 = 800_000_000;
/// the gap from the first release to the second press that makes two shorts a double (§6 line
/// 234). **Stand-in:** 300 ms, the double-click order of the desktops; no XR comparable gives a
/// number. Flagged.
pub const DOUBLE_NS: u64 = 300_000_000;
/// system + select held together — force quit (§6 line 235). **Stand-in:** 1 s; the comparables
/// state the chord (Apple Crown + top button, Deck Steam+B long) but not its hold. Flagged.
pub const CHORD_NS: u64 = 1_000_000_000;

/// What the reserved control asked for. The compositor's, never a client's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemAction {
    /// short press: summon/dismiss the shell over the primary app (§6 line 231)
    Summon,
    /// long press: recenter — rigid re-seat of head-relative content (§6 line 233; research/36 §8)
    Recenter,
    /// double press: show/hide planes, or passthrough on tiers with a dedicated button (§6 line 234)
    Toggle,
/// system + select held: force quit the primary app's scope (§6 line 235)
    Quit,
}

/// The press map's state. One system control at a time: where a target has both an HMD-body and a
/// controller control they carry *identical* semantics (§6 lines 210-213, ruled Q-E), so both feed
/// this one map rather than two.
#[derive(Default)]
pub struct PressMap {
    down_since: Option<u64>,
    /// select was down at some point during this system hold: the chord claims the press, so
    /// neither the short nor the long action may fire from it
    chorded: bool,
    /// this hold already produced an action (long press / chord); its release emits nothing
    fired: bool,
    select_since: Option<u64>,
    /// the release time of a short press waiting out the double window
    pending_short: Option<u64>,
    /// this press began inside the double window of the previous short
    doubling: bool,
}

impl PressMap {
    /// A `system` edge. Returns the action this edge completes, if any.
    pub fn system(&mut self, pressed: bool, now_ns: u64) -> Option<SystemAction> {
        if pressed {
            if self.down_since.is_some() {
                // level-triggered sources repeat the press; only the edge counts
                return None;
            }
            self.doubling = matches!(self.pending_short, Some(prev) if now_ns.saturating_sub(prev) <= DOUBLE_NS);
            self.pending_short = None;
            self.down_since = Some(now_ns);
            self.chorded = self.select_since.is_some();
            self.fired = false;
            None
        } else {
            let since = self.down_since.take()?;
            let held = now_ns.saturating_sub(since);
            let claimed = self.fired || self.chorded || self.select_since.is_some();
            let doubling = std::mem::take(&mut self.doubling);
            self.chorded = false;
            self.fired = false;
            if claimed {
                self.pending_short = None;
                return None;
            }
            if held >= SHORT_MAX_NS {
                // between short and long the platforms' split has no action
                return None;
            }
            if doubling {
                Some(SystemAction::Toggle)
            } else {
                self.pending_short = Some(now_ns);
                None
            }
        }
    }

    /// A `select` edge — recorded for the chord only. Select is **not** reserved input: it is
    /// forwarded, and this call has no say in that.
    pub fn select(&mut self, pressed: bool, now_ns: u64) {
        if pressed {
            if self.select_since.is_none() {
                self.select_since = Some(now_ns);
            }
            if self.down_since.is_some() {
                self.chorded = true;
            }
        } else {
            self.select_since = None;
        }
    }

    /// The timers: the long press and the chord fire while the control is still held (Horizon
    /// and PICO both act on the hold, not its release), the short press fires when the double
    /// window closes. At most one of the three can come due in one tick.
    pub fn tick(&mut self, now_ns: u64) -> Option<SystemAction> {
        if let Some(since) = self.down_since {
            if self.fired {
                return None;
            }
            if let Some(sel) = self.select_since {
                if now_ns.saturating_sub(since.max(sel)) >= CHORD_NS {
                    self.fired = true;
                    return Some(SystemAction::Quit);
                }
                return None;
            }
            if self.chorded {
                // select was held during this press and let go: a failed chord, not a long press
                return None;
            }
            if now_ns.saturating_sub(since) >= LONG_NS {
                self.fired = true;
                return Some(SystemAction::Recenter);
            }
            return None;
        }
        if let Some(prev) = self.pending_short {
            if now_ns.saturating_sub(prev) > DOUBLE_NS {
                self.pending_short = None;
                return Some(SystemAction::Summon);
            }
        }
        None
    }

    pub fn held(&self) -> bool {
        self.down_since.is_some()
    }
}

// -------------------------------------------------------------------------------------------
// The recogniser: which samples carry the role, and what each edge means. Pure — no `Zxr`.
// -------------------------------------------------------------------------------------------

/// What the recogniser made of one sample.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reserve {
    /// not reserved input (or `select`, recorded for the chord and forwarded)
    None,
    /// the `system` control's edge
    System(bool),
    /// a hand's palm gesture is in progress; `onset` is its first sample
    Gesture { onset: bool },
    /// the non-dominant hand's completed menu gesture (research/68 §3.5)
    Menu,
}

pub struct Verdict {
    pub reserve: Reserve,
    pub flow: Flow,
    pub action: Option<SystemAction>,
}

/// The index of a kind in [`SourceKind::ALL`] — the recogniser's per-kind edge memory is a fixed
/// array, not a map (no allocation per event).
fn kind_index(k: SourceKind) -> usize {
    SourceKind::ALL.iter().position(|x| *x == k).unwrap_or(0)
}

#[derive(Default)]
pub struct Recogniser {
    pub map: PressMap,
    gesture: [bool; SourceKind::ALL.len()],
    menu: [bool; SourceKind::ALL.len()],
}

impl Recogniser {
    pub fn sample(&mut self, s: &Sample) -> Verdict {
        let i = kind_index(s.kind);
        // The palm gesture is level-triggered on the hand data, `FB_hand_tracking_aim`'s shape:
        // every sample carrying the bit is consumed, which *is* the platforms' "suspend your own
        // gesture processing" in Wayland terms (research/68 §3.5) — the client sees nothing of
        // the hand while the system gesture runs.
        if s.flags.contains(Flags::SYSTEM_GESTURE) {
            let onset = !self.gesture[i];
            self.gesture[i] = true;
            let action = if onset { self.map.system(true, s.time_ns) } else { None };
            return Verdict { reserve: Reserve::Gesture { onset }, flow: Flow::Consumed, action };
        }
        if self.gesture[i] {
            // The gesture ends on the next *state* sample of that hand (one with a pose). A
            // sample without a pose is not hand state — notably the cancel release this stage
            // pushed at onset, which must reach the seat and so must not be consumed here.
            if s.pose.is_some() {
                self.gesture[i] = false;
                let action = self.map.system(false, s.time_ns);
                return Verdict { reserve: Reserve::System(false), flow: Flow::Consumed, action };
            }
            return Verdict { reserve: Reserve::None, flow: Flow::Continue, action: None };
        }
        // `MENU_PRESSED` is a completion, not a hold: its onset is the whole event.
        let menu = s.flags.contains(Flags::MENU_PRESSED);
        if menu && !self.menu[i] {
            self.menu[i] = true;
            return Verdict { reserve: Reserve::Menu, flow: Flow::Consumed, action: Some(SystemAction::Summon) };
        }
        self.menu[i] = menu;
        if menu {
            return Verdict { reserve: Reserve::Menu, flow: Flow::Consumed, action: None };
        }
        match s.button {
            Some((Button::System, pressed)) => {
                let action = self.map.system(pressed, s.time_ns);
                Verdict { reserve: Reserve::System(pressed), flow: Flow::Consumed, action }
            }
            Some((Button::Select, pressed)) => {
                self.map.select(pressed, s.time_ns);
                Verdict { reserve: Reserve::None, flow: Flow::Continue, action: None }
            }
            _ => Verdict { reserve: Reserve::None, flow: Flow::Continue, action: None },
        }
    }
}

/// The synthetic release that cancels a hand's open contacts when the system gesture takes the
/// hand: a release edge on a source that is no longer tracked. Lane C maps release-after-loss
/// onto `wl_touch.cancel` — the protocol's word for "the compositor took the contact away", which
/// is exactly what the platforms' "the app must stand down" means for a client that already has a
/// `down` (research/68 §3.5 lines 253-256, 278-279).
pub fn cancel_sample(kind: SourceKind, now_ns: u64) -> Sample {
    let mut c = Sample::new(kind, now_ns);
    c.button = Some((Button::Select, false));
    c.tracked = false;
    c.ready = false;
    c
}

// -------------------------------------------------------------------------------------------
// The stage
// -------------------------------------------------------------------------------------------

/// The `Slot::Reserved` stage: the recogniser, the press map's timers, and the compositor-side
/// effect of each action.
#[derive(Default)]
pub struct Reserved {
    pub rec: Recogniser,
    actions: Vec<SystemAction>,
    pub summons: u64,
    pub recenters: u64,
    pub toggles: u64,
    pub quits: u64,
    /// hands whose open contacts were cancelled by a gesture onset
    pub gesture_cancels: u64,
}

impl Reserved {
    /// Every action this stage produced since the last call, in order. The shell and the lock
    /// machine read it; nothing below this stage ever does.
    pub fn take_actions(&mut self) -> Vec<SystemAction> {
        std::mem::take(&mut self.actions)
    }

    fn apply(&mut self, a: SystemAction, st: &mut Zxr) {
        match a {
            SystemAction::Summon => {
                self.summons += 1;
                // §6 line 231: the game becomes VISIBLE, not FOCUSED, and zxr composes again.
                // Leaving quiet mode is the part of that zxr already has (spec §7 rev 3.3).
                let was_quiet = st.quiet;
                if was_quiet {
                    st.set_quiet(false);
                }
                tracing::info!(was_quiet, "reserved: summon (native-openxr-apps §6 short press)");
            }
            SystemAction::Toggle => {
                self.toggles += 1;
                let was_quiet = st.quiet;
                if was_quiet {
                    st.set_quiet(false);
                }
                tracing::info!(was_quiet, "reserved: toggle — show/hide planes or passthrough (§6 double press)");
            }
            SystemAction::Recenter => {
                self.recenters += 1;
                // TODO(recenter): re-seating is the runtime's. The hook is a `LOCAL` re-anchor of
                // zxr's own reference space plus the primary session's — the `xrRequestExitSession`
                // class of runtime call zxr does not have on Monado yet (native-openxr-apps §6
                // line 233 "long = recenter", research/66 §14's upstream list; spatial-input §2
                // line 179 "long press recenters (research/36 §8)"). Pinned places stay put.
                tracing::info!(count = self.recenters, "reserved: recenter (§6 long press) — runtime re-seat pending");
            }
            SystemAction::Quit => {
                self.quits += 1;
                // TODO(quit): the launcher owns the scope, so the kill is its (native-openxr-apps
                // §3 "Launch, primary, close"): request exit, then kill the systemd scope after a
                // timeout. This stage only recognises the chord.
                tracing::info!(count = self.quits, "reserved: force-quit chord (§6 system+select) — scope kill is the launcher's (§3)");
            }
        }
        self.actions.push(a);
    }
}

impl Stage for Reserved {
    fn name(&self) -> &'static str {
        "reserved:system"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        let v = self.rec.sample(s);
        if let Reserve::Gesture { onset: true } = v.reserve {
            st.input.queue.push(cancel_sample(s.kind, s.time_ns));
            self.gesture_cancels += 1;
            tracing::info!(kind = ?s.kind, "reserved: system gesture onset — that hand's contacts cancelled");
        }
        // A consumed press is user activity (KWin's spy sees every event, including the ones its
        // filters go on to eat — `references/kwin/src/input.cpp:3169-3172`).
        if matches!(v.reserve, Reserve::System(true) | Reserve::Gesture { onset: true } | Reserve::Menu) {
            activity::notify_flagged(st, s.time_ns, s.flags);
        }
        if let Some(a) = v.action {
            self.apply(a, st);
        }
        v.flow
    }

    fn tick(&mut self, st: &mut Zxr, now_ns: u64) {
        if let Some(a) = self.rec.map.tick(now_ns) {
            self.apply(a, st);
        }
    }
}

// -------------------------------------------------------------------------------------------
// Tests — the press map's boundaries and the recogniser's verdicts, on a fake clock
// -------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Side;

    const MS: u64 = 1_000_000;

    fn hand(t: u64, gesture: bool) -> Sample {
        let mut s = Sample::new(SourceKind::Hand(Side::Right), t);
        s.pose = Some(openxr::Posef::IDENTITY);
        s.tracked = true;
        if gesture {
            s.flags.insert(Flags::SYSTEM_GESTURE);
        }
        s
    }

    fn system(t: u64, pressed: bool) -> Sample {
        Sample::new(SourceKind::Controller(Side::Right), t).with_button(Button::System, pressed)
    }

    #[test]
    fn short_press_summons_after_the_double_window() {
        let mut m = PressMap::default();
        assert_eq!(m.system(true, 0), None);
        assert_eq!(m.system(false, 100 * MS), None, "a short press waits out the double window");
        assert_eq!(m.tick(200 * MS), None);
        assert_eq!(m.tick(500 * MS), Some(SystemAction::Summon));
        assert_eq!(m.tick(900 * MS), None, "fires once");
    }

    #[test]
    fn a_press_between_short_and_long_does_nothing() {
        let mut m = PressMap::default();
        m.system(true, 0);
        assert_eq!(m.system(false, 500 * MS), None);
        assert_eq!(m.tick(2_000 * MS), None);
    }

    #[test]
    fn long_press_recenters_while_still_held_and_the_release_is_silent() {
        let mut m = PressMap::default();
        m.system(true, 0);
        assert_eq!(m.tick(799 * MS), None, "799 ms is not long yet");
        assert_eq!(m.tick(800 * MS), Some(SystemAction::Recenter));
        assert_eq!(m.tick(1_500 * MS), None, "once per hold");
        assert_eq!(m.system(false, 1_600 * MS), None);
        assert_eq!(m.tick(2_000 * MS), None, "and no summon after it");
    }

    #[test]
    fn two_shorts_inside_the_window_toggle() {
        let mut m = PressMap::default();
        m.system(true, 0);
        m.system(false, 100 * MS);
        assert_eq!(m.system(true, 300 * MS), None, "second press inside the 300 ms gap");
        assert_eq!(m.system(false, 380 * MS), Some(SystemAction::Toggle));
        assert_eq!(m.tick(1_000 * MS), None, "the pending short was claimed by the double");
    }

    #[test]
    fn a_second_press_outside_the_window_is_two_summons() {
        let mut m = PressMap::default();
        m.system(true, 0);
        m.system(false, 100 * MS);
        assert_eq!(m.tick(500 * MS), Some(SystemAction::Summon));
        m.system(true, 600 * MS);
        assert_eq!(m.system(false, 700 * MS), None);
        assert_eq!(m.tick(1_100 * MS), Some(SystemAction::Summon));
    }

    #[test]
    fn the_chord_quits_and_beats_the_long_press() {
        let mut m = PressMap::default();
        m.select(true, 0);
        m.system(true, 50 * MS);
        assert_eq!(m.tick(800 * MS), None, "the long press does not fire while select is held");
        assert_eq!(m.tick(1_049 * MS), None);
        assert_eq!(m.tick(1_050 * MS), Some(SystemAction::Quit), "1 s after the later of the two");
        assert_eq!(m.tick(3_000 * MS), None);
        assert_eq!(m.system(false, 3_100 * MS), None);
    }

    #[test]
    fn a_failed_chord_produces_nothing() {
        let mut m = PressMap::default();
        m.system(true, 0);
        m.select(true, 100 * MS);
        m.select(false, 200 * MS);
        assert_eq!(m.tick(1_500 * MS), None, "not a long press: select claimed the hold");
        assert_eq!(m.system(false, 1_600 * MS), None);
        assert_eq!(m.tick(2_500 * MS), None);
    }

    #[test]
    fn the_system_control_is_always_consumed_and_select_is_not() {
        let mut r = Recogniser::default();
        for (t, pressed) in [(0, true), (100 * MS, false)] {
            let v = r.sample(&system(t, pressed));
            assert_eq!(v.flow, Flow::Consumed, "the system control never reaches a client");
        }
        let mut sel = Sample::new(SourceKind::Controller(Side::Right), 200 * MS);
        sel.button = Some((Button::Select, true));
        assert_eq!(r.sample(&sel).flow, Flow::Continue, "select is forwarded");
        // and a repeat press (level-triggered source) is still consumed
        assert_eq!(r.sample(&system(300 * MS, true)).flow, Flow::Consumed);
        assert_eq!(r.sample(&system(300 * MS, true)).flow, Flow::Consumed);
    }

    #[test]
    fn nothing_else_is_reserved() {
        let mut r = Recogniser::default();
        for k in SourceKind::ALL {
            let mut s = Sample::new(k, 0);
            s.pose = Some(openxr::Posef::IDENTITY);
            s.tracked = true;
            assert_eq!(r.sample(&s).flow, Flow::Continue, "{k:?} plain state sample");
        }
    }

    #[test]
    fn a_gesture_onset_is_one_cancel_and_the_hold_recenters() {
        let mut r = Recogniser::default();
        let mut onsets = 0;
        // 20 frames of the gesture at 90 Hz: every one consumed, exactly one onset
        for f in 0..20u64 {
            let v = r.sample(&hand(f * 11 * MS, true));
            assert_eq!(v.flow, Flow::Consumed, "the client stands down for the whole gesture");
            if v.reserve == (Reserve::Gesture { onset: true }) {
                onsets += 1;
            }
        }
        assert_eq!(onsets, 1, "exactly one cancel per gesture");
        // held past the long threshold → recenter
        assert_eq!(r.map.tick(900 * MS), Some(SystemAction::Recenter));
        // the cancel sample this stage pushes is not itself reserved: it must reach the seat
        let c = cancel_sample(SourceKind::Hand(Side::Right), 10 * MS);
        assert!(!c.tracked && c.button == Some((Button::Select, false)));
        assert_eq!(r.sample(&c).flow, Flow::Continue);
        // …and the gesture ends on the next hand state sample without the flag
        let v = r.sample(&hand(1_000 * MS, false));
        assert_eq!(v.flow, Flow::Consumed);
        assert_eq!(v.reserve, Reserve::System(false));
        assert_eq!(r.sample(&hand(1_011 * MS, false)).flow, Flow::Continue, "and then the hand is the client's again");
    }

    #[test]
    fn a_menu_gesture_summons_once() {
        let mut r = Recogniser::default();
        let mut m = hand(0, false);
        m.flags.insert(Flags::MENU_PRESSED);
        let v = r.sample(&m);
        assert_eq!(v.flow, Flow::Consumed);
        assert_eq!(v.action, Some(SystemAction::Summon));
        m.time_ns = 11 * MS;
        assert_eq!(r.sample(&m).action, None, "the completion is an edge");
    }
}
