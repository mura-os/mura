//! The bounded window-management seam — `zxr_window_management_v1` served
//! ([protocols/zxr-window-management-v1.xml](../../../../protocols/zxr-window-management-v1.xml)
//! rev 1; window-workspace-management §11; research/64 §11). One manager client at a time
//! (river's `river-window-management-v1` shape: a second binder is told `unavailable`); river's
//! manage/render double-buffered sequences; every request maps onto a [`super::Policy`] verb;
//! the compositor's invariants — frames, `limits`, focus *rules*, hit testing — never cross the
//! wire (`limits` is published, `focus` needs an `interaction` serial, poses are clamped).
//!
//! **The disconnect contract** (§13 Q5-disconnect, Hyprland's shape): when the binder dies the
//! floor continues with every placement untouched; engines revert to `free` for spawn; a
//! reconnecting manager receives the full picture (places, windows, their state) before its first
//! `manage_start` and takes over.
//!
//! **Sequences.** `Idle → Manage(serial) → Render(serial) → Idle`. Management requests (assign,
//! propose_dimensions, maximize/fullscreen, focus) are queued and applied at `manage_finish`;
//! rendering requests (set_pose, hide/show, flags, engine, arrange) at `render_finish`. Requests
//! outside their sequence raise `sequence_order`; a sequence left open past [`UNRESPONSIVE_NS`]
//! raises `unresponsive` (river's timeout). A sequence starts when the compositor has something
//! to say (a new window, a changed pose, a client request, an interaction) or the manager asked
//! (`manage_dirty`), at the policy tick.
//!
//! Budget: one global, no thread; per tick a compare of each managed window's pose against the
//! last reported one (≤ N ≈ 50).

use std::collections::{HashMap, HashSet};

use openxr as xr;
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId};
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum};
use smithay::utils::Serial;

use super::{lifecycle, Engine};
use crate::scene::{Flags, MemberId, PlaceId};
use crate::state::Zxr;
use crate::xr::math;

pub use generated::{zxr_managed_place_v1, zxr_managed_window_v1, zxr_window_manager_v1};
use zxr_managed_place_v1::ZxrManagedPlaceV1;
use zxr_managed_window_v1::ZxrManagedWindowV1;
use zxr_window_manager_v1::ZxrWindowManagerV1;

#[allow(non_snake_case, non_upper_case_globals, non_camel_case_types, dead_code, clippy::all)]
mod generated {
    use smithay::reexports::wayland_server;

