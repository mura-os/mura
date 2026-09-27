//! The `free` engine's geometry (window-workspace-management §3, §4): the spawn pose and the
//! angular free-slot allocator.
//!
//! **Spawn pose** — head-relative: `spawn.distance` along the head's forward projected to the
//! horizontal, `spawn.elevation` below the eye line, facing the head (visionOS "where they're
//! looking… about two meters in front"; Android XR "5° below eye level"; Horizon "slightly below
//! the line of sight" — research/64 §2 [external]; kwin-vr `distance` + `turnToFaceKeepRoll`).
//!
//! **The allocator** — kwin-vr's `SpaceAllocator3D` (`spaceallocator3d.cpp:262-300`): every
//! existing member is projected onto the sphere around the head as an angular box
//! (azimuth × elevation); candidate slots are generated in concentric rings from the forward
//! direction (`generateSpherePoints`, `:225-246`, ring step `atan(w/d) + spacing` × granularity),
//! and the first candidate whose box overlaps no existing box wins; when none does, straight
//! ahead ("Fallback… what would be better to do here?", `:298-299`). Here the candidates are
//! walked eye-line band first (columns of azimuth, then the bands above and below) so a second
//! window lands beside the first (research/36 §2 "second window adjacent"). zen's seat capsule
//! (`zns/include/zns/bounded.h:18-24`) is the same idea with polar coordinates and no overlap
//! test. Spacing 0.05 rad is kwin-vr's default (`spaceallocator3d.h:91`), granularity 0.5 (`:96`).

use openxr as xr;

use super::Limits;
use crate::xr::math;

/// `wm.spawn.*` (settings.rs `Prefs::policy_cfg`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnCfg {
    pub distance_m: f32,
    /// degrees; negative = below the eye line
    pub elevation_deg: f32,
    /// lateral offset of a sibling from its parent, metres
    pub sibling_offset_m: f32,
    /// the sibling's yaw toward the viewer, radians
    pub sibling_yaw_rad: f32,
    /// angular gap between slots, radians (kwin-vr `spacing`)
    pub spacing_rad: f32,
}

impl Default for SpawnCfg {
    fn default() -> Self {
        SpawnCfg { distance_m: 1.5, elevation_deg: 0.0, sibling_offset_m: 0.9, sibling_yaw_rad: 0.35, spacing_rad: 0.05 }
    }
}

/// The head's view basis: position, horizontal forward, right, world up.
#[derive(Clone, Copy, Debug)]
pub struct ViewBasis {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 { [0.0, 0.0, -1.0] } else { [v[0] / l, v[1] / l, v[2] / l] }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl ViewBasis {
    /// From a head pose: the forward is the head's −Z projected to the horizontal (a wearer
    /// looking down still gets windows at eye level — visionOS "regardless of… whether they're
    /// sitting, standing, or lying down").
    pub fn from_head(head: xr::Posef) -> ViewBasis {
        let f = math::rotate(head.orientation, [0.0, 0.0, -1.0]);
        let mut fh = [f[0], 0.0, f[2]];
        if fh[0] * fh[0] + fh[2] * fh[2] < 1e-6 {
            // looking straight up or down: keep the head's +Y-projected right as the reference
            let r = math::rotate(head.orientation, [1.0, 0.0, 0.0]);
            fh = cross([0.0, 1.0, 0.0], [r[0], 0.0, r[2]]);
        }
        let forward = norm(fh);
        let up = [0.0, 1.0, 0.0];
        let right = norm(cross(forward, up));
        ViewBasis { position: [head.position.x, head.position.y, head.position.z], forward, right, up }
    }

    /// The angular box a plane of `size` metres centred at `centre` subtends from the head.
    pub fn project(&self, centre: xr::Vector3f, size: [f32; 2]) -> AngularBounds {
        let d = [centre.x - self.position[0], centre.y - self.position[1], centre.z - self.position[2]];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(0.05);
        let az = dot(d, self.right).atan2(dot(d, self.forward));
        let el = (dot(d, self.up) / dist).clamp(-1.0, 1.0).asin();
        let half_az = (size[0] * 0.5 / dist).atan();
        let half_el = (size[1] * 0.5 / dist).atan();
        AngularBounds { centre_az: az, centre_el: el, half_az, half_el }
    }

