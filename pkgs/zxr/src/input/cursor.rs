//! Cursors (spatial-input §7, ruled; research/63 §7 "The cursor").
//!
//! | source class | drawn |
//! |---|---|
//! | gaze (touch-class) | nothing — plane-level emphasis (`emphasis.rs`) |
//! | hand ray, head ray (touch-class); poke | a compositor **reticle** at the hit, sized in visual angle |
//! | controller ray (pointer-class) | the reticle **plus** the client's cursor meaning |
//! | mouse / trackpad (pointer-class) | the pointer-class cursor on the plane |
//!
//! (spatial-input.md:310-321.) "`cursor-shape-v1` is preferred so the compositor renders one
//! theme at one scale across applications (the protocol's own stated reason)"
//! (`references/smithay/src/wayland/cursor_shape.rs:1-4`; `cursor-shape-v1.xml:27-29`); else the
//! client's `wl_pointer.set_cursor` surface is drawn on the plane with its hotspot (wxrc
//! `src/input.c:448-512`, motorcar, Simula — research/63 §7 :325-328).
//!
//! This file is the **state**: where the reticle is this tick (as a world pose and a size), the
//! one procedural ring texture, and which client cursor is current (smithay delivers both
//! `cursor-shape-v1` names and `set_cursor` surfaces through `SeatHandler::cursor_image` as one
//! `CursorImageStatus`, `input/pointer/cursor_image.rs:33-43`). The reticle is presented as a
//! band-5 quad by the frame procedure from [`Cursors::reticle_quad`]; the client cursor is drawn
//! by the panel pass from [`Cursors::client_cursor`] — both patches are in the lane report.
//!
//! **Hidden while typing:** a key sample hides the pointer-class cursor; motion shows it again
//! (the desktops' behaviour; GNOME/KDE hide the cursor on key press when the setting is on —
//! here it is the rule because a plane-mounted cursor over text is what the design's §8 keyboard
//! rule is about — flagged as a judgment).
//!
//! **Stand-in (flagged):** the reticle subtends **1.5°** of visual angle (diameter) at the hit
//! distance — the brief's number; HoloLens' ≥ 2° is a *target* size, MRTK3's reticle scales
//! with distance (`MRTKRayReticleVisual.cs:161-166`) without a stated angle.
//!
//! Budget: one 64×64 texture created once; a few scalars per tick; no allocation.

use openxr as xr;
use smithay::input::pointer::CursorImageStatus;

use crate::render::{Renderer, Texture};
use crate::xr::math;

/// Reticle diameter in degrees of visual angle at the hit distance (stand-in — flagged).
pub const RETICLE_DEG: f32 = 1.5;
/// The reticle texture's side in pixels.
pub const RETICLE_PX: u32 = 64;
/// Offset along the plane normal toward the viewer so the reticle quad sits on, not in, the plane.
pub const RETICLE_LIFT_M: f32 = 0.002;

/// Which class of cursor is current — the §7 rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    /// gaze: nothing
    #[default]
    None,
    /// hand / head ray, poke: the reticle alone
    Reticle,
    /// controller ray: reticle plus the client's cursor meaning
    ReticleAndClient,
    /// mouse / trackpad: the pointer-class cursor
    Client,
}

/// The cursor state for this tick. The reticle (a ray's) and the client cursor (the logical
/// pointer's, on its plane) are independent: a mouse under gaze targeting has a cursor and no
/// reticle; a non-owning controller has a reticle and no cursor (spatial-input §5 :266-268, §7).
pub struct Cursors {
    /// the reticle's world pose (plane orientation, at the hit, lifted) and diameter in metres
    reticle: Option<(xr::Posef, f32)>,
    /// the logical pointer is on a plane: the client cursor is drawable there
    client_on_plane: bool,
    texture: Option<Texture>,
    client: CursorImageStatus,
    hidden_typing: bool,
}

impl Default for Cursors {
    fn default() -> Self {
        Cursors { reticle: None, client_on_plane: false, texture: None, client: CursorImageStatus::default_named(), hidden_typing: false }
    }
}

/// Diameter in metres of a disc subtending `deg` degrees at `distance` metres.
pub fn size_for_angle(deg: f32, distance: f32) -> f32 {
    2.0 * distance.max(0.0) * (deg.to_radians() * 0.5).tan()
}

impl Cursors {
    /// The §7 row in effect, derived from what is drawable.
    pub fn kind(&self) -> Kind {
        match (self.reticle.is_some(), self.client_on_plane) {
            (false, false) => Kind::None,
            (true, false) => Kind::Reticle,
            (true, true) => Kind::ReticleAndClient,
            (false, true) => Kind::Client,
        }
    }

