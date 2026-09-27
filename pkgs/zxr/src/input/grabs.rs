//! The `Grabs` slot — the window grab (window-workspace-management §4a, normative from
//! research/76; spatial-input §1a "WM policy: move/resize/grab-all").
//!
//! **Two ways in, one grab.** (1) A client's `xdg_toplevel.move` / `resize(edges)` with a valid
//! serial ([`crate::state::Zxr`]'s `XdgShellHandler` posts a [`GrabRequest`] on `Input`; this
//! stage takes it on the next sample of a kind whose commit is held) — kwin-vr on KWin's move
//! (`VrWindowManipulation.qml:60-84`), wxrd/wxrc/river/zen on the request. (2) The compositor's
//! own affordances: **the bar** under the plane's bottom edge, `wm.grab.bar_deg` high, hit by the
//! commit-class gesture of any kind (the platforms' window bar [external]; zen's nameplate,
//! `bounded-nameplate.c:140-142`) — its outer ends resize; and **body grabs** by a grab-class input
//! that is not the client's commit: a controller's grasp (xrdesktop `xrd-shell.c:848-859`, wayvr
//! `input.rs:611-627`), a mouse with the desktop modifier (`Super` + button, GNOME
//! `mouse-button-modifier`, KWin `CommandAll1`).
//!
//! **Move math** — the field's one shape: at grab the plane's world pose is stored in the grabbing
//! ray's frame (`offset = ray⁻¹ ∘ plane`, wayvr `input.rs:870-878`; kwin-vr `getRelativePose`,
//! `Xray.qml:56-62`; g3k's tip transform with the grab point as pivot, `g3k-controller.c:274-316`;
//! MRTK3's attach point) and re-applied every tick as the ray moves. A mouse has no ray: it moves
//! the plane by its delta in plane-local metres, the desktops' shape. **Depth**: the secondary
//! axis (stick Y, wheel) pushes/pulls along the ray multiplicatively, `d ← d·(1 + rate·axis·Δt)`
//! (xrdesktop `_perform_push_pull`, `xrd-shell.c:728-743`; wayvr `:1017-1022` scales by
//! distance too), clamped to the compositor's `limits` (`hardware.input.comfort.*`). **Facing**:
//! with `wm.move.billboard` the plane looks at the head, upright (wayvr `realign`, StereoKit
//! `ui_move_face_user`); without it the ray's rotation delta applies. **Resize** is logical
//! pixels through `xdg_toplevel` configure from the ray's plane-local motion on the grabbed edges
//! (wxrc `input.c:253-265`), clamped by `max_angular_deg` at the current distance — there is no
//! scale verb (§3 principle 3). **Release keeps the pose.**
//!
//! **While grabbed** the grabbing kind's samples end here (`Flow::Consumed`): the client receives
//! nothing from it (wayvr `pause_movement`, KWin's move seat-op); the plane's follow is paused
//! (`Input::grabbed`). The grab starts on the commit's press and ends on its release or the
//! source's loss (the tier's release, research/70 — a `Select` release with `tracked = false`).
//!
//! Budget: one extended plane cast per commit press and per tick for the bar hover (≤ N planes),
//! one `set_local` per grabbed-kind sample; no allocation in steady state. The bar is drawn as one
//! quad from one fixed swapchain, only for the hovered or grabbed plane (the cursor's shape,
//! research/70 §9).

use openxr as xr;
use smithay::utils::Serial;

use super::{Button, Flow, Sample, SourceKind, Stage};
use crate::scene::{self, MemberId, Shape};
use crate::state::Zxr;
use crate::xr::math;

/// `wm.grab.*`, `wm.move.billboard`, `hardware.input.comfort.*` (settings.rs `Prefs::grab_cfg`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrabCfg {
    /// the bar's height in degrees of visual angle at the plane's distance
    pub bar_deg: f32,
    /// push/pull rate per second on the secondary axis
    pub depth_rate: f32,
    pub min_distance_m: f32,
    pub max_distance_m: f32,
    /// the largest angle a plane's width may subtend (the resize ceiling)
    pub max_angular_deg: f32,
    pub billboard: bool,
    /// the hand's pinch ladder (the commit); the controller's grasp uses the same
    pub pinch_close: f32,
    pub pinch_open: f32,
}

impl Default for GrabCfg {
    fn default() -> Self {
        GrabCfg { bar_deg: 2.0, depth_rate: 3.0, min_distance_m: 0.4, max_distance_m: 5.0, max_angular_deg: 90.0, billboard: true, pinch_close: 0.75, pinch_open: 0.5 }
    }
}

/// Which edges a resize moves (xdg-shell's `resize_edge` bits, as smithay names them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Edges {
    pub fn any(self) -> bool {
        self.top || self.bottom || self.left || self.right
    }
}

/// What a grab does to its plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Move,
    /// resize by these edges from `start_px`, the plane's logical size at grab
    Resize { edges: Edges, start_px: (i32, i32) },
}

