//! Lifecycle (window-workspace-management §5): hidden, minimized, maximized, fullscreen, closed —
//! the verbs and the `xdg_toplevel` states they carry. The floor is the manager when no external
//! one is connected: the client's `set_maximized` / `set_fullscreen` / `set_minimized` requests
//! are decided here (§5 "who decides: manager on request"); the seam re-emits them to a
//! connected manager instead (Phase 3).

use openxr as xr;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

use super::Policy;
use crate::scene::{MemberId, Shape};
use crate::state::Zxr;

/// The member's lifecycle state (§5's table).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Life {
    #[default]
    Mapped,
    /// not rendered, keeps place and pose (`hidden` payload + `suspended`)
    Hidden,
    /// hidden, kept, shown by the dock client with an indicator (Q1 ruled)
    Minimized,
    /// fills the limits' width at its distance; the saved geometry restores it
    Maximized(Saved),
    /// fills the band; siblings hidden while it lasts; the saved geometry restores it
    Fullscreen(Saved),
}

/// What maximize/fullscreen restore: the logical size and the place-local pose before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub size_px: (i32, i32),
    pub local: xr::Posef,
}

/// `wm.minimize` (§5, Q1 ruled 2026-09-26).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Minimize {
    /// park on the dock client with an indicator; **degrades to close** while no dock client runs
    #[default]
    Dock,
    Close,
}

impl Minimize {
    pub fn parse(s: &str) -> Option<Minimize> {
        match s {
            "dock" => Some(Minimize::Dock),
            "close" => Some(Minimize::Close),
            _ => None,
        }
    }
}

/// Whether a dock client is present to show minimized members (the launcher/dock of ADR 0012,
/// `foreign-toplevel-list` + `xdg-activation`). **Hook**: none exists yet — the shell plane's;
/// until it does `Minimize::Dock` degrades to close, the ruled behaviour.
pub fn dock_present(_st: &Zxr) -> bool {
    false
}

fn set_state(st: &mut Zxr, member: MemberId, state: xdg_toplevel::State, on: bool, size: Option<(i32, i32)>) {
    let Some(t) = st.scene.get(member).and_then(|m| m.m.window.toplevel().cloned()) else { return };
    t.with_pending_state(|s| {
        if on {
            s.states.set(state);
        } else {
            s.states.unset(state);
        }
        if let Some((w, h)) = size {
            s.size = Some((w.max(1), h.max(1)).into());
        }
    });
    t.send_pending_configure();
}

/// **Hide / show** (§5 hidden): the payload's `hidden` and `xdg_toplevel.suspended`; the pose is
/// kept. `ctl hide` is the same verb.
pub fn set_hidden(st: &mut Zxr, member: MemberId, on: bool) {
    if let Some(m) = st.scene.get_mut(member) {
        m.m.hidden = on;
    }
    let quiet = st.quiet;
    st.set_suspended(member, on || quiet);
    let s = st.policy.state_mut(member);
    if on {
        if s.life == Life::Mapped {
            s.life = Life::Hidden;
        }
    } else if matches!(s.life, Life::Hidden | Life::Minimized) {
        s.life = Life::Mapped;
    }
}

/// **Minimize** (§5, Q1): per `wm.minimize` — park hidden with the dock's indicator when a dock
/// client exists, else close is the verb ("relaunch-into-place" is the place's restore, §5).
pub fn minimize(st: &mut Zxr, member: MemberId) {
    st.policy.minimizes += 1;
    match st.policy.cfg.minimize {
        Minimize::Dock if dock_present(st) => {
            set_hidden(st, member, true);
            st.policy.state_mut(member).life = Life::Minimized;
            tracing::info!(?member, "policy: minimized (dock indicator)");
        }
        _ => {
            tracing::info!(?member, minimize = ?st.policy.cfg.minimize, "policy: minimize → close (no dock client; wm §5 Q1 degrade)");
            close(st, member);
        }
    }
}

/// **Close** (§5): `xdg_toplevel.close`; the client decides.
pub fn close(st: &mut Zxr, member: MemberId) {
    if let Some(t) = st.scene.get(member).and_then(|m| m.m.window.toplevel().cloned()) {
        t.send_close();
    }
}