    /// Place the reticle at a plane hit: the plane's world pose, the plane-local point and the
    /// distance along the ray (sized by visual angle: constant apparent size, §7).
    pub fn set_reticle(&mut self, plane_world: xr::Posef, local: [f32; 2], distance: f32) {
        let p = math::pose_apply(plane_world, [local[0], local[1], RETICLE_LIFT_M]);
        let pose = xr::Posef { orientation: plane_world.orientation, position: xr::Vector3f { x: p[0], y: p[1], z: p[2] } };
        self.reticle = Some((pose, size_for_angle(RETICLE_DEG, distance)));
    }

    /// No reticle this tick (gaze targeting, no hit, or a pointer-class cursor without a ray).
    pub fn clear_reticle(&mut self) {
        self.reticle = None;
    }

    /// Whether the logical pointer is on a plane (the client cursor is drawable there).
    pub fn set_client_on_plane(&mut self, on: bool) {
        self.client_on_plane = on;
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

    /// The client cursor to draw on the plane at the pointer, if any: `None` for touch-class
    /// kinds, while typing, and when the client asked for `Hidden`.
    pub fn client_cursor(&self) -> Option<&CursorImageStatus> {
        if !self.client_on_plane || self.hidden_typing {
            return None;
        }
        match &self.client {
            CursorImageStatus::Hidden => None,
            other => Some(other),
        }
    }

    /// Create the ring texture once (an anti-aliased white ring, BGRA, premultiplied).
    pub fn ensure_texture(&mut self, renderer: &mut Renderer) {
        if self.texture.is_some() {
            return;
        }
        let mut tex = match renderer.create_shm_texture(RETICLE_PX, RETICLE_PX) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("reticle texture: {e}");
                return;
            }
        };
        let pixels = ring_pixels(RETICLE_PX);
        match renderer.upload_shm(&mut tex, &pixels, RETICLE_PX * 4) {
            Ok(()) => self.texture = Some(tex),
            Err(e) => {
                tracing::warn!("reticle upload: {e}");
                renderer.destroy_texture(tex);
            }
        }
    }

    /// The reticle to present this tick as a band-5 quad: its world pose, its size in metres
    /// (square) and the ring texture. `None` when there is no reticle or the texture is not up.
    pub fn reticle_quad(&self) -> Option<(xr::Posef, [f32; 2], &Texture)> {
        let (pose, d) = self.reticle?;
        let tex = self.texture.as_ref()?;
        Some((pose, [d, d], tex))
    }

    /// Give the texture back to the renderer (teardown).
    pub fn destroy(&mut self, renderer: &mut Renderer) {
        if let Some(t) = self.texture.take() {
            renderer.destroy_texture(t);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reticle_size_is_constant_visual_angle() {
        let near = size_for_angle(RETICLE_DEG, 1.0);
        let far = size_for_angle(RETICLE_DEG, 2.0);
        assert!((far / near - 2.0).abs() < 1e-5);
        assert!((near - 0.02618).abs() < 1e-4, "{near}");
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
    fn client_cursor_follows_class_and_typing() {
        let mut c = Cursors::default();
        assert!(c.client_cursor().is_none(), "gaze: nothing");
        assert_eq!(c.kind(), Kind::None);
        c.set_client_on_plane(true);
        assert_eq!(c.kind(), Kind::Client);
        assert!(matches!(c.client_cursor(), Some(CursorImageStatus::Named(_))));
        c.on_key();
        assert!(c.client_cursor().is_none(), "hidden while typing");
        c.on_motion();
        assert!(c.client_cursor().is_some());
        c.set_client_cursor(CursorImageStatus::Hidden);
        assert!(c.client_cursor().is_none());
        c.set_client_cursor(CursorImageStatus::default_named());
        c.set_client_on_plane(false);
        c.set_reticle(math::pose_identity(), [0.0, 0.0], 1.5);
        assert_eq!(c.kind(), Kind::Reticle);
        assert!(c.client_cursor().is_none(), "a hand ray has no client cursor");
        assert!(c.reticle_quad().is_none(), "no texture yet");
        c.set_client_on_plane(true);
        assert_eq!(c.kind(), Kind::ReticleAndClient);
        let p = math::pose_apply(math::pose_identity(), [0.0, 0.0, RETICLE_LIFT_M]);
        assert!((p[2] - RETICLE_LIFT_M).abs() < 1e-7);
    }
}
