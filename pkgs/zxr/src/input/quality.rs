//! Gaze quality (spatial-input §3 lines 194-199, §9 lines 350-362; ADR 0013 amendment item 1).
//!
//! The extension states the rule this module encodes: the tracking state of position and
//! direction is coupled — a runtime must set both tracked bits or clear both
//! (`references/openxr-docs/specification/sources/chapters/extensions/ext/ext_eye_gaze_interaction.adoc:139-146`)
//! — and "a nominal eye gaze pose is suitable for use cases such as aiming or targeting, while a
//! sub-nominal eye gaze pose has degraded performance and should: not be relied on"; applications
//! "should: be very careful when using sub-nominal eye gaze pose, since behavior varies
//! considerably for different users and manufacturers" (`:147-158`). So **only a nominal pose
//! aims**, and a sub-nominal one never does.
//!
//! Falling out of the gaze tier is *not* the same event as losing the nominal pose. §3 keeps gaze
//! targeting until gaze has been "absent or sub-nominal for longer than the fallback timeout",
//! and §9 requires the fallback be "surfaced as the tier change it is; never a guessed pose". The
//! two are separated here: [`GazeClassifier::pose_usable`] goes false the instant quality drops
//! (nothing guesses a ray), while [`GazeClassifier::holds_tier`] stays true for the timeout so the
//! wearer's blink does not throw them down a tier and back.
//!
//! The timeout must therefore exceed a blink (100–400 ms), which is a unit-test property below.

use super::{Quality, Sample};

/// A blink's duration, the floor the fallback timeout must clear. The 100–400 ms range is the
/// brief's (design §3's stand-in band is 500–1500 ms, "HoloLens' order", which clears it).
pub const BLINK_MIN_NS: u64 = 100_000_000;
pub const BLINK_MAX_NS: u64 = 400_000_000;

/// The eyes→head fallback timeout and the return window.
#[derive(Clone, Copy, Debug)]
pub struct GazeCfg {
    /// how long gaze may be sub-nominal/lost before the tier falls (spatial-input §3: stand-in
    /// **500–1500 ms**, HoloLens' order) — **800 ms used, flagged**: the design gives a band, not
    /// a number, and no comparable in `references/` states one for an eyes→head fallback.
    pub fallback_ns: u64,
    /// how long gaze must be *continuously* nominal before it may (re)take the tier. **Stand-in =
    /// `fallback_ns`, flagged**: the design names the return hysteresis but gives it no number,
    /// and no comparable states one. Symmetry with the drop is the only argument for this value.
    pub return_ns: u64,
}

impl Default for GazeCfg {
    fn default() -> Self {
        GazeCfg { fallback_ns: 800_000_000, return_ns: 800_000_000 }
    }
}

impl GazeCfg {
    /// The property §9's fallback rests on: the wearer's blink must not cost them the tier.
    pub fn exceeds_blink(&self) -> bool {
        self.fallback_ns > BLINK_MAX_NS
    }
}

/// What the eye tracker is doing now — the extension's own four states.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GazeState {
    /// both tracked bits set and the runtime called it nominal: the only state that aims
    Nominal,
    /// the runtime cleared `POSITION_TRACKED_BIT` but still offers a pose (`adoc:150-158`)
    SubNominal,
    /// tracked but no usable pose, or no sample within the staleness window
    #[default]
    Lost,
    /// no eye tracker, or the profile is not bound (the contract's eye-tracking class absent)
    Unavailable,
}

/// The classifier: quality in, "may gaze aim" and "does gaze hold the tier" out.
#[derive(Clone, Debug)]
pub struct GazeClassifier {
    pub cfg: GazeCfg,
    state: GazeState,
    /// the tier claim: survives a drop for `fallback_ns`, and is only (re)taken after `return_ns`
    holds: bool,
    /// start of the current uninterrupted nominal run, if any
    nominal_since: Option<u64>,
    /// start of the current uninterrupted non-nominal run, if any
    degraded_since: Option<u64>,
}

impl Default for GazeClassifier {
    fn default() -> Self {
        GazeClassifier::new(GazeCfg::default())
    }
}

