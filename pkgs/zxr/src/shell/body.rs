//! The body frame (spec §5; shell-plane §2; `zxr-layer-anchoring-v1` `body`): **the head's
//! position and the head's yaw, pitch and roll removed, the yaw re-seated lazily.**
//!
//! No headset tracks a torso; every comparable derives "body" from the head the same way and
//! differs only in when the yaw re-seats. MRTK3's `Follow` solver keeps the element in front
//! of the head, yaw-only when `IgnoreReferencePitchAndRoll`, and re-orients only past an
//! angular leash (30° horizontal by default — `mrtk3/…/Solvers/Follow.cs:88-206, 466-582`);
//! Overte's avatar torso is the head with roll and pitch cancelled, rotated to the head's facing
//! once a moving average of it is more than 30° off (`overte/interface/src/avatar/MyAvatar.cpp:4478-4501,
//! 5229-5243`; `MyAvatar.h:2697`); wayvr's anchor is the HMD pose snapped upright, captured when
//! the overlay set is shown and held until the next show or a grab (`wayvr/wayvr/src/windowing/
//! manager.rs:1130-1135`, `windowing/mod.rs:33-49`); visionOS seeds placement from the head and
//! re-seats only on an explicit recenter (research/64). The position follows the head every tick
//! in all of them (height is head height — sitting, standing or lying down).
//!
//! The re-seat is the lazy-follow the window manager already has (`policy/follow.rs`: kwin-vr's
//! threshold/delay/rate/stop, the wearer's `wm.follow.*` values) applied to one yaw instead of a
//! member's position: when the head's yaw has been more than `threshold` off the body's for
//! `delay`, the body turns toward it at `rate` per second and stops within `stop`. Per tick: one
//! `atan2`, one compare, and a slerp only while moving.

use openxr as xr;

use crate::policy::follow::{FollowCfg, Follower};
use crate::state::Zxr;
use crate::xr::math;

/// The heading (radians about +Y) of an orientation's forward, the pitch and roll discarded.
pub fn yaw_of(q: xr::Quaternionf) -> f32 {
    let f = math::rotate(q, [0.0, 0.0, -1.0]);
    // forward (0,0,-1) is yaw 0; +x is a positive (left-handed about +Y) turn in OpenXR's frame
    (-f[0]).atan2(-f[2])
}

/// Signed difference `to − from` wrapped to (−π, π].
pub fn yaw_delta(from: f32, to: f32) -> f32 {
    let mut d = to - from;
    while d > core::f32::consts::PI {
        d -= 2.0 * core::f32::consts::PI;
    }
    while d <= -core::f32::consts::PI {
        d += 2.0 * core::f32::consts::PI;
    }
    d
}

/// The body's own state: its yaw and the re-seat timer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub yaw: f32,
    pub seated: bool,
    pub follower: Follower,
    pub reseats: u64,
}

impl Body {
    /// One tick: the body's pose from the head's, pure. Returns the pose to write.
    pub fn tick(&mut self, head: xr::Posef, cfg: &FollowCfg, now_ns: u64) -> xr::Posef {
        let head_yaw = yaw_of(head.orientation);
        if !self.seated {
            // the first head pose seeds the body (wayvr's capture, visionOS's placement)
            self.yaw = head_yaw;
            self.seated = true;
            self.follower.last_ns = Some(now_ns);
        }
        let delta = yaw_delta(self.yaw, head_yaw);
        let angle = delta.abs();
        let f = &mut self.follower;
        let dt = f.last_ns.map(|l| now_ns.saturating_sub(l) as f32 * 1e-9).unwrap_or(0.0).min(0.1);
        f.last_ns = Some(now_ns);
        if f.moving {
            if angle.to_degrees() <= cfg.stop_deg {
                f.moving = false;
                f.off_since_ns = None;
            } else {
                // kwin-vr's `followSpeed`: the remaining angle shrinks by rate·dt per tick
                let step = angle * (cfg.rate * dt).min(1.0);
                self.yaw += delta.signum() * step;
                self.reseats += 1;
            }
        } else if angle.to_degrees() > cfg.threshold_deg {
            let since = *f.off_since_ns.get_or_insert(now_ns);
            if now_ns.saturating_sub(since) >= cfg.delay_ms.saturating_mul(1_000_000) {
                f.moving = true;
            }
        } else {
            f.off_since_ns = None;
        }
        math::pose_yaw([head.position.x, head.position.y, head.position.z], self.yaw)
    }
}

