# 53 — Proximity/presence native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Hardware path only; lock/doff policy remains in doc 12 and ADR 0007. This supersedes proximity
enablement detail—not bootstrap policy—in doc 42.

## Verdict

| Device/profile | Evidence | State |
|---|---|---|
| Quest 1 / `monterey:base` | stock optical wear behavior; native identity/path incomplete | A0 |
| Lynx / `lynx-r1:base` | STK3X3X inventory/stock logs; mainline driver disabled | A0 |
| Lynx / `lynx-r1:mod:postmarketos-mainline` | explicit proximity regression relative to stock | A0 |
| Galaxy XR / `sm-i610:base` | SSC proximity→Monado head-detect source; runtime reported | A0-qualified; runtime-through-R4 reported |
| PFDM / `pfdm-mr:base` | vendor wear-sensor listing only | A0 |
| Frame / `deckard:base` | exact VCNL4040 DT/driver; no userspace/Monado consumer | A0 |
| Quest 3 / `eureka:base` | physical/stock wear detection; native path unknown | A0 |

`sm-i610:mod:arm-proximity-slider` is a qualifying COMMUNITY-ADOPTED profile: a reproducible cover
deliberately forces “near.” It shares base L1–L6 but changes physical semantics. No current
contract can express an intentionally defeated presence source.

## Per-target L0–L7

### Quest 1

Interior optical sensor placement and stock sleep/wake are community-reported. The archived kernel
contains no joined sensor compatible/DT/native ABI; stock VR power-manager control is a service
knob, not sensor ABI. No Monado target driver. Keep `proximitySource=none` until identity, transport,
permissions and consumer close.

### Lynx R1

Public hardware inventory names STK3X3X and vendor docs describe wear-triggered standby
([external] [exact hardware dump](https://raw.githubusercontent.com/ellyq/lynx-mainline/285eee1ad89f537a0f2afc34db4da8598729bdc2/hardware.txt)).
Stock logs report near/far to Android. Dumped DT has no matching node; pinned mainline config
disables STK3310 and all proximity/distance drivers
(`references/pmaports/device/testing/linux-lynx-r1/config-lynx-r1.aarch64:7092,7218-7236`).
SLPI firmware presence does not join this sensor. Lynx must not be described as native IIO today.

### Samsung Galaxy XR

- **L0:** vendor behavior and community mods place the unknown sensor on the left arm.
- **L1/L2:** fork discovers SSC service 400 `"proximity"` SUID over QRTR/QMI and requests on-change
  events (`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/galaxyxr_ssc.c:45-60,494-505,763-775`).
- **L3:** ADSP/SSC firmware and `/mnt/vendor/persist/sensors/registry` are required but unpinned.
- **L4–L6:** QMI/protobuf state `1=near`, `2=far` feeds
  `XRT_INPUT_GENERIC_HEAD_DETECT`, `supported.presence=true`, and OpenXR presence events. Monado
  source is verified; runtime behavior including panel power is reported.
- **L7:** no SM-I610 Mura device/donor closure. Correct eventual source class is `ssc`, but not yet
  qualified.

The slider profile intentionally makes presence continuously true while covered; qualification
must prove uncovered restoration and must not treat presence as authentication.

### Play For Dream MR

Vendor lists a wear-detection sensor. Placement/component/bus/power/DT/firmware/HAL ABI/service/
Monado path are all unknown. No Galaxy transfer through `anorak`.

### Valve Steam Frame

MP DT identifies VCNL4040 at I²C `0x60` with supplies and threshold parameters
(`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:2285-2309`);
kernel enables IIO/VCNL4000. Standard IIO raw/threshold ABI is expected, but no runtime node,
consumer or event route is archived; DT has no IRQ, so polling may be required unless missing
kernel source changes semantics. Upstream Monado has no IIO intake/Deckard driver.
`proximitySource=iio` is a static hardware fact, not R1 support.

### Meta Quest 3

Interior sensor placement and stock sleep behavior are documented/reported, but chip, bus, DT,
firmware/service and native ABI are unknown. No target Monado driver; keep none.

## Artifact and state boundaries

Most wear sensors need no standalone firmware, but SSC/MCU routes inherit their exact donor
firmware and registry ABI. Galaxy's sensor registry is per-unit protected state; Lynx SLPI is a
proprietary pinned donor; Quest service closures are proprietary; Frame threshold values are
board-DT data but matching custom kernel source is missing. Presence state itself is ephemeral and
never identity or authentication evidence.

## Runtime evidence bundle

R1 enumerates exact IIO node, SSC SUID, HID report or equivalent and Monado advertises presence.
R2 controlled cover/uncover advances raw/boolean state without errors/resets. R3 verifies polarity,
threshold/hysteresis, timestamp, fit range, face-interface behavior and false positives. R4 proves
initial/change events for ordinary greeter/session clients with device handoff. R5 covers cold
boot, suspend, repeated fit, doff/don, greeter transition, profile attachment/removal and XR load.

OpenXR requires initial state after `xrBeginSession`, change events, and none after end. Sensor
debounce is separate from authentication grace. Presence never authenticates; don resumes only
inside explicit policy grace.

## Contract/preflight consequences

`proximitySource` must mean a qualified native producer, not stock hardware. B1b currently does not
check presence; a future check needs backend-specific enumeration and initial-sample validity.
Frame's enum outruns its Monado integration; Galaxy's source is implemented but donor closure is
unpinned. Contract lock-trigger defaults and the doff policy described by ADR 0007 should be
reconciled separately; this audit changes no policy or options.

## Contradictions / deciders

- Doc 42's “only PSVR2 upstream” is stale (other non-target drivers exist), but no target driver is
  added by that correction.
- Lynx IIO prose versus disabled driver/no DT: board support work.
- Galaxy optimistic initial “present” before first sample: first-sample validity test.
- Frame no-IRQ VCNL route versus threshold-event expectation: retail IIO/journal/source.
- Five silent `none` defaults correctly avoid false support but make doff unavailable.
- Slider profile versus profile-free contract: later hardware-profile policy decision.
