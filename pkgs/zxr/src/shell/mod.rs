//! The shell-layer half of the compositor (spec §4 rev 3.12, §8, §9, §10; shell-plane.md §2;
//! research/77): `wlr-layer-shell` + `zxr-layer-anchoring-v1` served, layer surfaces as scene
//! members of bands 2/4/5 arranged per frame with wlroots' arithmetic, the wearer's placement
//! table, the privileged-global filter and the trusted connection, `ext-session-lock` for the
//! desktop profile.
//!
//! **Arrangement** (`arrange`): the algorithm every wlroots user, KWin and smithay share
//! (`references/wlroots/types/scene/layer_shell_v1.c:61-114`) run per *frame* in the frame's pixel
//! rectangle — bounds = the frame's usable rectangle, or the full rectangle when the zone is −1;
//! size 0 stretches between the two anchors minus margins; anchored edges pin, the rest centre; a
//! positive zone shrinks the usable rectangle by `zone + margin` on the surface's one exclusive
//! edge (`wlr_layer_shell_v1.c:657-684`). Two passes in sway's order — positive zones first, then
//! the rest, each overlay→top→bottom→background (`references/sway/sway/desktop/layer_shell.c:56-93`).
//! It runs on a layer surface's commit, map and unmap, never per tick. The usable rectangle is
//! per frame: a world-frame panel never shrinks the head frame (research/77 §3.2).
//!
//! **Where a surface sits is the wearer's** (owner ruling 2026-09-27; research/77 §3.3a): a
//! `shell.place:<namespace>` row wins, else the client's anchoring request, else the **world**
//! fallback (the head's extent `shell.head.*` on the shell's anchor — floating in front of the wearer
//! where it was summoned, head free, re-seated on recenter; nothing is head-locked unless it asks,
//! spatial-input §13, ruled 2026-09-28/29; anchor.rs). Hyprland's layer rules by namespace are the precedent
//! (`references/hyprland/src/desktop/rule/layerRule/LayerRule.cpp:96-115`). Seed rows for the
//! carried components' namespaces are `place::seed`.
//!
//! **Budget** (spec §12 fence, research/77 §7): arrange is O(layer members) integer arithmetic on
//! layer events only; one quad per mapped layer member; the filter is one predicate per global
//! per registry/bind; the still-pointer rule (input/pointer.rs) removes 62 wake-ups/s per resting
//! client. No thread, no per-tick work.

pub mod anchor;
pub mod anchoring;
pub mod filter;
pub mod layer;
pub mod lock;
pub mod place;

use std::time::Duration;

use openxr as xr;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::desktop::utils::{send_frames_surface_tree, under_from_surface_tree};
use smithay::desktop::{LayerSurface, Window, WindowSurfaceType};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::text_input::TextInputSeat;
use smithay::utils::{Logical, Point, Rectangle, Size};
use smithay::wayland::compositor::{with_states, SurfaceData};
use smithay::wayland::session_lock::LockSurface;
use smithay::wayland::shell::wlr_layer::{Anchor, ExclusiveZone, KeyboardInteractivity, Layer, LayerSurfaceCachedState, LayerSurfaceData};
use smithay::wayland::shell::xdg::ToplevelSurface;

use crate::scene::{FrameId, MemberId, Shape};
use crate::state::Zxr;
use crate::xr::math;

pub use anchoring::Frame;
pub use place::{PlaceFrame, PlaceRow};

// ---------------------------------------------------------------------------------------------
// The member's surface: one of the three roles a plane can carry
// ---------------------------------------------------------------------------------------------

/// What a member's plane shows: an xdg toplevel (the window tiers), a layer surface (the shell
/// layers) or a lock surface (`ext-session-lock`, the desktop profile). The five methods every
/// caller used on `Window` are kept, so the tick, the hit test and the WM floor read one type.
#[derive(Clone, Debug)]
pub enum Surface {
    Window(Window),
    Layer(LayerSurface),
    Lock(LockSurface),
}

impl Surface {
    pub fn toplevel(&self) -> Option<&ToplevelSurface> {
        match self {
            Surface::Window(w) => w.toplevel(),
            _ => None,
        }
    }

    pub fn is_window(&self) -> bool {
        matches!(self, Surface::Window(_))
    }

    /// The root `wl_surface` of the tree this member shows.
    pub fn wl_surface(&self) -> Option<WlSurface> {
        match self {
            Surface::Window(w) => w.toplevel().map(|t| t.wl_surface().clone()),
            Surface::Layer(l) => Some(l.wl_surface().clone()),
            Surface::Lock(l) => Some(l.wl_surface().clone()),
        }
    }

    /// The plane's pixel rectangle: the window geometry, or the surface's view for the roles that
    /// have none.
    pub fn geometry(&self) -> Rectangle<i32, Logical> {
        match self {
            Surface::Window(w) => w.geometry(),
            Surface::Layer(l) => l.geometry(),
            Surface::Lock(l) => with_renderer_surface_state(l.wl_surface(), |s| s.view().map(|v| Rectangle::new(v.offset, v.dst))).flatten().unwrap_or_default(),
        }
    }

    pub fn surface_under(&self, point: Point<f64, Logical>, ty: WindowSurfaceType) -> Option<(WlSurface, Point<i32, Logical>)> {
        match self {
            Surface::Window(w) => w.surface_under(point, ty),
            Surface::Layer(l) => l.surface_under(point, ty),
            Surface::Lock(l) => under_from_surface_tree(l.wl_surface(), point, (0, 0), ty),
        }
    }

    pub fn send_frame<F>(&self, output: &Output, time: Duration, throttle: Option<Duration>, primary: F)
    where
        F: FnMut(&WlSurface, &SurfaceData) -> Option<Output> + Copy,
    {
        match self {
            Surface::Window(w) => w.send_frame(output, time, throttle, primary),
            Surface::Layer(l) => l.send_frame(output, time, throttle, primary),
            Surface::Lock(l) => send_frames_surface_tree(l.wl_surface(), output, time, throttle, primary),
        }
    }