/// A client's `xdg_toplevel.move`/`resize` request, posted by the shell handler for this stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrabRequest {
    pub member: MemberId,
    pub op: Op,
    pub serial: Serial,
    /// CLOCK_MONOTONIC ns when it was posted — a request nobody picks up expires
    pub at_ns: u64,
}

/// The bar of the plane under a ray, for the frame procedure: world pose of the bar's centre,
/// its size in metres, and whether a grab holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarLayer {
    pub member: MemberId,
    pub pose: xr::Posef,
    pub size: [f32; 2],
    pub grabbed: bool,
}

/// Where on the bar a hit landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarZone {
    /// the middle: move
    Middle,
    /// the outer ends: resize by the nearer bottom corner
    LeftEnd,
    RightEnd,
}

/// The fraction of the bar's width at each end that resizes rather than moves.
pub const BAR_END_FRACTION: f32 = 0.15;

/// The bar panel's fixed pixel size (stretched over the plane's width; the strip is featureless
/// along X so the stretch costs nothing).
pub const BAR_PX: [u32; 2] = [128, 16];

/// The bar's pixels: a translucent white strip with soft top and bottom edges and brighter end
/// caps marking the resize zones — ARGB8888 like the cursor ring (`cursor::ring_pixels`). The
/// decoration chrome renderer owns the look; this is the plainest thing that reads as a handle.
pub fn bar_pixels(w: u32, h: u32) -> Vec<u8> {
    let mut px = vec![0u8; (w * h * 4) as usize];
    let end = (w as f32 * BAR_END_FRACTION) as u32;
    for y in 0..h {
        // soft edges: full alpha in the middle rows, fading over one row at each edge
        let fy = (y as f32 + 0.5) / h as f32;
        let edge = (fy.min(1.0 - fy) * h as f32).clamp(0.0, 1.0);
        for x in 0..w {
            let cap = x < end || x >= w - end;
            let a = (edge * if cap { 0.85 } else { 0.55 } * 255.0) as u8;
            let i = ((y * w + x) * 4) as usize;
            // premultiplied ARGB, white
            px[i] = a;
            px[i + 1] = a;
            px[i + 2] = a;
            px[i + 3] = a;
        }
    }
    px
}

/// The bar's height in metres for a plane at `distance` (the visual angle `bar_deg`).
pub fn bar_height_m(bar_deg: f32, distance: f32) -> f32 {
    2.0 * distance.max(0.05) * (bar_deg.to_radians() * 0.5).tan()
}

/// Cast a ray against the **bar** of one plane: the strip of height `bar_h` below the plane's
/// bottom edge, spanning its width. Returns the distance along the ray, the plane-local hit
/// (y below −half.y) and the zone.
pub fn bar_hit(origin: [f32; 3], dir: [f32; 3], plane_world: xr::Posef, half: [f32; 2], bar_h: f32) -> Option<(f32, [f32; 2], BarZone)> {
    // the bar as a plane region: extend the extents downward and require the hit below the plane
    let ext_half = [half[0], half[1] + bar_h * 0.5];
    let centre = math::pose_apply(plane_world, [0.0, -bar_h * 0.5, 0.0]);
    let bar_pose = xr::Posef { orientation: plane_world.orientation, position: xr::Vector3f { x: centre[0], y: centre[1], z: centre[2] } };
    let (t, local_in_bar) = scene::ray_plane(origin, dir, bar_pose, ext_half)?;
    // back into the plane's local frame (the bar centre is bar_h/2 below the plane centre)
    let local = [local_in_bar[0], local_in_bar[1] - bar_h * 0.5];
    if local[1] > -half[1] || local[1] < -half[1] - bar_h {
        return None;
    }
    let end = half[0] * 2.0 * BAR_END_FRACTION;
    let zone = if local[0] < -half[0] + end {
        BarZone::LeftEnd
    } else if local[0] > half[0] - end {
        BarZone::RightEnd
    } else {
        BarZone::Middle
    };
    Some((t, local, zone))
}

/// The pure grab: what the stage stores for one grabbed plane.
#[derive(Clone, Copy, Debug)]
pub struct Grab {
    pub member: MemberId,
    /// the grabbing source
    pub kind: SourceKind,
    pub op: Op,
    /// the plane's world pose in the grabbing ray's frame at grab time (`ray⁻¹ ∘ plane`)
    offset: xr::Posef,
    /// the plane's world pose at grab (the mouse's reference; the resize's start)
    start_world: xr::Posef,
    /// the ray's plane-local hit at grab (the resize's reference point)
    start_local: [f32; 2],
    /// the grabbing point's plane-local position now — a mouse accumulates its delta here
    cursor_local: [f32; 2],
    /// the last ray pose seen (for the mouse-less kinds' rotation delta)
    last_ray: Option<xr::Posef>,
    /// the client's request serial, when the grab came from one
    pub serial: Option<Serial>,
    /// the button that holds it (`Select` for the bar / a request; `Grip` for a body grab); a
    /// hand holds by its pinch value instead
    pub held_by: Option<Button>,
    pub started_ns: u64,
    pub moves: u32,
}

