//! The hand bridge (spatial-input §10 "Hand aim, pinch and poke: whose job", rev 0.1 — lines
//! 363-394): while Monado has no `XR_EXT_hand_interaction` device, derive from
//! `XR_EXT_hand_tracking` joints the same aim pose, pinch value, poke pose and `ready` gate the
//! runtime is meant to provide, **and** the system-gesture flags `XR_FB_hand_tracking_aim`
//! shapes (`SYSTEM_GESTURE_BIT_FB`, `DOMINANT_HAND_BIT_FB`, `MENU_PRESSED_BIT_FB` —
//! `openxr-docs/specification/registry/xr.xml:8374-8376`), behind the same [`Sample`] the
//! action path fills, with [`Flags::BRIDGED`] set. When Monado grows the device (the upstream
//! item §10 names) `XrCore::sync_samples` stops calling this and the file deletes.
//!
//! **Derivations, each from the comparable that solved the same problem for the same joints:**
//!
//! - **aim pose** — origin at the index knuckle, direction from a shoulder pivot through it:
//!   Monado's own controller emulation over joints, `drivers/ht_ctrl_emu/ht_ctrl_emu.cpp:308-359`
//!   (chest = head − 10 cm − 7 cm, shoulder = chest ± 15.5 cm along the yaw-flattened facing;
//!   ray from `INDEX_PROXIMAL`); MRTK3's polyfill ray is the same shape with a head-space pivot
//!   (`Utilities/PoseSource/PolyfillHandRayPoseSource.cs:16-20, 50-62`, `HandRay.cs:73-75,
//!   89-99`). Monado's is ported because the design's mechanism *is* a Monado device.
//! - **pinch value** — thumb-tip–index-tip distance, 1.0 at the activation distance and linear
//!   to 0.0 at 8 cm, with hysteresis 1.0 cm close / 1.5 cm open:
//!   StereoKit `StereoKitC/hands/input_hand.cpp:395-403, 406-409` (the design's named stand-in;
//!   MRTK3's 0.25/0.75-of-index-length pair, `MRTKHandsAggregatorConfig.cs:20-35` +
//!   `MRTKHandsAggregatorSubsystem.cs:304-315`, is the alternative §10 records). Linear-to-tip
//!   distance is what `pinch_ext/value` requires (`ext_hand_interaction.adoc:328-330`).
//! - **poke pose** — the index tip joint (`ext_hand_interaction.adoc:218-222`: "a fingertip").
//! - **ready** — the hand tracked (its aim joints carry TRACKED bits) and the palm not turned to
//!   the face: MRTK3's `isReadyToPinch = handIsUp && handIsFacingAway`
//!   (`MRTKHandsAggregatorSubsystem.cs:309`), StereoKit's ray only while the palm faces away
//!   (`ui/ui_core.cpp:109`).
//! - **system gesture** — palm toward the head (palm normal · (head − palm) > cos 35°) with the
//!   pinch held ≥ 300 ms → `SYSTEM_GESTURE`; the pinch released after that on the non-dominant
//!   hand → `MENU_PRESSED` for one sample; `DOMINANT` on the dominant hand. The platforms' shape
//!   (research/68 §3.1, §3.5); Monado implements none of it.
//!
//! **Stand-ins** (every number here; spatial-input §15 measures them at M1): 1.0/1.5 cm and 8 cm
//! (StereoKit), the 35° cone and 300 ms hold (no comparable publishes theirs — flagged), the
//! dominant hand = right until the setting exists (flagged), the ht_ctrl_emu body constants.
//!
//! Pure functions over `[Posef; 26]` plus a small per-hand [`State`]; the injector's
//! `source <hand> joints …` reaches the same code through [`bridge_from_joints`].

use openxr as xr;

use super::{Flags, Quality, Sample, Side, SourceKind};
use crate::xr::math;

/// `input.hand.dominant`'s default (preferences.nix); the runtime value is [`BridgeCfg::dominant`].
pub const DOMINANT: Side = Side::Right;
/// StereoKit `input_hand.cpp:400`: pinch activates at 1.0 cm tip distance …
pub const PINCH_CLOSE_M: f32 = 0.010;
/// … and releases at 1.5 cm (the `was_trigger ? 1.5f : 1.0f` hysteresis).
pub const PINCH_OPEN_M: f32 = 0.015;
/// StereoKit `input_hand.cpp:406`: the distance at which the pinch value reaches 0.
pub const PINCH_MAX_M: f32 = 0.08;
/// cos 35°: the palm-toward-head cone (stand-in, no comparable publishes its angle).
pub const PALM_FACING_COS: f32 = 0.819_152;
/// the pinch-and-hold duration that completes the reserved gesture (stand-in;
/// `system.gesture.hold_ms`).
pub const HOLD_NS: u64 = 300_000_000;