    pub mod __interfaces {
        use wayland_backend;
        wayland_scanner::generate_interfaces!("../../protocols/zxr-window-management-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("../../protocols/zxr-window-management-v1.xml");
}

/// A manage or render sequence left open longer than this is the manager's fault (river's
/// timeout shape): the compositor posts `unresponsive` and drops it — the floor continues.
pub const UNRESPONSIVE_NS: u64 = 5_000_000_000;

/// The capabilities this compositor honours (§11): focus with a serial, hide/show, engine +
/// arrange. Not yet: emphasis (`wm.focus.{dim,sibling_alpha}` have no value), exclusive (the
/// grant path is native-apps §4's, M2).
fn capabilities() -> zxr_window_manager_v1::Capability {
    zxr_window_manager_v1::Capability::Focus | zxr_window_manager_v1::Capability::Hide | zxr_window_manager_v1::Capability::Engine
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Manage(u32),
    Render(u32),
}

/// A management request queued for `manage_finish`.
#[derive(Clone, Copy, Debug)]
enum ManageOp {
    Assign(MemberId, Option<PlaceId>),
    Propose(MemberId, i32, i32),
    Maximized(MemberId, bool),
    Fullscreen(MemberId, bool),
    Focus(MemberId, u32),
}

/// A rendering request queued for `render_finish`.
#[derive(Clone, Copy, Debug)]
enum RenderOp {
    Pose(MemberId, xr::Posef),
    Hidden(MemberId, bool),
    Flags(MemberId, Flags),
    Engine(PlaceId, Engine),
    Arrange(PlaceId),
}

/// The one connected manager.
struct Binder {
    manager: ZxrWindowManagerV1,
    client: ClientId,
    windows: HashMap<MemberId, ZxrManagedWindowV1>,
    places: HashMap<PlaceId, ZxrManagedPlaceV1>,
    /// the place-local pose last reported in `state`, per window
    reported: HashMap<MemberId, (Option<PlaceId>, xr::Posef)>,
    reported_dims: HashMap<MemberId, (i32, i32)>,
    /// windows the manager left unassigned (`assign` with no place): not presented — the
    /// floor's hidden — and reported with no place until assigned again
    unassigned: HashSet<MemberId>,
    /// windows the manager hid (`hide`); shown again with `show` — or when the manager goes
    hidden: HashSet<MemberId>,
    phase: Phase,
    phase_since_ns: u64,
    serial: u32,
    dirty: bool,
    manage_ops: Vec<ManageOp>,
    render_ops: Vec<RenderOp>,
    /// the serials `interaction` announced, accepted by `focus` (the compositor's rule decides
    /// whether one is recent enough)
    interactions: Vec<(u32, Serial)>,
}

/// The seam's state on `Zxr`.
#[derive(Default)]
pub struct Seam {
    #[allow(dead_code)] // held so the global lives as long as the compositor
    global: Option<GlobalId>,
    binder: Option<Binder>,
    pub binds: u64,
    pub refusals: u64,
    pub disconnects: u64,
    pub manage_sequences: u64,
    pub render_sequences: u64,
    pub requests_applied: u64,
    pub errors: u64,
}

impl Seam {
    pub fn connected(&self) -> bool {
        self.binder.is_some()
    }
}

/// Serve the global (the session's; never in `--greeter` mode — session-auth §5's "no privileged
/// globals") and the seam state that owns it.
pub fn serve(dh: &DisplayHandle) -> Seam {
    let id = dh.create_global::<Zxr, ZxrWindowManagerV1, ()>(1, ());
    tracing::info!("zxr_window_manager_v1 served (window-workspace-management §11; one manager at a time)");
    Seam { global: Some(id), ..Seam::default() }
}

// ---------------------------------------------------------------------------------------------
// Poses on the wire: 16 binary32, column-major, rigid
// ---------------------------------------------------------------------------------------------

pub fn pose_to_bytes(p: xr::Posef) -> Vec<u8> {
    let m = math::pose_to_mat(p);
    let mut out = Vec::with_capacity(64);
    for v in m {
        out.extend_from_slice(&v.to_ne_bytes());
    }
    out
}

/// A rigid transform from 16 floats; `None` when the array is malformed or not rigid.
pub fn pose_from_bytes(b: &[u8]) -> Option<xr::Posef> {
    if b.len() != 64 {
        return None;
    }
    let mut m = [0f32; 16];
    for (i, c) in b.chunks_exact(4).enumerate() {
        m[i] = f32::from_ne_bytes([c[0], c[1], c[2], c[3]]);
    }
    // columns 0..3 are the rotation's axes; rigid = orthonormal, det +1
    let x = [m[0], m[1], m[2]];
    let y = [m[4], m[5], m[6]];
    let z = [m[8], m[9], m[10]];
    let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let eps = 1e-3;
    if (len(x) - 1.0).abs() > eps || (len(y) - 1.0).abs() > eps || (len(z) - 1.0).abs() > eps || dot(x, y).abs() > eps || dot(x, z).abs() > eps || dot(y, z).abs() > eps {
        return None;
    }
    let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
    if (det - 1.0).abs() > eps || !m.iter().all(|v| v.is_finite()) {
        return None;
    }
    // rotation matrix → quaternion (Shepperd's method)
    let (m00, m11, m22) = (x[0], y[1], z[2]);
    let t = m00 + m11 + m22;
    let q = if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        xr::Quaternionf { w: 0.25 * s, x: (y[2] - z[1]) / s, y: (z[0] - x[2]) / s, z: (x[1] - y[0]) / s }
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        xr::Quaternionf { w: (y[2] - z[1]) / s, x: 0.25 * s, y: (x[1] + y[0]) / s, z: (z[0] + x[2]) / s }
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        xr::Quaternionf { w: (z[0] - x[2]) / s, x: (x[1] + y[0]) / s, y: 0.25 * s, z: (y[2] + z[1]) / s }
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        xr::Quaternionf { w: (x[1] - y[0]) / s, x: (z[0] + x[2]) / s, y: (y[2] + z[1]) / s, z: 0.25 * s }
    };
    Some(xr::Posef { orientation: q, position: xr::Vector3f { x: m[12], y: m[13], z: m[14] } })
}

/// The wire `flags` bitfield is `scene::Flags` bit for bit (draggable 1, managed 2, hoverable 4,
/// pinned 8 — xrdesktop's vocabulary on both sides).
fn flags_from_wire(f: u32) -> Flags {
    Flags((f & 0xF) as u8)
}

fn engine_to_wire(e: Engine) -> zxr_managed_place_v1::Engine {
    match e {
        Engine::Free => zxr_managed_place_v1::Engine::Free,
        Engine::Custom => zxr_managed_place_v1::Engine::Custom,
    }
}

// ---------------------------------------------------------------------------------------------
// The picture the manager sees
// ---------------------------------------------------------------------------------------------

/// Send a window's identity events (once, after creation).
fn describe_window(st: &Zxr, b: &Binder, member: MemberId, w: &ZxrManagedWindowV1) {
    w.kind(zxr_managed_window_v1::Kind::Plane);
    if let Some(m) = st.scene.get(member) {
        if let Some(t) = m.m.window.toplevel() {
            let (app_id, title) = smithay::wayland::compositor::with_states(t.wl_surface(), |states| {
                let d = states.data_map.get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>().map(|d| d.lock().unwrap());
                (d.as_ref().and_then(|d| d.app_id.clone()).unwrap_or_default(), d.as_ref().and_then(|d| d.title.clone()).unwrap_or_default())
            });
            w.app_id(app_id);
            w.title(title);
            let parent = t.parent().and_then(|p| st.member_for_root(&p)).and_then(|pm| b.windows.get(&pm));
            w.parent(parent);
        }
    }
}

/// The `dimensions` event when the size changed since last reported (part of a render sequence).
fn send_dimensions(st: &Zxr, b: &mut Binder, member: MemberId) {
    let Some(w) = b.windows.get(&member) else { return };
    let Some(size) = st.logical_size(member) else { return };
    if b.reported_dims.get(&member) != Some(&size) {
        w.dimensions(size.0, size.1);
        b.reported_dims.insert(member, size);
    }
}

/// The `state` event when the place or the place-local pose changed since last reported (before
/// manage_start).
fn send_state(st: &Zxr, b: &mut Binder, member: MemberId, force: bool) {
    let Some(w) = b.windows.get(&member) else { return };
    let Some(m) = st.scene.get(member) else { return };
    let place_id = if b.unassigned.contains(&member) { None } else { Some(m.place) };
    let changed = b.reported.get(&member).map(|(p, r)| *p != place_id || !pose_eq(*r, m.local)).unwrap_or(true);
    if changed || force {
        let place = place_id.and_then(|p| b.places.get(&p));
        w.state(place, pose_to_bytes(m.local), zxr_managed_window_v1::ScaleMode::Angular);
        b.reported.insert(member, (place_id, m.local));
    }
}

fn pose_eq(a: xr::Posef, b: xr::Posef) -> bool {
    let dp = (a.position.x - b.position.x).abs() + (a.position.y - b.position.y).abs() + (a.position.z - b.position.z).abs();
    let dq = (a.orientation.x - b.orientation.x).abs() + (a.orientation.y - b.orientation.y).abs() + (a.orientation.z - b.orientation.z).abs() + (a.orientation.w - b.orientation.w).abs();
    dp < 1e-5 && dq < 1e-5
}

/// A place's object with its engine and membership, `done`-terminated.
fn announce_place(st: &Zxr, b: &mut Binder, dh: &DisplayHandle, client: &Client, place: PlaceId) {
    if b.places.contains_key(&place) {
        return;
    }
    let Ok(obj) = client.create_resource::<ZxrManagedPlaceV1, PlaceId, Zxr>(dh, 1, place) else { return };
    b.manager.place(&obj);
    obj.engine(engine_to_wire(st.policy.engine(place)));
    for (id, m) in st.scene.iter() {
        if m.place == place {
            if let Some(w) = b.windows.get(&id) {
                obj.member_enter(w);
            }
        }
    }
    obj.done();
    b.places.insert(place, obj);
}

/// A window's object with its identity, then its place's `member_enter`.
fn announce_window(st: &Zxr, b: &mut Binder, dh: &DisplayHandle, client: &Client, member: MemberId) {
    if b.windows.contains_key(&member) {
        return;
    }
    let Ok(obj) = client.create_resource::<ZxrManagedWindowV1, MemberId, Zxr>(dh, 1, member) else { return };
    b.manager.window(&obj);
    describe_window(st, b, member, &obj);
    b.windows.insert(member, obj);
    if let Some(place) = st.scene.get(member).map(|m| m.place) {
        if let Some(p) = b.places.get(&place) {
            p.member_enter(&b.windows[&member]);
            p.done();
        }
    }
    b.dirty = true;
}

/// The whole picture at bind: capabilities, limits, every place, every mapped window, then the
/// first manage sequence.
fn full_picture(st: &mut Zxr, dh: &DisplayHandle, client: &Client, now_ns: u64) {
    let Some(mut b) = st.seam.binder.take() else { return };
    b.manager.capabilities(capabilities());
    let l = st.policy.cfg.limits;
    b.manager.limits(l.min_distance_m as f64, l.max_distance_m as f64, l.max_angular_deg as f64);
    // windows first (place membership needs the window objects), then places; the windows in
    // the floor's most-recently-used order so the manager starts with the focus history too
    let mut members: Vec<MemberId> = st.scene.iter().filter(|(_, m)| m.m.mapped() && m.m.window.toplevel().is_some()).map(|(id, _)| id).collect();
    let mru = st.policy.mru();
    members.sort_by_key(|id| mru.iter().position(|m| m == id).unwrap_or(usize::MAX));
    for id in &members {
        if !b.windows.contains_key(id) {
            if let Ok(obj) = client.create_resource::<ZxrManagedWindowV1, MemberId, Zxr>(dh, 1, *id) {
                b.manager.window(&obj);
                describe_window(st, &b, *id, &obj);
                b.windows.insert(*id, obj);
            }
        }
    }
    let places: Vec<PlaceId> = {
        let mut v: Vec<PlaceId> = st.scene.iter().map(|(_, m)| m.place).collect();
        v.push(st.scene.default_place);
        v.sort_by_key(|p| p.0.index());
        v.dedup();
        v
    };
    for p in places {
        announce_place(st, &mut b, dh, client, p);
    }
    for id in &members {
        send_dimensions(st, &mut b, *id);
        send_state(st, &mut b, *id, true);
    }
    b.dirty = true;
    st.seam.binder = Some(b);
    begin_manage(st, now_ns);
}

fn begin_manage(st: &mut Zxr, now_ns: u64) {
    let Some(b) = st.seam.binder.as_mut() else { return };
    if b.phase != Phase::Idle || !b.dirty {
        return;
    }
    // the changed state since the last sequence
    let members: Vec<MemberId> = b.windows.keys().copied().collect();
    let mut b = st.seam.binder.take().unwrap();
    for id in members {
        send_state(st, &mut b, id, false);
    }
    b.serial = b.serial.wrapping_add(1);
    b.phase = Phase::Manage(b.serial);
    b.phase_since_ns = now_ns;
    b.dirty = false;
    b.manager.manage_start(b.serial);
    st.seam.manage_sequences += 1;
    st.seam.binder = Some(b);
}

fn begin_render(st: &mut Zxr, now_ns: u64) {
    let Some(b) = st.seam.binder.as_mut() else { return };
    let members: Vec<MemberId> = b.windows.keys().copied().collect();
    let mut b = st.seam.binder.take().unwrap();
    for id in members {
        send_dimensions(st, &mut b, id);
    }
    b.serial = b.serial.wrapping_add(1);
    b.phase = Phase::Render(b.serial);
    b.phase_since_ns = now_ns;
    b.manager.render_start(b.serial);
    st.seam.render_sequences += 1;
    st.seam.binder = Some(b);
}

// ---------------------------------------------------------------------------------------------
// Hooks from the compositor
// ---------------------------------------------------------------------------------------------

/// A toplevel was mapped: announce it (the floor has already placed it, rev 1).
pub fn window_mapped(st: &mut Zxr, member: MemberId) {
    let Some(b) = st.seam.binder.as_ref() else { return };
    let Some(client) = b.manager.client() else { return };
    let dh = st.dh.clone();
    let mut b = st.seam.binder.take().unwrap();
    announce_window(st, &mut b, &dh, &client, member);
    st.seam.binder = Some(b);
}

/// A toplevel is gone: `closed`, the place's `member_leave`, the object inert.
pub fn window_closed(st: &mut Zxr, member: MemberId) {
    let Some(b) = st.seam.binder.as_mut() else { return };
    if let Some(w) = b.windows.remove(&member) {
        if let Some(place) = st.scene.get(member).map(|m| m.place) {
            if let Some(p) = b.places.get(&place) {
                p.member_leave(&w);
                p.done();
            }
        }
        w.closed();
        b.reported.remove(&member);
        b.reported_dims.remove(&member);
        b.unassigned.remove(&member);
        b.hidden.remove(&member);
        b.dirty = true;
    }
}

/// A commit on a window (spatial-input §6): the `interaction` serial a `focus` may carry.
pub fn interaction(st: &mut Zxr, member: MemberId, serial: Serial) {
    let Some(b) = st.seam.binder.as_mut() else { return };
    if let Some(w) = b.windows.get(&member) {
        let s: u32 = serial.into();
        w.interaction(s);
        b.interactions.push((s, serial));
        if b.interactions.len() > 64 {
            b.interactions.remove(0);
        }
        b.dirty = true;
    }
}

/// What a client asked for, re-emitted to the manager (design rule 5; §11).
#[derive(Clone, Copy, Debug)]
pub enum ClientRequest {
    Move(Serial),
    Resize(Serial, u32),
    Maximize(bool),
    Fullscreen(bool),
    Minimize,
}

pub fn client_request(st: &mut Zxr, member: MemberId, req: ClientRequest) -> bool {
    let Some(b) = st.seam.binder.as_mut() else { return false };
    let Some(w) = b.windows.get(&member) else { return false };
    match req {
        ClientRequest::Move(s) => w.move_requested(s.into()),
        ClientRequest::Resize(s, edges) => w.resize_requested(s.into(), zxr_managed_window_v1::ResizeEdge::from_bits_truncate(edges)),
        ClientRequest::Maximize(true) => w.maximize_requested(),
        ClientRequest::Maximize(false) => w.unmaximize_requested(),
        ClientRequest::Fullscreen(true) => w.fullscreen_requested(),
        ClientRequest::Fullscreen(false) => w.exit_fullscreen_requested(),
        ClientRequest::Minimize => w.minimize_requested(),
    }
    b.dirty = true;
    true
}

/// The per-tick drive: start a pending sequence; drop an unresponsive manager.
pub fn tick(st: &mut Zxr, now_ns: u64) {
    let Some(b) = st.seam.binder.as_mut() else { return };
    match b.phase {
        Phase::Idle => {
            // a pose the wearer or the floor changed, or a place, is state to report
            if !b.dirty {
                let changed = b.windows.keys().any(|id| st.scene.get(*id).map(|m| b.reported.get(id).map(|(p, r)| *p != (if b.unassigned.contains(id) { None } else { Some(m.place) }) || !pose_eq(*r, m.local)).unwrap_or(true)).unwrap_or(false));
                if changed {
                    b.dirty = true;
                }
            }
            if b.dirty {
                begin_manage(st, now_ns);
            }
        }
        Phase::Manage(_) | Phase::Render(_) => {
            if now_ns.saturating_sub(b.phase_since_ns) > UNRESPONSIVE_NS {
                tracing::warn!(?b.phase, "seam: manager unresponsive — dropped; the floor continues");
                b.manager.post_error(zxr_window_manager_v1::Error::Unresponsive, "sequence not finished in time");
                st.seam.errors += 1;
                disconnect(st);
            }
        }
    }
}

/// The binder is gone (death, `destroy`, or dropped): the floor continues, placements untouched,
/// engines back to `free` for spawn (§13 Q5-disconnect). Windows the manager had hidden or left
/// unassigned are presented again: hidden exists only while something can unhide it (the same
/// condition Q1 gives minimize — a hidden window with no manager and no dock is unreachable).
/// A sequence in flight is discarded, not applied (river force-finishes; the atomic contract
/// here is that state applies at `*_finish`, and a dead manager's half-sequence has no finish).
fn disconnect(st: &mut Zxr) {
    if let Some(b) = st.seam.binder.take() {
        for (place, _) in b.places.iter() {
            st.policy.set_engine(*place, Engine::Free);
        }
        let mut shown = 0;
        for m in b.hidden.iter().chain(b.unassigned.iter()) {
            if st.scene.get(*m).map(|x| x.m.hidden).unwrap_or(false) {
                lifecycle::set_hidden(st, *m, false);
                shown += 1;
            }
        }
        st.policy.manager_connected = false;
        st.seam.disconnects += 1;
        tracing::info!(windows = b.windows.len(), shown, discarded = b.manage_ops.len() + b.render_ops.len(), "seam: manager disconnected — placements kept, engines free, floor continues");
    }
}

fn apply_manage(st: &mut Zxr, ops: Vec<ManageOp>, interactions: &[(u32, Serial)]) {
    for op in ops {
        st.seam.requests_applied += 1;
        match op {
            ManageOp::Assign(member, Some(place)) => {
                st.scene.reparent(member, place);
                // presented again if it was the unassignment that hid it (an explicit `hide` stays)
                if st.seam.binder.as_mut().map(|b| b.unassigned.remove(&member)).unwrap_or(false) {
                    lifecycle::set_hidden(st, member, false);
                }
            }
            ManageOp::Assign(member, None) => {
                // unassigned = not presented (the protocol); the floor's shape for that is hidden
                if let Some(b) = st.seam.binder.as_mut() {
                    b.unassigned.insert(member);
                }
                lifecycle::set_hidden(st, member, true);
            }
            ManageOp::Propose(member, w, h) => {
                let cur = st.logical_size(member).unwrap_or((800, 600));
                st.request_size(member, if w > 0 { w } else { cur.0 }, if h > 0 { h } else { cur.1 });
            }
            ManageOp::Maximized(member, on) => lifecycle::maximize(st, member, on),
            ManageOp::Fullscreen(member, on) => lifecycle::fullscreen(st, member, on),
            ManageOp::Focus(member, serial) => {
                // the compositor's rule: a known interaction serial, judged by focus.rs; zero or
                // unknown → urgency (spatial-input §6, wm §11 Q5-focus)
                match interactions.iter().find(|(s, _)| *s == serial && serial != 0).map(|(_, s)| *s) {
                    Some(s) => {
                        crate::input::focus::manager_focus_request(st, member, s);
                    }
                    None => {
                        crate::input::focus::set_urgent(st, member, true);
                        st.focus.activations_urgent += 1;
                    }
                }
            }
        }
    }
}

fn apply_render(st: &mut Zxr, ops: Vec<RenderOp>) {
    for op in ops {
        st.seam.requests_applied += 1;
        match op {
            RenderOp::Pose(member, pose) => {
                // the pose is place-local on the wire; clamp through the limits in world terms
                let Some(m) = st.scene.get(member) else { continue };
                let Some(pw) = st.scene.place_world(m.place) else { continue };
                let world = math::pose_mul(pw, pose);
                super::set_pose(st, member, world);
                st.policy.moved_by_wearer(member);
            }
            RenderOp::Hidden(member, on) => {
                if let Some(b) = st.seam.binder.as_mut() {
                    if on {
                        b.hidden.insert(member);
                    } else {
                        b.hidden.remove(&member);
                    }
                }
                lifecycle::set_hidden(st, member, on)
            }
            RenderOp::Flags(member, flags) => {
                st.scene.set_flags(member, flags);
            }
            RenderOp::Engine(place, e) => {
                st.policy.set_engine(place, e);
                if let Some(b) = st.seam.binder.as_ref() {
                    if let Some(p) = b.places.get(&place) {
                        p.engine(engine_to_wire(e));
                        p.done();
                    }
                }
            }
            RenderOp::Arrange(place) => {
                if st.policy.engine(place) == Engine::Free {
                    super::arrange(st, place);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------------------------

impl GlobalDispatch<ZxrWindowManagerV1, ()> for Zxr {
    /// The WM seam is in the privileged set (spec §10 rev 3.12; shell-plane §2.2): hidden from
    /// security-context clients like the layer shell.
    fn can_view(client: Client, _global_data: &()) -> bool {
        crate::shell::filter::unrestricted(&client)
    }

    fn bind(state: &mut Zxr, dh: &DisplayHandle, client: &Client, resource: New<ZxrWindowManagerV1>, _global_data: &(), data_init: &mut DataInit<'_, Zxr>) {
        let manager = data_init.init(resource, ());
        state.seam.binds += 1;
        if state.seam.binder.is_some() {
            // one manager at a time (river): the second is told so and should destroy the object
            manager.unavailable();
            state.seam.refusals += 1;
            tracing::info!("seam: a second window manager bound — unavailable");
            return;
        }
        state.seam.binder = Some(Binder {
            manager,
            client: client.id(),
            windows: HashMap::new(),
            places: HashMap::new(),
            reported: HashMap::new(),
            reported_dims: HashMap::new(),
            unassigned: HashSet::new(),
            hidden: HashSet::new(),
            phase: Phase::Idle,
            phase_since_ns: 0,
            serial: 0,
            dirty: false,
            manage_ops: Vec::new(),
            render_ops: Vec::new(),
            interactions: Vec::new(),
        });
        state.policy.manager_connected = true;
        let now = crate::state::now_ns();
        tracing::info!("seam: window manager bound — full picture, then manage_start");
        full_picture(state, dh, client, now);
    }
}

impl Dispatch<ZxrWindowManagerV1, ()> for Zxr {
    fn request(state: &mut Zxr, _client: &Client, resource: &ZxrWindowManagerV1, request: zxr_window_manager_v1::Request, _data: &(), _dh: &DisplayHandle, _init: &mut DataInit<'_, Zxr>) {
        use zxr_window_manager_v1::Request as R;
        let is_binder = state.seam.binder.as_ref().map(|b| b.manager == *resource).unwrap_or(false);
        if !is_binder {
            // a refused binder's requests are ignored (its object is inert)
            return;
        }
        let now = crate::state::now_ns();
        match request {
            R::ManageFinish { serial } => {
                let b = state.seam.binder.as_mut().unwrap();
                if b.phase != Phase::Manage(serial) {
                    resource.post_error(zxr_window_manager_v1::Error::SequenceOrder, format!("manage_finish {serial} outside its sequence"));
                    state.seam.errors += 1;
                    disconnect(state);
                    return;
                }
                let ops = std::mem::take(&mut b.manage_ops);
                let interactions = b.interactions.clone();
                b.phase = Phase::Idle;
                apply_manage(state, ops, &interactions);
                begin_render(state, now);
            }
            R::RenderFinish { serial } => {
                let b = state.seam.binder.as_mut().unwrap();
                if b.phase != Phase::Render(serial) {
                    resource.post_error(zxr_window_manager_v1::Error::SequenceOrder, format!("render_finish {serial} outside its sequence"));
                    state.seam.errors += 1;
                    disconnect(state);
                    return;
                }
                let ops = std::mem::take(&mut b.render_ops);
                b.phase = Phase::Idle;
                apply_render(state, ops);
            }
            R::ManageDirty => {
                if let Some(b) = state.seam.binder.as_mut() {
                    b.dirty = true;
                }
            }
            R::Stop => {
                if let Some(b) = state.seam.binder.as_ref() {
                    b.manager.finished();
                }
                disconnect(state);
            }
            R::Destroy => {
                disconnect(state);
            }
        }
    }

    fn destroyed(state: &mut Zxr, _client: ClientId, resource: &ZxrWindowManagerV1, _data: &()) {
        if state.seam.binder.as_ref().map(|b| b.manager == *resource).unwrap_or(false) {
            disconnect(state);
        }
    }
}

/// Whether a request is in the right sequence; posts the error and disconnects if not.
fn in_sequence(state: &mut Zxr, management: bool) -> bool {
    let Some(b) = state.seam.binder.as_ref() else { return false };
    let ok = match (management, b.phase) {
        (true, Phase::Manage(_)) => true,
        (false, Phase::Manage(_) | Phase::Render(_)) => true,
        _ => false,
    };
    if !ok {
        b.manager.post_error(zxr_window_manager_v1::Error::SequenceOrder, if management { "management state outside a manage sequence" } else { "rendering state outside a sequence" });
        state.seam.errors += 1;
        disconnect(state);
    }
    ok
}

impl Dispatch<ZxrManagedWindowV1, MemberId> for Zxr {
    fn request(state: &mut Zxr, _client: &Client, resource: &ZxrManagedWindowV1, request: zxr_managed_window_v1::Request, member: &MemberId, _dh: &DisplayHandle, _init: &mut DataInit<'_, Zxr>) {
        use zxr_managed_window_v1::Request as R;
        let member = *member;
        // an inert object (closed window, or not this binder's) ignores everything but destroy
        let live = state.seam.binder.as_ref().map(|b| b.windows.get(&member) == Some(resource)).unwrap_or(false);
        if !live {
            return;
        }
        match request {
            R::Destroy => {}
            R::Close => lifecycle::close(state, member),
            R::Assign { place } => {
                if in_sequence(state, true) {
                    let pid = place.and_then(|p| state.seam.binder.as_ref().and_then(|b| b.places.iter().find(|(_, o)| **o == p).map(|(id, _)| *id)));
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.manage_ops.push(ManageOp::Assign(member, pid));
                    }
                }
            }
            R::SetPose { pose } => {
                if in_sequence(state, false) {
                    match pose_from_bytes(&pose) {
                        Some(p) => {
                            if let Some(b) = state.seam.binder.as_mut() {
                                b.render_ops.push(RenderOp::Pose(member, p));
                            }
                        }
                        None => {
                            if let Some(b) = state.seam.binder.as_ref() {
                                b.manager.post_error(zxr_window_manager_v1::Error::InvalidPose, "set_pose: not 16 binary32 or not rigid");
                            }
                            state.seam.errors += 1;
                            disconnect(state);
                        }
                    }
                }
            }
            R::ProposeDimensions { width, height } => {
                if width < 0 || height < 0 {
                    if let Some(b) = state.seam.binder.as_ref() {
                        b.manager.post_error(zxr_window_manager_v1::Error::InvalidDimensions, "negative dimensions");
                    }
                    state.seam.errors += 1;
                    disconnect(state);
                } else if in_sequence(state, true) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.manage_ops.push(ManageOp::Propose(member, width, height));
                    }
                }
            }
            R::SetScaleMode { .. } => {} // angular only (wm §3 principle 3); the request is honoured as a no-op
            R::SetFlags { flags } => {
                if in_sequence(state, false) {
                    let f = match flags {
                        WEnum::Value(v) => v.bits(),
                        WEnum::Unknown(v) => v,
                    };
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.render_ops.push(RenderOp::Flags(member, flags_from_wire(f)));
                    }
                }
            }
            R::Hide => {
                if in_sequence(state, false) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.render_ops.push(RenderOp::Hidden(member, true));
                    }
                }
            }
            R::Show => {
                if in_sequence(state, false) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.render_ops.push(RenderOp::Hidden(member, false));
                    }
                }
            }
            R::SetMaximized { maximized } => {
                if in_sequence(state, true) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.manage_ops.push(ManageOp::Maximized(member, maximized != 0));
                    }
                }
            }
            R::SetFullscreen { fullscreen } => {
                if in_sequence(state, true) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.manage_ops.push(ManageOp::Fullscreen(member, fullscreen != 0));
                    }
                }
            }
            R::Focus { serial } => {
                if in_sequence(state, true) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.manage_ops.push(ManageOp::Focus(member, serial));
                    }
                }
            }
            R::GrantExclusive { .. } => {} // capability not advertised: ignored (M2)
        }
    }
}

