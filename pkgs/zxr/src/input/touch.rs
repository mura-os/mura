//! The touch-class transport (spatial-input §5 "Touch-class — hands and gaze", ruled; ADR 0013
//! amendment item 2; research/63 §5 "Either hand, touch semantics").
//!
//! "The seat's `wl_touch`. A commit is `down(id, surface, x, y)` at the plane-local hit; holding
//! is `motion`; release is `up`; `frame` groups them. Each hand is a contact id, so two hands
//! are two contacts and toolkits' native two-finger zoom/rotate apply … The client never sees
//! a position before `down` — gaze privacy is a property of the transport (§9)."
//! (spatial-input.md:250-256). Scroll is drag or flick and pinch-and-hold is the toolkit's
//! long-press: nothing to do here beyond `motion` while down.
//!
//! Ported shape: smithay's `TouchHandle::{down, up, motion, frame, cancel}`
//! (`references/smithay/src/input/touch/mod.rs:334-403`); the focus tuple is the surface and
//! its origin in the plane's logical space, the location the plane-logical point, exactly as
//! smithay's pointer path (`input/pointer/mod.rs:800-805` subtracts the origin).
//!
//! **Contact ids:** `Hand(Left)` = 0, `Hand(Right)` = 1, any other committer (gaze or head
//! targeting with a controller trigger, `hmdButtons.select`, dwell) = 2 — "whatever commits
//! commits at the gaze target" (spatial-input §3 tier 1) is one contact. Fixed by this lane's
//! brief; flagged in the report as a judgment (no comparable numbers its contacts).
//!
//! **Release after tracking loss → `wl_touch.cancel`** (`touch/mod.rs:392-403`: "Use in case you
//! decide the touch stream is a global gesture … no further events will be sent until a new
//! touch point appears"): a sample for a down contact whose `tracked`/`ready` dropped cancels
//! the session rather than synthesising an `up` at a position the hand never reached.
//!
//! **Pinch stand-ins (flagged):** a commit edge is the sample's `Button::Select` edge when it has
//! one, else `values.pinch` crossing **≥ 0.75** (close) / **≤ 0.25** (open) — MRTK3's polyfill
//! thresholds as spatial-input §10 lists them, inverted for `pinch_ext/value` (1 = closed).
//!
//! Budget: a three-slot contact table, an eight-bool pinch latch, and a reused op buffer; no
//! allocation per sample in steady state; no thread.

use smithay::backend::input::{InputTime, TouchSlot};
use smithay::input::touch::{DownEvent, MotionEvent, TouchHandle, UpEvent};
use smithay::utils::SERIAL_COUNTER;

use super::{Button, Flow, Hit, Sample, Side, SourceKind};
use crate::scene::MemberId;
use crate::state::Zxr;

/// The contact id of commits that are not a hand's own (gaze / head targeting, any committer).
pub const CONTACT_COMMIT: u32 = 2;
pub const CONTACT_COUNT: usize = 3;

/// `pinch_ext/value` at or above which the pinch is a commit (stand-in, MRTK3 — flagged).
pub const PINCH_CLOSE: f32 = 0.75;
/// `pinch_ext/value` at or below which a closed pinch releases (stand-in, MRTK3 — flagged).
pub const PINCH_OPEN: f32 = 0.25;

/// One contact id per hand; everything else commits as the targeting source's contact.
pub fn contact_id(kind: SourceKind) -> u32 {
    match kind {
        SourceKind::Hand(Side::Left) => 0,
        SourceKind::Hand(Side::Right) => 1,
        _ => CONTACT_COMMIT,
    }
}

fn kind_index(kind: SourceKind) -> usize {
    SourceKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
}

/// A contact that is down: which kind put it down, on which member, and where it last was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub kind: SourceKind,
    pub member: MemberId,
    pub local: [f32; 2],
}

/// The pure contact table: down contacts by id and the per-kind pinch latch.
#[derive(Debug, Default)]
pub struct ContactIds {
    slots: [Option<Contact>; CONTACT_COUNT],
    pinch_closed: [bool; 8],
}

impl ContactIds {
    pub fn is_down(&self, id: u32) -> bool {
        self.slots.get(id as usize).map(|s| s.is_some()).unwrap_or(false)
    }

    pub fn contact(&self, id: u32) -> Option<Contact> {
        self.slots.get(id as usize).copied().flatten()
    }

    pub fn down_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    fn set(&mut self, id: u32, c: Option<Contact>) {
        if let Some(s) = self.slots.get_mut(id as usize) {
            *s = c;
        }
    }