/// Monado `ht_ctrl_emu.cpp:310-312`: body constants for the shoulder pivot — the defaults of
/// `input.body.*` (the wearer's, per the owner's reclassification in research/73 Q2).
const SHOULDER_HALF_WIDTH_M: f32 = (39.0 / 2.0 - 4.0) * 0.01;
const HEAD_LENGTH_M: f32 = 0.10;
const NECK_LENGTH_M: f32 = 0.07;

/// Everything the bridge reads from settings (settings.rs `Prefs::bridge_cfg`): the wearer's
/// preferences (dominant hand, body model, gesture hold) and the tracker's calibrations (the
/// metre ladder of the pinch, the palm cone — `hardware.input.*`, immutable).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BridgeCfg {
    pub dominant: Side,
    pub pinch_close_m: f32,
    pub pinch_open_m: f32,
    pub pinch_max_m: f32,
    pub palm_facing_cos: f32,
    pub hold_ns: u64,
    pub shoulder_half_m: f32,
    pub head_len_m: f32,
    pub neck_len_m: f32,
}

impl Default for BridgeCfg {
    fn default() -> Self {
        BridgeCfg { dominant: DOMINANT, pinch_close_m: PINCH_CLOSE_M, pinch_open_m: PINCH_OPEN_M, pinch_max_m: PINCH_MAX_M, palm_facing_cos: PALM_FACING_COS, hold_ns: HOLD_NS, shoulder_half_m: SHOULDER_HALF_WIDTH_M, head_len_m: HEAD_LENGTH_M, neck_len_m: NECK_LENGTH_M }
    }
}

pub const PALM: usize = 0;
pub const WRIST: usize = 1;
pub const THUMB_TIP: usize = 5;
pub const INDEX_PROXIMAL: usize = 7;
pub const INDEX_TIP: usize = 10;

/// The joints the derivations read; a hand is `tracked` only when all of them carry both
/// TRACKED bits (an occluded thumb or index means no pinch value and no `ready`).
pub const REQUIRED_JOINTS: [usize; 5] = [PALM, WRIST, THUMB_TIP, INDEX_PROXIMAL, INDEX_TIP];

pub type Joints = [xr::Posef; xr::HAND_JOINT_COUNT];

/// Per-hand state between ticks: the pinch hysteresis, the hold timer, the active gesture.
#[derive(Clone, Copy, Default, Debug)]
pub struct State {
    pub pinched: bool,
    hold_since_ns: Option<u64>,
    gesture: bool,
}

/// What one tick's derivation yields for one hand.
#[derive(Clone, Copy, Debug)]
pub struct Derived {
    pub aim: Option<xr::Posef>,
    pub poke: Option<xr::Posef>,
    pub pinch: f32,
    pub pinched: bool,
    pub tracked: bool,
    pub ready: bool,
    pub flags: Flags,
}

fn v3(p: xr::Vector3f) -> [f32; 3] {
    [p.x, p.y, p.z]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = len(a);
    if l > 1e-6 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, -1.0]
    }
}

/// A rotation whose −Z is `forward` and whose +Y is as close to `up` as possible (Monado's
/// `math_quat_from_plus_x_z` shape, `ht_ctrl_emu.cpp:349-357`).
pub fn look_rotation(forward: [f32; 3], up: [f32; 3]) -> xr::Quaternionf {
    let z = norm(scale(forward, -1.0));
    let mut x = cross(up, z);
    if len(x) < 1e-4 {
        x = cross([0.0, 0.0, -1.0], z);
    }
    let x = norm(x);
    let y = cross(z, x);
    // column-major rotation matrix → quaternion
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let t = m00 + m11 + m22;
    let q = if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    };
    xr::Quaternionf { x: q[0], y: q[1], z: q[2], w: q[3] }
}

/// The palm's outward normal: the joint's +Y points out of the *back* of the hand
/// (`ext_hand_tracking.adoc:564-569`), so the palm surface faces −Y.
pub fn palm_normal(palm: xr::Posef) -> [f32; 3] {
    math::rotate(palm.orientation, [0.0, -1.0, 0.0])
}

