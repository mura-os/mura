//! Ray stabilization, sticky targeting, relaxation, and event-time compensation.
//!
//! This is the `stabilize` stage ruled by `docs/architecture/spatial-input.md:214-227` and
//! `specs/zxr-core.md:431-434`. The filter is the temporal form of MRTK3's `StabilizedRay`
//! (`references/mrtk3/org.mixedrealitytoolkit.core/Utilities/StabilizedRay.cs:70-110`) with the
//! hand-ray half-lives selected at
//! `references/mrtk3/org.mixedrealitytoolkit.input/Utilities/PoseSource/LOSAngularOffsetHandRayPoseSource.cs:18-23,40-46`.
//! Hand aim is already normally runtime-stabilized
//! (`references/openxr-docs/specification/sources/chapters/extensions/ext/ext_hand_interaction.adoc:78-103`);
//! the same small filter remains deterministic across all XR ray kinds and is most visible on
//! the head floor.

use openxr as xr;

use super::{Button, Flow, Sample, Side, SourceKind, Stage};
use crate::state::Zxr;

pub const POSITION_HALF_LIFE_S: f32 = 0.01;
pub const DIRECTION_HALF_LIFE_S: f32 = 0.05;
pub const STICKY_THRESHOLD: f32 = 0.5;
pub const RAY_RELAXATION_THRESHOLD: f32 = 0.5;
pub const GAZE_RELAXATION_THRESHOLD: f32 = 0.1;
/// Stand-in from MRTK3's UI select threshold
/// (`references/mrtk3/org.mixedrealitytoolkit.core/Interactables/StatefulInteractable.cs:52-64`);
/// tracker-specific pinch thresholds remain an M1 measurement (research/63 §3).
pub const PINCH_CLOSED_THRESHOLD: f32 = 0.9;
/// Explicit design stand-in: compensate the pose 50 ms before the commit edge.
pub const COMPENSATION_NS: u64 = 50_000_000;

/// The stabiliser's numbers — `hardware.input.stabilize.*` (the tracker's calibration,
/// immutable; settings.rs `Prefs::stabilize_cfg`) plus the one layered preference,
/// `input.pointer.click_freeze_ms` (the compensation window; Q2 ruled, GNOME
/// `org.gnome.desktop.a11y.mouse` click-assist's shape). The consts above are the defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StabilizeCfg {
    pub position_half_life_s: f32,
    pub direction_half_life_s: f32,
    pub sticky: f32,
    pub relaxation_ray: f32,
    pub relaxation_gaze: f32,
    pub pinch_closed: f32,
    pub compensation_ns: u64,
}

impl Default for StabilizeCfg {
    fn default() -> Self {
        StabilizeCfg {
            position_half_life_s: POSITION_HALF_LIFE_S,
            direction_half_life_s: DIRECTION_HALF_LIFE_S,
            sticky: STICKY_THRESHOLD,
            relaxation_ray: RAY_RELAXATION_THRESHOLD,
            relaxation_gaze: GAZE_RELAXATION_THRESHOLD,
            pinch_closed: PINCH_CLOSED_THRESHOLD,
            compensation_ns: COMPENSATION_NS,
        }
    }
}
const HISTORY_NS: u64 = 250_000_000;
const HISTORY_LEN: usize = 32;
const KIND_COUNT: usize = 8;

const IDENTITY: xr::Posef = xr::Posef {
    orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
    position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 },
};

fn kind_index(kind: SourceKind) -> usize {
    match kind {
        SourceKind::Head => 0,
        SourceKind::Gaze => 1,
        SourceKind::Hand(Side::Left) => 2,
        SourceKind::Hand(Side::Right) => 3,
        SourceKind::Controller(Side::Left) => 4,
        SourceKind::Controller(Side::Right) => 5,
        SourceKind::Pointer => 6,
        SourceKind::Keyboard => 7,
    }
}

fn has_ray_pose(kind: SourceKind) -> bool {
    matches!(kind, SourceKind::Head | SourceKind::Gaze | SourceKind::Hand(_) | SourceKind::Controller(_))
}

fn half_life_alpha(dt_s: f32, half_life_s: f32) -> f32 {
    if half_life_s <= 0.0 {
        1.0
    } else {
        1.0 - 0.5_f32.powf(dt_s.max(0.0) / half_life_s)
    }
}

