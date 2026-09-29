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
//! - **Dwell, in two layers** (§13, ruled 2026-09-29; [`dwell_active`]): the *accessibility toggle*
//!   `input.dwell.enabled` — KWin's dwell clicker, GNOME's hover click, visionOS's Dwell Control — on
//!   whatever pointer targets, for a wearer who cannot press; and the *input floor* — the head ray
//!   when no usable select button exists (`Peripherals::floor_dwell`), automatic, the fault case
//!   preflight P7 names. Every target device has a select, so the floor is the exception; the
//!   toggle is the setting. The stage shape is KWin's dwell
//!   clicker: a single-shot delay timer, then a dwell animation, then the click, with motion past
//!   a threshold resetting the lot (`references/kwin/src/plugins/dwellclicker/dwellclicker.cpp:91-114`
//!   for delay→dwell→click, `:150-153` for the two intervals being separate settings, `:163-183`
//!   for start/stop). KWin injects the click through an input device of its own
//!   (`dwellclicker.cpp:194` `input()->addInputDevice(m_device.get())`); zxr's equivalent is a
//!   `Button::Select` press and release queued as samples of the dwelling kind, so the whole chain
//!   below — stabilize, tier, hit, grabs, seat — treats the dwell commit exactly like a physical
//!   one. **The anchor is the hit point on the target** (ruled 2026-09-28,
//!   spatial-input §13): KWin arms on the pointer's *position* moving past `motionThreshold`
//!   (`dwellclicker.cpp:252-277`), and a ray's position is where it lands on a plane — so a plane
//!   the head carries never reads as "still" while the head turns, and a still plane under a
//!   moving head settles where the ray points. The tolerance is visual angle at that point. Only
//!   the targeting tier's source drives the machine; a new target rearms it (MRTK3's dwell belongs
//!   to the interactable it started on, `InteractorDwellManager.cs`). Progress is published for
//!   the reticle's fill (Cardboard's fuse; KWin's dwell animation).
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
use crate::scene::MemberId;
use crate::state::Zxr;

/// dwell onset — the delay before the dwell itself starts (KWin's `delayTime`,
/// `dwellclicker.cpp:150`). **Stand-in:** 200 ms, the middle of §13's 150-250 ms (HoloLens;
/// research/42 §5 lines 525-528's literature range). Flagged: a range, not a measurement.
pub const ONSET_NS: u64 = 200_000_000;
/// the dwell proper (KWin's `dwellTime`, `dwellclicker.cpp:152`). **Stand-in:** 750 ms, the middle
/// of §13's 650-850 ms; research/42 §5 line 525 records "~600 ms generic, 400 ms for
/// alphanumerics, 800 ms for icons, ≥ 1000 ms judged unusable" and Rajanna & Hansen's 550 ms in
/// VR. Flagged.
pub const DWELL_NS: u64 = 750_000_000;
/// movement tolerance for a pose source, in **visual angle at the hit point on the target**
/// (ruled 2026-09-28, spatial-input §13: the anchor is the point the ray hits on the plane,
/// KWin's pointer position, `dwellclicker.cpp:260-275` — never the ray's direction in the world,
/// which a plane carried by the same head would make constant). **Stand-in:** 2° (§13 requires "a
/// movement tolerance" and gives no number; 2° is the order of KWin's pixel threshold at a panel's
/// arm's length). Flagged.
pub const TOLERANCE_DEG: f32 = 2.0;
/// movement tolerance for `SourceKind::Pointer`, in accumulated device units. **Stand-in:** 20,
/// the order of KWin's dwell-clicker motion threshold. Flagged.
pub const TOLERANCE_PX: f64 = 20.0;

