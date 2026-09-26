//! Source loss and cancel semantics (spatial-input §3 lines 210-213: "a change never happens
//! mid-gesture"; §9 lines 357-359: the fallback is "surfaced as the tier change it is; never a
//! guessed pose").
//!
//! A hand that leaves the cameras' view, a controller that is set down, an eye tracker that drops
//! out — each can do so *while the wearer is committing*. The rule this module enforces: **the
//! gesture ends before the tier moves**. Concretely, when a kind with a commit in progress stops
//! being tracked, a synthetic *release* is emitted for that kind and the tier transition is held
//! back until that release has passed through the chain. Without it the client would be left with
//! an unmatched `wl_touch.down` / button press — the state kwin-vr's rule ("do not stop movement
//! when we already started", spatial-input §3 line 211) and MRTK3's near-mode latch both avoid.
//!
//! **Transport note for lane C:** a `Button::Select` *release* sample whose pose is untracked is a
//! release **after loss**, and is the input the transport maps to `wl_touch.cancel` for
//! touch-class kinds (the protocol's "the compositor has cancelled the touch sequence") and to an
//! ordinary button release for pointer-class ones. `released_after_loss()` names it.
//!
//! Gesture thresholds come from the comparables: the analog commit threshold is WiVRn's
//! `trigger_click_thd = 0.7` ("Threshold on the trigger value to register a click",
//! `references/wivrn/client/constants.h:42-43`, applied at `client/render/imgui_impl.cpp:645`) —
//! and `pinch_ext/value` is the same shape of 0..1 analog, where "the value XR_TRUE or 1.0f
//! represents that the finger and thumb are touching each other" and the value "should: be linear
//! to the distance between the finger and thumb tips"
//! (`references/openxr-docs/specification/sources/chapters/extensions/ext/ext_hand_interaction.adoc:314-330`).
//! The poke threshold is WiVRn's fingertip band (`constants.h:48-49`).

use super::{Button, Sample, SourceKind};

/// The number of kinds; the index of [`SourceKind::ALL`] is the index here.
pub const KINDS: usize = SourceKind::ALL.len();

/// Dense index of a kind, for the fixed-size per-kind tables the arbiter keeps (no allocation per
/// event — AGENTS rule 6).
pub fn kind_index(k: SourceKind) -> usize {
    SourceKind::ALL.iter().position(|x| *x == k).expect("SourceKind::ALL is the closed set")
}

/// The XR kinds that carry a tracked pose. libinput kinds (`Pointer`, `Keyboard`) never set
/// `tracked`, so "untracked" says nothing about them and they are exempt from loss detection.
pub fn is_spatial(k: SourceKind) -> bool {
    matches!(k, SourceKind::Head | SourceKind::Gaze | SourceKind::Hand(_) | SourceKind::Controller(_))
}

/// Whether a sample is a *tracking report* — a per-tick state sample from the runtime — rather
/// than a pure device event. An `hmdButtons` press arrives on `SourceKind::Head` through libinput
/// with no pose and says nothing about head tracking; an `Off`/untracked state sample does.
pub fn reports_tracking(s: &Sample) -> bool {
    is_spatial(s.kind) && (s.pose.is_some() || !s.is_event())
}

/// Whether a sample is the release that follows a loss — the transport's `wl_touch.cancel` case.
pub fn released_after_loss(s: &Sample) -> bool {
    s.button == Some((Button::Select, false)) && !s.tracked
}

