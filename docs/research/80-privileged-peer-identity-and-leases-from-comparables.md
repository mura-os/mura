# 80 — One privileged client among same-uid peers: identity, leases and the no-controller default, from comparables

**Date:** 2026-09-29. **What this is:** the comparables pass C0 requires before it is written
(ADR 0006 amendment 4 D6; [specs/composition.md §5.3](../../specs/composition.md) (a)–(d), gate
§7.7; [implementation-path.md §3 "The C-track"](../architecture/implementation-path.md)). C0 is
specified as *what* — server-derived identity at accept, the controller role as a lease "granted
to a client whose peer identity is the session compositor's unit", fail-closed verbs, a runtime
that stays usable with no controller — but not *how*. `SO_PEERCRED` yields `{pid, uid, gid}`;
zxr and every OpenXR app run as the same uid; so the identity primitive is a real design decision,
and the shipping per-user services that face the same problem answer it differently. This doc
reads them (AGENTS rule 7) and separates what they converge on from what the owner must rule
(rule 8). It builds no code. The failure-mode record that motivates C0 is research/79 §4a and is
not re-read here.

**Three questions, stated up front.**

- **Q1 — the identity primitive.** How does a per-user service tell its one privileged peer from
  ordinary same-uid clients, and why that way?
- **Q2 — lease semantics.** One holder; grant and refusal; revocation on disconnect; what a second
  would-be holder gets; what happens when the holder restarts (D4 proves zxr restarts inside the
  session); what the service does with no holder.
- **Q3 — retrofitting authorisation onto verbs that were open.** zxr already calls `libmonado`'s
  system verbs; so does `monado-ctl`. What did comparables do when they put a gate on something
  that every client could previously call, and is the check a per-connection class or per-call?

**Sources.** Pinned clones in `references/` (MANIFEST.json): `pipewire` (2943841e), `wireplumber`
(81606cbb — pinned by this pass), `seatd` (427b5d95), `libei` (a9bf31da), `systemd`, `kwin`
(d84a316b), `kscreenlocker` (8b41a223), `hyprland` (c6f11721), `monado` (upstream study pin),
`aosp-system-core`. All are depth-1 clones, so every "why" below is an in-tree comment, a man
page, NEWS, or an upstream commit/MR fetched by API and marked **[external]**. Line numbers were
re-read against the pins while writing.

## 0. Baselines — Monado's IPC today, and Mura's unit layout

- **Accept reads nothing.** `ipc_server_mainloop_linux.c:239` `accept(ml->listen_socket, NULL,
  NULL)` → `ipc_server_handle_client_connected(vs, fd)` (`ipc_server_process.c:944`), which
  only finds a free thread slot (`:958-965`, `IPC_MAX_CLIENTS 32`, `ipc_protocol.h:41`, set by
  MR !2644 "ipc: bump max clients to 32" [external]). No `getsockopt` anywhere under `src/xrt/ipc/`.
- **Identity is client-asserted.** `ipc_client_connection.c:340` `desc.pid = pid; // Extra
  info.` → `ipc_server_handler.c:343` `ics->client_state.pid = client_desc->pid`. Nothing
  reads it for a decision.
- **The system verbs are open to every connection.** `ipc_handle_system_set_primary_client`
  (`ipc_server_handler.c:1563-1570` → `ipc_server_set_active_client`), `set_focused_client`
  (`:1573-1578`, logs "UNIMPLEMENTED"), `toggle_io_client` (`:1581-1588`),
  `set_client_io_blocks` (`:1591-1607`, added by MR !2727, 2026-02 [external], with no
  authorisation discussion in its description). Callers today: `monado-ctl` (`targets/ctl/
  main.c:152` `set_primary`), `libmonado` (`monado.h:282-322`), and through it zxr
  (`pkgs/zxr/src/state.rs:208,865`; research/67) and `dev-session`.
- **The default policy with no controller already exists.** `update_server_state_locked`
  (`ipc_server_process.c:598-655`): if the active client is gone or not displayable, fall
  through to the first non-overlay active session, else the idle wallpaper (`set_idle`);
  comment `:616-621` names "the monado-ctl application or other app making a 'set active
  application' ipc call" as the expected caller. This is composition §5.3(c)'s rule; it needs
  no change.
- **Monado already admits a pre-connected fd.** On Android the client makes a `socketpair`
  and hands one end over Binder (`ipc/android/.../Client.java:191-197`
  `ParcelFileDescriptor.createSocketPair(); monado.connect(theirs)`); the service side takes it
  with `nativeAddClient` → `ipc_server_mainloop_add_fd` (`service-lib/service_target.cpp:99,
  207-211`; `ipc_server_mainloop_android.c:98,185`) into the same
  `ipc_server_handle_client_connected`. Binder's `getCallingUid` is not consulted (grep empty).
  So "identity = who handed you the fd" is a path Monado's IPC core already has; Linux simply
  never exposes it.
