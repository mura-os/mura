//! `Slot::Mode` — the greeter/lock gate and user presence (ADR 0007 lines 56-67 and 97-109;
//! spatial-input.md §1a line 115 "mode — `--greeter` / lock: only the auth scene may receive
//! anything below this line", line 76's presence row, §13).
//!
//! **The lock model** (ADR 0007 lines 56-67). I1: "while locked, no client colour/depth buffer is
//! sampled and **no input reaches any client** (focus is withdrawn; the seat routes only to the
//! lock scene)". This stage is the input half of I1, and the reason it sits *below*
//! `Slot::Reserved` and `Slot::A11y`: the wearer's escape must work while locked, and the
//! accessibility transforms "must keep working on a locked screen" (spatial-input §13 lines
//! 426-430), which is KWin's own order — `SlowKeys … DwellClicker, EisInput` before
//! `VirtualTerminal, LockScreen` (`references/kwin/src/input.h:366-393`). KWin's `LockScreenFilter`
//! is installed immediately after the a11y plugins and the activity spy
//! (`references/kwin/src/input.cpp:3169-3178`) and eats everything it is not routing to the
//! locker — the same shape as `gate` below.
//!
//! **What still passes while gated.** `Keyboard` samples: the auth scene is a PAM conversation and
//! needs them (ADR 0007 lines 74-82 — the lock UI renders generic PAM prompts with a digit-pad
//! fast path *and a virtual keyboard path*). The mode-gated destination for everything that
//! passes is the **auth scene**, not a client; there is no client focus while gated (I1). The
//! second exception the design names — members of an `exclusive` layer-shell surface — has no
//! implementation to gate: zxr serves no `wlr-layer-shell` yet, so [`ModeGate::exclusive`] is the
//! documented hook and is empty.
//!
//! **Presence** (ADR 0007 lines 97-102): doff, seen through `XR_EXT_user_presence` in the
//! compositor's own OpenXR loop, "→ blank panels immediately + start a grace timer; don within
//! grace (default ~30-60 s) → resume without re-auth". **The grace timer and the blank are the
//! lock machine's, not this stage's** — this stage raises [`ModeGate::presence_state`] and
//! suspends the XR sources, and stops there. "Presence never *unlocks* without a biometric"
//! (line 100), so nothing here ever leaves `Mode::Locked`.
//!
//! **Settings the lock machine reads** (settings.rs `Prefs`, resolved and live; declared in
//! `lib/contract/preferences.nix` and `default.nix`): `session.lock.{enabled,on_doff,on_idle,
//! on_suspend,doff_grace_s}`, `session.idle.{delay_s,lock_delay_s}`, `session.docked.{lock_on_doff,
//! deep_idle_after_s}` — the ADR 0007 ladder's numbers. The grace and idle timers are not built
//! (ADR 0007's lock machine, M1); until they are, the keys reach `Prefs` and nothing else. The
//! one built consumer is `session.idle.count_emulated_input` → `activity::Activity::count_emulated`.

use super::activity;
use super::reserved::cancel_sample;
use super::{Flow, Mode, Sample, SourceKind, Stage};
use crate::scene::MemberId;
use crate::state::Zxr;

/// What the runtime last said about the wearer's head (`XR_EXT_user_presence`). Read by the lock
/// machine, which owns the grace timer (ADR 0007 lines 97-100).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Presence {
    #[default]
    Unknown,
    Present,
    /// doff: the lock machine blanks the panels and starts the grace timer from here
    Absent,
}

/// The XR-side kinds, the ones presence suspends. Peripherals (`Pointer`, `Keyboard`) are on the
/// seat through libinput and keep working with the headset off the head — spatial-input §1a's
/// presence row and the docked branch of ADR 0007's ladder (lines 103-108) both require it.
pub fn is_xr(k: SourceKind) -> bool {
    matches!(k, SourceKind::Head | SourceKind::Gaze | SourceKind::Hand(_) | SourceKind::Controller(_))
}

/// The gate, pure. One rule, no state: what the mode and the suspension say about one sample.
pub fn gate(mode: Mode, xr_suspended: bool, exclusive: bool, s: &Sample) -> Flow {
    if mode != Mode::Normal && !exclusive {
        // ADR 0007 I1: nothing below this line reaches a client. The auth scene's own keys are
        // the exception the PAM conversation needs.
        return if s.kind == SourceKind::Keyboard { Flow::Continue } else { Flow::Consumed };
    }
    if xr_suspended && is_xr(s.kind) {
        return Flow::Consumed;
    }
    Flow::Continue
}

/// The `Slot::Mode` stage.
#[derive(Default)]
pub struct ModeGate {
    pub presence_state: Presence,
    /// presence transitions this stage acted on (the journal's `input_presence_changes` counts the
    /// events; this counts the ones that reached the stage)
    pub presence_transitions: u64,
    /// samples consumed because the compositor is in greeter/lock mode
    pub gated: u64,
    /// samples consumed because the XR sources are suspended (doff / docked)
    pub xr_gated: u64,
    /// cancel samples pushed for XR sources at a doff
    pub doff_cancels: u64,
}

impl ModeGate {
    // The activity record lives on `Input::activity` (KWin's spy shape, `references/kwin/src/input.cpp:3169-3172`);
    // read it there — this stage only feeds it (`don` counts as activity).

    /// The `exclusive` layer-shell hook (spatial-input §1a line 115; ADR 0007's "the seat routes
    /// only to the lock scene"): a member whose surface holds an exclusive layer-shell role may
    /// receive while the mode gate is closed. zxr serves no layer shell yet, so this is always
    /// `false` and the call site is the one line that changes when it does.
    fn exclusive(&self, _st: &Zxr, _s: &Sample) -> bool {
        false
    }