/// Commit thresholds, with hysteresis so a value sitting on the boundary does not chatter.
#[derive(Clone, Copy, Debug)]
pub struct GestureCfg {
    /// `pinch_ext/value` (or a trigger) at or above this is a commit — WiVRn's
    /// `trigger_click_thd` (`references/wivrn/client/constants.h:42-43`)
    pub pinch_close: f32,
    /// …and it is released below this. **Stand-in, flagged**: WiVRn states only the one
    /// threshold; the release value is MRTK3's select progress **0.5** (spatial-input §4's sticky
    /// hover), composed with WiVRn's close value by this lane.
    pub pinch_open: f32,
    /// poke depth, metres along the plane normal, negative behind the plane: a crossing at or
    /// past this is `down` — WiVRn's `fingertip_distance_touching_thd_hi = -0.01` ("Distance to
    /// register a click", `references/wivrn/client/constants.h:48-49`)
    pub poke_down_m: f32,
    /// …and the finger is up once it is back in front of the plane. **Stand-in, flagged**: the
    /// 1 cm of hysteresis is WiVRn's own click distance read as a band, with spatial-input §3's
    /// StereoKit rule ("the focus volume grows once focused so a finger passing through the plane
    /// does not lose the target") as the reason for the deeper release.
    pub poke_up_m: f32,
}

impl Default for GestureCfg {
    fn default() -> Self {
        GestureCfg { pinch_close: 0.7, pinch_open: 0.5, poke_down_m: -0.01, poke_up_m: 0.0 }
    }
}

/// What a kind is committing with, now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gesture {
    /// the profile's `select` / `aim_activate` / `hmdButtons.selectRole` is down
    pub select: bool,
    /// `pinch_ext/value` is above the commit threshold
    pub pinch: bool,
    /// the poke tip has crossed the plane in the normal's direction
    pub poke: bool,
    /// a release has been synthesised for this kind and has not come back through the chain yet;
    /// the tier stays put until it has (the "before any tier change" of the brief)
    pub pending_release: bool,
}

impl Gesture {
    pub fn active(&self) -> bool {
        self.select || self.pinch || self.poke
    }
    /// Whether this kind blocks a tier transition: a live commit, or one being torn down.
    pub fn blocks(&self) -> bool {
        self.active() || self.pending_release
    }
}

/// Per-kind gesture state plus the loss→cancel rule. Fixed-size tables; no allocation per event.
#[derive(Clone, Debug)]
pub struct LossTracker {
    pub cfg: GestureCfg,
    g: [Gesture; KINDS],
    last_pose: [Option<openxr::Posef>; KINDS],
    /// kinds that lost tracking while committing (journal)
    pub losses: u64,
    /// synthetic releases emitted (equal to `losses` unless a pending one expired)
    pub releases: u64,
}

impl Default for LossTracker {
    fn default() -> Self {
        LossTracker { cfg: GestureCfg::default(), g: [Gesture::default(); KINDS], last_pose: [None; KINDS], losses: 0, releases: 0 }
    }
}

impl LossTracker {
    /// Fold one sample into the gesture state. Returns the synthetic release to queue when this
    /// sample is a loss that interrupted a commit — at most one per loss.
    pub fn observe(&mut self, s: &Sample) -> Option<Sample> {
        let i = kind_index(s.kind);
        let report = reports_tracking(s);
        let lost = report && (!s.tracked || !s.ready || s.pose.is_none());
        if report {
            if let Some(p) = s.pose {
                self.last_pose[i] = Some(p);
            }
        }

        // a release coming back through the chain — ours or the device's — closes the tear-down
        if s.button == Some((Button::Select, false)) {
            self.g[i].pending_release = false;
            self.g[i].select = false;
        }
        // …but a *press* on a source the runtime is not tracking never starts a commit: an
        // untracked source has nothing to commit at (§9: never a guessed pose)
        if !lost {
            if s.button == Some((Button::Select, true)) {
                self.g[i].select = true;
            }
            // Level-triggered analog values, with hysteresis (adoc:314-330; constants.h:42-43),
            // and only while the hand is `ready_ext`: the standard requires `pinch_ext/value` to
            // read 0 when `ready_ext` is false (`ext_hand_interaction.adoc:334-338`), so folding
            // an unready hand's zeros would silently *end* a gesture that the loss branch below
            // must *cancel* instead.
            if matches!(s.kind, SourceKind::Hand(_)) && s.ready {
                if self.g[i].pinch {
                    self.g[i].pinch = s.values.pinch > self.cfg.pinch_open;
                } else {
                    self.g[i].pinch = s.values.pinch >= self.cfg.pinch_close;
                }
                if self.g[i].poke {
                    self.g[i].poke = s.values.poke <= self.cfg.poke_up_m;
                } else {
                    self.g[i].poke = s.values.poke <= self.cfg.poke_down_m;
                }
            }
        }

        if !lost {
            return None;
        }
        if !self.g[i].active() {
            // nothing to cancel; drop any stale analog state the untracked sample carried
            self.g[i].select = false;
            self.g[i].pinch = false;
            self.g[i].poke = false;
            return None;
        }
        // the gesture ends before the tier moves
        self.g[i] = Gesture { select: false, pinch: false, poke: false, pending_release: true };
        self.losses += 1;
        self.releases += 1;
        let mut rel = Sample::new(s.kind, s.time_ns);
        rel.button = Some((Button::Select, false));
        // the last good pose travels with it so the transport can place the cancel; the tracked
        // bits stay clear, so nothing downstream aims with it (§9: never a guessed pose)
        rel.pose = self.last_pose[i];
        rel.xr_time = s.xr_time;
        Some(rel)
    }

