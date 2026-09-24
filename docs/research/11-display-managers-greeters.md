# 11 — Display managers, greeters, and the seat/DRM handoff

**Date:** 2026-09-22. Research for the Mura session/login model (feeds ADR 0007). Written in the
main session from the local clones and targeted web verification (the Opus subagent hit a resource
limit; sources are the same). File paths are relative to repo roots under `references/`; pinned
commits in `references/MANIFEST.json`.

**The XR twist that governs everything here:** on a headset nothing legible renders before the XR
display path is up — panel/DRM bring-up, lens distortion, IPD, and at least rotational (IMU)
tracking. So the greeter/login surface needs Monado + an XR compositor *already running* before any
user session exists. This inverts the desktop assumption that a display manager can hand a bare
compositor to the GPU. Mura's compositor is a Wayland-native, client-renders /
compositor-composites XR shell that is itself an OpenXR client of Monado
([ADR 0006](../architecture/adr/0006-compositor-strategy.md),
[zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md)). The hypothesis this doc
evaluates: **the greeter is the zxr compositor run in a restricted `--greeter` mode as a dedicated
greeter user, driven by greetd.**

---

## 1. Four concepts, precisely separated

- **Display-manager daemon** — a privileged, long-lived service that owns the login VT/seat, runs
  the PAM authentication conversation, and launches the chosen session as the target user. Examples:
  greetd, GDM, SDDM (KDE's default), LightDM.
- **Greeter (UI)** — just a program the daemon runs to collect credentials and a session choice. It
  is *not* privileged with auth itself; it relays a PAM conversation to/from the daemon. Examples:
  agreety, gtkgreet, tuigreet, ReGreet, SDDM's QML themes, GDM's greeter (a restricted GNOME Shell).
- **Session** — the program the daemon `exec`s as the user after auth (a compositor, a shell). On
  Mura this is the zxr compositor + shell.
- **Lock screen** — runs *inside* an already-authenticated session; the session compositor hides
  content and shows a locker that re-authenticates the *same* user. Distinct from the greeter (which
  authenticates *any* user, pre-session). Covered in [doc 12](12-lock-screens-and-appliance-login.md).

The daemon/greeter split is the important one: it lets the greeter be *any program*, including a
whole compositor. That is precisely the seam Mura exploits.

## 2. The greetd model (the one we transpose)

greetd is a tiny daemon that does exactly one thing well: broker a PAM conversation over a socket so
that an arbitrary program can be the greeter. It is the cleanest model and the one to adopt.

### 2.1 The IPC protocol — complete

Transport: a `$GREETD_SOCK` UNIX socket; framing is `len: u32` (native byte order) + a JSON payload
(`greetd/man/greetd-ipc-7.scd:16-34`, `greetd/greetd_ipc/src/lib.rs:15-27`). The full message set
(`greetd/greetd_ipc/src/lib.rs`):

**Requests (greeter → greetd):**

| Request | Fields | Meaning |
|---|---|---|
| `create_session` | `username: string` | begin a login attempt; → `auth_message` \| `success` \| `error` (`lib.rs:56-67`) |
| `post_auth_message_response` | `response: string?` | answer the last auth message (omit for info/error messages); → `auth_message` \| `success` \| `error` (`lib.rs:68-75`) |
| `start_session` | `cmd: [string]`, `env: [string]` | start the authenticated session with this command + extra env; **starts only after the greeter process exits** (`lib.rs:76-83`, `greetd-ipc-7.scd:49-51`) |
| `cancel_session` | — | abort the in-configuration session (`lib.rs:84-88`) |

**Responses (greetd → greeter):**

| Response | Fields | Meaning |
|---|---|---|
| `success` | — | request succeeded (`lib.rs:135-136`) |
| `error` | `error_type: auth_error \| error`, `description: string` | `auth_error` = bad creds (non-fatal, retry); `error` = general (`lib.rs:140-143`, `92-100`) |
| `auth_message` | `auth_message_type: visible \| secret \| info \| error`, `auth_message: string` | a PAM prompt to render and answer (`lib.rs:154-157`, `103-117`) |

The protocol makes **no assumption about the questions** — a greeter must render whatever prompts
PAM emits and must not try to auto-answer (`lib.rs:148-153`). This is the property that lets us
build a non-textual XR greeter: `visible`/`secret` prompts become an XR text field or a controller-
ray PIN pad, `info`/`error` become panels; the greeter just returns strings.

### 2.2 The session-launch and PAM mechanism — concrete

greetd forks a **session worker** that owns the entire PAM lifecycle
(`greetd/greetd/src/session/worker.rs`). The sequence, from the code:

1. Parent sends `InitiateLogin { service, class: Greeter|User, user, authenticate, tty, ... }`
   (`worker.rs:54-63`). **Greeters run with `class = Greeter` and `authenticate = false`** — they
   do not authenticate; the *user* session does (`worker.rs:38-51, 132-135`). This is the crux of
   "greeter as an unprivileged Ut process."
2. `PamSession::start(service, user, conv)`; if authenticating, `pam.authenticate()` →
   `pam.acct_mgmt()` (with `NewAuthTokenRequired` → change token) → `pam.setcred(ESTABLISH_CRED)`
   (`worker.rs:130-146`). The PAM conversation callback (`SessionConv`) is what turns PAM prompts
   into the `auth_message` IPC responses and back.
3. For a greeter session, greetd injects `GREETD_SOCK=<listener_path>` into the greeter's env via
   PAM (`worker.rs:150-154`) — that is how the greeter finds the socket.
4. Parent sends `Args { env, cmd }`; then `Start` (`worker.rs:156-170`).
5. `setsid()` → becomes session leader; TTY handling (§2.3); PAM env prep — `XDG_SEAT=seat0`,
   `XDG_SESSION_CLASS=greeter|user`, `USER`/`LOGNAME`/`HOME`/`SHELL`/`TERM` — then
   `pam.open_session()` (which triggers `pam_systemd.so` → a **logind session**) (`worker.rs:176-232`).
6. Fork a child; the child `initgroups`/`setgid`/`setuid` to the target user, sets
   `PDEATHSIG=SIGTERM`, `chdir($HOME)`, and `execve("/bin/sh", ["-c", "exec <cmd>"], pam_env)`
   (`worker.rs:255-292`). PAM must stay in the parent (closing the session needs root), so the
   command runs in a grandchild.

**Config** (`greetd/config.toml`): `[terminal] vt = 1`; `[default_session] command`, `user`
(default `"greeter"`). Crucially, greetd also supports `initial_session` (autologin — §5).

### 2.3 VT handling — and what a VT-less headset does instead

The worker's `TerminalMode` is either `Terminal { path, vt, switch }` or **`Stdin`**
(`worker.rs:28-35, 178-207`). The `Terminal` path opens the VT, sets `KD_TEXT`, clears it, and
optionally `VT_SETACTIVATE`s to switch to it, then takes it as controlling TTY. This is the desktop
mechanism. A headset has no meaningful VTs; the escape hatch already exists — greetd's `Stdin` mode
skips all VT manipulation, and logind still provides the seat/session. The DRM-master handoff
(§4) is done by logind per-session, not by the VT switch itself; the VT switch is only how desktop
logind decides which session is *active*. On a single-seat headset we run one seat (`seat0`) with a
fixed VT (VT1, as the NixOS module already pins) or stdin mode, and the greeter→session transition
is a session handoff on that one seat, not a VT dance.

### 2.4 gtkgreet + cage — the concrete "compositor as greeter" precedent today

greetd greeters that are graphical run *inside a compositor*. The canonical stack is **gtkgreet
inside cage** (or inside sway with layer-shell). `cage` is a **kiosk compositor**: it
`spawn_primary_client`s exactly one client from argv and exits when that client exits
(`cage/cage.c:157-206, 689, 724`; README: "A kiosk runs a single, maximized application"). So the
greeter today is literally: greetd runs `cage gtkgreet` as the `greeter` user; cage brings up
wlroots + one Wayland client (gtkgreet); gtkgreet speaks `$GREETD_SOCK`; when the user picks a
session, gtkgreet calls `start_session` and exits, cage exits, greetd starts the real session.

**This is exactly the shape of the XR greeter**, with cage's role played by the zxr compositor in a
restricted mode: instead of spawning an arbitrary single client, it composes one *built-in* auth
scene (login panel + session list) and speaks `$GREETD_SOCK`. The difference from cage is that the
XR greeter must also bring up Monado + the XR display path (distortion/IPD/IMU) — which is why the
greeter cannot be a generic 2D kiosk and must be our compositor.

## 3. GDM and SDDM: the "greeter is the shell" and "themed greeter" patterns

**GDM** is the closest precedent for our hypothesis: the greeter is GNOME Shell run in a restricted
"greeter" mode, as a dedicated unprivileged `gdm` user, in its own logind session; on login it hands
off to the user's own GNOME Shell session. The value we take: (a) maximum code reuse — greeter and
session are the *same compositor binary* in different modes; (b) the greeter runs as a locked-down
dedicated user, so pre-auth code has minimal privilege. (GDM's exact internal handoff — its
`pam_gdm`, `pam-extensions/`, and the launcher in `daemon/` — is the reference implementation; our
transposition uses greetd for the daemon role rather than GDM's bespoke daemon, so we need the
*pattern*, not GDM's code.)

**SDDM** (KDE Plasma's default DM) uses a QML-themed greeter (`sddm-greeter`) that the SDDM daemon
launches; themes are QML, which is attractive for a rich UI but is a 2D desktop model. Its
historical Wayland-greeter story has been awkward (the greeter needing its own compositor;
`sddm-greeter --qt-wayland` running inside weston/kwin), which is a cautionary tale: a DM that
assumes it can spawn a bare greeter compositor does not fit the headset, where the greeter compositor
*is* the XR runtime consumer. We take nothing structural from SDDM beyond confirming that "themed
greeter as a separate program" is the wrong coupling for us.

## 4. The seat/session/DRM-master handoff

Two independent brokers exist: **systemd-logind** (the common one; greetd's `pam_systemd`
integration uses it) and **seatd**/`libseat` (a minimal standalone alternative, `seatd/`). Both
solve the same problem: a compositor is not root, so it cannot `open()` DRM/input devices directly;
the broker opens them and passes fds, and arbitrates which session is *active* (holds DRM master) on
a seat.

- Under logind: each session (greeter, then user) is a logind session created at `pam_open_session`
  (greetd's `worker.rs:232`). The active session on a seat holds DRM master; on switch, logind sends
  the outgoing session `PauseDevice` (drop master) and the incoming `ResumeDevice` (gain master),
  via the `TakeDevice`/`ReleaseDevice`/`PauseDevice`/`ResumeDevice` D-Bus API. On desktop this is
  triggered by VT switching; with greetd's sequential model the greeter session simply *ends* (its
  compositor exits) before the user session's compositor starts, so it is a clean release→acquire,
  not a live switch.
- Under seatd/libseat: same contract, smaller surface (`seatd/` provides `libseat` with a logind
  backend *or* a standalone seatd daemon). Useful if we want to avoid a hard logind dependency on
  the appliance image.

**For a VT-less headset** the key realization: because greetd starts the session only *after the
greeter exits* (`start_session` semantics, `lib.rs:76-83`), there is never a live two-compositor DRM
contention to resolve — greeter compositor exits (releasing DRM master + closing its Monado), then
the session compositor starts and acquires. The handoff is temporal, not a VT flip.

## 5. Autologin: `initial_session` is the appliance mechanism

greetd's config distinguishes (NixOS wiki; `greetd` config):

- **`default_session`** — an authenticated greeter (asks for credentials).
- **`initial_session`** — a command + user run **automatically at boot with no password**, once.
  This is the appliance/kiosk autologin, and it is exactly what SteamOS/Jovian's
  `gamescope-session` uses to boot straight into the session.

So the two Mura profiles map directly onto greetd config:

- **Appliance:** `initial_session = { user = owner; command = <zxr session>; }` — boot straight into
  the owner's XR session; security is the in-session compositor lock (doc 12), not a greeter.
- **Multi-user:** `default_session = { user = greeter; command = <zxr compositor --greeter>; }` — the
  XR greeter authenticates and picks a user/session.

## 6. NixOS surface

`services.greetd` exists upstream (`nixos/modules/services/display-managers/greetd.nix`): it aliases
`greetd.service` to `display-manager.service`, defaults the greeter user to `greeter`, sets
`security.pam.services.greetd` (`startSession = true`, `allowNullPassword`), pins the greeter to VT1
with `Conflicts=getty@tty1.service`, `Type=idle`, `wantedBy=graphical.target`, and disables
`autovt@tty1`. It also wires `services.displayManager.sessionData.desktops` (generated
`wayland-sessions`/`xsessions` `.desktop` files). Consequences for Mura:

- We **reuse `services.greetd`** as the daemon; we do **not** use `services.displayManager.sddm/gdm`
  (wrong compositor coupling, §3).
- greetd only accepts **env-less commands**, so the session command is a wrapper. Our session
  chooser should map `mura.xr.shell` values (`zxr`/`stardust`/`wayvr`) to session commands from
  the *module system*, not from hand-written `.desktop` files — the greeter lists declared sessions.
- The `greeter` user needs `seat`/`video`/`input` group access for the XR display path, and access
  to the per-unit calibration (system state, `/var/lib` or vendor persist — never `$HOME`), because
  the greeter renders through the full distortion/IPD path before any user logs in.

## 7. The Monado two-instance question — resolved

Concern: greeter-user Monado, then session-user Monado, run sequentially — do they conflict?

Finding from the clone: Monado's IPC socket is created **in `$XDG_RUNTIME_DIR`** — the systemd unit
is `ListenStream=%t/monado_comp_ipc` (`monado/src/xrt/targets/service/monado.in.socket:10`, `%t` =
per-user runtime dir), and both server and client resolve it via
`u_file_get_path_in_runtime_dir(XRT_IPC_MSG_SOCK_FILENAME, ...)`
(`monado/src/xrt/ipc/server/ipc_server_mainloop_linux.c:94`,
`monado/src/xrt/ipc/client/ipc_client_connection.c:237`); there is also a per-runtime-dir PID lock
(`monado/src/xrt/auxiliary/util/u_process.c:30`). Because the greeter and the user are **different
users with different `$XDG_RUNTIME_DIR`**, the socket paths and PID locks **do not collide** — the
socket is not the contention point.

The real shared resources are **DRM master** and **hidraw/udev device access** (the HMD, IMU,
cameras). These are brokered by logind/seatd per session (§4), and greetd's exit-then-start
sequencing guarantees the greeter's Monado has fully torn down (releasing DRM master and device fds)
before the session's Monado starts. **Verdict: sequential two-instance handoff is sound**, provided
the teardown ordering is: greeter compositor stops → its `monado-service` stops (closing DRM/hidraw)
→ greeter logind session ends → session logind session starts → session `monado-service` starts →
session compositor starts. This is the ordinary greetd lifecycle; no Monado change is required. The
one caveat to verify on hardware: some HMD/IMU devices are slow to release/re-acquire (USB
re-enumeration), so the handoff may need a brief settle, and firmware/calibration state must be
system-level so the second instance re-reads it (it must not live in the greeter user's `$HOME`).

## 8. What Mura should adopt

1. **greetd as the daemon**, both profiles: `initial_session` for appliance autologin, `default_session`
   for the multi-user XR greeter. Reuse `services.greetd`.
2. **Greeter = the zxr compositor in `--greeter` mode**, run as the `greeter` user (GDM pattern via
   cage's single-client precedent): brings up Monado + the XR display path, composes one built-in
   auth scene, speaks `$GREETD_SOCK`, exits on `start_session`. Same binary as the shell, restricted
   mode.
3. **The greetd IPC contract as-is** — render `visible`/`secret`/`info`/`error` prompts as XR panels /
   PIN pad; never auto-answer; relay strings. This makes the auth UI PAM-agnostic (password, TOTP,
   PIN, fingerprint all just work).
4. **Sequential handoff, per-user Monado**, relying on greetd's exit-then-start and logind/seatd DRM
   brokering (§4, §7). No shared system-wide Monado.
5. **Sessions from the module system** (`mura.xr.shell` values) rather than `.desktop` files.
6. **Calibration + firmware as system state** so both Monado instances read it pre- and post-login.

## 9. What Mura should reject

- **A system-wide shared Monado across users** — fights Monado's per-user `$XDG_RUNTIME_DIR` service
  model and creates a cross-user socket/security problem; unnecessary given the sequential handoff.
- **SDDM/GDM as the daemon** — wrong coupling (they assume they spawn the greeter compositor / a 2D
  themed greeter); we want greetd's minimal broker and our own compositor as greeter.
- **VT-switching assumptions** — design for one seat, fixed VT or stdin mode; the handoff is
  temporal, not a VT flip.
- **`.desktop`-file session discovery** as the source of truth — our sessions are typed module
  values.

## 10. Open questions

1. **HMD device release latency** across the greeter→session handoff (USB re-enumeration of the HMD/
   IMU) — does it need an explicit settle/keep-alive, or can the session Monado inherit warm devices?
   Needs a hardware spike.
2. **Does the greeter need cameras at all?** Per the tiered-tracking policy (IMU-only pre-auth,
   [ADR 0007 to come]), the greeter should run rotation-only; confirm Monado can start with a
   camera-less driver profile and the session upgrades to 6DoF.
3. **greetd `Stdin` vs fixed-VT on the appliance** — which gives the cleanest boot with Plymouth /
   the pre-Monado splash (doc 12 §7)?
4. **Multi-user calibration** — per-unit lens/distortion is system state, but per-*user* IPD is a
   preference; how does the greeter pick a safe default IPD before it knows the user, and hand the
   user's IPD to the session? (Likely: greeter uses a safe mechanical default; session applies user
   IPD post-login.)
5. **logind vs seatd on the appliance image** — is a full logind dependency warranted, or does
   seatd/libseat suffice and shrink the closure?

---

## 11. Addendum (2026-09-24) — pre-authentication greeter furniture: inventory and mechanisms

Read-only code study for the XR greeter's "standard set" — what an ordinary Linux greeter exposes
*before* anyone is authenticated, and the mechanism and permission behind each item. Feeds
[multi-user.md §2](../architecture/multi-user.md) and the Phase 3 review with
[doc 42](42-input-bootstrap.md). Enumeration, picker, last-user, switch-user and guest are in
[doc 41 §1](41-multi-user-login-landscape.md) and are not repeated. Paths are
`<repo>/<path>:<line>` under `references/` (MANIFEST 2026-09-24). Note on plasma-workspace: the
pinned master has **no `sddm-theme/` and no lock-screen QML under `lookandfeel/`**; what remains
shared is `components/loginlockscreen/Footer.qml` and
`lookandfeel/components/{SessionManagementScreen,UserList,Clock,Battery,ActionButton}.qml`
(`plasma-workspace/lookandfeel/CMakeLists.txt:5-15`). "SDDM" rows cite SDDM's stock themes plus
those shared Plasma components; the Breeze login QML now lives in KDE's `plasma-login-manager`
[external, not pinned].

### 11.A Power menu

| Greeter | Mechanism | Show/hide decision |
|---|---|---|
| GDM | gnome-shell → `org.gnome.SessionManager` `Shutdown`/`Reboot` (`gnome-shell/js/misc/systemActions.js:495,502`), gnome-session → login1 [external]; suspend via login1 `Suspend` (`js/misc/loginManager.js`) | `CanShutdown/CanReboot/CanSuspend` ≠ UNAVAILABLE (`systemActions.js:359-365,381-387,403-409`) AND not `org.gnome.login-screen disable-restart-buttons` when `isGreeter` (`systemActions.js:371-378,393-400,415-419`; key `gdm/data/org.gnome.login-screen.gschema.xml:105`). Log out / switch user / lock hidden in greeter mode (`systemActions.js:353,431,446`) |
| SDDM | Greeter never touches login1: theme calls `sddm.powerOff()` → `GreeterMessages::PowerOff` over the daemon socket (`sddm/src/greeter/GreeterProxy.cpp:91-92`) → **root daemon** `PowerManager::powerOff()` → login1 `PowerOff(true)` (`sddm/src/daemon/SocketServer.cpp:147-183`, `src/daemon/PowerManager.cpp:172-190`); ConsoleKit2 fallback (`PowerManager.cpp:124-126,203-207`); UPower backend runs `HaltCommand`/`RebootCommand` (`PowerManager.cpp:89-98`, `src/common/Configuration.h:43-44`) | Daemon sends `Capabilities` on connect (`SocketServer.cpp:125`) from login1 `Can*` == "yes" (`PowerManager.cpp:144-161`); themes bind `sddm.canSuspend`/`canHibernate` (`sddm/data/themes/elarun/Main.qml:192,201`); power-off/reboot always shown in stock themes (`maldives/Main.qml:258,268`) |
| LightDM | Greeter process calls login1 `PowerOff/Reboot/Suspend/Hibernate` with `interactive=FALSE`, CK then UPower fallback (`lightdm/liblightdm-gobject/power.c:58-74,162-173,292,343`) | `lightdm_get_can_*` = login1 `Can*` reply `== "yes"` only (`power.c:117-128,257-259,308-310`) — a `"challenge"` answer hides the button |
| gtkgreet | **None.** Clock, question box, command selector only (`gtkgreet/gtkgreet/window.c:54-61,126-142`) | — |
| regreet | Spawns `[commands] reboot`/`poweroff` (`regreet/src/gui/model.rs:191-231`), defaults `reboot`/`poweroff` (`src/constants.rs:43-45`; sample `systemctl …`, `regreet.sample.toml:41-46`) | Always shown (`src/gui/templates.rs:213-218`) |
| tuigreet | Shutdown/reboot default `shutdown -h/-r now`, suspend/hibernate default `loginctl suspend/hibernate`, in `setsid` unless `--power-no-setsid`; overridable `--power-*` (`tuigreet/crates/tuigreet/src/power.rs:41-82`, `src/greeter.rs:767-791,1072-1096`) | Always present |

**Polkit from a greeter session.** login1 defaults (`systemd/src/login/org.freedesktop.login1.policy`):
`power-off`, `reboot`, `suspend`, `hibernate` and their `-multiple-sessions` variants are
`allow_any=auth_admin_keep / allow_inactive=auth_admin_keep / allow_active=yes`
(`:168-186,201-219,267-284,299-316`); the `-ignore-inhibit` variants are `auth_admin_keep` for
all three (`:190-197,223-230,288-295,320-327`). logind picks the `-multiple-sessions` action iff
another *user-class* session exists — greeter sessions never count
(`SESSION_CLASS_IS_INHIBITOR_LIKE` excludes `SESSION_GREETER`; `systemd/src/login/logind-shutdown.c:35-40,106-110`,
`logind-session.h:69`). A root caller (SDDM's daemon) short-circuits polkit
(`systemd/src/shared/bus-polkit.c:20-47`).

**Is a greeter session "active"?** `session_is_active` is `seat->active == s`
(`systemd/src/login/logind-session.c:1114-1121`). On a seat with VTs the active session owns the
foreground VT (`logind-seat.c:535-564`); on a **VT-less seat every newly attached session is
auto-activated** (`logind-seat.c:728-733`). A greeter is a real seat session
(`SESSION_CLASS_CAN_TAKE_DEVICE` includes `SESSION_GREETER`, `logind-session.h:63`), so the
*displayed* greeter is active and `allow_active=yes` applies: greetd/LightDM greeters power off
with no authentication and no root helper. [external] polkit reads `Active` from logind for the
subject's session (polkit `polkitbackendsessionmonitor-systemd.c`).

### 11.B Session chooser

| Greeter | List source | Remembered where | Hidden when single |
|---|---|---|---|
| GDM | `libgdm gdm_get_session_ids()` over `$XDG_DATA_DIRS/{xsessions,wayland-sessions}` (`gdm/libgdm/gdm-sessions.c:272-320,354`) plus `gdm/greeter/wayland-sessions` (`gdm/daemon/gdm-session.c:383-409`) | AccountsService per-user `Session`/`SessionType` (`gdm/daemon/gdm-session-settings.c:302-303,391-395`); greeter → daemon `SelectSession` (`gnome-shell/js/gdm/loginDialog.js:552`) | **Yes**: `if (ids.length === 1) return` (`loginDialog.js:566-568`); also hidden if the user is already logged in (`:563-565`) |
| SDDM | `SessionModel` from `[Wayland] SessionDir` then `[X11] SessionDir`, with dir watcher (`sddm/src/greeter/SessionModel.cpp:50-68`; defaults `Configuration.h:71,82`) | Daemon state `Last.Session` (global, not per-user) when `RememberLastSession` (`sddm/src/daemon/Display.cpp:506-507`); `lastIndex` preselects (`SessionModel.cpp:169-170`) | Theme decision; stock themes always show the combo (`maldives/Main.qml:191-192`) |
| LightDM | `lightdm_get_sessions()` from `sessions-directory` / `remote-sessions-directory` (`lightdm/liblightdm-gobject/session.c:194-201,222`) | Per-user `~/.dmrc [Desktop] Session` **and** AccountsService `SetXSession` (`lightdm/common/user-list.c:1197,1422-1428`); hint `default-session` = seat `user-session` (`lightdm/src/greeter.c:577`) | Greeter-specific |
| gtkgreet | `/etc/greetd/environments` or `-c` (`gtkgreet/man/gtkgreet-1.scd:16-17,95-96`) | None | Combo (with free-text entry) only when no `-c` (`window.c:129-136`, `main.c:28`) |
| regreet | `/usr/share/xsessions:/usr/share/wayland-sessions` or `$XDG_DATA_DIRS` (`regreet/src/constants.rs:51-53`, `src/sysutil.rs:110-124`) | `/var/lib/regreet/state.toml` per-user last session (`src/gui/model.rs:427-430,596`); `skip_selection` (`regreet.sample.toml:6`) | Always shown (`src/gui/component.rs:80`) |
| tuigreet | `$XDG_DATA_DIRS/{wayland-sessions,xsessions}` (`tuigreet/crates/tuigreet/src/info.rs:50-57`), `--sessions`/`--xsessions` (`greeter.rs:651-663`) | `--remember-session` / `--remember-user-session` (`greeter.rs:707-712,365-395`) | Always a menu |

### 11.C Accessibility menu

| Greeter | Toggles | Mechanism |
|---|---|---|
| GDM | High contrast, magnifier, large text, screen reader, on-screen keyboard, visual bell, sticky/slow/bounce/mouse keys (`gnome-shell/js/gdm/loginDialog.js:329-350`) | gsettings on the **gdm user's** dconf: `org.gnome.desktop.a11y.interface high-contrast`, `.a11y.applications screen-magnifier/screen-reader/screen-keyboard-enabled`, `.interface text-scaling-factor`, `.wm.preferences visual-bell`, `.a11y.keyboard *keys-enable` (`js/ui/status/accessibility.js:10-30,150-322`). GDM's dconf profile forces `always-show-universal-access-status=true` (`gdm/data/dconf/defaults/00-upstream-settings:12-13`). Panel also carries `dwellClick` + `keyboard` (input source) (`js/ui/sessionMode.js:62`) |
| SDDM | Stock themes: keyboard-layout indicator only (`elarun/Main.qml:287`). Plasma shared footer adds **OSK toggle + layout switcher** (`plasma-workspace/components/loginlockscreen/Footer.qml:45-100`) | OSK = `org.kde.KWin /VirtualKeyboard` D-Bus `forceActivate`/`active` (`plasma-workspace/components/keyboardlayout/virtualkeyboard.cpp:13-28`); Plasma's greeter compositor is `kwin_wayland --inputmethod plasma-keyboard` (`plasma-workspace/sddm-wayland-session/plasma-wayland.conf:7`); SDDM sets `QT_IM_MODULE` from `InputMethod` (default `qtvirtualkeyboard`, disabled on Wayland) (`sddm/src/greeter/GreeterApp.cpp:354-361`, `Configuration.h:48`) |
| LightDM | Core: none. [external] lightdm-gtk-greeter has an a11y indicator (OSK via `onboard`, high-contrast, font scaling) — https://github.com/Xubuntu/lightdm-gtk-greeter | greeter-local |
| gtkgreet / regreet / tuigreet | None (tuigreet has `--kb-*` key bindings only, `greeter.rs:1098`) | — |

### 11.D Network menu

| Greeter | Present pre-auth? | Can it *connect*? |
|---|---|---|
| GDM | Yes: `gdm` mode loads `networkAgent` + quickSettings (`sessionMode.js:56-62`) | **Yes.** Wi-Fi connect uses `add_and_activate_connection_async` (`js/ui/status/network.js:963`); toggles reactive iff `org.freedesktop.NetworkManager.network-control` allowed (`network.js:2171-2177`). If `settings.modify.system` is not allowed, the connection is scoped `permissions=user:gdm` (`network.js:954-961`), which NM authorises as `settings.modify.own` (`networkmanager/src/core/settings/nm-settings.c:2645-2654`). GDM ships a rule granting `settings.modify.system` to the gdm group when `subject.local && subject.active` (`gdm/data/polkit-gdm.rules.in:1-8`), so the connection becomes system-wide and survives into the user session. Captive-portal handling skipped in greeter mode (`network.js:2232`) |
| SDDM / LightDM / gtkgreet / regreet / tuigreet | No network UI in core/stock themes | — |

NM defaults (`networkmanager/data/org.freedesktop.NetworkManager.policy.in`): `network-control`
and `wifi.scan` `any=auth_admin / inactive=yes / active=yes` (`:67-83`); `settings.modify.own`
`any=auth_self_keep / inactive=yes / active=yes` (`:105-111`); `settings.modify.system`
`auth_admin_keep` ×3 (`:115-121`); `enable-disable-wifi`, `wifi.share.protected/open`
`inactive=no / active=yes` (`:40-45,87-101`). Any active greeter session may activate existing
connections and add *own* ones without auth; system-wide ones need a distro rule like GDM's —
**a discretionary policy choice for Mura** (AGENTS.md rule 4), recorded in doc 42 §7.3.

### 11.E Clock / banner / hostname

| Greeter | One line |
|---|---|
| GDM | Panel `dateMenu` clock (`sessionMode.js:61`); banner from `banner-message-enable/-source/-text/-path` with file monitor (`gdm/data/org.gnome.login-screen.gschema.xml:69-96`, `loginDialog.js:879-932`); no hostname |
| SDDM | Themes: `Clock` + `sddm.hostName` from daemon `HostName` (`maldives/Main.qml:75,102`, `GreeterProxy.cpp:59-60,187`); Plasma `Clock.qml` (`lookandfeel/components/Clock.qml:21,34`); `Battery.qml` (`:28-43`) |
| LightDM | Greeter-specific; core passes no banner |
| gtkgreet | Clock label, `strftime` (`window.c:54-61,224-228`); no banner |
| regreet | `[widget.clock]` format/timezone/locale + `greeting_msg` (`regreet.sample.toml:51-76`, `src/config.rs:19-26`) |
| tuigreet | `--time`/`--time-format`, `--greeting`, `--issue` (`greeter.rs:684-704,1068-1070`) |

### 11.F Zero accounts and empty password

| Greeter | Empty user list | Empty password |
|---|---|---|
| GDM | Daemon: initial-setup (doc 41 §1.2). Shell: `numItems()===0` forces `disableUserList` (`loginDialog.js:847-852`) → "Username" entry (`:1017-1021,1108-1111`); cancel hidden (`:865-876`) | Worker passes `PAM_DISALLOW_NULL_AUTHTOK` **only for non-local displays** (`gdm/daemon/gdm-session-worker.c:1363-1364,1387-1392`); GDM's PAM files carry **no `nullok`** (`pam-arch/gdm-password.pam:3`, `pam-redhat/gdm-password.pam:2`) — the distro decides. PAM success without a prompt → `verification-complete` → session (`js/gdm/authPrompt.js:658-672`, `loginDialog.js:1274`) |
| SDDM | Stock themes are text-field (`maldives/Main.qml:120-130`); `UserModel.rowCount()==0` (`UserModel.cpp:183`) | `pam_authenticate(flags=0)` (`sddm/src/helper/backend/PamHandle.h:111`, `PamHandle.cpp:87`); **SDDM ships no PAM files** |
| LightDM | `greeter-show-manual-login` → hint `show-manual-login` (`lightdm/data/lightdm.conf:105`, `src/greeter.c:579`, `liblightdm-gobject/greeter.c:1139`); `hide-users` (`greeter.c:578`) | `pam_authenticate(pam_handle, 0)` (`lightdm/src/session-child.c:337`); shipped `data/pam/lightdm` lacks `nullok`; `lightdm-greeter`/`lightdm-autologin` use `pam_permit` (`data/pam/lightdm-greeter:8`, `lightdm-autologin:12`) |
| gtkgreet | Always a username question | Relays greetd; greetd `pam_authenticate` with caller flags (`greetd/greetd/src/pam/session.rs:45-49`), ships no PAM files, requires `/etc/pam.d/greetd` (`greetd/src/server.rs:172-174,206-209`). NixOS sets `allowNullPassword = true` for greetd (`nixos/modules/services/display-managers/greetd.nix:78-82`) |
| regreet / tuigreet | regreet lists AccountsService users with a manual entry; tuigreet text-first with optional `--user-menu` (`greeter.rs:716-727`) | PAM-driven via greetd, as above |

### 11.G Lock screen vs greeter

GNOME: `unlock-dialog` mode drops `dateMenu` but *adds* the `a11y` indicator to the panel
(greeter mode puts a11y in the dialog's button group), keeps `dwellClick`/`keyboard`/
`quickSettings`, and loads the same `networkAgent`/`polkitAgent` (`gnome-shell/js/ui/sessionMode.js:51-78`).
The unlock dialog adds a notification stack (`js/ui/unlockDialog.js:43-101`) and "Switch User…"
iff `userManager.can_switch()` and not `disable-user-switching` (`unlockDialog.js:668-679,1062-1065`);
no session chooser, no user list. Power-off/reboot are suppressed on the lock screen when the
action needs auth or `org.gnome.desktop.screensaver` restart is disabled (`systemActions.js:371-377`).
Plasma: kscreenlocker's fallback theme offers password + "Switch Users" only
(`kscreenlocker/greeter/fallbacktheme/Greeter.qml:115-127`, `LockScreen.qml:74-84`), where
`canSwitchUser` = `KAuthorized start_new_session` ∧ backend (`plasma-workspace/libkworkspace/sessionmanagement.cpp:123-125`,
`sessionmanagementbackend.cpp:240-243`); the shared `Footer.qml` (OSK toggle, layout switcher,
battery) serves both login and lock (`Footer.qml:16-19`). GDM's dconf profile hard-locks
`disable-user-switching=true` and `disable-lock-screen=true` for the greeter user
(`gdm/data/dconf/defaults/00-upstream-settings:23-30`).

### 11.H The standard set

Every mainstream graphical greeter exposes pre-auth: (1) **a power menu** with at least power-off
and reboot (all but gtkgreet), suspend/hibernate gated on a login1 `Can*` probe; (2) **a session
chooser** from `wayland-sessions`/`xsessions` `.desktop` files (gtkgreet reads
`/etc/greetd/environments`), GDM alone hiding it for a single session; (3) **a clock**; (4) **a
fallback username entry** when no listable users exist. The mechanism behind (1) is uniform: the
displayed greeter session is logind-*active*, so `org.freedesktop.login1.power-off/reboot/suspend/hibernate`
`allow_active=yes` grants it without a privileged helper (SDDM's root-daemon proxy is an
exception, not a requirement). Optional tier: **a11y toggles** (GDM full set via gsettings; Plasma
OSK via KWin D-Bus; nothing in the greetd family), **banner/greeting** (GDM, regreet, tuigreet),
**hostname** (SDDM), **battery** (Plasma), **keyboard-layout switcher** (GDM, Plasma), and — GDM
only — **a network menu that can join Wi-Fi**, which works because `network-control` and
`settings.modify.own` are `allow_active=yes` and GDM ships a rule lifting the greeter to
`settings.modify.system`. Empty-password login is uniformly deferred to the distro's PAM `nullok`;
no greeter ships `nullok` itself, and GDM alone forbids null tokens (remote displays only).

### 11.I Gaps

- **VT-less seat activeness**: `logind-seat.c:728-733` auto-activates every new session on a
  VT-less seat. If the greeter and a lingering user session coexist for any window, the *newest*
  holds `Active`; the greeter's power-menu polkit depends on it — confirm on hardware with greetd's
  exit-then-start ordering (§4, session-auth §5).
- **Pre-auth Wi-Fi scope**: parity with GDM needs a Mura polkit rule for the greeter user
  (`settings.modify.system` when `subject.local && subject.active`); otherwise pre-login
  connections are `permissions=user:greeter` and invisible to the logged-in user. Discretionary
  (doc 42 §7.3).
- **Breeze SDDM theme** not pinned (`plasma-login-manager` [external]).
- gnome-session's `CanShutdown` → login1 mapping, lightdm-gtk-greeter a11y, and polkit's
  session-activeness backend are [external].
- **Empty-password UX**: no greeter special-cases a passwordless account; all rely on PAM success
  without a prompt. The XR greeter must be tested with `nullok` to confirm the `auth_message`-less
  `success` path of greetd IPC (`greetd/greetd_ipc/src/lib.rs:56-67`) renders sensibly.