fn quat_normalize(q: xr::Quaternionf) -> xr::Quaternionf {
    let n = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    if n <= f32::EPSILON {
        return IDENTITY.orientation;
    }
    xr::Quaternionf { x: q.x / n, y: q.y / n, z: q.z / n, w: q.w / n }
}

fn quat_slerp(from: xr::Quaternionf, mut to: xr::Quaternionf, t: f32) -> xr::Quaternionf {
    let from = quat_normalize(from);
    to = quat_normalize(to);
    let mut dot = from.x * to.x + from.y * to.y + from.z * to.z + from.w * to.w;
    if dot < 0.0 {
        dot = -dot;
        to = xr::Quaternionf { x: -to.x, y: -to.y, z: -to.z, w: -to.w };
    }
    if dot > 0.9995 {
        return quat_normalize(xr::Quaternionf {
            x: from.x + (to.x - from.x) * t,
            y: from.y + (to.y - from.y) * t,
            z: from.z + (to.z - from.z) * t,
            w: from.w + (to.w - from.w) * t,
        });
    }
    let theta = dot.clamp(-1.0, 1.0).acos();
    let sin_theta = theta.sin();
    let a = ((1.0 - t) * theta).sin() / sin_theta;
    let b = (t * theta).sin() / sin_theta;
    quat_normalize(xr::Quaternionf {
        x: from.x * a + to.x * b,
        y: from.y * a + to.y * b,
        z: from.z * a + to.z * b,
        w: from.w * a + to.w * b,
    })
}

/// Allocation-free temporal ray filter. MRTK3 filters position and direction separately; this
/// keeps the equivalent ray orientation as a unit quaternion so the `pose → -Z` contract remains
/// intact. Half-lives are seconds, as specified by spatial-input §4.
#[derive(Clone, Copy)]
pub struct RayFilter {
    pose: xr::Posef,
    time_ns: u64,
    initialized: bool,
}

impl RayFilter {
    pub const fn new() -> Self {
        Self { pose: IDENTITY, time_ns: 0, initialized: false }
    }

    pub fn update(&mut self, pose: xr::Posef, time_ns: u64) -> xr::Posef {
        self.update_with(pose, time_ns, POSITION_HALF_LIFE_S, DIRECTION_HALF_LIFE_S)
    }

    /// [`update`](Self::update) with the calibration's half-lives.
    pub fn update_with(&mut self, pose: xr::Posef, time_ns: u64, position_half_life_s: f32, direction_half_life_s: f32) -> xr::Posef {
        if !self.initialized {
            self.pose = pose;
            self.time_ns = time_ns;
            self.initialized = true;
            return pose;
        }
        let dt_s = time_ns.saturating_sub(self.time_ns) as f32 * 1e-9;
        self.time_ns = time_ns;
        let pa = half_life_alpha(dt_s, position_half_life_s);
        let da = half_life_alpha(dt_s, direction_half_life_s);
        self.pose.position.x += (pose.position.x - self.pose.position.x) * pa;
        self.pose.position.y += (pose.position.y - self.pose.position.y) * pa;
        self.pose.position.z += (pose.position.z - self.pose.position.z) * pa;
        self.pose.orientation = quat_slerp(self.pose.orientation, pose.orientation, da);
        self.pose
    }
}

impl Default for RayFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Sticky value lock with a separate relaxation threshold. It ports MRTK3's “stay hovering”
/// and roll-off guard (`references/mrtk3/org.mixedrealitytoolkit.input/Interactors/Ray/MRTKRayInteractor.cs:56-70,92-116,202-216`)
/// and gaze-pinch's 0.5/0.1 split
/// (`references/mrtk3/org.mixedrealitytoolkit.input/Interactors/GazePinch/GazePinchInteractor.cs:80-105,174-190`).
/// Locking the stabilized ray value makes the selected target survive hand-induced drift without
/// another scene pass in this stage.
#[derive(Clone, Copy)]
pub struct TargetLock<T: Copy> {
    locked: Option<T>,
    relaxed: bool,
}

impl<T: Copy> TargetLock<T> {
    pub const fn new() -> Self {
        Self { locked: None, relaxed: true }
    }

    pub fn update(&mut self, current: T, progress: f32, relaxation: f32) -> T {
        self.update_with(current, progress, relaxation, STICKY_THRESHOLD)
    }

