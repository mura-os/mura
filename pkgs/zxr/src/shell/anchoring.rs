//! `zxr_layer_anchoring_v1` served (`protocols/zxr-layer-anchoring-v1.xml`; spec §4 rev 3.12;
//! research/77 §1, §3). The manager advertises the frames the compositor can anchor to and
//! extends a layer surface with a frame, a world pose, an angular size and an exclusive angle —
//! all double-buffered and applied with the layer surface's `wl_surface.commit`
//! (`apply_pending`, called from the layer commit path before arranging). A client that never
//! creates the extension is presented in the head frame with the compositor's size (`:118-120`).
//!
//! The wearer's placement row (`place.rs`) wins over what this protocol requests (owner ruling,
//! shell-plane §2.6); the request is the placement when no row exists.
//!
//! Privileged: advertised to unrestricted connections only (`filter::unrestricted`), like the
//! layer shell it extends.

use std::sync::Mutex;

use openxr as xr;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum};
use smithay::wayland::compositor::with_states;

use crate::state::Zxr;

pub use generated::{zxr_anchored_layer_v1, zxr_layer_anchoring_v1};
use zxr_anchored_layer_v1::ZxrAnchoredLayerV1;
use zxr_layer_anchoring_v1::ZxrLayerAnchoringV1;

#[allow(non_upper_case_globals, non_camel_case_types, unused_imports, dead_code, clippy::all)]
mod generated {
    use smithay::reexports::wayland_server;

    pub mod __interfaces {
        use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::server::__interfaces::*;
        use wayland_backend;
        wayland_scanner::generate_interfaces!("../../protocols/zxr-layer-anchoring-v1.xml");
    }
    use self::__interfaces::*;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::server::*;

    wayland_scanner::generate_server_code!("../../protocols/zxr-layer-anchoring-v1.xml");
}

/// The protocol's frame enum, owned here so the rest of the shell never names generated types.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Frame {
    #[default]
    Head = 0,
    HandLeft = 1,
    HandRight = 2,
    World = 3,
    Docked = 4,
}

impl Frame {
    pub fn parse(s: &str) -> Option<Frame> {
        Some(match s {
            "head" => Frame::Head,
            "hand_left" => Frame::HandLeft,
            "hand_right" => Frame::HandRight,
            "world" => Frame::World,
            "docked" => Frame::Docked,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Frame::Head => "head",
            Frame::HandLeft => "hand_left",
            Frame::HandRight => "hand_right",
            Frame::World => "world",
            Frame::Docked => "docked",
        }
    }

    fn from_wire(f: zxr_layer_anchoring_v1::Frame) -> Frame {
        use zxr_layer_anchoring_v1::Frame as W;
        match f {
            W::Head => Frame::Head,
            W::HandLeft => Frame::HandLeft,
            W::HandRight => Frame::HandRight,
            W::World => Frame::World,
            W::Docked => Frame::Docked,
        }
    }
}

/// The double-buffered request state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Request {
    pub frame: Option<Frame>,
    pub pose: Option<xr::Posef>,
    pub angular_size_deg: f32,
    pub exclusive_angle_deg: f32,
}

#[derive(Default)]
struct AnchoringState {
    pending: Request,
    current: Request,
    object: Option<ZxrAnchoredLayerV1>,
    last_extent: Option<(f32, f32)>,
}

type AnchoringData = Mutex<AnchoringState>;

/// The `zxr_anchored_layer_v1` object's user data: the layer surface it extends.
pub struct AnchoredData {
    /// `None`: created for a layer surface zxr no longer knows — inert from birth
    pub surface: Option<WlSurface>,
}

fn with_state<T>(surface: &WlSurface, f: impl FnOnce(&mut AnchoringState) -> T) -> Option<T> {
    if !surface.is_alive() {
        return None;
    }
    with_states(surface, |states| {
        states.data_map.insert_if_missing_threadsafe(AnchoringData::default);
        let mut s = states.data_map.get::<AnchoringData>().unwrap().lock().unwrap();
        Some(f(&mut s))
    })
}

