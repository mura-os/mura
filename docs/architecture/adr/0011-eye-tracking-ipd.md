# ADR 0011: Eye tracking and IPD — Monado-side eye-frame service, rotation-center IPD, event-gated motors

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [28-eye-tracking-stack](../../research/28-eye-tracking-stack.md),
[29-eye-hardware-ipd-per-target](../../research/29-eye-hardware-ipd-per-target.md).
Extends [adr/0008-perception-services-placement.md](0008-perception-services-placement.md) (perception
services Monado-side) to the eye-camera group; implements the per-user-IPD and biometric hooks
deferred by [adr/0007-session-greeter-lock.md](0007-session-greeter-lock.md).

## Context

OpenXR has no IPD API — rendering IPD is the per-view pose separation, owned by the runtime; Monado
drivers today only *read* mechanical IPD sensors and turn an `eye_relation` vector into view poses
(`u_device.h:146-151` documents the override point). Motorized auto-IPD is eye tracking plus a servo:
IR-illuminated per-eye cameras → pupil detection → 3D eye-model fit → **eyeball rotation center**
(the vergence-invariant quantity; raw pupil-center distance is fixation-dependent by ~2 mm at near
fixation) → lens motors + rendering/distortion update ([28 §1](../../research/28-eye-tracking-stack.md)).

The hardware audit ([29](../../research/29-eye-hardware-ipd-per-target.md)) split the targets:
Quest 1 and Lynx R1 have **no eye cameras** (manual-sensed IPD); Galaxy XR and Play For Dream are
**motorized-auto** (eye-tracked servo, Galaxy XR also iris auth); Steam Frame has eye cameras for
foveation with a **manual** dial. Critically, **no target documents plain V4L2 access to eye
cameras** — everywhere they sit behind vendor runtime/DSP services, so donor acquisition must capture
the vendor ET service closure, and the `device-specific`/`android-backed` adaptation paths are the
realistic ones.

## Decision

### 1. Contract modeling

- **`spatial.hardware.ipd.source`** ∈ `fixed | manual | manual-sensed | stored | motorized-auto` —
  the source of the rendering-IPD value: hardcoded default; unsensed mechanical (user-entered value);
  device-reported mechanism position; per-user stored software value (fixed optics — Lynx-class);
  eye-tracked motorized servo. Plus `spatial.hardware.ipd.defaultMeters` (safe pre-auth/greeter
  default, per ADR 0007).
- **`spatial.adaptation.eyes`** joins the per-subsystem backend matrix:
  `none | native | android-backed | device-specific` (default `none` — most targets lack the
  hardware). `android-backed` = the donor's ET service closure; `native` = our own pipeline on
  directly accessible cameras; `device-specific` = bespoke (e.g. vendor DSP protocol).
- Assertions: `ipd.source = motorized-auto` requires `eyes.backend != none`; an `android-backed`
  eyes backend requires a pinned donor (like every other subsystem).

### 2. Pipeline and placement (extends ADR 0008, with an honest correction)

The ET service runs **Monado-side as a consumer of a session-scoped eye-camera frame group** —
ADR 0008's shape — but for *different* reasons than passthrough: ADR 0008's pose-at-exposure
argument **does not apply** (eye cameras are rigid to the device), and the shared-frames argument is
weak (nothing else consumes eye frames). The binding reasons ([28 §4a](../../research/28-eye-tracking-stack.md)):
**Monado is itself the consumer** (gaze must answer `get_tracked_pose` in the input pipeline; IPD
feeds `get_view_poses`/`eye_relation` — both Monado-internal), camera bring-up already lives in the
frameserver, the clock domain is shared for free, and the **privacy boundary** (clients receive gaze
*pose* via `XR_EXT_eye_gaze_interaction`, never eye images) falls out of the existing IPC design.

