//! The binding filter and the trusted connection (spec §9, §10 rev 3.12; shell-plane §2.2–2.3;
//! research/77 §5).
//!
//! wayland-server evaluates `GlobalDispatch::can_view(client)` when a client's registry is
//! created and again on every `bind` — a bind of a hidden global is a protocol error
//! (`wayland-server-0.31.14/src/global.rs:104-121`; `wayland-backend-0.3.17/src/rs/server_impl/registry.rs:112-135`).
//! Its input is the `ClientData` given to `insert_client`, so the two bits live there and are
//! set **at insert**, never later (niri `ClientState.restricted`, `references/niri/src/niri.rs:7077-7085`;
//! cosmic-comp `security_context: Some`, `references/cosmic-comp/src/state.rs:154-173`):
//!
//! - `restricted`: the stream came through a `wp_security_context_v1` listener (smithay's
//!   `SecurityContextListenerSource`) — the privileged set is hidden (research/30's rule;
//!   niri, cosmic-comp, Hyprland; KWin omits layer-shell and Mura does not,
//!   `references/kwin/src/wayland_server.cpp:131-139`).
//! - `trusted`: the stream is one end of the socketpair zxr handed to a child it spawned
//!   (`spawn_trusted`; kscreenlocker's `WAYLAND_SOCKET`, ADR 0007 amendment) — the mode gate's
//!   exception and the composed set while gated (spec §9).
//!
//! `disconnected` is the one hook a client's death fires; a trusted client's raises `gone`, read
//! on the next tick (spec §2: one loop, no `&mut Zxr` in the hook), and the journal counts it
//! (`trusted_lost`). Nothing here unlocks (ADR 0007 I3). What a loss *means* is the role's
//! ([`TrustedRole`], spec §9 rev 3.13; shell-plane §2.3/§3.2):
//!
//! - **`Primary`** — greeter mode's kiosk child, the greeter program. Its death ends zxr with the
//!   child's exit status: cage's rule (`references/cage/cage.c:99-114` `sigchld_handler` →
//!   `server_terminate`, `:200-215` the child's status becomes cage's), because greetd starts the
//!   session when *its* greeter process — zxr — exits (`greetd/src/session/worker.rs`; research/78
//!   §1), and restarts it on failure (`Restart=always`).
//! - **`Osk`** — the on-screen keyboard, zxr's child in every mode. Restarted on death within
//!   KWin's bound: up to five starts in twenty seconds, then given up with a warning
//!   (`references/kwin/src/inputmethod.cpp:88-96, 916-928`).
//! - **`Other`** — any further `--trusted`/`--shell-fd` connection (the lock program on the desktop
//!   profile arrives on the public socket and is not one): counted, nothing more; its unit
//!   restarts it.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Child;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::Client;
use smithay::wayland::compositor::CompositorClientState;
use smithay::wayland::security_context::{SecurityContext, SecurityContextHandler, SecurityContextListenerSource};

use crate::state::Zxr;

/// What a trusted connection is to zxr (module doc).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TrustedRole {
    #[default]
    Other,
    /// greeter mode's kiosk child: its death ends zxr (cage's rule)
    Primary,
    /// the on-screen keyboard: restarted within KWin's bound
    Osk,
}

/// Per-connection data: the compositor's client state and the two admission bits.
#[derive(Default, Debug)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// arrived through a security context: the privileged set is hidden
    pub restricted: bool,
    /// the socketpair child (greeter/lock program, the OSK): admitted while gated
    pub trusted: bool,
    pub role: TrustedRole,
    /// set by `disconnected`; the tick reads and clears it (`take_trusted_losses`)
    pub gone: AtomicBool,
}

impl ClientState {
    pub fn restricted() -> Self {
        ClientState { restricted: true, ..Default::default() }
    }
    pub fn trusted_as(role: TrustedRole) -> Self {
        ClientState { trusted: true, role, ..Default::default() }
    }
}

/// A child zxr spawned over a socketpair, by role (`--trusted`, `--osk`).
#[derive(Debug)]
pub struct TrustedChild {
    pub role: TrustedRole,
    pub cmd: String,
    pub pid: u32,
    /// OSK: crashes counted since the last quiet 20 s (`inputmethod.cpp:88-96, 916-928`)
    pub crashes: u32,
    pub last_crash_ns: u64,
    /// OSK: gave up restarting
    pub abandoned: bool,
}

/// KWin's restart bound for its input method (`references/kwin/src/inputmethod.cpp:88-96, 916-928`):
/// a crash (a signal death; a plain exit is not restarted) increments a counter that a 20 s
/// single-shot timer clears; the method is restarted while the counter is below five.
pub const OSK_CRASH_MAX: u32 = 5;
pub const OSK_CRASH_WINDOW_NS: u64 = 20_000_000_000;

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        if self.trusted {
            self.gone.store(true, Ordering::Release);
        }
    }
}

/// The predicate every privileged global is created with: not restricted.
pub fn unrestricted(client: &Client) -> bool {
    client.get_data::<ClientState>().map(|c| !c.restricted).unwrap_or(false)
}