- **systemd is already in the loop.** `XRT_HAVE_SYSTEMD` (`CMakeLists.txt:267`) makes the
  service take `sd_listen_fds` (`ipc_server_mainloop_linux.c:61-69`, exactly one fd,
  `SD_LISTEN_FDS_START + 0`); the shipped `monado.in.socket` is `ListenStream=%t/monado_comp_ipc`
  with no `SocketMode`. Mura runs it as the user's `monado.socket`/`monado.service`
  (`modules/xr/default.nix:22-27`), and zxr as `mura-compositor.service`, `Type=notify`, in
  `mura-session.target` (`modules/os/session.nix:38-43,140-146`). In greeter mode zxr runs as
  the `greeter` user with that user's own Monado. So: the controller is *always* a named systemd
  user unit, and Monado is *never* its parent (socket activation makes them independent).
- **Upstream has no position.** All-state MR/issue searches for `SO_PEERCRED`, peercred,
  authoriz*, permission, privileged, primary client, set_focused_client, io_blocks, client
  identity, trusted client (2026-09-29, 73 hits) contain nothing on IPC client authorisation.
  The one identity gate Monado ever had — Android's `OPENXR` permission on connect — was
  *removed* by MR !1213 (2022-04 [external]: "it will cause permission issue if permission
  container is installed after application"). `!1354` (the `comp_multi` listener sketch, C3's
  candidate) says nothing about who may listen. C0 would be the first authorisation code in
  Monado's IPC.

## 1. PipeWire — a class per listening socket, forged keys refused, nothing single-holder

*Problem it solves:* one per-user media daemon; a session manager (WirePlumber) must be able to
set every other client's permissions; sandboxed apps must be distinguishable from host apps.

- **Credentials are collected, not decided on.** `module-protocol-native.c:641-662` reads
  `SO_PEERCRED` into `pipewire.sec.pid/uid/gid` (+ `SO_PEERGROUPS`, `SO_PEERSEC`). The pid's only
  uses are the Flatpak probe (`/proc/<pid>/root/.flatpak-info`, `flatpak-utils.h:66-105`) and
  the portal-pid match (`module-portal.c:97-118`, against the session bus owner of
  `org.freedesktop.portal.Desktop`).
- **The identity primitive is the listener.** Every client gets `pipewire.sec.socket = <name of
  the socket it arrived on>` (`module-protocol-native.c:1513`); by default two sockets exist,
  `pipewire-0` and `pipewire-0-manager` (`:1722-1738`). `module-access.c:214-218` maps socket →
  access class; the default table is `{"pipewire-0-manager": "unrestricted"}` (`:349-354`).
  Only `"unrestricted"` grants anything (`PW_PERM_ALL`, `:242-248`); everything else waits.
  **Why [external, MR !1727, 2023-10]:** "session manager has pipewire.access=unrestricted, all
  other applications get pipewire.access=restricted, so that session manager can decide … module-
  access could previously identify the session manager (the 'allowed client') only based on the
  executable path, which is a bit unusual approach. … allowing access control with the usual unix
  permission system." Executable-path identity was **removed** at the same time (commit
  `f89757e1` [external]).
- **Clients cannot forge the class.** `impl-client.c:183-185` `/* Refuse all security keys */ if
  (spa_strstartswith(key, "pipewire.sec.")) goto deny;`; `pipewire.access` writes are ignored
  (`:171-177`). NEWS: "Security related properties are made read-only now."
- **Per-connection class at admission, per-call bitmask afterwards.** The access check "is only
  performed once per client" (`module-access.c:35-37`); thereafter one dispatcher line checks each
  method's static requirement against the resource's bits (`module-protocol-native.c:413-423`:
  `required = demarshal[msg->opcode].permissions | PW_PERM_X; if ((required & permissions) !=
  required) … -EACCES`). A refused call gets an error event, not a disconnect. A client may only
  *lower* its own permissions (`impl-client.c:747-748, 786-787`).
- **No manager present: restricted clients wait forever.** "Busy" = the daemon stops reading the
  socket (`impl-client.c:241-246`, `module-protocol-native.c:450-467`); no timeout, no refusal
  (`doc/dox/internals/access.dox:67-73`). The one "is a manager present" check is capability-
  advertised, not lease-based: portal clients are gated only if some client has set
  `pipewire.access.portal.gate-supported=true` (`impl-core.c:193-225`; MR !2809 [external]).
