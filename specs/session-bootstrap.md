# Session bootstrap: the wrapper greetd execs, `mura-session.target`, and the environment contract

**Status:** **rev 4 (2026-09-28, G3 — the zxr session)** — rev 3 with zxr as the compositor in `mura-compositor.service`: §4.5/§7 readiness through `mura-session finalize`, now **spawned by zxr** once its socket is bound (the compositor knows only `NOTIFY_SOCKET` and `WAYLAND_DISPLAY`; `STOPPING=1` native), sway and its drop-in gone from the module and the target, §5 the tree with zxr, §8 re-run with zxr (rev 3.15 of zxr-core, gate 11). **Flagged (rule 4):** rev 3 §7 foresaw zxr calling `sd_notify` *and* setting its own variables; the chosen shape keeps one code path for the environment publication (the same child every stand-in used) and leaves the `systemctl`/`dbus-update-activation-environment` calls out of the compositor — the owner may prefer the in-process shape. Rev 3 (2026-09-25, D4 rev 3 landed) — rev 2 recorded what rung **D4** verified
with **uwsm** as the wrapper. rev 3 replaces uwsm with **`mura-session`** (`pkgs/mura-session`,
Rust, `libc` only) over **static** user units, keeping every mechanism rev 2 verified and
re-running the same conformance checklist (§8). Why: the ruling that *no interpreter sits on
the session-start path* (§9; AGENTS.md) — uwsm is 6.4k lines of Python and three interpreter
starts per login, plus login-time unit generation and a `daemon-reload`. Revised at **G3** (rev 4)
when the zxr session replaced sway. Normative for `modules/os/session.nix` and
`pkgs/mura-session`.
**Design source:** [implementation-path.md §2 (ii) B6/B6a](../docs/architecture/implementation-path.md),
[ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md) (session body, crash/restart,
boot-locked restart), [session-auth.md §5](session-auth.md) (greetd's exit-then-start ordering),
[first-run-onboarding.md §5.3](../docs/architecture/first-run-onboarding.md) (the passwordless
posture the PAM session step inherits).
**Grounding:** "XDG" here is the Base Directory spec (`$XDG_RUNTIME_DIR`); the systemd
`graphical-session-pre.target` / `graphical-session.target` conventions are freedesktop's and are
used unmodified so upstream portals, PipeWire and D-Bus activation integrate without patches.
The unit semantics are uwsm's (v0.26.7 at the pinned nixpkgs revision, `lib/systemd/user/*`,
cited as *engineering evidence* of a working shape), copied into static units; systemd citations
to `references/systemd`.
**Budget impact** (overview invariant 9): login-time process work only — one static binary
(~620 KiB, no runtime beyond libc), a handful of `systemctl` calls, then a process asleep for the
session's lifetime; nothing on the frame path. Measured in the VM (§9): login → graphical
session **5.9 s → 2.0 s** against uwsm.

## 1. The problem this spec closes

greetd's session worker `exec`s **one command** as the user after PAM `open_session`. A systemd
*target* is not an executable, a user unit cannot `BindsTo=` the system manager's session scope,
and greetd (multi-user profile) restarts the greeter the moment its session command returns —
so whatever greetd execs must (a) bring the user's systemd manager to the XR session target,
(b) publish the compositor's runtime environment to user services *only after the compositor is
ready*, and (c) **stay alive until the whole session is torn down**, or the next greeter races
the compositor for the DRM device. That executable is the **session wrapper**. On the appliance
profile greetd's `initial_session` execs the same wrapper; only the return path differs (§6).

**A fourth requirement, found in the D4 review (F1):** a compositor started as a *user unit*
runs outside the logind session scope. libseat's logind backend looks the session up by PID,
fails, and falls back to the `XDG_SESSION_ID` environment variable — so the wrapper must carry
the login session's identity into the compositor unit or seat acquisition fails. This is the
classic "compositor as systemd user service" trap; uwsm's answer (an `EnvironmentFile=` on the
compositor unit) is the one kept.

## 2. Actors