/// Where a targeting ray lands this sample: the plane and the point on it (the hit stage's
/// `MemberHit`, cast here ahead of it because the transform must shape the sample first).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HitPoint {
    pub member: MemberId,
    /// plane-local metres from the plane's centre
    pub local: [f32; 2],
    /// metres from the ray's origin to the point: the visual-angle scale of the tolerance
    pub distance: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Anchor {
    /// a pose source: the target and the point on it where the settle began
    Hit(HitPoint),
    /// `SourceKind::Pointer`: the accumulated position where the settle began
    Px(f64, f64),
}

/// The dwell state machine, pure: one targeting source at a time (the tier rule guarantees that,
/// §3), a settle anchor, and the two intervals. Fires once per settle and rearms only on movement
/// past the tolerance — KWin's `start()`/`stop()` pair (`dwellclicker.cpp:163-183`) — or on a new
/// target under the same ray (MRTK3 `InteractorDwellManager`: the dwell belongs to the
/// interactable it started on).
pub struct Dwell {
    pub(crate) kind: Option<SourceKind>,
    anchor: Option<Anchor>,
    /// running position of the pointer, in accumulated device units
    px: (f64, f64),
    settled_at: u64,
    fired: bool,
    /// `input.dwell.{onset_ms,complete_ms,tolerance_deg}` (settings.rs); the consts are the defaults
    onset_ns: u64,
    dwell_ns: u64,
    /// tan of the tolerance angle: metres of plane per metre of distance
    tolerance_tan: f32,
}

impl Default for Dwell {
    fn default() -> Self {
        Dwell { kind: None, anchor: None, px: (0.0, 0.0), settled_at: 0, fired: false, onset_ns: ONSET_NS, dwell_ns: DWELL_NS, tolerance_tan: TOLERANCE_DEG.to_radians().tan() }
    }
}

impl Dwell {
    /// `input.dwell.{onset_ms,complete_ms,tolerance_deg}`.
    pub fn set_timing(&mut self, onset_ms: u64, complete_ms: u64, tolerance_deg: f32) {
        self.onset_ns = onset_ms.saturating_mul(1_000_000);
        self.dwell_ns = complete_ms.saturating_mul(1_000_000);
        self.tolerance_tan = tolerance_deg.clamp(0.0, 89.0).to_radians().tan();
    }

    fn rearm(&mut self, a: Anchor, now_ns: u64) {
        self.anchor = Some(a);
        self.settled_at = now_ns;
        self.fired = false;
    }

    pub fn reset(&mut self) {
        self.anchor = None;
        self.fired = false;
    }

    /// The target the settle is anchored on, while a pose source is settling.
    pub fn target(&self) -> Option<MemberId> {
        match self.anchor {
            Some(Anchor::Hit(h)) => Some(h.member),
            _ => None,
        }
    }

    /// Dwell progress for the reticle (Cardboard's fuse ring, KWin's dwell animation
    /// `dwellclicker.cpp:91-114`): `Some(0..=1)` from the onset's end to the commit while a settle
    /// is anchored and has not fired; `None` otherwise. `0` during the onset — the ring shows
    /// nothing for a glance that merely passes over a target.
    pub fn progress(&self, now_ns: u64) -> Option<f32> {
        if self.anchor.is_none() || self.fired {
            return None;
        }
        let since = now_ns.saturating_sub(self.settled_at);
        if since < self.onset_ns {
            return Some(0.0);
        }
        if self.dwell_ns == 0 {
            return Some(1.0);
        }
        Some(((since - self.onset_ns) as f32 / self.dwell_ns as f32).min(1.0))
    }

