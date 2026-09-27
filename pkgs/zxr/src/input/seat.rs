//! The `Seat` slot (spatial-input §1a: "seat — `wl_touch` / `wl_pointer` / `wl_keyboard`
//! emission; xdg-activation; cursors (§7)"): the last stage. It dispatches each sample to one of
//! the two transports by class (§5, ruled; ADR 0013 amendment item 2), keys to the seat's
//! keyboard, and keeps the per-tick presentation state — cursors (§7) and touch-class emphasis
//! (§4) — that the frame procedure reads.
//!
//! **Routing** ([`route`]): a key → the keyboard; a mouse/trackpad sample → the pointer transport
//! (mice are pointer-class whatever targets, §5 :258); an axis while gaze targets → the pointer
//! transport's gaze-scroll exception (§5 :273-278); otherwise by the tier's class — the
//! *targeting* kind's, falling back to the sample kind's own when the Tier stage has not run.
//! **Which hit** ([`hits_for`]): a hand commits at its own aim hit (each hand a contact, §5
//! :251-253); any other committer under a targeting source commits *at the target* ("whatever
//! commits … commits at the gaze target", §3 tier 1) — the targeting kind's hit, falling back to
//! its own.
//!
//! **The R0 floor** stays until the Hit stage produces its first hit: a `Head` sample with no hit
//! stage behind it drives the pointer through `Zxr::update_gaze_pointer` exactly as
//! `HeadFloor` did (the spine's stand-in), so behaviour is unchanged while lanes A/B land.
//!
//! **Protocols served here** (spec §8 "Protocols served for input"): `relative-pointer`,
//! `pointer-constraints`, `pointer-gestures` are created by this stage because `Zxr::new` does
//! not serve them yet — a placement the report flags with the patch to move them there.
//! `cursor-shape-v1` needs `TabletSeatHandler for Zxr` (smithay `cursor_shape.rs:252-258`),
//! an `impl … for Zxr` this lane may not add: patch in the report.
//!
//! Budget: one array scan over ≤ 8 hits per sample; no allocation per sample in steady state.

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point};
use smithay::wayland::pointer_constraints::PointerConstraintsState;
use smithay::wayland::pointer_gestures::PointerGesturesState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;

use super::cursor::{Cursors, PointerPoint};
use super::emphasis::Emphasis;
use super::pointer::PointerTransport;
use super::touch::TouchTransport;
use super::{Class, Flow, Hit, Sample, Selection, SourceKind, Stage};
use crate::scene::{self, MemberId, Shape};
use crate::state::Zxr;

/// Where the seat stage sends a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Keyboard,
    Touch,
    Pointer,
    /// the R0 head-ray pointer, while no Hit stage has produced a hit
    HeadFloor,
}

/// The routing rule (see the module doc).
pub fn route(s: &Sample, tier: Option<Selection>, hits_seen: bool) -> Route {
    if s.key.is_some() || s.kind == SourceKind::Keyboard {
        return Route::Keyboard;
    }
    if s.kind == SourceKind::Pointer {
        return Route::Pointer;
    }
    if s.axis.is_some() && tier.map(|t| t.targeting) == Some(SourceKind::Gaze) {
        return Route::Pointer;
    }
    if s.kind == SourceKind::Head && !hits_seen && tier.is_none() {
        return Route::HeadFloor;
    }
    match tier.map(|t| t.class).unwrap_or_else(|| s.kind.class()) {
        Class::Touch => Route::Touch,
        Class::Pointer => Route::Pointer,
    }
}

/// This tick's hits relevant to a sample: (its own kind's, the target's, the head's). The
/// target is the targeting kind's hit — gaze, degrading to the head ray's (§8 warp rule).
pub fn hits_for<'a>(hits: &'a [Hit], s: &Sample, tier: Option<Selection>) -> (Option<&'a Hit>, Option<&'a Hit>, Option<&'a Hit>) {
    let of = |k: SourceKind| hits.iter().find(|h| h.kind == k);
    let own = of(s.kind);
    let head = of(SourceKind::Head);
    let look = tier.and_then(|t| of(t.targeting)).or(head);
    (own, look, head)
}