    /// `Window::on_commit` refreshes the window's cached geometry; the other roles cache nothing.
    pub fn on_commit(&self) {
        if let Surface::Window(w) = self {
            w.on_commit();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Frames and their rectangles
// ---------------------------------------------------------------------------------------------

/// The head frame's fallback rectangle and distance (`shell.head.*`; spec §14 — the wearer's
/// placement table decides real placement, this is where unaware clients and head-locked
/// transients land).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HeadCfg {
    pub extent_h_deg: f32,
    pub extent_v_deg: f32,
    pub distance_m: f32,
}

impl Default for HeadCfg {
    fn default() -> Self {
        HeadCfg { extent_h_deg: 90.0, extent_v_deg: 70.0, distance_m: 0.5 }
    }
}

/// A frame's pixel rectangle at its canonical distance: the "output" the arrange algorithm
/// runs in (research/77 §3.1). `ppd` converts pixels to degrees both ways.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameRect {
    pub frame: Frame,
    pub size: Size<i32, Logical>,
    pub ppd: f32,
    pub distance_m: f32,
    /// the non-exclusive zone left after the last arrange (the "usable area")
    pub usable: Rectangle<i32, Logical>,
}

impl FrameRect {
    pub fn extent_deg(&self) -> (f32, f32) {
        (self.size.w as f32 / self.ppd, self.size.h as f32 / self.ppd)
    }
}

/// The pixel width of every frame rectangle: the virtual output's width (`XR-1`, `state.rs`).
pub const FRAME_PX_W: i32 = 1920;

/// The output mode height that makes the output *be* the head rectangle (research/77 §3.3):
/// unaware clients size themselves from the mode, so the mode follows the extents.
pub fn head_mode_size(cfg: &HeadCfg) -> Size<i32, Logical> {
    let ppd = FRAME_PX_W as f32 / cfg.extent_h_deg.max(1.0);
    Size::new(FRAME_PX_W, ((cfg.extent_v_deg.max(1.0) * ppd).round() as i32).max(1))
}

// ---------------------------------------------------------------------------------------------
// The layer members
// ---------------------------------------------------------------------------------------------

/// One layer surface's shell state (the protocol state lives in smithay's cached state; this is
/// what arrangement and placement add).
#[derive(Debug)]
pub struct LayerEntry {
    pub member: MemberId,
    pub surface: LayerSurface,
    pub namespace: String,
    /// creation order: the protocol's "implementation-defined" tie within a layer, and the
    /// exclusive override's "most recently mapped first"
    pub serial: u64,
    /// the frame the surface is presented in this arrange (row > client > head)
    pub frame: Frame,
    /// the arranged pixel box in the frame rectangle
    pub box_px: Option<Rectangle<i32, Logical>>,
    /// mapped = has a buffer (the flatten's `mapped` on the payload is the same fact; kept here
    /// for the focus scan without a scene lookup)
    pub mapped: bool,
    pub mapped_at_serial: u64,
}

/// The shell state on `Zxr`.
#[derive(Default)]
pub struct Shell {
    pub head: HeadCfg,
    /// the wearer's rows (`shell.place:<namespace>`, explicit values only)
    pub rows: std::collections::HashMap<String, PlaceRow>,
    pub layers: Vec<LayerEntry>,
    /// per frame, after the last arrange
    pub rects: Vec<FrameRect>,
    next_serial: u64,
    /// the `Prefs` generation the head config and rows were taken from
    pub prefs_generation: u64,
    /// the frames the anchoring global advertises (a bitfield of `1 << Frame`)
    pub frames_available: u32,
    // counters (spec §11 rev 3.12)
    pub arranges: u64,
    pub configures: u64,
    pub focus_overrides: u64,
    pub last_override: Option<MemberId>,
    pub lock: lock::LockState,
    /// the shell's world anchor: seeded from the first head pose, re-seated on recenter (anchor.rs)
    pub anchor: anchor::Anchor,
    /// a `typed` member hanging below a world-frame window: re-posed when the window moves
    pub typed_follow: Option<TypedFollow>,
    /// the typed surface's member at the last arrange: a change re-arranges (the OSK's frame moves)
    pub typed_member: Option<MemberId>,
}

impl Shell {
    pub fn new() -> Self {
        Shell { frames_available: (1 << Frame::Head as u32) | (1 << Frame::World as u32), ..Default::default() }
    }

    pub fn next_serial(&mut self) -> u64 {
        self.next_serial += 1;
        self.next_serial
    }

    pub fn entry(&self, member: MemberId) -> Option<&LayerEntry> {
        self.layers.iter().find(|e| e.member == member)
    }

    pub fn entry_mut(&mut self, member: MemberId) -> Option<&mut LayerEntry> {
        self.layers.iter_mut().find(|e| e.member == member)
    }

    pub fn entry_for_surface(&self, surface: &WlSurface) -> Option<&LayerEntry> {
        self.layers.iter().find(|e| e.surface.wl_surface() == surface)
    }

    pub fn rect(&self, frame: Frame) -> Option<&FrameRect> {
        self.rects.iter().find(|r| r.frame == frame)
    }

    /// The frame a request resolves to: head and world exist (spec §5; the world for a layer surface
    /// is the shell's anchor, anchor.rs); hand → world and docked → head are the protocol's fallbacks.
    pub fn resolve_frame(&self, wanted: Frame) -> Frame {
        if self.frames_available & (1 << wanted as u32) != 0 {
            return wanted;
        }
        match wanted {
            Frame::HandLeft | Frame::HandRight => Frame::World,
            Frame::Docked | Frame::Head => Frame::Head,
            Frame::World => Frame::World,
        }
    }

