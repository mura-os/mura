//! The window-management **floor** — the in-process policy module of
//! [window-workspace-management.md](../../../../docs/architecture/window-workspace-management.md)
//! (§2 verbs, §3 placement, §4 the `free` engine, §4a the grab, §5 lifecycle, §7 attachment,
//! §8 clutter, §11 the seam it is shaped for), spec zxr-core §3 `policy`. Ruled minimal
//! (2026-09-26): the compositor carries **`free` only** — head-relative spawn into a free angular
//! slot and a one-shot `tidy` — plus what is the compositor's whatever the engine: lifecycle,
//! attachment, limits, focus rules. `arc`/`dock`/`band` are external managers over the seam
//! (`seam.rs`, Phase 3); when one owns a place its engine is `Custom` and this module only keeps
//! the floor's invariants.
//!
//! **Shape.** [`Policy`] is the only writer of the scene verbs for band-3 members (spec §5a: one
//! mutation phase per tick; here every verb runs synchronously from the request that caused it
//! — a shell request, an input stage, the seam — and `tick` runs the timed ones). Every operation
//! is seam-shaped: `spawn`/`assign`, `set_pose`, `propose_dimensions` (a configure), `hide`/`show`,
//! `maximize`/`fullscreen`, `arrange`, `focus` with a serial (`input/focus.rs`), so the wire maps
//! onto it one to one.
//!
//! **Comparables** are research/64 (placement, arrangement, lifecycle, follow) and research/76
//! (the grab); each function names the one it follows.
//!
//! Budget: the allocator is O(candidates × members) per spawn/tidy (tens × tens); `tick` is one
//! pass over the members with a follow timer (≤ N ≈ 50) — no allocation in steady state.

pub mod follow;
pub mod free;
pub mod lifecycle;
pub mod seam;

use std::collections::HashMap;

use openxr as xr;

use crate::scene::{Flags, MemberId, PlaceId, Shape};
use crate::state::{Payload, Zxr};
use crate::xr::math;

pub use follow::{Attachment, FollowCfg};
pub use free::{Allocator, AngularBounds, SpawnCfg};
pub use lifecycle::{Life, Minimize};

/// The placement limits the compositor never delegates (§11): how near, how far, how large.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub min_distance_m: f32,
    pub max_distance_m: f32,
    pub max_angular_deg: f32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { min_distance_m: 0.4, max_distance_m: 5.0, max_angular_deg: 90.0 }
    }
}

impl Limits {
    /// Clamp a world pose's distance from `head` into `[min, max]` along the head→pose ray.
    pub fn clamp_pose(&self, pose: xr::Posef, head: xr::Vector3f) -> xr::Posef {
        let d = [pose.position.x - head.x, pose.position.y - head.y, pose.position.z - head.z];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-5 {
            return pose;
        }
        let want = len.clamp(self.min_distance_m, self.max_distance_m);
        if (want - len).abs() < 1e-6 {
            return pose;
        }
        let s = want / len;
        xr::Posef { orientation: pose.orientation, position: xr::Vector3f { x: head.x + d[0] * s, y: head.y + d[1] * s, z: head.z + d[2] * s } }
    }

    /// The widest plane allowed at `distance`, metres.
    pub fn max_width_m(&self, distance: f32) -> f32 {
        2.0 * distance.max(0.05) * (self.max_angular_deg.to_radians() * 0.5).tan()
    }
}

/// Which engine places a place's members (§4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Engine {
    /// the floor: head-relative spawn into a free angular slot, one-shot tidy
    #[default]
    Free,
    /// an external manager owns the place's arrangement (seam `set_engine(custom)`); the floor
    /// keeps its invariants and spawns with `free` if the manager does not place a new member
    Custom,
}

/// Everything the policy reads from settings (settings.rs `Prefs::policy_cfg`).
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyCfg {
    pub spawn: SpawnCfg,
    pub limits: Limits,
    pub minimize: Minimize,
    pub follow: FollowCfg,
    /// `wm.follow.default`: new windows lazy-follow (ruled: never by default)
    pub follow_default: bool,
    /// `wm.move.billboard`
    pub billboard: bool,
}

impl Default for PolicyCfg {
    fn default() -> Self {
        PolicyCfg { spawn: SpawnCfg::default(), limits: Limits::default(), minimize: Minimize::default(), follow: FollowCfg::default(), follow_default: false, billboard: true }
    }
}

/// The policy's state for one member.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemberState {
    pub life: Life,
    pub attachment: Attachment,
    /// the member was placed by the engine and has not been moved by the wearer since — a
    /// sibling arriving later may be grouped against it; `false` once grabbed
    pub at_spawn: bool,
}

