//! Touch-class hover emphasis (spatial-input §4 "Hover", ruled; §7 row "gaze (touch-class)").
//!
//! For touch-class sources the client receives nothing before `down`; the *compositor* renders
//! plane-level emphasis of the targeted member — "with a ramp of the HoloLens order (500–1000
//! ms). No cursor." (spatial-input.md:237-240). This file is the pure state: which member is
//! targeted, and an `emphasis ∈ [0, 1]` per member that ramps toward 1 while targeted and back
//! toward 0 after. The presentation — `XR_KHR_composition_layer_color_scale_bias` on the quad
//! layer of the targeted member (Monado: `oxr_extension_support.py:47`) — is applied by the
//! frame procedure from [`Emphasis::emphasis_of`]; nothing here touches the runtime.
//!
//! **Stand-in (flagged):** the ramp is 700 ms, the midpoint of the design's 500–1000 ms HoloLens
//! range (spatial-input §4; research/63 §2's HoloLens evidence). Measured at the M1 gate.
//!
//! Budget: two `(MemberId, f32)` slots and one subtraction per tick; no allocation.

use crate::scene::MemberId;

/// The design's stand-in ramp (spatial-input §4: "500–1000 ms"); 700 ms is a flagged midpoint.
pub const RAMP_NS: u64 = 700_000_000;

/// One member's emphasis level and the direction it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Level {
    member: MemberId,
    value: f32,
}

/// The emphasis state: the member currently targeted (ramping up) and the one most recently
/// released (ramping down). A third target while both slots are busy drops the fading one —
/// two members cannot both be "the target" and a fade under a fade is not worth a list.
#[derive(Debug, Default)]
pub struct Emphasis {
    current: Option<Level>,
    fading: Option<Level>,
    last_ns: Option<u64>,
    ramp_ns: u64,
}

impl Emphasis {
    pub fn new() -> Emphasis {
        Emphasis { current: None, fading: None, last_ns: None, ramp_ns: RAMP_NS }
    }

    pub fn with_ramp_ns(ramp_ns: u64) -> Emphasis {
        Emphasis { ramp_ns: ramp_ns.max(1), ..Emphasis::new() }
    }

    /// The member currently targeted by the touch-class targeting source, or none.
    pub fn target(&self) -> Option<MemberId> {
        self.current.map(|l| l.member)
    }

    /// Set (or clear) the targeted member. Retargeting moves the old target to the fading slot
    /// at its current level; retargeting *back* to the fading member resumes from its level.
    pub fn set_target(&mut self, target: Option<MemberId>) {
        if self.current.map(|l| l.member) == target {
            return;
        }
        let old = self.current.take();
        let resumed = match (target, self.fading) {
            (Some(t), Some(f)) if f.member == t => {
                self.fading = None;
                Some(Level { member: t, value: f.value })
            }
            (Some(t), _) => Some(Level { member: t, value: 0.0 }),
            (None, _) => None,
        };
        if let Some(o) = old {
            if o.value > 0.0 {
                self.fading = Some(o);
            }
        }
        self.current = resumed;
    }

    /// Advance the ramps to `now_ns`. Called once per tick.
    pub fn tick(&mut self, now_ns: u64) {
        let dt = match self.last_ns {
            Some(last) => now_ns.saturating_sub(last),
            None => 0,
        };
        self.last_ns = Some(now_ns);
        if dt == 0 {
            return;
        }
        let step = dt as f32 / self.ramp_ns as f32;
        if let Some(c) = &mut self.current {
            c.value = (c.value + step).min(1.0);
        }
        if let Some(f) = &mut self.fading {
            f.value -= step;
            if f.value <= 0.0 {
                self.fading = None;
            }
        }
    }

    /// The emphasis of a member in `[0, 1]`: its ramp level while targeted or fading, else 0.
    pub fn emphasis_of(&self, member: MemberId) -> f32 {
        if let Some(c) = self.current {
            if c.member == member {
                return c.value;
            }
        }
        if let Some(f) = self.fading {
            if f.member == member {
                return f.value.max(0.0);
            }
        }
        0.0
    }

    /// Whether any member has a non-zero emphasis (the frame procedure may skip the chain).
    pub fn any(&self) -> bool {
        self.current.map(|c| c.value > 0.0).unwrap_or(false) || self.fading.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Flags, Scene, Shape};
    use crate::xr::math;

    fn two_members() -> (MemberId, MemberId) {
        let mut s: Scene<()> = Scene::new();
        let a = s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap();
        let b = s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap();
        (a, b)
    }

    #[test]
    fn ramps_to_one_over_the_stand_in_and_back() {
        let (a, _) = two_members();
        let mut e = Emphasis::new();
        e.tick(0);
        e.set_target(Some(a));
        e.tick(350_000_000);
        assert!((e.emphasis_of(a) - 0.5).abs() < 1e-3, "{}", e.emphasis_of(a));
        e.tick(700_000_000);
        assert!((e.emphasis_of(a) - 1.0).abs() < 1e-6);
        e.tick(2_000_000_000);
        assert_eq!(e.emphasis_of(a), 1.0, "clamped at 1");
        e.set_target(None);
        e.tick(2_350_000_000);
        assert!((e.emphasis_of(a) - 0.5).abs() < 1e-3);
        e.tick(2_700_000_000);
        assert_eq!(e.emphasis_of(a), 0.0);
        assert!(!e.any());
    }

    #[test]
    fn retarget_fades_the_old_and_ramps_the_new() {
        let (a, b) = two_members();
        let mut e = Emphasis::new();
        e.tick(0);
        e.set_target(Some(a));
        e.tick(700_000_000);
        e.set_target(Some(b));
        e.tick(1_050_000_000);
        assert!((e.emphasis_of(a) - 0.5).abs() < 1e-3);
        assert!((e.emphasis_of(b) - 0.5).abs() < 1e-3);
        // back to `a` before its fade ends: resumes from its level
        e.set_target(Some(a));
        assert!((e.emphasis_of(a) - 0.5).abs() < 1e-3);
        assert!((e.emphasis_of(b) - 0.5).abs() < 1e-3);
        e.tick(1_400_000_000);
        assert!((e.emphasis_of(a) - 1.0).abs() < 1e-3);
        assert_eq!(e.emphasis_of(b), 0.0);
    }

    #[test]
    fn untargeted_members_are_zero_and_same_target_is_idempotent() {
        let (a, b) = two_members();
        let mut e = Emphasis::new();
        assert_eq!(e.emphasis_of(a), 0.0);
        e.tick(0);
        e.set_target(Some(a));
        e.tick(350_000_000);
        e.set_target(Some(a));
        assert!((e.emphasis_of(a) - 0.5).abs() < 1e-3, "re-setting the same target does not restart the ramp");
        assert_eq!(e.emphasis_of(b), 0.0);
    }
}