impl GazeClassifier {
    pub fn new(cfg: GazeCfg) -> Self {
        debug_assert!(cfg.exceeds_blink(), "the eyes→head timeout must outlast a blink (§9)");
        GazeClassifier { cfg, state: GazeState::Lost, holds: false, nominal_since: None, degraded_since: None }
    }

    /// One gaze sample. `Quality::Nominal` counts only with both tracked bits set — the
    /// extension couples them (`ext_eye_gaze_interaction.adoc:139-146`).
    pub fn observe(&mut self, s: &Sample, now_ns: u64) {
        let state = match s.quality {
            Quality::Nominal if s.tracked && s.pose.is_some() => GazeState::Nominal,
            Quality::Nominal => GazeState::Lost,
            Quality::SubNominal => GazeState::SubNominal,
            Quality::Lost => GazeState::Lost,
            // a tracker that disappears mid-session is a loss, not a capability statement, and
            // §9's fallback is the same one; treated alike, flagged as a judgment
            Quality::Unavailable => GazeState::Unavailable,
        };
        self.set(state, now_ns);
    }

    /// No gaze sample arrived within the staleness window: the same as a loss.
    pub fn stale(&mut self, now_ns: u64) {
        self.set(GazeState::Lost, now_ns);
    }

    /// Timers only — called from the stage's per-tick hook so the fallback fires without a sample.
    pub fn tick(&mut self, now_ns: u64) {
        self.settle(now_ns);
    }

    fn set(&mut self, state: GazeState, now_ns: u64) {
        if state == GazeState::Nominal {
            self.degraded_since = None;
            if self.nominal_since.is_none() {
                self.nominal_since = Some(now_ns);
            }
        } else {
            self.nominal_since = None;
            if self.degraded_since.is_none() {
                self.degraded_since = Some(now_ns);
            }
        }
        self.state = state;
        self.settle(now_ns);
    }

    fn settle(&mut self, now_ns: u64) {
        if self.holds {
            if let Some(d) = self.degraded_since {
                if now_ns.saturating_sub(d) >= self.cfg.fallback_ns {
                    self.holds = false;
                }
            }
        } else if let Some(n) = self.nominal_since {
            if now_ns.saturating_sub(n) >= self.cfg.return_ns {
                self.holds = true;
            }
        }
    }

    pub fn state(&self) -> GazeState {
        self.state
    }

    /// Whether gaze may be the targeting tier now (§3 tier 1; the timeout of §3 line 199 and the
    /// return window both live here).
    pub fn holds_tier(&self) -> bool {
        self.holds
    }

    /// Whether the *pose* may aim: nominal only (`ext_eye_gaze_interaction.adoc:147-158`). During
    /// the fallback grace gaze still holds the tier but nothing aims with a sub-nominal ray —
    /// §4's target lock holds the last nominal target instead, and §9 forbids a guessed pose.
    pub fn pose_usable(&self) -> bool {
        self.state == GazeState::Nominal
    }

