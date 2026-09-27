//! Scene hit stage (`docs/architecture/spatial-input.md:230-235`,
//! `specs/zxr-core.md:431-434`; comparable survey: `docs/research/63-xr-input-focus-selection-from-comparables.md:99-155`).
//!
//! The scene's nearest mapped plane is the baseline. A class callback is deliberately part of
//! the pure helper even though members do not carry a class yet: the WM floor can classify its
//! future compositor affordances without changing hit mechanics.

use openxr as xr;

use super::{Flow, Hit, Sample, Side, SourceKind, Stage};
use crate::scene::{self, MemberId, Scene, Shape};
use crate::state::Zxr;
use crate::xr::math;

/// Input arbitration class, distinct from `input::Class` (the touch/pointer transport class).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Affordance,
    Shell,
    Content,
}

impl Class {
    fn priority(self) -> u8 {
        match self {
            Class::Content => 0,
            Class::Shell => 1,
            Class::Affordance => 2,
        }
    }

    /// Until members carry an explicit class, composition bands are the hook: bands 4–5 are
    /// shell planes and band 3 is client content (`specs/zxr-core.md:96-107`).
    pub fn from_band(band: Option<u8>) -> Self {
        match band {
            Some(4 | 5) => Class::Shell,
            _ => Class::Content,
        }
    }
}

/// Stand-in: a shell/affordance may lose up to 2 cm of ray depth to content and still win.
/// spatial-input §4 specifies the affordance depth-offset rule but no value, and the surveyed
/// comparables provide none; this must be measured/replaced when WM affordance geometry lands.
pub const CLASS_DEPTH_EPSILON_M: f32 = 0.02;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MemberHit {
    pub member: MemberId,
    pub local: [f32; 2],
    pub distance: f32,
    pub class: Class,
}

fn candidate_wins(candidate: MemberHit, current: MemberHit, epsilon_m: f32) -> bool {
    let cp = candidate.class.priority();
    let bp = current.class.priority();
    if cp > bp && candidate.distance <= current.distance + epsilon_m {
        return true;
    }
    if bp > cp && current.distance <= candidate.distance + epsilon_m {
        return false;
    }
    candidate.distance < current.distance
}

/// Pure, allocation-free member hit with a future-proof member-class parameter.
///
/// `Scene::hit` supplies the normal nearest-plane answer. The second pass only applies the
/// spatial-input §4 class rule: a higher class may win while it remains within the depth-offset
/// band. With all members classified alike, this is exactly nearest-hit.
pub fn hit_member<M>(
    scene: &Scene<M>,
    origin: [f32; 3],
    dir: [f32; 3],
    mapped: impl Fn(&M) -> bool,
    class_of: impl Fn(MemberId, Option<u8>) -> Class,
) -> Option<MemberHit> {
    hit_member_with(scene, origin, dir, mapped, class_of, CLASS_DEPTH_EPSILON_M)
}

/// [`hit_member`] with the depth-offset band from the calibration
/// (`hardware.input.hit.class_epsilon_m`).
pub fn hit_member_with<M>(
    scene: &Scene<M>,
    origin: [f32; 3],
    dir: [f32; 3],
    mapped: impl Fn(&M) -> bool,
    class_of: impl Fn(MemberId, Option<u8>) -> Class,
    epsilon_m: f32,
) -> Option<MemberHit> {
    let (member, local, distance) = scene.hit(origin, dir, |m| mapped(m))?;
    let mut best = MemberHit { member, local, distance, class: class_of(member, scene.band(member)) };

    for (id, member) in scene.iter() {
        if !mapped(&member.m) {
            continue;
        }
        let Shape::Plane { size } = member.shape else { continue };
        let Some(world) = scene.world_pose(id) else { continue };
        let Some((distance, local)) = scene::ray_plane(origin, dir, world, [size[0] * 0.5, size[1] * 0.5]) else { continue };
        let candidate = MemberHit { member: id, local, distance, class: class_of(id, scene.band(id)) };
        if candidate_wins(candidate, best, epsilon_m) {
            best = candidate;
        }
    }
    Some(best)
}

