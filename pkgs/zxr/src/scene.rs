//! The scene (specs/zxr-core.md §3 `scene`, §4, §5, §5a): three typed arenas — `frames`
//! (spaces the runtime locates), `places` (a frame, a band, a local pose), `members` (a place, a
//! local pose, a shape, the frontend's payload) — with generational handles; poses, not
//! matrices; a per-tick *layer list* (one quad per mapped 2D member, ordered band ascending then
//! nearest-last; overflow into the projection pass) rather than a draw list; the mutation API
//! that is the policy boundary; and the ray → plane hit test the `input` module runs over.
//!
//! This module names no Wayland or Vulkan type: the member payload `M` is the frontend's.
//! Placement is R0's fan (motorcar's `WindowManager` shape, `windowmanager.cpp:147-159`) written
//! through `add` until M1's `free` engine.

// The mutation API and the frame/place vocabulary are the policy boundary (spec §5a); M1's
// `policy` and `input` consume what R0's fan and gaze pointer do not yet.
#![allow(dead_code)]

use crate::xr::math::{self, Mat4};
use openxr as xr;

/// Metres per logical pixel: 8.3 px/cm, 1000 px ≈ 1.2 m at the default distance (a comfortable panel).
/// The default of `wm.density_px_per_cm` (8.3 px/cm; kwin-vr's `ppu` 20 is named in the key's
/// description, research/73 Q9) — the live value is [`Layout::m_per_px`].
pub const M_PER_PX: f32 = 1.0 / 830.0;
/// The default of `wm.spawn.distance_m` (1.5 m), as the plane's −Z.
pub const PLANE_DISTANCE: f32 = -1.5;
/// The defaults of `wm.spawn.{sibling_offset_m,sibling_yaw_rad}` (R0's fan).
pub const FAN_OFFSET_M: f32 = 0.9;
pub const FAN_YAW_RAD: f32 = 0.35;

/// The scene's scale and placement — `wm.density_px_per_cm` and `wm.spawn.*`
/// (window-workspace-management §3; settings.rs `Prefs::layout`). One struct so a density change
/// re-derives every plane from its window geometry (`Zxr::rescale_planes`) and a spawn change
/// applies to the next window only — a placement is a decision made, not a binding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// metres per logical pixel (`1 / (density_px_per_cm · 100)`)
    pub m_per_px: f32,
    /// the spawn distance from the head, metres
    pub spawn_distance_m: f32,
    /// the spawn elevation relative to the eye line, degrees (negative = below)
    pub spawn_elevation_deg: f32,
    pub sibling_offset_m: f32,
    pub sibling_yaw_rad: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { m_per_px: M_PER_PX, spawn_distance_m: -PLANE_DISTANCE, spawn_elevation_deg: 0.0, sibling_offset_m: FAN_OFFSET_M, sibling_yaw_rad: FAN_YAW_RAD }
    }
}

impl Layout {
    /// `wm.density_px_per_cm` → metres per pixel; a non-positive density is the default.
    pub fn m_per_px_of(density_px_per_cm: f32) -> f32 {
        if density_px_per_cm > 0.0 { 1.0 / (density_px_per_cm * 100.0) } else { M_PER_PX }
    }

    /// The plane extents in metres of a `w × h` logical geometry.
    pub fn plane_size(&self, w: i32, h: i32) -> [f32; 2] {
        [w.max(1) as f32 * self.m_per_px, h.max(1) as f32 * self.m_per_px]
    }
}

// ---------------------------------------------------------------------------------------------
// Arena
// ---------------------------------------------------------------------------------------------

/// A slot index plus the generation it was allotted in; a stale handle never aliases a reused slot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Handle {
    idx: u32,
    gen: u32,
}

impl Handle {
    pub fn index(self) -> usize {
        self.idx as usize
    }
}

struct Slot<T> {
    gen: u32,
    val: Option<T>,
}

/// A generational arena: O(1) insert/remove/lookup, stable handles, slots reused.
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena { slots: Vec::new(), free: Vec::new(), len: 0 }
    }
}

impl<T> Arena<T> {
    pub fn insert(&mut self, val: T) -> Handle {
        self.len += 1;
        if let Some(idx) = self.free.pop() {
            let s = &mut self.slots[idx as usize];
            s.gen = s.gen.wrapping_add(1);
            s.val = Some(val);
            return Handle { idx, gen: s.gen };
        }
        self.slots.push(Slot { gen: 0, val: Some(val) });
        Handle { idx: (self.slots.len() - 1) as u32, gen: 0 }
    }

    pub fn remove(&mut self, h: Handle) -> Option<T> {
        let s = self.slots.get_mut(h.idx as usize)?;
        if s.gen != h.gen || s.val.is_none() {
            return None;
        }
        self.len -= 1;
        self.free.push(h.idx);
        s.val.take()
    }

    pub fn get(&self, h: Handle) -> Option<&T> {
        let s = self.slots.get(h.idx as usize)?;
        if s.gen == h.gen { s.val.as_ref() } else { None }
    }