    /// doff (`present == Some(false)`): suspend the XR sources, close every open contact, and mark
    /// every composed plane not-composed. ADR 0007 lines 98-99; spec §5a rev 3.4's one rule for
    /// "not composed" is `xdg_toplevel.suspended`, which is what `Zxr::set_suspended` sends.
    fn doff(&mut self, st: &mut Zxr, now_ns: u64) {
        st.input.xr_suspended = true;
        self.presence_state = Presence::Absent;
        // The gesture state of the XR sources lives in the stabilize/tier stages, which this
        // stage cannot see; a release for every XR kind is idempotent for the ones that had no
        // contact and is the only cheap way to guarantee none is left open. Lane C maps
        // release-after-loss to `wl_touch.cancel`.
        for k in SourceKind::ALL {
            if is_xr(k) {
                st.input.queue.push(cancel_sample(k, now_ns));
                self.doff_cancels += 1;
            }
        }
        let ids: Vec<MemberId> = st.scene.iter().filter(|(_, m)| m.m.mapped()).map(|(id, _)| id).collect();
        for id in ids {
            st.set_suspended(id, true);
        }
        tracing::info!(members = self.doff_cancels, "mode: doff — XR sources suspended, planes not composed (ADR 0007 doff→blank; grace timer is the lock machine's)");
    }

    /// don (`present == Some(true)`): the XR sources resume and the planes are composed again
    /// unless quiet mode says otherwise. Don is user activity; it is **not** an unlock (ADR 0007
    /// line 100: "Presence never *unlocks* without a biometric").
    fn don(&mut self, st: &mut Zxr, now_ns: u64) {
        st.input.xr_suspended = false;
        self.presence_state = Presence::Present;
        let quiet = st.quiet;
        let ids: Vec<MemberId> = st.scene.iter().filter(|(_, m)| m.m.mapped()).map(|(id, _)| id).collect();
        for id in ids {
            let hidden = st.scene.get(id).map(|m| m.m.hidden).unwrap_or(false);
            st.set_suspended(id, quiet || hidden);
        }
        activity::notify(st, now_ns);
        tracing::info!(quiet, mode = ?st.input.mode, "mode: don — XR sources resumed");
    }
}

impl Stage for ModeGate {
    fn name(&self) -> &'static str {
        "mode:greeter-lock"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        let exclusive = self.exclusive(st, s);
        let flow = gate(st.input.mode, st.input.xr_suspended, exclusive, s);
        if flow == Flow::Consumed {
            if st.input.mode != Mode::Normal {
                self.gated += 1;
            } else {
                self.xr_gated += 1;
            }
        }
        flow
    }

    fn tick(&mut self, st: &mut Zxr, now_ns: u64) {
        if !st.input.presence_changed {
            return;
        }
        st.input.presence_changed = false;
        self.presence_transitions += 1;
        match st.input.present {
            Some(false) => self.doff(st, now_ns),
            Some(true) => self.don(st, now_ns),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Button, Side};

    fn sample(k: SourceKind) -> Sample {
        let mut s = Sample::new(k, 1);
        if is_xr(k) {
            s.pose = Some(openxr::Posef::IDENTITY);
            s.tracked = true;
        }
        s
    }

    #[test]
    fn normal_mode_gates_nothing() {
        for k in SourceKind::ALL {
            assert_eq!(gate(Mode::Normal, false, false, &sample(k)), Flow::Continue, "{k:?}");
        }
    }

    #[test]
    fn greeter_and_locked_consume_everything_but_the_keyboard() {
        for mode in [Mode::Greeter, Mode::Locked] {
            for k in SourceKind::ALL {
                let want = if k == SourceKind::Keyboard { Flow::Continue } else { Flow::Consumed };
                assert_eq!(gate(mode, false, false, &sample(k)), want, "{mode:?} {k:?}");
            }
            // a commit from a controller is gated too — focus is withdrawn (ADR 0007 I1)
            let press = sample(SourceKind::Controller(Side::Right)).with_button(Button::Select, true);
            assert_eq!(gate(mode, false, false, &press), Flow::Consumed);
            // …and an exclusive layer-shell member would pass (the documented hook)
            assert_eq!(gate(mode, false, true, &press), Flow::Continue);
        }
    }

    #[test]
    fn suspension_gates_the_xr_kinds_and_leaves_the_peripherals() {
        for k in SourceKind::ALL {
            let want = if is_xr(k) { Flow::Consumed } else { Flow::Continue };
            assert_eq!(gate(Mode::Normal, true, false, &sample(k)), want, "{k:?}");
        }
        assert!(is_xr(SourceKind::Head) && is_xr(SourceKind::Gaze));
        assert!(is_xr(SourceKind::Hand(Side::Left)) && is_xr(SourceKind::Controller(Side::Left)));
        assert!(!is_xr(SourceKind::Pointer) && !is_xr(SourceKind::Keyboard));
    }

    #[test]
    fn a_doff_pushes_one_cancel_per_xr_kind() {
        // the shape `doff` relies on, without a `Zxr`: the XR kinds and their cancels
        let cancels: Vec<Sample> = SourceKind::ALL.iter().filter(|k| is_xr(**k)).map(|k| cancel_sample(*k, 7)).collect();
        assert_eq!(cancels.len(), 6, "head, gaze, two hands, two controllers");
        for c in &cancels {
            assert!(!c.tracked, "a cancel is a release on a source that is gone");
            assert_eq!(c.button, Some((Button::Select, false)));
        }
    }
}
