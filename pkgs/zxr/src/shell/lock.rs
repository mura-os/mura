//! `ext-session-lock-v1` — the built-in lock's mechanism since ADR 0007 amendment 2 (rev 6 of
//! session-auth: the lock program is an `ext-session-lock` client under a user unit) and the
//! desktop profile's protocol alike (spec §9 rev 3.13; research/77 §2.4; research/78 §9). The lock
//! is compositor state — the same `Mode::Locked` `zxr ctl mode` sets — and the protocol's
//! obligations are its mechanics: `locked` is sent only after a frame was composed with zero
//! untrusted samples (I2: `confirm_after_frame`); a lock surface is a member of band 5 on the
//! head frame, trusted (it *is* the lock scene); a second `lock` while the locker is alive is
//! refused, **a `lock` after the locker died is accepted** — cosmic-comp's rule
//! (`references/cosmic-comp/src/wayland/handlers/session_lock.rs:19-31`: refuse only while the
//! previous `ext_session_lock_v1`'s client still exists; smithay names the dead state `Defunct`,
//! `session_lock/lock.rs:136-144`, and leaves the policy to the compositor); a lock client's death
//! leaves the mode locked and the scene opaque until the next locker (I3; sway paints red and
//! keeps the lock, `lock.c:245-258`).
//!
//! **Triggers** ([`triggers`]): zxr never locks by itself — it asks logind, exactly as swayidle
//! (`swayidle/main.c`: a timeout runs a command, `loginctl lock-session` in every shipped config)
//! and KWin's `ScreenLocker::KSldApp` do; the lock program's unit listens for logind's `Lock`
//! (cosmic-greeter `src/logind.rs:94-139`) and takes the lock. The ladder is ADR 0007's, the
//! numbers `Prefs`': `session.lock.on_doff` after `doff_grace_s` of absence (presence is the mode
//! stage's), `session.lock.on_idle` after `session.idle.delay_s + lock_delay_s` without activity
//! (GNOME's `lock-delay` shape: seconds after the blank), `zxr ctl lock` on request. The command
//! is `--lock-command` (default `loginctl lock-session`; the nested harness substitutes). Nothing
//! fires while already locked, in greeter mode, or with `session.lock.enabled` off.

use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
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
    /// the `ext_session_lock_v1` that holds the lock (its client's liveness is the relock rule)
    owner: Option<smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1>,
    pub surfaces: Vec<(MemberId, LockSurface)>,
    pub locks: u64,
    pub refused: u64,
    /// the doff instant the grace runs from (`triggers`)
    absent_since_ns: Option<u64>,
    /// a trigger fired and the lock has not arrived yet: no second command until it does or
    /// activity resumes
    trigger_pending: bool,
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
            let owner_alive = self.shell.lock.owner.as_ref().map(|o| self.dh.get_client(o.id()).is_ok()).unwrap_or(false);
            if owner_alive {
                // cosmic-comp: refuse while the lock client is alive — dropping the locker sends `finished`
                self.shell.lock.refused += 1;
                tracing::warn!("ext-session-lock: a lock is active; refused");
                drop(confirmation);
                return;
            }
            // the previous locker died (Defunct): the new one takes over; its surfaces replace
            // the dead ones and the mode never left `Locked`
            let ids: Vec<MemberId> = self.shell.lock.surfaces.drain(..).map(|(id, _)| id).collect();
            for id in ids {
                remove_member(self, id);
            }
            self.journal.lock_relocks += 1;
            tracing::info!("ext-session-lock: the previous locker is gone; a new locker takes the lock (relock)");
        }
        self.shell.lock.locks += 1;
        self.input.mode = Mode::Locked;
        self.shell.lock.owner = Some(confirmation.ext_session_lock().clone());
        self.shell.lock.trigger_pending = false;
        if self.shell.lock.locked {
            // already confirmed to the session: the new locker is told at once (I2 held for the first)
            confirmation.lock();
        } else {
            self.shell.lock.pending = Some(confirmation);
        }
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
        self.shell.lock.owner = None;
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
            tracing::warn!("ext-session-lock: the lock client's surfaces are gone while locked — the mode stays (I3); the next locker is accepted");
        }
    }
}

/// The wearer took the headset off / put it on (the mode stage's presence transitions).
pub fn note_presence(st: &mut Zxr, present: bool, now_ns: u64) {
    st.shell.lock.absent_since_ns = if present { None } else { Some(now_ns) };
    if present {
        st.shell.lock.trigger_pending = false;
    }
}

