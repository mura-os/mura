//! `Slot::A11y` — the accessibility transforms (spatial-input.md §13 lines 417-437, §14 line 440's
//! settings; §1a line 117 "a11y — dwell-as-commit, sticky/slow keys, pointer gain — transforms on
//! raw events", and line 84's placement row: "in-compositor pipeline stages, ahead of the
//! lock/greeter mode… KWin's filter order puts them first").
//!
//! **Why here.** §13 lines 426-430: the transforms "must shape raw events before any policy sees
//! them and must keep working on a locked screen". KWin's order is the same and is what this slot
//! ports: `SlowKeys, BounceKeys, StickyKeys, MouseKeys, DwellClicker, EisInput` ahead of
//! `VirtualTerminal, LockScreen` (`references/kwin/src/input.h:366-393`). zxr's one divergence is
//! that `Slot::Reserved` is ahead of this slot rather than behind it (see `reserved.rs`).
//!
//! **What is here now**
//! - **Dwell as a commit method on any tier** (§13 line 421). The stage shape is KWin's dwell
//!   clicker: a single-shot delay timer, then a dwell animation, then the click, with motion past
//!   a threshold resetting the lot (`references/kwin/src/plugins/dwellclicker/dwellclicker.cpp:91-114`
//!   for delay→dwell→click, `:150-153` for the two intervals being separate settings, `:163-183`
//!   for start/stop). KWin injects the click through an input device of its own
//!   (`dwellclicker.cpp:194` `input()->addInputDevice(m_device.get())`); zxr's equivalent is a
//!   `Button::Select` press and release queued as samples of the dwelling kind, so the whole chain
//!   below — stabilize, tier, hit, grabs, seat — treats the dwell commit exactly like a physical
//!   one. Off by default.
//! - **Pointer gain** (§14 `input.pointer.gain`; spec §8 "libinput flat profile + compositor
//!   gain"): a multiplier on `SourceKind::Pointer` deltas and nothing else.
//!
//! **What is placement only — future stages of this slot, in KWin's order**
//! (`references/kwin/src/input.h:366-393`, and each plugin's own directory):
//! - sticky keys — `references/kwin/src/plugins/stickykeys/`
//! - slow keys — `references/kwin/src/plugins/slowkeys/`
//! - bounce keys — `references/kwin/src/plugins/bouncekeys/`
//! - mouse keys (`MouseKeys` in the order; pointer motion from the keypad)
//!
//! They are key transforms with nothing XR-specific about them; they land with lane F's keyboard
//! path, as separate stages of this slot in the order above, and are **not** implemented here.

use super::{Button, Flow, Sample, SourceKind, Stage};
use crate::state::Zxr;
use openxr as xr;

/// dwell onset — the delay before the dwell itself starts (KWin's `delayTime`,
/// `dwellclicker.cpp:150`). **Stand-in:** 200 ms, the middle of §13's 150-250 ms (HoloLens;
/// research/42 §5 lines 525-528's literature range). Flagged: a range, not a measurement.
pub const ONSET_NS: u64 = 200_000_000;
/// the dwell proper (KWin's `dwellTime`, `dwellclicker.cpp:152`). **Stand-in:** 750 ms, the middle
/// of §13's 650-850 ms; research/42 §5 line 525 records "~600 ms generic, 400 ms for
/// alphanumerics, 800 ms for icons, ≥ 1000 ms judged unusable" and Rajanna & Hansen's 550 ms in
/// VR. Flagged.
pub const DWELL_NS: u64 = 750_000_000;
/// movement tolerance for a pose source, as the cosine of the angle between the ray now and the ray
/// at the anchor. **Stand-in:** 2° of visual angle (§13 line 422 requires "a movement tolerance"
/// and gives no number; 2° is the order of KWin's pixel threshold at a panel's arm's length).
/// Flagged.
pub const TOLERANCE_COS: f32 = 0.999_390_8;
/// movement tolerance for `SourceKind::Pointer`, in accumulated device units. **Stand-in:** 20,
/// the order of KWin's dwell-clicker motion threshold. Flagged.
pub const TOLERANCE_PX: f64 = 20.0;