    pub fn get_mut(&mut self, h: Handle) -> Option<&mut T> {
        let s = self.slots.get_mut(h.idx as usize)?;
        if s.gen == h.gen { s.val.as_mut() } else { None }
    }

    pub fn contains(&self, h: Handle) -> bool {
        self.get(h).is_some()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.val.as_ref().map(|v| (Handle { idx: i as u32, gen: s.gen }, v)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle, &mut T)> {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| {
            let gen = s.gen;
            s.val.as_mut().map(|v| (Handle { idx: i as u32, gen }, v))
        })
    }

    /// Handles of every live slot (allocation-free callers keep and reuse the vector).
    pub fn handles_into(&self, out: &mut Vec<Handle>) {
        out.clear();
        out.extend(self.iter().map(|(h, _)| h));
    }
}

// ---------------------------------------------------------------------------------------------
// Frames, places, members
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FrameId(pub Handle);
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PlaceId(pub Handle);
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MemberId(pub Handle);

/// Where a frame's pose comes from each tick.
pub enum Space {
    /// the session's base space (LOCAL): the identity
    Base,
    /// the head: the midpoint of the two `xrLocateViews` poses the tick already has
    Views,
    /// a runtime space located by the batched `xrLocateSpacesKHR` (hands, STAGE, anchors — M1)
    Xr(xr::Space),
    /// a pose pushed by a Mura service (mapping-service anchors before the EXT family)
    Service,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameKind {
    World,
    Head,
    LeftHand,
    RightHand,
    Anchor,
    Docked,
    Peer,
    /// the eye-gaze action space (spatial-input §2)
    Gaze,
}

pub struct Frame {
    pub space: Space,
    pub kind: FrameKind,
    /// in LOCAL
    pub pose: xr::Posef,
    pub valid: bool,
}

pub struct Place {
    pub frame: FrameId,
    pub local: xr::Posef,
    /// composition band, spec §4 (1 environment … 6 foreground); 2D windows are band 3
    pub band: u8,
    /// above its band-mates: the OSK raised over the surface it types into (phoc's rule,
    /// shell/mod.rs `update_osk_band`); the flatten draws raised last within a band and the hit
    /// test prefers it among coplanar planes
    pub raised: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    /// a 2D plane; extents in metres
    Plane { size: [f32; 2] },
    /// a 3D client's volume (M2); half extents in metres
    Volume { half: [f32; 3] },
}

/// xrdesktop's vocabulary (`xrd-window.h`): what policy may do with a member.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Flags(pub u8);

impl Flags {
    pub const DRAGGABLE: Flags = Flags(1);
    pub const MANAGED: Flags = Flags(2);
    pub const HOVERABLE: Flags = Flags(4);
    pub const PINNED: Flags = Flags(8);
    pub const WINDOW: Flags = Flags(1 | 2 | 4);

