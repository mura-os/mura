# ADR 0007: Session, greeter, and lock model — autologin appliance + greetd multi-user, lock as compositor state

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [11-display-managers-greeters](../../research/11-display-managers-greeters.md),
[12-lock-screens-and-appliance-login](../../research/12-lock-screens-and-appliance-login.md).
Builds on [adr/0006](0006-compositor-strategy.md) (the zxr compositor is a single OpenXR client of
Monado that composites everything itself) and [zxr-shell-v2-composition.md](../zxr-shell-v2-composition.md).

## Context

The login/lock surface is the one piece of userspace that needs the *entire* XR display path —
panel/DRM bring-up, lens distortion, IPD, and at least rotational (IMU) tracking — running **before
any user session exists**. That inverts the desktop assumption that a display manager hands a bare
compositor to the GPU: on a headset, Monado + an XR compositor must already be up for anything
legible to render. The research settled the mechanics:

- **greetd** is the right daemon: a tiny PAM broker over a socket whose greeter can be *any program*,
  including a whole compositor; its `initial_session` gives passwordless autologin and its
  `default_session` gives an authenticated greeter ([11 §2, §5](../../research/11-display-managers-greeters.md)).
- **The greeter can be our compositor in a restricted mode** (the GDM "greeter is the shell" pattern,
  concretely demonstrated by gtkgreet-in-cage's single-client kiosk model —
  [11 §2.4, §3](../../research/11-display-managers-greeters.md)).
- **Sequential per-user Monado handoff is sound**: Monado's IPC socket lives in per-user
  `$XDG_RUNTIME_DIR`, so two sequential instances (greeter user → session user) don't collide; the
  real shared resources (DRM master, hidraw) are brokered by logind/seatd, and greetd's
  exit-then-start sequencing means there is never live two-compositor contention
  ([11 §4, §7](../../research/11-display-managers-greeters.md)).
- **A lock screen maps cleanly onto internal compositor state**: because our compositor composites
  everything and submits one projection layer, "locked" is just "compose only the lock scene, route
  input only to it" — the `ext-session-lock-v1` obligations translate into an internal state machine,
  and the protocol's own `finished` clause anticipates compositors that authenticate internally
  ([12 §2.4](../../research/12-lock-screens-and-appliance-login.md)). Android's shell-integrated
  Keyguard is the consumer-scale proof.

## Decision

### Two profiles

**Appliance profile (default — Quest/SteamOS model).** No display-manager UI, ever. Auto-login the
owner account straight into the zxr session (greetd `initial_session`, the mechanism Jovian's
`gamescope-session` uses via hidden-SDDM autologin — [12 §5.1](../../research/12-lock-screens-and-appliance-login.md)).
Security is the **compositor-integrated lock**, which is the *only* auth surface the user sees. The
session comes up **locked whenever a credential is enrolled** (Quest "lock on power-on/sleep"), so
the greeter-less boot is still safe. The session body is a systemd user target
(`spatial-session.target`) owning Monado, the compositor, and shell services, so crash/restart is
systemd's job; any mutable "next-boot" override must be self-clearing (Jovian's failsafe).

**Multi-user / desktop profile.** greetd `default_session` runs the **zxr compositor in `--greeter`
mode** as a dedicated `greeter` user: it brings up Monado + the XR display path (IMU-only tracking,
no client Wayland socket), composes one built-in auth scene (login panel + session list), speaks
`$GREETD_SOCK`, and exits on `start_session`; greetd then starts the chosen user session (its own
Monado + compositor). Same compositor binary, restricted mode. The dev/desktop profile also keeps
the standard `ext-session-lock-v1` path so ordinary tooling (swayidle + swaylock/hyprlock) works.

### The lock model: internal compositor state, three invariants

The built-in lock is **not** a separate process and **not** (by default) the `ext-session-lock-v1`
protocol; it is a compositor state machine obeying ([12 §2.4](../../research/12-lock-screens-and-appliance-login.md)):

- **I1** — while locked, no client colour/depth buffer is sampled and no input reaches any client
  (focus is withdrawn; the seat routes only to the lock scene).
- **I2** — "locked" is reported externally (logind `SetLockedHint`, any suspend sequencer) **only
  after** a composition pass containing zero client samples has been submitted via `xrEndFrame`
  (the same present-before-suspend race the protocol guards against).
- **I3** — unlock happens only via a successful PAM conversation or an explicitly configured grace
  policy; a compositor/runtime crash restarts the session **into the locked state**.

`ext-session-lock-v1` is **exposed only** in the dev/desktop profile and (behind privileged-client
policy) for third-party headset lockers composed as a head-locked quad — never the mechanism for the
built-in lock.

### PAM out of process

