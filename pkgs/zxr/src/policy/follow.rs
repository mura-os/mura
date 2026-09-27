//! Attachment (window-workspace-management §7; places-model layer 2): `rigid` by default (ruled
//! 2026-09-26 — "apps stay in the workplace they originated"), `lazy-follow` opt-in per window.
//!
//! **Lazy-follow** (HoloLens "Follow me", Horizon "move with you"; kwin-vr's follow —
//! `kwinvr.kcfg:37-61`: `followFovH` 40°, `followDelay` 0.5 s, `followSpeed` 2.0,
//! `followStopFovH` 4°): when the head has been more than `threshold` off the member's centre for
//! `delay`, the member re-seats toward the head's forward at `rate` per second and stops within
//! `stop`; never while a member of the place is grabbed (WayVR `pause_movement`). The re-seat
//! rotates the member about the head at its distance (kwin-vr `rotateGrabbedObjectAroundCameraToRay`
//! is the same motion), facing the head upright.
//!
//! **Out-of-view fallback** (§7) is a presentation in the head frame, the overlay-class place of
//! places-model §4.3 — not built here (no shell client asks for it yet). **Billboard** while moving
//! is the grab's (`input/grabs.rs`).

use openxr as xr;

use crate::scene::MemberId;
use crate::state::Zxr;

/// `wm.follow.*` (settings.rs `Prefs::policy_cfg`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FollowCfg {
    pub threshold_deg: f32,
    pub delay_ms: u64,
    /// the rate is kwin-vr's `followSpeed`: a fraction of the remaining angle per second
    pub rate: f32,
    pub stop_deg: f32,
}

impl Default for FollowCfg {
    fn default() -> Self {
        FollowCfg { threshold_deg: 40.0, delay_ms: 500, rate: 2.0, stop_deg: 4.0 }
    }
}

/// A lazy-following member's timer state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Follower {
    /// when the head first went past the threshold (None = within it)
    pub off_since_ns: Option<u64>,
    /// moving toward the head's forward until within `stop`
    pub moving: bool,
    pub last_ns: Option<u64>,
}

/// How a member is attached to its place (§7).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Attachment {
    #[default]
    Rigid,
    LazyFollow(Follower),
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 { [0.0, 0.0, -1.0] } else { [v[0] / l, v[1] / l, v[2] / l] }
}

/// The angle (radians) between the head's forward and the direction to `p`.
pub fn off_angle(head: xr::Posef, p: xr::Vector3f) -> f32 {
    let f = crate::xr::math::rotate(head.orientation, [0.0, 0.0, -1.0]);
    let d = norm([p.x - head.position.x, p.y - head.position.y, p.z - head.position.z]);
    (f[0] * d[0] + f[1] * d[1] + f[2] * d[2]).clamp(-1.0, 1.0).acos()
}

/// One step of the re-seat: rotate `p` about the head toward the head's forward by at most
/// `max_step` radians (a slerp of the direction at the same distance). Returns the new position
/// and the remaining angle.
pub fn step_toward(head: xr::Posef, p: xr::Vector3f, max_step: f32) -> (xr::Vector3f, f32) {
    let f = norm(crate::xr::math::rotate(head.orientation, [0.0, 0.0, -1.0]));
    let dv = [p.x - head.position.x, p.y - head.position.y, p.z - head.position.z];
    let dist = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
    if dist < 1e-5 {
        return (p, 0.0);
    }
    let d = [dv[0] / dist, dv[1] / dist, dv[2] / dist];
    let cos = (f[0] * d[0] + f[1] * d[1] + f[2] * d[2]).clamp(-1.0, 1.0);
    let angle = cos.acos();
    if angle < 1e-4 {
        return (p, 0.0);
    }
    let t = (max_step / angle).min(1.0);
    // slerp d → f by t
    let sin = angle.sin();
    let (a, b) = if sin < 1e-5 { (1.0 - t, t) } else { (((1.0 - t) * angle).sin() / sin, (t * angle).sin() / sin) };
    let nd = norm([a * d[0] + b * f[0], a * d[1] + b * f[1], a * d[2] + b * f[2]]);
    let np = xr::Vector3f { x: head.position.x + nd[0] * dist, y: head.position.y + nd[1] * dist, z: head.position.z + nd[2] * dist };
    (np, angle - angle * t)
}

