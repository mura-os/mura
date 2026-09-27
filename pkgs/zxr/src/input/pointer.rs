//! The pointer-class transport (spatial-input §5 "Pointer-class — mice, trackpads, controllers
//! when targeting", ruled; §8 "Where the mouse pointer is"; ADR 0013 amendment items 2, 3, 4,
//! 6; research/63 §5, §7, §8).
//!
//! "The seat's `wl_pointer`: `enter/motion/button/axis/frame`. Mouse buttons and controller
//! `select` → `BTN_LEFT`; the profile's secondary button → `BTN_RIGHT`; wheel → `axis` source
//! `wheel` with `axis_discrete`; trackpad and controller stick → `axis` source
//! `finger`/`continuous` with `axis_stop` … Relative motion and pointer constraints are served."
//! (spatial-input.md:258-264). "**One logical pointer per seat** … the one that last committed
//! owns the pointer; the other's ray is drawn but inert until it commits. Mice and controllers
//! share the same pointer." (:266-268; xrdesktop `xrd-input-synth.c:198-205` — "when left
//! clicking with a controller that is *not* used to do input synth, make this controller do
//! input synth"; WiVRn `imgui_impl.cpp:726-768` switches on trigger edge or scroll).
//!
//! **The mouse** (spatial-input §8, ruled; ADR 0013 item 6): "it lives on a plane and moves in
//! that plane's local coordinates, 1:1 like a screen (libinput **flat** profile with a compositor
//! gain …). When the wearer's look has moved to another member and the mouse moves, the pointer
//! **warps** to the looked-at plane at the gaze point … degrading to the head ray's hit when gaze
//! is unavailable. When the pointer leaves a plane's bounds without a look change, it continues
//! as an **angular ray from the head** … until it lands on another plane" (:325-335). The
//! angular ray is implemented as: with no plane, the pointer is the head ray's hit (the brief's
//! reduction; wxrc's yaw/pitch integration `input.c:300-307` is the fuller shape — flagged).
//!
//! **The gaze-scroll exception** (spatial-input §5 :273-278, §9; ADR 0013 item 3): a stick or
//! wheel while gaze targets → `enter` at the gaze hit, `axis`, `frame`, `leave`, nothing else.
//!
//! Ported smithay shape: `PointerHandle::{motion, relative_motion, button, axis, frame}`
//! (`references/smithay/src/input/pointer/mod.rs:232-311`); `motion` with a focus is enter or
//! motion by smithay's own bookkeeping (`:800-823`); `motion(None)` is the leave. The position
//! passed is the plane's logical point and the focus origin the surface's origin in that space,
//! so the surface-local point is what `hit_surface_at` computed (R0's `update_gaze_pointer`
//! passed the surface-local point as both and so always entered at (0,0) — flagged).
//!
//! **Stand-ins (flagged):** compositor gain 1.0 logical px per device unit (the brief's; the
//! design gives none — `input.pointer.gain` is the setting); a wheel detent scrolls 15 logical
//! px (niri `input/mod.rs:3542-3547`: `v120 / 120 * 15`, the desktop convention libinput's
//! degrees-per-click became).
//!
//! Budget: fixed state, a reused op buffer; no allocation per event in steady state.

use smithay::backend::input::{Axis, AxisRelativeDirection, AxisSource as SmAxisSource, ButtonState, InputTime};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent, PointerHandle, RelativeMotionEvent};
use smithay::utils::{Point, SERIAL_COUNTER};

use super::{AxisSource, Button, Flow, Hit, Sample, SourceKind};
use crate::scene::{MemberId, M_PER_PX};
use crate::state::Zxr;

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// Logical px one wheel detent scrolls (niri `src/input/mod.rs:3542-3547`; flagged stand-in).
pub const WHEEL_PX_PER_DETENT: f64 = 15.0;
/// Compositor gain: logical px per device unit on the flat profile (flagged stand-in, 1.0).
pub const DEFAULT_GAIN: f64 = 1.0;