Baseline pipeline (per authenticated session): **PuRe-class classical detector (CPU; LGPL
`pupil-detectors` lineage, not EyeRecToo's non-commercial sources) → ported pye3d two-sphere model
with refraction correction** — the only studied implementation whose primary output *is* the
vergence-invariant rotation center, in metric eye-camera coordinates ([28 §1.2](../../research/28-eye-tracking-stack.md)).
Outputs: (i) gaze via an eyes-role `xrt_device` + relation history (the in-tree PSVR2/WiVRn pattern),
(ii) filtered rotation-center IPD into the driver's `eye_relation`, (iii) an actuation-proposal
event. A quantized RITnet-class CNN front end is a per-device NPU substitution behind the same
ellipse+confidence interface. A VIT-style plugin ABI (option b) is deferred until a second backend
exists; an external Python process (option c) is the **prototyping rig only**.

### 3. Motors: event-shaped, never a servo loop

Actuation is `propose → confirm-or-idle-policy → move → re-settle`: median-filtered IPD over
minutes of high-confidence samples, a dead-band (~0.5 mm class) with hysteresis, rate-limited
proposals; the actuator is a bounded, slow, encoder-fed positioner in the **per-device Monado
driver** (a device control, like WMR's control packets — never generic). During travel the eye
models are **frozen** (cameras may move with the lens tubes) and per-unit extrinsics + distortion
are re-resolved at the new encoder position before unfreezing
([28 §5.2–5.4](../../research/28-eye-tracking-stack.md)).

### 4. Calibration, sessions, privacy (ties to ADR 0007)

- Per-unit eye-camera intrinsics/extrinsics and distortion, **keyed by IPD encoder position**
  (interpolatable table), are **system state** — same rule as all calibration.
- The per-user long-term eye model and measured IPD are **per-user session state**; the greeter/lock
  renders with `ipd.defaultMeters` and the session applies the user's value post-login — exactly the
  ADR 0007 handoff, now with a mechanism.
- Eye cameras are **session-scoped**: off pre-auth (ADR 0007 tiered tracking); the greeter never
  sees eye frames. Gaze reaches clients only through `XR_EXT_eye_gaze_interaction` (pose, not
  pixels); raw eye images never leave the service. Iris authentication (Galaxy XR-class hardware;
  Vision Pro Optic ID as the reference) is the future biometric-unlock path ADR 0007 reserved — it
  would run beside PAM as a parallel verifier inside this same privacy boundary, never replacing the
  credential.
- Measurement UX: a fixation-target "IPD wizard" at enrollment (controlled vergence, fast
  convergence) plus continuous rotation-center monitoring for drift/re-donning prompts
  ([28 §5.1](../../research/28-eye-tracking-stack.md)).

## Consequences

- Contract gains `spatial.hardware.ipd.*` and `spatial.adaptation.eyes` with assertions and tests;
  the qualification matrix gains per-device rows: *IPD source*, *eye-camera access class*, *ET
  capability*, *iris auth* (values per [29's matrix](../../research/29-eye-hardware-ipd-per-target.md)).
- Donor pipeline: for `android-backed` eyes, the donor manifest must capture the vendor ET
  service/DSP closure and ET calibration files — no target offers V4L2 eye cameras
  ([29 §executive result](../../research/29-eye-hardware-ipd-per-target.md)).
- Engineering items (backlog, post-spike): the pye3d C/C++ port as a Monado frame sink; the
  PuRe-class detector; the eyes-role device + `eye_relation` plumbing; the per-device motor driver
  interface. Open questions carried in [28 §6](../../research/28-eye-tracking-stack.md) (BSP eye-camera
  access + timestamps per target, encoder-swept calibration production, kappa/visual-axis
  calibration UX, NPU contention, per-app gaze permission policy, upstreaming posture,
  foveation latency budget).

## Alternatives considered

- **ET in the compositor / per-app:** rejected — ADR 0008's deadline rule and the camera privacy
  boundary; the compositor imports results, never frames.
- **PCCR (glint-based) as the required design:** rejected as baseline — needs calibrated illuminator
  geometry unavailable on donor hardware; per-device enhancement where doc 29 establishes LED
  geometry.
- **Direct gaze regression (NVGaze-style) as primary:** rejected — no explicit eyeball center means
  no IPD; possible later as a low-power gaze-only mode.
- **Closed-loop motor servoing off the raw estimator:** rejected — event-shaped, hysteresis-gated
  actuation only.
- **A second plugin ABI now (VIT-style):** deferred — it is a refinement of the chosen placement,
  added when a second backend materializes.
- **Skipping contract modeling until a device needs it:** rejected — the IPD source already differs
  across all five targets today.