    /// The `FrameId` in the scene for a resolved frame: a world-framed layer surface hangs off the
    /// shell's anchor (seeded in front of the wearer, re-seated on recenter — anchor.rs), so the
    /// scene's bare world frame carries only the window tiers' places.
    pub fn frame_id(&self, scene: &crate::scene::Scene<crate::state::Payload>, frame: Frame) -> FrameId {
        match frame {
            Frame::World => scene.anchor,
            _ => scene.head,
        }
    }
}

/// Band of a layer (spec §4): background is the environment's band 1 (accepted, not composed),
/// bottom 2, top 4, overlay 5.
pub fn band_of(layer: Layer) -> u8 {
    match layer {
        Layer::Background => 1,
        Layer::Bottom => 2,
        Layer::Top => 4,
        Layer::Overlay => 5,
    }
}

fn layer_rank(layer: Layer) -> u8 {
    match layer {
        Layer::Overlay => 0,
        Layer::Top => 1,
        Layer::Bottom => 2,
        Layer::Background => 3,
    }
}

/// wlroots' `wlr_layer_surface_v1_get_exclusive_edge`: the client's explicit edge, else a
/// single anchored edge or the odd edge of a three-edge bar; corners and full anchors none.
pub fn exclusive_edge(s: &LayerSurfaceCachedState) -> Option<Anchor> {
    if !matches!(s.exclusive_zone, ExclusiveZone::Exclusive(z) if z > 0) {
        return None;
    }
    if let Some(e) = s.exclusive_edge {
        return Some(e);
    }
    let a = s.anchor;
    let lr = Anchor::LEFT | Anchor::RIGHT;
    let tb = Anchor::TOP | Anchor::BOTTOM;
    if a == Anchor::TOP || a == lr | Anchor::TOP {
        Some(Anchor::TOP)
    } else if a == Anchor::BOTTOM || a == lr | Anchor::BOTTOM {
        Some(Anchor::BOTTOM)
    } else if a == Anchor::LEFT || a == tb | Anchor::LEFT {
        Some(Anchor::LEFT)
    } else if a == Anchor::RIGHT || a == tb | Anchor::RIGHT {
        Some(Anchor::RIGHT)
    } else {
        None
    }
}

/// `wlr_scene_layer_surface_v1_configure`'s box arithmetic (`scene/layer_shell_v1.c:64-106`),
/// pure: the box for one surface given the full and usable rectangles.
pub fn layer_box(s: &LayerSurfaceCachedState, full: Rectangle<i32, Logical>, usable: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let bounds = if matches!(s.exclusive_zone, ExclusiveZone::DontCare) { full } else { usable };
    let m = s.margin;
    let a = s.anchor;
    let (mut w, mut h) = (s.size.w, s.size.h);
    let x = if w == 0 {
        w = (bounds.size.w - (m.left + m.right)).max(0);
        bounds.loc.x + m.left
    } else if a.contains(Anchor::LEFT) && a.contains(Anchor::RIGHT) {
        bounds.loc.x + bounds.size.w / 2 - w / 2
    } else if a.contains(Anchor::LEFT) {
        bounds.loc.x + m.left
    } else if a.contains(Anchor::RIGHT) {
        bounds.loc.x + bounds.size.w - w - m.right
    } else {
        bounds.loc.x + bounds.size.w / 2 - w / 2
    };
    let y = if h == 0 {
        h = (bounds.size.h - (m.top + m.bottom)).max(0);
        bounds.loc.y + m.top
    } else if a.contains(Anchor::TOP) && a.contains(Anchor::BOTTOM) {
        bounds.loc.y + bounds.size.h / 2 - h / 2
    } else if a.contains(Anchor::TOP) {
        bounds.loc.y + m.top
    } else if a.contains(Anchor::BOTTOM) {
        bounds.loc.y + bounds.size.h - h - m.bottom
    } else {
        bounds.loc.y + bounds.size.h / 2 - h / 2
    };
    Rectangle::new((x, y).into(), (w.max(0), h.max(0)).into())
}

/// `layer_surface_exclusive_zone` (`scene/layer_shell_v1.c:23-51`): shrink the usable rectangle
/// on the exclusive edge by `zone + margin`, clamped at zero (wlroots and smithay; never river's
/// kill).
pub fn shrink_usable(s: &LayerSurfaceCachedState, zone_px: i32, usable: &mut Rectangle<i32, Logical>) {
    let m = s.margin;
    match exclusive_edge(s) {
        Some(Anchor::TOP) => {
            let d = zone_px + m.top;
            usable.loc.y += d;
            usable.size.h -= d;
        }
        Some(Anchor::BOTTOM) => usable.size.h -= zone_px + m.bottom,
        Some(Anchor::LEFT) => {
            let d = zone_px + m.left;
            usable.loc.x += d;
            usable.size.w -= d;
        }
        Some(Anchor::RIGHT) => usable.size.w -= zone_px + m.right,
        _ => {}
    }
    usable.size.w = usable.size.w.max(0);
    usable.size.h = usable.size.h.max(0);
}

/// A quaternion about +X (pitch, radians; positive tilts the top away).
pub fn quat_pitch(p: f32) -> xr::Quaternionf {
    let (s, c) = (p * 0.5).sin_cos();
    xr::Quaternionf { x: s, y: 0.0, z: 0.0, w: c }
}

/// The pose in a frame of a plane centred at (azimuth, elevation) degrees and `distance` metres,
/// facing the frame origin, with an extra pitch (degrees) about the plane's own horizontal axis.
pub fn pose_at(az_deg: f32, el_deg: f32, distance: f32, pitch_deg: f32) -> xr::Posef {
    let az = az_deg.to_radians();
    let el = el_deg.to_radians();
    let pos = [distance * az.sin() * el.cos(), distance * el.sin(), -distance * az.cos() * el.cos()];
    // yaw by -az about +Y (a plane to the right turns to face the origin), then pitch up by el
    let yaw = math::pose_yaw([0.0, 0.0, 0.0], -az).orientation;
    let ori = math::quat_mul(yaw, quat_pitch(el + pitch_deg.to_radians()));
    xr::Posef { orientation: ori, position: xr::Vector3f { x: pos[0], y: pos[1], z: pos[2] } }
}

/// Plane extents in metres of `px` pixels at `ppd` and `distance`.
pub fn plane_size_px(px: Size<i32, Logical>, ppd: f32, distance: f32) -> [f32; 2] {
    let ang = |p: i32| (p.max(1) as f32 / ppd).to_radians();
    [2.0 * distance * (ang(px.w) * 0.5).tan(), 2.0 * distance * (ang(px.h) * 0.5).tan()]
}

/// The configuration this arrange reads from the prefs (head config, rows). Called by
/// `settings::apply` and at start.
pub fn take_prefs(st: &mut Zxr) {
    let p = &st.prefs;
    let head = HeadCfg { extent_h_deg: p.shell_head_extent_h_deg.clamp(20.0, 180.0), extent_v_deg: p.shell_head_extent_v_deg.clamp(20.0, 180.0), distance_m: p.shell_head_distance_m.clamp(0.2, 3.0) };
    let changed = head != st.shell.head || st.shell.rows != p.shell_place_rows;
    st.shell.head = head;
    st.shell.rows = p.shell_place_rows.clone();
    st.shell.prefs_generation = p.generation;
    if changed {
        // the virtual output is the head rectangle (research/77 §3.3): its mode follows
        let size = head_mode_size(&head);
        let mode = smithay::output::Mode { size: (size.w, size.h).into(), refresh: 60_000 };
        if st.output.current_mode() != Some(mode) {
            st.output.change_current_state(Some(mode), None, None, None);
            st.output.set_preferred(mode);
        }
        arrange(st);
    }
}

/// The pixel rectangle of a frame this arrange (research/77 §3.1): the head rectangle's extent at
/// the head distance for every frame — the world frame's layer surfaces hang off the shell's
/// anchor at the same distance the head frame's do (rev 3.16; research/77 §9 Q2's spawn-distance
/// world rectangle described world surfaces placed among windows, a case the `typed` rule now
/// covers per window). Exclusive bands apply in both.
fn frame_rect(st: &Zxr, frame: Frame) -> FrameRect {
    let h = st.shell.head;
    let size = head_mode_size(&h);
    let ppd = FRAME_PX_W as f32 / h.extent_h_deg.max(1.0);
    FrameRect { frame, size, ppd, distance_m: h.distance_m, usable: Rectangle::from_size(size) }
}

/// Arrange every layer member (the doc comment above): per frame, two passes in sway's order;
/// the pose and plane of each member are written into the scene, the client is configured with
/// the arranged size when it changed and the initial configure has been sent, and the frames'
/// usable rectangles are kept for the window tiers (`exclusive_occupancy`) and `zxr ctl list`.
pub fn arrange(st: &mut Zxr) {
    st.shell.arranges += 1;
    st.journal.layer_arranges += 1;
    st.shell.typed_follow = None;
    // resolve every entry's frame first (row > client > seed > world)
    let typed = typed_target(st);
    st.shell.typed_member = typed.map(|t| t.member);
    let mut typed_members: Vec<MemberId> = Vec::new();
    let frames: Vec<Frame> = {
        let mut fs = Vec::new();
        for i in 0..st.shell.layers.len() {
            let e = &st.shell.layers[i];
            // the wearer's row > the client's request > the seed row > world (shell-plane §2.6:
            // seeds are the defaults for clients that ask nothing — the unaware ones; the world
            // because nothing the wearer aims at is head-locked, spatial-input §13)
            let asked = st.shell.rows.get(&e.namespace).and_then(|r| r.frame).or_else(|| anchoring::requested_frame(e.surface.wl_surface()).map(PlaceFrame::Frame)).or_else(|| place::seed(&e.namespace).and_then(|r| r.frame));
            let wanted = match asked {
                Some(PlaceFrame::Frame(f)) => f,
                // `typed`: the frame of the surface being typed into; without one, where it was
                Some(PlaceFrame::Typed) => {
                    typed_members.push(e.member);
                    typed.map(|t| t.frame).unwrap_or(e.frame)
                }
                None => Frame::World,
            };
            let f = st.shell.resolve_frame(wanted);
            st.shell.layers[i].frame = f;
            // the member's place follows the frame; a `typed` member hanging below a world *window*
            // is posed in world coordinates (`typed_pose`, `typed_tick`), so its place is the bare
            // world frame rather than the shell's anchor
            let member = st.shell.layers[i].member;
            let under_window = typed.map(|t| t.window).unwrap_or(false) && typed_members.contains(&member);
            let fid = if under_window { st.scene.world } else { st.shell.frame_id(&st.scene, f) };
            if let Some(place) = st.scene.get(member).map(|m| m.place) {
                st.scene.reparent_place(place, fid);
            }
            if !fs.contains(&f) {
                fs.push(f);
            }
        }
        // the head and world rectangles always exist: the lock surface and the window tiers read them
        for f in [Frame::Head, Frame::World] {
            if !fs.contains(&f) {
                fs.push(f);
            }
        }
        fs
    };
    let mut rects: Vec<FrameRect> = Vec::with_capacity(frames.len());
    for frame in frames {
        let mut rect = frame_rect(st, frame);
        let full = Rectangle::from_size(rect.size);
        let mut usable = full;
        // sway's order: exclusive pass then the rest; overlay → top → bottom → background;
        // creation order within a layer
        let mut order: Vec<(bool, u8, u64, usize)> = st
            .shell
            .layers
            .iter()
            .enumerate()
            .filter(|(_, e)| e.frame == frame)
            .map(|(i, e)| {
                let s = e.surface.cached_state();
                let excl = matches!(s.exclusive_zone, ExclusiveZone::Exclusive(z) if z > 0);
                (!excl, layer_rank(s.layer), e.serial, i)
            })
            .collect();
        order.sort();
        for (_, _, _, i) in order {
            let (surface, member, namespace) = {
                let e = &st.shell.layers[i];
                (e.surface.clone(), e.member, e.namespace.clone())
            };
            let s = surface.cached_state();
            // a `typed` member under a world window is arranged against the *window's* rectangle
            // (WiVRn sizes its keyboard to its GUI; a 1920-px keyboard beside a 700-px window is
            // not "bound to the panel"), and reserves nothing from the world frame
            let typed_window = typed.filter(|t| t.window && typed_members.contains(&member));
            let window_rect = typed_window.and_then(|t| st.logical_size(t.member)).map(|(w, h)| Rectangle::from_size((w, h).into()));
            let bx = match window_rect {
                Some(wr) => layer_box(&s, wr, wr),
                None => layer_box(&s, full, usable),
            };
            // the exclusive band: the client's zone, or its exclusive angle in the same units
            let zone_px = match s.exclusive_zone {
                ExclusiveZone::Exclusive(z) => {
                    let ang = anchoring::exclusive_angle(surface.wl_surface());
                    if ang > 0.0 {
                        (ang * rect.ppd).round() as i32
                    } else {
                        z as i32
                    }
                }
                _ => 0,
            };
            let mapped = st.shell.layers[i].mapped;
            if mapped && zone_px > 0 && window_rect.is_none() {
                shrink_usable(&s, zone_px, &mut usable);
            }
            // the size the client is asked for (arrange → configure, research/77 §2.2)
            let size_changed = surface.layer_surface().with_pending_state(|p| {
                let new = Some(bx.size);
                let changed = p.size != new;
                p.size = new;
                changed
            });
            let initial_sent = with_states(surface.wl_surface(), |states| states.data_map.get::<LayerSurfaceData>().map(|d| d.lock().unwrap().initial_configure_sent).unwrap_or(false));
            if size_changed && initial_sent {
                surface.layer_surface().send_pending_configure();
                st.shell.configures += 1;
                st.journal.layer_configures += 1;
            }
            st.shell.layers[i].box_px = Some(bx);
            // the pose: the row's placement, else the arranged box's centre in the frame
            // the wearer's row > the client's request > the seed row > the arranged box's centre
            let client_width = anchoring::requested_angular_size(surface.wl_surface());
            let client_pose = if frame == Frame::World { anchoring::requested_pose(surface.wl_surface()) } else { None };
            let client_asked = anchoring::requested_frame(surface.wl_surface()).is_some() || client_pose.is_some() || client_width > 0.0;
            let row = st.shell.rows.get(&namespace).cloned().or_else(|| if client_asked { None } else { place::seed(&namespace) });
            let (caz, cel) = box_centre_deg(bx, &rect);
            let r = row.clone().unwrap_or_default();
            let (az, el, dist, pitch) = (r.azimuth_deg.unwrap_or(caz), r.elevation_deg.unwrap_or(cel), r.distance_m.unwrap_or(rect.distance_m), r.pitch_deg.unwrap_or(0.0));
            let width_deg = r.width_deg.filter(|w| *w > 0.0).unwrap_or(client_width);
            let mut size_m = plane_size_px(bx.size, rect.ppd, dist);
            if width_deg > 0.0 && bx.size.w > 0 {
                let w = 2.0 * dist * (width_deg.to_radians() * 0.5).tan();
                size_m = [w, w * bx.size.h as f32 / bx.size.w as f32];
            }
            let mut pose = match (row.is_some(), client_pose) {
                (false, Some(p)) => p,
                _ => pose_at(az, el, dist, pitch),
            };
            // a `typed` member under a world-frame window hangs below that window (WiVRn's
            // offset); its plane is sized at the window's distance so its pixels match the frame's
            if let Some(t) = typed_window {
                {
                    if let (Some(win), Some(wsize)) = (st.scene.world_pose(t.member), st.scene.get(t.member).and_then(|m| match m.shape { Shape::Plane { size } => Some(size), _ => None })) {
                        let head = st.input.head.map(|h| [h.position.x, h.position.y, h.position.z]).unwrap_or([0.0; 3]);
                        let d = [win.position.x - head[0], win.position.y - head[1], win.position.z - head[2]];
                        let wdist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(0.2);
                        size_m = plane_size_px(bx.size, rect.ppd, wdist);
                        pose = typed_pose(win, wsize, size_m);
                        st.shell.typed_follow = Some(TypedFollow { osk: member, window: t.member, last: win });
                    }
                }
            }
            st.scene.set_local(member, pose);
            st.scene.set_shape(member, Shape::Plane { size: size_m });
            // the frame extent event when it changed (anchoring.rs keeps the last sent)
            let (eh, ev) = rect.extent_deg();
            anchoring::send_extent(surface.wl_surface(), eh, ev);
        }
        rect.usable = usable;
        rects.push(rect);
    }
    st.shell.rects = rects;
    // the exclusive override may have moved (a mapped/unmapped exclusive surface)
    crate::input::focus::layer_focus_changed(st);
}

/// The surface being typed into, as the arrangement sees it (`PlaceFrame::Typed`; research/36 §7:
/// every shipping keyboard is bound to the panel with the focused field). smithay's *active*
/// text input (an enabled `zwp_text_input_v3`) names the surface; its member gives the frame —
/// a layer member's arranged frame, a window's the world, the lock surface's the world (anchor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypedTarget {
    pub member: MemberId,
    pub frame: Frame,
    /// a window in the world: the typed member hangs below it (`typed_pose`)
    pub window: bool,
}