/// `Button` → evdev code for `wl_pointer.button` (spatial-input §5 :259-260). `Menu`, `Back`,
/// `System` and `Grip` are not pointer buttons: the reserved stage and the shell own them.
pub fn button_code(b: Button) -> Option<u32> {
    match b {
        Button::Select => Some(BTN_LEFT),
        Button::Secondary => Some(BTN_RIGHT),
        Button::Middle => Some(BTN_MIDDLE),
        Button::Code(c) => Some(c),
        Button::Menu | Button::Back | Button::System | Button::Grip => None,
    }
}

/// `Sample.axis` + `axis_source` → `AxisFrame` (spatial-input §5 :261-262): wheel = discrete
/// `v120` steps plus a value; finger/continuous = value, and a `stop` on the axis that returned
/// to 0 after moving (smithay: "Using `AxisSource::Finger` requires a stop event to be sent",
/// `input/pointer/mod.rs:1117-1124`; libinput guarantees the terminating 0 for finger,
/// `backend/input/mod.rs:363-369`).
#[derive(Debug, Default, Clone, Copy)]
pub struct AxisMap {
    moving: (bool, bool),
}

impl AxisMap {
    /// The frame for one axis sample, or `None` when there is nothing to send (a 0 on an axis
    /// that was not moving).
    pub fn frame(&mut self, axis: (f64, f64), source: AxisSource, time: InputTime) -> Option<AxisFrame> {
        let mut f = AxisFrame::new(time).source(match source {
            AxisSource::Wheel => SmAxisSource::Wheel,
            AxisSource::Finger => SmAxisSource::Finger,
            AxisSource::Continuous => SmAxisSource::Continuous,
        });
        let mut any = false;
        for (i, (a, v)) in [(Axis::Horizontal, axis.0), (Axis::Vertical, axis.1)].into_iter().enumerate() {
            let moving = if i == 0 { &mut self.moving.0 } else { &mut self.moving.1 };
            f = f.relative_direction(a, AxisRelativeDirection::Identical);
            if v != 0.0 {
                any = true;
                match source {
                    AxisSource::Wheel => {
                        f = f.v120(a, (v * 120.0).round() as i32).value(a, v * WHEEL_PX_PER_DETENT);
                    }
                    AxisSource::Finger | AxisSource::Continuous => {
                        f = f.value(a, v);
                        *moving = true;
                    }
                }
            } else if *moving {
                any = true;
                f = f.stop(a);
                *moving = false;
            }
        }
        if any { Some(f) } else { None }
    }
}

/// One logical pointer per seat, handed to the pointer-class device that last committed
/// (spatial-input §5 :266-268; ADR 0013 item 4; xrdesktop `xrd-input-synth.c:198-205`).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PointerOwner {
    owner: Option<SourceKind>,
    pub handoffs: u64,
}

impl PointerOwner {
    pub fn owner(&self) -> Option<SourceKind> {
        self.owner
    }

    /// A commit by `kind` (button press, scroll, mouse motion): it owns the pointer from now.
    /// Returns whether ownership changed.
    pub fn commit(&mut self, kind: SourceKind) -> bool {
        if self.owner == Some(kind) {
            return false;
        }
        if self.owner.is_some() {
            self.handoffs += 1;
        }
        self.owner = Some(kind);
        true
    }

    /// May this kind's ray move the pointer now? The owner's may; with no owner yet the first
    /// ray to arrive takes it (xrdesktop's default primary controller — flagged).
    pub fn drives(&mut self, kind: SourceKind) -> bool {
        match self.owner {
            None => {
                self.owner = Some(kind);
                true
            }
            Some(o) => o == kind,
        }
    }
}

/// What the transport will send, planned by the pure logic and executed by the adapter.
#[derive(Debug, Clone)]
pub enum PtrOp {
    /// pointer over a plane point: smithay makes it `enter` or `motion`
    Move { member: MemberId, local: [f32; 2] },
    /// the pointer is on no surface: `leave`
    Leave,
    /// relative motion in logical px (relative-pointer)
    Relative { dx: f64, dy: f64 },
    Button { code: u32, pressed: bool },
    Axis(AxisFrame),
    Frame,
}

