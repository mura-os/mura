# Session bootstrap: the wrapper greetd execs, `mura-session.target`, and the environment contract

**Status:** **rev 2 (2026-09-24, D4 landed)** — rev 1 was a draft written before the rung; rev 2
records what the D-track rung **D4** verified in both VM fixtures against the sway stand-in
([implementation-path.md §3c](../docs/architecture/implementation-path.md)) and names the
mechanisms by file. The wrapper is **uwsm** (Universal Wayland Session Manager, `programs.uwsm`
in NixOS), not a Mura program: the rev-1 `mura-session-wrapper` was never written, because the
shape it described already exists (AGENTS rule 1) — evaluated against §4 and adopted. Revised
again at **G3** when the zxr session replaces sway. Normative for `modules/os/session.nix`.
**Design source:** [implementation-path.md §2 (ii) B6/B6a](../docs/architecture/implementation-path.md),
[ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md) (session body, crash/restart,
boot-locked restart), [session-auth.md §5](session-auth.md) (greetd's exit-then-start ordering),
[first-run-onboarding.md §5.3](../docs/architecture/first-run-onboarding.md) (the passwordless
posture the PAM session step inherits).
**Grounding:** "XDG" here is the Base Directory spec (`$XDG_RUNTIME_DIR`); the systemd
`graphical-session-pre.target` / `graphical-session.target` conventions are freedesktop's and are
used unmodified so upstream portals, PipeWire and D-Bus activation integrate without patches.
uwsm citations are to its source at the pinned nixpkgs revision (v0.26.7, `uwsm/main.py`,
`systemd/user/*.in`, `uwsm-libexec/signal-handler.sh`); systemd citations to `references/systemd`.
**Budget impact** (overview invariant 9): login-time process work; the wrapper sleeps for the
lifetime of the session; nothing on the frame path.

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
classic "compositor as systemd user service" trap, and it is precisely what uwsm was written for.

## 2. Actors

| Actor | Runs as | Role |
|---|---|---|
| greetd session worker | root → drops to the user | PAM (`greetd` service): `authenticate`, `open_session` (which starts `pam_systemd` → logind session + user manager + `$XDG_RUNTIME_DIR`), then **forks** the session command — the worker itself is the logind session **leader** (verified: `loginctl show-session -p Leader` is the `greetd --session-worker` PID) |
| `mura-session` (the wrapper) | the user | `writeShellScriptBin` execing `uwsm start -F -N Mura -D mura -- <compositor>`; the leader's child, in the session scope; owns coupling between the greetd session and the user target; returns only after teardown |
| `uwsm` | the user | generates per-compositor drop-ins in `$XDG_RUNTIME_DIR/systemd/user/`, saves the login environment, starts `wayland-session-bindpid@<its own pid>.service`, then execs `signal-handler.sh`, which forks `systemctl --user start --wait wayland-session-envelope@<id>.target` and stops it on `SIGTERM/HUP/INT` (`uwsm/main.py` "#### START"; `signal-handler.sh`) |
| user `systemd --user` | the user | hosts uwsm's session units, `mura-session.target` and everything under it |
| `wayland-wm@<id>.service` | user unit (uwsm) | **the compositor unit**: `Type=notify`, `NotifyAccess=all`, `EnvironmentFile=-%t/uwsm/env_session.conf`, `TimeoutStartSec` = the readiness bound; `<id>` is derived from the binary name — `sway` for the stand-in (`systemd/user/wayland-wm@.service.in`) |
| `wayland-session@<id>.target` / `wayland-session-envelope@<id>.target` | user units (uwsm) | the session and its envelope; `wayland-session@` `BindsTo=graphical-session.target` and `Wants=mura-session.target` (Mura drop-in) |
| `mura-session.target` | user unit (Mura) | the XR session body named across the corpus: `Wants=monado.socket`, `PartOf=graphical-session.target`, ordered after `graphical-session-pre.target` and **before** `graphical-session.target` (§5) |
| `monado.socket`/`monado.service` (user) | the user | socket-activated OpenXR runtime; the session's *own* Monado instance (`services.monado`) |
| the compositor | the user | `zxr` (or sway, the stand-in until M1); publishes its variables and signals readiness (§4.5) |
| `graphical-session-pre.target` / `graphical-session.target` | user units (upstream) | freedesktop's layering points; Mura adds nothing to them, only orders around them |

## 3. Environment: three classes, three publication moments

| Class | Examples | Set by | Visible to |
|---|---|---|---|
| **Static** | `MURA_PROFILE` (`appliance`/`multi-user`), `XDG_SESSION_TYPE=wayland`, locale (`LANG`), `XDG_CURRENT_DESKTOP=mura` | `environment.d` drop-ins generated by the NixOS module (`/etc/environment.d/60-mura.conf`); PAM `pam_env` for the greetd service; `XDG_CURRENT_DESKTOP` by `uwsm start -D mura` | the user manager at start, hence every user unit (verified: `systemctl --user show-environment`) |
| **Session-specific** | `XDG_SESSION_ID`, `XDG_SEAT`, `XDG_VTNR` (from PAM/greetd) | saved by `uwsm start` into `$XDG_RUNTIME_DIR/uwsm/env_session.conf`, the `EnvironmentFile=` of `wayland-wm@.service` (`uwsm/main.py` `Varnames.session_specific`) | **the compositor unit only** — deliberately *not* the user manager, which outlives sessions (verified: present in sway's `/proc/<pid>/environ`, absent from `show-environment`). This is the F1 seat mechanism |
| **Compositor-created** | `WAYLAND_DISPLAY`, `DISPLAY` (Xwayland, when present) | the compositor, published **only after** it is ready — `uwsm finalize` exports them to the user manager and D-Bus and sends `READY=1` (sway stand-in); zxr will `sd_notify` natively | user services started *after* `graphical-session-pre.target`; D-Bus-activated services via the activation environment |
| **Dependent** | nothing set; these are *consumers* | portals, PipeWire/WirePlumber, the settings daemon, notification service, shell services | ordered `After=graphical-session-pre.target`, `PartOf=graphical-session.target` — upstream's own conventions |

Rules: **no compositor-created variable is ever exported before readiness** (verified:
`WAYLAND_DISPLAY` in the manager names a bound socket); **the compositor never inherits a
previous instance's compositor-created variables** — on a direct restart (§5) the manager still
holds the dead instance's values, and a compositor that inherits `WAYLAND_DISPLAY` tries to run
nested inside itself (found at D4 with sway) — hence `UnsetEnvironment=WAYLAND_DISPLAY DISPLAY`
on the compositor unit; and no dependent service is ordered before `graphical-session-pre.target`.

## 4. The wrapper, step by step (as uwsm performs it)

1. **Entry.** greetd's worker has forked `mura-session` as the user; `$XDG_RUNTIME_DIR`,
   `$XDG_SESSION_ID` and the static environment are set. `uwsm start` needs a running user
   manager (`pam_systemd` started it) and a session D-Bus (`$XDG_RUNTIME_DIR/bus`).
2. **Environment.** `uwsm start` saves the login environment (`env_login`) and the
   session-specific variables (`env_session.conf`, §3). The preloader unit
   `wayland-wm-env@<id>.service` (`uwsm aux prepare-env`) imports the login environment into
   the user manager with an explicit never-export list and records what to clean up on stop —
   never a bare `import-environment`.
3. **Start the session body**: `systemctl --user start --wait wayland-session-envelope@<id>.target`
   (forked by `signal-handler.sh`). The envelope requires `wayland-session@<id>.target`, which
   requires `wayland-wm@<id>.service` and — Mura's drop-in — wants `mura-session.target`.
4. **Wait for readiness**: `wayland-wm@<id>.service` is `Type=notify`; systemd waits for
   `READY=1` up to `TimeoutStartSec`, the **readiness bound** =
   `mura.xr.session.readinessTimeoutSeconds` (contract, default 30 s; a Mura drop-in). On timeout
   the unit fails with `Result=timeout`, uwsm's `OnFailure=wayland-session-shutdown.target`
   tears the session down, and step 7 follows (verified: a `sleep` stub is torn down at 30 s).
5. **Publish the compositor-created environment**: the compositor itself, once it has its socket,
   runs `uwsm finalize [VAR…]`, which `systemctl --user set-environment`s and
   `dbus-update-activation-environment`s the variables and then sends `READY=1` to the unit
   (`uwsm/main.py` `finalize`). `graphical-session-pre.target` and `graphical-session.target` are
   reached through uwsm's unit ordering, not by the compositor starting them. The wrapper does
   not publish anything — it only waits.
6. **Sleep, coupled.** The wrapper (`signal-handler.sh`) stays alive for the session's lifetime,
   waiting on the `--wait` systemctl; `wayland-session-bindpid@<wrapper pid>.service` binds the
   graphical session to that PID so a dead wrapper also ends the session. `SIGTERM/HUP/INT`
   (greetd or logind tearing the session down) → `systemctl --user stop` of the envelope, then
   continue waiting for that stop.
7. **Teardown, in order.** Stopping the envelope stops `wayland-session@`, `wayland-wm@`
   (the compositor), `graphical-session.target` and everything `PartOf=` it — `mura-session.target`
   and Monado included; the preloader's `ExecStop` removes the exported variables. Only then does
   the `--wait` return and the wrapper exit; greetd's worker then closes the PAM session.
   **Exit status is not the failure signal** (rev 2 correction): the wrapper waits on a *target*,
   and targets do not fail, so it returns `0` on a normal logout *and* on a readiness timeout;
   greetd never inspects it anyway (the greeter restarts / the autologin re-runs either way, §6).
   The failure record is the unit result and the journal (`wayland-wm@<id>.service: start
   operation timed out`, `Failed with result 'timeout'`); the B1b crash-loop counter consumes
   that, not an exit code.

The wrapper has no privileges and no D-Bus surface of its own. Nothing here is Mura code except
the two drop-ins and the thin target of §5.

## 5. Units and ordering

```
wayland-session-envelope@sway.target (uwsm)          ← what the wrapper --wait's on
└── Requires= wayland-session@sway.target (uwsm)     BindsTo=graphical-session.target
    ├── Requires= wayland-session-pre@sway.target → wayland-wm-env@sway.service (env preloader)
    ├── Requires= wayland-wm@sway.service          # THE COMPOSITOR; sway until M1 (STAND-IN)
    └── Wants=    mura-session.target (Mura drop-in)
            ├── Wants=  monado.socket                # the session's own Monado, socket-activated
            ├── After=  graphical-session-pre.target
            ├── Before= graphical-session.target     # the compositor reaches it on readiness
            └── PartOf= graphical-session.target     # stops with the session
graphical-session.target: portals, pipewire/wireplumber, settings daemon, shell services —
                          upstream ordering, unmodified
```

Two Mura drop-ins on uwsm's units, both in `modules/os/session.nix`:

- `wayland-session@.target.d/overrides.conf`: `Wants=mura-session.target`. **Ordering rule
  learned at D4:** target units implicitly order `After=` their `Wants=`, so `mura-session.target`
  must be ordered *before* `graphical-session.target`, never after it — the rev-1 shape was an
  ordering cycle (`wayland-session@ → mura-session → graphical-session → wayland-session@`) and
  systemd silently deleted the job.
- `wayland-wm@.service.d/overrides.conf`, the compositor unit: `Restart=on-failure`,
  **`RestartMode=direct`**, `RestartSec=1s`, `StartLimitIntervalSec=60s` / `StartLimitBurst=3`,
  `TimeoutStartSec=<readiness bound>`, `UnsetEnvironment=WAYLAND_DISPLAY DISPLAY`. Rationale:
  ADR 0007's crash semantics restart the compositor *inside* the session (into the locked state
  once authd exists, D5); uwsm ships `Restart=no` + `OnFailure=wayland-session-shutdown.target`
  (compositor death = session end). Since systemd v254 a failing service passes through
  `failed` before an auto-restart and `OnFailure=` fires *every* time
  (`references/systemd/src/core/service.c` `SERVICE_FAILED_BEFORE_AUTO_RESTART`;
  `unit.c` `unit_notify`) — so `Restart=` alone still ended the session (measured).
  `RestartMode=direct` is the standard answer: auto-restarts skip the failed state, dependents
  are not notified, and `OnFailure=` fires only when the start-rate limit is hit — at which
  point the session ends as uwsm intends and the B1b crash-loop ladder takes over.
  [engineering judgment, D4; decider for the burst/interval values: the project owner]
- `monado.service`/`monado.socket`: the upstream `services.monado` user units. **Two Monado
  instances never overlap**: the greeter's Monado (owned by greetd's greeter session) has exited
  before greetd forks the wrapper (session-auth §5's exit-then-start), and the session's Monado
  belongs to the user manager and dies with `mura-session.target`.
- Lifetimes are **manager-correct**: nothing in the user manager references the system manager's
  session scope; the *wrapper process* — in that scope — is the only coupling, one-directional
  (scope teardown → SIGTERM → wrapper stops the envelope), plus uwsm's `bindpid` unit in the
  other direction (wrapper death → session shutdown).

## 6. Profiles

- **Multi-user:** greetd `default_session` runs the greeter; `start_session` → the greeter exits
  → greetd forks `mura-session` as the chosen user (gtkgreet's `/etc/greetd/environments` lists
  `mura-session`; PAM's `pam_env` supplies `PATH`). Wrapper return → greetd restarts the greeter
  (`services.greetd.restart = true`, the NixOS default when no `initial_session` is set). Logout
  is therefore: envelope target stops → wrapper returns → greeter (verified: `systemctl --user
  stop wayland-session@sway.target` → gtkgreet back within seconds, the old session gone, no DRM
  master errors). User switch = logout + login.
- **Appliance / default image:** greetd `initial_session` forks `mura-session` as `mura` at boot;
  `services.greetd.restart = false` (NixOS sets this default when `initial_session` exists, so a
  crash does not autologin in a loop — the compositor unit's `Restart=` handles crashes inside
  the session, verified: `kill -9 sway` → back in the same logind session). Wrapper return
  (logout) leaves greetd's `default_session`: on the default image that is the same autologin
  again — an explicit "power off / restart" is the way out, exactly the Steam Deck shape.

## 7. Stand-in specifics (D4, removed at M1)

sway as the session body: `mura-session` execs `uwsm start -F -N Mura -D mura -- <sway>`, so the
unit is `wayland-wm@sway.service`. The NixOS sway module's default `config.d/nixos.conf` starts
`sway-session.target` → `graphical-session.target` itself, *before* uwsm's readiness ordering;
`modules/os/session.nix` `mkForce`s that file to one line — `exec uwsm finalize SWAYSOCK I3SOCK`
— so the compositor performs §4.5 through uwsm (uwsm's documented sway integration). sway has no
sd-notify of its own; `uwsm finalize` sends `READY=1` on its behalf. The line carries
`# STAND-IN — replaced at M1 by zxr`; zxr will call `sd_notify` directly (the unit is
`NotifyAccess=all`) and set its own variables, and `finalize` leaves with sway.

## 8. Conformance checklist (VM, both fixtures — `tests/vm/default-image.nix`, `tests/vm/multi-user.nix`)

1. **Seat from inside the user unit (F1):** sway's cgroup is `wayland-wm@sway.service`;
   `XDG_SESSION_ID` is in `env_session.conf` and in the compositor's environment and *not* in
   the user manager's; no libseat errors in the journal. **Verified, both fixtures.**
2. **Wrapper lifetime = session lifetime:** one logind session for the login; its leader is
   greetd's worker; the PID bound by `wayland-session-bindpid@` is the wrapper, in the session
   scope, descending from the leader. **Verified.**
3. **No pre-readiness leak:** `WAYLAND_DISPLAY` in the user manager names a bound socket; the
   static variables (`MURA_PROFILE`, `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`) are present;
   nothing of greetd's own (`GREETD_SOCK`) leaked. **Verified.**
4. **Logout without a race:** multi-user fixture, stop the session target → the greeter is back,
   the old session is gone, no `EBUSY`/DRM-master failures in the journal. **Verified.**
5. **Compositor crash:** `kill -9 sway` → restarted by the unit inside the *same* logind session;
   the wrapper does not return; `mura-session.target` and `graphical-session.target` come back.
   **Verified.** (With a credential set, the session comes back locked — ADR 0007 — verified in
   D5 once authd exists.)
6. **Readiness timeout:** a compositor that never signals readiness (`sleep` stub through the
   same wrapper, in an SSH-created logind session) is torn down at the bound (30 s) with
   `Result=timeout`; the session shuts down; the wrapper returns. **Verified.** Its exit status is
   `0` — see §4 step 7.
7. **`mura-session.target` and Monado:** the target is active with the session and
   `monado.socket` is listening; no ordering cycle in the user manager's journal. **Verified.**
8. **Two Monados never overlap:** at no instant do a greeter-owned and a session-owned Monado
   both hold the HMD — *not yet exercised*: the stand-in greeter runs no Monado (G1/G2).

## 9. Open items

Each names its decider: the compositor unit's start-rate limit (`StartLimitBurst=3` in 60 s,
[mine]) and its interplay with the B1b crash-loop counter — decider: D6; the readiness-timeout
default (30 s, schema value; the VM's sway is ready in ~3 s — decider: real-hardware
measurements at G1); how the docked-mode flat presenter (ADR 0015) joins the target — decider:
the docked-mode rung after G3. Closed by D4: the wrapper is uwsm, not a Mura program; whether the
wrapper is a transient scope (no — a plain process, as uwsm and GDM/SDDM-equivalents do).