fn xr_kind(kind: SourceKind) -> bool {
    matches!(kind, SourceKind::Head | SourceKind::Gaze | SourceKind::Hand(_) | SourceKind::Controller(_))
}

/// MRTK3's poke reticle magnetism range (`ReticleMagnetism.cs:37-40` `magnetRange = 0.07f`):
/// the reference shape spatial-input §4 names for `input.magnetism.enabled`.
pub const MAGNET_RANGE_M: f32 = 0.07;

pub struct HitStage {
    /// Total member hits emitted since stage construction; logged every 60 ticks.
    pub hit_count: u64,
    ticks: u64,
    /// `input.magnetism.enabled` (settings.rs; off by default, spatial-input §4)
    pub magnetism: bool,
    /// `hardware.input.hit.class_epsilon_m` (the calibration; settings.rs)
    pub class_epsilon_m: f32,
    /// poke hits produced by magnetism rather than the ray
    pub magnetised: u64,
    prefs_gen: u64,
}

impl HitStage {
    pub const fn new() -> Self {
        Self { hit_count: 0, ticks: 0, magnetism: false, class_epsilon_m: CLASS_DEPTH_EPSILON_M, magnetised: 0, prefs_gen: 0 }
    }

    /// Poke magnetism: the fingertip's ray missed every plane; if a mapped plane's nearest point
    /// is within [`MAGNET_RANGE_M`], that point is the poke's hit (MRTK3 `ReticleMagnetism`:
    /// the reticle moves to the collider's closest point inside the range, `:162-200`).
    fn magnetise(&mut self, kind: SourceKind, tip: xr::Posef, time_ns: u64, st: &mut Zxr) {
        let p = [tip.position.x, tip.position.y, tip.position.z];
        let mut best: Option<(f32, MemberId, [f32; 2])> = None;
        let gated = st.input.mode != super::Mode::Normal;
        for (id, member) in st.scene.iter() {
            if !(member.m.mapped() && !member.m.hidden && (!gated || member.m.trusted)) {
                continue;
            }
            let Shape::Plane { size } = member.shape else { continue };
            let Some(world) = st.scene.world_pose(id) else { continue };
            let (d, local) = scene::nearest_point_on_plane(p, world, [size[0] * 0.5, size[1] * 0.5]);
            if d <= MAGNET_RANGE_M && best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, id, local));
            }
        }
        if let Some((distance, member, local)) = best {
            st.input.hits.push(Hit { kind, member, local, distance, time_ns });
            self.hit_count += 1;
            self.magnetised += 1;
        }
    }

    fn cast(&mut self, kind: SourceKind, pose: xr::Posef, time_ns: u64, st: &mut Zxr) {
        let origin = [pose.position.x, pose.position.y, pose.position.z];
        let dir = math::rotate(pose.orientation, [0.0, 0.0, -1.0]);
        // while the mode gate is closed only the trusted members are composed, so only they are
        // hit (spec §9 rev 3.12)
        let gated = st.input.mode != super::Mode::Normal;
        let Some(hit) = hit_member_with(
            &st.scene,
            origin,
            dir,
            |p| p.mapped() && !p.hidden && (!gated || p.trusted),
            |_, band| Class::from_band(band),
            self.class_epsilon_m,
        ) else {
            return;
        };

        // zen's parent-bubbling contract, recorded in research/63 §2: a missing child surface
        // does not reject the already-hit member plane. `hit_surface_at == None` therefore falls
        // back to this member/local pair (the toplevel geometry) for the transport lane.
        let _surface_declined = st.hit_surface_at(hit.member, hit.local).is_none();
        st.input.hits.push(Hit {
            kind,
            member: hit.member,
            local: hit.local,
            distance: hit.distance,
            time_ns,
        });
        self.hit_count += 1;
    }
}

