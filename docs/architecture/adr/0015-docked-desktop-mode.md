# ADR 0015: Docked desktop mode — one session, flat presentation, quiescence ladder

**Status:** accepted (draft)
**Date:** 2026-09-23
**Context sources:** the external-video-out verification in
[07 §External video-out](../../research/07-device-landscape.md); the capture taxonomy
([spatial-sharing.md §2.2](../spatial-sharing.md)); the session/presence model
([ADR 0007](0007-session-greeter-lock.md)); perception placement and its unit boundaries
([ADR 0008](0008-perception-services-placement.md)); the KWin VR study's virtual-screen evidence
([31 §2.6](../../research/31-kwin-vr.md)). Extends the output-path structure in
[desktop-environment.md §6.2](../desktop-environment.md).

## Context

A standalone headset with USB-C DisplayPort alt-mode can drive a monitor. That enables the
DeX-class product question: when docked, is Mura a PC? The verified hardware reality
(doc 07): Quest 3 supports video-out officially, Galaxy XR demonstrably (community-verified,
vendor-undocumented), Lynx R1 reportedly, Steam Frame **cannot** (USB 2.0 port only). So the
feature is real on flagship targets and impossible on others — it must be a fact-gated option,
never an assumption.

Three candidate architectures were weighed:

- **B1 — exclusive session switch:** greetd offers a stock Plasma/GNOME session on the monitor;
  the XR session ends. Nearly free to package; no continuity; a different world.
- **B2 — concurrent second compositor:** KWin/mutter drives the external connector while
  zxr/Monado drive the HMD. Founders on DRM mastership (one master per device; full compositors
  cannot run as lease clients), doubles session state, and puts two compositors' thermal load on
  an XR2-class SoC.
- **B3 — zxr drives the docked output itself:** the same session presents flat on the monitor.
  This is how DeX actually works on phones (one compositor, both surfaces), and it is what our
  window model already implies: windows are not bound to outputs (ADR 0013's mirrored seam), so
  "docked desktop" is placement policy plus 2D presentation, not a second desktop.

The performance objection to B3 — a mobile SoC must not burn XR-grade power while the headset
sits on the desk — is answered by observing that everything expensive about XR mode is already a
separable, gated subsystem: perception services are independent units under
`mura-session.target` (ADR 0008), tiered tracking exists in the boot direction (ADR 0007's
IMU-only greeter), Monado is socket-activated ([overview.md](../overview.md)), and 2D desktop
composition is damage-driven rather than cadence-driven.

## Decision

**B3, in two tiers, gated on a new contract fact, with a quiescence ladder that reaches B1's
power profile without a session switch. B1 is demoted to a packaged escape hatch; B2 is
rejected.**

### The contract fact

`mura.hardware.externalDisplay = none | dp-altmode | usb-display` (per-device declared fact;
defaults per doc 07's verification table). Everything below is absent when `none`
(Steam Frame, Quest 1).

### Tier 1 — Mirror

Plugging in a display may mirror, per user selection: `full-scene × head-view (mono crop)`
(spectate-style) or a `window-set × flat-composition` surface — the **scanout realization of the
capture-taxonomy cells** ([spatial-sharing.md §2.2](../spatial-sharing.md)); always undistorted
(post-distortion stays a debug artifact). Prior art is Meta's wired mirroring (hotplug
auto-start, DRM blanking, no audio — doc 07), with one deliberate inversion: **Meta mirrors
passthrough by default; we follow the §2.2 default — passthrough excluded unless explicitly
consented.** Mirroring to a physically attached display is presenting to a room: the active-share
badge duty applies.

### Tier 2 — Docked desktop

The compositor gains a **third output path** beside the OpenXR loop and the desktop dev window:
a flat-composition target scanned out on the external DRM connector. Properties:

- **Same session, same apps, two presentations.** A docked window-set presents flat on the
  monitor; layer-shell panels anchor to it with *original* desktop semantics (no
  reinterpretation needed); keyboard/mouse arrive over the same dock. Doff and keep typing;
  don and the same windows are back in their places.
- **Presentation policy, not model state.** Which windows present on the docked output is
  placement policy over the unchanged window model. This is the one place something shaped like
  window↔output assignment legitimately returns — as **per-output presentation policy**
  (including fullscreen/direct-scanout on the docked connector), never as ownership in the model
  (the ADR 0013 seam stands).
- **Greeter and lock render on the monitor too.** The `--greeter` scene and the lock scene gain a
  flat presentation for the docked case — auth with a keyboard before donning, and recovery when
  the headset is doffed or its battery dead.

### The quiescence ladder

States and transitions (dock detect = DRM connector hotplug; presence = `XR_EXT_user_presence`):

| State | Perception units | Monado / XR loop | Docked output | Entered by |
|---|---|---|---|---|
| Undocked active | per session policy | full cadence | — | undock |
| Docked active | per session policy | full cadence | live (second presentation) | dock detect |
| Docked quiesced — **soft** | stopped (cameras off) | Monado alive, session idle, panels off | damage-driven, direct scanout eligible | doff while docked |
| Docked quiesced — **deep** | stopped | Monado + perception units stopped (systemd target subset; socket activation re-arms) | damage-driven | idle timer and/or on-external-power policy |

- **Soft→active on don is near-instant** (session resumes); **deep→active costs Monado re-init
  plus a relocalization pass** — which the mapping design's boot-reloc flow must handle anyway
  (anchored places survive service restarts, ADR 0009).
- In both quiesced states the compositor's cost profile is that of a plain 2D compositor: one
  flat scene, repaint on damage only, no `xrWaitFrame` cadence, no eye buffers, no perception —
  i.e. **B1's performance with B3's continuity**. Spatial scene *state* (window transforms,
  places) persists in memory throughout.
- Mechanism: `mura-session.target` gains a docked subset target; the perception services and
  Monado are already the right unit granularity (ADR 0008, overview).

### Presence and lock interaction (amends ADR 0007's ladder)

Doff-while-docked enters quiescence, **not** the blank-plus-grace-then-lock path: "headset off"
≠ "walked away" when the user is typing at the desk. Whether doff-while-docked *also* locks the
docked presentation is user policy (`lockOnDoffWhileDocked`, default off while input activity
continues on docked peripherals; the ordinary idle-to-lock ladder still applies to the docked
output, and the lock scene renders flat on it). Undocking while doffed rejoins the normal ADR
0007 doff ladder. ADR 0007's invariants I1–I3 are unchanged — locked means locked on *every*
presentation.

### Power/thermal ownership

Idle-depth selection (soft vs deep, timers, on-external-power bias) belongs to the power/thermal
policy component (registry service-plane row — this is its second concrete customer after
suspend sequencing). Docked-active additionally inherits the XR thermal budget for sustained
desktop workloads when quiesced — a product benefit worth stating: the headset is a *better*
desktop when the XR stack is off.

### Contract policy surface (doc-only until packaging)

`mura.xr.session.docked.{enable, lockOnDoffWhileDocked, deepIdleAfter}` — declared in
[device-contract.md](../device-contract.md) as doc-ahead-of-implementation, per the registry
§10.2 convention.

## Rationale

- **B3 is the only architecture consistent with the window model.** Windows have no output
  binding; a "docked desktop" that respawned a different compositor would re-create the 2D
  assumption we deleted, and lose the session-continuity product (the DeX precedent: one
  compositor, both surfaces).
- **The quiescence ladder removes B1's only real advantage.** The power argument for a separate
  2D session dissolves once perception, cadence, and composition are independently stoppable —
  and they already are, by prior decisions (ADR 0007 tiering, ADR 0008 unit boundaries,
  socket-activated Monado). Damage-driven flat composition is not an optimization we add; it is
  what a Wayland compositor does when no XR loop is running.
- **The hardware survey forces the fact-gating posture**: the strongest near-term Linux target
  (Steam Frame) can never dock; the flagship XR target (Galaxy XR) demonstrably can. A feature
  this asymmetric must live behind a declared fact, like eye tracking and IPD motors before it.
- **KWin VR's virtual-screen experience is the cautionary tale** ([31 §2.6](../../research/31-kwin-vr.md),
  and the user reports quoted there): screens-first models produce "four tilted screens hanging
  from the ceiling"; the fork's own author advises disabling physical screens and manipulating
  windows. Docked mode inverts cleanly *because* our primary model is windows-in-space — the flat
  output is a presentation of it, not the other way around.