- **What actually keeps a same-uid stranger off the manager socket: nothing in code.** The user
  `pipewire.socket` sets no `SocketMode`; the *system* instance does (`pipewire-manager.socket`
  `SocketMode=0600`, MR !1753 [external]: "so that restricted clients cannot use it to get
  unrestricted access. It is assumed that … wireplumber is running as pipewire user"). For the
  per-user daemon the separation is meaningful only against a sandbox whose mount namespace lacks
  the `-manager` path. PipeWire's authors never claimed same-uid isolation.
- **Retrofit shape (`access.legacy`).** Before 2023: exe-path allow-lists and a *client-asserted*
  `pipewire.client.access` (commit `d85862af` [external]). The socket model was added beside it;
  the old behaviour stayed the default under a flag — `module-access.c:321-328` `/* When time
  comes, we should change this to false */ impl->legacy = true;` — and is still the default three
  years on; removed options warn rather than fail (`:330-337`); both sockets are always created so
  old and new managers both work; WirePlumber's switch was version-gated
  (`wireplumber/src/main.c:237-240` `pw_check_library_version(0, 3, 84)`).

*WirePlumber, the grant side:* it identifies itself only by connecting to the `-manager` socket
first (`main.c:237-240`), grants by pushing permission arrays (`lib/wp/permission-manager.c:
293-345`), and on restart re-evaluates every pre-existing client through its object manager
(`lib/wp/object-manager.c:45-48`; `modules/module-standard-event-source.c:417-427`). The daemon
neither revokes grants when the manager disconnects nor tracks who granted them; lifecycle
coupling is systemd's (`wireplumber.service.in` `BindsTo=pipewire.service`). **There is no
single-holder concept:** any number of clients on the manager socket are unrestricted, and
`wpctl` connects there by default (NEWS 0.5.15; PipeWire MR !2776 [external]).

*Transfer:* the "class stamped at accept, forbid the client from writing it, one dispatcher
check" mechanics transfer whole. The trust claim does not: PipeWire draws its boundary at the
sandbox, and treats every unsandboxed same-uid process as trusted — the very population Mura's
question is about. "Wait forever with no manager" is the opposite of §5.3(c).

## 2. seatd — a filesystem-gated daemon with a clean single-holder machine

*Problem it solves:* hand seat devices to exactly one compositor session at a time, across a uid
boundary (seatd is root; clients are users).

- **Credentials read, only logged.** `seatd/client.c:30-40` `SO_PEERCRED` → `client->uid/gid/pid`
  (`:77-101`); the only consumer is `server.c:145-146` `log_infof("New client connected (pid: %d,
  uid: %d, gid: %d)")`. No comparison to anything. The identity primitive is **who can connect**:
  `seatd.c:42-52` `chmod 0770` + `chown` to `-u`/`-g`; `contrib/systemd/seatd.service:7-8`
  "Specify the group you'd like to grant access"; `seatd-launch.c:114-125` per-session `chmod
  0700` "Restrict access to the socket to just us". Root is not special-cased.
- **The builtin backend is an inherited fd.** `libseat/backend/seatd.c:677-725` `socketpair()` +
  `fork()`; the child runs `server_add_client(&server, fd)` (`:694-700`) — no listener, no accept;
  `client.c:106-110` "The built-in backend version of seatd should terminate once its only client
  disconnects." Same shape as libei's fd backend (§3) and Monado's Android path (§0).
- **Single holder, holder-initiated hand-off, no takeover.** `seat_open_client` (`seat.c:550-554`)
  → `EBUSY` "seat already has an active client". On a VT-bound seat a second `OPEN_SEAT` is refused
  outright (`:199-204` `EBUSY`); on a non-VT-bound seat it is **queued** in `CLIENT_NEW` until
  activated (`:232-242`, comment `:25-29`). Only the active client may request a switch
  (`:663-667` `EPERM` "client is not active"). Switching is disable → ack → enable with devices
  *deactivated*, not closed (`drm_drop_master`, `EVIOCREVOKE`; `:603-606` "certain device fds …
  must maintain the exact same file description").
- **Revocation is the connection's lifetime; no holder → idle.** On disconnect all devices are
  revoked and `seat_activate` picks the next queued client, else the VT match, else the first;
  none → `log_infof("No clients on %s to activate")` (`:157-159`) and the seat idles. A restarted
  holder is a brand-new client (`:206-210` "client cannot be reused"); nothing is remembered.
- **Per-connection state machine, never per-call credentials.** `seat_open_device` `:333-337`
  `EPERM` "client is not active" — the check is the client's *state*, decided by the protocol.

*Transfer:* the identity half does not (uid-crossing boundary; Mura's peers share a uid). The
lease half transfers almost verbatim: one active holder, EBUSY-or-queue for a second, holder-
initiated hand-off, revoke on disconnect, idle with none, no memory across restart.

## 3. libei — the library refuses to decide; the preferred backend is an inherited fd

*Problem it solves:* an input-emulation server (the compositor) admitting emulated-input clients,
some via a portal, some spawned by the compositor itself.

- **What the server derives from credentials: only the pid, on demand, socket backend only.**
  `libeis-socket.c:219-234` `eis_backend_socket_get_client_pid` (`SO_PEERCRED` → `.pid`); accept
  itself reads nothing (`:106-123`); the socket file is `fchmod 0600` (`:194-195`).
- **The embedder decides.** `libeis.h:273-280` `EIS_EVENT_CLIENT_CONNECT`: "The server is expected
  to either call eis_client_connect() or eis_client_disconnect()"; `:58-63` "It is up to the
  implementation to disconnect clients that it does not want to allow"; the client's declared name
  is documented as untrusted (`:1050-1055`). `README.md:249-253` lists authentication as an open
  question outside Flatpak.
- **The fd backend is the recommended identity, with the reason stated.** `libeis.h:772-779`:
  "This is the preferred backend to use for EIS implementations as it keeps the file descriptors
  private for each client and is not subject to race conditions or unauthorized clients attempting
  to connect." `:791-799` on the socket backend: "it is up to the EIS implementation to
  authenticate clients. In almost all cases, the EIS implementation should use
  eis_setup_backend_fd() instead. This backend is primarily useful for testing and debugging."
  `libeis-fd.c:62-96`: `socketpair`, one end wrapped as the client, the other returned to the
  embedder to hand over. The portal path is exactly this: xdg-desktop-portal authenticates, opens
  the connection, hands the fd to the client, "the portal has no further influence"
  (`README.md:232-242`).
- **How embedders actually decide (the three in the corpus).** mutter: fd backend only
  (`meta-eis.c:319-323`). KWin input-capture: **strict single holder per context** — on
  `CLIENT_CONNECT`, disconnect if the direction is wrong, disconnect if `m_client` is already set
  ("unexpected client connection"), else connect and remember (`plugins/eis/eisinputcapture.cpp:
  237-251`); the context is destroyed when the requesting D-Bus name dies (`eisbackend.cpp:63-70`).
  KWin↔Xwayland: the one same-uid **spawner-knows-the-pid** check in the corpus —
  `xwaylandeiscontext.cpp:35-44` `if (kwinApp()->xwaylandPid() != pid) { … "Non-xwayland process"
  … eis_client_disconnect }`, over a socket KWin `unlink()`s immediately and passes as
  `LIBEI_SOCKET=/proc/self/fd/N` (`eisbackend.cpp:49-59`, "resorting to this hack", libei issue
  63 [external]). A plain `pid_t` compare — racy by systemd's analysis (§4).

*Transfer:* the fd backend transfers if something can hand zxr a pre-connected fd — but Monado is
not zxr's spawner and socket activation keeps them independent (§0). The spawner-pid check
transfers only where the daemon spawned the peer, which it does not. The single-holder-per-
context rule and name-death revocation transfer as lease semantics.

## 4. systemd — the primitive "identity is the peer's unit" actually requires

*Problem it solves:* PID 1 and the user manager must attribute messages on a shared socket to the
unit whose process sent them, without being fooled by pid reuse.

- **Read a pidfd, not a pid.** `socket-util.c:960-980` `getpeerpidref`: `SO_PEERPIDFD` first, fall
  back to `SO_PEERCRED` only when unsupported. `pidref.c:83-86`: compare pidfd inode numbers "so
  that we don't spuriously consider two PidRefs equal if the pid has been reused once";
  `pidref.c:406-430` `pidref_verify` after every `/proc` read: "we might have read the data from a
  recycled PID." `man/sd_pid_get_owner_uid.xml:296-299` on the pid-based `sd_peer_get_*`: "not
  suitable for authorization purposes, as they are subject to races"; `:282-284` on the pidfd
  variants: "not subject to recycle race conditions as the process is pinned by the file
  descriptor during the whole duration of the invocation."
- **pid → user unit is `/proc/<pid>/cgroup` parsing, verified.** `sd-login.c:175-197`
  `sd_pidfd_get_user_unit`: `pidfd_get_pid` → `sd_pid_get_user_unit` → `pidfd_verify_pid` → return.
  `cgroup-util.c:569-595, 924-958, 977-989` strip `user@<uid>.service`/`session-<id>.scope` and
  parse the unit name. `man:222-231`: fails `-ENODATA` for processes not under a user manager.
- **`NotifyAccess` is the worked example of "trust = the peer's unit".** The notify socket is
  `SO_PASSCRED` + `SO_PASSPIDFD` (`core/manager.c:1128-1135`; `:5324-5331` "fall back to using
  parent PID from ucreds and accept some raciness" on old kernels); each datagram's sender becomes
  a `PidRef` (`shared/notify-recv.c:140-217`), mapped to a unit by cgroup membership plus watched
  pids (`manager.c:3040-3147`; `cgroup.c:3474-3477` "a process might be owned by multiple units,
  we return only one"); then `service_notify_message_authorized` (`core/service.c:5390-5433`):
  `none` → refuse; `all` → any pid in the unit's cgroup; `main`/`exec` → `pidref_equal` against the
  pid systemd itself spawned. It is per-message because the socket is a datagram socket — there
  is no connection to attach a class to. Documented caveat (`man/systemd.service.xml:1187-1214`):
  attribution fails if the sender exits before the message is processed; `sd_notify_barrier()` is
  the remedy.
- **sd-bus states the posture:** `bus-socket.c:289-291` "We don't do any real authentication here.
  Instead, if the owner of this bus wanted authentication they should have checked SO_PEERCRED
  before even creating the bus object."

*Transfer:* directly — zxr *is* a named user unit (`mura-compositor.service`), Monado already
links libsystemd, so `getpeerpidref` → `sd_pidfd_get_user_unit` → compare to a configured unit
name is available, recycle-safe, and yields a stable admin-visible identity. What no comparable
does is use it as a per-user service's authorisation gate; systemd uses it in PID 1 / the user
manager for its own spawned services (`main`/`exec`) or their cgroups (`all`). KWin uses the same
lookup only to *lower* a client's class (`isSandboxed`, §5), never to raise one.

## 5. KWin and kscreenlocker — helpers over a socketpair; credential heuristics dropped one by one

*Problem it solves:* a compositor with privileged protocols (window management, fake input,
screencast, lock overlay) and helpers it launches itself (greeter, input method, Xwayland).

- **Three identity primitives are computed; only two are trusted.** (a) `wl_client_get_
  credentials` → pid → `/proc/<pid>/exe`, and `isSandboxed(pid)` = logind slice `app.slice` +
  unit prefix `app-flatpak-`/`snap.` (`wayland/clientconnection.cpp:31-57, 94-99`; commit
  `97b5274a` 2026-06 [external]). (b) `wp_security_context_v1`: the sandbox engine hands KWin a
  listen fd and every arrival is tagged with its app-id, forced sandboxed (`wayland/display.cpp:
  271-285`; `clientconnection.cpp:191-195`; commit `4f9531ad` 2023-11 [external]: "securely
  identify the client for a given connection, without relying on the process name"). (c) **A
  compositor-made socketpair for each spawned helper**: `wayland_server.cpp:649-666`
  `socketpair(...); ret.connection = m_display->createClient(sx[0]); ret.fd = sx[1];` — greeter,
  Xwayland, input method each keep their `ClientConnection*` (`:668-713`). The header is explicit
  that credentials are useless on these: `clientconnection.h:64-69` "if the ClientConnection got
  created with Display::createClient the pid will be identical to the process running the
  KWin::Display." Identity is pointer equality with the stored connection.
- **What is gated on what — the global filter, once per connection.** `wayland_server.cpp:131-164`
  `allowInterface`: Xwayland-only globals for the Xwayland connection; the input-method connection
  sees everything; the restricted set (`org_kde_plasma_window_management`, `org_kde_kwin_fake_
  input`, `zkde_screencast_unstable_v1`, `kde_lockscreen_overlay_v1`, …) is hidden **only from
  sandboxed clients**; `return true` otherwise. **An unsandboxed same-uid client gets every
  privileged global.** The exe path is not consulted by the filter at all.
- **Why the credential heuristics were removed — the arc, in upstream's words [external].**
  2019 (`f3247761`): exe path + sha256 + `.desktop` `X-KDE-Wayland-Interfaces` allow-list. 2023
  (`4016406e` "Drop isTrustedOrigin check"): "sandboxed apps could have a different mount
  namespace to kwin, therefore lying about the executable path was doable. … **Anything not
  sandboxed can circumvent these checks anyway.** This significantly improves application launch
  time." 2026 (`f9bf0ee6`): "it provides pseudo-security. The desktop files can be changed or
  overriden by the user, which negates all the benefits." What remains of exe-path identity is a
  heuristic with a TODO to remove it (`xdgactivationv1.cpp:38-43` `== "plasmashell"`; commit
  `17cda3e9` [external]). `org_kde_kwin_fake_input`'s in-protocol `authenticate` sets a flag with
  `// TODO: make secure` (`backends/fakeinput/fakeinputbackend.cpp:118-124`).
- **The lock role: re-grantable only by the compositor, only to the child it spawns; fail closed.**
  `initScreenLocker` (`wayland_server.cpp:609-647`): on `aboutToStartGreeter` destroy the previous
  connection, make a fresh socketpair, hand the fd to ksld; on `unlocked` destroy it. Commit
  `92f1fabe` 2024-06 [external]: "Otherwise, Wayland objects and protocol errors can cause the new
  instance to not start." kscreenlocker `ksldapp.cpp:383-423` puts the fd in `WAYLAND_SOCKET` and
  starts the greeter; on abnormal exit (`:198-210`) it retries with software rendering while
  `m_greeterCrashedCounter < 4`, then raises an in-process `EmergencyWindow`; the state stays
  `Locked` throughout. Commit `acce013f` 2015 [external]: "Directly unlocking in the error case is
  not an option as that could be used to attack the screen locker infrastructure." The greeter
  itself disables reconnection: `greeter/main.cpp:71-72` `qunsetenv("QT_WAYLAND_RECONNECT")`
  "Kwin will re-lock if it restarts, reconnecting would leave us with two greeters but only one
  functional." **Why an inherited fd at all [external, kscreenlocker `11963a9b`, 2015]:** "ksld
  didn't validate whether the windows tagged with this property belong to the screenlocker_greet
  process it started … It creates anonymous unix sockets for the connection and passes one
  filedescriptor to the started greeter process … only windows ids passed through the Wayland
  socket connection are accepted."
- **Other helpers: bounded restarts, then usable without them.** Input method: restart while `<
  5` crashes, fresh socketpair each time (`inputmethod.cpp:846-862, 916-928`); Xwayland:
  `XwaylandCrashPolicy::Restart` up to a count per 10 minutes (`xwaylandlauncher.cpp:318-343`).

*Transfer:* the spawned-helper shape is the exact fit for "one privileged peer" and is what Mura's
own greeter/OSK already copy — but it requires the *daemon that owns the role* to be the spawner,
which Monado is not for zxr. The arc is the strongest evidence in the corpus that a credential-
derived heuristic (exe path, `.desktop`) is not an identity: a mature project built one, kept it
four years, and removed it as "pseudo-security". The lock role's fail-closed-with-out-of-band-
escape and the helpers' bounded-restart-then-usable are two different answers to "usable without
it", chosen per role.

## 6. Hyprland — credentials logged, filesystem as the gate, refuse a second locker

- `ipc/s1/Unix.cpp:54-64` reads `SO_PEERCRED`, keeps only the pid, threads it into every request;
  the sole consumer is `plugin load` (`Commands.cpp:1783-1784`) for the opt-in permission dialogs.
  Nothing refuses another uid; `dispatch`/`keyword`/`reload` are unrestricted. The socket lives in
  a `mkdir 0700` per-instance directory under `XDG_RUNTIME_DIR` (`Compositor.cpp:196-231`); the
  instance signature disambiguates, it does not authenticate.
- Global filter mirrors KWin's: `Compositor.cpp:267-272` — if the client is not security-context
  sandboxed, `return true`; else hide everything not on a static whitelist of unprivileged globals
  (`managers/ProtocolManager.cpp:348-404`). Unsandboxed same-uid clients see all of it.
- The `DynamicPermissionManager` (off by default, `ConfigValues.cpp:715`) is a per-request consent
  dialog keyed by `/proc/pid/exe`, cached per `wl_client*` and wiped on client destroy; it fails
  open when the dialog helper is missing (`DynamicPermissionManager.cpp:279-283`). PR #9930
  [external] frames it as UX, not isolation.
- **Second lock client refused while locked** (`managers/SessionLockManager.cpp:52-59`
  `sendDenied()`) unless `misc:allow_session_lock_restore` ("will allow you to restart a lockscreen
  app in case it crashes", default false); the compositor stays locked with a dead locker
  ("lockdead" rendering) and does not respawn it.

*Transfer:* confirms KWin on every point; adds the "refuse the newcomer, optionally allow re-grant
by config" position for Q2.

## 7. Android (`aosp-system-core`) — the counter-example, cited as evidence only (AGENTS rule 2)

`init/property_service.cpp` and `debuggerd/tombstoned` gate by `SO_PEERCRED` uid/gid plus SELinux
label (`SO_PEERSEC`) — identity by *uid*, which works because Android gives every app its own
uid. That is the assumption Mura does not have (one desktop uid) and does not want (invariant
10: the wearer is the administrator, not a licensee of per-app sandboxes). Monado's own Android
target ignores even that (`getCallingUid` unused, §0). Nothing here transfers as policy; it
confirms that "uid" is the only credential any platform treats as an authorisation identity.

## 8. Comparison on the three questions

**Q1 — the identity primitive.** No comparable authenticates same-uid peers by `SO_PEERCRED`.
What they actually rely on, from least to most assumed:

- *Possession of a pre-connected fd* — libei fd backend (the documented "preferred" backend,
  §3), libseat builtin (§2), KWin's three helpers and kscreenlocker's greeter (§5), Monado's own
  Android admission (§0). Identity is "who handed you the fd"; nothing is authenticated at accept
  because there is no accept. Requires the daemon (or its portal/spawner) to be in the peer's
  ancestry or to have a channel to it.
- *Which listening socket the client used* — PipeWire's `-manager` socket (§1). Class stamped at
  accept, unforgeable by the client, enforced only by filesystem/namespace visibility. Upstream
  chose it explicitly over exe-path identity, and does not claim it separates same-uid peers.
- *The peer's systemd user unit via pidfd* — systemd's own primitive (§4), used by KWin only to
  classify sandboxes (§5). Stable, named, recycle-safe; assumes a user manager and cgroup layout;
  no per-user service in the corpus uses it as its authorisation gate.
- *Spawner-known pid* — KWin↔Xwayland (§3). Works only when the daemon spawned the peer; racy.
- *Exe path / `.desktop` metadata* — built and then removed by KWin ("pseudo-security"), removed
  by PipeWire ("a bit unusual approach"), kept only as an opt-in consent UX by Hyprland.

**Q2 — lease semantics.** The single-holder projects (seatd, KWin's EIS contexts, KWin/Hyprland
locks) converge: exactly one active holder; revocation tied to the connection's lifetime; a
restarted holder is a new client with no persisted token; no holder → the service idles (seatd)
or falls back to its built-in rule (KWin's helpers) — never auto-promotes an ordinary client. They
split on **what a second would-be holder gets**: refuse `EBUSY` (seatd VT-bound; Hyprland lock),
queue until the holder leaves (seatd non-VT), disconnect the newcomer (KWin EIS), refuse unless a
config flag allows re-grant (Hyprland). None displaces a live holder. PipeWire has no lease at all.

**Q3 — retrofitting a gate; class vs per-call.** Every stream-socket service decides the class
**once at accept/connect** and stores it as connection state; later calls check the state (seatd
`EPERM` "client is not active"; PipeWire's one dispatcher line against a static per-method table;
KWin/Hyprland's global filter at advertise time). The only per-message check is systemd's notify
socket, because a datagram socket has no connection. On retrofit: PipeWire added the new mechanism
beside the old and left the old open by default under `access.legacy` for three years; KWin,
faced with a gate that could not hold, removed it rather than tighten it. **No comparable put a
hard gate on an existing verb for same-uid peers.** Refused calls return an error (PipeWire
`-EACCES`, seatd `EPERM`), they do not disconnect.

## 9. Determinations (confident; the precedent named)

1. **Read credentials at accept and attach a class to the connection; check the class, not the
   credentials, on every later call.** Unanimous across PipeWire, seatd, libei's embedders, KWin,
   Hyprland; sd-bus states it as the rule. For Monado: `ipc_server_handle_client_connected` is the
   one place; `ipc_client_state` gets a `role` the client cannot write (PipeWire's `pipewire.sec.*`
   refusal is the model — `ipc_handle_instance_describe_client` must not be able to set it).
2. **The client-asserted pid becomes informational.** Every comparable that reads `SO_PEERCRED`
   overrides or ignores what the client says about itself; §5.3(a) as written.
3. **Refused verbs return an error, not a disconnect.** PipeWire `-EACCES`, seatd `EPERM`; Monado
   has `XRT_ERROR_IPC_FAILURE`-class results already. §5.3(d) "fail closed" is satisfied by an
   error return.
4. **The no-controller default is Monado's existing `update_server_state_locked`.** It is exactly
   seatd's "no clients to activate → idle" and KWin's helper fallback; §5.3(c) needs no new code.
5. **Revocation is the connection's lifetime; a restarted controller is a new client.** seatd,
   KWin (fresh socketpair every restart, "reusing the old one can cause issues"), kscreenlocker
   (`QT_WAYLAND_RECONNECT` unset). No lease token survives the process.
6. **Exe-path or `.desktop`-derived identity is out.** Built and removed by KWin, removed by
   PipeWire, opt-in consent UX only in Hyprland. It will not be proposed for C0.
7. **`SO_PEERCRED`'s pid alone is not an identity.** Racy per systemd (`pidref.c`, the man page's
   "not suitable for authorization"); if a pid is read at all it is read as a pidfd
   (`SO_PEERPIDFD`, fallback `SO_PEERCRED` only when unsupported — `getpeerpidref`'s shape).

## 10. Open items — decider: the owner (rule 8)

Each with the comparables' actual positions and the consequence of each. None is invented.

> **Ruled 2026-09-30** ([ADR 0006 amendment 5](../architecture/adr/0006-compositor-strategy.md);
> normative text in [composition §5.3 rev 1](../../specs/composition.md)). The owner's criteria:
> consistent with existing systems, efficient, secure as far as same-uid allows, not foreclosing
> sandboxed clients. **O1 → (A)**, the listening socket is the class, with the sandbox class
> (`wp_security_context_v1`'s listener shape plus PipeWire/KWin's Flatpak lowering) designed
> now; (B) and (C) not taken for the reasons §8 gives. **O2 → queue** (seatd non-VT-bound),
> promoted on the holder's disconnect. **O3 → (ii)**, the `access.legacy` shape:
> `IPC_REQUIRE_CONTROLLER`, upstream default open while no holder, Mura's unit closed. The
> boundary Monado/zxr: Monado enforces (class, lease, static list, fd admission), zxr holds
> policy; no permission model enters the runtime.

**O1 — the identity primitive for the controller class.** The comparables offer three that are
real; they split on who the spawner is, and Mura's layout (§0: zxr is a systemd user unit, Monado
is socket-activated, neither spawns the other) is the deciding fact.

- *(A) A second listening socket*, `monado_comp_ipc_controller`, class = the socket (PipeWire).
  Mechanism: a second `ListenStream=` in `monado.socket`; Monado takes `sd_listen_fds` > 1 (today
  it takes exactly fd 0, `ipc_server_mainloop_linux.c:61-69`) and stamps the role by listener;
  zxr connects to the controller path (`libmonado`/`ipc_client_connection` grow a path option).
  Consequence: smallest code, standard mechanism, unforgeable by protocol; **but** within one uid
  the controller socket is reachable by any process that can read `XDG_RUNTIME_DIR` — PipeWire
  accepts exactly this and draws the boundary at the sandbox. Honest as a *role* separator and a
  fail-closed guard against a misbehaving app; not a security boundary against a hostile same-uid
  process (which, per KWin, no unsandboxed-client scheme is).
- *(B) The peer's systemd user unit via pidfd*, class = `sd_pidfd_get_user_unit(peer) ==
  mura-compositor.service` (systemd's primitive; NotifyAccess as the worked example).
  Consequence: a stable, named, recycle-safe identity that matches §5.3(b)'s wording literally;
  the check is ~30 lines at accept plus a configured unit name (env or `--controller-unit`);
  Monado already links libsystemd. Costs: a hard dependency on a systemd user manager for the
  role (`-ENODATA` otherwise — `dev-session` and nested harnesses would need a way to declare the
  controller, e.g. (A) or an env override); **no per-user service in the corpus does this for
  authorisation** — it is systemd's own tool used in a place it has not been used; upstream Monado
  may see it as distro-specific.
- *(C) An inherited fd* (libei's "preferred", libseat builtin, KWin/kscreenlocker, Monado's
  Android path). Consequence: the strongest identity and Monado already has the admission entry
  point (`ipc_server_mainloop_add_fd`); **but** it needs a spawner relationship Mura does not
  have — either Monado spawns zxr (contradicts socket activation and D4's independent restart) or
  zxr spawns Monado (contradicts the runtime outliving the compositor and the greeter/session
  hand-over). Achievable only by re-plumbing who starts whom.
- The combinations the comparables themselves use: KWin does (C) for helpers *and* the systemd
  lookup for classification; systemd does (B) with `SO_PEERCRED` fallback. (A)+(B) — a controller
  socket whose arrivals must also be the configured unit when a user manager is present — is a
  composition no comparable ships and is therefore *not* offered as an option here; if the owner
  wants defence in depth it is a new design, flagged as such.

**O2 — what a second controller connection gets.** Positions: refuse with an error while a holder
is live (seatd VT-bound `EBUSY`; Hyprland `sendDenied`); queue and promote when the holder leaves
(seatd non-VT); disconnect the newcomer (KWin EIS); refuse unless a config flag permits re-grant
(Hyprland `allow_session_lock_restore`). Consequence for Mura's D4 restart: with *refuse* the old
zxr's connection must be gone before the new one asks — `Restart=` after crash satisfies this, a
live-swap does not; with *queue* the new zxr waits and takes over on the old one's exit with no
race; *displace the live holder* has no comparable and is not offered.

**O3 — gating the existing verbs.** `set_primary_client`, `set_focused_client`, `toggle_io_client`,
`set_client_io_blocks` are open today and used by `monado-ctl`, `libmonado`, zxr and
`dev-session`. Positions: (i) gate them behind the controller class now, `-EACCES`-style error
for others — §5.3(d) literally; consequence: `monado-ctl` and any script using `libmonado` to set
the primary must run as the controller or lose the verb, and the change is a behaviour change for
upstream users; (ii) PipeWire's retrofit — gate the new C3 verbs from day one, leave the existing
four open under a flag whose default flips later (`access.legacy`'s shape); consequence: no
upstream breakage, but the DisplayXR failure mode ("gates fail open without an orchestrator")
persists for exactly the verbs zxr relies on until the flag flips; (iii) KWin's position — do not
gate what cannot be enforced against same-uid peers; consequence: C0 becomes identity + lease for
*new* verbs only. The comparables lean (ii) for compatibility and give no precedent for (i) on an
existing verb.

## 11. What C0 becomes — sized against the fork

Independent of O1–O3 (the determinations, §9):

- `src/xrt/ipc/server/ipc_server_process.c` `ipc_server_handle_client_connected` (`:944`): read
  the peer (`SO_PEERPIDFD`, fallback `SO_PEERCRED`) and set `ics->client_state.role` before the
  thread starts; the Android path keeps `role = app`.
- `src/xrt/ipc/shared/ipc_protocol.h` `struct ipc_app_state` (`:395-401`): add `role`
  (`app|controller`), keep `pid` as informational; `ipc_handle_instance_describe_client`
  (`ipc_server_handler.c:339-343`) must not touch `role`.
- A lease table in `ipc_server` beside `global_state` (`ipc_server.h`): `controller_index` or
  `-1`; set at accept for a controller-class connection per O2; cleared in the client-destroy
  path; `update_server_state_locked` unchanged (§9.4).
- `ipc_server_handler.c:1563-1607`: the four system verbs consult the lease per O3 and return an
  error result when refused; the C3 verbs are written gated from the start.
- `src/xrt/targets/libmonado/monado.{h,c}`: whatever O1 needs on the client side (a controller
  socket path for (A); nothing for (B); an fd-adopting constructor for (C)); a `mnd_root_get_role`
  or error mapping so zxr can tell "refused" from "failed".
- `src/xrt/targets/service/monado.in.socket` for (A): a second `ListenStream=`; and
  `ipc_server_mainloop_linux.c:61-69` to take both fds (`sd_listen_fds_with_names` gives the
  socket unit's `FileDescriptorName=`, the standard way to tell them apart).
- Tests: a probe client on the ordinary path is refused the controller verbs (composition §7.7);
  a controller connects, disconnects, reconnects (D4's restart) and the lease follows per O2; no
  controller → `update_server_state_locked`'s fallback, unchanged (§7.7's second half).

*The fit check against Mura's tree (not a precedent):* zxr connects from `mura-compositor.service`
under the user manager in every mode (session and greeter), so (A) and (B) are both wirable
without changing who starts whom; (C) is not. `dev-session` and the nested harness start
`monado-service` as a plain child with no socket unit, so (A) needs the service to open the
controller socket itself when not socket-activated, and (B) needs an override for a process that
is under no user unit. These are consequences for the owner's O1, not arguments.

## 12. Sources

Pinned: `references/{pipewire,wireplumber,seatd,libei,systemd,kwin,kscreenlocker,hyprland,monado,
aosp-system-core}` at MANIFEST.json's commits. [external]: PipeWire MRs !1727, !1732, !1753,
!2776, !2809 and commits `a2bf4ce9`, `bb120a07`, `3d322917`, `f89757e1`, `d85862af`, `0e831c52`,
`08a45900`, `516f86c3`, `e0b09e7a`, `1962a8ca` (gitlab.freedesktop.org/pipewire); KWin commits
`97b5274a`, `4f9531ad`, `4016406e`, `f9bf0ee6`, `5bc7c913`, `f2964e5c`, `81f7436f`, `950c27bb`,
`17cda3e9`, `92f1fabe`, `ec75361d`, `3a137198`, `450bbaaf` (invent.kde.org/plasma/kwin);
kscreenlocker `11963a9b`, `0d4ebefb`, `decf7c9d`, `e72f7bf8`, `acce013f`, `1f6708b4`; Hyprland PR
#9930; libei issue 63; Monado MRs !1213, !1354, !1908, !2644, !2727 and the 2026-09-29 all-state
search (gitlab.freedesktop.org/monado/monado). Repo: `specs/composition.md`, `docs/architecture/
implementation-path.md`, `adr/0006-compositor-strategy.md`, `modules/xr/default.nix`,
`modules/os/session.nix`, research/67, research/79.