impl Grab {
    /// Start a grab of `member` (world pose `plane_world`) by a ray at `ray`, hitting the plane
    /// at plane-local `local`.
    pub fn start(member: MemberId, kind: SourceKind, op: Op, plane_world: xr::Posef, ray: Option<xr::Posef>, local: [f32; 2], now_ns: u64) -> Grab {
        let offset = match ray {
            Some(r) => math::pose_mul(math::pose_inverse(r), plane_world),
            None => plane_world,
        };
        Grab { member, kind, op, offset, start_world: plane_world, start_local: local, cursor_local: local, last_ray: ray, serial: None, held_by: None, started_ns: now_ns, moves: 0 }
    }

    /// The distance from the ray's origin to the plane's centre, along the offset.
    pub fn distance(&self) -> f32 {
        let p = self.offset.position;
        (p.x * p.x + p.y * p.y + p.z * p.z).sqrt()
    }

    /// A new ray pose: the plane's new world pose (`ray ∘ offset`), facing the head upright when
    /// `billboard`, else carrying the ray's rotation (the offset already does).
    pub fn follow(&mut self, ray: xr::Posef, head: Option<xr::Posef>, billboard: bool) -> xr::Posef {
        self.last_ray = Some(ray);
        self.moves += 1;
        let mut world = math::pose_mul(ray, self.offset);
        if billboard {
            if let Some(h) = head {
                world.orientation = face(world.position, h.position);
            }
        }
        world
    }

    /// The mouse's move: a plane-local delta (metres) applied to the start pose, in the plane's
    /// own axes; the plane keeps its depth and orientation.
    pub fn slide(&mut self, dx_m: f32, dy_m: f32) -> xr::Posef {
        self.moves += 1;
        let p = math::pose_apply(self.start_world, [dx_m, dy_m, 0.0]);
        xr::Posef { orientation: self.start_world.orientation, position: xr::Vector3f { x: p[0], y: p[1], z: p[2] } }
    }

    /// Push (axis > 0) or pull along the ray: `d ← d·(1 + rate·axis·dt)`, clamped to the limits.
    /// Returns the new distance.
    pub fn push(&mut self, axis: f32, dt_s: f32, cfg: &GrabCfg) -> f32 {
        let d = self.distance();
        if d <= 1e-4 {
            return d;
        }
        let factor = (1.0 + cfg.depth_rate * axis * dt_s).max(0.05);
        let nd = (d * factor).clamp(cfg.min_distance_m, cfg.max_distance_m);
        let s = nd / d;
        self.offset.position = xr::Vector3f { x: self.offset.position.x * s, y: self.offset.position.y * s, z: self.offset.position.z * s };
        nd
    }

    /// The resize: the ray's plane-local hit now against the one at grab, in logical px at the
    /// scene's density, per the edges; clamped to `[min_px, max_px]` per axis.
    pub fn resize_px(&self, local_now: [f32; 2], m_per_px: f32, min_px: i32, max_px: (i32, i32)) -> Option<(i32, i32)> {
        let Op::Resize { edges, start_px } = self.op else { return None };
        let dx = ((local_now[0] - self.start_local[0]) / m_per_px).round() as i32;
        let dy = ((local_now[1] - self.start_local[1]) / m_per_px).round() as i32;
        let mut w = start_px.0;
        let mut h = start_px.1;
        if edges.right {
            w += dx;
        }
        if edges.left {
            w -= dx;
        }
        // plane-local y is up; the bottom edge grows downward
        if edges.bottom {
            h -= dy;
        }
        if edges.top {
            h += dy;
        }
        Some((w.clamp(min_px, max_px.0.max(min_px)), h.clamp(min_px, max_px.1.max(min_px))))
    }
}

/// A rotation that turns a plane at `pos` to face `head`, upright (world +Y up; wayvr `realign`'s
/// "perfectly upright" branch, StereoKit `quat_lookat_up`). The plane's +Z faces the viewer, so
/// its forward (−Z) points away from the head.
pub fn face(pos: xr::Vector3f, head: xr::Vector3f) -> xr::Quaternionf {
    let fwd = [pos.x - head.x, pos.y - head.y, pos.z - head.z];
    let len = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2]).sqrt();
    if len < 1e-4 {
        return xr::Quaternionf::IDENTITY;
    }
    super::bridge::look_rotation(fwd, [0.0, 1.0, 0.0])
}

/// Which edges a bar zone resizes by.
pub fn edges_for(zone: BarZone) -> Edges {
    match zone {
        BarZone::Middle => Edges::default(),
        BarZone::LeftEnd => Edges { bottom: true, left: true, ..Edges::default() },
        BarZone::RightEnd => Edges { bottom: true, right: true, ..Edges::default() },
    }
}