/// The hit a touch-class commit lands on: a hand's own aim hit; any other committer's the
/// target's (falling back to its own when the target has none).
pub fn commit_hit<'a>(s: &Sample, tier: Option<Selection>, own: Option<&'a Hit>, look: Option<&'a Hit>) -> Option<&'a Hit> {
    let is_target = tier.map(|t| t.targeting == s.kind).unwrap_or(true);
    if matches!(s.kind, SourceKind::Hand(_)) || is_target { own } else { look.or(own) }
}

/// A plane-local point → the surface under it, the point in the plane's logical space and the
/// surface's origin in that space (smithay's `(focus, origin)` + `location` convention,
/// `references/smithay/src/input/pointer/mod.rs:800-805`).
pub fn plane_point(st: &Zxr, member: MemberId, local: [f32; 2]) -> Option<(WlSurface, Point<f64, Logical>, Point<f64, Logical>)> {
    let (_, surface, surface_local) = st.hit_surface_at(member, local)?;
    let m = st.scene.get(member)?;
    let Shape::Plane { size } = m.shape else { return None };
    let g = m.m.window.geometry();
    let (x, y) = scene::local_to_logical(local, size, (g.loc.x, g.loc.y, g.size.w, g.size.h));
    let logical = Point::<f64, Logical>::from((x, y));
    Some((surface, logical, logical - surface_local))
}

/// Focus follows the commit, never hover (spatial-input §6, ruled): a `down` or `button` press
/// on a member makes it the keyboard focus and raises it; `focus_window` counts the change.
pub fn commit_focus(st: &mut Zxr, member: MemberId) {
    // the focus module (spatial-input §6): kb focus + Activated, the focus stack, the commit
    // serial the activation rule compares against; `focus_window` inside counts the change
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    crate::input::focus::commit_focus(st, member, serial);
}

/// The stage in the `Seat` slot.
pub struct SeatStage {
    pub touch: TouchTransport,
    pub pointer: PointerTransport,
    pub cursors: Cursors,
    pub emphasis: Emphasis,
    _relative_pointer: RelativePointerManagerState,
    _constraints: PointerConstraintsState,
    _gestures: PointerGesturesState,
    /// the Hit stage has produced a hit at least once: the R0 floor is retired
    hits_seen: bool,
    pub keys: u64,
}

impl SeatStage {
    pub fn new(st: &mut Zxr) -> SeatStage {
        let dh = st.dh.clone();
        let stage = SeatStage {
            touch: TouchTransport::new(st),
            pointer: PointerTransport::new(st),
            cursors: Cursors::default(),
            emphasis: Emphasis::new(),
            _relative_pointer: RelativePointerManagerState::new::<Zxr>(&dh),
            _constraints: PointerConstraintsState::new::<Zxr>(&dh),
            _gestures: PointerGesturesState::new::<Zxr>(&dh),
            hits_seen: false,
            keys: 0,
        };
        tracing::info!("seat stage: wl_touch + wl_pointer transports; relative-pointer, pointer-constraints, pointer-gestures served");
        stage
    }

    /// Per-tick presentation state from the tier and this tick's hits: the targeting ray's hit
    /// (hand, head, controller — never gaze, §7) and the logical pointer's plane point, which
    /// `Cursors::layer` resolves to the one cursor element; and the touch-class emphasis target (§4).
    fn present(&mut self, st: &mut Zxr) {
        let tier = st.input.tier;
        // the ray whose reticle is drawn: the tier's targeting kind; before the Tier stage lands,
        // the pointer's owning ray (a controller or the head) — a stand-in that retires itself
        let targeting = tier.map(|t| t.targeting).or_else(|| self.pointer.logic.owner.owner().filter(|k| matches!(k, SourceKind::Controller(_) | SourceKind::Head | SourceKind::Hand(_))));
        let hit = targeting.and_then(|k| st.input.hits.iter().find(|h| h.kind == k).copied());
        match (targeting, hit) {
            (Some(SourceKind::Gaze), _) | (None, _) | (_, None) => self.cursors.clear_reticle(),
            (Some(_), Some(h)) => match st.scene.world_pose(h.member) {
                Some(world) => self.cursors.set_reticle(world, h.local, h.distance),
                None => self.cursors.clear_reticle(),
            },
        }
        // the logical pointer on its plane: the point, its distance from the head (the layer's
        // visual-angle scale), and the owning kind (a ray owner keeps its ring around the image)
        let pointer = self.pointer.logic.plane.and_then(|(member, local)| {
            let plane_world = st.scene.world_pose(member)?;
            let p = crate::xr::math::pose_apply(plane_world, [local[0], local[1], 0.0]);
            let distance = st.input.head.map(|h| {
                let d = [p[0] - h.position.x, p[1] - h.position.y, p[2] - h.position.z];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            }).unwrap_or(1.0);
            Some(PointerPoint { plane_world, local, distance, owner: self.pointer.logic.owner.owner() })
        });
        self.cursors.set_pointer(pointer);
        self.cursors.set_targeting(targeting);
        let touch_target = match tier {
            Some(t) if t.class == Class::Touch => hit.map(|h| h.member),
            _ => None,
        };
        self.emphasis.set_target(touch_target);
    }
}