Authentication runs in a small `spatial-authd` helper over a socketpair (swaylock's fork model /
kscreenlocker's auth-boundary split), so the compositor never links libpam and a hung/crashing PAM
module (fprintd timeout, network modules) can't stall `xrWaitFrame`. NixOS service
`security.pam.services.spatial-lock` (owned by the module), `pam_faillock` included; the lock UI
renders **generic** PAM prompts (`visible`/`secret`/`info`/`error`) with a controller-ray PIN-pad
fast path and a ray-reachable virtual keyboard fallback. PIN is a `pam_spatial_pin`-style
argon2-hashed credential in per-unit system state (MVP: owner-password-is-PIN). Biometrics (iris/
face) come later as a parallel unlock path beside PAM, never replacing it — the eye-camera privacy
boundary and hardware substrate for iris auth are specified in
[adr/0011-eye-tracking-ipd.md](0011-eye-tracking-ipd.md).

### Cross-cutting requirements

- **Per-unit calibration (lens/distortion/IPD) is system state** (`/var/lib/spatial/` or vendor
  persist), never `$HOME` — pre-auth greeter/lock rendering and the pre-Monado splash all need it
  before any user logs in. This tightens [device-contract.md](../device-contract.md)'s calibration
  paths and `protectedPartitions` wording (calibration is `backupOnlySensitive`/system, per-user
  IPD is a preference the session applies post-login).
- **Tiered tracking:** greeter/lock run rotation-only (IMU); cameras/SLAM (full 6DoF) come up only
  with the authenticated session — faster boot and a privacy property (cameras stay off until auth).
- **Doff/don/idle policy** ([12 §6.2](../../research/12-lock-screens-and-appliance-login.md)):
  doff (via `XR_EXT_user_presence` in the compositor's own OpenXR loop) → blank panels immediately +
  start a grace timer; don within grace (default ~30–60 s) → resume without re-auth; don after grace,
  idle-past-lock, explicit lock, suspend/resume, and boot → re-auth. Presence never *unlocks* without
  a biometric ("a head is here" ≠ "the owner"). Serve `ext-idle-notify-v1`, honor
  `zwp_idle_inhibit_v1`.
  *Amended 2026-09-23 ([ADR 0015](0015-docked-desktop-mode.md)):* the ladder gains a **docked
  branch** — doff while a docked output is active enters the quiescence ladder (XR stack down,
  flat output live) instead of blank-and-grace; whether it also locks the docked presentation is
  the `lockOnDoffWhileDocked` policy, and the ordinary idle-to-lock ladder still applies to the
  docked output. The greeter and lock scenes gain a flat presentation on the docked monitor.
  Invariants I1–I3 are untouched: locked means locked on every presentation.
- **Boot splash:** no Plymouth on the appliance HMD (a full-panel undistorted splash is visually
  broken through lenses); dark panels or, as a stretch goal, a static per-eye *pre-distorted* logo
  driven from system-state calibration. The boot-locked compositor is the first legible UI; anything
  before it is cosmetic.
- **Sessions come from the module system** (`spatial.xr.shell` values: `zxr`/`stardust`/`wayvr`),
  surfaced to the greeter — not hand-written `.desktop` files.

## Consequences

- New contract options under `spatial.xr.session.*` (see below) select profile and lock policy,
  mirroring the `spatial.xr.shell` pattern; typed, defaulted, assertion-checked.
- `services.greetd` is reused as the daemon in both profiles (`initial_session` vs `default_session`);
  `services.displayManager.sddm/gdm` are **not** used ([11 §6, §9](../../research/11-display-managers-greeters.md)).
- The compositor gains a `--greeter` mode and an internal lock state machine; `spatial-authd` and
  `security.pam.services.spatial-lock` are new components.
- [overview.md](../overview.md) and [zxr-shell-v2-composition.md](../zxr-shell-v2-composition.md) are
  updated: the session/login path is now a defined part of the common layer, and "lock = compose
  only the lock scene" is noted as a composition-policy state.

## Alternatives considered

- **System-wide shared Monado across users** — rejected; fights Monado's per-user runtime-dir service
  model and creates a cross-user socket/security problem, unnecessary given the sequential handoff
  ([11 §7](../../research/11-display-managers-greeters.md)).
- **SDDM/GDM as the daemon** — rejected; wrong coupling (they assume they spawn the greeter compositor
  / a 2D themed greeter). greetd's minimal broker + our compositor-as-greeter is the fit.
- **kscreenlocker-style separate lock-UI process** — rejected; the lock scene must render through the
  compositor's Monado path anyway, so a second XR-rendering process buys isolation we already take at
  the PAM boundary, at the cost of restart supervision and a bespoke trust channel
  ([12 §3](../../research/12-lock-screens-and-appliance-login.md)).
- **Mandatory `ext-session-lock-v1` for the built-in lock** — rejected as the mechanism; internal
  state is conformant in spirit and simpler. Kept as a compatibility surface for dev/third-party.
- **Presence-based unlock without biometrics** — rejected; don ≠ owner.

## Open questions

Carried from [11 §10](../../research/11-display-managers-greeters.md) and
[12 §8](../../research/12-lock-screens-and-appliance-login.md): HMD device-release latency across the
greeter→session handoff (USB re-enumeration); Monado's actual `XR_EXT_user_presence` coverage per
target (qualification-matrix item); grace-window default and any "same head re-donned" heuristic
(needs a privacy/security review); logind vs seatd on the appliance image (**default resolved to
logind** at implementation-path B2, forced at G2; seatd stays an appliance-minimization option);
and whether the desktop profile's `ext-session-lock-v1` support should extend to third-party
headset lockers. *PIN storage/enrollment UX is **closed** by
[ADR 0017](0017-first-run-provisioning.md): option (b) `pam_spatial_pin` (argon2 hash in the
`enrollment/` state class, enrolled through `spatial-provisiond` at OOBE), with
owner-password-is-PIN as the recorded appliance bridge.*