/// The stage in the `Grabs` slot.
pub struct GrabsStage {
    pub cfg: GrabCfg,
    prefs_gen: u64,
    /// one grab at a time (the tier has one targeting kind; a second grab would fight it)
    grab: Option<Grab>,
    /// per-kind pinch latch for hands (the commit ladder, as the transports judge it)
    pinch_closed: [bool; SourceKind::ALL.len()],
    /// per-kind `Select` held state (buttons), for picking up a client's request
    select_held: [bool; SourceKind::ALL.len()],
    /// per-kind grasp latch for the controller body grab
    grasp_closed: [bool; SourceKind::ALL.len()],
    /// the plane whose bar the targeting ray hovers (drawn), from the last tick
    hover: Option<BarLayer>,
    pub grabs_started: u64,
    pub grabs_from_requests: u64,
    pub grabs_released: u64,
    pub resizes: u64,
    pub pushes: u64,
}

/// A client's request expires if no held commit picks it up within this window.
const REQUEST_TTL_NS: u64 = 1_000_000_000;

fn kind_index(k: SourceKind) -> usize {
    SourceKind::ALL.iter().position(|x| *x == k).unwrap_or(0)
}

fn is_ray(kind: SourceKind) -> bool {
    matches!(kind, SourceKind::Controller(_) | SourceKind::Head | SourceKind::Hand(_))
}

impl Default for GrabsStage {
    fn default() -> Self {
        GrabsStage::new()
    }
}

impl GrabsStage {
    pub fn new() -> GrabsStage {
        GrabsStage { cfg: GrabCfg::default(), prefs_gen: 0, grab: None, pinch_closed: [false; SourceKind::ALL.len()], select_held: [false; SourceKind::ALL.len()], grasp_closed: [false; SourceKind::ALL.len()], hover: None, grabs_started: 0, grabs_from_requests: 0, grabs_released: 0, resizes: 0, pushes: 0 }
    }

    pub fn grab(&self) -> Option<&Grab> {
        self.grab.as_ref()
    }

    /// The commit edge a sample carries for grabbing purposes: `Some(true)` press, `Some(false)`
    /// release; `Select` for buttons, the pinch ladder for hands. Updates the hand latch.
    fn select_edge(&mut self, s: &Sample) -> Option<bool> {
        if let Some((Button::Select, pressed)) = s.button {
            self.select_held[kind_index(s.kind)] = pressed;
            return Some(pressed);
        }
        if matches!(s.kind, SourceKind::Hand(_)) && s.button.is_none() {
            let i = kind_index(s.kind);
            if !self.pinch_closed[i] && s.values.pinch >= self.cfg.pinch_close {
                self.pinch_closed[i] = true;
                return Some(true);
            }
            if self.pinch_closed[i] && s.values.pinch <= self.cfg.pinch_open {
                self.pinch_closed[i] = false;
                return Some(false);
            }
        }
        None
    }

    /// The controller's grasp as a grab-class edge (`Button::Grip` where a profile has a click,
    /// else the `grasp` value on the pinch ladder).
    fn grasp_edge(&mut self, s: &Sample) -> Option<bool> {
        if let Some((Button::Grip, pressed)) = s.button {
            return Some(pressed);
        }
        if matches!(s.kind, SourceKind::Controller(_)) && s.button.is_none() {
            let i = kind_index(s.kind);
            if !self.grasp_closed[i] && s.values.grasp >= self.cfg.pinch_close {
                self.grasp_closed[i] = true;
                return Some(true);
            }
            if self.grasp_closed[i] && s.values.grasp <= self.cfg.pinch_open {
                self.grasp_closed[i] = false;
                return Some(false);
            }
        }
        None
    }

    /// The bar under any mapped, draggable plane hit by a ray.
    fn cast_bar(&self, st: &Zxr, origin: [f32; 3], dir: [f32; 3]) -> Option<(MemberId, f32, [f32; 2], BarZone)> {
        let mut best: Option<(MemberId, f32, [f32; 2], BarZone)> = None;
        for (id, m) in st.scene.iter() {
            if !(m.m.mapped() && !m.m.hidden) || !m.flags.contains(scene::Flags::DRAGGABLE) {
                continue;
            }
            let Shape::Plane { size } = m.shape else { continue };
            let Some(world) = st.scene.world_pose(id) else { continue };
            let d = distance_from(st, world.position);
            let bar_h = bar_height_m(self.cfg.bar_deg, d);
            if let Some((t, local, zone)) = bar_hit(origin, dir, world, [size[0] * 0.5, size[1] * 0.5], bar_h) {
                if best.map(|b| t < b.1).unwrap_or(true) {
                    best = Some((id, t, local, zone));
                }
            }
        }
        best
    }

