//! `ext-session-lock-v1` served for the desktop profile (ADR 0007: never the built-in lock's
//! mechanism; spec §9 rev 3.12; research/77 §2.4). The lock is compositor state — the same
//! `Mode::Locked` the built-in lock and `zxr ctl mode` set — and the protocol's obligations are
//! its mechanics: `locked` is sent only after a frame was composed with zero untrusted samples
//! (I2: `confirm_after_frame`); a lock surface is a member of band 5 on the head frame, trusted
//! (it *is* the lock scene); a second `lock` while one is alive is refused (cosmic-comp
//! `session_lock.rs:19-43`); a lock client's death leaves the mode locked and the scene opaque
//! (I3; sway paints red and keeps the lock, `lock.c:245-258`; smithay's `Defunct`).

use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker};

use super::{plane_size_px, pose_at, Frame, Surface};
use crate::input::Mode;
use crate::scene::{Flags, MemberId, Shape};
use crate::state::{Payload, Zxr};
use crate::xr::math;

#[derive(Default)]
pub struct LockState {
    /// the locker awaiting I2's frame
    pending: Option<SessionLocker>,
    /// `locked` was sent
    pub locked: bool,
    pub surfaces: Vec<(MemberId, LockSurface)>,
    pub locks: u64,
    pub refused: u64,
}

impl LockState {
    pub fn status(&self) -> &'static str {
        if self.locked {
            "locked"
        } else if self.pending.is_some() {
            "locking"
        } else {
            "unlocked"
        }
    }

    pub fn active(&self) -> bool {
        self.locked || self.pending.is_some()
    }
}

impl SessionLockHandler for Zxr {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        if self.shell.lock.active() {
            // cosmic-comp: refuse while a lock client is alive — dropping the locker sends `finished`
            self.shell.lock.refused += 1;
            tracing::warn!("ext-session-lock: a lock is active; refused");
            drop(confirmation);
            return;
        }
        self.shell.lock.locks += 1;
        self.input.mode = Mode::Locked;
        self.shell.lock.pending = Some(confirmation);
        // focus leaves every window (I1); the lock surface takes it when it maps
        self.focus_window(None);
        tracing::info!("ext-session-lock: locking — `locked` after the first frame with no untrusted sample (I2)");
    }

    fn unlock(&mut self) {
        let ids: Vec<MemberId> = self.shell.lock.surfaces.drain(..).map(|(id, _)| id).collect();
        for id in ids {
            remove_member(self, id);
        }
        self.shell.lock.pending = None;
        self.shell.lock.locked = false;
        self.input.mode = Mode::Normal;
        let restore = self.focus.stack.restore(|m| self.scene.get(m).map(|x| x.m.mapped()).unwrap_or(false));
        self.focus_window(restore);
        tracing::info!("ext-session-lock: unlocked");
    }

    fn new_surface(&mut self, surface: LockSurface, _output: WlOutput) {
        let rect = self.shell.rect(Frame::Head).copied().unwrap_or_else(|| super::FrameRect { frame: Frame::Head, size: super::head_mode_size(&self.shell.head), ppd: super::FRAME_PX_W as f32 / self.shell.head.extent_h_deg, distance_m: self.shell.head.distance_m, usable: smithay::utils::Rectangle::from_size(super::head_mode_size(&self.shell.head)) });
        surface.with_pending_state(|s| s.size = Some((rect.size.w as u32, rect.size.h as u32).into()));
        surface.send_configure();
        let place = self.scene.add_place(self.scene.head, math::pose_identity(), 5);
        let payload = Payload { window: Surface::Lock(surface.clone()), panel: None, dirty: false, mapped_at: 0, last_frame_callback: 0, hidden: false, urgent: false, requested_at_commit: None, pending_activation: None, trusted: true };
        let prev = self.scene.focused;
        let id = self.scene.add(place, pose_at(0.0, 0.0, rect.distance_m, 0.0), Shape::Plane { size: plane_size_px(rect.size, rect.ppd, rect.distance_m) }, Flags(0), payload).expect("place is live");
        self.scene.focus(prev);
        self.shell.lock.surfaces.push((id, surface));
    }
}

fn remove_member(st: &mut Zxr, id: MemberId) {
    if let Some(m) = st.scene.remove(id) {
        if let Some(panel) = m.m.panel {
            st.retire_panel(panel);
        }
        st.scene.remove_place(m.place);
    }
    st.focus.stack.remove(id);
}

/// A lock surface's commit: map on the first buffer; the keyboard goes to it (the protocol's
/// "input only to the lock surface").
pub fn commit(st: &mut Zxr, id: MemberId, surface: &WlSurface) {
    let has_buffer = with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false);
    let Some(m) = st.scene.get_mut(id) else { return };
    if has_buffer && !m.m.mapped() {
        m.m.mapped_at = st.frame_id.max(1);
        st.set_keyboard_focus(Some(surface.clone()));
    }
}

/// End of a tick: I2 — the first frame composed while `Mode::Locked` (the flatten admitted only
/// trusted members) confirms the lock; dead lock surfaces are dropped (their client died: the
/// mode stays, the scene is opaque, `Defunct` is smithay's word for it).
pub fn after_frame(st: &mut Zxr) {
    if st.input.mode == Mode::Locked {
        if let Some(locker) = st.shell.lock.pending.take() {
            locker.lock();
            st.shell.lock.locked = true;
            tracing::info!("ext-session-lock: `locked` sent after a frame with no untrusted sample");
        }
    }
    let dead: Vec<MemberId> = st.shell.lock.surfaces.iter().filter(|(_, s)| !s.alive()).map(|(id, _)| *id).collect();
    if !dead.is_empty() {
        st.shell.lock.surfaces.retain(|(_, s)| s.alive());
        for id in dead {
            remove_member(st, id);
        }
        if st.shell.lock.locked {
            tracing::warn!("ext-session-lock: the lock client's surfaces are gone while locked — the mode stays (I3)");
        }
    }
}