    pub fn contains(self, f: Flags) -> bool {
        self.0 & f.0 == f.0
    }
}

pub struct Member<M> {
    pub place: PlaceId,
    pub local: xr::Posef,
    pub shape: Shape,
    pub flags: Flags,
    pub m: M,
}

impl<M> Member<M> {
    pub fn half_size(&self) -> [f32; 2] {
        match self.shape {
            Shape::Plane { size } => [size[0] * 0.5, size[1] * 0.5],
            Shape::Volume { half } => [half[0], half[1]],
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The per-tick layer list
// ---------------------------------------------------------------------------------------------

/// One quad layer to submit: a mapped 2D member with its world pose this tick.
#[derive(Clone, Copy, Debug)]
pub struct QuadEntry {
    pub member: MemberId,
    pub band: u8,
    pub raised: bool,
    pub world: xr::Posef,
    pub size: [f32; 2],
    pub dist2: f32,
}

/// The flatten's output, reused every tick (no allocation in steady state).
#[derive(Default)]
pub struct Submit {
    /// quads in submission order: band ascending, nearest last within a band
    pub quads: Vec<QuadEntry>,
    /// members past the quad budget, drawn in the projection pass this tick
    pub overflow: Vec<QuadEntry>,
    scratch: Vec<QuadEntry>,
}

impl Submit {
    pub fn clear(&mut self) {
        self.quads.clear();
        self.overflow.clear();
        self.scratch.clear();
    }
}

// ---------------------------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------------------------

pub struct Scene<M> {
    pub frames: Arena<Frame>,
    pub places: Arena<Place>,
    pub members: Arena<Member<M>>,
    pub focused: Option<MemberId>,
    /// the two frames every session has (spec §5: "M1 ships one world frame and one head frame")
    pub world: FrameId,
    pub head: FrameId,
    /// the window tier's place until M1's engines: on the world frame, band 3
    pub default_place: PlaceId,
    pub submit: Submit,
    /// `wm.density_px_per_cm`, `wm.spawn.*` (settings.rs)
    pub layout: Layout,
    spawned: usize,
    handles: Vec<Handle>,
}

impl<M> Default for Scene<M> {
    fn default() -> Self {
        Scene::new()
    }
}

impl<M> Scene<M> {
    pub fn new() -> Self {
        let mut frames = Arena::default();
        let world = FrameId(frames.insert(Frame { space: Space::Base, kind: FrameKind::World, pose: math::pose_identity(), valid: true }));
        let head = FrameId(frames.insert(Frame { space: Space::Views, kind: FrameKind::Head, pose: math::pose_identity(), valid: false }));
        let mut places = Arena::default();
        let default_place = PlaceId(places.insert(Place { frame: world, local: math::pose_identity(), band: 3, raised: false }));
        Scene { frames, places, members: Arena::default(), focused: None, world, head, default_place, submit: Submit::default(), layout: Layout::default(), spawned: 0, handles: Vec::new() }
    }

    // ---- frames ----

    pub fn add_frame(&mut self, space: Space, kind: FrameKind) -> FrameId {
        FrameId(self.frames.insert(Frame { space, kind, pose: math::pose_identity(), valid: false }))
    }

    /// Write a located pose (the head from the views each tick; `Xr`/`Service` frames from
    /// their sources).
    pub fn set_frame_pose(&mut self, f: FrameId, pose: xr::Posef, valid: bool) {
        if let Some(fr) = self.frames.get_mut(f.0) {
            fr.pose = pose;
            fr.valid = valid;
        }
    }

    /// The frames the views do not give — the batched locate exists only when this is non-empty
    /// (spec §5a).
    pub fn xr_frames(&self) -> impl Iterator<Item = (FrameId, &xr::Space)> {
        self.frames.iter().filter_map(|(h, f)| match &f.space {
            Space::Xr(s) => Some((FrameId(h), s)),
            _ => None,
        })
    }

    // ---- places ----

    pub fn add_place(&mut self, frame: FrameId, local: xr::Posef, band: u8) -> PlaceId {
        PlaceId(self.places.insert(Place { frame, local, band, raised: false }))
    }

    /// `pin` / `grab-all` / `assign-to-frame`: one index write.
    pub fn reparent_place(&mut self, p: PlaceId, frame: FrameId) -> bool {
        if !self.frames.contains(frame.0) {
            return false;
        }
        match self.places.get_mut(p.0) {
            Some(pl) => {
                pl.frame = frame;
                true
            }
            None => false,
        }
    }

    /// A layer member's own place goes with it (never the default place).
    pub fn remove_place(&mut self, p: PlaceId) -> bool {
        if p == self.default_place || self.members.iter().any(|(_, m)| m.place == p) {
            return false;
        }
        self.places.remove(p.0).is_some()
    }

    /// `set_layer`: a layer surface moved bands.
    pub fn set_place_band(&mut self, p: PlaceId, band: u8) -> bool {
        match self.places.get_mut(p.0) {
            Some(pl) => {
                pl.band = band;
                true
            }
            None => false,
        }
    }

    /// Raise or lower a place within its band (`Place::raised`).
    pub fn set_place_raised(&mut self, p: PlaceId, raised: bool) -> bool {
        match self.places.get_mut(p.0) {
            Some(pl) => {
                pl.raised = raised;
                true
            }
            None => false,
        }
    }

    /// Whether a member's place is raised within its band.
    pub fn raised(&self, id: MemberId) -> Option<bool> {
        let m = self.members.get(id.0)?;
        Some(self.places.get(m.place.0)?.raised)
    }

    pub fn place_world(&self, p: PlaceId) -> Option<xr::Posef> {
        let pl = self.places.get(p.0)?;
        let fr = self.frames.get(pl.frame.0)?;
        Some(math::pose_mul(fr.pose, pl.local))
    }

    // ---- members: the mutation API (the policy boundary) ----

    pub fn add(&mut self, place: PlaceId, local: xr::Posef, shape: Shape, flags: Flags, m: M) -> Option<MemberId> {
        if !self.places.contains(place.0) {
            return None;
        }
        let id = MemberId(self.members.insert(Member { place, local, shape, flags, m }));
        self.focused = Some(id);
        Some(id)
    }

    /// The stand-in placement (R0's fan): slot 0 centre, then alternating right/left with a
    /// small yaw toward the viewer, in the default place — at `wm.spawn.distance_m`, raised or
    /// lowered by `wm.spawn.elevation_deg`, siblings `wm.spawn.sibling_offset_m` apart and
    /// `wm.spawn.sibling_yaw_rad` turned toward the viewer.
    pub fn add_fanned(&mut self, shape: Shape, flags: Flags, m: M) -> MemberId {
        let n = self.spawned as i32;
        let slot = (n + 1) / 2 * if n % 2 == 1 { 1 } else { -1 };
        self.spawned += 1;
        let l = self.layout;
        let d = l.spawn_distance_m.max(0.1);
        let y = d * l.spawn_elevation_deg.to_radians().tan();
        let local = math::pose_yaw([slot as f32 * l.sibling_offset_m, y, -d], -(slot as f32) * l.sibling_yaw_rad);
        let place = self.default_place;
        self.add(place, local, shape, flags, m).expect("default place is live")
    }

    pub fn remove(&mut self, id: MemberId) -> Option<Member<M>> {
        let m = self.members.remove(id.0)?;
        if self.focused == Some(id) {
            self.focused = self.members.iter().last().map(|(h, _)| MemberId(h));
        }
        Some(m)
    }

    pub fn reparent(&mut self, id: MemberId, place: PlaceId) -> bool {
        if !self.places.contains(place.0) {
            return false;
        }
        match self.members.get_mut(id.0) {
            Some(m) => {
                m.place = place;
                true
            }
            None => false,
        }
    }

    pub fn set_local(&mut self, id: MemberId, local: xr::Posef) -> bool {
        match self.members.get_mut(id.0) {
            Some(m) => {
                m.local = local;
                true
            }
            None => false,
        }
    }

    pub fn set_shape(&mut self, id: MemberId, shape: Shape) -> bool {
        match self.members.get_mut(id.0) {
            Some(m) => {
                m.shape = shape;
                true
            }
            None => false,
        }
    }

    pub fn set_flags(&mut self, id: MemberId, flags: Flags) -> bool {
        match self.members.get_mut(id.0) {
            Some(m) => {
                m.flags = flags;
                true
            }
            None => false,
        }
    }

    /// Focus a live member, or nothing.
    pub fn focus(&mut self, id: Option<MemberId>) -> bool {
        match id {
            Some(id) if !self.members.contains(id.0) => false,
            other => {
                self.focused = other;
                true
            }
        }
    }

    pub fn focus_next(&mut self) {
        self.handles.clear();
        self.members.handles_into(&mut self.handles);
        let next = match (self.handles.is_empty(), self.focused) {
            (true, _) => None,
            (false, Some(MemberId(h))) => {
                let i = self.handles.iter().position(|x| *x == h).map(|i| (i + 1) % self.handles.len()).unwrap_or(0);
                Some(MemberId(self.handles[i]))
            }
            (false, None) => Some(MemberId(self.handles[0])),
        };
        self.focused = next;
    }

    // ---- reads ----

    pub fn get(&self, id: MemberId) -> Option<&Member<M>> {
        self.members.get(id.0)
    }

    pub fn get_mut(&mut self, id: MemberId) -> Option<&mut Member<M>> {
        self.members.get_mut(id.0)
    }

    pub fn focused(&self) -> Option<&Member<M>> {
        self.focused.and_then(|id| self.members.get(id.0))
    }

    pub fn focused_mut(&mut self) -> Option<&mut Member<M>> {
        let id = self.focused?;
        self.members.get_mut(id.0)
    }

    /// The first member whose payload satisfies `pred` (the frontend's surface → member lookup;
    /// linear over N ≈ 50).
    pub fn find(&self, pred: impl Fn(&M) -> bool) -> Option<MemberId> {
        self.members.iter().find(|(_, m)| pred(&m.m)).map(|(h, _)| MemberId(h))
    }

    pub fn iter(&self) -> impl Iterator<Item = (MemberId, &Member<M>)> {
        self.members.iter().map(|(h, m)| (MemberId(h), m))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (MemberId, &mut Member<M>)> {
        self.members.iter_mut().map(|(h, m)| (MemberId(h), m))
    }

    /// `frame.pose ∘ place.local ∘ member.local`.
    pub fn world_pose(&self, id: MemberId) -> Option<xr::Posef> {
        let m = self.members.get(id.0)?;
        Some(math::pose_mul(self.place_world(m.place)?, m.local))
    }

    /// The band of a member's place.
    pub fn band(&self, id: MemberId) -> Option<u8> {
        let m = self.members.get(id.0)?;
        Some(self.places.get(m.place.0)?.band)
    }

    // ---- the flatten ----

    /// Build this tick's layer list from the mapped 2D members (`mapped(&M)` decides). The quad
    /// budget is allotted by band priority — band 5 first, then 4, 3, 2, nearest-first within a
    /// band — and quads are ordered band ascending, nearest last (spec §5a). Members past the
    /// budget go to `overflow` for the projection pass.
    pub fn flatten(&mut self, head: [f32; 3], budget: usize, mapped: impl Fn(&M) -> bool) -> &Submit {
        let mut submit = std::mem::take(&mut self.submit);
        self.flatten_into(&mut submit, head, budget, mapped);
        self.submit = submit;
        &self.submit
    }

    /// `flatten` into a caller-owned scratch (the tick takes `self.submit` out, flattens, and
    /// puts it back, so the list can be read while the scene is mutated).
    pub fn flatten_into(&self, submit: &mut Submit, head: [f32; 3], budget: usize, mapped: impl Fn(&M) -> bool) {
        let Scene { frames, places, members, .. } = self;
        submit.clear();
        for (h, m) in members.iter() {
            if !mapped(&m.m) {
                continue;
            }
            let Shape::Plane { size } = m.shape else { continue };
            let Some(pl) = places.get(m.place.0) else { continue };
            let Some(fr) = frames.get(pl.frame.0) else { continue };
            if !(2..=5).contains(&pl.band) {
                continue;
            }
            let world = math::pose_mul(math::pose_mul(fr.pose, pl.local), m.local);
            let d = [world.position.x - head[0], world.position.y - head[1], world.position.z - head[2]];
            submit.scratch.push(QuadEntry { member: MemberId(h), band: pl.band, raised: pl.raised, world, size, dist2: d[0] * d[0] + d[1] * d[1] + d[2] * d[2] });
        }
        // band descending, raised first, nearest first: the allotment order
        submit.scratch.sort_by(|a, b| b.band.cmp(&a.band).then(b.raised.cmp(&a.raised)).then(a.dist2.total_cmp(&b.dist2)).then(a.member.0.idx.cmp(&b.member.0.idx)));
        for (i, q) in submit.scratch.iter().enumerate() {
            if i < budget {
                submit.quads.push(*q);
            } else {
                submit.overflow.push(*q);
            }
        }
        // submission order: band ascending, raised last within a band, nearest last
        submit.quads.sort_by(|a, b| a.band.cmp(&b.band).then(a.raised.cmp(&b.raised)).then(b.dist2.total_cmp(&a.dist2)).then(a.member.0.idx.cmp(&b.member.0.idx)));
    }

    // ---- the hit test ----

    /// Cast a world-space ray against every mapped plane; nearest hit wins. Returns the member,
    /// the plane-local point (metres, y up) and the distance. `mapped(&M)` filters.
    pub fn hit(&self, origin: [f32; 3], dir: [f32; 3], mapped: impl Fn(&M) -> bool) -> Option<(MemberId, [f32; 2], f32)> {
        let mut best: Option<(f32, MemberId, [f32; 2])> = None;
        for (h, m) in self.members.iter() {
            if !mapped(&m.m) {
                continue;
            }
            let Shape::Plane { size } = m.shape else { continue };
            let Some(world) = self.place_world(m.place).map(|pw| math::pose_mul(pw, m.local)) else { continue };
            if let Some((t, local)) = ray_plane(origin, dir, world, [size[0] * 0.5, size[1] * 0.5]) {
                if best.map(|b| t < b.0).unwrap_or(true) {
                    best = Some((t, MemberId(h), local));
                }
            }
        }
        best.map(|(t, id, local)| (id, local, t))
    }
}

/// Ray (origin, direction) against the plane `z = 0` of `pose`, extents `half` — returns the
/// distance along the ray and the plane-local hit point, or None if missed / behind.
pub fn ray_plane(origin: [f32; 3], dir: [f32; 3], pose: xr::Posef, half: [f32; 2]) -> Option<(f32, [f32; 2])> {
    let inv = math::pose_inverse(pose);
    let o = math::pose_apply(inv, origin);
    let d = math::rotate(inv.orientation, dir);
    if d[2].abs() < 1e-5 {
        return None;
    }
    let t = -o[2] / d[2];
    if t <= 0.0 {
        return None;
    }
    let hx = o[0] + d[0] * t;
    let hy = o[1] + d[1] * t;
    if hx.abs() <= half[0] && hy.abs() <= half[1] {
        Some((t, [hx, hy]))
    } else {
        None
    }
}

/// The point of the plane `z = 0` of `pose` (extents `half`) nearest to a world point — the
/// projection clamped to the extents — with the distance between them. Poke magnetism
/// (`input.magnetism.enabled`, MRTK3 `ReticleMagnetism.cs:37` `magnetRange`): a fingertip
/// near a plane but not over it is drawn to the nearest point of it.
pub fn nearest_point_on_plane(point: [f32; 3], pose: xr::Posef, half: [f32; 2]) -> (f32, [f32; 2]) {
    let inv = math::pose_inverse(pose);
    let p = math::pose_apply(inv, point);
    let local = [p[0].clamp(-half[0], half[0]), p[1].clamp(-half[1], half[1])];
    let d = [p[0] - local[0], p[1] - local[1], p[2]];
    ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(), local)
}

/// A plane-local point (metres, y up) → logical surface coordinates of a `w × h` geometry with
/// origin `(gx, gy)`, for a plane of extents `size` metres.
pub fn local_to_logical(local: [f32; 2], size: [f32; 2], geo: (i32, i32, i32, i32)) -> (f64, f64) {
    let (gx, gy, w, h) = geo;
    let x = (local[0] / size[0] + 0.5) as f64 * w as f64 + gx as f64;
    let y = (0.5 - local[1] / size[1]) as f64 * h as f64 + gy as f64;
    (x, y)
}

/// A plane's model matrix for a pass.
pub fn model(pose: xr::Posef) -> Mat4 {
    math::pose_to_mat(pose)
}

// ---------------------------------------------------------------------------------------------
// Tests: no runtime, a unit payload
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(w: f32, h: f32) -> Shape {
        Shape::Plane { size: [w, h] }
    }

    #[test]
    fn layout_drives_the_fan_and_the_plane_scale() {
        // `wm.density_px_per_cm` 20 (kwin-vr's ppu): 0.5 mm per px; 1000 px = 0.5 m
        assert!((Layout::m_per_px_of(20.0) - 0.0005).abs() < 1e-9);
        assert_eq!(Layout::m_per_px_of(0.0), M_PER_PX, "a non-positive density is the default");
        let l = Layout { m_per_px: Layout::m_per_px_of(20.0), ..Layout::default() };
        let s = l.plane_size(1000, 500);
        assert!((s[0] - 0.5).abs() < 1e-6 && (s[1] - 0.25).abs() < 1e-6);
        // `wm.spawn.*`: distance 2 m, 10° below the eye line, siblings 1 m apart
        let mut sc: Scene<()> = Scene::new();
        sc.layout = Layout { spawn_distance_m: 2.0, spawn_elevation_deg: -10.0, sibling_offset_m: 1.0, sibling_yaw_rad: 0.2, ..Layout::default() };
        let a = sc.add_fanned(plane(1.0, 1.0), Flags::WINDOW, ());
        let b = sc.add_fanned(plane(1.0, 1.0), Flags::WINDOW, ());
        let pa = sc.world_pose(a).unwrap().position;
        let pb = sc.world_pose(b).unwrap().position;
        assert!((pa.z + 2.0).abs() < 1e-6, "at the spawn distance");
        assert!((pa.y - 2.0 * (-10.0f32).to_radians().tan()).abs() < 1e-6, "lowered by the elevation");
        assert!((pb.x - 1.0).abs() < 1e-6, "the sibling one offset to the right");
    }

    #[test]
    fn nearest_point_on_plane_clamps_to_the_extents() {
        let pose = math::pose_yaw([0.0, 0.0, -1.0], 0.0);
        // a fingertip 5 cm in front of the plane, over it: the projection, 5 cm away
        let (d, local) = nearest_point_on_plane([0.1, 0.2, -0.95], pose, [0.5, 0.5]);
        assert!((d - 0.05).abs() < 1e-6);
        assert!((local[0] - 0.1).abs() < 1e-6 && (local[1] - 0.2).abs() < 1e-6);
        // beside the plane: the nearest edge point, at the in-plane gap
        let (d, local) = nearest_point_on_plane([0.6, 0.0, -1.0], pose, [0.5, 0.5]);
        assert!((d - 0.1).abs() < 1e-6);
        assert!((local[0] - 0.5).abs() < 1e-6);
        // MRTK3's range: 7 cm beside it is in, 8 cm is out
        let (d, _) = nearest_point_on_plane([0.57, 0.0, -1.0], pose, [0.5, 0.5]);
        assert!(d <= crate::input::hit::MAGNET_RANGE_M + 1e-6);
        let (d, _) = nearest_point_on_plane([0.58, 0.0, -1.0], pose, [0.5, 0.5]);
        assert!(d > crate::input::hit::MAGNET_RANGE_M);
    }

    #[test]
    fn arena_handles_do_not_alias_reused_slots() {
        let mut a: Arena<u32> = Arena::default();
        let h1 = a.insert(1);
        assert_eq!(a.remove(h1), Some(1));
        let h2 = a.insert(2);
        assert_eq!(h1.index(), h2.index());
        assert_ne!(h1, h2);
        assert_eq!(a.get(h1), None);
        assert_eq!(a.get(h2), Some(&2));
        assert_eq!(a.remove(h1), None);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn ray_hits_centre_of_facing_plane() {
        let (t, local) = ray_plane([0.0; 3], [0.0, 0.0, -1.0], math::pose_yaw([0.0, 0.0, PLANE_DISTANCE], 0.0), [0.5, 0.3]).unwrap();
        assert!((t - 1.5).abs() < 1e-5);
        assert!(local[0].abs() < 1e-5 && local[1].abs() < 1e-5);
    }

    #[test]
    fn ray_misses_outside_extents_and_behind() {
        assert!(ray_plane([0.0; 3], [0.0, 0.0, -1.0], math::pose_yaw([1.0, 0.0, PLANE_DISTANCE], 0.0), [0.5, 0.3]).is_none());
        assert!(ray_plane([0.0; 3], [0.0, 0.0, 1.0], math::pose_yaw([0.0, 0.0, PLANE_DISTANCE], 0.0), [0.5, 0.3]).is_none());
    }

    #[test]
    fn yawed_plane_is_hit_where_it_actually_is() {
        // plane at x = 0.9, yawed −0.35 rad toward the viewer (the fan's second slot): a ray aimed
        // at its centre hits at local (0, 0)
        let pos = [0.9, 0.0, PLANE_DISTANCE];
        let l = (pos[0] * pos[0] + pos[2] * pos[2]).sqrt();
        let dir = [pos[0] / l, 0.0, pos[2] / l];
        let (_, local) = ray_plane([0.0; 3], dir, math::pose_yaw(pos, -0.35), [0.5, 0.3]).unwrap();
        assert!(local[0].abs() < 1e-4 && local[1].abs() < 1e-4);
    }

    #[test]
    fn pose_composition_matches_matrix_composition() {
        let a = math::pose_yaw([1.0, 2.0, 3.0], 0.7);
        let b = math::pose_yaw([-0.5, 0.25, 1.0], -1.3);
        let ab = math::pose_mul(a, b);
        let via_pose = math::pose_apply(ab, [0.3, -0.2, 0.9]);
        let via_mat = math::transform_point(&math::mul(&math::pose_to_mat(a), &math::pose_to_mat(b)), [0.3, -0.2, 0.9]);
        for i in 0..3 {
            assert!((via_pose[i] - via_mat[i]).abs() < 1e-4, "{via_pose:?} vs {via_mat:?}");
        }
        let round = math::pose_apply(math::pose_inverse(ab), via_pose);
        for (r, e) in round.iter().zip([0.3, -0.2, 0.9]) {
            assert!((r - e).abs() < 1e-4);
        }
    }

    #[test]
    fn world_pose_composes_frame_place_member() {
        let mut s: Scene<()> = Scene::new();
        let frame = s.add_frame(Space::Service, FrameKind::Anchor);
        s.set_frame_pose(frame, math::pose_yaw([10.0, 0.0, 0.0], 0.0), true);
        let place = s.add_place(frame, math::pose_yaw([0.0, 1.0, 0.0], 0.0), 3);
        let id = s.add(place, math::pose_yaw([0.0, 0.0, -1.0], 0.0), plane(1.0, 1.0), Flags::WINDOW, ()).unwrap();
        let w = s.world_pose(id).unwrap();
        assert!((w.position.x - 10.0).abs() < 1e-6 && (w.position.y - 1.0).abs() < 1e-6 && (w.position.z + 1.0).abs() < 1e-6);
        // reparenting the place to the world frame moves the member with it: one index write
        assert!(s.reparent_place(place, s.world));
        let w = s.world_pose(id).unwrap();
        assert!(w.position.x.abs() < 1e-6);
    }

    #[test]
    fn fan_alternates_sides() {
        let mut s: Scene<u8> = Scene::new();
        let ids: Vec<MemberId> = (0..5u8).map(|i| s.add_fanned(plane(1.0, 1.0), Flags::WINDOW, i)).collect();
        let xs: Vec<i32> = ids.iter().map(|id| (s.world_pose(*id).unwrap().position.x / 0.9).round() as i32).collect();
        assert_eq!(xs, vec![0, 1, -1, 2, -2]);
    }

    #[test]
    fn hit_picks_nearest_mapped_plane() {
        let mut s: Scene<bool> = Scene::new();
        let far = s.add(s.default_place, math::pose_yaw([0.0, 0.0, -3.0], 0.0), plane(2.0, 2.0), Flags::WINDOW, true).unwrap();
        let near = s.add(s.default_place, math::pose_yaw([0.0, 0.0, -1.0], 0.0), plane(0.5, 0.5), Flags::WINDOW, true).unwrap();
        let unmapped = s.add(s.default_place, math::pose_yaw([0.0, 0.0, -0.5], 0.0), plane(0.5, 0.5), Flags::WINDOW, false).unwrap();
        let (id, _, t) = s.hit([0.0; 3], [0.0, 0.0, -1.0], |m| *m).unwrap();
        assert_eq!(id, near);
        assert!((t - 1.0).abs() < 1e-5);
        assert_ne!(id, unmapped);
        assert_ne!(id, far);
    }

    #[test]
    fn quads_are_band_ascending_nearest_last_and_budget_is_by_band_priority() {
        let mut s: Scene<()> = Scene::new();
        let overlay = s.add_place(s.head, math::pose_identity(), 5);
        let far_window = s.add(s.default_place, math::pose_yaw([0.0, 0.0, -5.0], 0.0), plane(1.0, 1.0), Flags::WINDOW, ()).unwrap();
        let near_window = s.add(s.default_place, math::pose_yaw([0.0, 0.0, -1.0], 0.0), plane(1.0, 1.0), Flags::WINDOW, ()).unwrap();
        let osd = s.add(overlay, math::pose_yaw([0.0, 0.0, -9.0], 0.0), plane(0.2, 0.1), Flags::default(), ()).unwrap();
        // unlimited: band 3 (far then near), then band 5
        let sub = s.flatten([0.0; 3], 128, |_| true);
        let order: Vec<MemberId> = sub.quads.iter().map(|q| q.member).collect();
        assert_eq!(order, vec![far_window, near_window, osd]);
        assert!(sub.overflow.is_empty());
        // budget 2: the overlay plane (band 5) is kept even though it is the farthest; the
        // far window overflows
        let sub = s.flatten([0.0; 3], 2, |_| true);
        let order: Vec<MemberId> = sub.quads.iter().map(|q| q.member).collect();
        assert_eq!(order, vec![near_window, osd]);
        assert_eq!(sub.overflow.iter().map(|q| q.member).collect::<Vec<_>>(), vec![far_window]);
        // budget 0: everything overflows
        let sub = s.flatten([0.0; 3], 0, |_| true);
        assert!(sub.quads.is_empty());
        assert_eq!(sub.overflow.len(), 3);
    }

    #[test]
    fn focus_follows_add_and_survives_remove() {
        let mut s: Scene<()> = Scene::new();
        let a = s.add_fanned(plane(1.0, 1.0), Flags::WINDOW, ());
        let b = s.add_fanned(plane(1.0, 1.0), Flags::WINDOW, ());
        assert_eq!(s.focused, Some(b));
        s.focus_next();
        assert_eq!(s.focused, Some(a));
        s.remove(a);
        assert_eq!(s.focused, Some(b));
        s.remove(b);
        assert_eq!(s.focused, None);
        assert!(!s.focus(Some(a)));
        assert!(s.focus(None));
    }

    /// The arena invariants (spec §5a), checked after every operation of a seeded random sweep:
    /// every live member names a live place, every live place a live frame, no stale handle
    /// resolves, focus names a live member or nothing, and the flatten sees exactly the mapped
    /// members of bands 2–5.
    fn verify_invariants(s: &mut Scene<bool>, stale: &[MemberId]) {
        for (_, m) in s.members.iter() {
            let pl = s.places.get(m.place.0).expect("live member → live place");
            s.frames.get(pl.frame.0).expect("live place → live frame");
        }
        for (_, pl) in s.places.iter() {
            assert!(s.frames.contains(pl.frame.0));
        }
        for id in stale {
            assert!(s.get(*id).is_none(), "stale handle resolved");
        }
        if let Some(f) = s.focused {
            assert!(s.members.contains(f.0), "focus names a dead member");
        }
        let expected = s.iter().filter(|(_, m)| m.m && matches!(m.shape, Shape::Plane { .. }) && s.places.get(m.place.0).map(|p| (2..=5).contains(&p.band)).unwrap_or(false)).count();
        let sub = s.flatten([0.0; 3], 4, |m| *m);
        assert_eq!(sub.quads.len() + sub.overflow.len(), expected);
        assert!(sub.quads.len() <= 4);
        for w in sub.quads.windows(2) {
            assert!(w[0].band <= w[1].band, "quads not band-ascending");
            if w[0].band == w[1].band {
                assert!(w[0].dist2 >= w[1].dist2, "quads not nearest-last within a band");
            }
        }
    }

    #[test]
    fn property_sweep_of_the_verbs_holds_the_invariants() {
        // a seeded LCG: dependency-free, reproducible
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rnd = move |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n.max(1)
        };
        let mut s: Scene<bool> = Scene::new();
        let mut live: Vec<MemberId> = Vec::new();
        let mut stale: Vec<MemberId> = Vec::new();
        let mut places = vec![s.default_place];
        let mut frames = vec![s.world, s.head];
        for step in 0..4000 {
            match rnd(10) {
                0 | 1 | 2 => {
                    let place = places[rnd(places.len() as u64) as usize];
                    let mapped = rnd(4) != 0;
                    let pose = math::pose_yaw([rnd(7) as f32 - 3.0, rnd(3) as f32 - 1.0, -(rnd(5) as f32) - 0.5], rnd(6) as f32 * 0.5);
                    if let Some(id) = s.add(place, pose, plane(0.5 + rnd(3) as f32 * 0.25, 0.5), Flags::WINDOW, mapped) {
                        live.push(id);
                    }
                }
                3 => {
                    if !live.is_empty() {
                        let id = live.swap_remove(rnd(live.len() as u64) as usize);
                        assert!(s.remove(id).is_some());
                        stale.push(id);
                    }
                }
                4 => {
                    if !live.is_empty() {
                        let id = live[rnd(live.len() as u64) as usize];
                        let place = places[rnd(places.len() as u64) as usize];
                        assert!(s.reparent(id, place));
                    }
                }
                5 => {
                    if !live.is_empty() {
                        let id = live[rnd(live.len() as u64) as usize];
                        assert!(s.set_local(id, math::pose_yaw([0.0, 0.0, -(rnd(9) as f32) - 0.5], 0.0)));
                    }
                }
                6 => {
                    let f = frames[rnd(frames.len() as u64) as usize];
                    places.push(s.add_place(f, math::pose_identity(), 2 + rnd(4) as u8));
                }
                7 => {
                    let f = s.add_frame(Space::Service, FrameKind::Anchor);
                    s.set_frame_pose(f, math::pose_yaw([rnd(5) as f32, 0.0, 0.0], 0.0), true);
                    frames.push(f);
                }
                8 => {
                    let p = places[rnd(places.len() as u64) as usize];
                    let f = frames[rnd(frames.len() as u64) as usize];
                    assert!(s.reparent_place(p, f));
                }
                _ => {
                    if rnd(2) == 0 {
                        s.focus_next();
                    } else if let Some(id) = stale.last() {
                        assert!(!s.focus(Some(*id)), "step {step}: focusing a removed member succeeded");
                    }
                }
            }
            verify_invariants(&mut s, &stale);
        }
        assert!(!stale.is_empty() && !live.is_empty());
    }
}