impl Dispatch<ZxrManagedPlaceV1, PlaceId> for Zxr {
    fn request(state: &mut Zxr, _client: &Client, resource: &ZxrManagedPlaceV1, request: zxr_managed_place_v1::Request, place: &PlaceId, _dh: &DisplayHandle, _init: &mut DataInit<'_, Zxr>) {
        use zxr_managed_place_v1::Request as R;
        let place = *place;
        let live = state.seam.binder.as_ref().map(|b| b.places.get(&place) == Some(resource)).unwrap_or(false);
        if !live {
            return;
        }
        match request {
            R::Destroy => {}
            R::SetEngine { engine } => {
                if in_sequence(state, false) {
                    let e = match engine {
                        WEnum::Value(zxr_managed_place_v1::Engine::Custom) => Some(Engine::Custom),
                        WEnum::Value(zxr_managed_place_v1::Engine::Free) => Some(Engine::Free),
                        // arc / dock / band are external managers' own arrangements (§4): a manager
                        // asking for one arranges itself — custom
                        WEnum::Value(_) => Some(Engine::Custom),
                        WEnum::Unknown(_) => None,
                    };
                    if let (Some(e), Some(b)) = (e, state.seam.binder.as_mut()) {
                        b.render_ops.push(RenderOp::Engine(place, e));
                    }
                }
            }
            R::Arrange => {
                if in_sequence(state, false) {
                    if let Some(b) = state.seam.binder.as_mut() {
                        b.render_ops.push(RenderOp::Arrange(place));
                    }
                }
            }
            R::SetEmphasis { .. } => {} // capability not advertised (no declared value, wm §8)
        }
    }
}