    fn begin(&mut self, st: &mut Zxr, member: MemberId, kind: SourceKind, op: Op, ray: Option<xr::Posef>, local: [f32; 2], held_by: Option<Button>, serial: Option<Serial>, now_ns: u64) {
        let Some(world) = st.scene.world_pose(member) else { return };
        let mut g = Grab::start(member, kind, op, world, ray, local, now_ns);
        g.held_by = held_by;
        g.serial = serial;
        self.grab = Some(g);
        self.grabs_started += 1;
        if serial.is_some() {
            self.grabs_from_requests += 1;
        }
        st.input.grabbed = Some(member);
        st.input.grabbing_kind = Some(kind);
        tracing::info!(?member, ?kind, ?op, from_request = serial.is_some(), "grab: start");
    }

    fn end(&mut self, st: &mut Zxr, reason: &str) {
        if let Some(g) = self.grab.take() {
            self.grabs_released += 1;
            st.input.grabbed = None;
            st.input.grabbing_kind = None;
            st.journal.grab_moves += g.moves as u64;
            tracing::info!(member = ?g.member, kind = ?g.kind, moves = g.moves, reason, "grab: end (the pose stays)");
        }
    }

    /// Apply a new world pose to the grabbed plane as a place-local pose.
    fn place(&self, st: &mut Zxr, member: MemberId, world: xr::Posef) {
        let Some(m) = st.scene.get(member) else { return };
        let Some(place_world) = st.scene.place_world(m.place) else { return };
        let local = math::pose_mul(math::pose_inverse(place_world), world);
        st.scene.set_local(member, local);
    }

    /// The resize clamp: the plane's width may not exceed `max_angular_deg` at its distance.
    fn max_px(&self, st: &Zxr, member: MemberId) -> (i32, i32) {
        let d = st.scene.world_pose(member).map(|w| distance_from(st, w.position)).unwrap_or(1.5);
        let max_m = 2.0 * d * (self.cfg.max_angular_deg.to_radians() * 0.5).tan();
        let px = (max_m / st.scene.layout.m_per_px).max(64.0) as i32;
        (px, px)
    }

    fn apply_resize(&mut self, st: &mut Zxr, member: MemberId, local_now: [f32; 2]) {
        let Some(g) = self.grab.as_ref() else { return };
        let max = self.max_px(st, member);
        let Some((w, h)) = g.resize_px(local_now, st.scene.layout.m_per_px, 32, max) else { return };
        st.request_size(member, w, h);
        self.resizes += 1;
    }
}

/// Distance from the head (or the origin before the first head pose) to a world point.
fn distance_from(st: &Zxr, p: xr::Vector3f) -> f32 {
    let h = st.input.head.map(|h| h.position).unwrap_or(xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 });
    let d = [p.x - h.x, p.y - h.y, p.z - h.z];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(0.05)
}

impl Stage for GrabsStage {
    fn name(&self) -> &'static str {
        "grabs:wm"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        let select = self.select_edge(s);
        let grasp = self.grasp_edge(s);

        // ---- an active grab by this kind: follow, push, resize, or end -------------------------
        if let Some(g) = self.grab.as_ref().copied() {
            if g.kind != s.kind {
                return Flow::Continue;
            }
            // end: the holding commit released, or the source lost; a shortcut grab (held by
            // nothing) ends on the next press (kwin-vr `Main.qml:68-77`)
            let released = match g.held_by {
                Some(Button::Grip) => grasp == Some(false),
                Some(_) => select == Some(false),
                None => select == Some(true),
            } || (is_ray(s.kind) && s.pose.is_some() && !s.tracked && s.button.is_none());
            if released {
                self.end(st, if !is_ray(s.kind) || s.tracked { "release" } else { "source lost" });
                return Flow::Consumed;
            }
            // depth on the secondary axis (a controller stick, a wheel); dt from the sample clock
            if let Some((_, ay)) = s.axis {
                if ay != 0.0 {
                    let dt = 1.0 / 60.0; // one tick's worth: the axis is sampled per tick
                    let axis = if matches!(s.axis_source, Some(super::AxisSource::Wheel)) { -(ay as f32) * 0.25 } else { -(ay as f32) };
                    if let Some(gm) = self.grab.as_mut() {
                        gm.push(axis, dt, &self.cfg);
                        self.pushes += 1;
                    }
                }
            }
            match g.op {
                Op::Move => {
                    if let Some(ray) = s.pose.filter(|_| is_ray(s.kind)) {
                        let head = st.input.head;
                        let billboard = self.cfg.billboard;
                        let world = self.grab.as_mut().map(|gm| gm.follow(ray, head, billboard));
                        if let Some(w) = world {
                            self.place(st, g.member, w);
                        }
                    } else if let Some((dx, dy)) = s.delta {
                        // the mouse: plane-local slide by its delta at the scene's density
                        let mpp = st.scene.layout.m_per_px;
                        let (dx_m, dy_m) = ((dx as f32) * mpp, -(dy as f32) * mpp);
                        if let Some(gm) = self.grab.as_mut() {
                            // accumulate: the start pose moves with every delta
                            let w = gm.slide(dx_m, dy_m);
                            gm.start_world = w;
                            self.place(st, g.member, w);
                        }
                    } else if s.axis.is_some() {
                        // depth only: re-place from the last ray
                        if let Some(gm) = self.grab.as_mut() {
                            if let Some(ray) = gm.last_ray {
                                let head = st.input.head;
                                let billboard = self.cfg.billboard;
                                let w = gm.follow(ray, head, billboard);
                                self.place(st, g.member, w);
                            }
                        }
                    }
                }
                Op::Resize { .. } => {
                    // the ray's plane-local point now (the plane extended, so a hit past its
                    // edge still resizes); a mouse accumulates its delta into the same point
                    let local_now = if let Some(ray) = s.pose.filter(|_| is_ray(s.kind)) {
                        st.scene.world_pose(g.member).and_then(|w| {
                            let o = [ray.position.x, ray.position.y, ray.position.z];
                            let d = math::rotate(ray.orientation, [0.0, 0.0, -1.0]);
                            scene::ray_plane(o, d, w, [50.0, 50.0]).map(|(_, l)| l)
                        })
                    } else if let Some((dx, dy)) = s.delta {
                        let mpp = st.scene.layout.m_per_px;
                        self.grab.as_mut().map(|gm| {
                            gm.cursor_local = [gm.cursor_local[0] + (dx as f32) * mpp, gm.cursor_local[1] - (dy as f32) * mpp];
                            gm.cursor_local
                        })
                    } else {
                        None
                    };
                    if let Some(l) = local_now {
                        self.apply_resize(st, g.member, l);
                    }
                }
            }
            return Flow::Consumed;
        }