## Alternatives considered

- **B2 — concurrent second compositor:** rejected. One DRM master per device; KWin/mutter cannot
  run as lease clients (leases hand connectors to *clients* like Monado, not to full
  compositors); two compositors' state and thermal load; and every cross-compositor question
  (input focus, clipboard, lock) would need inventing. The failure shape is the inverse of the
  KWin VR plugin's.
- **B1 — exclusive alternative session as the *primary* answer:** demoted, not deleted. It
  remains packaging-trivial (a greetd session entry) and ships as an escape hatch / purist
  option, but it kills the XR session, loses all spatial state, and its power advantage is
  matched by the quiescence ladder.
- **Post-distortion mirror as a user-facing mode:** rejected ([spatial-sharing.md §2.2](../spatial-sharing.md)
  — debug artifact; barrel-warped output is useless on a monitor).
- **Docked mode as a *capture* consumer** (mirror via PipeWire stream to a monitor-driving
  helper process): rejected for the attached-display case — scanout is direct, lower-latency,
  and does not spend an encode/transport path; the capture taxonomy governs *semantics* (which
  cells may be shown), not the transport.

## Consequences

- [desktop-environment.md](../desktop-environment.md): the §6.2 output paths become three; §5
  XR-redefinitions gains the docked-mode row ("the same session, flat presentation — not a
  different desktop").
- [ADR 0007](0007-session-greeter-lock.md) is amended: the doff/don ladder gains the docked
  branch; greeter/lock gain the monitor presentation; policy ownership for quiescence lives here.
- [device-contract.md](../device-contract.md) gains `mura.hardware.externalDisplay` and the
  `mura.xr.session.docked.*` options (doc-only).
- [component-registry.md](../component-registry.md) gains the docked output path (authority) and
  docked-mode policy/quiescence ladder (system+authority) rows; the power/thermal row gains this
  ADR as a customer.
- New open questions carried, each with its decider: dock-detect debounce and multi-monitor
  docks (decider: docked-mode implementation measurement on real dock hardware); audio routing
  while docked (Meta's mirror ships no audio; ours should route to dock outputs — decider: the
  audio-policy design round, registry gap 16); whether the docked output participates in capture
  as an ordinary `MONITOR` source (it should — one taxonomy; decider: the capture-tool design
  confirms no exception is needed); per-output fractional scaling on the monitor vs metric
  sizing in-space (decider: M1's sizing model applied to the docked presentation).
