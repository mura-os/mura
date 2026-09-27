//! Cursors (spatial-input §7, ruled; research/63 §7 "The cursor"; research/70 §9 one layer).
//!
//! | source class | drawn |
//! |---|---|
//! | gaze (touch-class) | nothing — plane-level emphasis (`emphasis.rs`) |
//! | hand ray, head ray (touch-class); poke | a compositor **reticle** at the hit, sized in visual angle |
//! | controller ray (pointer-class) | the reticle **plus** the client's cursor meaning, one image |
//! | mouse / trackpad (pointer-class) | the pointer-class cursor on the plane |
//!
//! (spatial-input.md §7.) "`cursor-shape-v1` is preferred so the compositor renders one
//! theme at one scale across applications (the protocol's own stated reason)"
//! (`references/smithay/src/wayland/cursor_shape.rs:1-4`; `cursor-shape-v1.xml:27-29`); else the
//! client's `wl_pointer.set_cursor` surface is drawn on the plane with its hotspot (wxrc
//! `src/input.c:448-512`, motorcar, Simula — research/63 §7).
//!
//! **One cursor layer at a time (ruled 2026-09-27, research/70 §9).** The seat has one logical
//! pointer (§5, ADR 0013 item 4), so it has one cursor element: the client's cursor when the
//! pointer is on a plane, the ray's reticle otherwise, and for a ray that owns the pointer the
//! ring composited around the image in the same panel. Never two layers — a layer costs the
//! runtime's squasher a pass per view per frame and a slot of the layer budget (research/65 §2.1;
//! Quest publishes ~0.1 ms and 16 layers [external], research/67 §1). The cursor-plane shape:
//! one fixed-size plane, one image, composited by whoever scans out (DRM `cursor` planes; kwin-vr
//! `VrKwinCursor.qml:20-41` is one node whose texture is rebuilt only on `currentCursorChanged`).
//! While a **mouse** owns the pointer on a plane, no ray reticle is shown at all (the pointer is
//! the targeting feedback; the head ray's look changes no focus, §6, and only decides where the
//! pointer warps, §8) — the owner's ruling on §7's "as above". Under **gaze** there is nothing
//! to rule here: the pointer transport releases a ray-owned pointer when gaze takes the tier
//! (§5, `pointer.rs` `release_for_gaze`) and the seat clears the reticle, so no plane and no
//! hit resolve to no layer; a mouse-owned pointer is not released and keeps its cursor.
//! Two per-user preferences (§14, `runtime`): [`RayCursor`] — what a ray that owns the pointer
//! shows (ring around the image, image, or ring; default both) — and [`Scale`] — a constant
//! visual angle or the plane's pixels (default angle). Both reach the seat through the control
//! socket (`zxr ctl cursor ray|scale …`) until `org.mura.Settings1` carries them.
//!
//! This file is the **state**: what the one layer is this tick ([`Cursors::layer`]) — its world
//! pose, its scale, its content and the key the frame procedure compares to redraw — and the one
//! procedural ring. smithay delivers both `cursor-shape-v1` names and `set_cursor` surfaces
//! through `SeatHandler::cursor_image` as one `CursorImageStatus` (`input/pointer/cursor_image.rs:33-43`).
//! The frame procedure (`main.rs`) owns the one `CURSOR_PX`² swapchain, draws into it on a key
//! change only, and submits the one band-5 quad.
//!
//! **Hidden while typing:** a key sample hides the pointer-class cursor; motion shows it again
//! (the desktops' behaviour; GNOME/KDE hide the cursor on key press when the setting is on —
//! here it is the rule because a plane-mounted cursor over text is what the design's §8 keyboard
//! rule is about — flagged as a judgment). A ray that owns the pointer keeps its ring meanwhile.
//!
//! **Stand-ins (flagged, research/70 §5):** the layer's 64 px span subtends **1.5°** of visual
//! angle at the point's distance — the brief's number; HoloLens' ≥ 2° is a *target* size, MRTK3's
//! reticle scales with distance (`MRTKRayReticleVisual.cs:161-166`) without a stated angle. The
//! client image is drawn at its theme pixel size inside that span (a 24–32 px cursor ≈ 0.6–0.75°),
//! so the whole layer follows §7's dynamic-scale rule. The layer is lifted **1 mm** off the plane
//! (kwin-vr 15 mm, motorcar 10 mm; no comparable states a reason).
//!
//! Budget: one 64×64 ring rendered once (frame procedure); a few scalars per tick; no allocation.