pub fn typed_target(st: &Zxr) -> Option<TypedTarget> {
    let mut typed: Option<WlSurface> = None;
    st.seat.text_input().with_active_text_input(|_, surface| {
        if typed.is_none() {
            typed = Some(surface.clone());
        }
    });
    let member = st.member_for_root(&typed?)?;
    if let Some(e) = st.shell.entry(member) {
        return Some(TypedTarget { member, frame: e.frame, window: false });
    }
    let m = st.scene.get(member)?;
    if m.m.window.is_window() {
        Some(TypedTarget { member, frame: Frame::World, window: true })
    } else {
        Some(TypedTarget { member, frame: Frame::World, window: false })
    }
}

/// A `typed` member hanging below a world-frame window, and the window pose it was posed for.
#[derive(Clone, Copy, Debug)]
pub struct TypedFollow {
    pub osk: MemberId,
    pub window: MemberId,
    pub last: xr::Posef,
}

/// WiVRn's keyboard offset from the panel it types into (`wivrn/client/constants.h:87-88`:
/// `keyboard_position = (0, −0.3, 0.1)`, `keyboard_pitch = −0.6` rad): hinged below the panel's
/// bottom edge with a small gap, brought 0.1 m toward the wearer, tilted −0.6 rad to face them.
/// The offsets scale with the two planes' heights rather than WiVRn's fixed GUI.
pub const TYPED_GAP_M: f32 = 0.05;
pub const TYPED_TOWARD_M: f32 = 0.1;
pub const TYPED_PITCH_RAD: f32 = -0.6;