/// The size that fills the limits' angular width at the member's distance, keeping its aspect
/// (the `free` engine's "spawn size × `wm.size.maximized`" has no declared value yet — the
/// comfort limit is the ceiling the design gives; stand-in, flagged).
fn filling_size(st: &Zxr, member: MemberId) -> Option<(i32, i32)> {
    let m = st.scene.get(member)?;
    let Shape::Plane { size } = m.shape else { return None };
    let world = st.scene.world_pose(member)?;
    let head = st.input.head.or(st.last_head_pose).unwrap_or(xr::Posef::IDENTITY).position;
    let d = [world.position.x - head.x, world.position.y - head.y, world.position.z - head.z];
    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(0.05);
    let max_w = st.policy.cfg.limits.max_width_m(dist);
    let aspect = if size[0] > 1e-4 { size[1] / size[0] } else { 0.75 };
    let mpp = st.scene.layout.m_per_px;
    let w = (max_w / mpp).round().max(64.0) as i32;
    let h = ((max_w * aspect) / mpp).round().max(64.0) as i32;
    Some((w, h))
}

fn saved(st: &Zxr, member: MemberId) -> Option<Saved> {
    let m = st.scene.get(member)?;
    let g = m.m.window.geometry().size;
    Some(Saved { size_px: (g.w.max(1), g.h.max(1)), local: m.local })
}

/// **Maximize** (§5): the `maximized` state and a configure to the filling size; the geometry
/// before is saved for `unmaximize`. A second maximize is idempotent.
pub fn maximize(st: &mut Zxr, member: MemberId, on: bool) {
    let life = st.policy.state(member).life;
    match (on, life) {
        (true, Life::Maximized(_)) | (false, Life::Mapped) => {}
        (true, _) => {
            let Some(s) = saved(st, member) else { return };
            let Some(size) = filling_size(st, member) else { return };
            st.policy.maximizes += 1;
            st.policy.state_mut(member).life = Life::Maximized(s);
            set_state(st, member, xdg_toplevel::State::Maximized, true, Some(size));
            tracing::info!(?member, ?size, "policy: maximized");
        }
        (false, Life::Maximized(s)) => {
            st.policy.state_mut(member).life = Life::Mapped;
            set_state(st, member, xdg_toplevel::State::Maximized, false, Some(s.size_px));
            st.scene.set_local(member, s.local);
        }
        (false, _) => {}
    }
}

/// **Fullscreen** (§5): the `fullscreen` state at the filling size, and the other members of
/// the place hidden while it lasts; `unfullscreen` restores both.
pub fn fullscreen(st: &mut Zxr, member: MemberId, on: bool) {
    let life = st.policy.state(member).life;
    let Some(place) = st.scene.get(member).map(|m| m.place) else { return };
    match (on, life) {
        (true, Life::Fullscreen(_)) => {}
        (true, _) => {
            let Some(s) = saved(st, member) else { return };
            let Some(size) = filling_size(st, member) else { return };
            st.policy.fullscreens += 1;
            st.policy.state_mut(member).life = Life::Fullscreen(s);
            set_state(st, member, xdg_toplevel::State::Fullscreen, true, Some(size));
            let siblings: Vec<MemberId> = st.scene.iter().filter(|(id, m)| *id != member && m.place == place && m.m.mapped() && !m.m.hidden).map(|(id, _)| id).collect();
            for sib in siblings {
                set_hidden(st, sib, true);
                st.policy.state_mut(sib).life = Life::Hidden;
            }
            tracing::info!(?member, ?size, "policy: fullscreen (siblings hidden)");
        }
        (false, Life::Fullscreen(s)) => {
            st.policy.state_mut(member).life = Life::Mapped;
            set_state(st, member, xdg_toplevel::State::Fullscreen, false, Some(s.size_px));
            st.scene.set_local(member, s.local);
            // siblings this fullscreen hid come back; ones hidden on their own account stay
            let hidden: Vec<MemberId> = st.scene.iter().filter(|(id, m)| *id != member && m.place == place && m.m.hidden).map(|(id, _)| id).collect();
            for sib in hidden {
                if st.policy.state(sib).life == Life::Hidden {
                    set_hidden(st, sib, false);
                }
            }
        }
        (false, _) => {}
    }
}

impl Policy {
    /// Whether any member of `place` is fullscreen (a new sibling is hidden while it lasts, §5).
    pub fn place_fullscreen(&self, st: &Zxr, place: crate::scene::PlaceId) -> bool {
        self.members_map().iter().any(|(id, s)| matches!(s.life, Life::Fullscreen(_)) && st.scene.get(*id).map(|m| m.place == place).unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimize_parses_the_key_and_defaults_to_dock() {
        assert_eq!(Minimize::parse("dock"), Some(Minimize::Dock));
        assert_eq!(Minimize::parse("close"), Some(Minimize::Close));
        assert_eq!(Minimize::parse("park"), None);
        assert_eq!(Minimize::default(), Minimize::Dock);
    }
}