/// The in-process window manager.
#[derive(Default)]
pub struct Policy {
    pub cfg: PolicyCfg,
    prefs_gen: u64,
    engines: HashMap<PlaceId, Engine>,
    members: HashMap<MemberId, MemberState>,
    /// most-recently-used order, front = most recent (the focus stack's twin, for tidy)
    mru: Vec<MemberId>,
    /// an external manager is connected (the seam); the floor defers placement to it
    pub manager_connected: bool,
    pub spawns: u64,
    pub spawns_sibling: u64,
    pub spawns_fallback: u64,
    pub arranges: u64,
    pub follows: u64,
    pub recenters: u64,
    pub maximizes: u64,
    pub fullscreens: u64,
    pub minimizes: u64,
}

impl Policy {
    pub fn engine(&self, place: PlaceId) -> Engine {
        self.engines.get(&place).copied().unwrap_or_default()
    }

    pub fn set_engine(&mut self, place: PlaceId, engine: Engine) {
        self.engines.insert(place, engine);
    }

    pub fn state(&self, member: MemberId) -> MemberState {
        self.members.get(&member).copied().unwrap_or_default()
    }

    pub fn state_mut(&mut self, member: MemberId) -> &mut MemberState {
        self.members.entry(member).or_default()
    }

    /// A member became the most recently used (a commit focused it).
    pub fn touch(&mut self, member: MemberId) {
        self.mru.retain(|m| *m != member);
        self.mru.insert(0, member);
    }

    pub fn forget(&mut self, member: MemberId) {
        self.members.remove(&member);
        self.mru.retain(|m| *m != member);
    }

    pub fn mru(&self) -> &[MemberId] {
        &self.mru
    }

    pub(crate) fn members_map(&self) -> &HashMap<MemberId, MemberState> {
        &self.members
    }

    /// The wearer moved the member (a grab): it is no longer at its spawn slot.
    pub fn moved_by_wearer(&mut self, member: MemberId) {
        self.state_mut(member).at_spawn = false;
    }
}

// ---------------------------------------------------------------------------------------------
// The verbs over `Zxr` (the scene, the clients)
// ---------------------------------------------------------------------------------------------

/// The head pose the policy places against: the runtime's this tick, else the identity.
fn head(st: &Zxr) -> xr::Posef {
    st.input.head.or(st.last_head_pose).unwrap_or(xr::Posef::IDENTITY)
}

/// The angular footprint of every mapped member of `place`, as the allocator sees it.
fn occupancy(st: &Zxr, place: PlaceId, head: xr::Posef, except: Option<MemberId>) -> Vec<AngularBounds> {
    let basis = free::ViewBasis::from_head(head);
    let mut out = Vec::new();
    for (id, m) in st.scene.iter() {
        // only mapped planes occupy: an unmapped one has no size yet (its geometry is the
        // client's first commit) and is re-placed at map (`placed_at_map`)
        if m.place != place || Some(id) == except || !m.m.mapped() || m.m.hidden {
            continue;
        }
        let Shape::Plane { size } = m.shape else { continue };
        let Some(world) = st.scene.world_pose(id) else { continue };
        out.push(basis.project(world.position, size));
    }
    // the head frame's exclusive bands (spec §4 rev 3.12: the window tiers honour the head
    // frame's usable rectangle at spawn — a panel's or the OSK's strip is occupied)
    out.extend(crate::shell::exclusive_occupancy(st));
    out
}

/// The place a new window goes to (places-model §4 currency): the focused member's place, else
/// the default place. (The head-frame transient place waits for the places machinery, §5.)
pub fn current_place(st: &Zxr) -> PlaceId {
    st.scene.focused.and_then(|id| st.scene.get(id).map(|m| m.place)).unwrap_or(st.scene.default_place)
}

