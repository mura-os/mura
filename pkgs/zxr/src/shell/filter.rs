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
//! (`trusted_lost`). The unit restarts the program; nothing here unlocks (ADR 0007 I3).

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::Client;
use smithay::wayland::compositor::CompositorClientState;
use smithay::wayland::security_context::{SecurityContext, SecurityContextHandler, SecurityContextListenerSource};

use crate::state::Zxr;

/// Per-connection data: the compositor's client state and the two admission bits.
#[derive(Default, Debug)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// arrived through a security context: the privileged set is hidden
    pub restricted: bool,
    /// the socketpair child (greeter/lock program, the OSK): admitted while gated
    pub trusted: bool,
    /// set by `disconnected`; the tick reads and clears it (`take_trusted_losses`)
    pub gone: AtomicBool,
}

impl ClientState {
    pub fn restricted() -> Self {
        ClientState { restricted: true, ..Default::default() }
    }
    pub fn trusted() -> Self {
        ClientState { trusted: true, ..Default::default() }
    }
}

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

/// Admit one end of a socketpair as a trusted client.
pub fn admit_trusted(st: &mut Zxr, stream: UnixStream) -> Result<Client, String> {
    let data = Arc::new(ClientState::trusted());
    st.trusted_clients.push(data.clone());
    let client = st.dh.insert_client(stream, data).map_err(|e| format!("insert_client (trusted): {e}"))?;
    st.journal.clients_trusted += 1;
    tracing::info!("trusted client admitted over a socketpair");
    Ok(client)
}

/// Admit an fd zxr inherited (`--shell-fd N`: the unit or a wrapper made the socketpair).
pub fn admit_trusted_fd(st: &mut Zxr, fd: i32) -> Result<Client, String> {
    // SAFETY: the fd was named on the command line for this purpose and is owned from here on
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    admit_trusted(st, UnixStream::from(owned))
}

/// Spawn `cmd` (a shell command line) with one end of a fresh socketpair as `WAYLAND_SOCKET`,
/// and admit the other end as trusted — kscreenlocker's channel (`ksldapp.cpp:377-423`) and
/// KWin's for its input method (`inputmethod.cpp:864-926`, research/75 §4.2). No socket name
/// is given: libwayland-client takes `WAYLAND_SOCKET` first.
pub fn spawn_trusted(st: &mut Zxr, cmd: &str) -> Result<(), String> {
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
    st.children.push(child);
    admit_trusted(st, ours)?;
    Ok(())
}

/// The tick's read of `disconnected`: how many trusted clients went away since the last call.
/// Their data is dropped from the list; the journal counts them.
pub fn take_trusted_losses(st: &mut Zxr) -> u32 {
    let mut lost = 0;
    st.trusted_clients.retain(|c| {
        if c.gone.swap(false, Ordering::AcqRel) {
            lost += 1;
            false
        } else {
            true
        }
    });
    if lost > 0 {
        st.journal.trusted_lost += lost as u64;
        tracing::warn!(lost, "trusted client gone — the composed set is what remains; nothing unlocks (ADR 0007 I3), its unit restarts it");
    }
    lost
}