/// The ray a pose sample points along: -Z rotated by the orientation (OpenXR's convention).
fn forward(q: xr::Quaternionf) -> [f32; 3] {
    let (x, y, z, w) = (q.x, q.y, q.z, q.w);
    [-2.0 * (x * z + w * y), 2.0 * (w * x - y * z), -(1.0 - 2.0 * (x * x + y * y))]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Anchor {
    /// a pose source: the ray it held when the settle began
    Dir([f32; 3]),
    /// `SourceKind::Pointer`: the accumulated position where the settle began
    Px(f64, f64),
}

/// The dwell state machine, pure: one targeting source at a time (the tier rule guarantees that,
/// §3), a settle anchor, and the two intervals. Fires once per settle and rearms only on movement
/// past the tolerance — KWin's `start()`/`stop()` pair (`dwellclicker.cpp:163-183`).
#[derive(Default)]
pub struct Dwell {
    kind: Option<SourceKind>,
    anchor: Option<Anchor>,
    /// running position of the pointer, in accumulated device units
    px: (f64, f64),
    settled_at: u64,
    fired: bool,
}

impl Dwell {
    fn rearm(&mut self, a: Anchor, now_ns: u64) {
        self.anchor = Some(a);
        self.settled_at = now_ns;
        self.fired = false;
    }

    pub fn reset(&mut self) {
        self.anchor = None;
        self.fired = false;
    }

    /// Advance the machine with one sample. `true` means a commit is due now.
    pub fn step(&mut self, s: &Sample, now_ns: u64) -> bool {
        let here = match s.kind {
            SourceKind::Pointer => {
                let (dx, dy) = s.delta.unwrap_or((0.0, 0.0));
                self.px.0 += dx;
                self.px.1 += dy;
                Anchor::Px(self.px.0, self.px.1)
            }
            k if k.targets() => {
                // an untracked source is not dwelling on anything
                let Some(p) = s.pose.filter(|_| s.tracked) else {
                    self.reset();
                    return false;
                };
                Anchor::Dir(forward(p.orientation))
            }
            // keyboards do not dwell (§13's dwell is a *commit method on a tier*)
            _ => return false,
        };
        if self.kind != Some(s.kind) {
            // a tier change is never mid-gesture (§3 line 210), so the new source starts settling
            self.kind = Some(s.kind);
            self.rearm(here, now_ns);
            return false;
        }
        let moved = match (self.anchor, here) {
            (Some(Anchor::Dir(a)), Anchor::Dir(b)) => dot(a, b) < TOLERANCE_COS,
            (Some(Anchor::Px(ax, ay)), Anchor::Px(bx, by)) => (bx - ax).hypot(by - ay) > TOLERANCE_PX,
            _ => true,
        };
        if moved {
            self.rearm(here, now_ns);
            return false;
        }
        if !self.fired && now_ns.saturating_sub(self.settled_at) >= ONSET_NS + DWELL_NS {
            self.fired = true;
            return true;
        }
        false
    }
}

/// The two samples a dwell commit becomes: a press and its release on the dwelling kind, so every
/// stage below sees an ordinary commit. KWin's dwell clicker does the same through an input device
/// of its own (`references/kwin/src/plugins/dwellclicker/dwellclicker.cpp:188-194`).
pub fn commit_samples(kind: SourceKind, now_ns: u64) -> [Sample; 2] {
    // marked `Flags::A11Y`: a transform's commit, not a device's (KWin gives its dwell clicks a
    // device of their own, `plugins/dwellclicker/dwellclicker.cpp:188-194`)
    let mut down = Sample::new(kind, now_ns).with_button(Button::Select, true);
    let mut up = Sample::new(kind, now_ns).with_button(Button::Select, false);
    down.flags.insert(crate::input::Flags::A11Y);
    up.flags.insert(crate::input::Flags::A11Y);
    [down, up]
}

/// The `Slot::A11y` stage.
pub struct A11y {
    /// `input.dwell.enabled` (§14 line 440). Off by default — §13 makes dwell "a commit method",
    /// not the commit method.
    pub enabled: bool,
    /// `input.pointer.gain` (§14 line 441).
    pub gain: f64,
    dwell: Dwell,
    pub dwell_commits: u64,
}

impl Default for A11y {
    fn default() -> Self {
        A11y { enabled: false, gain: 1.0, dwell: Dwell::default(), dwell_commits: 0 }
    }
}

impl A11y {
    /// `input.dwell.enabled`. The control-socket grammar this lane wants for it is in its report.
    pub fn set_dwell(&mut self, on: bool) {
        self.enabled = on;
        self.dwell.reset();
        tracing::info!(on, "a11y: dwell as a commit method (spatial-input §13)");
    }

    /// `input.pointer.gain`.
    pub fn set_gain(&mut self, gain: f64) {
        self.gain = gain.max(0.0);
        tracing::info!(gain = self.gain, "a11y: pointer gain (spatial-input §14)");
    }

    pub fn dwell_state(&self) -> &Dwell {
        &self.dwell
    }
}

impl Stage for A11y {
    fn name(&self) -> &'static str {
        "a11y:dwell-gain"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        // Pointer gain: a transform on the raw event, before anything reads it. Axis deltas are the
        // scroll source's own units and are not gained (libinput's flat profile applies to motion).
        if s.kind == SourceKind::Pointer && self.gain != 1.0 {
            if let Some((dx, dy)) = s.delta {
                s.delta = Some((dx * self.gain, dy * self.gain));
            }
        }
        if self.enabled && self.dwell.step(s, s.time_ns) {
            let kind = s.kind;
            st.input.queue.extend_from_slice(&commit_samples(kind, s.time_ns));
            self.dwell_commits += 1;
            tracing::info!(?kind, count = self.dwell_commits, "a11y: dwell commit");
        }
        Flow::Continue
    }

    /// Settings pushed through `Input` (the control socket now; `org.mura.Settings1` later,
    /// spatial-input §14) are taken here, once per tick.
    fn tick(&mut self, st: &mut Zxr, _now_ns: u64) {
        if let Some(on) = st.input.a11y_dwell.take() {
            self.set_dwell(on);
        }
        if let Some(g) = st.input.a11y_gain.take() {
            self.set_gain(g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{AxisSource, Side};

    const MS: u64 = 1_000_000;
    const FIRE_MS: u64 = (ONSET_NS + DWELL_NS) / MS; // 950

    fn yaw(deg: f32) -> xr::Posef {
        let h = deg.to_radians() * 0.5;
        xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: h.sin(), z: 0.0, w: h.cos() }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } }
    }

    fn gaze(t_ms: u64, deg: f32) -> Sample {
        let mut s = Sample::new(SourceKind::Gaze, t_ms * MS);
        s.pose = Some(yaw(deg));
        s.tracked = true;
        s
    }

    fn pointer(t_ms: u64, delta: Option<(f64, f64)>) -> Sample {
        let mut s = Sample::new(SourceKind::Pointer, t_ms * MS);
        s.delta = delta;
        s
    }

    #[test]
    fn dwell_fires_once_per_settle() {
        let mut d = Dwell::default();
        assert!(!d.step(&gaze(0, 0.0), 0), "the first sample only anchors");
        for t in (11..FIRE_MS).step_by(11) {
            assert!(!d.step(&gaze(t, 0.0), t * MS), "not yet at {t} ms");
        }
        assert!(d.step(&gaze(FIRE_MS, 0.0), FIRE_MS * MS), "onset 200 + dwell 750");
        for t in (FIRE_MS + 11..FIRE_MS + 500).step_by(11) {
            assert!(!d.step(&gaze(t, 0.0), t * MS), "and never again while it sits still");
        }
    }

    #[test]
    fn movement_past_the_tolerance_rearms_and_inside_it_does_not() {
        let mut d = Dwell::default();
        d.step(&gaze(0, 0.0), 0);
        // 1° is inside the 2° tolerance: the settle survives and still fires on time
        assert!(!d.step(&gaze(500, 1.0), 500 * MS));
        assert!(d.step(&gaze(FIRE_MS, 1.0), FIRE_MS * MS));
        // 5° is past it: the machine rearms from there
        let mut d = Dwell::default();
        d.step(&gaze(0, 0.0), 0);
        assert!(!d.step(&gaze(900, 5.0), 900 * MS), "rearmed at 900 ms");
        assert!(!d.step(&gaze(900 + FIRE_MS - 11, 5.0), (900 + FIRE_MS - 11) * MS));
        assert!(d.step(&gaze(900 + FIRE_MS, 5.0), (900 + FIRE_MS) * MS), "fires 950 ms after the rearm");
    }

    #[test]
    fn a_lost_source_does_not_dwell() {
        let mut d = Dwell::default();
        d.step(&gaze(0, 0.0), 0);
        let mut lost = gaze(100, 0.0);
        lost.tracked = false;
        assert!(!d.step(&lost, 100 * MS));
        assert!(!d.step(&gaze(FIRE_MS, 0.0), FIRE_MS * MS), "the settle restarted when tracking came back");
        assert!(d.step(&gaze(FIRE_MS * 2, 0.0), FIRE_MS * 2 * MS));
    }

    #[test]
    fn the_pointer_dwells_on_accumulated_motion() {
        let mut d = Dwell::default();
        d.step(&pointer(0, None), 0);
        assert!(!d.step(&pointer(500, Some((3.0, 4.0))), 500 * MS), "5 units is inside the 20 tolerance");
        assert!(d.step(&pointer(FIRE_MS, None), FIRE_MS * MS));
        assert!(!d.step(&pointer(FIRE_MS + 11, Some((30.0, 0.0))), (FIRE_MS + 11) * MS), "a 30-unit jump rearms");
    }

    #[test]
    fn keyboards_never_dwell() {
        let mut d = Dwell::default();
        for t in (0..FIRE_MS * 2).step_by(11) {
            let mut s = Sample::new(SourceKind::Keyboard, t * MS);
            s.key = Some((30, true));
            assert!(!d.step(&s, t * MS));
        }
    }

    #[test]
    fn a_commit_is_a_press_then_a_release_of_the_same_kind() {
        let [down, up] = commit_samples(SourceKind::Hand(Side::Left), 7);
        assert_eq!(down.kind, SourceKind::Hand(Side::Left));
        assert_eq!(down.button, Some((Button::Select, true)));
        assert_eq!(up.button, Some((Button::Select, false)));
        assert_eq!(up.time_ns, 7);
    }

    #[test]
    fn gain_applies_to_pointer_deltas_only() {
        let mut a = A11y::default();
        a.set_gain(2.5);
        // the stage's transform, without a `Zxr`: the same expression `run` applies
        let apply = |a: &A11y, s: &mut Sample| {
            if s.kind == SourceKind::Pointer && a.gain != 1.0 {
                if let Some((dx, dy)) = s.delta {
                    s.delta = Some((dx * a.gain, dy * a.gain));
                }
            }
        };
        let mut p = pointer(0, Some((2.0, -4.0)));
        apply(&a, &mut p);
        assert_eq!(p.delta, Some((5.0, -10.0)));
        // axes are the scroll source's units, untouched
        let mut ax = pointer(0, None);
        ax.axis = Some((0.0, 10.0));
        ax.axis_source = Some(AxisSource::Wheel);
        apply(&a, &mut ax);
        assert_eq!(ax.axis, Some((0.0, 10.0)));
        // and an XR kind's motion is not a pointer delta
        let mut h = Sample::new(SourceKind::Hand(Side::Right), 0);
        h.delta = Some((2.0, 2.0));
        apply(&a, &mut h);
        assert_eq!(h.delta, Some((2.0, 2.0)));
    }

    #[test]
    fn dwell_is_off_by_default() {
        let a = A11y::default();
        assert!(!a.enabled);
        assert_eq!(a.gain, 1.0);
        assert_eq!(a.dwell_commits, 0);
    }
}
