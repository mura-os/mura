//! The EIS server (spatial-input §1a line 80 "emulated / remote input — libei clients; **zxr is
//! the EIS server**; the portal brokers consent"; research/68 §2 line 142 and D3 line 472).
//!
//! **Why a protocol and not a device**: libei's stated purpose is *separation* (emulated input is
//! a distinct channel), *distinction* (the server always knows which client is injecting) and
//! *control* (filter, discard, pause — a locked screen pauses every emulated device)
//! (`references/libei/README.md:32-71`). mutter, KWin and cosmic-comp all terminate EIS in the
//! compositor (research/68 §2). Here every EI event becomes a [`Sample`] with `Flags::EMULATED`
//! — the flag *is* the distinction; the mode stage's lock gate and the text stage's "physical
//! keys suppress the OSK" read it.
//!
//! **Shape**: smithay's `EiInput` (`references/smithay/src/backend/libei/mod.rs:61` the source,
//! `:72` `new(context)`, `:163` `impl EventSource` — a `reis::calloop::EisRequestSource` plus a
//! channel; `Connected` is when seats and devices may be added, `Event(InputEvent)` is the
//! backend-agnostic event, `TextKeysym`/`TextUtf8` are the `ei_text` interface) behind a
//! `reis::calloop::EisListenerSource` on a Unix socket — the module-doc example (`mod.rs:3-37`)
//! and cosmic-comp's `setup_ei` (`references/cosmic-comp/src/libei.rs:61-165`: one seat
//! `"default"`, a keyboard with the compositor's xkb config, a pointer, an absolute pointer with
//! per-output regions, touch; on disconnect it releases the keys and buttons that client held).
//!
//! **Socket**: `$XDG_RUNTIME_DIR/zxr-eis-<pid>` (logged; `LIBEI_SOCKET` is the client-side
//! convention `reis::ei::Context::connect_to_env` reads, relative to `XDG_RUNTIME_DIR`). The
//! portal fd path — `RemoteDesktop.ConnectToEIS` handing a socketpair end to the client — is
//! spatial-sharing §2's, later; this socket is the development seam, and **anything that can
//! reach the runtime dir can inject** — flagged (the unit will scope it, and the portal is the
//! consent gate the design names).
//!
//! **Devices offered**: keyboard (the seat's xkb config) and relative pointer. No absolute
//! pointer / touch: those need `ei_device.region`s, i.e. an output rectangle, and zxr's pointer
//! space is a plane's local coordinates or an unbounded angular ray (§8 lines 324-335; ADR 0013
//! constraint 2) — what region to advertise is an open item (report). `ei_text` is not offered
//! yet: it needs the compositor-as-IM path (`TextInputHandle::set_compositor_input_method`,
//! `references/smithay/src/wayland/text_input/text_input_handle.rs:134`) — future.
//!
//! **Mapping**: [`super::libinput::raw_of`] + [`super::libinput::sample_of`] with
//! `Flags::EMULATED` — exactly libinput's, one flag apart. `hmdButtons` roles apply too (an EI
//! client can press the head's select — the reserved stage sees `Button::System` from EI with the
//! EMULATED flag and may refuse it; its call).
//!
//! Budget (invariant 9): one loop dispatch per readable EI socket; per event one `match` and one
//! queued `Sample`; the listener costs nothing while no client is connected.

use std::path::PathBuf;

use smithay::backend::input::InputEvent;
use smithay::backend::libei::{EiInput, EiInputEvent};
use smithay::input::keyboard::XkbConfig;
use smithay::reexports::calloop::{LoopHandle, PostAction};
use smithay::reexports::reis::{calloop::EisListenerSource, eis};

use super::libinput::{raw_of, sample_of};
use super::{Flags, SourceKind};
use crate::state::Zxr;