    pub fn gesture(&self, k: SourceKind) -> Gesture {
        self.g[kind_index(k)]
    }

    /// Any kind committing now. The tier may not move while this holds (§3 line 211): the commit
    /// device need not be the targeting kind (ADR 0013 amendment item 1 — a controller trigger
    /// commits at the gaze target), so a commit on *any* kind pins the target.
    pub fn any_blocks(&self) -> bool {
        self.g.iter().any(|g| g.blocks())
    }

    pub fn any_pending(&self) -> bool {
        self.g.iter().any(|g| g.pending_release)
    }

    /// Safety valve: a pending release that never came back (the queue was dropped at a mode
    /// change) must not pin the tier for ever. The arbiter calls this after two ticks — the
    /// release is queued during one tick's drain and consumed in the next one's.
    pub fn expire_pending(&mut self) -> u64 {
        let mut n = 0;
        for g in self.g.iter_mut() {
            if g.pending_release {
                g.pending_release = false;
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Quality, Side, Values};
    use openxr as xr;

    fn tracked(kind: SourceKind, now: u64) -> Sample {
        let mut s = Sample::new(kind, now);
        s.pose = Some(xr::Posef::IDENTITY);
        s.tracked = true;
        s.ready = true;
        s.quality = Quality::Nominal;
        s
    }

    fn off(kind: SourceKind, now: u64) -> Sample {
        Sample::new(kind, now)
    }

    #[test]
    fn indices_cover_the_closed_set() {
        for (i, k) in SourceKind::ALL.iter().enumerate() {
            assert_eq!(kind_index(*k), i);
        }
        assert!(is_spatial(SourceKind::Gaze) && is_spatial(SourceKind::Head));
        assert!(!is_spatial(SourceKind::Pointer) && !is_spatial(SourceKind::Keyboard));
    }

    #[test]
    fn a_libinput_button_on_the_head_kind_is_not_a_tracking_report() {
        // hmdButtons arrive on SourceKind::Head through libinput with no pose (device-contract A2)
        let mut s = Sample::new(SourceKind::Head, 0);
        s.button = Some((Button::Select, true));
        assert!(!reports_tracking(&s));
        let mut l = LossTracker::default();
        assert!(l.observe(&s).is_none(), "a button press must never read as a head-tracking loss");
        assert!(l.gesture(SourceKind::Head).select);
    }

    #[test]
    fn loss_during_a_button_commit_emits_exactly_one_release() {
        let k = SourceKind::Controller(Side::Right);
        let mut l = LossTracker::default();
        let mut down = tracked(k, 0);
        down.button = Some((Button::Select, true));
        assert!(l.observe(&down).is_none());
        assert!(l.gesture(k).active() && l.any_blocks());

        let rel = l.observe(&off(k, 1_000_000)).expect("a release is synthesised");
        assert_eq!(rel.kind, k);
        assert_eq!(rel.button, Some((Button::Select, false)));
        assert!(!rel.tracked && released_after_loss(&rel), "lane C maps this to wl_touch.cancel");
        assert_eq!(l.losses, 1);
        assert!(!l.gesture(k).active());
        assert!(l.any_pending() && l.any_blocks(), "the tier stays put until the release lands");

        // a second untracked sample does not emit a second release
        assert!(l.observe(&off(k, 2_000_000)).is_none());
        assert_eq!(l.releases, 1);

        // …and the release itself, coming back through the chain, clears the block
        assert!(l.observe(&rel).is_none());
        assert!(!l.any_pending() && !l.any_blocks());
    }

    #[test]
    fn loss_without_a_gesture_emits_nothing() {
        let k = SourceKind::Hand(Side::Left);
        let mut l = LossTracker::default();
        assert!(l.observe(&tracked(k, 0)).is_none());
        assert!(l.observe(&off(k, 1)).is_none());
        assert_eq!(l.losses, 0);
        assert!(!l.any_blocks());
    }

    #[test]
    fn pinch_uses_wivrns_click_threshold_with_hysteresis() {
        let k = SourceKind::Hand(Side::Right);
        let mut l = LossTracker::default();
        let mut s = tracked(k, 0);
        s.values = Values { pinch: 0.69, ..Values::default() };
        l.observe(&s);
        assert!(!l.gesture(k).pinch, "0.69 is below trigger_click_thd = 0.7");
        s.values.pinch = 0.7;
        l.observe(&s);
        assert!(l.gesture(k).pinch);
        s.values.pinch = 0.6;
        l.observe(&s);
        assert!(l.gesture(k).pinch, "held through the hysteresis band");
        s.values.pinch = 0.4;
        l.observe(&s);
        assert!(!l.gesture(k).pinch);
    }

    #[test]
    fn poke_down_is_a_plane_crossing_and_survives_going_deeper() {
        let k = SourceKind::Hand(Side::Left);
        let mut l = LossTracker::default();
        let mut s = tracked(k, 0);
        s.values.poke = 0.05;
        l.observe(&s);
        assert!(!l.gesture(k).poke);
        s.values.poke = -0.02;
        l.observe(&s);
        assert!(l.gesture(k).poke, "crossed the plane (constants.h:48)");
        s.values.poke = -0.12;
        l.observe(&s);
        assert!(l.gesture(k).poke, "StereoKit's grown focus volume: deeper is still down");
        s.values.poke = 0.01;
        l.observe(&s);
        assert!(!l.gesture(k).poke);
    }

    #[test]
    fn a_pinch_interrupted_by_loss_is_cancelled_too() {
        let k = SourceKind::Hand(Side::Right);
        let mut l = LossTracker::default();
        let mut s = tracked(k, 0);
        s.values.pinch = 0.9;
        l.observe(&s);
        assert!(l.gesture(k).pinch);
        let rel = l.observe(&off(k, 5)).expect("hands out of view cancel their pinch");
        assert_eq!(rel.button, Some((Button::Select, false)));
        assert_eq!(l.losses, 1);
    }

    #[test]
    fn a_pending_release_expires_rather_than_pinning_the_tier_for_ever() {
        let k = SourceKind::Hand(Side::Right);
        let mut l = LossTracker::default();
        let mut s = tracked(k, 0);
        s.values.pinch = 0.9;
        l.observe(&s);
        l.observe(&off(k, 5)).unwrap();
        assert!(l.any_pending());
        assert_eq!(l.expire_pending(), 1);
        assert!(!l.any_pending() && !l.any_blocks());
    }
}