    /// Nanoseconds the current non-nominal run has lasted, for the stage's journal line.
    pub fn degraded_for(&self, now_ns: u64) -> u64 {
        self.degraded_since.map(|d| now_ns.saturating_sub(d)).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::SourceKind;

    fn gaze(quality: Quality, now: u64) -> Sample {
        let mut s = Sample::new(SourceKind::Gaze, now);
        s.pose = Some(openxr::Posef::IDENTITY);
        s.quality = quality;
        s.tracked = quality == Quality::Nominal;
        s.ready = matches!(quality, Quality::Nominal | Quality::SubNominal);
        s
    }

    /// §9's property, stated as a test: the fallback must outlast a blink, or a wearer blinking
    /// drops a tier and climbs back. Blinks are 100–400 ms.
    #[test]
    fn the_fallback_timeout_outlasts_a_blink() {
        let cfg = GazeCfg::default();
        assert!(cfg.fallback_ns > BLINK_MAX_NS, "800 ms must exceed the 400 ms blink ceiling");
        assert!(cfg.exceeds_blink());
        // and it sits inside §3's stand-in band, 500–1500 ms
        assert!((500_000_000..=1_500_000_000).contains(&cfg.fallback_ns));
        assert!(BLINK_MIN_NS < BLINK_MAX_NS);
        assert!(!GazeCfg { fallback_ns: BLINK_MAX_NS, return_ns: 0 }.exceeds_blink());
    }

    #[test]
    fn nominal_must_be_stable_for_the_return_window_before_gaze_targets() {
        let mut c = GazeClassifier::default();
        c.observe(&gaze(Quality::Nominal, 0), 0);
        assert!(!c.holds_tier(), "gaze does not take the tier on its first nominal sample");
        assert!(c.pose_usable());
        c.observe(&gaze(Quality::Nominal, 700_000_000), 700_000_000);
        assert!(!c.holds_tier());
        c.observe(&gaze(Quality::Nominal, 800_000_000), 800_000_000);
        assert!(c.holds_tier(), "…and takes it after a stable-nominal window");
    }

    #[test]
    fn a_blink_does_not_drop_the_tier_but_does_stop_the_ray() {
        let mut c = GazeClassifier::default();
        c.observe(&gaze(Quality::Nominal, 0), 0);
        c.observe(&gaze(Quality::Nominal, 800_000_000), 800_000_000);
        assert!(c.holds_tier());
        // a 300 ms blink: lost, then nominal again
        c.observe(&gaze(Quality::Lost, 900_000_000), 900_000_000);
        assert!(c.holds_tier(), "the tier survives the blink");
        assert!(!c.pose_usable(), "but no ray is guessed from a lost pose (§9)");
        c.observe(&gaze(Quality::Nominal, 1_200_000_000), 1_200_000_000);
        assert!(c.holds_tier());
        assert!(c.pose_usable());
    }

    #[test]
    fn sub_nominal_never_aims_and_falls_back_after_the_timeout() {
        let mut c = GazeClassifier::default();
        c.observe(&gaze(Quality::Nominal, 0), 0);
        c.observe(&gaze(Quality::Nominal, 800_000_000), 800_000_000);
        assert!(c.holds_tier());
        let t0 = 1_000_000_000;
        c.observe(&gaze(Quality::SubNominal, t0), t0);
        assert!(!c.pose_usable(), "sub-nominal never targets (adoc:147-158)");
        assert!(c.holds_tier());
        c.tick(t0 + 799_000_000);
        assert!(c.holds_tier(), "still inside the fallback timeout");
        c.tick(t0 + 800_000_000);
        assert!(!c.holds_tier(), "the eyes→head fallback fires at the timeout");
        // and the return is not instantaneous
        c.observe(&gaze(Quality::Nominal, t0 + 900_000_000), t0 + 900_000_000);
        assert!(!c.holds_tier());
        c.tick(t0 + 1_700_000_000);
        assert!(c.holds_tier());
    }

    #[test]
    fn staleness_and_unavailable_behave_as_a_loss() {
        let mut c = GazeClassifier::default();
        c.observe(&gaze(Quality::Nominal, 0), 0);
        c.observe(&gaze(Quality::Nominal, 800_000_000), 800_000_000);
        c.stale(1_000_000_000);
        assert_eq!(c.state(), GazeState::Lost);
        c.tick(1_800_000_000);
        assert!(!c.holds_tier());

        let mut c = GazeClassifier::default();
        c.observe(&gaze(Quality::Unavailable, 0), 0);
        c.tick(10_000_000_000);
        assert_eq!(c.state(), GazeState::Unavailable);
        assert!(!c.holds_tier(), "no eye tracker: gaze never takes the tier");
        assert!(!c.pose_usable());
    }

    #[test]
    fn a_nominal_quality_without_the_tracked_bits_is_not_nominal() {
        // the extension couples the bits (adoc:139-146): a pose claiming nominal with the bits
        // clear is a runtime bug, and is read as lost rather than aimed with
        let mut c = GazeClassifier::default();
        let mut s = gaze(Quality::Nominal, 0);
        s.tracked = false;
        c.observe(&s, 0);
        assert_eq!(c.state(), GazeState::Lost);
        assert!(!c.pose_usable());
    }
}