impl PartialEq for PtrOp {
    fn eq(&self, other: &Self) -> bool {
        use PtrOp::*;
        match (self, other) {
            (Move { member: a, local: b }, Move { member: c, local: d }) => a == c && b == d,
            (Leave, Leave) | (Frame, Frame) => true,
            (Relative { dx, dy }, Relative { dx: x, dy: y }) => dx == x && dy == y,
            (Button { code, pressed }, Button { code: c, pressed: p }) => code == c && pressed == p,
            (Axis(a), Axis(b)) => a.axis == b.axis && a.v120 == b.v120 && a.stop == b.stop && a.source == b.source,
            _ => false,
        }
    }
}

/// What the planner needs to know about this tick that is not in the sample.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// the tier's targeting kind (spatial-input §3), if the Tier stage has run
    pub targeting: Option<SourceKind>,
    /// this sample's own ray hit (ray kinds)
    pub own: Option<(MemberId, [f32; 2])>,
    /// the looked-at point: the targeting ray's hit — gaze, degrading to head (§8 warp rule)
    pub look: Option<(MemberId, [f32; 2])>,
    /// the head ray's hit (the angular-ray landing point when the pointer has no plane)
    pub head: Option<(MemberId, [f32; 2])>,
    /// half extents of the plane the pointer is on, for the bounds check (mouse motion)
    pub plane_half: Option<[f32; 2]>,
}

/// The pure pointer planner: ownership, the pointer's plane position, the axis latch.
#[derive(Debug)]
pub struct PointerLogic {
    pub owner: PointerOwner,
    pub axis: AxisMap,
    /// where the logical pointer is: on a plane (member, plane-local metres) or between planes
    pub plane: Option<(MemberId, [f32; 2])>,
    /// logical px per device unit (`input.pointer.gain`)
    pub gain: f64,
    on_surface: bool,
    pub gaze_scrolls: u64,
    pub warps: u64,
    /// ray-owned pointers released (`leave`) because gaze took the tier (spatial-input §5)
    pub releases: u64,
}

impl Default for PointerLogic {
    fn default() -> Self {
        PointerLogic { owner: PointerOwner::default(), axis: AxisMap::default(), plane: None, gain: DEFAULT_GAIN, on_surface: false, gaze_scrolls: 0, warps: 0, releases: 0 }
    }
}

impl PointerLogic {
    /// Gaze took the tier (spatial-input §3) while a **ray** (head, controller) owns the pointer:
    /// the ray no longer targets — with gaze present "gaze targets and the trigger commits" (ADR
    /// 0013 amendment item 1) — so its hover ends: `leave`, no plane, until the ray retakes the
    /// tier or a pointer-class device claims the pointer (owner kept). A mouse-owned pointer is
    /// not touched: its position is the mouse's, and a pointer coexists with gaze (visionOS's
    /// pointer "appears where you're looking" [external], research/63 §8). Never mid-gesture: the
    /// tier does not change while a commit is in progress (§3), so no `leave` with a button down.
    /// Returns whether it released.
    pub fn release_for_gaze(&mut self, targeting: Option<SourceKind>, out: &mut Vec<PtrOp>) -> bool {
        out.clear();
        let ray_owner = matches!(self.owner.owner(), Some(SourceKind::Controller(_) | SourceKind::Head | SourceKind::Hand(_)));
        if targeting == Some(SourceKind::Gaze) && ray_owner && self.plane.is_some() {
            self.move_to(None, out);
            out.push(PtrOp::Frame);
            self.releases += 1;
            return true;
        }
        false
    }

    fn move_to(&mut self, p: Option<(MemberId, [f32; 2])>, out: &mut Vec<PtrOp>) {
        match p {
            Some((member, local)) => {
                self.plane = Some((member, local));
                self.on_surface = true;
                out.push(PtrOp::Move { member, local });
            }
            None => {
                self.plane = None;
                if self.on_surface {
                    self.on_surface = false;
                    out.push(PtrOp::Leave);
                }
            }
        }
    }