/// `wp_security_context_manager_v1`'s own predicate: no context yet (nesting is forbidden,
/// `security-context-v1.xml:36-40`). The same bit today; named separately because the protocol
/// names it separately.
pub fn no_security_context(client: &Client) -> bool {
    unrestricted(client)
}

/// Whether a client is a trusted (socketpair) connection.
pub fn is_trusted(client: &Client) -> bool {
    client.get_data::<ClientState>().map(|c| c.trusted).unwrap_or(false)
}

impl SecurityContextHandler for Zxr {
    fn context_created(&mut self, source: SecurityContextListenerSource, context: SecurityContext) {
        tracing::info!(engine = ?context.sandbox_engine, app_id = ?context.app_id, "security context: listener registered; its clients are restricted");
        self.journal.security_contexts += 1;
        if let Err(e) = self.loop_handle.insert_source(source, move |stream, _, state| {
            // the restricted bit at insert — before the client's registry exists (§5.1)
            match state.dh.insert_client(stream, Arc::new(ClientState::restricted())) {
                Ok(_) => state.journal.clients_restricted += 1,
                Err(e) => tracing::warn!("insert_client (security context): {e}"),
            }
        }) {
            tracing::warn!("security context listener: {e}");
        }
    }
}

/// Admit one end of a socketpair as a trusted client with a role.
pub fn admit_trusted_as(st: &mut Zxr, stream: UnixStream, role: TrustedRole) -> Result<Client, String> {
    let data = Arc::new(ClientState::trusted_as(role));
    st.trusted_clients.push(data.clone());
    let client = st.dh.insert_client(stream, data).map_err(|e| format!("insert_client (trusted): {e}"))?;
    st.journal.clients_trusted += 1;
    tracing::info!(?role, "trusted client admitted over a socketpair");
    Ok(client)
}

/// Admit an fd zxr inherited (`--shell-fd N`: the unit or a wrapper made the socketpair).
pub fn admit_trusted_fd(st: &mut Zxr, fd: i32, role: TrustedRole) -> Result<Client, String> {
    // SAFETY: the fd was named on the command line for this purpose and is owned from here on
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    admit_trusted_as(st, UnixStream::from(owned), role)
}

/// Spawn `cmd` (a shell command line) with one end of a fresh socketpair as `WAYLAND_SOCKET`,
/// and admit the other end as trusted — kscreenlocker's channel (`ksldapp.cpp:377-423`) and
/// KWin's for its input method (`inputmethod.cpp:864-926`, research/75 §4.2). No socket name
/// is given: libwayland-client takes `WAYLAND_SOCKET` first.
pub fn spawn_trusted(st: &mut Zxr, cmd: &str, role: TrustedRole) -> Result<u32, String> {
    use std::os::unix::process::CommandExt;
    let (ours, theirs) = UnixStream::pair().map_err(|e| e.to_string())?;
    let fd = theirs.as_raw_fd();
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c").arg(cmd).env("WAYLAND_SOCKET", fd.to_string()).env_remove("WAYLAND_DISPLAY").env_remove("DISPLAY");
    // SAFETY: async-signal-safe calls only — undo the loop's signal block and clear CLOEXEC on
    // the child's end so it survives the exec
    unsafe {
        c.pre_exec(move || {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for s in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
                libc::sigaddset(&mut set, s);
            }
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
            }
            Ok(())
        });
    }
    let child = c.spawn().map_err(|e| format!("spawn trusted {cmd:?}: {e}"))?;
    drop(theirs);
    let pid = child.id();
    st.children.push(child);
    admit_trusted_as(st, ours, role)?;
    match st.trusted_children.iter_mut().find(|t| t.role == role && role != TrustedRole::Other) {
        Some(t) => t.pid = pid,
        None => st.trusted_children.push(TrustedChild { role, cmd: cmd.to_string(), pid, crashes: 0, last_crash_ns: 0, abandoned: false }),
    }
    Ok(pid)
}

/// What the tick learned from `disconnected`.
#[derive(Default, Debug)]
pub struct Losses {
    pub count: u32,
    /// the greeter-mode kiosk child died: its exit status, when zxr spawned it (cage's rule)
    pub primary: Option<i32>,
    pub osk: bool,
}