/// The `seam:` line of `zxr ctl list`.
pub fn describe(st: &Zxr) -> String {
    let s = &st.seam;
    let phase = s.binder.as_ref().map(|b| format!("{:?}", b.phase)).unwrap_or_else(|| "-".into());
    let client = s.binder.as_ref().map(|b| format!("{:?}", b.client)).unwrap_or_else(|| "-".into());
    format!(
        "seam: connected={} client={client} phase={phase} binds={} refusals={} disconnects={} manage_sequences={} render_sequences={} applied={} errors={}",
        s.connected(),
        s.binds,
        s.refusals,
        s.disconnects,
        s.manage_sequences,
        s.render_sequences,
        s.requests_applied,
        s.errors
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_round_trip_and_non_rigid_arrays_are_refused() {
        let p = math::pose_yaw([0.3, -0.2, -1.5], 0.7);
        let bytes = pose_to_bytes(p);
        assert_eq!(bytes.len(), 64);
        let q = pose_from_bytes(&bytes).expect("rigid");
        assert!(pose_eq(p, q) || pose_eq(p, xr::Posef { orientation: xr::Quaternionf { x: -q.orientation.x, y: -q.orientation.y, z: -q.orientation.z, w: -q.orientation.w }, position: q.position }), "{q:?}");
        assert!(pose_from_bytes(&bytes[..60]).is_none(), "wrong length");
        // a scaled matrix is not rigid
        let mut scaled = math::pose_to_mat(p);
        scaled[0] *= 2.0;
        let mut b = Vec::new();
        for v in scaled {
            b.extend_from_slice(&v.to_ne_bytes());
        }
        assert!(pose_from_bytes(&b).is_none());
    }

    #[test]
    fn flags_and_engines_map() {
        assert_eq!(flags_from_wire(1 | 8), Flags(1 | 8));
        assert_eq!(engine_to_wire(Engine::Custom), zxr_managed_place_v1::Engine::Custom);
        assert!(capabilities().contains(zxr_window_manager_v1::Capability::Focus));
        assert!(!capabilities().contains(zxr_window_manager_v1::Capability::Exclusive));
    }
}