use openxr as xr;
use smithay::input::pointer::CursorImageStatus;

use super::SourceKind;
use crate::xr::math;

/// Visual angle (degrees, diameter) the layer's `RETICLE_PX` span subtends at the point's
/// distance (stand-in — flagged).
pub const RETICLE_DEG: f32 = 1.5;
/// The ring texture's side in pixels; also the pixel span that subtends `RETICLE_DEG`.
pub const RETICLE_PX: u32 = 64;
/// The fixed side of the cursor panel (grow-only if a client image needs more around its
/// hotspot). The DRM cursor plane's shape: one fixed-size plane the image is drawn into.
pub const CURSOR_PX: u32 = 64;
/// Offset along the plane normal toward the viewer so the layer sits on, not in, the plane.
pub const CURSOR_LIFT_M: f32 = 0.001;

/// Which class of cursor is current — the §7 rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    /// gaze: nothing
    #[default]
    None,
    /// hand / head ray, poke: the reticle alone
    Reticle,
    /// controller ray owning the pointer: reticle plus the client's cursor meaning
    ReticleAndClient,
    /// mouse / trackpad: the pointer-class cursor
    Client,
}

/// What the one layer's panel holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    /// the procedural ring, centred
    Ring,
    /// the client's image, its hotspot at the centre
    Image,
    /// both: the ring around the image
    RingAndImage,
}

impl Content {
    pub fn has_ring(self) -> bool {
        matches!(self, Content::Ring | Content::RingAndImage)
    }
    pub fn has_image(self) -> bool {
        matches!(self, Content::Image | Content::RingAndImage)
    }
}

/// What a ray that owns the pointer shows (`input.cursor.ray`, spatial-input §14; the owner's
/// ruling 2026-09-27: `both` by default, a per-user preference). Applies only while a ray owns
/// the pointer; a mouse always shows the client's image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RayCursor {
    /// the ring composited around the client's image (§7's "reticle plus the client's cursor meaning")
    #[default]
    Both,
    /// the client's image alone — the desktop look; typing or `Hidden` then leaves nothing
    Image,
    /// the ring alone
    Ring,
}

impl RayCursor {
    pub fn parse(s: &str) -> Option<RayCursor> {
        Some(match s {
            "both" => RayCursor::Both,
            "image" => RayCursor::Image,
            "ring" => RayCursor::Ring,
            _ => return None,
        })
    }
}

/// How the cursor layer is scaled (`input.cursor.scale`, §14). `Angle`: `RETICLE_PX` px subtend
/// `RETICLE_DEG` at the point's distance — constant apparent size (visionOS's dynamic scale
/// [external]; MRTK3's reticle) — the default. `Plane`: the plane's own pixel scale (`M_PER_PX`),
/// so the cursor is proportioned to the content and shrinks with the window's distance (kwin-vr
/// `VrKwinCursor.qml:14,34`, the desktop model). Two shipping positions; a preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Scale {
    #[default]
    Angle,
    Plane,
}

impl Scale {
    pub fn parse(s: &str) -> Option<Scale> {
        Some(match s {
            "angle" => Scale::Angle,
            "plane" => Scale::Plane,
            _ => return None,
        })
    }
}
/// The one cursor layer this tick.
#[derive(Clone, Debug)]
pub struct CursorLayer {
    /// world pose of the layer's centre — the pointer point or the hit, on the plane's
    /// orientation, lifted `CURSOR_LIFT_M` toward the viewer
    pub pose: xr::Posef,
    /// metres per panel pixel at this distance (`RETICLE_PX` px = `RETICLE_DEG`); the quad's
    /// size is this times the panel's side
    pub m_per_px: f32,
    pub content: Content,
    /// the client image to draw when `content.has_image()`
    pub image: Option<CursorImageStatus>,
}

/// The logical pointer's position on a plane this tick (seat stage → `Cursors`).
#[derive(Clone, Copy, Debug)]
pub struct PointerPoint {
    /// the plane's world pose
    pub plane_world: xr::Posef,
    /// plane-local metres from the plane centre
    pub local: [f32; 2],
    /// distance from the head to the point, metres (the visual-angle scale)
    pub distance: f32,
    /// the kind that owns the pointer (`pointer.rs` `PointerOwner`)
    pub owner: Option<SourceKind>,
}