    /// Plan the ops for one pointer-class sample.
    pub fn plan(&mut self, s: &Sample, cx: &Context, out: &mut Vec<PtrOp>) {
        out.clear();
        let time = InputTime::from_micros(s.time_ns / 1000);

        // the gaze-scroll exception (§5 :273-278; ADR 0013 item 3): enter, axis, frame, leave
        if cx.targeting == Some(SourceKind::Gaze) && s.kind != SourceKind::Gaze {
            if let (Some((h, v)), Some(src)) = (s.axis, s.axis_source) {
                let Some((member, local)) = cx.look else { return };
                let Some(frame) = self.axis.frame((h, v), src, time) else { return };
                out.push(PtrOp::Move { member, local });
                out.push(PtrOp::Axis(frame));
                out.push(PtrOp::Frame);
                out.push(PtrOp::Leave);
                self.on_surface = false;
                self.gaze_scrolls += 1;
                return;
            }
        }

        match s.kind {
            SourceKind::Pointer => {
                if let Some((dx, dy)) = s.delta {
                    self.owner.commit(SourceKind::Pointer);
                    // warp when the look has moved to another member (§8; ADR 0013 item 6)
                    if let Some((lm, ll)) = cx.look {
                        if self.plane.map(|(m, _)| m != lm).unwrap_or(false) {
                            self.plane = Some((lm, ll));
                            self.warps += 1;
                        }
                    }
                    // between planes: the angular ray lands where the head ray hits
                    if self.plane.is_none() {
                        self.plane = cx.look.or(cx.head);
                    }
                    let (px, py) = (dx * self.gain, dy * self.gain);
                    if let Some((m, mut local)) = self.plane {
                        local[0] += (px * M_PER_PX as f64) as f32;
                        local[1] -= (py * M_PER_PX as f64) as f32;
                        let inside = cx.plane_half.map(|h| local[0].abs() <= h[0] && local[1].abs() <= h[1]).unwrap_or(true);
                        if inside {
                            self.move_to(Some((m, local)), out);
                        } else {
                            // left the plane's bounds: an angular ray until it lands (§8)
                            self.move_to(None, out);
                        }
                    }
                    out.push(PtrOp::Relative { dx: px, dy: py });
                    out.push(PtrOp::Frame);
                }
                if let Some((b, pressed)) = s.button {
                    if let Some(code) = button_code(b) {
                        if pressed {
                            self.owner.commit(SourceKind::Pointer);
                        }
                        out.push(PtrOp::Button { code, pressed });
                        out.push(PtrOp::Frame);
                    }
                }
                if let (Some(axis), Some(src)) = (s.axis, s.axis_source) {
                    if let Some(frame) = self.axis.frame(axis, src, time) {
                        self.owner.commit(SourceKind::Pointer);
                        out.push(PtrOp::Axis(frame));
                        out.push(PtrOp::Frame);
                    }
                }
            }
            SourceKind::Keyboard => {}
            ray => {
                // a commit hands the pointer to this ray (§5 :266-268); the owner's ray drives
                let press = matches!(s.button, Some((b, true)) if button_code(b).is_some());
                let scroll = s.axis.map(|a| a != (0.0, 0.0)).unwrap_or(false);
                if press || scroll {
                    self.owner.commit(ray);
                }
                if s.pose.is_some() && self.owner.drives(ray) {
                    let target = if s.tracked { cx.own } else { None };
                    if target != self.plane || (target.is_some() && !self.on_surface) {
                        self.move_to(target, out);
                        if !out.is_empty() {
                            out.push(PtrOp::Frame);
                        }
                    }
                }
                if self.owner.owner() == Some(ray) {
                    if let Some((b, pressed)) = s.button {
                        if let Some(code) = button_code(b) {
                            out.push(PtrOp::Button { code, pressed });
                            out.push(PtrOp::Frame);
                        }
                    }
                    if let (Some(axis), Some(src)) = (s.axis, s.axis_source) {
                        if let Some(frame) = self.axis.frame(axis, src, time) {
                            out.push(PtrOp::Axis(frame));
                            out.push(PtrOp::Frame);
                        }
                    }
                }
            }
        }
    }
}

/// The smithay adapter: executes planned ops on the seat's `wl_pointer`.
pub struct PointerTransport {
    pointer: PointerHandle<Zxr>,
    pub logic: PointerLogic,
    ops: Vec<PtrOp>,
    /// the plane the last `Move` landed on (the button's commit target for focus)
    last_member: Option<MemberId>,
    /// the focus tuple of the last `Move` (relative motion is delivered against it)
    last_focus: Option<(smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, Point<f64, smithay::utils::Logical>)>,
}