/// Commit: the pending request becomes current (atomically with the layer-shell state, which
/// smithay commits in the same `wl_surface.commit`).
pub fn apply_pending(surface: &WlSurface) -> bool {
    with_state(surface, |s| {
        let changed = s.pending != s.current;
        s.current = s.pending;
        changed
    })
    .unwrap_or(false)
}

/// The frame the client asked for, if any (current state).
pub fn requested_frame(surface: &WlSurface) -> Option<Frame> {
    with_state(surface, |s| s.current.frame).flatten()
}

/// The world pose the client set (current state; only meaningful in the world frame).
pub fn requested_pose(surface: &WlSurface) -> Option<xr::Posef> {
    with_state(surface, |s| s.current.pose).flatten()
}

/// The horizontal angular size the client asked for (0 = compositor's choice).
pub fn requested_angular_size(surface: &WlSurface) -> f32 {
    with_state(surface, |s| s.current.angular_size_deg).unwrap_or(0.0)
}

/// The exclusive angle the client set (0 = none; the layer-shell zone applies instead).
pub fn exclusive_angle(surface: &WlSurface) -> f32 {
    with_state(surface, |s| s.current.exclusive_angle_deg).unwrap_or(0.0)
}

/// `frame_extent` when the extents changed since the last one sent (or never sent).
pub fn send_extent(surface: &WlSurface, horizontal: f32, vertical: f32) {
    let _ = with_state(surface, |s| {
        let Some(obj) = &s.object else { return };
        let ext = ((horizontal * 100.0).round() / 100.0, (vertical * 100.0).round() / 100.0);
        if s.last_extent != Some(ext) {
            obj.frame_extent(ext.0 as f64, ext.1 as f64);
            s.last_extent = Some(ext);
        }
    });
}

/// `frames` to every bound manager (the set changed: a frame came or went — the hand/docked
/// frames of M1; nothing calls it while only head and world exist).
#[allow(dead_code)]
pub fn broadcast_frames(st: &Zxr) {
    for m in &st.shell_anchoring_managers {
        if m.is_alive() {
            m.frames(st.shell.frames_available);
        }
    }
}

/// 16 native-endian binary32 values, column-major → a rigid pose, or `None` when the matrix is
/// not a rigid transform (orthonormal rotation, unit scale) within tolerance.
pub fn pose_from_matrix(bytes: &[u8]) -> Option<xr::Posef> {
    if bytes.len() != 64 {
        return None;
    }
    let mut m = [0f32; 16];
    for (i, c) in bytes.chunks_exact(4).enumerate() {
        m[i] = f32::from_ne_bytes([c[0], c[1], c[2], c[3]]);
    }
    if m.iter().any(|v| !v.is_finite()) {
        return None;
    }
    // columns 0..2 are the basis, column 3 the translation; bottom row (0,0,0,1)
    if (m[3].abs() + m[7].abs() + m[11].abs() + (m[15] - 1.0).abs()) > 1e-3 {
        return None;
    }
    let c = |i: usize| [m[i * 4], m[i * 4 + 1], m[i * 4 + 2]];
    let (x, y, z) = (c(0), c(1), c(2));
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |v: [f32; 3]| (dot(v, v) - 1.0).abs() < 1e-2;
    if !(unit(x) && unit(y) && unit(z)) || dot(x, y).abs() > 1e-2 || dot(x, z).abs() > 1e-2 || dot(y, z).abs() > 1e-2 {
        return None;
    }
    // right-handed (det > 0)
    let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
    if det < 0.0 {
        return None;
    }
    // rotation matrix → quaternion (Shepperd)
    let (r00, r01, r02) = (x[0], y[0], z[0]);
    let (r10, r11, r12) = (x[1], y[1], z[1]);
    let (r20, r21, r22) = (x[2], y[2], z[2]);
    let tr = r00 + r11 + r22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        xr::Quaternionf { w: 0.25 * s, x: (r21 - r12) / s, y: (r02 - r20) / s, z: (r10 - r01) / s }
    } else if r00 > r11 && r00 > r22 {
        let s = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
        xr::Quaternionf { w: (r21 - r12) / s, x: 0.25 * s, y: (r01 + r10) / s, z: (r02 + r20) / s }
    } else if r11 > r22 {
        let s = (1.0 + r11 - r00 - r22).sqrt() * 2.0;
        xr::Quaternionf { w: (r02 - r20) / s, x: (r01 + r10) / s, y: 0.25 * s, z: (r12 + r21) / s }
    } else {
        let s = (1.0 + r22 - r00 - r11).sqrt() * 2.0;
        xr::Quaternionf { w: (r10 - r01) / s, x: (r02 + r20) / s, y: (r12 + r21) / s, z: 0.25 * s }
    };
    Some(xr::Posef { orientation: q, position: xr::Vector3f { x: m[12], y: m[13], z: m[14] } })
}