    /// Advance the machine with one sample; `hit` is where a tracked pose sample's ray lands, if
    /// anywhere. `true` means a commit is due now.
    pub fn step(&mut self, s: &Sample, hit: Option<HitPoint>, now_ns: u64) -> bool {
        let here = match s.kind {
            SourceKind::Pointer => {
                let (dx, dy) = s.delta.unwrap_or((0.0, 0.0));
                self.px.0 += dx;
                self.px.1 += dy;
                Anchor::Px(self.px.0, self.px.1)
            }
            k if k.targets() => {
                // an untracked source, or a ray into the void, is not dwelling on anything
                let Some(h) = hit.filter(|_| s.tracked && s.pose.is_some()) else {
                    self.reset();
                    return false;
                };
                Anchor::Hit(h)
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
            (Some(Anchor::Hit(a)), Anchor::Hit(b)) => {
                // the anchor is the point on the target: a new target, or a move across the plane
                // past the tolerance at this distance
                a.member != b.member || (b.local[0] - a.local[0]).hypot(b.local[1] - a.local[1]) > self.tolerance_tan * b.distance.max(0.05)
            }
            (Some(Anchor::Px(ax, ay)), Anchor::Px(bx, by)) => (bx - ax).hypot(by - ay) > TOLERANCE_PX,
            _ => true,
        };
        if moved {
            self.rearm(here, now_ns);
            return false;
        }
        if !self.fired && now_ns.saturating_sub(self.settled_at) >= self.onset_ns + self.dwell_ns {
            self.fired = true;
            return true;
        }
        false
    }
}

/// The hit a tracked pose sample's ray makes, the hit stage's own cast and predicate
/// (`hit.rs` `HitStage::cast`: mapped, not hidden, trusted-only while the mode gate is closed).
fn hit_of(s: &Sample, st: &Zxr) -> Option<HitPoint> {
    let p = s.pose.filter(|_| s.tracked && s.kind.targets() && s.kind != SourceKind::Pointer)?;
    let origin = [p.position.x, p.position.y, p.position.z];
    let dir = crate::xr::math::rotate(p.orientation, [0.0, 0.0, -1.0]);
    let gated = st.input.mode != super::Mode::Normal;
    let h = super::hit::hit_member_with(&st.scene, origin, dir, |m| m.mapped() && !m.hidden && (!gated || m.trusted), |_, band| super::hit::Class::from_band(band), st.prefs.hardware.hit_class_epsilon_m.max(0.0))?;
    Some(HitPoint { member: h.member, local: h.local, distance: h.distance })
}

/// The two samples a dwell commit becomes: a press and its release on the dwelling kind, **carrying
/// the firing sample's pose and tracking**, so every stage below sees an ordinary commit at the
/// point the ray holds (a touch-class `down` needs a tracked, ready sample with a hit — a bare
/// button sample is dropped, `touch.rs` `plan`). KWin's dwell clicker does the same through an
/// input device of its own, clicking at the pointer's position
/// (`references/kwin/src/plugins/dwellclicker/dwellclicker.cpp:188-194`).
pub fn commit_samples(from: &Sample) -> [Sample; 2] {
    // marked `Flags::A11Y`: a transform's commit, not a device's (KWin gives its dwell clicks a
    // device of their own, `plugins/dwellclicker/dwellclicker.cpp:188-194`)
    let one = |pressed: bool| {
        let mut s = Sample::new(from.kind, from.time_ns).with_button(Button::Select, pressed);
        s.pose = from.pose;
        s.tracked = from.tracked;
        s.ready = from.ready;
        s.quality = from.quality;
        s.xr_time = from.xr_time;
        s.flags.insert(crate::input::Flags::A11Y);
        s
    };
    [one(true), one(false)]
}

/// Which layer, if any, makes dwell the commit for a sample of `kind` (spatial-input §13, ruled
/// 2026-09-29). **Two layers, one machine:**
/// - the **accessibility toggle** `input.dwell.enabled` — KWin's dwell clicker (`metadata.json`
///   `Category: Accessibility`, `EnabledByDefault: false`), GNOME's hover click, visionOS Dwell
///   Control [external]: for a wearer who cannot press any button, on *whatever* pointer targets;
/// - the **input floor** — automatic, never a preference: the head ray on a device with no usable
///   select button (Cardboard's fuse is the one comparable that ships this; Mura's targets all have
///   a select, so this is the fault case P7 already names — a missing button driver), derived at
///   runtime by `Peripherals::floor_dwell`.
/// Only the tier's targeting source is a candidate (`drives`); a held controller silences the
/// head's floor dwell exactly as it takes the tier (PICO's Head Control Mode is a *no-controller*
/// mode, research/42 §4). Pure, so the gate is unit-tested.
pub fn dwell_active(enabled: bool, floor: bool, kind: SourceKind) -> bool {
    enabled || (floor && kind == SourceKind::Head)
}

/// The `Slot::A11y` stage.
pub struct A11y {
    /// `input.dwell.enabled` (§14): the accessibility toggle — dwell on whatever pointer targets.
    /// Off by default; the floor's dwell below is not this key.
    pub enabled: bool,
    /// the input floor's dwell: the head ray, when no usable select button exists (taken each tick
    /// from `Peripherals::floor_dwell`)
    pub floor: bool,
    /// `input.pointer.gain` (§14 line 441).
    pub gain: f64,
    dwell: Dwell,
    /// commits by the accessibility toggle and by the floor, separately (the `a11y:` diagnostics line)
    pub dwell_commits: u64,
    pub dwell_commits_floor: u64,
    /// the `Prefs::generation` last taken (settings.rs)
    prefs_gen: u64,
}

impl Default for A11y {
    fn default() -> Self {
        A11y { enabled: false, floor: false, gain: 1.0, dwell: Dwell::default(), dwell_commits: 0, dwell_commits_floor: 0, prefs_gen: 0 }
    }
}

impl A11y {
    /// `input.dwell.enabled`, the accessibility toggle. The machine is reset only when the toggle
    /// changes what is active for the settling kind — turning the toggle off must not cut a floor
    /// settle in progress.
    pub fn set_dwell(&mut self, on: bool) {
        let was = self.dwell.kind.map(|k| dwell_active(self.enabled, self.floor, k));
        self.enabled = on;
        let now = self.dwell.kind.map(|k| dwell_active(self.enabled, self.floor, k));
        if was != now {
            self.dwell.reset();
        }
        tracing::info!(on, floor = self.floor, "a11y: dwell click (the accessibility toggle; spatial-input §13)");
    }

