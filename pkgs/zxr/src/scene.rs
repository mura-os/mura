//! The scene (specs/zxr-core.md §3 `scene`, §4–§5): planes for the 2D tier in one world frame,
//! their stacking by depth, focus, and the ray→plane→surface hit test the input module uses.
//! R0 places new toplevels fanned in front of the viewer (motorcar's `WindowManager` shape,
//! `windowmanager.cpp:147-159`); the places model's frames arrive at M1.

use crate::xr::math::{self, Mat4};
use openxr as xr;
use smithay::desktop::{Window, WindowSurfaceType};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point};

/// Metres per logical pixel: 1000 px ≈ 1.2 m at the default distance (a comfortable panel).
pub const M_PER_PX: f32 = 0.0012;
pub const PLANE_DISTANCE: f32 = -1.5;

pub struct Plane {
    pub window: Window,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub mapped_at_frame: u64,
}

impl Plane {
    pub fn model(&self) -> Mat4 {
        math::model(self.pos, self.yaw)
    }
    /// Half extents in metres from the window's current geometry.
    pub fn half_size(&self) -> [f32; 2] {
        let g = self.window.geometry().size;
        [g.w.max(1) as f32 * M_PER_PX * 0.5, g.h.max(1) as f32 * M_PER_PX * 0.5]
    }
    /// A point on the plane (metres, plane-local, y up) → logical surface coordinates.
    pub fn local_to_logical(&self, local: [f32; 2]) -> Point<f64, Logical> {
        let g = self.window.geometry();
        let [hw, hh] = self.half_size();
        let x = (local[0] / (2.0 * hw) + 0.5) as f64 * g.size.w as f64 + g.loc.x as f64;
        let y = (0.5 - local[1] / (2.0 * hh)) as f64 * g.size.h as f64 + g.loc.y as f64;
        Point::from((x, y))
    }
}

#[derive(Default)]
pub struct Scene {
    pub planes: Vec<Plane>,
    pub focused: Option<usize>,
    spawned: usize,
}

impl Scene {
    pub fn add(&mut self, window: Window, frame: u64) -> usize {
        // fan: 0 centre, then alternating right/left with a small yaw toward the viewer
        let n = self.spawned as i32;
        let slot = (n + 1) / 2 * if n % 2 == 1 { 1 } else { -1 };
        let x = slot as f32 * 0.9;
        let yaw = -(slot as f32) * 0.35;
        self.spawned += 1;
        self.planes.push(Plane { window, pos: [x, 0.0, PLANE_DISTANCE], yaw, mapped_at_frame: frame });
        let idx = self.planes.len() - 1;
        self.focused = Some(idx);
        idx
    }

    pub fn remove(&mut self, surface: &WlSurface) -> Option<Plane> {
        let idx = self.planes.iter().position(|p| p.window.toplevel().map(|t| t.wl_surface() == surface).unwrap_or(false))?;
        let p = self.planes.remove(idx);
        self.focused = match self.focused {
            Some(f) if f == idx => self.planes.len().checked_sub(1),
            Some(f) if f > idx => Some(f - 1),
            other => other,
        };
        Some(p)
    }

    pub fn find(&self, surface: &WlSurface) -> Option<usize> {
        self.planes.iter().position(|p| p.window.toplevel().map(|t| t.wl_surface() == surface).unwrap_or(false))
    }

    pub fn focus_next(&mut self) {
        if self.planes.is_empty() {
            self.focused = None;
        } else {
            self.focused = Some(self.focused.map(|f| (f + 1) % self.planes.len()).unwrap_or(0));
        }
    }

    pub fn focused_window(&self) -> Option<&Window> {
        self.focused.and_then(|i| self.planes.get(i)).map(|p| &p.window)
    }

    /// Cast the head ray (pose → −Z) against every plane; nearest hit wins. Returns the plane,
    /// the surface under the hit and the surface-local point.
    pub fn hit(&self, pose: xr::Posef) -> Option<(usize, WlSurface, Point<f64, Logical>)> {
        let origin = [pose.position.x, pose.position.y, pose.position.z];
        let dir = math::rotate(pose.orientation, [0.0, 0.0, -1.0]);
        let mut best: Option<(f32, usize, [f32; 2])> = None;
        for (i, p) in self.planes.iter().enumerate() {
            if let Some((t, local)) = ray_plane(origin, dir, p.pos, p.yaw, p.half_size()) {
                if best.map(|b| t < b.0).unwrap_or(true) {
                    best = Some((t, i, local));
                }
            }
        }
        let (_, i, local) = best?;
        let p = &self.planes[i];
        let logical = p.local_to_logical(local);
        let (surface, loc) = p.window.surface_under(logical, WindowSurfaceType::ALL)?;
        Some((i, surface, logical - loc.to_f64()))
    }
}

/// Ray (origin, unit-ish direction) against a plane at `pos` yawed about Y by `yaw`, extents
/// `half` — returns the distance and the plane-local hit point, or None if missed / behind.
pub fn ray_plane(origin: [f32; 3], dir: [f32; 3], pos: [f32; 3], yaw: f32, half: [f32; 2]) -> Option<(f32, [f32; 2])> {
    // into plane space: translate by -pos, rotate by -yaw about Y
    let o = [origin[0] - pos[0], origin[1] - pos[1], origin[2] - pos[2]];
    let (s, c) = (-yaw).sin_cos();
    let rot = |v: [f32; 3]| [c * v[0] - s * v[2], v[1], s * v[0] + c * v[2]];
    let o = rot(o);
    let d = rot(dir);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_centre_of_facing_plane() {
        let (t, local) = ray_plane([0.0; 3], [0.0, 0.0, -1.0], [0.0, 0.0, PLANE_DISTANCE], 0.0, [0.5, 0.3]).unwrap();
        assert!((t - 1.5).abs() < 1e-5);
        assert!(local[0].abs() < 1e-5 && local[1].abs() < 1e-5);
    }

    #[test]
    fn ray_misses_outside_extents_and_behind() {
        assert!(ray_plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, PLANE_DISTANCE], 0.0, [0.5, 0.3]).is_none());
        assert!(ray_plane([0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, PLANE_DISTANCE], 0.0, [0.5, 0.3]).is_none());
    }

    #[test]
    fn yawed_plane_is_hit_where_it_actually_is() {
        // plane at x = 0.9, yawed −0.35 rad toward the viewer (the fan's second slot): a ray aimed
        // at its centre hits at local (0, 0)
        let pos = [0.9, 0.0, PLANE_DISTANCE];
        let dir = {
            let l = (pos[0] * pos[0] + pos[2] * pos[2]).sqrt();
            [pos[0] / l, 0.0, pos[2] / l]
        };
        let (_, local) = ray_plane([0.0; 3], dir, pos, -0.35, [0.5, 0.3]).unwrap();
        assert!(local[0].abs() < 1e-4 && local[1].abs() < 1e-4);
    }

    #[test]
    fn fan_alternates_sides() {
        // the placement rule as data: slot n → x = ±0.9·ceil(n/2), yaw toward the viewer
        let slots: Vec<i32> = (0..5).map(|n: i32| (n + 1) / 2 * if n % 2 == 1 { 1 } else { -1 }).collect();
        assert_eq!(slots, vec![0, 1, -1, 2, -2]);
    }
}