impl Default for HitStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Stage for HitStage {
    fn name(&self) -> &'static str {
        "hit:scene-members"
    }

    fn run(&mut self, sample: &mut Sample, st: &mut Zxr) -> Flow {
        if st.input.xr_suspended && xr_kind(sample.kind) {
            return Flow::Continue;
        }

        // Temporary integration rule requested for the lane: before the Tier stage lands,
        // `None` keeps the head floor targeting while allowing every other ray kind to prove its
        // hit path. Once Tier writes a selection, only that kind's aim ray is eligible.
        let aim_targets = st.input.tier.map(|selection| selection.targeting == sample.kind).unwrap_or_else(|| sample.kind.targets());
        if aim_targets {
            if let Some(pose) = sample.pose {
                self.cast(sample.kind, pose, sample.time_ns, st);
            }
        }

        // Direct hand poke is eligible independently of the far-ray tier (spatial-input §3–§4).
        // Both hits are side data when a sample carries both poses; lane C can apply the direct
        // touch band without this stage suppressing either observation.
        if matches!(sample.kind, SourceKind::Hand(Side::Left | Side::Right)) {
            if let Some(pose) = sample.poke_pose {
                let before = st.input.hits.len();
                self.cast(sample.kind, pose, sample.time_ns, st);
                if self.magnetism && st.input.hits.len() == before {
                    self.magnetise(sample.kind, pose, sample.time_ns, st);
                }
            }
        }
        Flow::Continue
    }

    fn tick(&mut self, st: &mut Zxr, _time_ns: u64) {
        if self.prefs_gen != st.prefs.generation {
            self.prefs_gen = st.prefs.generation;
            self.magnetism = st.prefs.magnetism_enabled;
            self.class_epsilon_m = st.prefs.hardware.hit_class_epsilon_m.max(0.0);
        }
        self.ticks += 1;
        if self.ticks % 60 == 0 {
            tracing::debug!(hits = self.hit_count, ticks = self.ticks, "input hit-stage counter");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Flags, Shape};

    fn pose(z: f32) -> xr::Posef {
        math::pose_yaw([0.0, 0.0, z], 0.0)
    }

    fn plane() -> Shape {
        Shape::Plane { size: [1.0, 1.0] }
    }

    #[test]
    fn nearest_mapped_plane_wins() {
        let mut scene: Scene<bool> = Scene::new();
        let far = scene.add(scene.default_place, pose(-2.0), plane(), Flags::WINDOW, true).unwrap();
        let near = scene.add(scene.default_place, pose(-1.0), plane(), Flags::WINDOW, true).unwrap();
        let hit = hit_member(&scene, [0.0; 3], [0.0, 0.0, -1.0], |mapped| *mapped, |_, band| Class::from_band(band)).unwrap();
        assert_eq!(hit.member, near);
        assert_ne!(hit.member, far);
        assert!((hit.distance - 1.0).abs() < 1e-5);
    }

    #[test]
    fn band_five_beats_nearer_content_within_standin_epsilon() {
        let mut scene: Scene<bool> = Scene::new();
        let overlay_place = scene.add_place(scene.world, math::pose_identity(), 5);
        let content = scene.add(scene.default_place, pose(-1.0), plane(), Flags::WINDOW, true).unwrap();
        let overlay =
            scene.add(overlay_place, pose(-(1.0 + CLASS_DEPTH_EPSILON_M * 0.5)), plane(), Flags::default(), true).unwrap();
        let hit = hit_member(&scene, [0.0; 3], [0.0, 0.0, -1.0], |mapped| *mapped, |_, band| Class::from_band(band)).unwrap();
        assert_eq!(hit.member, overlay);
        assert_ne!(hit.member, content);
        assert_eq!(hit.class, Class::Shell);
    }
}