/// **Spawn** (§3): a new toplevel's plane into the current place — beside its parent when it has
/// one (WayVR `Spread`, `window.rs:335-350`; never fully covering it, KWin `cascadeIfCovering`),
/// else at the engine's spawn pose: `spawn.distance` along the head's forward projected to the
/// horizontal, `spawn.elevation` below the eye line, facing the head, and if that slot is
/// occupied the nearest free angular slot (kwin-vr `SpaceAllocator3D::findFreePosition`,
/// `spaceallocator3d.cpp:262-300`). Returns the member.
pub fn spawn(st: &mut Zxr, shape: Shape, flags: Flags, payload: Payload, parent: Option<MemberId>) -> MemberId {
    let head = head(st);
    let place = parent.and_then(|p| st.scene.get(p).map(|m| m.place)).unwrap_or_else(|| current_place(st));
    let Shape::Plane { size } = shape else {
        // volumes (M2) take their pose from the client; place them at the spawn pose
        let pose = st.policy.cfg.spawn.spawn_pose(head);
        return add_world(st, place, pose, shape, flags, payload, false);
    };
    let occupied = occupancy(st, place, head, None);
    let cfg = st.policy.cfg.spawn;
    let basis = free::ViewBasis::from_head(head);
    // a parent: beside it, if that is free
    if let Some(p) = parent {
        if let Some(pw) = st.scene.world_pose(p) {
            let side = if basis.project(pw.position, size).centre_az >= 0.0 { -1.0 } else { 1.0 };
            let candidate = cfg.sibling_pose(pw, side, head);
            let bounds = basis.project(candidate.position, size);
            if !occupied.iter().any(|o| o.overlaps(&bounds)) {
                st.policy.spawns_sibling += 1;
                return add_world(st, place, candidate, shape, flags, payload, true);
            }
        }
    }
    let alloc = Allocator::new(cfg, st.policy.cfg.limits);
    let (pose, fallback) = alloc.find(head, size, &occupied);
    if fallback {
        st.policy.spawns_fallback += 1;
    }
    add_world(st, place, pose, shape, flags, payload, true)
}

fn add_world(st: &mut Zxr, place: PlaceId, world: xr::Posef, shape: Shape, flags: Flags, payload: Payload, at_spawn: bool) -> MemberId {
    let place_world = st.scene.place_world(place).unwrap_or(xr::Posef::IDENTITY);
    let local = math::pose_mul(math::pose_inverse(place_world), world);
    let id = st.scene.add(place, local, shape, flags, payload).expect("place is live");
    st.policy.spawns += 1;
    let follow_default = st.policy.cfg.follow_default;
    let s = st.policy.state_mut(id);
    s.at_spawn = at_spawn;
    s.life = Life::Mapped;
    s.attachment = if follow_default { Attachment::LazyFollow(follow::Follower::default()) } else { Attachment::Rigid };
    // a place with a fullscreen member hides its other members while it lasts (§5)
    if st.policy.place_fullscreen(st, place) {
        lifecycle::set_hidden(st, id, true);
    }
    id
}

/// The member's first commit gave it a size: place it for real (the spawn at `new_toplevel` saw
/// a 1×1 px geometry — xdg-shell's size is the client's first commit). Only a member still at its
/// engine slot moves; one a manager or the wearer already placed stays.
pub fn placed_at_map(st: &mut Zxr, member: MemberId, parent: Option<MemberId>) {
    // a place a manager arranges itself (`custom`) is the manager's to place; the floor places
    // everywhere else, connected manager or not (rev 1: presented at once, re-posed after)
    let place_engine = st.scene.get(member).map(|m| st.policy.engine(m.place)).unwrap_or_default();
    if !st.policy.state(member).at_spawn || place_engine == Engine::Custom {
        return;
    }
    let Some(m) = st.scene.get(member) else { return };
    let Shape::Plane { size } = m.shape else { return };
    let place = m.place;
    let head = head(st);
    let basis = free::ViewBasis::from_head(head);
    let occupied = occupancy(st, place, head, Some(member));
    let cfg = st.policy.cfg.spawn;
    if let Some(pw) = parent.and_then(|p| st.scene.world_pose(p)) {
        let side = if basis.project(pw.position, size).centre_az >= 0.0 { -1.0 } else { 1.0 };
        let candidate = cfg.sibling_pose(pw, side, head);
        if !occupied.iter().any(|o| o.overlaps(&basis.project(candidate.position, size))) {
            st.policy.spawns_sibling += 1;
            set_pose(st, member, candidate);
            return;
        }
    }
    let alloc = Allocator::new(cfg, st.policy.cfg.limits);
    let (pose, fallback) = alloc.find(head, size, &occupied);
    if fallback {
        st.policy.spawns_fallback += 1;
    }
    set_pose(st, member, pose);
}

/// **Set pose** (seam `set_pose`, a manager's or the grab's release): a world pose, clamped to
/// the limits, applied as the member's place-local pose.
pub fn set_pose(st: &mut Zxr, member: MemberId, world: xr::Posef) -> bool {
    let head = head(st);
    let world = st.policy.cfg.limits.clamp_pose(world, head.position);
    let Some(m) = st.scene.get(member) else { return false };
    let Some(place_world) = st.scene.place_world(m.place) else { return false };
    let local = math::pose_mul(math::pose_inverse(place_world), world);
    st.scene.set_local(member, local)
}