pub fn typed_pose(window: xr::Posef, window_size: [f32; 2], osk_size: [f32; 2]) -> xr::Posef {
    let kh = osk_size[1] * 0.5;
    // the OSK's centre, in the window's plane space: below the bottom edge by the gap and the
    // pitched half-height, and out toward the wearer (+Z is toward the wearer for a facing plane)
    let local = [0.0, -(window_size[1] * 0.5 + TYPED_GAP_M + kh * TYPED_PITCH_RAD.cos()), TYPED_TOWARD_M + kh * (-TYPED_PITCH_RAD).sin()];
    let p = math::pose_apply(window, local);
    xr::Posef { orientation: math::quat_mul(window.orientation, quat_pitch(TYPED_PITCH_RAD)), position: xr::Vector3f { x: p[0], y: p[1], z: p[2] } }
}

/// Per tick: a `typed` member follows the window it hangs below when that window moves (a grab,
/// a lazy-follow). One pose compare; a re-pose only on change.
pub fn typed_tick(st: &mut Zxr) {
    // the typed surface changed (a field focused in another window): the typed members' frame
    // follows it — one arrange, only when a layer member exists to move
    if !st.shell.layers.is_empty() {
        let now = typed_target(st).map(|t| t.member);
        if now.is_some() && now != st.shell.typed_member {
            st.shell.typed_member = now;
            arrange(st);
        }
    }
    let Some(f) = st.shell.typed_follow else { return };
    let Some(win) = st.scene.world_pose(f.window) else { return };
    let moved = |a: xr::Posef, b: xr::Posef| {
        (a.position.x - b.position.x).abs() > 1e-4 || (a.position.y - b.position.y).abs() > 1e-4 || (a.position.z - b.position.z).abs() > 1e-4 || (a.orientation.x - b.orientation.x).abs() > 1e-4 || (a.orientation.y - b.orientation.y).abs() > 1e-4 || (a.orientation.z - b.orientation.z).abs() > 1e-4 || (a.orientation.w - b.orientation.w).abs() > 1e-4
    };
    if !moved(win, f.last) {
        return;
    }
    let (Some(wsize), Some(osize)) = (
        st.scene.get(f.window).and_then(|m| match m.shape { Shape::Plane { size } => Some(size), _ => None }),
        st.scene.get(f.osk).and_then(|m| match m.shape { Shape::Plane { size } => Some(size), _ => None }),
    ) else { return };
    st.scene.set_local(f.osk, typed_pose(win, wsize, osize));
    if let Some(tf) = st.shell.typed_follow.as_mut() {
        tf.last = win;
    }
    st.journal.osk_follows += 1;
}