/// The inputs `layer()` resolved from this tick, for `zxr ctl list` and the harness (Copy, no
/// allocation): who owns the pointer, whether it is on a plane, whether a ray hit, the client's
/// image status and the typing hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    pub targeting: Option<SourceKind>,
    pub owner: Option<SourceKind>,
    pub pointer_on_plane: bool,
    pub reticle: bool,
    pub hidden_typing: bool,
    /// 'n' named, 's' surface, 'h' hidden
    pub client: char,
    /// the `cursor-shape-v1` name when named
    pub client_name: &'static str,
}

/// The cursor state for this tick. The reticle input (a targeting ray's hit) and the pointer
/// input (the logical pointer's plane point) are set independently by the seat stage; `layer()`
/// resolves them to one element.
pub struct Cursors {
    /// the targeting ray's hit: the plane's world pose, the plane-local point, the distance
    reticle: Option<(xr::Posef, [f32; 2], f32)>,
    /// the logical pointer on a plane
    pointer: Option<PointerPoint>,
    /// the tier's targeting kind this tick (`None` before the Tier stage has run) — diagnostics
    targeting: Option<SourceKind>,
    client: CursorImageStatus,
    hidden_typing: bool,
    /// `input.cursor.ray` / `input.cursor.scale` (§14) — pushed by the control socket until
    /// `org.mura.Settings1` delivers them
    ray_cursor: RayCursor,
    scale: Scale,
}

impl Default for Cursors {
    fn default() -> Self {
        Cursors { reticle: None, pointer: None, targeting: None, client: CursorImageStatus::default_named(), hidden_typing: false, ray_cursor: RayCursor::default(), scale: Scale::default() }
    }
}

/// Diameter in metres of a disc subtending `deg` degrees at `distance` metres.
pub fn size_for_angle(deg: f32, distance: f32) -> f32 {
    2.0 * distance.max(0.0) * (deg.to_radians() * 0.5).tan()
}

/// Metres per panel pixel so that `RETICLE_PX` pixels subtend `RETICLE_DEG` at `distance`.
pub fn m_per_px(distance: f32) -> f32 {
    size_for_angle(RETICLE_DEG, distance) / RETICLE_PX as f32
}

/// Whether a kind is a ray whose targeting feedback is the ring (§7: hand, head, controller).
fn is_ray(kind: SourceKind) -> bool {
    matches!(kind, SourceKind::Controller(_) | SourceKind::Head | SourceKind::Hand(_))
}

impl Cursors {
    /// The §7 row in effect, derived from the resolved layer.
    pub fn kind(&self) -> Kind {
        match self.layer().map(|l| l.content) {
            None => Kind::None,
            Some(Content::Ring) => Kind::Reticle,
            Some(Content::RingAndImage) => Kind::ReticleAndClient,
            Some(Content::Image) => Kind::Client,
        }
    }

    /// A targeting ray hit a plane: the plane's world pose, the plane-local point and the
    /// distance along the ray (sized by visual angle: constant apparent size, §7).
    pub fn set_reticle(&mut self, plane_world: xr::Posef, local: [f32; 2], distance: f32) {
        self.reticle = Some((plane_world, local, distance));
    }

    /// No targeting hit this tick (gaze targeting, no hit).
    pub fn clear_reticle(&mut self) {
        self.reticle = None;
    }

    /// Where the logical pointer is on a plane, and who owns it; `None` between planes.
    pub fn set_pointer(&mut self, p: Option<PointerPoint>) {
        self.pointer = p;
    }

    /// The tier's targeting kind this tick (§3), for diagnostics. Gaze needs no rule here: the
    /// pointer transport releases a ray-owned pointer when gaze takes the tier (§5,
    /// `PointerLogic::release_for_gaze`) and the seat clears the reticle, so no plane and no hit
    /// means no layer.
    pub fn set_targeting(&mut self, t: Option<SourceKind>) {
        self.targeting = t;
    }

    /// `input.cursor.ray` — what a ray that owns the pointer shows.
    pub fn set_ray_cursor(&mut self, r: RayCursor) {
        self.ray_cursor = r;
    }