    fn clear(&mut self) {
        self.slots = [None; CONTACT_COUNT];
    }

    /// The commit edge a sample carries: `Some(true)` = commit, `Some(false)` = release. The
    /// `Select` button edge wins when present; else the pinch value's hysteresis.
    pub fn commit_edge(&mut self, s: &Sample) -> Option<bool> {
        if let Some((Button::Select, pressed)) = s.button {
            return Some(pressed);
        }
        if s.button.is_some() {
            return None;
        }
        let i = kind_index(s.kind);
        let closed = self.pinch_closed[i];
        if !closed && s.values.pinch >= PINCH_CLOSE {
            self.pinch_closed[i] = true;
            return Some(true);
        }
        if closed && s.values.pinch <= PINCH_OPEN {
            self.pinch_closed[i] = false;
            return Some(false);
        }
        None
    }
}

/// What the transport will send, planned by the pure logic and executed by the adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchOp {
    Down { id: u32, member: MemberId, local: [f32; 2] },
    Motion { id: u32, member: MemberId, local: [f32; 2] },
    Up { id: u32 },
    /// `wl_touch.cancel`: the whole session (the protocol's shape)
    Cancel,
    Frame,
}

/// The pure per-sample planner over the contact table.
#[derive(Debug, Default)]
pub struct TouchLogic {
    pub contacts: ContactIds,
    pub downs: u64,
    pub cancels: u64,
}

impl TouchLogic {
    /// Plan the ops for one sample given the hit to commit or drag at (already resolved to a
    /// point a surface exists under — a hit with no surface is no hit).
    pub fn plan(&mut self, s: &Sample, hit: Option<(MemberId, [f32; 2])>, out: &mut Vec<TouchOp>) {
        out.clear();
        let id = contact_id(s.kind);
        match self.contacts.contact(id) {
            Some(c) if c.kind == s.kind => {
                // the contact this kind holds: loss cancels, a release edge lifts, else drag
                if !s.tracked || !s.ready {
                    out.push(TouchOp::Cancel);
                    self.contacts.clear();
                    self.cancels += 1;
                    return;
                }
                match self.contacts.commit_edge(s) {
                    Some(false) => {
                        out.push(TouchOp::Up { id });
                        out.push(TouchOp::Frame);
                        self.contacts.set(id, None);
                    }
                    _ => {
                        if let Some((member, local)) = hit {
                            if member != c.member || local != c.local {
                                out.push(TouchOp::Motion { id, member, local });
                                out.push(TouchOp::Frame);
                                self.contacts.set(id, Some(Contact { kind: s.kind, member, local }));
                            }
                        }
                    }
                }
            }
            Some(_) => {
                // another kind holds this id (a second committer at the gaze target): its edges
                // still advance the pinch latch so a later release is not misread as a commit
                let _ = self.contacts.commit_edge(s);
            }
            None => {
                // nothing before `down` (spatial-input §5/§9): only a commit edge with a hit
                if self.contacts.commit_edge(s) == Some(true) {
                    if let Some((member, local)) = hit {
                        if s.tracked && s.ready {
                            out.push(TouchOp::Down { id, member, local });
                            out.push(TouchOp::Frame);
                            self.contacts.set(id, Some(Contact { kind: s.kind, member, local }));
                            self.downs += 1;
                        }
                    }
                }
            }
        }
    }
}

/// The smithay adapter: executes planned ops on the seat's `wl_touch`.
pub struct TouchTransport {
    touch: TouchHandle<Zxr>,
    pub logic: TouchLogic,
    ops: Vec<TouchOp>,
}

impl TouchTransport {
    /// Adds the touch capability to the seat if `Zxr::new` did not (`Seat::add_touch`,
    /// `references/smithay/src/input/mod.rs:692-694`; clients are told through `wl_seat.capabilities`).
    pub fn new(st: &mut Zxr) -> TouchTransport {
        let touch = match st.seat.get_touch() {
            Some(t) => t,
            None => st.seat.add_touch(),
        };
        TouchTransport { touch, logic: TouchLogic::default(), ops: Vec::with_capacity(4) }
    }