impl Stage for SeatStage {
    fn name(&self) -> &'static str {
        "seat"
    }

    fn run(&mut self, s: &mut Sample, st: &mut Zxr) -> Flow {
        if !st.input.hits.is_empty() {
            self.hits_seen = true;
        }
        let tier = st.input.tier;
        match route(s, tier, self.hits_seen) {
            Route::Keyboard => {
                let Some((code, pressed)) = s.key else { return Flow::Continue };
                // the seat's keyboard focus (§6, §8): `KeyboardHandle::input`, as `Zxr::send_key`
                st.send_key(code, pressed);
                self.cursors.on_key();
                self.keys += 1;
                Flow::Consumed
            }
            Route::HeadFloor => {
                if let Some(pose) = s.pose {
                    st.update_gaze_pointer(pose);
                }
                Flow::Consumed
            }
            Route::Touch => {
                let hits = std::mem::take(&mut st.input.hits);
                let (own, look, _) = hits_for(&hits, s, tier);
                let hit = commit_hit(s, tier, own, look);
                let flow = self.touch.run(s, hit, st);
                st.input.hits = hits;
                flow
            }
            Route::Pointer => {
                let hits = std::mem::take(&mut st.input.hits);
                let (own, look, head) = hits_for(&hits, s, tier);
                let flow = self.pointer.run(s, own, look, head, tier.map(|t| t.targeting), st);
                st.input.hits = hits;
                if flow == Flow::Consumed && (s.delta.is_some() || s.pose.is_some()) {
                    self.cursors.on_motion();
                }
                flow
            }
        }
    }

    fn tick(&mut self, st: &mut Zxr, now_ns: u64) {
        if let Some(c) = st.input.cursor_image.take() {
            self.cursors.set_client_cursor(c);
        }
        self.present(st);
        self.emphasis.tick(now_ns);
        // publish the one cursor layer for the frame procedure (main.rs steps 4–6) and the journal
        st.input.cursor_layer = self.cursors.layer();
        st.input.cursor_inputs = Some(self.cursors.inputs());
        st.input.emphasis = self.emphasis.target().map(|m| (m, self.emphasis.emphasis_of(m)));
        st.journal.input_touch_downs = self.touch.logic.downs;
        st.journal.input_touch_cancels = self.touch.logic.cancels;
        st.journal.input_gaze_scrolls = self.pointer.logic.gaze_scrolls;
        st.journal.input_pointer_handoffs = self.pointer.logic.owner.handoffs;
        st.journal.input_pointer_warps = self.pointer.logic.warps;
        st.journal.input_cursor_named_ticks = st.input.cursor_named_ticks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{AxisSource, Button, Side};
    use crate::scene::{Flags, Scene};
    use crate::xr::math;

    fn sel(k: SourceKind, class: Class) -> Option<Selection> {
        Some(Selection { targeting: k, class, direct: false, changed_at_ns: 0 })
    }

    fn members() -> (MemberId, MemberId) {
        let mut s: Scene<()> = Scene::new();
        let a = s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap();
        let b = s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap();
        (a, b)
    }

    #[test]
    fn routing_by_class_with_the_mouse_and_gaze_scroll_exceptions() {
        let hand = Sample::new(SourceKind::Hand(Side::Right), 1).with_pose(math::pose_identity());
        let ctrl = Sample::new(SourceKind::Controller(Side::Right), 1).with_pose(math::pose_identity());
        let head = Sample::new(SourceKind::Head, 1).with_pose(math::pose_identity());
        let mut key = Sample::new(SourceKind::Keyboard, 1);
        key.key = Some((30, true));
        let mut mouse = Sample::new(SourceKind::Pointer, 1);
        mouse.delta = Some((1.0, 0.0));
        // no tier: the kind's own class; the head is the R0 floor until a hit has been seen
        assert_eq!(route(&hand, None, false), Route::Touch);
        assert_eq!(route(&ctrl, None, false), Route::Pointer);
        assert_eq!(route(&head, None, false), Route::HeadFloor);
        assert_eq!(route(&head, None, true), Route::Pointer);
        assert_eq!(route(&key, None, false), Route::Keyboard);
        assert_eq!(route(&mouse, None, false), Route::Pointer);
        // gaze targets (touch-class): a controller press commits as touch; the mouse stays pointer
        let gaze = sel(SourceKind::Gaze, Class::Touch);
        assert_eq!(route(&ctrl.with_button(Button::Select, true), gaze, true), Route::Touch);
        assert_eq!(route(&mouse, gaze, true), Route::Pointer);
        assert_eq!(route(&head, gaze, true), Route::Touch);
        // …but a stick under gaze is the scroll exception → pointer
        let mut stick = ctrl;
        stick.axis = Some((0.0, 1.0));
        stick.axis_source = Some(AxisSource::Continuous);
        assert_eq!(route(&stick, gaze, true), Route::Pointer);
        assert_eq!(route(&stick, sel(SourceKind::Controller(Side::Right), Class::Pointer), true), Route::Pointer);
        assert_eq!(route(&stick, sel(SourceKind::Hand(Side::Left), Class::Touch), true), Route::Touch);
        // controller targets: everything XR is pointer-class
        let c = sel(SourceKind::Controller(Side::Left), Class::Pointer);
        assert_eq!(route(&hand, c, true), Route::Pointer);
        assert_eq!(route(&head, c, true), Route::Pointer);
    }

    #[test]
    fn a_hand_commits_at_its_own_hit_and_another_committer_at_the_target() {
        let (a, b) = members();
        let hits = vec![
            Hit { kind: SourceKind::Gaze, member: a, local: [0.1, 0.1], distance: 1.5, time_ns: 1 },
            Hit { kind: SourceKind::Hand(Side::Right), member: b, local: [0.2, 0.2], distance: 1.0, time_ns: 1 },
            Hit { kind: SourceKind::Head, member: b, local: [0.3, 0.3], distance: 1.2, time_ns: 1 },
        ];
        let gaze = sel(SourceKind::Gaze, Class::Touch);
        let hand = Sample::new(SourceKind::Hand(Side::Right), 1).with_pose(math::pose_identity());
        let (own, look, head) = hits_for(&hits, &hand, gaze);
        assert_eq!(own.map(|h| h.member), Some(b));
        assert_eq!(look.map(|h| h.member), Some(a));
        assert_eq!(head.map(|h| h.member), Some(b));
        assert_eq!(commit_hit(&hand, gaze, own, look).map(|h| h.member), Some(b), "each hand a contact at its own aim");
        let ctrl = Sample::new(SourceKind::Controller(Side::Left), 1).with_button(Button::Select, true);
        let (own, look, _) = hits_for(&hits, &ctrl, gaze);
        assert!(own.is_none());
        assert_eq!(commit_hit(&ctrl, gaze, own, look).map(|h| h.member), Some(a), "a trigger commits at the gaze target");
        // gaze lost, head targets: the look degrades to the head hit
        let head_tier = sel(SourceKind::Head, Class::Touch);
        let (_, look, _) = hits_for(&hits, &ctrl, head_tier);
        assert_eq!(look.map(|h| h.local), Some([0.3, 0.3]));
        // no tier: own hit only
        let (own, look, _) = hits_for(&hits, &hand, None);
        assert_eq!(commit_hit(&hand, None, own, look).map(|h| h.member), Some(b));
        let ctrl_hit = hits.iter().find(|h| h.kind == SourceKind::Controller(Side::Left));
        assert!(ctrl_hit.is_none());
    }
}