    /// `input.cursor.scale` — visual angle or the plane's pixels.
    pub fn set_scale(&mut self, s: Scale) {
        self.scale = s;
    }

    pub fn ray_cursor(&self) -> RayCursor {
        self.ray_cursor
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// The client's cursor as smithay reported it (`SeatHandler::cursor_image`).
    pub fn set_client_cursor(&mut self, status: CursorImageStatus) {
        self.client = status;
    }

    /// A key was pressed: the pointer-class cursor hides until the pointer moves (§8).
    pub fn on_key(&mut self) {
        self.hidden_typing = true;
    }

    /// The pointer moved: the cursor shows again.
    pub fn on_motion(&mut self) {
        self.hidden_typing = false;
    }

    pub fn hidden_for_typing(&self) -> bool {
        self.hidden_typing
    }

    pub fn inputs(&self) -> Inputs {
        Inputs {
            targeting: self.targeting,
            owner: self.pointer.and_then(|p| p.owner),
            pointer_on_plane: self.pointer.is_some(),
            reticle: self.reticle.is_some(),
            hidden_typing: self.hidden_typing,
            client: match &self.client {
                CursorImageStatus::Named(_) => 'n',
                CursorImageStatus::Surface(_) => 's',
                CursorImageStatus::Hidden => 'h',
            },
            client_name: match &self.client {
                CursorImageStatus::Named(i) => i.name(),
                _ => "",
            },
        }
    }

    /// The client image, when it is drawable: not while typing, not when the client asked
    /// `Hidden`.
    fn client_image(&self) -> Option<&CursorImageStatus> {
        if self.hidden_typing {
            return None;
        }
        match &self.client {
            CursorImageStatus::Hidden => None,
            other => Some(other),
        }
    }

    /// The one layer this tick (the rule in the module doc), or nothing.
    pub fn layer(&self) -> Option<CursorLayer> {
        let scale = self.scale;
        let at = |plane_world: xr::Posef, local: [f32; 2], distance: f32| {
            let p = math::pose_apply(plane_world, [local[0], local[1], CURSOR_LIFT_M]);
            let mpp = match scale {
                Scale::Angle => m_per_px(distance),
                Scale::Plane => crate::scene::M_PER_PX,
            };
            (xr::Posef { orientation: plane_world.orientation, position: xr::Vector3f { x: p[0], y: p[1], z: p[2] } }, mpp)
        };
        if let Some(pt) = self.pointer {
            let image = self.client_image();
            let ray_owner = pt.owner.map(is_ray).unwrap_or(false);
            let (pose, m_per_px) = at(pt.plane_world, pt.local, pt.distance);
            // the ring the owning ray keeps while its image is hidden — at its hit if it has one
            let ring_alone = || {
                let (pose, m_per_px) = self.reticle.map(|(w, l, d)| at(w, l, d)).unwrap_or((pose, m_per_px));
                Some(CursorLayer { pose, m_per_px, content: Content::Ring, image: None })
            };
            return match (image, ray_owner, self.ray_cursor) {
                (Some(img), true, RayCursor::Both) => Some(CursorLayer { pose, m_per_px, content: Content::RingAndImage, image: Some(img.clone()) }),
                (Some(img), true, RayCursor::Image) | (Some(img), false, _) => Some(CursorLayer { pose, m_per_px, content: Content::Image, image: Some(img.clone()) }),
                (Some(_), true, RayCursor::Ring) | (None, true, RayCursor::Both | RayCursor::Ring) => ring_alone(),
                // a mouse, or a ray set to image-only, with nothing to show: no ray reticle either
                (None, true, RayCursor::Image) | (None, false, _) => None,
            };
        }
        let (w, l, d) = self.reticle?;
        let (pose, m_per_px) = at(w, l, d);
        Some(CursorLayer { pose, m_per_px, content: Content::Ring, image: None })
    }
}

/// An anti-aliased ring: outer radius 0.47·side, inner 0.34·side, one-pixel soft edges; white,
/// premultiplied BGRA so the quad blends with `BLEND_TEXTURE_SOURCE_ALPHA`.
pub fn ring_pixels(side: u32) -> Vec<u8> {
    let n = side as usize;
    let mut out = vec![0u8; n * n * 4];
    let c = (side as f32 - 1.0) * 0.5;
    let r_out = side as f32 * 0.47;
    let r_in = side as f32 * 0.34;
    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let r = (dx * dx + dy * dy).sqrt();
            // coverage: 1 inside the band, linear falloff over one pixel at both edges
            let a = ((r_out - r).clamp(0.0, 1.0)) * ((r - r_in).clamp(0.0, 1.0));
            let v = (a * 255.0).round() as u8;
            let i = (y * n + x) * 4;
            out[i] = v;
            out[i + 1] = v;
            out[i + 2] = v;
            out[i + 3] = v;
        }
    }
    out
}