impl PointerTransport {
    pub fn new(st: &mut Zxr) -> PointerTransport {
        let pointer = match st.seat.get_pointer() {
            Some(p) => p,
            None => st.seat.add_pointer(),
        };
        PointerTransport { pointer, logic: PointerLogic::default(), ops: Vec::with_capacity(8), last_member: None, last_focus: None }
    }

    /// Per tick: release a ray-owned pointer when gaze targets (`PointerLogic::release_for_gaze`).
    pub fn tick(&mut self, targeting: Option<SourceKind>, now_ns: u64, st: &mut Zxr) {
        let mut ops = std::mem::take(&mut self.ops);
        if self.logic.release_for_gaze(targeting, &mut ops) {
            self.deliver(&ops, InputTime::from_micros(now_ns / 1000), st);
        }
        self.ops = ops;
    }

    /// Deliver one pointer-class sample.
    pub fn run(&mut self, s: &Sample, own: Option<&Hit>, look: Option<&Hit>, head: Option<&Hit>, targeting: Option<SourceKind>, st: &mut Zxr) -> Flow {
        let pt = |h: Option<&Hit>| h.map(|h| (h.member, h.local));
        let plane_half = self.logic.plane.and_then(|(m, _)| st.scene.get(m)).map(|m| m.half_size());
        let cx = Context { targeting, own: pt(own), look: pt(look), head: pt(head), plane_half };
        let mut ops = std::mem::take(&mut self.ops);
        self.logic.plan(s, &cx, &mut ops);
        let time = InputTime::from_micros(s.time_ns / 1000);
        let delivered = self.deliver(&ops, time, st);
        self.ops = ops;
        if delivered { Flow::Consumed } else { Flow::Continue }
    }

    /// Execute planned ops on the seat's `wl_pointer`; returns whether anything was sent.
    fn deliver(&mut self, ops: &[PtrOp], time: InputTime, st: &mut Zxr) -> bool {
        let mut delivered = false;
        for op in ops.iter() {
            delivered = true;
            match op {
                PtrOp::Move { member, local } => {
                    let serial = SERIAL_COUNTER.next_serial();
                    match super::seat::plane_point(st, *member, *local) {
                        Some((surface, logical, origin)) => {
                            // a locked pointer (pointer-constraints) keeps its position; relative motion still flows
                            let locked = self.pointer.current_focus().as_ref() == Some(&surface) && is_locked(&surface, &self.pointer);
                            if !locked {
                                self.pointer.motion(st, Some((surface.clone(), origin)), &MotionEvent { location: logical, serial, time });
                            }
                            self.last_member = Some(*member);
                            self.last_focus = Some((surface.clone(), origin));
                            st.pointer_focus = Some(surface);
                        }
                        None => {
                            self.pointer.motion(st, None, &MotionEvent { location: (0.0, 0.0).into(), serial, time });
                            self.last_member = None;
                            self.last_focus = None;
                            st.pointer_focus = None;
                        }
                    }
                }
                PtrOp::Leave => {
                    let serial = SERIAL_COUNTER.next_serial();
                    self.pointer.motion(st, None, &MotionEvent { location: (0.0, 0.0).into(), serial, time });
                    self.last_member = None;
                    self.last_focus = None;
                    st.pointer_focus = None;
                }
                PtrOp::Relative { dx, dy } => {
                    let delta = Point::<f64, smithay::utils::Logical>::from((*dx, *dy));
                    // flat profile: the unaccelerated vector is the vector
                    self.pointer.relative_motion(st, self.last_focus.clone(), &RelativeMotionEvent { delta, delta_unaccel: delta, time });
                }
                PtrOp::Button { code, pressed } => {
                    let serial = SERIAL_COUNTER.next_serial();
                    self.pointer.button(st, &ButtonEvent { serial, time, button: *code, state: if *pressed { ButtonState::Pressed } else { ButtonState::Released } });
                    if *pressed {
                        if let Some(m) = self.last_member {
                            // focus follows the commit, never hover (spatial-input §6)
                            super::seat::commit_focus(st, m);
                        }
                    }
                }
                PtrOp::Axis(frame) => self.pointer.axis(st, frame.clone()),
                PtrOp::Frame => self.pointer.frame(st),
            }
        }
        delivered
    }
}

