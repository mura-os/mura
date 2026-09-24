# 52 — Eye-camera and IPD sensing/actuation native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
This supersedes only per-target enablement-path detail in [29](29-eye-hardware-ipd-per-target.md);
eye algorithms stay in [28](28-eye-tracking-stack.md), expression in doc 25.

## Verdict

| Device/profile | Physical/stock | Native state |
|---|---|---|
| Quest 1 / `monterey:base` | no eye cameras; manual-sensed IPD | A0 |
| Lynx / `lynx-r1:base` | no production eye cameras; manual-sensed IPD | A0 |
| Galaxy XR / `sm-i610:base` | four eye cams, gaze and Hall-derived IPD; motorized optics | A0-qualified; gaze/IPD-read runtime reported |
| PFDM / `pfdm-mr:base` | eye tracking and motorized optics; enterprise Y8 | A0 |
| Frame / `deckard:base` | two eye cams, stock gaze, manual IPD | A0 |
| Quest 3 / `eureka:base` | no eye cams; manual-sensed IPD | A0 |

No accessory profile qualifies. Arcturus is world-facing. No target has a demonstrated safe native
motor-write path; Galaxy Linux reads IPD but does not drive the motor.

## Per-target L0–L7

### Quest 1

Four cameras are outward. Teardown/community evidence indicates coupled manual optics with slide
potentiometer; bus, ADC, DT/driver, transfer curve and native ABI are unknown. Preserve
`persist/private/vision` without assuming any contains the curve. Contract consequence is only a
future runtime-qualified manual-sensed source; eyes/gaze remain none.

### Lynx R1

Production six-camera topology has no eye cameras. Independent lens movement and stock numeric IPD
are documented, but sensor count/type/bus, DT, conversion service and curve are unknown. Preserve
`device_calibration.xml` and lens CSVs. Pinned pmaports has no joined IPD/Hall path.

### Samsung Galaxy XR

- **L0:** four 640² inward cameras; four AKM `ak0997x` `trimag` Hall sensors feed an SSC `ipd`
  algorithm (`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:101-110`).
- **L1/L2:** SSC service 400 over QRTR/QMI emits on-change IPD event 717; the driver subscribes
  (`galaxyxr_ssc.c:45-69,410-506,760-779`). Motor power/control remains unknown.
- **L3:** OEM QNN/HVX model and `libgalaxyxr-eyetracking`; EFS display profile supplies optics.
  Eye/IR config paths exist, but complete hashes/licenses/sensor closure are unpinned.
- **L4/L5:** OEM library exposes derived samples/event fd, not raw pixels. Monado demand-opens the
  QNN/Titan eye path and closes it after the last gaze/foveation consumer.
- **L6:** combined gaze and IPD readback update relation/view poses; no per-user calibration and
  gaze origin remains head origin.
- **L7:** source/runtime is strong reference evidence, but cumulative qualification remains A0
  until opaque closure is pinned. Raw-camera and actuator-write remain A0.

### Play For Dream MR

Vendor documents eye tracking, automatic 51–78-mm optics and system IR emitters; exact eye-camera
count/geometry, LED allocation, buses, firmware/calibration and actuator protocol are unknown.
Enterprise DreamOS can return Y8 eye frames, a proprietary privileged stock API rather than V4L2.
No native service/Monado/motor path.

### Valve Steam Frame

Production model string records `2et`, and stock SteamVR gaze/foveation works. Sensor identity,
CCI/MIPI mapping, rails/IR, raw V4L2 ABI, calibration and Monado route remain unjoined. Manual dial
sensing is unverified; the ALS31300 Hall device must not be called IPD without physical/runtime
proof. `ipd.source` should eventually describe the physical manual mechanism, while gaze stays
unavailable until native R4.

### Meta Quest 3

No eye cameras. Manual wheel with stock numeric readout establishes physical manual-sensed intent,
but sensor type/bus/DT/curve/native ABI is unknown. Eyes/gaze remain none.

## Artifact and calibration boundaries

Factory eye intrinsics/extrinsics, IR geometry, IPD transfer curves, motor endpoints and
distortion-vs-position are per-unit system state. Per-user gaze/kappa/IPD is user state; iris
templates/keys are credential state. Galaxy's EFS profile and OEM model/library must be
build-bound and local; PFDM/Frame/Meta closures are proprietary or unavailable. Missing or
mismatched calibration permits read-only/default rendering only, never guessed actuation.

## Runtime gates

Gaze R1–R4 requires service/device enumeration, advancing monotonic samples, physical frame/origin/
combined-vs-per-eye semantics, occlusion handling, active-session ownership and bounded
gaze-to-photon latency. Raw-camera qualification additionally requires sensor identity, left/right,
format/rate, exposure/IR synchronization and an administrator diagnostic path while apps receive
poses only.

Motor writes do not qualify without independent readback, physical bounds, homing, timeout/stall/
obstruction and power-loss behavior. Commands clamp; models freeze and validity drops during
travel; completion atomically refreshes optics; failure stops motion and preserves usable
rendering. Factory eye/IR/actuator calibration, per-user gaze/IPD, and biometric credentials are
separate state classes.

## Contract/ADR consequences

Current `ipd.source` conflates mechanism, readback and Mura actuation. Galaxy proves the issue:
motorized hardware and readback exist without a Linux write path. `eyes.backend != none` cannot
prove motor safety. Raw-camera and vendor-derived-gaze are different backend capability classes;
Galaxy's OEM-derived path is legitimate device-specific evidence, while PuRe/pye3d remains the raw
path. `gaze` stays a runtime-qualified fact. ADR 0011's “encoder” wording should be generalized to
position readback. No new option follows automatically.

## Contradictions / deciders

- Galaxy motorized-auto hardware versus no motor command: matching motor HAL/source/runtime.
- Eye-service raw-image boundary versus administrator diagnostic access: ADR owner policy wording.
- Steam manual dial versus inherited fixed default: device declaration after sensing decision.
- Doc-28 universal illuminator language versus devices without eye cameras: narrow claim.
- Eye-camera motion with lens tubes: teardown/calibration sweep, not generic inference.
- Galaxy/PFDM/Quest 3 `anorak` strings: exact product/build identity only.