/// The EIS server's state on `Zxr`.
#[derive(Default)]
pub struct EiServer {
    pub socket_path: Option<PathBuf>,
    /// connected sender clients
    pub clients: u32,
    pub connections_total: u64,
    /// counters (journal patch in the lane report)
    pub events: u64,
    pub events_unmapped: u64,
    pub text_events: u64,
}

/// The socket path: `$XDG_RUNTIME_DIR/zxr-eis-<pid>` (`/tmp` when the dir is unset — dev only).
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(format!("zxr-eis-{}", std::process::id()))
}

/// Bind the listener and register it. `ZXR_NO_EI=1` skips it. Returns the path bound.
pub fn start(st: &mut Zxr, handle: &LoopHandle<'static, Zxr>) -> Result<Option<PathBuf>, String> {
    if std::env::var_os("ZXR_NO_EI").is_some() {
        tracing::info!("EIS server skipped (ZXR_NO_EI)");
        return Ok(None);
    }
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = eis::Listener::bind(&path).map_err(|e| format!("EIS listener {}: {e}", path.display()))?;
    let handle2 = handle.clone();
    handle
        .insert_source(EisListenerSource::new(listener), move |context, _, st: &mut Zxr| {
            accept(st, &handle2, context);
            Ok(PostAction::Continue)
        })
        .map_err(|e| format!("EIS listener source: {e}"))?;
    tracing::info!(path = %path.display(), "EIS server listening (libei sender contexts; samples carry Flags::EMULATED)");
    st.ei.socket_path = Some(path.clone());
    Ok(Some(path))
}

/// One accepted sender context → an `EiInput` source on the loop.
fn accept(st: &mut Zxr, handle: &LoopHandle<'static, Zxr>, context: eis::Context) {
    st.ei.connections_total += 1;
    let source = EiInput::new(context);
    let res = handle.insert_source(source, move |event, connection, st: &mut Zxr| match event {
        EiInputEvent::Connected => {
            st.ei.clients += 1;
            let seat = connection.add_seat("default");
            // the seat's keymap: smithay's default xkb config, the same the wl_keyboard was
            // created with (`Zxr::new`: `add_keyboard(Default::default(), …)`)
            if let Err(e) = seat.add_keyboard("zxr virtual keyboard", XkbConfig::default()) {
                tracing::warn!("EI keyboard device: {e:?}");
            }
            seat.add_pointer("zxr virtual pointer");
            tracing::info!(clients = st.ei.clients, "EI client connected: seat default, keyboard + pointer offered");
        }
        EiInputEvent::Disconnected => {
            st.ei.clients = st.ei.clients.saturating_sub(1);
            // keys/buttons the client still held are not released here (cosmic-comp's
            // release_ei_keyboard/pointer) — future, flagged in the report
            tracing::info!(clients = st.ei.clients, "EI client disconnected");
        }
        EiInputEvent::Event(ev) => {
            st.ei.events += 1;
            match &ev {
                InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. } => {}
                _ => match raw_of(&ev) {
                    Some((raw, t)) => {
                        let s = sample_of(raw, t, &st.peripherals.roles, Flags::EMULATED);
                        debug_assert!(s.flags.contains(Flags::EMULATED));
                        if s.kind == SourceKind::Head {
                            st.peripherals.hmd_buttons += 1;
                        }
                        st.input.push(s);
                    }
                    None => st.ei.events_unmapped += 1,
                },
            }
        }
        EiInputEvent::TextKeysym { .. } | EiInputEvent::TextUtf8 { .. } => {
            // `ei_text` is not offered on the seat; a client that sends it anyway is counted
            st.ei.text_events += 1;
        }
    });
    if let Err(e) = res {
        tracing::warn!("EI input source: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_per_pid_under_runtime_dir() {
        let p = socket_path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, format!("zxr-eis-{}", std::process::id()));
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(d) => assert_eq!(p.parent().unwrap(), PathBuf::from(d)),
            None => assert_eq!(p.parent().unwrap(), PathBuf::from("/tmp")),
        }
    }
}