/// Palm turned toward the head: normal · (head − palm) within the cone.
pub fn palm_faces_head(palm: xr::Posef, head_pos: [f32; 3]) -> bool {
    palm_faces_head_within(palm, head_pos, PALM_FACING_COS)
}

/// [`palm_faces_head`] with the cone from the calibration (`hardware.input.palm.cone_deg`).
pub fn palm_faces_head_within(palm: xr::Posef, head_pos: [f32; 3], facing_cos: f32) -> bool {
    let to_head = norm(sub(head_pos, v3(palm.position)));
    dot(palm_normal(palm), to_head) > facing_cos
}

/// The aim ray (`ht_ctrl_emu.cpp:308-359`): origin at the index knuckle, pointing from the
/// same-side shoulder through it; up = world +Y.
pub fn aim_pose(side: Side, joints: &Joints, head: xr::Posef) -> xr::Posef {
    aim_pose_with(&BridgeCfg::default(), side, joints, head)
}

/// [`aim_pose`] with the wearer's body model (`input.body.*`).
pub fn aim_pose_with(cfg: &BridgeCfg, side: Side, joints: &Joints, head: xr::Posef) -> xr::Posef {
    let head_pos = v3(head.position);
    let chest = add(add(head_pos, math::rotate(head.orientation, [0.0, -cfg.head_len_m, 0.0])), [0.0, -cfg.neck_len_m, 0.0]);
    let mut fwd = scale(norm(math::rotate(head.orientation, [0.0, 0.0, -1.0])), 2.0);
    fwd = add(fwd, norm(sub(v3(joints[WRIST].position), chest)));
    fwd[1] = 0.0;
    let fwd = norm(fwd);
    let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
    let shoulder = add(chest, scale(right, if side == Side::Right { cfg.shoulder_half_m } else { -cfg.shoulder_half_m }));
    let origin = v3(joints[INDEX_PROXIMAL].position);
    let dir = norm(sub(origin, shoulder));
    xr::Posef { orientation: look_rotation(dir, [0.0, 1.0, 0.0]), position: joints[INDEX_PROXIMAL].position }
}

/// StereoKit's activation (`input_hand.cpp:395-407`): 1.0 at or under the activation distance,
/// linear to 0.0 at [`PINCH_MAX_M`]; the activation distance is the hysteresis-side one.
pub fn pinch_value(tip_distance_m: f32, was_pinched: bool) -> f32 {
    pinch_value_with(&BridgeCfg::default(), tip_distance_m, was_pinched)
}

/// [`pinch_value`] on the calibration's metre ladder (`hardware.input.hand.pinch.{close_m,open_m,max_m}`).
pub fn pinch_value_with(cfg: &BridgeCfg, tip_distance_m: f32, was_pinched: bool) -> f32 {
    let act = if was_pinched { cfg.pinch_open_m } else { cfg.pinch_close_m };
    (1.0 - (tip_distance_m - act) / (cfg.pinch_max_m - act)).clamp(0.0, 1.0)
}

/// The full derivation for one hand this tick, on the default configuration (tests, the
/// stateless helper); the runtime path is [`derive_with`].
pub fn derive(side: Side, joints: &Joints, tracked: bool, head: xr::Posef, now_ns: u64, st: &mut State) -> Derived {
    derive_with(&BridgeCfg::default(), side, joints, tracked, head, now_ns, st)
}

/// The full derivation for one hand this tick.
pub fn derive_with(cfg: &BridgeCfg, side: Side, joints: &Joints, tracked: bool, head: xr::Posef, now_ns: u64, st: &mut State) -> Derived {
    let mut flags = Flags::BRIDGED;
    if side == cfg.dominant {
        flags.insert(Flags::DOMINANT);
    }
    if !tracked {
        // a lost hand releases everything; a gesture in progress is simply dropped (no menu)
        *st = State::default();
        return Derived { aim: None, poke: None, pinch: 0.0, pinched: false, tracked: false, ready: false, flags };
    }
    let tip_dist = len(sub(v3(joints[THUMB_TIP].position), v3(joints[INDEX_TIP].position)));
    let pinch = pinch_value_with(cfg, tip_dist, st.pinched);
    let pinched = tip_dist <= if st.pinched { cfg.pinch_open_m } else { cfg.pinch_close_m };
    let facing = palm_faces_head_within(joints[PALM], v3(head.position), cfg.palm_facing_cos);

    // the reserved gesture: posture-gated, deliberate (held), affordance only while held
    // (spatial-input §2 "The reserved system input")
    let mut menu = false;
    if facing && pinched {
        let since = *st.hold_since_ns.get_or_insert(now_ns);
        st.gesture = now_ns.saturating_sub(since) >= cfg.hold_ns;
    } else {
        if st.gesture && !pinched && facing && side != cfg.dominant {
            menu = true;
        }
        st.hold_since_ns = None;
        st.gesture = false;
    }
    st.pinched = pinched;
    if st.gesture {
        flags.insert(Flags::SYSTEM_GESTURE);
    }
    if menu {
        flags.insert(Flags::MENU_PRESSED);
    }
    Derived { aim: Some(aim_pose_with(cfg, side, joints, head)), poke: Some(joints[INDEX_TIP]), pinch, pinched, tracked: true, ready: !facing, flags }
}