/// Whether the focused surface holds an active `zwp_locked_pointer_v1` for this pointer.
fn is_locked(surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface, pointer: &PointerHandle<Zxr>) -> bool {
    use smithay::wayland::pointer_constraints::{with_pointer_constraint, PointerConstraint};
    with_pointer_constraint(surface, pointer, |c| c.map(|c| c.is_active() && matches!(&*c, PointerConstraint::Locked(_))).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Side;
    use crate::scene::{Flags, Scene, Shape};
    use crate::xr::math;

    fn members() -> (MemberId, MemberId) {
        let mut s: Scene<()> = Scene::new();
        let a = s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap();
        let b = s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap();
        (a, b)
    }

    #[test]
    fn button_codes_are_the_kernels() {
        assert_eq!(button_code(Button::Select), Some(0x110));
        assert_eq!(button_code(Button::Secondary), Some(0x111));
        assert_eq!(button_code(Button::Middle), Some(0x112));
        assert_eq!(button_code(Button::Code(0x113)), Some(0x113));
        assert_eq!(button_code(Button::Menu), None);
        assert_eq!(button_code(Button::System), None);
    }

    #[test]
    fn wheel_is_discrete_plus_value_and_finger_stops() {
        let t = InputTime::from_micros(1);
        let mut m = AxisMap::default();
        let f = m.frame((0.0, 1.0), AxisSource::Wheel, t).unwrap();
        assert_eq!(f.source, Some(SmAxisSource::Wheel));
        assert_eq!(f.v120, Some((0, 120)));
        assert_eq!(f.axis, (0.0, WHEEL_PX_PER_DETENT));
        assert_eq!(f.stop, (false, false));
        // a wheel returning 0 sends nothing (no stop for wheels)
        assert!(m.frame((0.0, 0.0), AxisSource::Wheel, t).is_none());
        // finger: value, then a stop when the axis returns to 0
        let f = m.frame((3.0, -2.0), AxisSource::Finger, t).unwrap();
        assert_eq!(f.source, Some(SmAxisSource::Finger));
        assert_eq!(f.axis, (3.0, -2.0));
        assert_eq!(f.v120, None);
        let f = m.frame((0.0, -1.0), AxisSource::Finger, t).unwrap();
        assert_eq!(f.stop, (true, false));
        assert_eq!(f.axis, (0.0, -1.0));
        let f = m.frame((0.0, 0.0), AxisSource::Finger, t).unwrap();
        assert_eq!(f.stop, (false, true));
        assert!(m.frame((0.0, 0.0), AxisSource::Finger, t).is_none());
        // continuous behaves as finger for the stop
        let _ = m.frame((0.0, 0.5), AxisSource::Continuous, t).unwrap();
        let f = m.frame((0.0, 0.0), AxisSource::Continuous, t).unwrap();
        assert_eq!(f.stop, (false, true));
    }

    #[test]
    fn gaze_releases_a_ray_owned_pointer_but_not_a_mouse() {
        let (a, _) = members();
        let mut l = PointerLogic::default();
        let mut out = Vec::new();
        // the head owns the pointer on plane a (the floor)
        assert!(l.owner.drives(SourceKind::Head));
        l.move_to(Some((a, [0.1, 0.0])), &mut out);
        assert!(l.plane.is_some());
        // no gaze: nothing happens
        assert!(!l.release_for_gaze(Some(SourceKind::Head), &mut out));
        assert!(out.is_empty() && l.plane.is_some());
        // gaze takes the tier: leave + frame, no plane, owner kept, counted once
        assert!(l.release_for_gaze(Some(SourceKind::Gaze), &mut out));
        assert_eq!(out, vec![PtrOp::Leave, PtrOp::Frame]);
        assert!(l.plane.is_none());
        assert_eq!(l.owner.owner(), Some(SourceKind::Head));
        assert_eq!(l.releases, 1);
        // already released: idempotent
        assert!(!l.release_for_gaze(Some(SourceKind::Gaze), &mut out));
        assert_eq!(l.releases, 1);
        // the head retakes the tier: its next sample re-enters
        let mut s = Sample::new(SourceKind::Head, 1).with_pose(math::pose_identity());
        s.tracked = true;
        let cx = Context { targeting: Some(SourceKind::Head), own: Some((a, [0.2, 0.0])), look: None, head: None, plane_half: None };
        l.plan(&s, &cx, &mut out);
        assert_eq!(out, vec![PtrOp::Move { member: a, local: [0.2, 0.0] }, PtrOp::Frame]);
        // a mouse-owned pointer is untouched under gaze
        let mut m = PointerLogic::default();
        assert!(m.owner.commit(SourceKind::Pointer));
        m.move_to(Some((a, [0.0, 0.0])), &mut out);
        assert!(!m.release_for_gaze(Some(SourceKind::Gaze), &mut out));
        assert!(m.plane.is_some() && out.is_empty());
        assert_eq!(m.releases, 0);
    }

    #[test]
    fn last_committer_owns_the_pointer() {
        let mut o = PointerOwner::default();
        let (l, r) = (SourceKind::Controller(Side::Left), SourceKind::Controller(Side::Right));
        // the first ray to arrive takes an unowned pointer
        assert!(o.drives(r));
        assert!(!o.drives(l), "the other's ray is inert until it commits");
        assert!(o.commit(l));
        assert!(o.drives(l) && !o.drives(r));
        assert!(!o.commit(l), "committing again changes nothing");
        // the mouse shares the same pointer and takes it on its own commit
        assert!(o.commit(SourceKind::Pointer));
        assert_eq!(o.owner(), Some(SourceKind::Pointer));
        assert_eq!(o.handoffs, 2);
    }

    #[test]
    fn rays_drive_only_when_owning_and_a_press_hands_off() {
        let (a, _) = members();
        let mut l = PointerLogic::default();
        let mut ops = Vec::new();
        let (lk, rk) = (SourceKind::Controller(Side::Left), SourceKind::Controller(Side::Right));
        let cx_r = Context { own: Some((a, [0.1, 0.0])), ..Default::default() };
        let cx_l = Context { own: Some((a, [-0.1, 0.0])), ..Default::default() };
        l.plan(&Sample::new(rk, 1).with_pose(math::pose_identity()), &cx_r, &mut ops);
        assert_eq!(ops, vec![PtrOp::Move { member: a, local: [0.1, 0.0] }, PtrOp::Frame]);
        // the left ray is drawn but drives nothing
        l.plan(&Sample::new(lk, 2).with_pose(math::pose_identity()), &cx_l, &mut ops);
        assert!(ops.is_empty());
        // its press: moves the pointer to its hit, then the button
        l.plan(&Sample::new(lk, 3).with_pose(math::pose_identity()).with_button(Button::Select, true), &cx_l, &mut ops);
        assert_eq!(ops, vec![PtrOp::Move { member: a, local: [-0.1, 0.0] }, PtrOp::Frame, PtrOp::Button { code: BTN_LEFT, pressed: true }, PtrOp::Frame]);
        // now the right ray is inert
        l.plan(&Sample::new(rk, 4).with_pose(math::pose_identity()), &cx_r, &mut ops);
        assert!(ops.is_empty());
        // the owner's ray leaving every plane is a leave, once
        let cx_none = Context::default();
        l.plan(&Sample::new(lk, 5).with_pose(math::pose_identity()), &cx_none, &mut ops);
        assert_eq!(ops, vec![PtrOp::Leave, PtrOp::Frame]);
        l.plan(&Sample::new(lk, 6).with_pose(math::pose_identity()), &cx_none, &mut ops);
        assert!(ops.is_empty());
    }

    #[test]
    fn gaze_scroll_is_exactly_enter_axis_leave() {
        let (a, _) = members();
        let mut l = PointerLogic::default();
        let mut ops = Vec::new();
        let cx = Context { targeting: Some(SourceKind::Gaze), look: Some((a, [0.2, 0.1])), ..Default::default() };
        let mut s = Sample::new(SourceKind::Controller(Side::Right), 1).with_pose(math::pose_identity());
        s.axis = Some((0.0, -1.0));
        s.axis_source = Some(AxisSource::Continuous);
        l.plan(&s, &cx, &mut ops);
        let kinds: Vec<&str> = ops
            .iter()
            .map(|o| match o {
                PtrOp::Move { .. } => "enter",
                PtrOp::Axis(_) => "axis",
                PtrOp::Frame => "frame",
                PtrOp::Leave => "leave",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["enter", "axis", "frame", "leave"]);
        assert_eq!(ops[0], PtrOp::Move { member: a, local: [0.2, 0.1] });
        assert_eq!(l.gaze_scrolls, 1);
        assert!(l.plane.is_none() || !l.on_surface);
        // a mouse wheel under gaze targeting takes the same path
        let mut w = Sample::new(SourceKind::Pointer, 2);
        w.axis = Some((0.0, 1.0));
        w.axis_source = Some(AxisSource::Wheel);
        l.plan(&w, &cx, &mut ops);
        assert_eq!(ops.len(), 4);
        assert_eq!(l.gaze_scrolls, 2);
        // without a gaze hit, nothing is disclosed
        let cx_none = Context { targeting: Some(SourceKind::Gaze), ..Default::default() };
        l.plan(&s, &cx_none, &mut ops);
        assert!(ops.is_empty());
    }

    #[test]
    fn mouse_moves_plane_local_warps_on_look_change_and_leaves_bounds() {
        let (a, b) = members();
        let mut l = PointerLogic::default();
        let mut ops = Vec::new();
        let mut mv = Sample::new(SourceKind::Pointer, 1);
        mv.delta = Some((10.0, 5.0));
        // no plane yet: lands where the head ray hits, then moves by the delta (y down → y up)
        let cx = Context { head: Some((a, [0.0, 0.0])), look: Some((a, [0.0, 0.0])), plane_half: None, ..Default::default() };
        l.plan(&mv, &cx, &mut ops);
        let expect = [10.0 * M_PER_PX, -5.0 * M_PER_PX];
        assert_eq!(ops, vec![PtrOp::Move { member: a, local: expect }, PtrOp::Relative { dx: 10.0, dy: 5.0 }, PtrOp::Frame]);
        assert_eq!(l.owner.owner(), Some(SourceKind::Pointer));
        // the look moved to `b`: the next motion warps there
        let cx_b = Context { head: Some((b, [0.3, 0.3])), look: Some((b, [0.3, 0.3])), plane_half: Some([0.5, 0.5]), ..Default::default() };
        l.plan(&mv, &cx_b, &mut ops);
        match ops[0] {
            PtrOp::Move { member, local } => {
                assert_eq!(member, b);
                assert!((local[0] - (0.3 + 10.0 * M_PER_PX)).abs() < 1e-6);
            }
            ref o => panic!("{o:?}"),
        }
        assert_eq!(l.warps, 1);
        // leaving the bounds: the pointer leaves the plane (angular ray until it lands)
        let mut far = Sample::new(SourceKind::Pointer, 2);
        far.delta = Some((1000.0, 0.0));
        let cx_same = Context { head: Some((b, [0.3, 0.3])), look: Some((b, [0.3, 0.3])), plane_half: Some([0.5, 0.5]), ..Default::default() };
        l.plan(&far, &cx_same, &mut ops);
        assert_eq!(ops[0], PtrOp::Leave);
        assert!(l.plane.is_none());
        // the next motion lands at the head hit again
        l.plan(&mv, &cx_same, &mut ops);
        assert!(matches!(ops[0], PtrOp::Move { member, .. } if member == b));
    }

    #[test]
    fn mouse_buttons_and_wheel_map_through() {
        let mut l = PointerLogic::default();
        let mut ops = Vec::new();
        let s = Sample::new(SourceKind::Pointer, 1).with_button(Button::Secondary, true);
        l.plan(&s, &Context::default(), &mut ops);
        assert_eq!(ops, vec![PtrOp::Button { code: BTN_RIGHT, pressed: true }, PtrOp::Frame]);
        let mut w = Sample::new(SourceKind::Pointer, 2);
        w.axis = Some((0.0, -2.0));
        w.axis_source = Some(AxisSource::Wheel);
        l.plan(&w, &Context::default(), &mut ops);
        assert!(matches!(&ops[0], PtrOp::Axis(f) if f.v120 == Some((0, -240))));
    }
}