    /// The world point at `distance` in direction (azimuth, elevation) from the head
    /// (kwin-vr `angularToWorld`, `spaceallocator3d.cpp:208-223`).
    pub fn to_world(&self, az: f32, el: f32, distance: f32) -> [f32; 3] {
        let (ce, se, ca, sa) = (el.cos(), el.sin(), az.cos(), az.sin());
        let dir = [
            self.forward[0] * ce * ca + self.right[0] * ce * sa + self.up[0] * se,
            self.forward[1] * ce * ca + self.right[1] * ce * sa + self.up[1] * se,
            self.forward[2] * ce * ca + self.right[2] * ce * sa + self.up[2] * se,
        ];
        let dir = norm(dir);
        [self.position[0] + dir[0] * distance, self.position[1] + dir[1] * distance, self.position[2] + dir[2] * distance]
    }
}

/// A plane's footprint on the sphere around the head, radians (kwin-vr `AngularBounds`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngularBounds {
    pub centre_az: f32,
    pub centre_el: f32,
    pub half_az: f32,
    pub half_el: f32,
}

impl AngularBounds {
    pub fn overlaps(&self, o: &AngularBounds) -> bool {
        let (a0, a1) = (self.centre_az - self.half_az, self.centre_az + self.half_az);
        let (b0, b1) = (o.centre_az - o.half_az, o.centre_az + o.half_az);
        if a1 < b0 || b1 < a0 {
            return false;
        }
        let (c0, c1) = (self.centre_el - self.half_el, self.centre_el + self.half_el);
        let (d0, d1) = (o.centre_el - o.half_el, o.centre_el + o.half_el);
        !(c1 < d0 || d1 < c0)
    }
}

impl SpawnCfg {
    /// The engine's spawn pose for a head: straight ahead at the distance and elevation, facing
    /// the head, upright.
    pub fn spawn_pose(&self, head: xr::Posef) -> xr::Posef {
        let basis = ViewBasis::from_head(head);
        let el = self.elevation_deg.to_radians();
        let p = basis.to_world(0.0, el, self.distance_m.max(0.1));
        let pos = xr::Vector3f { x: p[0], y: p[1], z: p[2] };
        xr::Posef { orientation: crate::input::grabs::face(pos, head.position), position: pos }
    }

    /// A sibling's pose beside its parent: `sibling_offset` to the parent's `side` (−1 left,
    /// +1 right along the parent's own X), turned `sibling_yaw` further toward the viewer, then
    /// re-faced to the head so it stays upright (WayVR `Spread` — left/down/closer — is the shape;
    /// the fan's offset and yaw are the declared numbers).
    pub fn sibling_pose(&self, parent: xr::Posef, side: f32, head: xr::Posef) -> xr::Posef {
        let p = math::pose_apply(parent, [side * self.sibling_offset_m, 0.0, 0.0]);
        let pos = xr::Vector3f { x: p[0], y: p[1], z: p[2] };
        let _ = self.sibling_yaw_rad; // the facing below yaws the sibling toward the head
        xr::Posef { orientation: crate::input::grabs::face(pos, head.position), position: pos }
    }
}

/// The azimuth the allocator fills before it uses the bands above and below: ±60°. **Stand-in,
/// flagged**: the comparables give a comfort *zone* (Android XR "the center 41° of a user's field
/// of view", HoloLens' comfort guidance [external]) but no rule for when to leave it; 60° is the
/// widest a seated wearer turns without moving the body (research/64 §2's discussion), so a
/// third window goes above or below rather than behind the shoulder.
pub const AZ_COMFORT_RAD: f32 = 60.0 * std::f32::consts::PI / 180.0;