    /// [`update`](Self::update) with the calibration's sticky threshold.
    pub fn update_with(&mut self, current: T, progress: f32, relaxation: f32, sticky: f32) -> T {
        if progress < relaxation {
            self.locked = None;
            self.relaxed = true;
            return current;
        }
        if let Some(locked) = self.locked {
            return locked;
        }
        if progress > sticky {
            if self.relaxed {
                self.locked = Some(current);
            }
            self.relaxed = false;
        }
        self.locked.unwrap_or(current)
    }
}

impl<T: Copy> Default for TargetLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
struct TimedPose {
    time_ns: u64,
    pose: xr::Posef,
}

const EMPTY_TIMED_POSE: TimedPose = TimedPose { time_ns: 0, pose: IDENTITY };

/// Fixed ring of stabilized poses. The recording/replay shape comes from
/// `references/xrdesktop/src/xrd-input-synth.c:340-362` and
/// `references/xrdesktop/src/xrd-shake-compensator.c:142-186`; unlike xrdesktop's allocating
/// `GQueue`, this embedded path has a fixed steady-state footprint.
#[derive(Clone, Copy)]
pub struct Compensator {
    poses: [TimedPose; HISTORY_LEN],
    start: usize,
    len: usize,
}

impl Compensator {
    pub const fn new() -> Self {
        Self { poses: [EMPTY_TIMED_POSE; HISTORY_LEN], start: 0, len: 0 }
    }

    pub fn record(&mut self, time_ns: u64, pose: xr::Posef) {
        while self.len > 0 && time_ns.saturating_sub(self.poses[self.start].time_ns) > HISTORY_NS {
            self.start = (self.start + 1) % HISTORY_LEN;
            self.len -= 1;
        }
        let slot = if self.len < HISTORY_LEN {
            let slot = (self.start + self.len) % HISTORY_LEN;
            self.len += 1;
            slot
        } else {
            let slot = self.start;
            self.start = (self.start + 1) % HISTORY_LEN;
            slot
        };
        self.poses[slot] = TimedPose { time_ns, pose };
    }

    /// Most recent pose at or before `time_ns`, falling back to the oldest retained pose.
    pub fn pose_at(&self, time_ns: u64) -> Option<xr::Posef> {
        if self.len == 0 {
            return None;
        }
        let mut chosen = self.poses[self.start].pose;
        for i in 0..self.len {
            let p = self.poses[(self.start + i) % HISTORY_LEN];
            if p.time_ns > time_ns {
                break;
            }
            chosen = p.pose;
        }
        Some(chosen)
    }
}

impl Default for Compensator {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Stabilize {
    filters: [RayFilter; KIND_COUNT],
    locks: [TargetLock<xr::Posef>; KIND_COUNT],
    compensators: [Compensator; KIND_COUNT],
    select_held: [bool; KIND_COUNT],
    previous_pinch: [f32; KIND_COUNT],
    /// the calibration and the layered click-freeze (settings.rs `Prefs::stabilize_cfg`)
    pub cfg: StabilizeCfg,
    prefs_gen: u64,
}

impl Stabilize {
    pub fn new() -> Self {
        Self {
            filters: [RayFilter::new(); KIND_COUNT],
            locks: [TargetLock::new(); KIND_COUNT],
            compensators: [Compensator::new(); KIND_COUNT],
            select_held: [false; KIND_COUNT],
            previous_pinch: [0.0; KIND_COUNT],
            cfg: StabilizeCfg::default(),
            prefs_gen: 0,
        }
    }
}

impl Default for Stabilize {
    fn default() -> Self {
        Self::new()
    }
}

impl Stage for Stabilize {
    fn name(&self) -> &'static str {
        "stabilize:ray-lock-compensate"
    }

    fn tick(&mut self, st: &mut Zxr, _now_ns: u64) {
        if self.prefs_gen != st.prefs.generation {
            self.prefs_gen = st.prefs.generation;
            self.cfg = st.prefs.stabilize_cfg();
        }
    }