    /// The floor's fact changed (a device came or went).
    fn set_floor(&mut self, floor: bool) {
        if self.floor == floor {
            return;
        }
        self.floor = floor;
        tracing::info!(floor, "a11y: the input floor's dwell (no usable select button ⇒ the head ray dwells)");
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
        // Dwell is the commit method of the *targeting* tier (§13, §3): only the source the tier
        // arbiter selected last tick drives the machine — a head ray sampled beside a controller
        // ray must not rearm the controller's settle, nor dwell for it. Before the first
        // selection, any targeting kind may.
        let targeting = st.input.tier.map(|t| t.targeting);
        let drives = s.kind == SourceKind::Pointer || targeting.map(|t| t == s.kind).unwrap_or(true);
        if drives && dwell_active(self.enabled, self.floor, s.kind) {
            // one ray cast per targeting sample while dwell is on (off by default): the anchor is
            // the point on the target, so the transform needs the hit before the hit stage runs
            let hit = hit_of(s, st);
            if self.dwell.step(s, hit, s.time_ns) {
                let kind = s.kind;
                st.input.queue.extend_from_slice(&commit_samples(s));
                if self.enabled {
                    self.dwell_commits += 1;
                } else {
                    self.dwell_commits_floor += 1;
                }
                tracing::info!(?kind, a11y = self.dwell_commits, floor = self.dwell_commits_floor, "a11y: dwell commit");
            }
        }
        Flow::Continue
    }

