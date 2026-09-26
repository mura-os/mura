//! "Is this controller in the wearer's hand?" (spatial-input §3 line 197: the controller tier
//! applies "when controllers are **held**"; ADR 0013 amendment item 1: "without eyes and with
//! controllers **in hand**, the controller aim ray targets").
//!
//! **What the standard already has, and why this is a fallback.** OpenXR names the component that
//! answers this directly: `proximity` — "The user is in physical proximity of input source … may:
//! be present for any kind of input source representing a physical component … must: be XR_TRUE if
//! the same input source is returning XR_TRUE for either a 'touch' or any other component that
//! implies physical contact" — and its boolean sibling `touch`, "The user has touched the input
//! source … may: be present for any other kind of input source if the device includes the
//! necessary sensor" (`references/openxr-docs/specification/sources/chapters/semantic_paths.adoc:551-554,
//! 578-588`). That is the capacitive grip/thumbrest sensor every modern profile exposes and it is
//! the *right* answer to "in hand". The guaranteed profile — `khr/simple_controller`, which
//! spatial-input §8 names as the floor — has neither, and today's `Sample` (input/mod.rs) carries
//! no proximity/touch field, so this module implements the fallback the brief specifies:
//! **tracked, and some input active within a timeout**. The report asks for the `Sample` field so
//! the sensor can supersede the heuristic wherever the profile has it.
//!
//! Activity is a button edge, an axis beyond its dead-band, or pose motion above a threshold —
//! WiVRn's controller-activation rule is the nearest comparable: a controller becomes the focused
//! one on a trigger rising edge or when its scroll axis exceeds `scroll_value_thd = 0.01`,
//! otherwise the last one stays (`references/wivrn/client/render/imgui_impl.cpp:726-768`,
//! `references/wivrn/client/constants.h:52-53`). WiVRn has no timeout — it never puts a controller
//! down — so the timeout below is a stand-in with no comparable, flagged.

use super::Sample;

/// Thresholds for "held".
#[derive(Clone, Copy, Debug)]
pub struct HeldCfg {
    /// how long after the last activity a tracked controller still counts as held. **Stand-in
    /// 2 s, flagged: no comparable states one** (WiVRn's focus switch is sticky with no expiry;
    /// xrdesktop's primary controller likewise, `references/xrdesktop/src/xrd-input-synth.c:198-205`).
    pub timeout_ns: u64,
    /// pose displacement between consecutive samples that counts as the wearer moving the
    /// controller, metres. **Invented, flagged: no comparable states one.** It must sit above
    /// tracker jitter and below a held hand's natural tremor; measuring it is an M1 gate item,
    /// and the `proximity`/`touch` component above should replace the whole heuristic.
    pub motion_m: f32,
    /// axis magnitude that counts as activity — WiVRn's `scroll_value_thd`
    /// (`references/wivrn/client/constants.h:52-53`: "Minimum scroll value to enable a controller")
    pub axis_thd: f64,
}

impl Default for HeldCfg {
    fn default() -> Self {
        HeldCfg { timeout_ns: 2_000_000_000, motion_m: 0.005, axis_thd: 0.01 }
    }
}

/// One controller's held state. Two of these live in the arbiter, one per [`super::Side`].
#[derive(Clone, Copy, Debug, Default)]
pub struct HeldTracker {
    /// the runtime reports the aim pose tracked and the device ready
    tracked: bool,
    /// when the wearer was last seen doing something with it; `None` = never
    last_active_ns: Option<u64>,
    last_pos: Option<[f32; 3]>,
    /// set by a `proximity`/`touch` report when one exists (see the module note); overrides the
    /// heuristic while it is `Some`
    in_hand: Option<bool>,
}

impl HeldTracker {
    /// One sample from this controller. `reports_tracking` is the caller's classification (see
    /// `loss::reports_tracking`): a pure device event (a button from libinput with no pose) says
    /// nothing about tracking and must not clear it.
    pub fn observe(&mut self, s: &Sample, reports_tracking: bool, cfg: &HeldCfg) {
        if reports_tracking {
            self.tracked = s.tracked && s.ready;
            if !self.tracked {
                // a controller the runtime stopped tracking is not in a hand; the gesture it may
                // have had in progress is loss.rs's business, not this module's
                self.last_pos = None;
                self.in_hand = None;
                return;
            }
        }

        let mut active = s.button.is_some();
        if let Some((h, v)) = s.axis {
            active |= h.abs() > cfg.axis_thd || v.abs() > cfg.axis_thd;
        }
        if let Some((dx, dy)) = s.delta {
            active |= dx != 0.0 || dy != 0.0;
        }
        if let Some(p) = s.pose {
            let now = [p.position.x, p.position.y, p.position.z];
            if let Some(last) = self.last_pos {
                let d2 = (now[0] - last[0]).powi(2) + (now[1] - last[1]).powi(2) + (now[2] - last[2]).powi(2);
                if d2 > cfg.motion_m * cfg.motion_m {
                    active = true;
                }
            }
            if self.last_pos.is_none() || active {
                self.last_pos = Some(now);
            }
        }
        if active {
            self.last_active_ns = Some(s.time_ns);
        }
    }