    /// Deliver one touch-class sample. `hit` is this sample's plane hit (the Hit stage's); the
    /// surface under it is resolved here so the planner only sees commit-able points.
    pub fn run(&mut self, s: &Sample, hit: Option<&Hit>, st: &mut Zxr) -> Flow {
        // one surface-tree hit per sample: the planner sees the point only if a surface is under it
        let resolved = hit.and_then(|h| super::seat::plane_point(st, h.member, h.local).map(|p| (h.member, h.local, p)));
        let mut ops = std::mem::take(&mut self.ops);
        self.logic.plan(s, resolved.as_ref().map(|(m, l, _)| (*m, *l)), &mut ops);
        let time = InputTime::from_micros(s.time_ns / 1000);
        let mut delivered = false;
        for op in ops.iter() {
            delivered = true;
            match *op {
                TouchOp::Down { id, member, .. } => {
                    let Some((_, _, (surface, logical, origin))) = resolved.as_ref() else { continue };
                    let serial = SERIAL_COUNTER.next_serial();
                    self.touch.down(st, Some((surface.clone(), *origin)), &DownEvent { slot: TouchSlot::from(Some(id)), location: *logical, serial, time });
                    // focus follows the commit, never hover (spatial-input §6)
                    super::seat::commit_focus(st, member);
                }
                TouchOp::Motion { id, .. } => {
                    let Some((_, _, (surface, logical, origin))) = resolved.as_ref() else { continue };
                    self.touch.motion(st, Some((surface.clone(), *origin)), &MotionEvent { slot: TouchSlot::from(Some(id)), location: *logical, time });
                }
                TouchOp::Up { id } => {
                    let serial = SERIAL_COUNTER.next_serial();
                    self.touch.up(st, &UpEvent { slot: TouchSlot::from(Some(id)), serial, time });
                }
                TouchOp::Cancel => {
                    self.touch.cancel(st);
                    tracing::info!(kind = ?s.kind, "wl_touch.cancel: release after tracking loss");
                }
                TouchOp::Frame => self.touch.frame(st),
            }
        }
        self.ops = ops;
        if delivered { Flow::Consumed } else { Flow::Continue }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Flags, Scene, Shape};
    use crate::xr::math;

    fn member() -> MemberId {
        let mut s: Scene<()> = Scene::new();
        s.add(s.default_place, math::pose_identity(), Shape::Plane { size: [1.0, 1.0] }, Flags::WINDOW, ()).unwrap()
    }

    fn hand(side: Side, pinch: f32) -> Sample {
        let mut s = Sample::new(SourceKind::Hand(side), 1).with_pose(math::pose_identity());
        s.values.pinch = pinch;
        s
    }

    #[test]
    fn one_contact_id_per_hand_and_one_for_every_other_committer() {
        assert_eq!(contact_id(SourceKind::Hand(Side::Left)), 0);
        assert_eq!(contact_id(SourceKind::Hand(Side::Right)), 1);
        for k in [SourceKind::Gaze, SourceKind::Head, SourceKind::Controller(Side::Left), SourceKind::Controller(Side::Right), SourceKind::Pointer] {
            assert_eq!(contact_id(k), CONTACT_COMMIT);
        }
    }

    #[test]
    fn down_only_with_a_hit_and_only_on_the_commit_edge() {
        let m = member();
        let mut l = TouchLogic::default();
        let mut ops = Vec::new();
        // hover: a tracked hand with an open pinch over a plane sends nothing
        l.plan(&hand(Side::Right, 0.0), Some((m, [0.0, 0.0])), &mut ops);
        assert!(ops.is_empty(), "nothing before down: {ops:?}");
        // a pinch in the air: nothing, but the latch closes
        l.plan(&hand(Side::Right, 0.9), None, &mut ops);
        assert!(ops.is_empty());
        // its release: still nothing (no contact was down)
        l.plan(&hand(Side::Right, 0.1), Some((m, [0.0, 0.0])), &mut ops);
        assert!(ops.is_empty());
        // the commit over the plane
        l.plan(&hand(Side::Right, 0.9), Some((m, [0.1, 0.2])), &mut ops);
        assert_eq!(ops, vec![TouchOp::Down { id: 1, member: m, local: [0.1, 0.2] }, TouchOp::Frame]);
        assert!(l.contacts.is_down(1) && !l.contacts.is_down(0));
        // holding: a drag is motion; the same point is nothing
        l.plan(&hand(Side::Right, 0.9), Some((m, [0.1, 0.2])), &mut ops);
        assert!(ops.is_empty());
        l.plan(&hand(Side::Right, 0.8), Some((m, [0.3, 0.2])), &mut ops);
        assert_eq!(ops, vec![TouchOp::Motion { id: 1, member: m, local: [0.3, 0.2] }, TouchOp::Frame]);
        // between the thresholds nothing changes (hysteresis)
        l.plan(&hand(Side::Right, 0.5), Some((m, [0.3, 0.2])), &mut ops);
        assert!(ops.is_empty());
        // release
        l.plan(&hand(Side::Right, 0.1), Some((m, [0.3, 0.2])), &mut ops);
        assert_eq!(ops, vec![TouchOp::Up { id: 1 }, TouchOp::Frame]);
        assert_eq!(l.contacts.down_count(), 0);
    }