fn box_centre_deg(bx: Rectangle<i32, Logical>, rect: &FrameRect) -> (f32, f32) {
    let cx = bx.loc.x as f32 + bx.size.w as f32 * 0.5 - rect.size.w as f32 * 0.5;
    let cy = rect.size.h as f32 * 0.5 - (bx.loc.y as f32 + bx.size.h as f32 * 0.5);
    (cx / rect.ppd, cy / rect.ppd)
}

/// The head and world (anchor) frames' exclusive bands as the window tiers see them (spec §4: the
/// tiers honour the shell frames' usable rectangles at spawn): the reserved strips, in
/// head-relative angles, for the free engine's occupancy pass. The anchor is upright at the
/// heading it was seeded with (anchor.rs), so its strips are the same arithmetic turned by the yaw
/// the head currently differs from it; the free engine's basis is the head's horizontal forward.
pub fn exclusive_occupancy(st: &Zxr) -> Vec<crate::policy::free::AngularBounds> {
    let mut out = Vec::new();
    let head_yaw = st.input.head.map(|h| anchor::yaw_of(h.orientation)).unwrap_or(0.0);
    // an anchor-frame azimuth `a` (positive right) sits at head-azimuth `a + (head_yaw − anchor_yaw)`:
    // a head turned left of the anchor sees the anchor's forward to its right
    let anchor_offset_deg = if st.shell.anchor.seated { anchor::yaw_delta(st.shell.anchor.yaw, head_yaw).to_degrees() } else { 0.0 };
    for (frame, az_offset) in [(Frame::Head, 0.0), (Frame::World, anchor_offset_deg)] {
        let Some(r) = st.shell.rect(frame) else { continue };
        frame_strips(r, az_offset, &mut out);
    }
    out
}

fn frame_strips(r: &FrameRect, az_offset_deg: f32, out: &mut Vec<crate::policy::free::AngularBounds>) {
    let full = Rectangle::from_size(r.size);
    let u = r.usable;
    if u == full {
        return;
    }
    let deg = |px: i32| px as f32 / r.ppd;
    let (hw, hh) = (deg(full.size.w) * 0.5, deg(full.size.h) * 0.5);
    // one strip per shrunk edge, in radians (the allocator's unit)
    let strip = |c_az: f32, c_el: f32, h_az: f32, h_el: f32| crate::policy::free::AngularBounds { centre_az: (c_az + az_offset_deg).to_radians(), centre_el: c_el.to_radians(), half_az: h_az.to_radians(), half_el: h_el.to_radians() };
    if u.loc.y > 0 {
        let d = deg(u.loc.y);
        out.push(strip(0.0, hh - d * 0.5, hw, d * 0.5));
    }
    let bottom = full.size.h - (u.loc.y + u.size.h);
    if bottom > 0 {
        let d = deg(bottom);
        out.push(strip(0.0, -(hh - d * 0.5), hw, d * 0.5));
    }
    if u.loc.x > 0 {
        let d = deg(u.loc.x);
        out.push(strip(-(hw - d * 0.5), 0.0, d * 0.5, hh));
    }
    let right = full.size.w - (u.loc.x + u.size.w);
    if right > 0 {
        let d = deg(right);
        out.push(strip(hw - d * 0.5, 0.0, d * 0.5, hh));
    }
}

/// The exclusive keyboard override (spec §8 rev 3.12; research/77 §4.2): the topmost mapped
/// `exclusive` surface on overlay, then top — most recently mapped first; on bottom/background
/// only while no window is mapped (niri `niri.rs:1354-1366`).
pub fn exclusive_override(st: &Zxr) -> Option<MemberId> {
    let pick = |layers: &[Layer]| {
        let mut best: Option<(u8, u64, MemberId)> = None;
        for e in &st.shell.layers {
            if !e.mapped {
                continue;
            }
            let s = e.surface.cached_state();
            if s.keyboard_interactivity != KeyboardInteractivity::Exclusive || !layers.contains(&s.layer) {
                continue;
            }
            let key = (layer_rank(s.layer), u64::MAX - e.mapped_at_serial, e.member);
            if best.map(|b| (key.0, key.1) < (b.0, b.1)).unwrap_or(true) {
                best = Some(key);
            }
        }
        best.map(|b| b.2)
    };
    if let Some(m) = pick(&[Layer::Overlay, Layer::Top]) {
        return Some(m);
    }
    let any_window = st.scene.iter().any(|(_, m)| m.m.window.is_window() && m.m.mapped());
    if any_window {
        return None;
    }
    pick(&[Layer::Bottom, Layer::Background])
}