/// Run the lock command now (`zxr ctl lock`, the triggers). One process, fire and forget: the
/// lock arrives as an `ext_session_lock_v1.lock` from the program logind wakes.
pub fn request_lock(st: &mut Zxr, why: &str) -> bool {
    if st.shell.lock.active() || st.input.mode != Mode::Normal {
        return false;
    }
    let cmd = st.lock_command.clone();
    match std::process::Command::new("/bin/sh").arg("-c").arg(&cmd).stdin(std::process::Stdio::null()).spawn() {
        Ok(child) => {
            st.children.push(child);
            st.shell.lock.trigger_pending = true;
            st.journal.lock_triggers += 1;
            tracing::info!(why, cmd, "lock requested");
            true
        }
        Err(e) => {
            tracing::error!(cmd, "lock command: {e}");
            false
        }
    }
}

/// The ladder's numbers (`Prefs`, ADR 0007).
#[derive(Clone, Copy, Debug)]
pub struct Ladder {
    pub enabled: bool,
    pub on_doff: bool,
    pub doff_grace_s: u64,
    pub on_idle: bool,
    pub idle_delay_s: u64,
    pub idle_lock_delay_s: u64,
}

impl Ladder {
    pub fn from_prefs(p: &crate::settings::Prefs) -> Self {
        Ladder {
            enabled: p.session_lock_enabled,
            on_doff: p.session_lock_on_doff,
            doff_grace_s: p.session_lock_doff_grace_s,
            on_idle: p.session_lock_on_idle,
            idle_delay_s: p.session_idle_delay_s,
            idle_lock_delay_s: p.session_idle_lock_delay_s,
        }
    }
}

/// The ladder, pure: which rung fires now, if any. `absent_since_ns` is the doff instant;
/// `idle_ns` the time since the last activity (`None`: no activity was ever seen — a session that
/// nobody touched yet does not lock itself on the idle rung; the doff rung still applies).
pub fn due(l: &Ladder, now_ns: u64, absent_since_ns: Option<u64>, idle_ns: Option<u64>) -> Option<&'static str> {
    if !l.enabled {
        return None;
    }
    if l.on_doff {
        if let Some(since) = absent_since_ns {
            if now_ns.saturating_sub(since) >= l.doff_grace_s * 1_000_000_000 {
                return Some("doff grace elapsed (session.lock.on_doff)");
            }
        }
    }
    if l.on_idle {
        if let Some(idle) = idle_ns {
            let at = (l.idle_delay_s + l.idle_lock_delay_s) * 1_000_000_000;
            if at > 0 && idle >= at {
                return Some("idle ladder (session.lock.on_idle)");
            }
        }
    }
    None
}

/// The ladder, once per tick: two comparisons and no allocation unless a trigger fires.
pub fn triggers(st: &mut Zxr, now_ns: u64) {
    if st.shell.lock.active() || st.shell.lock.trigger_pending || st.input.mode != Mode::Normal {
        return;
    }
    let l = Ladder::from_prefs(&st.prefs);
    let idle = (st.input.activity.events() > 0).then(|| st.input.activity.idle_ns(now_ns));
    if let Some(why) = due(&l, now_ns, st.shell.lock.absent_since_ns, idle) {
        request_lock(st, why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    fn ladder() -> Ladder {
        Ladder { enabled: true, on_doff: true, doff_grace_s: 45, on_idle: true, idle_delay_s: 300, idle_lock_delay_s: 0 }
    }

    #[test]
    fn doff_locks_after_the_grace_and_not_before() {
        let l = ladder();
        assert_eq!(due(&l, 100 * S, Some(60 * S), None), None, "44 s absent: within the grace");
        assert!(due(&l, 105 * S, Some(60 * S), None).is_some(), "45 s absent: the grace elapsed");
        assert_eq!(due(&Ladder { on_doff: false, ..l }, 200 * S, Some(0), None), None, "on_doff off");
    }

    #[test]
    fn idle_locks_at_delay_plus_lock_delay() {
        let l = Ladder { idle_lock_delay_s: 30, ..ladder() };
        assert_eq!(due(&l, 1000 * S, None, Some(329 * S)), None);
        assert!(due(&l, 1000 * S, None, Some(330 * S)).is_some(), "GNOME's lock-delay: seconds after the blank");
        assert_eq!(due(&l, 1000 * S, None, None), None, "no activity ever seen: the idle rung waits");
        assert_eq!(due(&Ladder { enabled: false, ..l }, 1000 * S, Some(0), Some(10_000 * S)), None, "session.lock.enabled off");
    }
}
