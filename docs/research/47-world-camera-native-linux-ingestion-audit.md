# 47 — World-camera native-Linux ingestion audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
World-facing and attachable cameras through frame delivery; warp/depth algorithms remain in
docs 13/14/16/22. Eye cameras are doc 52.

## Verdict

| Device/profile | Native evidence | State |
|---|---|---|
| Quest 1 / `monterey:base` | downstream Qualcomm camera foundations; no sensor graph/API | A0 |
| Lynx R1 / `lynx-r1:base` | stock six-camera graph/SDK; mainline exact sensor drivers disabled | A0 |
| Galaxy XR / `sm-i610:base` | Titan→dma-buf→Monado source verified; 90-fps behavior reported | S1; runtime-through-R4 reported |
| Play For Dream / `pfdm-mr:base` | enterprise Y8 tracking and VST still APIs; no native closure | A0 |
| Steam Frame / `deckard:base` | exact four-camera CAMSS static topology; no open consumer/calibration | A0 |
| Frame + Arcturus / `deckard:acc:arcturus-vision` | vendor A0 profile; strong inferred `arcimx616` donor join | A0 |
| Quest 3 / `eureka:base` | strong stock Camera2 RGB API; no native board route | A0 |

No row passes P-1. Galaxy is not formally R4 without captured logs and pinned Titan/kernel/
calibration closure. Steam base remains monochrome; color belongs only to the Arcturus profile.

## Per-target L0–L7

### Quest 1