/// The panel side an image of `w`×`h` with hotspot `(hx, hy)` needs so the hotspot sits at the
/// centre and nothing is clipped: at least `CURSOR_PX`, even, grow-only by the caller.
pub fn panel_side_for(w: u32, h: u32, hx: i32, hy: i32) -> u32 {
    let reach = |len: u32, hot: i32| -> u32 {
        let hot = hot.clamp(0, len as i32) as u32;
        hot.max(len - hot)
    };
    let need = 2 * reach(w, hx).max(reach(h, hy));
    need.max(CURSOR_PX).div_ceil(2) * 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Side;

    fn plane() -> xr::Posef {
        math::pose_identity()
    }

    #[test]
    fn layer_scale_is_constant_visual_angle() {
        let near = size_for_angle(RETICLE_DEG, 1.0);
        let far = size_for_angle(RETICLE_DEG, 2.0);
        assert!((far / near - 2.0).abs() < 1e-5);
        assert!((near - 0.02618).abs() < 1e-4, "{near}");
        assert!((m_per_px(1.0) * RETICLE_PX as f32 - near).abs() < 1e-7);
    }

    #[test]
    fn ring_is_transparent_at_centre_and_corners_and_opaque_in_the_band() {
        let px = ring_pixels(RETICLE_PX);
        let at = |x: u32, y: u32| px[((y * RETICLE_PX + x) * 4 + 3) as usize];
        assert_eq!(at(31, 31), 0);
        assert_eq!(at(0, 0), 0);
        let band = (RETICLE_PX as f32 * 0.405) as u32;
        assert_eq!(at(31 + band, 31), 255);
    }

    #[test]
    fn panel_side_is_fixed_for_theme_cursors_and_grows_around_the_hotspot() {
        assert_eq!(panel_side_for(24, 24, 4, 4), CURSOR_PX, "a 24 px arrow fits the fixed panel");
        assert_eq!(panel_side_for(32, 32, 16, 16), CURSOR_PX, "a 32 px I-beam fits");
        assert_eq!(panel_side_for(64, 64, 32, 32), CURSOR_PX, "a 64 px image centred fits exactly");
        assert_eq!(panel_side_for(64, 64, 0, 0), 128, "hotspot at a corner needs the reach both ways");
        assert_eq!(panel_side_for(100, 20, 50, 10), 100);
        assert_eq!(panel_side_for(50, 50, 25, 25), CURSOR_PX);
        assert_eq!(panel_side_for(33, 33, 0, 0), 66, "even");
    }

    #[test]
    fn precedence_one_element() {
        let mut c = Cursors::default();
        assert!(c.layer().is_none(), "gaze / nothing: no layer");
        assert_eq!(c.kind(), Kind::None);

        // a hand ray hit: the ring alone
        c.set_reticle(plane(), [0.1, 0.0], 1.5);
        let l = c.layer().expect("ring");
        assert_eq!(l.content, Content::Ring);
        assert_eq!(c.kind(), Kind::Reticle);
        assert!((l.pose.position.x - 0.1).abs() < 1e-6 && (l.pose.position.z - CURSOR_LIFT_M).abs() < 1e-6);
        assert!((l.m_per_px - m_per_px(1.5)).abs() < 1e-9);

        // a mouse on a plane while the head ray targets elsewhere: the client image only, at the
        // pointer — no second element for the ray
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [-0.2, 0.05], distance: 1.0, owner: Some(SourceKind::Pointer) }));
        let l = c.layer().expect("image");
        assert_eq!(l.content, Content::Image);
        assert_eq!(c.kind(), Kind::Client);
        assert!((l.pose.position.x + 0.2).abs() < 1e-6, "at the pointer, not the hit");
        assert!(matches!(l.image, Some(CursorImageStatus::Named(_))));

        // typing hides the mouse cursor, and shows no ray reticle in its place
        c.on_key();
        assert!(c.layer().is_none(), "hidden while typing; the mouse suppresses the ray's ring");
        c.on_motion();
        assert!(c.layer().is_some());

        // the client hid its cursor: same
        c.set_client_cursor(CursorImageStatus::Hidden);
        assert!(c.layer().is_none());
        c.set_client_cursor(CursorImageStatus::default_named());

        // a controller owns the pointer: ring around the image, one layer at the pointer
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [0.1, 0.0], distance: 1.5, owner: Some(SourceKind::Controller(Side::Right)) }));
        let l = c.layer().expect("ring and image");
        assert_eq!(l.content, Content::RingAndImage);
        assert_eq!(c.kind(), Kind::ReticleAndClient);
        assert!(l.content.has_ring() && l.content.has_image());

        // the controller's client image hidden (typing): the ring stays, at its hit
        c.on_key();
        let l = c.layer().expect("ring");
        assert_eq!(l.content, Content::Ring);
        assert!(l.image.is_none());
        c.on_motion();

        // the head owns the pointer (the floor): a ray owner too
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [0.0, 0.0], distance: 1.0, owner: Some(SourceKind::Head) }));
        assert_eq!(c.layer().unwrap().content, Content::RingAndImage);

        // gaze takes the tier: the transport releases the head-owned pointer (no plane) and the
        // seat clears the reticle — no layer, with no gaze rule in here
        c.set_targeting(Some(SourceKind::Gaze));
        c.set_pointer(None);
        c.clear_reticle();
        assert!(c.layer().is_none(), "gaze: nothing");
        // a mouse under gaze keeps its cursor (the transport does not release it)
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [0.0, 0.0], distance: 1.0, owner: Some(SourceKind::Pointer) }));
        assert_eq!(c.layer().unwrap().content, Content::Image, "a mouse under gaze keeps its cursor");
        c.set_targeting(Some(SourceKind::Head));
        c.set_reticle(plane(), [0.1, 0.0], 1.5);
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [0.0, 0.0], distance: 1.0, owner: Some(SourceKind::Head) }));

        // pointer between planes: the ray's ring at its hit
        c.set_pointer(None);
        assert_eq!(c.layer().unwrap().content, Content::Ring);
        c.clear_reticle();
        assert!(c.layer().is_none());
    }

    #[test]
    fn ray_cursor_and_scale_preferences() {
        let mut c = Cursors::default();
        assert_eq!(c.ray_cursor(), RayCursor::Both);
        assert_eq!(c.scale(), Scale::Angle);
        c.set_reticle(plane(), [0.1, 0.0], 1.5);
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [0.1, 0.0], distance: 1.5, owner: Some(SourceKind::Controller(Side::Left)) }));
        // image only for a ray owner: the desktop look; hidden while typing → nothing, like a mouse
        c.set_ray_cursor(RayCursor::Image);
        assert_eq!(c.layer().unwrap().content, Content::Image);
        c.on_key();
        assert!(c.layer().is_none());
        c.on_motion();
        // ring only: the XR-shell look, unaffected by typing
        c.set_ray_cursor(RayCursor::Ring);
        assert_eq!(c.layer().unwrap().content, Content::Ring);
        c.on_key();
        assert_eq!(c.layer().unwrap().content, Content::Ring);
        c.on_motion();
        // the preference never touches a mouse
        c.set_pointer(Some(PointerPoint { plane_world: plane(), local: [0.1, 0.0], distance: 1.5, owner: Some(SourceKind::Pointer) }));
        assert_eq!(c.layer().unwrap().content, Content::Image);
        // scale: angle vs the plane's pixels
        let angle = c.layer().unwrap().m_per_px;
        assert!((angle - m_per_px(1.5)).abs() < 1e-9);
        c.set_scale(Scale::Plane);
        assert!((c.layer().unwrap().m_per_px - crate::scene::M_PER_PX).abs() < 1e-9);
        assert_eq!(RayCursor::parse("both"), Some(RayCursor::Both));
        assert_eq!(RayCursor::parse("image"), Some(RayCursor::Image));
        assert_eq!(RayCursor::parse("nope"), None);
        assert_eq!(Scale::parse("plane"), Some(Scale::Plane));
        assert_eq!(Scale::parse("angle"), Some(Scale::Angle));
    }
}