// ---------------------------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------------------------

impl GlobalDispatch<ZxrLayerAnchoringV1, ()> for Zxr {
    fn bind(state: &mut Zxr, _dh: &DisplayHandle, _client: &Client, resource: New<ZxrLayerAnchoringV1>, _data: &(), data_init: &mut DataInit<'_, Zxr>) {
        let m = data_init.init(resource, ());
        m.frames(state.shell.frames_available);
        state.shell_anchoring_managers.retain(|m| m.is_alive());
        state.shell_anchoring_managers.push(m);
    }

    fn can_view(client: Client, _data: &()) -> bool {
        super::filter::unrestricted(&client)
    }
}

impl Dispatch<ZxrLayerAnchoringV1, ()> for Zxr {
    fn request(state: &mut Zxr, _client: &Client, manager: &ZxrLayerAnchoringV1, request: zxr_layer_anchoring_v1::Request, _data: &(), _dh: &DisplayHandle, data_init: &mut DataInit<'_, Zxr>) {
        match request {
            zxr_layer_anchoring_v1::Request::GetAnchoring { id, layer_surface } => {
                let Some(surface) = state.shell.layers.iter().find(|e| e.surface.layer_surface().shell_surface() == &layer_surface).map(|e| e.surface.wl_surface().clone()) else {
                    // the layer surface is gone: an inert object per the protocol's "becomes inert" rule
                    let _ = data_init.init(id, AnchoredData { surface: None });
                    return;
                };
                let already = with_state(&surface, |s| s.object.as_ref().map(|o| o.is_alive()).unwrap_or(false)).unwrap_or(false);
                if already {
                    manager.post_error(zxr_layer_anchoring_v1::Error::AlreadyAnchored, "the layer surface already has an anchoring object");
                    return;
                }
                let obj = data_init.init(id, AnchoredData { surface: Some(surface.clone()) });
                let _ = with_state(&surface, |s| {
                    s.object = Some(obj);
                    s.last_extent = None;
                });
            }
            zxr_layer_anchoring_v1::Request::Destroy => {}
        }
    }

    fn destroyed(state: &mut Zxr, _client: ClientId, resource: &ZxrLayerAnchoringV1, _data: &()) {
        state.shell_anchoring_managers.retain(|m| m != resource);
    }
}