/// Write a derivation into a sample (the same fields the action path fills).
pub fn fill(mut s: Sample, d: &Derived) -> Sample {
    s.pose = d.aim;
    s.poke_pose = d.poke;
    s.tracked = d.tracked;
    s.ready = d.ready;
    s.quality = if d.tracked { Quality::Nominal } else { Quality::Lost };
    s.values.pinch = d.pinch;
    // the hand-interaction profile's other two values have no joint derivation here: the
    // aim-activate gesture is the pinch while aiming (adoc:345-358), so it mirrors it; grasp
    // stays 0 (StereoKit's ring-curl grip is not ported — lane report)
    s.values.aim_activate = d.pinch;
    s.values.grasp = 0.0;
    s.flags.insert(d.flags);
    s
}

/// Parse the injector's `26×7` floats (pos xyz, quat xyzw per joint — `control.rs:55`).
pub fn joints_from_floats(f: &[f32]) -> Option<Joints> {
    if f.len() != xr::HAND_JOINT_COUNT * 7 {
        return None;
    }
    let mut out = [xr::Posef::IDENTITY; xr::HAND_JOINT_COUNT];
    for (i, j) in out.iter_mut().enumerate() {
        let c = &f[i * 7..i * 7 + 7];
        *j = xr::Posef { position: xr::Vector3f { x: c[0], y: c[1], z: c[2] }, orientation: xr::Quaternionf { x: c[3], y: c[4], z: c[5], w: c[6] } };
    }
    Some(out)
}

/// The injector's entry with state (hysteresis and the hold need memory between commands).
pub fn bridge_from_joints_with(kind: SourceKind, joints: &[f32], head: xr::Posef, now_ns: u64, st: &mut State) -> Sample {
    bridge_from_joints_cfg(&BridgeCfg::default(), kind, joints, head, now_ns, st)
}

/// [`bridge_from_joints_with`] on a configuration (the injector carries the settings').
pub fn bridge_from_joints_cfg(cfg: &BridgeCfg, kind: SourceKind, joints: &[f32], head: xr::Posef, now_ns: u64, st: &mut State) -> Sample {
    let side = match kind {
        SourceKind::Hand(s) => s,
        _ => Side::Right,
    };
    let mut s = Sample::new(SourceKind::Hand(side), now_ns);
    s.flags.insert(Flags::SYNTHETIC);
    match joints_from_floats(joints) {
        Some(j) => fill(s, &derive_with(cfg, side, &j, true, head, now_ns, st)),
        None => {
            let d = derive_with(cfg, side, &[xr::Posef::IDENTITY; xr::HAND_JOINT_COUNT], false, head, now_ns, st);
            fill(s, &d)
        }
    }
}