/// The per-tick write of the body frame, after the head's (`main.rs`).
pub fn tick(st: &mut Zxr, head: xr::Posef, now_ns: u64) {
    let cfg = st.policy.cfg.follow;
    let before = st.shell.body.reseats;
    let pose = st.shell.body.tick(head, &cfg, now_ns);
    st.journal.body_reseat_ticks += st.shell.body.reseats - before;
    let id = st.scene.body;
    st.scene.set_frame_pose(id, pose, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    fn cfg() -> FollowCfg {
        FollowCfg { threshold_deg: 40.0, delay_ms: 500, rate: 2.0, stop_deg: 4.0 }
    }

    fn head(yaw: f32, pitch: f32) -> xr::Posef {
        // yaw about +Y then pitch about +X (a head looking down while turned)
        let y = math::pose_yaw([0.0, 1.6, 0.0], yaw);
        let (s, c) = (pitch * 0.5).sin_cos();
        let p = xr::Quaternionf { x: s, y: 0.0, z: 0.0, w: c };
        xr::Posef { orientation: math::quat_mul(y.orientation, p), position: y.position }
    }

    #[test]
    fn a_pitched_head_gives_a_level_body_at_the_head() {
        let mut b = Body::default();
        let pose = b.tick(head(0.3, -0.7), &cfg(), 0);
        assert!((yaw_of(pose.orientation) - 0.3).abs() < 1e-3, "yaw kept");
        let up = math::rotate(pose.orientation, [0.0, 1.0, 0.0]);
        assert!((up[1] - 1.0).abs() < 1e-5, "no pitch or roll: up stays up");
        assert_eq!(pose.position.y, 1.6, "the body sits where the head is");
    }

    #[test]
    fn the_yaw_reseats_after_threshold_and_delay_and_stops_within_stop() {
        let mut b = Body::default();
        let c = cfg();
        b.tick(head(0.0, 0.0), &c, 0);
        // 30° off: within the 40° threshold — the body holds
        for i in 1..=20 {
            b.tick(head(30f32.to_radians(), 0.0), &c, i * S / 10);
        }
        assert!(b.yaw.abs() < 1e-4, "inside the threshold nothing moves");
        // 60° off: past the threshold; before the delay it still holds
        let t0 = 3 * S;
        b.tick(head(60f32.to_radians(), 0.0), &c, t0);
        b.tick(head(60f32.to_radians(), 0.0), &c, t0 + 300_000_000);
        assert!(b.yaw.abs() < 1e-4, "within the delay nothing moves");
        // after the delay it turns toward the head and settles within `stop`
        let mut t = t0 + 600_000_000;
        for _ in 0..200 {
            b.tick(head(60f32.to_radians(), 0.0), &c, t);
            t += S / 60;
        }
        let off = yaw_delta(b.yaw, 60f32.to_radians()).to_degrees().abs();
        assert!(off <= c.stop_deg + 0.5, "settled within stop: {off}°");
        assert!(!b.follower.moving);
        assert!(b.reseats > 0);
    }

    #[test]
    fn yaw_delta_wraps() {
        assert!((yaw_delta(3.0, -3.0) - (-3.0 - 3.0 + 2.0 * core::f32::consts::PI)).abs() < 1e-5);
        assert!((yaw_delta(0.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