    /// Settings are taken here, once per tick: the resolved preferences when their generation
    /// moved (settings.rs `apply`; `input.dwell.*`, `input.pointer.gain`), and the control
    /// socket's direct pushes through `Input` (the harness's path, spatial-input §14).
    fn tick(&mut self, st: &mut Zxr, now_ns: u64) {
        // the reticle's fill (cursor.rs): the settle's progress on its target, this tick
        self.set_floor(st.peripherals.floor_dwell());
        let active = self.dwell.kind.map(|k| dwell_active(self.enabled, self.floor, k)).unwrap_or(false);
        st.input.dwell_progress = if active { self.dwell.progress(now_ns).map(|p| (self.dwell.target(), p)) } else { None };
        if self.prefs_gen != st.prefs.generation {
            self.prefs_gen = st.prefs.generation;
            let p = &st.prefs;
            self.dwell.set_timing(p.dwell_onset_ms, p.dwell_complete_ms, p.dwell_tolerance_deg);
            if self.enabled != p.dwell_enabled {
                self.set_dwell(p.dwell_enabled);
            }
            if self.gain != p.pointer_gain.max(0.0) {
                self.set_gain(p.pointer_gain);
            }
        }
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

    /// two targets in a scene, for the anchors' member ids
    fn members() -> (MemberId, MemberId) {
        use crate::scene::{Flags, Scene, Shape};
        use crate::xr::math;
        let mut scene: Scene<bool> = Scene::new();
        let a = scene.add(scene.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::default(), true).unwrap();
        let b = scene.add(scene.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::default(), true).unwrap();
        (a, b)
    }

    fn gaze(t_ms: u64) -> Sample {
        let mut s = Sample::new(SourceKind::Gaze, t_ms * MS);
        s.pose = Some(crate::xr::math::pose_identity());
        s.tracked = true;
        s
    }

    /// the ray landing `deg` of visual angle off the anchor on a plane 1 m away
    fn at(member: MemberId, deg: f32) -> Option<HitPoint> {
        let x = deg.to_radians().tan();
        Some(HitPoint { member, local: [x, 0.0], distance: (1.0 + x * x).sqrt() })
    }

    fn pointer(t_ms: u64, delta: Option<(f64, f64)>) -> Sample {
        let mut s = Sample::new(SourceKind::Pointer, t_ms * MS);
        s.delta = delta;
        s
    }

    #[test]
    fn dwell_fires_once_per_settle() {
        let (m, _) = members();
        let mut d = Dwell::default();
        assert!(!d.step(&gaze(0), at(m, 0.0), 0), "the first sample only anchors");
        for t in (11..FIRE_MS).step_by(11) {
            assert!(!d.step(&gaze(t), at(m, 0.0), t * MS), "not yet at {t} ms");
        }
        assert!(d.step(&gaze(FIRE_MS), at(m, 0.0), FIRE_MS * MS), "onset 200 + dwell 750");
        for t in (FIRE_MS + 11..FIRE_MS + 500).step_by(11) {
            assert!(!d.step(&gaze(t), at(m, 0.0), t * MS), "and never again while it sits still");
        }
    }

    #[test]
    fn movement_past_the_tolerance_rearms_and_inside_it_does_not() {
        let (m, _) = members();
        let mut d = Dwell::default();
        d.step(&gaze(0), at(m, 0.0), 0);
        // 1° across the plane is inside the 2° tolerance: the settle survives and still fires on time
        assert!(!d.step(&gaze(500), at(m, 1.0), 500 * MS));
        assert!(d.step(&gaze(FIRE_MS), at(m, 1.0), FIRE_MS * MS));
        // 5° is past it: the machine rearms from there
        let mut d = Dwell::default();
        d.step(&gaze(0), at(m, 0.0), 0);
        assert!(!d.step(&gaze(900), at(m, 5.0), 900 * MS), "rearmed at 900 ms");
        assert!(!d.step(&gaze(900 + FIRE_MS - 11), at(m, 5.0), (900 + FIRE_MS - 11) * MS));
        assert!(d.step(&gaze(900 + FIRE_MS), at(m, 5.0), (900 + FIRE_MS) * MS), "fires 950 ms after the rearm");
    }

    /// The anchor is the point on the target, not the ray's direction (F2): a ray whose direction
    /// changes but lands on the same point — the head turning while its ray stays on a still
    /// plane's button, as the hit stage resolves it — keeps settling; a new target under a
    /// constant ray (a plane the head carries sliding past) rearms; the void resets.
    #[test]
    fn the_anchor_is_the_hit_point_on_the_target() {
        let (m, other) = members();
        let mut d = Dwell::default();
        d.step(&gaze(0), at(m, 0.0), 0);
        let mut turned = gaze(FIRE_MS);
        turned.pose = Some(crate::xr::math::pose_yaw([0.0; 3], 0.5));
        assert!(d.step(&turned, at(m, 0.0), FIRE_MS * MS), "a turned head on the same point still commits");
        let mut d = Dwell::default();
        d.step(&gaze(0), at(m, 0.0), 0);
        assert!(!d.step(&gaze(FIRE_MS), at(other, 0.0), FIRE_MS * MS), "a new target starts its own settle");
        assert!(d.step(&gaze(2 * FIRE_MS), at(other, 0.0), 2 * FIRE_MS * MS));
        let mut d = Dwell::default();
        d.step(&gaze(0), at(m, 0.0), 0);
        assert!(!d.step(&gaze(100), None, 100 * MS));
        assert!(d.progress(100 * MS).is_none(), "no anchor, no progress");
        assert!(!d.step(&gaze(FIRE_MS), at(m, 0.0), FIRE_MS * MS), "restarted when a target came back");
    }

    #[test]
    fn progress_runs_from_the_onset_to_the_commit() {
        let (m, _) = members();
        let mut d = Dwell::default();
        assert!(d.progress(0).is_none());
        d.step(&gaze(0), at(m, 0.0), 0);
        assert_eq!(d.progress(100 * MS), Some(0.0), "nothing during the onset");
        let p = d.progress((200 + 375) * MS).unwrap();
        assert!((p - 0.5).abs() < 1e-3, "{p}");
        assert!((d.progress(FIRE_MS * MS).unwrap() - 1.0).abs() < 1e-6);
        assert!(d.step(&gaze(FIRE_MS), at(m, 0.0), FIRE_MS * MS));
        assert!(d.progress((FIRE_MS + 1) * MS).is_none(), "fired: the ring empties");
        assert_eq!(d.target(), Some(m));
    }

    #[test]
    fn the_intervals_and_tolerance_are_the_settings() {
        // `input.dwell.{onset_ms,complete_ms,tolerance_deg}` = 150 / 650 / 6: fires at 800 ms,
        // and a 5° move stays inside the tolerance that 2° would have broken
        let (m, _) = members();
        let mut d = Dwell::default();
        d.set_timing(150, 650, 6.0);
        d.step(&gaze(0), at(m, 0.0), 0);
        assert!(!d.step(&gaze(799), at(m, 5.0), 799 * MS), "5° inside 6°: still settled, not yet due");
        assert!(d.step(&gaze(800), at(m, 5.0), 800 * MS), "fires at onset + complete");
    }

    #[test]
    fn a_lost_source_does_not_dwell() {
        let (m, _) = members();
        let mut d = Dwell::default();
        d.step(&gaze(0), at(m, 0.0), 0);
        let mut lost = gaze(100);
        lost.tracked = false;
        assert!(!d.step(&lost, at(m, 0.0), 100 * MS));
        assert!(!d.step(&gaze(FIRE_MS), at(m, 0.0), FIRE_MS * MS), "the settle restarted when tracking came back");
        assert!(d.step(&gaze(FIRE_MS * 2), at(m, 0.0), FIRE_MS * 2 * MS));
    }

    #[test]
    fn the_pointer_dwells_on_accumulated_motion() {
        let mut d = Dwell::default();
        d.step(&pointer(0, None), None, 0);
        assert!(!d.step(&pointer(500, Some((3.0, 4.0))), None, 500 * MS), "5 units is inside the 20 tolerance");
        assert!(d.step(&pointer(FIRE_MS, None), None, FIRE_MS * MS));
        assert!(!d.step(&pointer(FIRE_MS + 11, Some((30.0, 0.0))), None, (FIRE_MS + 11) * MS), "a 30-unit jump rearms");
    }

    #[test]
    fn keyboards_never_dwell() {
        let mut d = Dwell::default();
        for t in (0..FIRE_MS * 2).step_by(11) {
            let mut s = Sample::new(SourceKind::Keyboard, t * MS);
            s.key = Some((30, true));
            assert!(!d.step(&s, None, t * MS));
        }
    }

    #[test]
    fn a_commit_is_a_press_then_a_release_of_the_same_kind_at_the_same_pose() {
        let from = Sample::new(SourceKind::Hand(Side::Left), 7).with_pose(crate::xr::math::pose_yaw([0.0; 3], 0.3));
        let [down, up] = commit_samples(&from);
        assert_eq!(down.kind, SourceKind::Hand(Side::Left));
        assert_eq!(down.button, Some((Button::Select, true)));
        assert_eq!(up.button, Some((Button::Select, false)));
        assert_eq!(up.time_ns, 7);
        // the touch transport plans a `down` only for a tracked, ready sample with a pose
        assert!(down.tracked && down.ready && down.pose.is_some());
        assert!(down.flags.contains(crate::input::Flags::A11Y));
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
    fn the_a11y_toggle_is_off_by_default_and_the_floor_is_derived() {
        let a = A11y::default();
        assert!(!a.enabled && !a.floor);
        assert_eq!(a.gain, 1.0);
        assert_eq!((a.dwell_commits, a.dwell_commits_floor), (0, 0));
    }

    /// The two layers (spatial-input §13, ruled 2026-09-29): the floor dwells on the head ray only
    /// when no usable select exists; the accessibility toggle dwells on whatever targets.
    #[test]
    fn the_floor_dwells_on_the_head_ray_only_when_no_select_exists() {
        assert!(dwell_active(false, true, SourceKind::Head));
        assert!(!dwell_active(false, true, SourceKind::Controller(Side::Right)), "a controller has a button of its own");
        assert!(!dwell_active(false, true, SourceKind::Hand(Side::Left)), "a hand pinches");
        assert!(!dwell_active(false, false, SourceKind::Head), "a select exists: no floor dwell");
    }

    #[test]
    fn the_a11y_toggle_dwells_on_any_targeting_kind() {
        for k in [SourceKind::Head, SourceKind::Gaze, SourceKind::Controller(Side::Left), SourceKind::Hand(Side::Right), SourceKind::Pointer] {
            assert!(dwell_active(true, false, k), "{k:?}");
        }
    }

    #[test]
    fn turning_the_toggle_off_keeps_a_floor_settle() {
        let (m, _) = members();
        let mut a = A11y { enabled: true, floor: true, ..A11y::default() };
        let mut head = Sample::new(SourceKind::Head, 0);
        head.pose = Some(crate::xr::math::pose_identity());
        head.tracked = true;
        a.dwell.step(&head, at(m, 0.0), 0);
        a.set_dwell(false);
        assert!(a.dwell.progress(500 * MS).is_some(), "the floor still dwells on the head: the settle survives the toggle");
        // a controller settle under the toggle does not survive it: the floor never covered it
        let mut c = Sample::new(SourceKind::Controller(Side::Right), 0);
        c.pose = Some(crate::xr::math::pose_identity());
        c.tracked = true;
        let mut a = A11y { enabled: true, floor: true, ..A11y::default() };
        a.dwell.step(&c, at(m, 0.0), 0);
        a.set_dwell(false);
        assert!(a.dwell.progress(500 * MS).is_none());
    }
}