Four vendor-documented monochrome tracking cameras feed proprietary Insight. Meta's build-specific
kernel enables Qualcomm CSIPHY/CSID/ISPIF but disables the sensor driver
([external] [Meta kernel commit](https://github.com/facebookincubator/oculus-linux-kernel/commit/589280fc40ddbcc2287024c8b672568a0fdd68e7)).
No exact sensors/lanes/sync, firmware/calibration schema, V4L2 graph, Camera2 path, Monado source or
Mura declaration exists. `vision`, `persist` and `private` remain protected per-unit state.

### Lynx R1

- **L0:** vendor SDK documents stereo RGB NV12 1536×1404, merged stereo tracking Y8 1280×400, and
  stereo hand Y8 400×400 ([external] [Lynx capture documentation](https://portal.lynx-r.com/documentation/view/video-capture?version=9)).
- **L1/L2:** exact stock dump contains six CSIPHY/camera cells, rails, 24-MHz clocks, reset GPIOs,
  EEPROMs and tracking-light controls
  ([external] [exact dump commit](https://github.com/ellyq/lynx-mainline/blob/285eee1ad89f537a0f2afc34db4da8598729bdc2/dumps/lynx-dumped-dt.dts#L24569-L25564)).
  Mainline CAMSS is enabled, but reported OV4689/OV9282 drivers are disabled
  (`references/pmaports/device/testing/linux-lynx-r1/config-lynx-r1.aarch64:4239-4472`).
- **L3:** per-unit QVR calibration/lens files are recoverable from official firmware.
- **L4–L6:** stock LynxCapture/QXR returns pointers/AHardwareBuffers; ordinary frame timestamp is
  not exposed. ORB-SLAM integration is Android/QXR, not native Monado.
- **L7:** no backend/readiness record; exact role-to-sensor mapping and trustworthy timestamps are
  first static/runtime deciders.

### Samsung Galaxy XR

- **L0:** two 3000×3000 RGB, six 640×640 tracking and one dToF sensor are vendor-documented.
- **L1/L2:** exact board wiring/source is unpinned; the Linux reference delegates ISP ownership to
  Android `titan-server`.
- **L3:** per-unit EFS `rgb-left_curved`/`rgb-right_curved` is an atomic family containing
  intrinsics, IMU extrinsics, rolling-shutter timing and cover-window optics
  (`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:740-921`).
- **L4/L5:** Titan v12 uses `SOCK_SEQPACKET`, SCM_RIGHTS dma-buf rings, group/view/frame identity,
  RAW/NV12 formats, QTimer exposure metadata and explicit release
  (`src/external/titan/titan_proto.h:5-152,215-294`).
- **L6:** Monado opens stereo 3000²@93 NV12, validates both views and imports buffers once
  (`galaxyxr_passthrough.c:245-354,583-618,702-899`). Reported one-quad operation holds 90 fps.
- **L7:** donor bridge is source-verified but no Mura package/qualification. Only 3DoF is available,
  the 4.3-ms alignment sign is visual rather than objective, depth readout is incomplete and full
  concurrent P-1 load is untested.

### Play For Dream MR

Vendor material lists color/tracking/eye/depth classes, “11 cameras,” dual 32-MP spatial capture
and 22 IR emitters without a reliable role map. The enterprise sample exposes Y8 tracking streams
and NV21 VST **still** capture, not VST streaming
([external] [PFDM camera sample](https://github.com/PlayForDreamDevelopers/CameraSample-Unity/blob/main/README.md)).
No board CSI, kernel, firmware/calibration, dma-buf ABI or Monado adapter exists publicly. Galaxy
`anorak` evidence does not transfer.

### Valve Steam Frame base

- **L0:** production DT model `4slam 2et` identifies outward `ovti,og01a1bx` left/right and
  `ovti,og0ve10x` upper left/right; base passthrough is monochrome
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:4439-4583`).
- **L1/L2:** exact lanes, rails, clocks, resets/strobes, six CSIPHYs and SM8650 CAMSS are present;
  kernel enables CAMSS and product sensor drivers
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/config-6.18.0-deckard:3963-3992,4167-4172`).
- **L3:** `CAMERA_ICP.mbn` and ICP/BPS/IPE resources exist; calibration schema, source pairing and
  redistribution remain unknown.
- **L4:** V4L2/media/dma-buf and diagnostic tools ship, but no archived enumeration/frame.
- **L5–L7:** stock SteamVR proves operation through an unidentified proprietary route; no open
  service/Monado intake/readiness record.

### Arcturus Vision accessory

Arcturus is a sold, vendor-supported A0 profile. Vendor material states dual 32-MP Sony RGB, 64-mm
baseline and high-rate color modes ([external] [Arcturus Vision](https://arcturus.vision/)). The MP donor contains
two attachment-detectable `arcimx616` sensors named `passthrough_left/right`, paired EEPROMs,
detect/mux GPIOs and dual four-lane CAMSS routes
(`sm8650-mp.dts:4312-4398,4660-4684`; `CONFIG_VIDEO_ARCIMX616=y`).

The topology is verified; its attribution to Arcturus is **INFERRED** until attached/unattached
enumeration binds sensor IDs/profile. Image transport is MIPI in the DT; the expansion connector's
PCIe lane is not image-path evidence. EEPROM schema/license, hotplug daemon, stock package,
V4L2 nodes, native frames and Monado intake remain unknown. Base inherits host CAMSS/ICP only,
never RGB capability.

### Meta Quest 3

Physical/stock evidence supports two 4-MP RGB cameras, four tracking cameras and an active IR
projector. Horizon v74+ exposes left/right RGB through Camera2/MRUK as YUV420 up to 1280²@60,
with intrinsics/extrinsics, timestamps and NDK AHardwareBuffer
([external] [Meta native camera API](https://prod.developers.meta.com/horizon/documentation/native/android/pca-native-documentation/)).
That improves stock confidence only. No Eureka board camera graph, firmware/calibration closure,
safe native boot or Monado bridge exists. The projector assists depth; it is not a depth-camera
stream.

## Artifact and calibration boundaries

Camera firmware, synchronization metadata and calibration are one profile/revision-bound closure.
Quest donor services and factory state are proprietary; Lynx firmware redistribution is
unspecified and calibration is per-unit; Galaxy Titan/OEM licensing is unresolved and EFS is
per-unit; PFDM native closure is unavailable; Frame camera firmware/drivers are mixed-license
`localOnly` with missing corresponding source; Arcturus EEPROMs are attachment-specific and their
schema/license is unknown. None is transplanted between units or profiles.

## Runtime evidence bundle

R1 enumerates driver, firmware and media graph. R2 proves stereo atomicity, exposure/readout
metadata, clock mapping and bounded buffering. R3 establishes view identity, physical formats,
intrinsics/distortion/extrinsics and atomic calibration. R4 requires zero-copy delivery to the
ordinary active-session perception consumer with inactive-session release. R5 covers concurrent
SLAM+Mercury+render, reset/suspend, sustained bandwidth/thermal load and profile hotplug/removal.

Arcturus additionally requires attached/unattached topology diffs, sensor chip IDs, EEPROM
identity, hotplug cleanup and proof that detachment removes color capability.

## Contract/P-1 consequences

No profile passes P-1. Galaxy is the closest runtime reference; Frame is the strongest native
static candidate; Lynx is the easiest open-boot experiment. `mura.adaptation.camera` can classify
native versus device-specific/donor bridge, but research does not justify camera capability facts
or profile options. The typed camera-array schema requested by the perception backlog still needs
role/profile, sync group, format/rate, calibration URI/version, timestamp/exposure model,
color metadata and dma-buf capabilities.

## Contradictions / deciders

- Frame four mono base sensors versus attachment-ready RGB nodes: enumerate with accessory absent.
- Arcturus PCIe press shorthand versus MIPI media graph: attached `lspci` + `media-ctl`.
- `arcimx616` plus 32-MP Sony claims: strong IMX616 inference; runtime chip-ID log decides.
- Galaxy 6.5-MP launch output versus 3000² Titan mode: full sensor versus processed mode inventory.
- Galaxy 4.3-ms calibration sign: LED/photodiode plus motion test.
- Lynx OV9282/OV4689 BOM versus generic cells: probe logs/HAL metadata.
- PFDM camera count/category mismatch: owned-device DT/HAL dump.
- Quest 3 “depth sensor” language versus projector+stereo: synchronization/depth API evidence.
