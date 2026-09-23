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