/// Whether `member` is a layer surface that may hold keyboard focus by its own request
/// (`exclusive` or `on_demand`) — the stack admits it (research/77 §4.2).
pub fn layer_accepts_focus(st: &Zxr, member: MemberId) -> Option<bool> {
    let e = st.shell.entry(member)?;
    Some(e.surface.cached_state().keyboard_interactivity != KeyboardInteractivity::None)
}

/// `zxr ctl list`'s `shell:` and `zone:` lines (spec §11 rev 3.12).
pub fn describe(st: &Zxr) -> String {
    let mut s = String::new();
    for e in &st.shell.layers {
        let c = e.surface.cached_state();
        let bx = e.box_px.map(|b| format!("{}x{}+{}+{}", b.size.w, b.size.h, b.loc.x, b.loc.y)).unwrap_or_else(|| "-".into());
        let edge = exclusive_edge(&c).map(|a| format!("{a:?}")).unwrap_or_else(|| "none".into());
        let zone = match c.exclusive_zone {
            ExclusiveZone::Exclusive(z) => z.to_string(),
            ExclusiveZone::Neutral => "0".into(),
            ExclusiveZone::DontCare => "-1".into(),
        };
        let trusted = st.scene.get(e.member).map(|m| m.m.trusted).unwrap_or(false);
        s.push_str(&format!(
            "shell: member={} ns={} layer={:?} frame={} box={bx} anchor={:?} edge={edge} zone={zone} kb={:?} mapped={} trusted={trusted}\n",
            e.member.0.index(),
            e.namespace,
            c.layer,
            e.frame.name(),
            c.anchor,
            c.keyboard_interactivity,
            e.mapped
        ));
    }
    for r in &st.shell.rects {
        let (eh, ev) = r.extent_deg();
        s.push_str(&format!("zone: frame={:?} rect={}x{} extent={:.1}x{:.1}deg ppd={:.2} distance={:.2} usable={}x{}+{}+{}\n", r.frame, r.size.w, r.size.h, eh, ev, r.ppd, r.distance_m, r.usable.size.w, r.usable.size.h, r.usable.loc.x, r.usable.loc.y));
    }
    s.push_str(&format!(
        "shell-counters: layers={} arranges={} configures={} focus_overrides={} override={:?} restricted={} trusted={} trusted_lost={} binds_filtered={} motion_deduped={} lock={:?} relocks={} triggers={} mode={:?} frames={} members_composed={} osk_band={:?} osk_raised={:?} osk_raises={} osk_restarts={} anchor_yaw_deg={:.1} anchor_reseats={} typed={:?} osk_follows={}\n",
        st.shell.layers.len(),
        st.shell.arranges,
        st.shell.configures,
        st.shell.focus_overrides,
        st.shell.last_override.map(|m| m.0.index()),
        st.journal.clients_restricted,
        st.journal.clients_trusted,
        st.journal.trusted_lost,
        st.journal.binds_filtered,
        st.journal.pointer_motion_deduped,
        st.shell.lock.status(),
        st.journal.lock_relocks,
        st.journal.lock_triggers,
        st.input.mode,
        st.journal.frames,
        st.journal.members_composed,
        st.shell.layers.iter().find(|e| e.namespace == "osk").and_then(|e| st.scene.band(e.member)),
        st.shell.layers.iter().find(|e| e.namespace == "osk").and_then(|e| st.scene.raised(e.member)),
        st.journal.osk_raises,
        st.journal.osk_restarts,
        st.shell.anchor.yaw.to_degrees(),
        st.journal.anchor_reseats,
        st.shell.typed_member.map(|m| m.0.index()),
        st.journal.osk_follows
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::wayland::shell::wlr_layer::Margins;

    fn state(size: (i32, i32), anchor: Anchor, zone: ExclusiveZone, margin: Margins) -> LayerSurfaceCachedState {
        LayerSurfaceCachedState { size: size.into(), anchor, exclusive_zone: zone, exclusive_edge: None, margin, keyboard_interactivity: KeyboardInteractivity::None, layer: Layer::Top, last_acked: None }
    }

    fn m0() -> Margins {
        Margins { top: 0, right: 0, bottom: 0, left: 0 }
    }

    /// The OSK under a world window (WiVRn's offset): below its bottom edge, toward the wearer,
    /// pitched to face them; it moves with the window.
    #[test]
    fn typed_pose_hangs_below_the_window_and_tilts_toward_the_wearer() {
        // a 1.0 × 0.6 m window facing the wearer (+Z) at 1.5 m
        let win = xr::Posef { orientation: xr::Quaternionf::IDENTITY, position: xr::Vector3f { x: 0.2, y: 1.4, z: -1.5 } };
        let osk = [1.0, 0.3];
        let p = typed_pose(win, [1.0, 0.6], osk);
        assert!((p.position.x - 0.2).abs() < 1e-6, "centred under the window");
        assert!(p.position.y < 1.4 - 0.3 - TYPED_GAP_M, "below the bottom edge: {}", p.position.y);
        assert!(p.position.z > -1.5 + TYPED_TOWARD_M - 1e-6, "brought toward the wearer: {}", p.position.z);
        // the plane's normal (+Z) now points up-and-toward: a negative pitch about +X lifts it
        let n = math::rotate(p.orientation, [0.0, 0.0, 1.0]);
        assert!(n[1] > 0.5 && n[2] > 0.5, "tilted toward a wearer looking down: {n:?}");
        // a translated window carries the OSK with it
        let win2 = xr::Posef { position: xr::Vector3f { x: 0.7, ..win.position }, ..win };
        let p2 = typed_pose(win2, [1.0, 0.6], osk);
        assert!((p2.position.x - p.position.x - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bottom_bar_stretches_and_reserves() {
        // squeekboard's shape: width 0, height 360, bottom|left|right, zone 360
        let full = Rectangle::from_size((1920, 1493).into());
        let s = state((0, 360), Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT, ExclusiveZone::Exclusive(360), m0());
        let bx = layer_box(&s, full, full);
        assert_eq!(bx, Rectangle::new((0, 1493 - 360).into(), (1920, 360).into()));
        assert_eq!(exclusive_edge(&s), Some(Anchor::BOTTOM));
        let mut usable = full;
        shrink_usable(&s, 360, &mut usable);
        assert_eq!(usable, Rectangle::from_size((1920, 1133).into()));
    }

    #[test]
    fn top_right_toast_is_placed_inside_the_usable_area() {
        // mako's shape: fixed size, top|right, zone 0 (Neutral) → inside what a top bar left
        let full = Rectangle::from_size((1920, 1493).into());
        let usable = Rectangle::new((0, 40).into(), (1920, 1453).into());
        let s = state((300, 100), Anchor::TOP | Anchor::RIGHT, ExclusiveZone::Neutral, Margins { top: 10, right: 10, bottom: 0, left: 0 });
        let bx = layer_box(&s, full, usable);
        assert_eq!(bx, Rectangle::new((1920 - 300 - 10, 50).into(), (300, 100).into()));
        assert_eq!(exclusive_edge(&s), None);
    }

    #[test]
    fn dont_care_uses_the_full_rectangle_and_corners_reserve_nothing() {
        let full = Rectangle::from_size((1920, 1493).into());
        let usable = Rectangle::new((0, 200).into(), (1920, 1000).into());
        // a lock screen: all anchors, size 0, zone −1 → the full rectangle
        let lock = state((0, 0), Anchor::all(), ExclusiveZone::DontCare, m0());
        assert_eq!(layer_box(&lock, full, usable), full);
        // a corner widget with a positive zone reserves nothing (protocol §set_exclusive_zone)
        let corner = state((100, 100), Anchor::TOP | Anchor::LEFT, ExclusiveZone::Exclusive(50), m0());
        assert_eq!(exclusive_edge(&corner), None);
        let mut u = full;
        shrink_usable(&corner, 50, &mut u);
        assert_eq!(u, full);
    }

    #[test]
    fn bogus_zones_clamp_never_go_negative() {
        let full = Rectangle::from_size((1920, 1493).into());
        let s = state((0, 100), Anchor::TOP | Anchor::LEFT | Anchor::RIGHT, ExclusiveZone::Exclusive(5000), m0());
        let mut u = full;
        shrink_usable(&s, 5000, &mut u);
        assert_eq!(u.size.h, 0);
        assert_eq!(u.loc.y, 5000);
    }

    #[test]
    fn head_mode_follows_the_extents() {
        let m = head_mode_size(&HeadCfg::default());
        assert_eq!(m.w, 1920);
        assert_eq!(m.h, 1493);
        let (eh, ev) = FrameRect { frame: Frame::Head, size: m, ppd: 1920.0 / 90.0, distance_m: 0.5, usable: Rectangle::from_size(m) }.extent_deg();
        assert!((eh - 90.0).abs() < 0.01 && (ev - 70.0).abs() < 0.05);
    }

    #[test]
    fn pose_at_faces_the_origin() {
        let p = pose_at(30.0, 0.0, 0.5, 0.0);
        // to the right and ahead at 0.5 m
        assert!((p.position.x - 0.25).abs() < 1e-3 && (p.position.z + 0.433).abs() < 1e-3);
        // the plane's normal (+Z in plane space) points back at the origin
        let n = math::rotate(p.orientation, [0.0, 0.0, 1.0]);
        let to_origin = [-p.position.x / 0.5, 0.0, -p.position.z / 0.5];
        assert!((n[0] - to_origin[0]).abs() < 1e-3 && (n[2] - to_origin[2]).abs() < 1e-3);
        let low = pose_at(0.0, -30.0, 0.5, 0.0);
        assert!(low.position.y < 0.0);
        let n = math::rotate(low.orientation, [0.0, 0.0, 1.0]);
        assert!(n[1] > 0.0, "a low plane tilts up toward the head: {n:?}");
    }

    #[test]
    fn plane_size_from_pixels() {
        let s = plane_size_px((1920, 1493).into(), 1920.0 / 90.0, 0.5);
        // 90° at 0.5 m is 1 m wide
        assert!((s[0] - 1.0).abs() < 1e-3);
    }
}

/// The OSK above the surface it types into — phoc's rule (`references/phoc/src/layer-shell.c:446-499`
/// `phoc_layer_shell_update_osk`: while the focused layer surface's layer is ≥ the `osk`
/// surface's and the input method is enabled on it, the OSK is composed on `overlay`, "as
/// otherwise keyboard input isn't possible"; re-evaluated on every arrange, `:290-293`). zxr's
/// terms: the surface with the **active text input** (smithay's word for an enabled
/// `zwp_text_input_v3`) is a member with a band; while that band is ≥ the OSK's own, the OSK
/// member's place moves to that band, **raised** within it (`Place::raised`: drawn last, hit
/// first among coplanar planes — phoc's `overlay` with wlroots' later-on-top order); otherwise
/// its own layer's band, not raised. A lock surface (band 5) and an `overlay` greeter (5) both
/// raise a `top` OSK (4) into 5; an xdg toplevel (3) raises nothing. Band 6 is the compositor's
/// own (the cursor) and stays out of reach. Per tick: one scan of the layer list and one mutex
/// probe.
pub fn update_osk_band(st: &mut Zxr) {
    let Some(i) = st.shell.layers.iter().position(|e| e.namespace == "osk" && e.mapped) else { return };
    let (osk_member, own_band) = {
        let e = &st.shell.layers[i];
        (e.member, band_of(e.surface.cached_state().layer))
    };
    let Some(place) = st.scene.get(osk_member).map(|m| m.place) else { return };
    let mut typed: Option<WlSurface> = None;
    st.seat.text_input().with_active_text_input(|_, surface| {
        if typed.is_none() {
            typed = Some(surface.clone());
        }
    });
    let target_band = typed.and_then(|s| st.member_for_root(&s)).filter(|m| *m != osk_member).and_then(|m| st.scene.band(m));
    let (want, raised) = match target_band {
        Some(b) if b >= own_band => (b.min(5), true),
        _ => (own_band, false),
    };
    if st.scene.band(osk_member) != Some(want) {
        st.scene.set_place_band(place, want);
    }
    if st.scene.raised(osk_member) != Some(raised) {
        st.scene.set_place_raised(place, raised);
        if raised {
            st.journal.osk_raises += 1;
            tracing::info!(band = want, own = own_band, "OSK raised above the surface it types into (phoc's rule)");
        }
    }
}