/// The stateless form the lane brief names: a fresh [`State`] each call, so the pinch is judged
/// at the close threshold and the hold never completes. Prefer [`bridge_from_joints_with`].
pub fn bridge_from_joints(kind: SourceKind, joints: &[f32], head: xr::Posef, now_ns: u64) -> Sample {
    let mut st = State::default();
    bridge_from_joints_with(kind, joints, head, now_ns, &mut st)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32, y: f32, z: f32) -> xr::Vector3f {
        xr::Vector3f { x, y, z }
    }

    /// A rotation about +X by `deg`: maps the palm's −Y (its surface normal) to
    /// (0, −cos, −sin) — +90° faces −Z (away from a head looking down −Z), −90° faces +Z.
    fn about_x(deg: f32) -> xr::Quaternionf {
        let (s, c) = (deg.to_radians() * 0.5).sin_cos();
        xr::Quaternionf { x: s, y: 0.0, z: 0.0, w: c }
    }

    /// A synthetic right hand 45 cm in front of, 10 cm below and 20 cm right of a head at the
    /// origin looking down −Z: thumb tip and index tip `tip_dist` apart, palm facing the head
    /// or away.
    fn hand(tip_dist: f32, facing_head: bool) -> Joints {
        let p = [0.2f32, -0.1, -0.45];
        let palm = xr::Posef { orientation: about_x(if facing_head { -90.0 } else { 90.0 }), position: pos(p[0], p[1], p[2]) };
        let mut j = [palm; xr::HAND_JOINT_COUNT];
        j[WRIST].position = pos(p[0], p[1], p[2] + 0.06);
        j[INDEX_PROXIMAL].position = pos(p[0], p[1], p[2] - 0.05);
        j[THUMB_TIP].position = pos(p[0] - 0.03, p[1], p[2] - 0.06);
        j[INDEX_TIP].position = pos(p[0] - 0.03 + tip_dist, p[1], p[2] - 0.06);
        j
    }

    const HEAD: xr::Posef = xr::Posef::IDENTITY;

    #[test]
    fn open_hand_is_ready_and_unpinched() {
        let mut st = State::default();
        let d = derive(Side::Right, &hand(0.08, false), true, HEAD, 0, &mut st);
        assert_eq!(d.pinch, 0.0);
        assert!(!d.pinched);
        assert!(d.ready && d.tracked);
        assert!(d.flags.contains(Flags::BRIDGED) && d.flags.contains(Flags::DOMINANT));
        assert!(!d.flags.contains(Flags::SYSTEM_GESTURE));
        // poke = index tip; aim origin = knuckle
        assert_eq!(d.poke.unwrap().position.x, hand(0.08, false)[INDEX_TIP].position.x);
        assert_eq!(d.aim.unwrap().position.z, hand(0.08, false)[INDEX_PROXIMAL].position.z);
    }

    #[test]
    fn aim_ray_points_from_the_shoulder_through_the_knuckle() {
        let j = hand(0.08, false);
        let a = aim_pose(Side::Right, &j, HEAD);
        let fwd = math::rotate(a.orientation, [0.0, 0.0, -1.0]);
        // the ray goes away from the head (−Z) and, for a hand held right of centre, slightly
        // rightward and upward from a shoulder below and inside the knuckle
        assert!(fwd[2] < -0.9, "{fwd:?}");
        assert!(fwd[0] > 0.0 && fwd[1] > 0.0, "{fwd:?}");
        // the left shoulder for the left hand: the mirrored x component
        let mut jl = j;
        for p in jl.iter_mut() {
            p.position.x = -p.position.x;
        }
        let l = math::rotate(aim_pose(Side::Left, &jl, HEAD).orientation, [0.0, 0.0, -1.0]);
        assert!((l[0] + fwd[0]).abs() < 1e-4 && (l[1] - fwd[1]).abs() < 1e-4 && (l[2] - fwd[2]).abs() < 1e-4, "{l:?} vs {fwd:?}");
    }

    #[test]
    fn pinch_has_hysteresis() {
        let mut st = State::default();
        // 1.2 cm: not yet closed (needs ≤ 1.0 cm)
        let d = derive(Side::Right, &hand(0.012, false), true, HEAD, 0, &mut st);
        assert!(!d.pinched);
        // 0.5 cm: pinched, value saturated
        let d = derive(Side::Right, &hand(0.005, false), true, HEAD, 1, &mut st);
        assert!(d.pinched && d.pinch >= 0.75, "{d:?}");
        // back to 1.2 cm: still pinched (release needs > 1.5 cm)
        let d = derive(Side::Right, &hand(0.012, false), true, HEAD, 2, &mut st);
        assert!(d.pinched && d.pinch >= 0.75, "{d:?}");
        // 2 cm: released; the value is linear toward 0 at 8 cm
        let d = derive(Side::Right, &hand(0.02, false), true, HEAD, 3, &mut st);
        assert!(!d.pinched && d.pinch < 1.0 && d.pinch > 0.0, "{d:?}");
        assert_eq!(pinch_value(PINCH_MAX_M, false), 0.0);
        assert_eq!(pinch_value(0.0, false), 1.0);
    }

    #[test]
    fn system_gesture_needs_posture_and_hold_and_clears_when_the_palm_turns() {
        let mut st = State::default();
        // palm toward the head, pinched: not yet (hold)
        let d = derive(Side::Right, &hand(0.005, true), true, HEAD, 0, &mut st);
        assert!(!d.flags.contains(Flags::SYSTEM_GESTURE));
        assert!(!d.ready, "a palm turned to the face is not in the ready pose");
        let d = derive(Side::Right, &hand(0.005, true), true, HEAD, HOLD_NS - 1, &mut st);
        assert!(!d.flags.contains(Flags::SYSTEM_GESTURE));
        let d = derive(Side::Right, &hand(0.005, true), true, HEAD, HOLD_NS, &mut st);
        assert!(d.flags.contains(Flags::SYSTEM_GESTURE));
        // still held: stays on
        let d = derive(Side::Right, &hand(0.005, true), true, HEAD, HOLD_NS * 2, &mut st);
        assert!(d.flags.contains(Flags::SYSTEM_GESTURE));
        // palm turns away with the pinch still held: clears, no menu
        let d = derive(Side::Right, &hand(0.005, false), true, HEAD, HOLD_NS * 3, &mut st);
        assert!(!d.flags.contains(Flags::SYSTEM_GESTURE) && !d.flags.contains(Flags::MENU_PRESSED));
        // pinched again away from the face for a long time: never a gesture without the posture
        let d = derive(Side::Right, &hand(0.005, false), true, HEAD, HOLD_NS * 10, &mut st);
        assert!(!d.flags.contains(Flags::SYSTEM_GESTURE));
    }

    #[test]
    fn menu_pressed_fires_once_on_the_non_dominant_hand_only() {
        for (side, expect) in [(Side::Left, true), (Side::Right, false)] {
            let mut st = State::default();
            derive(side, &hand(0.005, true), true, HEAD, 0, &mut st);
            let d = derive(side, &hand(0.005, true), true, HEAD, HOLD_NS, &mut st);
            assert!(d.flags.contains(Flags::SYSTEM_GESTURE));
            // release the pinch with the palm still facing: completion
            let d = derive(side, &hand(0.03, true), true, HEAD, HOLD_NS + 1, &mut st);
            assert_eq!(d.flags.contains(Flags::MENU_PRESSED), expect, "{side:?}");
            assert!(!d.flags.contains(Flags::SYSTEM_GESTURE));
            // one sample only
            let d = derive(side, &hand(0.03, true), true, HEAD, HOLD_NS + 2, &mut st);
            assert!(!d.flags.contains(Flags::MENU_PRESSED));
        }
    }

    #[test]
    fn lost_tracking_drops_everything() {
        let mut st = State::default();
        derive(Side::Left, &hand(0.005, true), true, HEAD, 0, &mut st);
        derive(Side::Left, &hand(0.005, true), true, HEAD, HOLD_NS, &mut st);
        let d = derive(Side::Left, &hand(0.005, true), false, HEAD, HOLD_NS + 1, &mut st);
        assert!(!d.tracked && !d.ready && d.aim.is_none() && d.pinch == 0.0);
        assert_eq!(d.flags, Flags::BRIDGED);
        assert!(!st.pinched);
    }

    #[test]
    fn injector_floats_round_trip_and_fill_a_sample() {
        let j = hand(0.005, false);
        let mut f = Vec::with_capacity(26 * 7);
        for p in j.iter() {
            f.extend_from_slice(&[p.position.x, p.position.y, p.position.z, p.orientation.x, p.orientation.y, p.orientation.z, p.orientation.w]);
        }
        let s = bridge_from_joints(SourceKind::Hand(Side::Left), &f, HEAD, 7);
        assert_eq!(s.kind, SourceKind::Hand(Side::Left));
        assert!(s.flags.contains(Flags::BRIDGED) && s.flags.contains(Flags::SYNTHETIC) && !s.flags.contains(Flags::DOMINANT));
        assert!(s.tracked && s.ready && s.pose.is_some() && s.poke_pose.is_some());
        assert!(s.values.pinch >= 0.75);
        assert_eq!(s.quality, Quality::Nominal);
        // a malformed vector is a lost hand, not a panic
        let s = bridge_from_joints(SourceKind::Hand(Side::Left), &f[..10], HEAD, 8);
        assert!(!s.tracked && s.pose.is_none());
    }

    #[test]
    fn look_rotation_basis() {
        let q = look_rotation([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
        let f = math::rotate(q, [0.0, 0.0, -1.0]);
        assert!((f[2] + 1.0).abs() < 1e-5);
        let q = look_rotation([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let f = math::rotate(q, [0.0, 0.0, -1.0]);
        let u = math::rotate(q, [0.0, 1.0, 0.0]);
        assert!((f[0] - 1.0).abs() < 1e-5, "{f:?}");
        assert!((u[1] - 1.0).abs() < 1e-5, "{u:?}");
    }
}