/// The free-slot search.
pub struct Allocator {
    cfg: SpawnCfg,
    limits: Limits,
    /// kwin-vr `searchGranularity`: the ring step as a fraction of a slot
    granularity: f32,
}

impl Allocator {
    pub fn new(cfg: SpawnCfg, limits: Limits) -> Allocator {
        Allocator { cfg, limits, granularity: 0.5 }
    }

    /// Find a free slot for a plane of `size` given the `occupied` boxes; returns the pose
    /// (facing the head, upright) and whether it fell back to straight ahead.
    pub fn find(&self, head: xr::Posef, size: [f32; 2], occupied: &[AngularBounds]) -> (xr::Posef, bool) {
        let basis = ViewBasis::from_head(head);
        let d = self.cfg.distance_m.clamp(self.limits.min_distance_m, self.limits.max_distance_m);
        let el0 = self.cfg.elevation_deg.to_radians();
        let half_az = (size[0] * 0.5 / d).atan();
        let half_el = (size[1] * 0.5 / d).atan();
        let step = ((size[0] / d).atan() + self.cfg.spacing_rad) * self.granularity;
        // candidates: the centre, then concentric rings out to 180°
        let place_at = |az: f32, el: f32| -> Option<xr::Posef> {
            let c = AngularBounds { centre_az: az, centre_el: el, half_az: half_az + self.cfg.spacing_rad * 0.5, half_el: half_el + self.cfg.spacing_rad * 0.5 };
            if occupied.iter().any(|o| o.overlaps(&c)) {
                return None;
            }
            let p = basis.to_world(az, el, d);
            let pos = xr::Vector3f { x: p[0], y: p[1], z: p[2] };
            Some(xr::Posef { orientation: crate::input::grabs::face(pos, head.position), position: pos })
        };
        if let Some(p) = place_at(0.0, el0) {
            return (p, false);
        }
        // kwin-vr walks concentric rings; here the same candidate density is walked **eye-line
        // band first**: every azimuth on the spawn elevation before the band above or below it, so
        // a second window lands beside the first, not over it (research/36 §2, research/64 §2:
        // "second window adjacent" — visionOS, Horizon, Android XR). Bands and columns step by
        // the slot's own angular size × the granularity.
        let step_el = (((size[1] / d).atan() + self.cfg.spacing_rad) * self.granularity).max(0.01);
        let step_az = step.max(0.01);
        let n_el = (std::f32::consts::FRAC_PI_2 / step_el).ceil() as i32;
        let n_az = (std::f32::consts::PI / step_az).ceil() as i32;
        // two passes: the comfortable azimuth range first (bands above and below before a window
        // goes behind the shoulder), then the rest of the sphere
        for &az_limit in &[AZ_COMFORT_RAD, std::f32::consts::PI] {
            for band in 0..=n_el {
                for &sign_el in if band == 0 { &[1.0f32][..] } else { &[1.0f32, -1.0][..] } {
                    let el = el0 + sign_el * band as f32 * step_el;
                    if el.abs() > std::f32::consts::FRAC_PI_2 {
                        continue;
                    }
                    for col in 0..=n_az {
                        let az_abs = col as f32 * step_az;
                        if az_abs > az_limit {
                            break;
                        }
                        for &sign_az in if col == 0 { &[1.0f32][..] } else { &[1.0f32, -1.0][..] } {
                            let az = sign_az * az_abs;
                            if band == 0 && col == 0 {
                                continue; // the centre was tried first
                            }
                            if let Some(p) = place_at(az, el) {
                                return (p, false);
                            }
                        }
                    }
                }
            }
        }
        let p = basis.to_world(0.0, el0, d);
        let pos = xr::Vector3f { x: p[0], y: p[1], z: p[2] };
        (xr::Posef { orientation: crate::input::grabs::face(pos, head.position), position: pos }, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head_at(yaw: f32) -> xr::Posef {
        math::pose_yaw([0.0, 1.6, 0.0], yaw)
    }

    #[test]
    fn spawn_pose_is_ahead_at_the_distance_below_the_eye_line_facing_the_head() {
        let cfg = SpawnCfg { distance_m: 2.0, elevation_deg: -5.0, ..SpawnCfg::default() };
        let p = cfg.spawn_pose(head_at(0.0));
        assert!((p.position.z + 2.0 * (5.0f32).to_radians().cos()).abs() < 1e-4, "{:?}", p.position);
        assert!(p.position.y < 1.6, "below the eye line");
        assert!(((1.6 - p.position.y) - 2.0 * (5.0f32).to_radians().sin()).abs() < 1e-4);
        // the plane's +Z looks back at the head
        let n = math::rotate(p.orientation, [0.0, 0.0, 1.0]);
        let to_head = norm([0.0 - p.position.x, 1.6 - p.position.y, 0.0 - p.position.z]);
        assert!(dot(n, to_head) > 0.999);
        // a head yawed 90° left spawns to the left; a head pitched down still spawns at eye level
        let p = cfg.spawn_pose(head_at(std::f32::consts::FRAC_PI_2));
        assert!(p.position.x < -1.9, "{:?}", p.position);
        let down = xr::Posef { orientation: xr::Quaternionf { x: -0.383, y: 0.0, z: 0.0, w: 0.924 }, position: xr::Vector3f { x: 0.0, y: 1.6, z: 0.0 } };
        let p2 = cfg.spawn_pose(down);
        assert!((p2.position.y - p.position.y).abs() < 1e-3, "the horizontal projection");
    }

    #[test]
    fn the_allocator_never_overlaps_and_prefers_the_sides() {
        let cfg = SpawnCfg::default();
        let alloc = Allocator::new(cfg, Limits::default());
        let head = head_at(0.0);
        let basis = ViewBasis::from_head(head);
        let size = [0.8, 0.6];
        let mut occupied = Vec::new();
        let mut poses = Vec::new();
        for _ in 0..6 {
            let (p, fallback) = alloc.find(head, size, &occupied);
            assert!(!fallback);
            let b = basis.project(p.position, size);
            for o in &occupied {
                assert!(!b.overlaps(o), "slot overlaps an earlier one: {b:?} vs {o:?}");
            }
            occupied.push(b);
            poses.push(p);
        }
        // the first is straight ahead, the second and third beside it at the same height
        assert!(poses[0].position.x.abs() < 1e-3);
        assert!((poses[1].position.y - poses[0].position.y).abs() < 0.05 && poses[1].position.x.abs() > 0.5, "{:?}", poses[1].position);
        // every slot is at the spawn distance
        for p in &poses {
            let d = ((p.position.x).powi(2) + (p.position.y - 1.6).powi(2) + (p.position.z).powi(2)).sqrt();
            assert!((d - 1.5).abs() < 1e-3);
        }
    }

    #[test]
    fn a_full_sphere_falls_back_to_straight_ahead() {
        let alloc = Allocator::new(SpawnCfg::default(), Limits::default());
        let head = head_at(0.0);
        // one enormous box covers everything
        let occupied = [AngularBounds { centre_az: 0.0, centre_el: 0.0, half_az: 10.0, half_el: 10.0 }];
        let (p, fallback) = alloc.find(head, [0.8, 0.6], &occupied);
        assert!(fallback);
        assert!(p.position.x.abs() < 1e-3 && p.position.z < 0.0);
    }

    #[test]
    fn limits_clamp_the_distance_along_the_head_ray() {
        let l = Limits::default();
        let head = xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 };
        let near = l.clamp_pose(math::pose_yaw([0.0, 0.0, -0.1], 0.0), head);
        assert!((near.position.z + 0.4).abs() < 1e-5);
        let far = l.clamp_pose(math::pose_yaw([0.0, 0.0, -9.0], 0.0), head);
        assert!((far.position.z + 5.0).abs() < 1e-5);
        assert!((l.max_width_m(1.0) - 2.0).abs() < 1e-4, "90° at 1 m is 2 m wide");
    }
}