        // ---- no grab: a client's request, or a press on an affordance -------------------------
        let now = s.time_ns;
        let ray = s.pose.filter(|_| is_ray(s.kind));
        let hit_local = |st: &Zxr, member: MemberId| -> Option<[f32; 2]> { st.input.hits.iter().find(|h| h.kind == s.kind && h.member == member).map(|h| h.local) };

        // (1) a client's move/resize request: the kind whose commit is held takes it
        if let Some(req) = st.input.grab_request {
            if now.saturating_sub(req.at_ns) > REQUEST_TTL_NS {
                st.input.grab_request = None;
            } else {
                let holding = match s.kind {
                    SourceKind::Hand(_) => self.pinch_closed[kind_index(s.kind)],
                    SourceKind::Pointer | SourceKind::Controller(_) | SourceKind::Head => self.select_held[kind_index(s.kind)] || select == Some(true),
                    _ => false,
                };
                if holding && (ray.is_some() || s.delta.is_some() || select == Some(true)) {
                    st.input.grab_request = None;
                    let local = hit_local(st, req.member).unwrap_or([0.0, 0.0]);
                    self.begin(st, req.member, s.kind, req.op, ray, local, Some(Button::Select), Some(req.serial), now);
                    return Flow::Consumed;
                }
            }
        }