| Actor | Runs as | Role |
|---|---|---|
| greetd session worker | root → drops to the user | PAM (`greetd` service): `authenticate`, `open_session` (which starts `pam_systemd` → logind session + user manager + `$XDG_RUNTIME_DIR`), then **forks** the session command — the worker itself is the logind session **leader** (verified: `loginctl show-session -p Leader` is the `greetd --session-worker` PID) |
| `mura-session start` (the wrapper) | the user | `pkgs/mura-session`, one static binary; the leader's child, in the session scope; writes `session.env`, exports the static class, binds the session to its own PID, `systemctl --user start --wait mura-session.target`, stops it on `SIGTERM/HUP/INT`, cleans up; returns only after teardown (§4) |
| user `systemd --user` | the user | hosts the four static Mura units and everything under `graphical-session.target` |
| `mura-compositor.service` | user unit (Mura, static) | **the compositor unit**: `Type=notify`, `NotifyAccess=all`, `EnvironmentFile=-%t/mura/session.env`, `TimeoutStartSec` = the readiness bound, `Restart=on-failure` + `RestartMode=direct`; `ExecStart` is the compositor — sway until M1 (**STAND-IN**) |
| `mura-session.target` | user unit (Mura, static) | the XR session body named across the corpus and **what the wrapper `--wait`s on**: `Requires=mura-compositor.service`, `Wants=monado.socket`, `BindsTo=graphical-session.target`, ordered after `graphical-session-pre.target` and **before** `graphical-session.target` (§5) |
| `mura-session-bindpid@<pid>.service` | user unit (Mura, static template) | `waitpid -e <pid>` (util-linux) on the wrapper: a dead wrapper ends the session |
| `mura-session-shutdown.target` | user unit (Mura, static) | the one-way exit: `Conflicts=` the whole graphical session, `StopWhenUnneeded`; every Mura unit's `OnSuccess=`/`OnFailure=` (`replace-irreversibly`) |
| `mura-session finalize` | the user, inside the compositor unit (zxr's child) | the readiness hook (§4.5, §7): publishes the compositor-created variables and sends `READY=1` on the unit's `NOTIFY_SOCKET` |
| `monado.socket`/`monado.service` (user) | the user | socket-activated OpenXR runtime; the session's *own* Monado instance (`services.monado`) |
| the compositor | the user | `zxr` (rev 4; sway was the stand-in through G2); binds its socket, then spawns `mura-session finalize` to publish its variables and signal readiness (§4.5, §7) |
| `graphical-session-pre.target` / `graphical-session.target` | user units (upstream) | freedesktop's layering points; Mura adds nothing to them, only orders around them |

## 3. Environment: three classes, three publication moments

| Class | Examples | Set by | Visible to |
|---|---|---|---|
| **Static** | `MURA_PROFILE` (`appliance`/`multi-user`), `XDG_SESSION_TYPE=wayland`, locale (`LANG`), `XDG_CURRENT_DESKTOP=mura`, `XDG_SESSION_DESKTOP=mura`, `XDG_SESSION_CLASS` | `environment.d` drop-ins generated by the NixOS module (`/etc/environment.d/60-mura.conf`, plus NixOS's own `50-systemd-path.conf` for `PATH`); the wrapper's **explicit** `set-environment` of the four `XDG_*` above (step 2) — never a bare `import-environment` | the user manager at start, hence every user unit (verified: `systemctl --user show-environment`) |
| **Session-specific** | `XDG_SESSION_ID`, `XDG_SEAT`, `XDG_VTNR` (from PAM/greetd) | written by the wrapper into `$XDG_RUNTIME_DIR/mura/session.env` (0600, directory 0700), the `EnvironmentFile=` of `mura-compositor.service` | **the compositor unit only** — deliberately *not* the user manager, which outlives sessions (verified: present in sway's `/proc/<pid>/environ`, absent from `show-environment`). This is the F1 seat mechanism |
| **Compositor-created** | `WAYLAND_DISPLAY`, `DISPLAY` (Xwayland, when present) | the compositor, published **only after** it is ready — `mura-session finalize` exports them to the user manager and D-Bus and sends `READY=1` (sway stand-in); zxr will `sd_notify` natively | user services started *after* `graphical-session-pre.target`; D-Bus-activated services via the activation environment |
| **Dependent** | nothing set; these are *consumers* | portals, PipeWire/WirePlumber, the settings daemon, notification service, shell services | ordered `After=graphical-session-pre.target`, `PartOf=graphical-session.target` — upstream's own conventions |

Rules: **no compositor-created variable is ever exported before readiness** (verified:
`WAYLAND_DISPLAY` in the manager names a bound socket); **the compositor never inherits a
previous instance's compositor-created variables** — on a direct restart (§5) the manager still
holds the dead instance's values, and a compositor that inherits `WAYLAND_DISPLAY` tries to run
nested inside itself (found at D4 with sway) — hence `UnsetEnvironment=WAYLAND_DISPLAY DISPLAY`
on the compositor unit; **the login's session type is not imported** — greetd's login is a `tty`
session to PAM, and importing `XDG_SESSION_TYPE` from it would overwrite `wayland` in the manager
(found at D4 rev 3; uwsm hard-codes `wayland` for the same reason); **the compositor unit has no
unit-private `PATH`** — it inherits the manager's session `PATH` (`/run/wrappers/bin`, the
per-user and system profiles) like any desktop session unit, or sway's `exec` lines fail with
`ENOENT` (found at D4 rev 3); and no dependent service is ordered before
`graphical-session-pre.target`.

## 4. The wrapper, step by step (`mura-session start`)

1. **Entry.** greetd's worker has forked `mura-session start` as the user; `$XDG_RUNTIME_DIR`,
   `$XDG_SESSION_ID` and the static environment are set. The wrapper waits (≤ 30 s, polling
   `systemctl --user is-system-running`) for the user manager `pam_systemd` started.
2. **Environment.** The session-specific variables go to `$XDG_RUNTIME_DIR/mura/session.env`
   (§3). The static `XDG_*` identity is `set-environment`ed into the user manager from an
   explicit list — `XDG_CURRENT_DESKTOP=mura`, `XDG_SESSION_DESKTOP=mura`,
   `XDG_SESSION_TYPE=wayland`, and `XDG_SESSION_CLASS` from the login — never a bare
   `import-environment` (the manager's `PATH`, locale and profile variables come from
   `environment.d`, not the login shell).
3. **Bind, then start the session body.** `systemctl --user start mura-session-bindpid@<own
   pid>.service`, then fork `systemctl --user start --wait mura-session.target` (signals default
   in the child). The target requires `mura-compositor.service` and binds to
   `graphical-session.target`.
4. **Wait for readiness**: `mura-compositor.service` is `Type=notify`; systemd waits for
   `READY=1` up to `TimeoutStartSec`, the **readiness bound** =
   `mura.xr.session.readinessTimeoutSeconds` (contract, default 30 s). On timeout the unit fails
   with `Result=timeout`, `OnFailure=mura-session-shutdown.target` tears the session down, and
   step 7 follows (verified: a never-notifying `sleep` stub is torn down at 30 s).
5. **Publish the compositor-created environment**: the compositor itself, once it has its socket,
   runs `mura-session finalize [VAR…]`, which `systemctl --user set-environment`s and
   `dbus-update-activation-environment --systemd`s `WAYLAND_DISPLAY`, `DISPLAY` (when set) and
   the named variables, then sends `READY=1` on the inherited `NOTIFY_SOCKET` (`sendto` on an
   `AF_UNIX` datagram socket, abstract or filesystem — the sd_notify wire protocol, no library).
   `graphical-session-pre.target` and `graphical-session.target` are reached through the unit
   ordering, not by the compositor starting them. The wrapper does not publish anything — it
   only waits.
6. **Sleep, coupled.** The wrapper stays alive for the session's lifetime, polling the `--wait`
   child; `mura-session-bindpid@<pid>.service` binds the graphical session to its PID so a dead
   wrapper also ends the session. `SIGTERM/HUP/INT` (greetd or logind tearing the session down)
   → `systemctl --user stop --no-block mura-session.target`, then continue waiting for that stop.
7. **Teardown, in order.** Stopping `mura-session.target` stops `mura-compositor.service`
   (`BindsTo=`), `graphical-session.target` (`BindsTo=`/`PropagatesStopTo=`) and everything
   `PartOf=` it — Monado included; the wrapper then `unset-environment`s what it exported plus
   `WAYLAND_DISPLAY DISPLAY`, removes `session.env`, and exits `0`. Only then does greetd's
   worker close the PAM session. **Exit status is not the failure signal**: the wrapper waits on
   a *target*, and targets do not fail, so it returns `0` on a normal logout *and* on a readiness
   timeout; greetd never inspects it anyway (the greeter restarts / the autologin re-runs either
   way, §6). The failure record is the unit result and the journal
   (`mura-compositor.service: start operation timed out`, `Failed with result 'timeout'`); the
   B1b crash-loop counter consumes that, not an exit code.

The wrapper has no privileges and no D-Bus surface of its own. `systemctl` and
`dbus-update-activation-environment` are store paths baked at build time (`MURA_SYSTEMCTL`,
`MURA_DBUS_UPDATE_ENV`), so nothing depends on the login `PATH`. A test-only `--target UNIT`
flag points the wrapper at a different target (the readiness-bound test, §8 item 6); production
callers pass nothing.

## 5. Units and ordering

All four units are static files in `/etc/systemd/user` (NixOS `systemd.user.*`), generated at
build time — no login-time unit generation, no `daemon-reload`.

```
mura-session.target                                  ← what the wrapper --wait's on
├── Requires= mura-compositor.service                # THE COMPOSITOR: zxr (rev 4; sway until G2)
│     Type=notify NotifyAccess=all EnvironmentFile=-%t/mura/session.env
│     BindsTo=mura-session.target  Before=mura-session.target graphical-session.target
│     Wants=/After=graphical-session-pre.target  PropagatesStopTo=mura-session.target graphical-session.target
│     Restart=on-failure RestartMode=direct RestartSec=1s StartLimitBurst=3/60s
│     TimeoutStartSec=<readiness bound> TimeoutStopSec=10s UnsetEnvironment=WAYLAND_DISPLAY DISPLAY
│     OnSuccess=/OnFailure=mura-session-shutdown.target (replace-irreversibly) Slice=session.slice
├── Wants=    monado.socket                          # the session's own Monado, socket-activated
├── BindsTo=  graphical-session.target               # starts it; stops with it
├── After=    graphical-session-pre.target
├── Before=   graphical-session.target               # the compositor reaches it on readiness
└── StopWhenUnneeded=yes  Conflicts=mura-session-shutdown.target
mura-session-bindpid@<wrapper pid>.service           # waitpid -e; OnSuccess/OnFailure → shutdown
mura-session-shutdown.target                         # DefaultDependencies=no; Conflicts= graphical-session-pre.target
                                                     #   graphical-session.target xdg-desktop-autostart.target; StopWhenUnneeded
graphical-session.target: portals, pipewire/wireplumber, settings daemon, shell services —
                          upstream ordering, unmodified
```

- **Ordering rule learned at D4:** target units implicitly order `After=` their
  `Requires=`/`Wants=`, so `mura-session.target` must be ordered *before*
  `graphical-session.target`, never after it — the rev-1 shape was an ordering cycle systemd
  silently deleted. (Verified rev 3: no `ordering cycle` in the user manager's journal.)
- **Crash semantics on the compositor unit:** ADR 0007 restarts the compositor *inside* the
  session (into the locked state once authd exists, D5). uwsm shipped `Restart=no` +
  `OnFailure=` (compositor death = session end). Since systemd v254 a failing service passes
  through `failed` before an auto-restart and `OnFailure=` fires *every* time
  (`references/systemd/src/core/service.c` `SERVICE_FAILED_BEFORE_AUTO_RESTART`;
  `unit.c` `unit_notify`) — so `Restart=` alone still ended the session (measured at D4).
  `RestartMode=direct` is the standard answer: auto-restarts skip the failed state, dependents
  are not notified, and `OnFailure=` fires only when the start-rate limit is hit — at which
  point the session ends and the B1b crash-loop ladder takes over. The burst/interval values
  are plasmashell's, with its stated reason ([research/56 §1](../docs/research/56-defaults-from-comparables.md)).
- **What was dropped from uwsm's tree, and why it is safe:** the per-compositor templating
  (`@<id>` instances — Mura has one compositor), the envelope and pre targets (they existed to
  sequence uwsm's env-preloader unit, which the wrapper's step 2 replaces), `wayland-wm-env@`
  (login-environment import with a never-export list — replaced by the explicit list),
  `wayland-session-waitenv`, the app slices and `fumon`. The kept invariants: compositor
  `BindsTo=` the session target; the session target `BindsTo=` `graphical-session.target`;
  every unit's end pulls the shutdown target irreversibly; `bindpid` in the other direction.
- `monado.service`/`monado.socket`: the upstream `services.monado` user units. **Two Monado
  instances never overlap**: the greeter's Monado (owned by greetd's greeter session) has exited
  before greetd forks the wrapper (session-auth §5's exit-then-start), and the session's Monado
  belongs to the user manager and dies with `mura-session.target`.
- Lifetimes are **manager-correct**: nothing in the user manager references the system manager's
  session scope; the *wrapper process* — in that scope — is the only coupling, one-directional
  (scope teardown → SIGTERM → wrapper stops the target), plus `bindpid` in the other direction
  (wrapper death → session shutdown).
- `services.dbus.implementation = "broker"` stays (uwsm's module set it; the reason — activation
  environment handling for units the session starts — holds without uwsm). `mkDefault`, so a
  profile may choose otherwise.

## 6. Profiles

- **Multi-user:** greetd `default_session` runs the greeter; `start_session` → the greeter exits
  → greetd forks `mura-session start` as the chosen user (gtkgreet's `/etc/greetd/environments`
  lists `mura-session start`; PAM's `pam_env` supplies `PATH`). Wrapper return → greetd restarts
  the greeter (`services.greetd.restart = true`, the NixOS default when no `initial_session` is
  set). Logout is therefore: `mura-session.target` stops → wrapper returns → greeter (verified:
  `systemctl --user stop mura-session.target` → gtkgreet back within seconds, the old session
  gone, `session.env` removed, no DRM master errors). User switch = logout + login.
- **Appliance / default image:** greetd `initial_session` forks `mura-session start` as `mura`
  at boot; `services.greetd.restart = false` (NixOS sets this default when `initial_session`
  exists, so a crash does not autologin in a loop — the compositor unit's `Restart=` handles
  crashes inside the session, verified: `kill -9 sway` → back in the same logind session).
  Wrapper return (logout) leaves greetd's `default_session`: on the default image that is the
  same autologin again — an explicit "power off / restart" is the way out, exactly the Steam
  Deck shape.

## 7. The compositor's readiness (rev 4: zxr; D4–G2: sway, the stand-in)

**Rev 4.** `mura-compositor.service`'s `ExecStart` is `zxr`. Once the listening socket is bound
and `WAYLAND_DISPLAY` is in its environment, zxr — when `NOTIFY_SOCKET` is set and the mode is not
greeter — spawns **`mura-session finalize`** as a child (its defaults are the compositor-created class): the child runs
`systemctl --user set-environment`, `dbus-update-activation-environment --systemd` and sends
`READY=1` over the inherited `NOTIFY_SOCKET` (`NotifyAccess=all`), exactly what the sway drop-in
did; at teardown zxr writes `STOPPING=1` to the socket itself (`sd_notify(3)`'s datagram, no
library). `DISPLAY` follows the same path once satellite is up. The compositor therefore knows
no systemd beyond two environment variables, and the environment publication has one
implementation for every compositor the unit ever ran. **Flagged (rule 4):** rev 3 wrote that zxr
would call `sd_notify` directly *and* set its own variables in-process; the spawned-child shape is
the plan's choice (one code path, nothing bus-shaped in the compositor), not a forced one.

**D4–G2, removed at rev 4.** sway as the session body: the NixOS sway module's default
`config.d/nixos.conf` started `sway-session.target` → `graphical-session.target` itself, *before*
the readiness ordering; `modules/os/session.nix` `mkForce`d that file to one line —
`exec mura-session finalize SWAYSOCK I3SOCK` — so the compositor performed §4.5 through the
wrapper's `finalize` subcommand. sway has no sd-notify of its own; `finalize` sent `READY=1` on
its behalf from inside the unit.

## 8. Conformance checklist (VM, both fixtures — `tests/vm/default-image.nix`, `tests/vm/multi-user.nix`)

Re-verified in full at rev 3 with the same meaning; only unit and path names changed.

1. **Seat from inside the user unit (F1):** sway's cgroup is `mura-compositor.service`;
   `XDG_SESSION_ID` is in `session.env` (mode 0600) and in the compositor's environment and
   *not* in the user manager's; no libseat errors in the journal. **Verified, both fixtures.**
2. **Wrapper lifetime = session lifetime:** one logind session for the login; its leader is
   greetd's worker; the PID bound by `mura-session-bindpid@` is the wrapper (`mura-session
   start` on its command line), in the session scope, descending from the leader. **Verified.**
3. **No pre-readiness leak:** `WAYLAND_DISPLAY` in the user manager names a bound socket; the
   static variables (`MURA_PROFILE`, `XDG_SESSION_TYPE=wayland`, `XDG_CURRENT_DESKTOP=mura`) are
   present; nothing of greetd's own (`GREETD_SOCK`) leaked; `TimeoutStartUSec` is the contract
   value. **Verified.**
4. **Logout without a race:** multi-user fixture, stop `mura-session.target` → the greeter is
   back, the old session is gone, `session.env` is removed, no `EBUSY`/DRM-master failures in
   the journal. **Verified.**
5. **Compositor crash:** `kill -9 sway` → restarted by the unit inside the *same* logind session;
   the wrapper does not return; `mura-session.target` and `graphical-session.target` come back.
   **Verified.** (With a credential set, the session comes back locked — ADR 0007 — verified in
   D5 once authd exists.)
6. **Readiness timeout:** a compositor that never signals readiness (TEST-ONLY
   `mura-compositor-stub.service` / `mura-session-stub.target`, same shape as the real units,
   reached through `mura-session start --target`, in an SSH-created logind session) is torn
   down at the bound (30 s) with `Result=timeout`; the stub target ends; the wrapper returns.
   **Verified.** Its exit status is `0` — see §4 step 7.
7. **`mura-session.target` and Monado:** the target is active with the session and
   `monado.socket` is listening; no ordering cycle in the user manager's journal. **Verified.**
8. **Two Monados never overlap:** at no instant do a greeter-owned and a session-owned Monado
   both hold the HMD — *not yet exercised*: the stand-in greeter runs no Monado (G1/G2).
9. **No interpreter on the session-start path (rev 3):** no `uwsm` or `python` process owned by
   the login user; no `/run/user/<uid>/uwsm`; no `wayland-wm@*` unit; no `daemon-reload` in the
   user manager's journal at login. **Verified, default image.**

## 9. Timing, the language ruling, and open items

**Measured (VM, pixman renderer, same host, multi-user fixture, journal monotonic clock; PAM
`session opened` → `Reached target Current graphical user session`):**

| | uwsm (rev 2, `/tmp/c-multi.log`) | `mura-session` (rev 3) |
|---|---|---|
| session opened → `graphical-session-pre.target` | 2.69 s | 1.07 s |
| → compositor ready + `graphical-session.target` | +3.27 s | +0.59 s (incl. sway's own start) |
| **login → graphical session** | **5.96 s** | **1.65 s** (two runs: 1.65 s, 1.96 s) |

On the default image the fixture prints the same interval from the journal on every run
(`login -> compositor ready: 1.19s`, PAM `session opened` → `ActiveEnterTimestampMonotonic` of
the compositor unit) and asserts it under 15 s. The uwsm figure carried three Python interpreter
starts (`uwsm start`, `uwsm aux prepare-env`, `uwsm aux exec`), unit generation into
`$XDG_RUNTIME_DIR/systemd/user` and a `daemon-reload`. The rev 3 figure is dominated by the user
manager's own start (`pam_systemd` → manager basic) and sway. On the target SoC every one of
those seconds is longer; the wrapper's remaining share is a few `systemctl` round-trips.

**Ruling recorded (2026-09-24, project owner):** no shipping desktop or appliance puts an
interpreter on the session-start path, and Mura does not either; Rust is the language for Mura
programs (the compositor's language). This spec's wrapper is the first consequence; uwsm is
retained in the corpus as engineering evidence only.

**Open items**, each naming its decider: the compositor unit's start-rate limit's interplay
with the B1b crash-loop counter — decider: the owner's ruling on research/56 Q1 (the values
themselves are sourced: `StartLimitBurst=3` in 60 s is plasmashell's, chosen because systemd's
5-in-10 s default cannot catch a component whose start-and-crash cycle exceeds 2 s —
`plasma-workspace/shell/plasma-plasmashell.service.in:5-6`, commit 9149b81e; research/56 §1);
the readiness-timeout default (30 s, schema value; the VM's sway is ready in ~1 s
— decider: real-hardware measurements at G1); how the docked-mode flat presenter (ADR 0015)
joins the target — decider: the docked-mode rung after G3; **uwsm's app-launch side**
(`uwsm app`: launching applications as transient user scopes/services under `app-graphical.slice`
so the compositor's OOM/kill behaviour never takes the apps with it) is *not* ported — the shell
(zxr) will launch applications and will need the same mechanism (`systemd-run --user --scope`
or D-Bus `StartTransientUnit`); decider: the zxr shell design at M1/G3. Closed by D4: the
wrapper is a plain process, not a transient scope (as uwsm and GDM/SDDM-equivalents do). Closed
by rev 3: the wrapper is a Mura program; its unit tree is static.
