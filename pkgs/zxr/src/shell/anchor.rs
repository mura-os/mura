//! The shell's world anchor (spec §5; shell-plane §2.6; `zxr-layer-anchoring-v1` rev 3 `world`):
//! **the head's position and heading, captured when the scene first appears and on recenter,
//! never per tick.** World-framed layer surfaces — the greeter, the lock, the OSK's fallback, a
//! bar — hang off it, so they float where they were summoned, hold still while the wearer looks
//! or moves around, and come back in front on the one recenter gesture.
//!
//! This is the convention every shipping platform follows, and the only shape with comparables:
//! wayvr's `Positioning::Floating` — "Stays in place, recenters relative to HMD"
//! (`wayvr/wlx-common/src/windowing.rs:8-16`; the anchor captured from `snap_upright(hmd)` when the
//! set is shown, `wayvr/src/windowing/manager.rs:1131-1139`); visionOS — windows world-fixed,
//! head anchoring "the thing not to do", recenter = Crown long-press (research/64 §7); HoloLens —
//! world-fixed with a per-window *opt-in* Follow me (MRTK3 `Solvers/Follow.cs`, a behaviour on a
//! world object, not a frame); SteamVR's dashboard placed where summoned [external]. OpenXR itself
//! has `VIEW`, `LOCAL`, `LOCAL_FLOOR`, `STAGE` and no body space (`openxr-docs …/spaces.adoc`).
//! Mura's earlier "body frame" (head position per tick + a lazily re-seated yaw, rev 3.14) had no
//! comparable — research/78 §9 F23 records its withdrawal. Following remains what Q6 ruled it:
//! opt-in per surface (`wm.follow.*`, `policy/follow.rs`), never a frame.
//!
//! Budget: one pose write at the seed and one per recenter; nothing per tick.

use openxr as xr;

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

/// The anchor's own state: seeded or not, its heading, how often recenter moved it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Anchor {
    pub seated: bool,
    pub yaw: f32,
    pub reseats: u64,
}

impl Anchor {
    /// The pose the anchor takes from a head pose: the head's position, its heading, upright.
    pub fn pose_from(head: xr::Posef) -> (xr::Posef, f32) {
        let yaw = yaw_of(head.orientation);
        (math::pose_yaw([head.position.x, head.position.y, head.position.z], yaw), yaw)
    }
}

/// Per tick, after the head is written: seed the anchor from the first valid head pose (the
/// scene appears in front of the wearer — wayvr's capture on show). Nothing afterwards.
pub fn tick(st: &mut Zxr, head: xr::Posef) {
    if st.shell.anchor.seated {
        return;
    }
    let (pose, yaw) = Anchor::pose_from(head);
    st.shell.anchor.seated = true;
    st.shell.anchor.yaw = yaw;
    let id = st.scene.anchor;
    st.scene.set_frame_pose(id, pose, true);
    tracing::info!(yaw_deg = yaw.to_degrees(), "anchor: seeded from the first head pose (the scene appears in front of the wearer)");
}

/// Recenter (`policy::recenter`; the reserved long press, `zxr ctl wm recenter`): the anchor is
/// re-seated at the head — research/64 §7's "a rigid re-seat of everything head-relative".
pub fn reseat(st: &mut Zxr) {
    let Some(head) = st.input.head else { return };
    let (pose, yaw) = Anchor::pose_from(head);
    st.shell.anchor.seated = true;
    st.shell.anchor.yaw = yaw;
    st.shell.anchor.reseats += 1;
    st.journal.anchor_reseats += 1;
    let id = st.scene.anchor;
    st.scene.set_frame_pose(id, pose, true);
    tracing::info!(yaw_deg = yaw.to_degrees(), reseats = st.shell.anchor.reseats, "anchor: re-seated at the head (recenter)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(pos: [f32; 3], yaw_deg: f32, pitch_deg: f32) -> xr::Posef {
        let yaw = math::pose_yaw(pos, yaw_deg.to_radians()).orientation;
        let ori = math::quat_mul(yaw, crate::shell::quat_pitch(pitch_deg.to_radians()));
        xr::Posef { orientation: ori, position: xr::Vector3f { x: pos[0], y: pos[1], z: pos[2] } }
    }

    #[test]
    fn the_anchor_takes_the_heads_position_and_heading_upright() {
        let (pose, yaw) = Anchor::pose_from(head([0.3, 1.6, -0.2], 30.0, -25.0));
        assert!((pose.position.x - 0.3).abs() < 1e-6 && (pose.position.y - 1.6).abs() < 1e-6 && (pose.position.z + 0.2).abs() < 1e-6);
        assert!((yaw.to_degrees() - 30.0).abs() < 0.01, "{}", yaw.to_degrees());
        // pitch discarded: the anchor's forward is level
        let f = math::rotate(pose.orientation, [0.0, 0.0, -1.0]);
        assert!(f[1].abs() < 1e-5, "level forward, got y={}", f[1]);
    }

    #[test]
    fn yaw_delta_wraps() {
        assert!((yaw_delta(170f32.to_radians(), -170f32.to_radians()).to_degrees() - 20.0).abs() < 0.01);
        assert!((yaw_delta(-170f32.to_radians(), 170f32.to_radians()).to_degrees() + 20.0).abs() < 0.01);
    }
}