impl Dispatch<ZxrAnchoredLayerV1, AnchoredData> for Zxr {
    fn request(_state: &mut Zxr, _client: &Client, obj: &ZxrAnchoredLayerV1, request: zxr_anchored_layer_v1::Request, data: &AnchoredData, _dh: &DisplayHandle, _data_init: &mut DataInit<'_, Zxr>) {
        use zxr_anchored_layer_v1::{Error, Request as R};
        let Some(surface) = data.surface.as_ref().filter(|s| s.is_alive()) else { return }; // inert
        match request {
            R::SetFrame { frame } => {
                let WEnum::Value(f) = frame else { return };
                let f = Frame::from_wire(f);
                let _ = with_state(surface, |s| {
                    s.pending.frame = Some(f);
                    if f != Frame::World {
                        s.pending.pose = None;
                    }
                });
            }
            R::SetPose { matrix } => {
                let pending_world = with_state(surface, |s| s.pending.frame == Some(Frame::World)).unwrap_or(false);
                if !pending_world {
                    obj.post_error(Error::InvalidPose, "set_pose while the pending frame is not world");
                    return;
                }
                match pose_from_matrix(&matrix) {
                    Some(p) => {
                        let _ = with_state(surface, |s| s.pending.pose = Some(p));
                    }
                    None => obj.post_error(Error::InvalidPose, "matrix is not a rigid transform (16 binary32, column-major)"),
                }
            }
            R::SetAngularSize { degrees } => {
                let d = degrees as f32;
                if !(d > 0.0 && d <= 180.0) {
                    obj.post_error(Error::InvalidAngularSize, format!("angular size {d} not in (0, 180]"));
                    return;
                }
                let _ = with_state(surface, |s| s.pending.angular_size_deg = d);
            }
            R::SetExclusiveAngle { degrees } => {
                let d = degrees as f32;
                if !(0.0..180.0).contains(&d) {
                    obj.post_error(Error::InvalidExclusiveAngle, format!("exclusive angle {d} negative or not smaller than the frame extent"));
                    return;
                }
                let _ = with_state(surface, |s| s.pending.exclusive_angle_deg = d);
            }
            R::Destroy => {}
        }
    }

    fn destroyed(state: &mut Zxr, _client: ClientId, _resource: &ZxrAnchoredLayerV1, data: &AnchoredData) {
        // "The layer surface falls back to the compositor's default presentation for its layer,
        // as if the anchoring object had never existed."
        let Some(surface) = data.surface.as_ref() else { return };
        let live = with_state(surface, |s| {
            *s = AnchoringState::default();
        })
        .is_some();
        if live && state.shell.entry_for_surface(surface).is_some() {
            super::arrange(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(m: [f32; 16]) -> Vec<u8> {
        m.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn identity_and_translation_are_rigid() {
        let mut m = [0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m[12] = 1.0;
        m[13] = 2.0;
        m[14] = -3.0;
        let p = pose_from_matrix(&bytes(m)).expect("rigid");
        assert!((p.orientation.w - 1.0).abs() < 1e-6);
        assert_eq!((p.position.x, p.position.y, p.position.z), (1.0, 2.0, -3.0));
    }

    #[test]
    fn a_yaw_matrix_becomes_the_yaw_quaternion() {
        let a = 90f32.to_radians();
        let (s, c) = a.sin_cos();
        // rotation about +Y by 90°, column-major
        let m = [c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0];
        let p = pose_from_matrix(&bytes(m)).expect("rigid");
        let q = crate::xr::math::pose_yaw([0.0; 3], a).orientation;
        assert!((p.orientation.y - q.y).abs() < 1e-5 && (p.orientation.w - q.w).abs() < 1e-5, "{:?} vs {:?}", p.orientation, q);
    }

    #[test]
    fn scaled_and_short_matrices_are_rejected() {
        let mut m = [0f32; 16];
        m[0] = 2.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        assert!(pose_from_matrix(&bytes(m)).is_none());
        assert!(pose_from_matrix(&[0u8; 12]).is_none());
    }

    #[test]
    fn frame_names_round_trip() {
        for f in [Frame::Head, Frame::HandLeft, Frame::HandRight, Frame::World, Frame::Docked] {
            assert_eq!(Frame::parse(f.name()), Some(f));
        }
    }
}