        // (2a) the bar: a commit press whose ray lands on a plane's bar
        if select == Some(true) {
            if let Some(r) = ray {
                let o = [r.position.x, r.position.y, r.position.z];
                let d = math::rotate(r.orientation, [0.0, 0.0, -1.0]);
                if let Some((member, _t, local, zone)) = self.cast_bar(st, o, d) {
                    let op = match zone {
                        BarZone::Middle => Op::Move,
                        z => Op::Resize { edges: edges_for(z), start_px: st.logical_size(member).unwrap_or((800, 600)) },
                    };
                    self.begin(st, member, s.kind, op, ray, local, Some(Button::Select), None, now);
                    return Flow::Consumed;
                }
            }
            // a mouse press with the desktop modifier on a plane's body: a body grab
            if s.kind == SourceKind::Pointer && st.modifier_logo() {
                if let Some(h) = st.input.hits.iter().find(|h| h.kind == SourceKind::Pointer).copied() {
                    self.begin(st, h.member, s.kind, Op::Move, None, h.local, Some(Button::Select), None, now);
                    return Flow::Consumed;
                }
            }
        }
        // (2b) a controller's grasp on a plane's body
        if grasp == Some(true) {
            if let Some(r) = ray {
                if let Some(h) = st.input.hits.iter().find(|h| h.kind == s.kind).copied() {
                    self.begin(st, h.member, s.kind, Op::Move, Some(r), h.local, Some(Button::Grip), None, now);
                    return Flow::Consumed;
                }
            }
        }
        Flow::Continue
    }

    fn tick(&mut self, st: &mut Zxr, now_ns: u64) {
        if self.prefs_gen != st.prefs.generation {
            self.prefs_gen = st.prefs.generation;
            self.cfg = st.prefs.grab_cfg();
        }
        // a grabbed plane that vanished (closed) ends the grab; so does `ctl grab end`
        if let Some(g) = self.grab {
            if st.scene.get(g.member).is_none() {
                self.end(st, "member gone");
            } else if st.input.grab_end {
                self.end(st, "ctl");
            }
        }
        st.input.grab_end = false;
        // `ctl grab focused`: the head ray grabs the member, held by nothing until the next press
        if let Some(member) = st.input.grab_shortcut.take() {
            if self.grab.is_none() {
                let ray = st.input.head;
                let local = [0.0, 0.0];
                self.begin(st, member, SourceKind::Head, Op::Move, ray, local, None, None, now_ns);
            }
        }
        // the bar to draw: the grabbed plane's, else the one the targeting ray hovers
        let target = self.grab.map(|g| (g.member, true)).or_else(|| {
            let targeting = st.input.tier.map(|t| t.targeting)?;
            if !is_ray(targeting) {
                return None;
            }
            // the targeting kind's latest ray pose is the hit's source; recover the ray from the
            // head for the head kind, else from the last sample the seat saw is not kept — use
            // the hit itself: a hit on the plane means the ray is not on the bar
            let head = st.input.head?;
            if targeting != SourceKind::Head {
                return None;
            }
            let o = [head.position.x, head.position.y, head.position.z];
            let d = math::rotate(head.orientation, [0.0, 0.0, -1.0]);
            self.cast_bar(st, o, d).map(|(m, _, _, _)| (m, false))
        });
        self.hover = target.and_then(|(member, grabbed)| {
            let m = st.scene.get(member)?;
            let Shape::Plane { size } = m.shape else { return None };
            let world = st.scene.world_pose(member)?;
            let d = distance_from(st, world.position);
            let bar_h = bar_height_m(self.cfg.bar_deg, d);
            let c = math::pose_apply(world, [0.0, -size[1] * 0.5 - bar_h * 0.5, 0.0005]);
            Some(BarLayer { member, pose: xr::Posef { orientation: world.orientation, position: xr::Vector3f { x: c[0], y: c[1], z: c[2] } }, size: [size[0], bar_h], grabbed })
        });
        st.input.grab_bar = self.hover;
        st.journal.grabs_started = self.grabs_started;
        st.journal.grabs_from_requests = self.grabs_from_requests;
        st.journal.grabs_released = self.grabs_released;
        st.journal.grab_resizes = self.resizes;
        st.journal.grab_pushes = self.pushes;
        // a stale request nobody picked up
        if let Some(req) = st.input.grab_request {
            if now_ns.saturating_sub(req.at_ns) > REQUEST_TTL_NS {
                st.input.grab_request = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Side;

    fn plane_at(z: f32) -> xr::Posef {
        math::pose_yaw([0.0, 0.0, z], 0.0)
    }

    fn ray_from(origin: [f32; 3], yaw: f32) -> xr::Posef {
        math::pose_yaw(origin, yaw)
    }

    fn member() -> MemberId {
        let mut s: crate::scene::Scene<()> = crate::scene::Scene::new();
        s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, scene::Flags::WINDOW, ()).unwrap()
    }

    #[test]
    fn the_bar_is_below_the_plane_and_its_ends_resize() {
        // a 1×0.6 plane at 1.5 m; the bar 2° high at 1.5 m ≈ 5.2 cm
        let world = plane_at(-1.5);
        let half = [0.5, 0.3];
        let bar_h = bar_height_m(2.0, 1.5);
        assert!((bar_h - 0.0524).abs() < 0.001);
        // a ray from the origin aimed at the bar's middle (y = −0.3 − bar_h/2 at z = −1.5)
        let y = -0.3 - bar_h * 0.5;
        let dir_len = (1.5f32 * 1.5 + y * y).sqrt();
        let dir = [0.0, y / dir_len, -1.5 / dir_len];
        let (t, local, zone) = bar_hit([0.0; 3], dir, world, half, bar_h).expect("hits the bar");
        assert!((t - dir_len).abs() < 1e-3);
        assert!(local[1] < -0.3 && local[1] > -0.3 - bar_h, "below the plane's bottom edge: {local:?}");
        assert_eq!(zone, BarZone::Middle);
        // the right end
        let dir = [0.47 / dir_len, y / dir_len, -1.5 / dir_len];
        let (_, _, zone) = bar_hit([0.0; 3], dir, world, half, bar_h).unwrap();
        assert_eq!(zone, BarZone::RightEnd);
        assert_eq!(edges_for(zone), Edges { bottom: true, right: true, ..Edges::default() });
        // the plane's body is not the bar
        assert!(bar_hit([0.0; 3], [0.0, 0.0, -1.0], world, half, bar_h).is_none());
        // and nothing below the bar is either
        let y2 = -0.3 - bar_h * 1.5;
        let l2 = (1.5f32 * 1.5 + y2 * y2).sqrt();
        assert!(bar_hit([0.0; 3], [0.0, y2 / l2, -1.5 / l2], world, half, bar_h).is_none());
    }

    #[test]
    fn a_grab_keeps_the_plane_where_it_was_relative_to_the_ray() {
        let world = plane_at(-1.5);
        let ray = ray_from([0.0, 0.0, 0.0], 0.0);
        let mut g = Grab::start(member(), SourceKind::Controller(Side::Right), Op::Move, world, Some(ray), [0.0, -0.3], 0);
        assert!((g.distance() - 1.5).abs() < 1e-5);
        // the ray does not move: the plane does not move (no jump at grab time)
        let w = g.follow(ray, None, false);
        assert!((w.position.z + 1.5).abs() < 1e-5 && w.position.x.abs() < 1e-5);
        // the ray yaws 90° left: the plane swings to −X at the same distance
        let w = g.follow(ray_from([0.0; 3], std::f32::consts::FRAC_PI_2), None, false);
        assert!((w.position.x + 1.5).abs() < 1e-4 && w.position.z.abs() < 1e-4, "{:?}", w.position);
        // the ray's origin moves 0.2 m up: the plane follows
        let w = g.follow(ray_from([0.0, 0.2, 0.0], 0.0), None, false);
        assert!((w.position.y - 0.2).abs() < 1e-5);
        assert_eq!(g.moves, 3);
    }

    #[test]
    fn billboard_faces_the_head_upright() {
        let world = plane_at(-1.5);
        let ray = ray_from([0.0; 3], 0.0);
        let mut g = Grab::start(member(), SourceKind::Head, Op::Move, world, Some(ray), [0.0, 0.0], 0);
        let head = xr::Posef { orientation: xr::Quaternionf::IDENTITY, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } };
        // swung to the left with billboard: the plane's +Z points back at the head
        let w = g.follow(ray_from([0.0; 3], std::f32::consts::FRAC_PI_2), Some(head), true);
        let normal = math::rotate(w.orientation, [0.0, 0.0, 1.0]);
        let to_head = [-w.position.x, -w.position.y, -w.position.z];
        let l = (to_head[0] * to_head[0] + to_head[2] * to_head[2]).sqrt();
        let dot = (normal[0] * to_head[0] + normal[1] * to_head[1] + normal[2] * to_head[2]) / l;
        assert!(dot > 0.999, "faces the head: dot={dot}");
        let up = math::rotate(w.orientation, [0.0, 1.0, 0.0]);
        assert!(up[1] > 0.999, "upright: {up:?}");
    }

    #[test]
    fn push_is_multiplicative_and_clamped() {
        let world = plane_at(-1.5);
        let ray = ray_from([0.0; 3], 0.0);
        let mut g = Grab::start(member(), SourceKind::Controller(Side::Left), Op::Move, world, Some(ray), [0.0, 0.0], 0);
        let cfg = GrabCfg::default();
        // one second of full push at rate 3.0 in 60 steps ≈ ×(1+0.05)^60 ≈ 18.7 → clamped to 5 m
        for _ in 0..60 {
            g.push(1.0, 1.0 / 60.0, &cfg);
        }
        assert!((g.distance() - cfg.max_distance_m).abs() < 1e-4);
        for _ in 0..600 {
            g.push(-1.0, 1.0 / 60.0, &cfg);
        }
        assert!((g.distance() - cfg.min_distance_m).abs() < 1e-4, "pulled to the floor: {}", g.distance());
        // a small push from 1 m by 0.5 for one tick: ×(1 + 3·0.5/60)
        let mut g = Grab::start(member(), SourceKind::Controller(Side::Left), Op::Move, plane_at(-1.0), Some(ray), [0.0, 0.0], 0);
        let nd = g.push(0.5, 1.0 / 60.0, &cfg);
        assert!((nd - 1.025).abs() < 1e-4);
    }

    #[test]
    fn resize_moves_the_grabbed_edges_in_pixels_and_clamps() {
        let world = plane_at(-1.5);
        let ray = ray_from([0.0; 3], 0.0);
        let edges = Edges { bottom: true, right: true, ..Edges::default() };
        let g = Grab::start(member(), SourceKind::Hand(Side::Right), Op::Resize { edges, start_px: (800, 600) }, world, Some(ray), [0.5, -0.3], 0);
        let mpp = 1.0 / 830.0;
        // the hand moves 8.3 cm right and 4.15 cm down: +69 px wide, +34 px tall
        let (w, h) = g.resize_px([0.583, -0.3415], mpp, 32, (4000, 4000)).unwrap();
        assert_eq!((w, h), (869, 634));
        // the clamp
        let (w, _) = g.resize_px([5.0, -0.3], mpp, 32, (1000, 1000)).unwrap();
        assert_eq!(w, 1000);
        // a left/top grab grows the other way
        let g = Grab::start(member(), SourceKind::Hand(Side::Right), Op::Resize { edges: Edges { left: true, top: true, ..Edges::default() }, start_px: (800, 600) }, world, Some(ray), [-0.5, 0.3], 0);
        let (w, h) = g.resize_px([-0.583, 0.3415], mpp, 32, (4000, 4000)).unwrap();
        assert_eq!((w, h), (869, 634));
        assert!(g.resize_px([0.0, 0.0], mpp, 32, (10, 10)).is_some());
    }
}