    /// The profile's `proximity`/`touch` answer, when the device has the sensor and lane D binds
    /// it. While set it decides, and the activity heuristic is not consulted.
    pub fn set_in_hand(&mut self, in_hand: bool, now_ns: u64) {
        self.in_hand = Some(in_hand);
        if in_hand {
            self.last_active_ns = Some(now_ns);
        }
    }

    /// Held = tracked **and** (the sensor says so, or activity within the timeout).
    pub fn held(&self, now_ns: u64, cfg: &HeldCfg) -> bool {
        if !self.tracked {
            return false;
        }
        if let Some(in_hand) = self.in_hand {
            return in_hand;
        }
        match self.last_active_ns {
            Some(t) => now_ns.saturating_sub(t) < cfg.timeout_ns,
            None => false,
        }
    }

    pub fn tracked(&self) -> bool {
        self.tracked
    }

    pub fn last_active_ns(&self) -> Option<u64> {
        self.last_active_ns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Button, Side, SourceKind};
    use openxr as xr;

    fn ctrl(now: u64) -> Sample {
        let mut s = Sample::new(SourceKind::Controller(Side::Right), now);
        s.pose = Some(xr::Posef::IDENTITY);
        s.tracked = true;
        s.ready = true;
        s.quality = crate::input::Quality::Nominal;
        s
    }

    fn at(now: u64, pos: [f32; 3]) -> Sample {
        let mut s = ctrl(now);
        s.pose = Some(xr::Posef { orientation: xr::Quaternionf::IDENTITY, position: xr::Vector3f { x: pos[0], y: pos[1], z: pos[2] } });
        s
    }

    #[test]
    fn a_tracked_but_untouched_controller_is_not_held() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        h.observe(&ctrl(0), true, &cfg);
        h.observe(&ctrl(1_000_000_000), true, &cfg);
        assert!(h.tracked());
        assert!(!h.held(1_000_000_000, &cfg), "a controller on the table is tracked, not held");
    }

    #[test]
    fn a_button_makes_it_held_and_the_timeout_releases_it() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        let mut s = ctrl(1_000_000_000);
        s.button = Some((Button::Select, true));
        h.observe(&s, true, &cfg);
        assert!(h.held(1_000_000_000, &cfg));
        assert!(h.held(2_999_000_000, &cfg), "still held just inside the 2 s stand-in");
        assert!(!h.held(3_000_000_000, &cfg), "put down: the tier is released at the timeout");
    }

    #[test]
    fn pose_motion_above_the_threshold_counts_as_activity() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        h.observe(&at(0, [0.0, 0.0, 0.0]), true, &cfg);
        // jitter below the threshold is not activity
        h.observe(&at(100_000_000, [0.001, 0.0, 0.0]), true, &cfg);
        assert!(!h.held(100_000_000, &cfg));
        // a real move is
        h.observe(&at(200_000_000, [0.02, 0.0, 0.0]), true, &cfg);
        assert!(h.held(200_000_000, &cfg));
        assert!(!h.held(2_200_000_000, &cfg));
    }

    #[test]
    fn axis_activity_uses_wivrns_dead_band() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        let mut s = ctrl(0);
        s.axis = Some((0.005, 0.0));
        h.observe(&s, true, &cfg);
        assert!(!h.held(0, &cfg), "below scroll_value_thd = 0.01");
        let mut s = ctrl(1_000);
        s.axis = Some((0.5, 0.0));
        h.observe(&s, true, &cfg);
        assert!(h.held(1_000, &cfg));
    }

    #[test]
    fn losing_tracking_releases_the_tier_immediately() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        let mut s = ctrl(0);
        s.button = Some((Button::Select, true));
        h.observe(&s, true, &cfg);
        assert!(h.held(0, &cfg));
        let mut off = ctrl(10_000_000);
        off.pose = None;
        off.tracked = false;
        off.ready = false;
        h.observe(&off, true, &cfg);
        assert!(!h.held(10_000_000, &cfg));
    }

    #[test]
    fn a_device_event_without_a_pose_does_not_clear_tracking() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        h.observe(&at(0, [0.0; 3]), true, &cfg);
        let mut ev = Sample::new(SourceKind::Controller(Side::Right), 5_000_000);
        ev.button = Some((Button::Menu, true));
        h.observe(&ev, false, &cfg);
        assert!(h.tracked());
        assert!(h.held(5_000_000, &cfg));
    }

    #[test]
    fn the_proximity_sensor_decides_when_the_profile_has_one() {
        let cfg = HeldCfg::default();
        let mut h = HeldTracker::default();
        h.observe(&ctrl(0), true, &cfg);
        h.set_in_hand(true, 0);
        assert!(h.held(60_000_000_000, &cfg), "no timeout applies while the sensor answers");
        h.set_in_hand(false, 0);
        assert!(!h.held(0, &cfg));
    }
}