    fn run(&mut self, sample: &mut Sample, _st: &mut Zxr) -> Flow {
        if !has_ray_pose(sample.kind) {
            return Flow::Continue;
        }
        let i = kind_index(sample.kind);
        if let Some((Button::Select, pressed)) = sample.button {
            self.select_held[i] = pressed;
        }

        let cfg = self.cfg;
        if let Some(pose) = sample.pose {
            let filtered = self.filters[i].update_with(pose, sample.time_ns, cfg.position_half_life_s, cfg.direction_half_life_s);
            self.compensators[i].record(sample.time_ns, filtered);
            sample.pose = Some(filtered);
        }

        let progress = match sample.kind {
            SourceKind::Hand(_) | SourceKind::Gaze => sample.values.pinch,
            SourceKind::Head | SourceKind::Controller(_) => u8::from(self.select_held[i]) as f32,
            SourceKind::Pointer | SourceKind::Keyboard => 0.0,
        };
        let relaxation = if sample.kind == SourceKind::Gaze { cfg.relaxation_gaze } else { cfg.relaxation_ray };
        if let Some(pose) = sample.pose {
            sample.pose = Some(self.locks[i].update_with(pose, progress, relaxation, cfg.sticky));
        }

        // research/63 §3 records Meta's reason: closing a pinch shifts the hand. Apply the
        // compensation window (`input.pointer.click_freeze_ms`, default the calibration's 50 ms)
        // at the commit edge, after sticky locking, so the transport sees the onset-time target
        // rather than the shifted ray.
        let button_commit = matches!(sample.button, Some((Button::Select, true)));
        let pinch_commit = matches!(sample.kind, SourceKind::Hand(_)) && self.previous_pinch[i] < cfg.pinch_closed && sample.values.pinch >= cfg.pinch_closed;
        self.previous_pinch[i] = sample.values.pinch;
        if button_commit || pinch_commit {
            let onset = sample.time_ns.saturating_sub(cfg.compensation_ns);
            if let Some(pose) = self.compensators[i].pose_at(onset) {
                sample.pose = Some(pose);
            }
        }

        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaw(degrees: f32, x: f32) -> xr::Posef {
        let r = degrees.to_radians() * 0.5;
        xr::Posef {
            orientation: xr::Quaternionf { x: 0.0, y: r.sin(), z: 0.0, w: r.cos() },
            position: xr::Vector3f { x, y: 0.0, z: 0.0 },
        }
    }

    fn yaw_degrees(pose: xr::Posef) -> f32 {
        (2.0 * pose.orientation.y.atan2(pose.orientation.w)).to_degrees()
    }

    #[test]
    fn jitter_settles_and_step_tracks_within_three_half_lives() {
        let frame_ns = 1_000_000_000 / 90;
        let mut filter = RayFilter::new();
        filter.update(yaw(0.0, 0.0), 0);
        let mut out = IDENTITY;
        for frame in 1..=18 {
            let noise = if frame % 2 == 0 { 1.0 } else { -1.0 };
            out = filter.update(yaw(noise, 0.0), frame * frame_ns);
        }
        assert!(yaw_degrees(out).abs() < 0.1, "residual={}°", yaw_degrees(out));

        let start = 18 * frame_ns;
        for frame in 1..=13 {
            out = filter.update(yaw(10.0, 0.0), start + frame * frame_ns);
        }
        assert!((10.0 - yaw_degrees(out)).abs() <= 1.4, "step output={}°", yaw_degrees(out));
    }

    #[test]
    fn sticky_target_survives_drift_until_kind_specific_relaxation() {
        let mut ray = TargetLock::new();
        assert_eq!(ray.update(0.0_f32, 0.0, RAY_RELAXATION_THRESHOLD), 0.0);
        assert_eq!(ray.update(0.0, 0.6, RAY_RELAXATION_THRESHOLD), 0.0);
        assert_eq!(ray.update(5.0, 0.5, RAY_RELAXATION_THRESHOLD), 0.0);
        assert_eq!(ray.update(5.0, 0.49, RAY_RELAXATION_THRESHOLD), 5.0);

        let mut gaze = TargetLock::new();
        assert_eq!(gaze.update(0.0_f32, 0.6, GAZE_RELAXATION_THRESHOLD), 0.0);
        assert_eq!(gaze.update(5.0, 0.2, GAZE_RELAXATION_THRESHOLD), 0.0);
        assert_eq!(gaze.update(5.0, 0.09, GAZE_RELAXATION_THRESHOLD), 5.0);
    }

    #[test]
    fn closing_shift_commits_pose_from_fifty_ms_earlier() {
        let mut c = Compensator::new();
        c.record(1_000_000_000, yaw(0.0, 0.0));
        c.record(1_025_000_000, yaw(0.0, 0.01));
        c.record(1_050_000_000, yaw(0.0, 0.02));
        let committed = c.pose_at(1_050_000_000 - COMPENSATION_NS).unwrap();
        assert!(committed.position.x.abs() < 1e-6, "x={}", committed.position.x);
    }
}