/// The tick's read of `disconnected`: which trusted clients went away since the last call.
/// Their data is dropped from the list; the journal counts them.
pub fn take_trusted_losses(st: &mut Zxr, now_ns: u64) -> Losses {
    let mut losses = Losses::default();
    let mut roles = Vec::new();
    st.trusted_clients.retain(|c| {
        if c.gone.swap(false, Ordering::AcqRel) {
            losses.count += 1;
            roles.push(c.role);
            false
        } else {
            true
        }
    });
    if losses.count > 0 {
        st.journal.trusted_lost += losses.count as u64;
    }
    // a lost connection: the role's process is reaped if it has already exited, else told to
    // leave (SIGTERM) and polled from here on — the compositor never blocks on a child
    let mut exited: Vec<(TrustedRole, i32)> = Vec::new();
    for role in roles {
        let pid = st.trusted_children.iter().find(|t| t.role == role).map(|t| t.pid);
        match pid.map(|p| reap(st, p, role, now_ns)) {
            Some(Some(status)) => exited.push((role, status)),
            Some(None) => tracing::warn!(?role, "trusted client's connection gone; its process is still alive — SIGTERM sent, reaped when it exits"),
            None => {
                if role == TrustedRole::Other {
                    tracing::warn!("trusted client gone — the composed set is what remains; nothing unlocks (ADR 0007 I3), its unit restarts it");
                } else {
                    exited.push((role, 0));
                }
            }
        }
    }
    // the children told to leave earlier: exited now? SIGKILL past the grace (KWin `terminate()`
    // then `kill()` on its input method; cage waits on the process, not the connection)
    let mut i = 0;
    while i < st.reaping.len() {
        let r = &mut st.reaping[i];
        match r.child.try_wait() {
            Ok(Some(status)) => {
                let r = st.reaping.remove(i);
                exited.push((r.role, exit_code(status)));
                continue;
            }
            Ok(None) if !r.killed && now_ns.saturating_sub(r.since_ns) > REAP_GRACE_NS => {
                // SAFETY: a pid this process spawned and has not yet reaped
                unsafe {
                    libc::kill(r.child.id() as i32, libc::SIGKILL);
                }
                r.killed = true;
                tracing::warn!(pid = r.child.id(), ?r.role, "trusted child did not exit within the grace; SIGKILL");
            }
            Ok(None) => {}
            Err(_) => {
                st.reaping.remove(i);
                continue;
            }
        }
        i += 1;
    }
    for (role, status) in exited {
        match role {
            TrustedRole::Primary => {
                losses.primary = Some(status);
                tracing::warn!(status, "trusted primary (the greeter program) gone — greeter mode ends with its status (cage's rule)");
            }
            TrustedRole::Osk => {
                // KWin restarts on `CrashExit` only: a signal death. A plain exit is the OSK's own word.
                losses.osk = status > 128;
                tracing::warn!(status, crash = losses.osk, "trusted OSK gone");
            }
            TrustedRole::Other => {}
        }
    }
    losses
}

/// How long a child whose connection is gone may take to exit after SIGTERM before SIGKILL.
pub const REAP_GRACE_NS: u64 = 1_000_000_000;

/// A spawned child whose connection is gone but whose process has not exited: told to leave,
/// polled each tick (`take_trusted_losses`), never waited on.
pub struct Reaping {
    pub child: Child,
    pub role: TrustedRole,
    pub since_ns: u64,
    pub killed: bool,
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| {
        use std::os::unix::process::ExitStatusExt;
        128 + status.signal().unwrap_or(0)
    })
}

/// Reap a spawned child by pid without blocking: its exit status in the shell's convention
/// (128 + signal) if it has exited; else SIGTERM and `None` — the tick polls it (`Reaping`).
fn reap(st: &mut Zxr, pid: u32, role: TrustedRole, now_ns: u64) -> Option<i32> {
    let i = st.children.iter().position(|c| c.id() == pid)?;
    let mut child = st.children.remove(i);
    match child.try_wait() {
        Ok(Some(status)) => Some(exit_code(status)),
        Ok(None) => {
            // SAFETY: a pid this process spawned and has not yet reaped
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            st.reaping.push(Reaping { child, role, since_ns: now_ns, killed: false });
            None
        }
        Err(_) => Some(0),
    }
}

/// The OSK died: restart it inside KWin's bound (`inputmethod.cpp:88-96`: five starts in twenty
/// seconds, then stop with a warning). Returns whether it was restarted.
pub fn restart_osk(st: &mut Zxr, now_ns: u64) -> bool {
    let Some(i) = st.trusted_children.iter().position(|t| t.role == TrustedRole::Osk) else { return false };
    if st.trusted_children[i].abandoned {
        return false;
    }
    {
        let t = &mut st.trusted_children[i];
        if now_ns.saturating_sub(t.last_crash_ns) >= OSK_CRASH_WINDOW_NS {
            t.crashes = 0;
        }
        t.crashes += 1;
        t.last_crash_ns = now_ns;
        if t.crashes >= OSK_CRASH_MAX {
            t.abandoned = true;
            tracing::error!(crashes = t.crashes, window_s = OSK_CRASH_WINDOW_NS / 1_000_000_000, "OSK keeps crashing; not restarted (KWin's bound)");
            return false;
        }
    }
    let cmd = st.trusted_children[i].cmd.clone();
    match spawn_trusted(st, &cmd, TrustedRole::Osk) {
        Ok(pid) => {
            st.journal.osk_restarts += 1;
            tracing::info!(pid, "OSK restarted");
            true
        }
        Err(e) => {
            tracing::error!("OSK restart: {e}");
            false
        }
    }
}