    #[test]
    fn two_hands_are_two_contacts() {
        let m = member();
        let mut l = TouchLogic::default();
        let mut ops = Vec::new();
        l.plan(&hand(Side::Left, 0.9), Some((m, [-0.1, 0.0])), &mut ops);
        assert_eq!(ops[0], TouchOp::Down { id: 0, member: m, local: [-0.1, 0.0] });
        l.plan(&hand(Side::Right, 0.9), Some((m, [0.1, 0.0])), &mut ops);
        assert_eq!(ops[0], TouchOp::Down { id: 1, member: m, local: [0.1, 0.0] });
        assert_eq!(l.contacts.down_count(), 2);
    }

    #[test]
    fn select_button_edge_wins_over_the_pinch_value() {
        let m = member();
        let mut l = TouchLogic::default();
        let mut ops = Vec::new();
        // a controller trigger committing at the gaze target: contact 2
        let press = Sample::new(SourceKind::Controller(Side::Right), 1).with_pose(math::pose_identity()).with_button(Button::Select, true);
        l.plan(&press, Some((m, [0.0, 0.0])), &mut ops);
        assert_eq!(ops[0], TouchOp::Down { id: CONTACT_COMMIT, member: m, local: [0.0, 0.0] });
        let release = Sample::new(SourceKind::Controller(Side::Right), 2).with_pose(math::pose_identity()).with_button(Button::Select, false);
        l.plan(&release, Some((m, [0.0, 0.0])), &mut ops);
        assert_eq!(ops, vec![TouchOp::Up { id: CONTACT_COMMIT }, TouchOp::Frame]);
        // a non-select button is not a commit
        let menu = Sample::new(SourceKind::Controller(Side::Right), 3).with_pose(math::pose_identity()).with_button(Button::Menu, true);
        l.plan(&menu, Some((m, [0.0, 0.0])), &mut ops);
        assert!(ops.is_empty());
    }

    #[test]
    fn release_after_loss_is_cancel_not_up() {
        let m = member();
        let mut l = TouchLogic::default();
        let mut ops = Vec::new();
        l.plan(&hand(Side::Left, 0.9), Some((m, [0.0, 0.0])), &mut ops);
        assert!(l.contacts.is_down(0));
        // lane A's loss path: a sample with the pose gone
        let mut lost = Sample::new(SourceKind::Hand(Side::Left), 5);
        lost.tracked = false;
        lost.ready = false;
        l.plan(&lost, None, &mut ops);
        assert_eq!(ops, vec![TouchOp::Cancel]);
        assert_eq!(l.contacts.down_count(), 0);
        assert_eq!(l.cancels, 1);
        // an untracked pinch never goes down
        let mut s = hand(Side::Left, 0.9);
        s.tracked = false;
        l.plan(&s, Some((m, [0.0, 0.0])), &mut ops);
        assert!(ops.is_empty());
    }

    #[test]
    fn a_second_committer_on_a_held_contact_is_ignored_but_latched() {
        let m = member();
        let mut l = TouchLogic::default();
        let mut ops = Vec::new();
        let head = Sample::new(SourceKind::Head, 1).with_pose(math::pose_identity()).with_button(Button::Select, true);
        l.plan(&head, Some((m, [0.0, 0.0])), &mut ops);
        assert_eq!(l.contacts.contact(CONTACT_COMMIT).map(|c| c.kind), Some(SourceKind::Head));
        // a controller pinch while the head holds contact 2: no second down on the same id
        let mut ctrl = Sample::new(SourceKind::Controller(Side::Left), 2).with_pose(math::pose_identity());
        ctrl.values.pinch = 0.9;
        l.plan(&ctrl, Some((m, [0.2, 0.0])), &mut ops);
        assert!(ops.is_empty());
        let head_up = Sample::new(SourceKind::Head, 3).with_pose(math::pose_identity()).with_button(Button::Select, false);
        l.plan(&head_up, Some((m, [0.0, 0.0])), &mut ops);
        assert_eq!(ops[0], TouchOp::Up { id: CONTACT_COMMIT });
        // the controller's later release is not a commit: its latch was advanced
        ctrl.values.pinch = 0.1;
        l.plan(&ctrl, Some((m, [0.2, 0.0])), &mut ops);
        assert!(ops.is_empty());
    }
}