/// The per-tick follow pass (called from `policy::tick`).
pub fn tick(st: &mut Zxr, now_ns: u64) {
    let Some(head) = st.input.head else { return };
    let cfg = st.policy.cfg.follow;
    let grabbed = st.input.grabbed;
    let followers: Vec<MemberId> = st.policy.followers();
    for member in followers {
        if grabbed == Some(member) {
            // WayVR `pause_movement`: a grab owns the pose
            if let Attachment::LazyFollow(f) = &mut st.policy.state_mut(member).attachment {
                f.off_since_ns = None;
                f.moving = false;
                f.last_ns = Some(now_ns);
            }
            continue;
        }
        let Some(world) = st.scene.world_pose(member) else { continue };
        let angle = off_angle(head, world.position);
        let mut new_pos = None;
        {
            let Attachment::LazyFollow(f) = &mut st.policy.state_mut(member).attachment else { continue };
            let dt = f.last_ns.map(|l| now_ns.saturating_sub(l) as f32 * 1e-9).unwrap_or(0.0).min(0.1);
            f.last_ns = Some(now_ns);
            if f.moving {
                if angle.to_degrees() <= cfg.stop_deg {
                    f.moving = false;
                    f.off_since_ns = None;
                } else {
                    // kwin-vr's `followSpeed`: the remaining angle shrinks by rate·dt per tick
                    let step = angle * (cfg.rate * dt).min(1.0);
                    let (p, _) = step_toward(head, world.position, step.max(0.0));
                    new_pos = Some(p);
                }
            } else if angle.to_degrees() > cfg.threshold_deg {
                let since = *f.off_since_ns.get_or_insert(now_ns);
                if now_ns.saturating_sub(since) >= cfg.delay_ms.saturating_mul(1_000_000) {
                    f.moving = true;
                }
            } else {
                f.off_since_ns = None;
            }
        }
        if let Some(p) = new_pos {
            let pose = xr::Posef { orientation: crate::input::grabs::face(p, head.position), position: p };
            super::set_pose(st, member, pose);
            st.policy.follows += 1;
        }
    }
}

impl super::Policy {
    /// Members with a lazy-follow attachment.
    pub fn followers(&self) -> Vec<MemberId> {
        self.members_map().iter().filter(|(_, s)| matches!(s.attachment, Attachment::LazyFollow(_))).map(|(id, _)| *id).collect()
    }

    /// `set_follow` (the bar's or the launcher's opt-in; a manager's `set_flags` later).
    pub fn set_follow(&mut self, member: MemberId, on: bool) {
        let s = self.state_mut(member);
        s.attachment = if on { Attachment::LazyFollow(Follower::default()) } else { Attachment::Rigid };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xr::math;

    #[test]
    fn off_angle_and_the_step_toward_the_forward() {
        let head = math::pose_yaw([0.0, 0.0, 0.0], 0.0);
        // a plane 60° to the left at 1.5 m
        let a = (60.0f32).to_radians();
        let p = xr::Vector3f { x: -1.5 * a.sin(), y: 0.0, z: -1.5 * a.cos() };
        assert!((off_angle(head, p).to_degrees() - 60.0).abs() < 1e-3);
        // a 10° step keeps the distance and reduces the angle by 10°
        let (np, remaining) = step_toward(head, p, (10.0f32).to_radians());
        let d = (np.x * np.x + np.z * np.z).sqrt();
        assert!((d - 1.5).abs() < 1e-4);
        assert!((remaining.to_degrees() - 50.0).abs() < 1e-2);
        assert!((off_angle(head, np).to_degrees() - 50.0).abs() < 1e-2);
        // a step past the target lands exactly ahead
        let (np, remaining) = step_toward(head, p, 2.0);
        assert!(remaining.abs() < 1e-4 && np.x.abs() < 1e-4 && (np.z + 1.5).abs() < 1e-4);
    }
}