/// **Arrange** — tidy (§4 `free`, §8): the most recently used members of `place` onto free angular
/// slots at the spawn distance, most recent first at the centre; pinned and unmanaged members
/// untouched and counted as occupied (Android XR Tidy, xrdesktop `arrange_sphere`).
pub fn arrange(st: &mut Zxr, place: PlaceId) -> usize {
    let head = head(st);
    let basis = free::ViewBasis::from_head(head);
    let cfg = st.policy.cfg.spawn;
    let alloc = Allocator::new(cfg, st.policy.cfg.limits);
    // the fixed members occupy first
    let mut occupied: Vec<AngularBounds> = Vec::new();
    let mut movable: Vec<MemberId> = Vec::new();
    for (id, m) in st.scene.iter() {
        if m.place != place || !m.m.mapped() || m.m.hidden {
            continue;
        }
        let Shape::Plane { size } = m.shape else { continue };
        let Some(world) = st.scene.world_pose(id) else { continue };
        if m.flags.contains(Flags::PINNED) || !m.flags.contains(Flags::MANAGED) {
            occupied.push(basis.project(world.position, size));
        } else {
            movable.push(id);
        }
    }
    // MRU order: the stack first, then anyone the stack does not know (never focused)
    let mru = st.policy.mru.clone();
    movable.sort_by_key(|id| mru.iter().position(|m| m == id).unwrap_or(usize::MAX));
    let mut moved = 0;
    for id in movable {
        let Some(Shape::Plane { size }) = st.scene.get(id).map(|m| m.shape) else { continue };
        let (pose, _) = alloc.find(head, size, &occupied);
        occupied.push(basis.project(pose.position, size));
        if set_pose(st, id, pose) {
            moved += 1;
            st.policy.state_mut(id).at_spawn = true;
        }
    }
    st.policy.arranges += 1;
    moved
}

/// **Recenter** (§7, research/36 §8; the reserved input's long press): a rigid re-seat of every
/// head-relative member toward the head's current forward — here, the floor's tidy of the current
/// place at the head (Android XR's recenter brings the layout in front of the wearer; xrdesktop
/// `arrange_sphere`); pinned members exempt (`arrange` skips them). The runtime's own `LOCAL`
/// re-anchor remains the Monado upstream item (native-openxr-apps §6).
pub fn recenter(st: &mut Zxr) -> usize {
    // the shell's world anchor comes along: the greeter, lock, bars re-seat in front of the wearer
    // with the windows (research/64 §7: "a rigid re-seat of everything head-relative")
    crate::shell::anchor::reseat(st);
    let place = current_place(st);
    st.policy.recenters += 1;
    arrange(st, place)
}

/// **Tick**: settings by generation, then the timed attachments (§7 lazy-follow).
pub fn tick(st: &mut Zxr, now_ns: u64) {
    if st.policy.prefs_gen != st.prefs.generation {
        st.policy.prefs_gen = st.prefs.generation;
        st.policy.cfg = st.prefs.policy_cfg();
    }
    follow::tick(st, now_ns);
    seam::tick(st, now_ns);
}

/// A member left the scene.
pub fn removed(st: &mut Zxr, member: MemberId) {
    st.policy.forget(member);
}

/// The `policy:` line of `zxr ctl list`.
pub fn describe(st: &Zxr) -> String {
    let p = &st.policy;
    let following = p.members.values().filter(|s| matches!(s.attachment, Attachment::LazyFollow(_))).count();
    let h = head(st);
    format!(
        "policy: head=({:.2},{:.2},{:.2} q={:.2},{:.2},{:.2},{:.2}) engine={:?} manager={} spawns={} (sibling={} fallback={}) arranges={} recenters={} follows={} following={} maximizes={} fullscreens={} minimizes={} limits=[{:.2},{:.2}]m/{:.0}deg minimize={:?}",
        h.position.x,
        h.position.y,
        h.position.z,
        h.orientation.x,
        h.orientation.y,
        h.orientation.z,
        h.orientation.w,
        p.engine(st.scene.default_place),
        p.manager_connected,
        p.spawns,
        p.spawns_sibling,
        p.spawns_fallback,
        p.arranges,
        p.recenters,
        p.follows,
        following,
        p.maximizes,
        p.fullscreens,
        p.minimizes,
        p.cfg.limits.min_distance_m,
        p.cfg.limits.max_distance_m,
        p.cfg.limits.max_angular_deg,
        p.cfg.minimize
    )
}
