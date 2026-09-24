# 46 — HMD display/panel/DRM native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Internal eye displays only; USB-C/external-display facts remain in
[07 §External video-out](07-device-landscape.md) and ADR 0015.

## Verdict

| Device/profile | Evidence | State |
|---|---|---|
| Quest 1 / `monterey:base` | strong old downstream dual-panel FB/MDSS blueprint | A0 |
| Lynx R1 / `lynx-r1:base` | stock dual-panel DRM verified; mainline panel route absent | A0 |
| Galaxy XR / `sm-i610:base` | complete dual-SDE/lease/Monado source; 72/90 Hz reported on glass | S1; runtime-through-R4 reported |
| Play For Dream / `pfdm-mr:base` | panel and calibration marketing facts only | A0 |
| Steam Frame / `deckard:base` | strongest exact MP DT/config; source/calibration/runtime missing | A0 |
| Quest 3 / `eureka:base` | detailed stock DTS/modules; no native boot/Monado | A0 |

No display accessory/mod profile qualifies. Panel suppliers and board revisions are SKU
constraints. Galaxy is not formally R4 without device-generated logs and pinned opaque closure.

## Per-target L0–L7

### Quest 1

- **L0:** two 1440×1600 OLED panels, SDC/AUO variants, nominally 72 Hz.
- **L1/L2 — VERIFIED [external]:** Meta's build-specific 4.4 source defines dual DSI,
  regulators/reset/TE, dual-controller sync, DSC and command tables
  ([external] [VS1 DT](https://github.com/facebookincubator/oculus-linux-kernel/blob/589280fc40ddbcc2287024c8b672568a0fdd68e7/arch/arm/boot/dts/qcom/vs1.dtsi#L525-L663),
  [SDC panel](https://github.com/facebookincubator/oculus-linux-kernel/blob/589280fc40ddbcc2287024c8b672568a0fdd68e7/arch/arm/boot/dts/qcom/dsi-panel-sdc-lightman-video.dtsi#L1-L82)).
- **L3:** AUO/SDC calibration blobs exist in an old dump, but schema, consumer and per-unit status
  are unknown.
- **L4–L7:** stock uses legacy FB/MDSS, not a qualified modern DRM connector; no KMS route,
  calibration consumer or native Monado frame. The source commit must not be treated as final-v50
  ABI evidence.

### Lynx R1

- **L0/L1:** dual 1600×1600 LCD, JDI/BOE R63455 variants, dual DSI/DSC, external backlight and
  boost. Stock logs bind JDI; firmware notes add BOE 90 Hz support
  ([external] [DT at exact dump commit](https://github.com/ellyq/lynx-mainline/blob/285eee1ad89f537a0f2afc34db4da8598729bdc2/dumps/lynx-dumped-dt.dts#L23659-L23739)).
- **L2:** pinned config enables MSM DRM/DPU/DSI but no matching R63455/JDI headset-panel driver
  (`references/pmaports/device/testing/linux-lynx-r1/config-lynx-r1.aarch64:4545-4727`).
  `deviceinfo_drm=true` is intent, not runtime proof.
- **L3:** per-unit `device_calibration.xml` and lens CSVs live under `/mnt/vendor/persist/qvr`;
  erase-all destroys them.
- **L4–L7:** no native connector/page flip, distortion consumer, Monado frame or P3/P6 result.

### Samsung Galaxy XR

- **L0:** dual Sony ECX344A 3552×3840 micro-OLED.
- **L1/L2:** two SDE DRM devices, one DSI connector and four 888-pixel SSPP slices per eye, with
  clock-ganged scanout (`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:22-61,460-498`).
- **L3:** per-unit EFS `display_profile_v2` is authoritative: per-color ray grids, transforms,
  FOV and calibration IPD. `/product` is another unit's devkit fallback
  (`galaxyxr_profile.c:5-31`; `galaxyxr_hmd.c:817-838`).
- **L4/L5:** atomic KMS with explicit fences; KWin/sddm grants one DRM lease per device and eye
  identity is bound to stable MDSS sysfs paths.
- **L6:** custom Monado target imports a 7104×3840 UBWC/linear image, scans four slices per eye and
  consumes the profile for distortion.
- **L7/runtime:** source is verified and 72/90 Hz on-glass behavior is community-reported; 60 Hz is
  broken. No pinned kernel/firmware/calibration hash bundle or Mura qualification exists, so state
  remains S1.

### Play For Dream MR

Vendor docs identify dual BOE 3840×3552 90-Hz micro-OLED and claim per-unit color/geometric
calibration. No public L1–L6 evidence exists: board DSI, panel driver, firmware, calibration
location/schema, DRM nodes and Monado route are unknown. Galaxy facts cannot transfer through
`anorak`.

### Valve Steam Frame

- **L0:** dual 2160×2160 LCD; public 72–120 Hz and experimental 144 Hz.
- **L1/L2:** MP donor contains SM8650 DPU, dual DSI/PHY, `deckard-panel`, per-eye MP3317
  backlights/rails, resets and preboost
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:2630-2792,4715-4982`).
  Kernel config enables MSM DRM/DSI and `CONFIG_DRM_PANEL_DECKARD`
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/config-6.18.0-deckard:4369-4458,4494-4499`).
- **L3:** panel source and distortion/calibration closure were not located; corresponding
  `linux-618-deckard` source is missing.
- **L4–L6:** stock SteamOS/SteamVR proves product operation, but no captured connector/mode/KMS
  trace, calibration consumer or Monado first frame exists.
- **L7:** exact static evidence is strongest after Galaxy, but incomplete L3/L5/L6 keeps A0.

### Meta Quest 3

- **L0:** dual 2064×2208 LCD at 72/90/120 Hz.
- **L1/L2 — VERIFIED stock [external]:** Android-14 donor DTS defines dual display clocks/SDE,
  BOE/JDI/Sharp panels, C-PHY/DSC, dynamic modes, backlight/TE/reset and temperature compensation
  ([external] [Eureka DTS](https://dumps.tadiphone.dev/dumps/oculus/eureka/-/blob/eureka-user-14-UP1A.231005.007.A1-50974260049300520-release-keys/vendor_boot/dts/01_dtbdump_Eureka.dts#L28733-L29192)).
- **L3:** demura/DDIC-CAC hooks exist; actual optical/demura calibration is absent from the public
  dump and likely per-unit.
- **L4–L7:** stock composer only; no safe native boot, DRM inventory, Monado HMD/distortion route
  or first frame.

## Artifact and calibration boundaries

Quest, Lynx, Galaxy and Quest 3 optical/panel calibration is per-unit protected state unless a
specific generic fallback is proven. Galaxy's EFS profile is atomic across display, IMU, camera
and optics and is indexed through doc 51 rather than copied here. Frame's built-in panel driver
source and optical calibration remain missing from a mixed-license `localOnly` donor. PFDM
calibration is vendor-claimed but unavailable. Panel supplier/revision always binds the evidence.

## Runtime evidence bundle

- **R1:** both panels bind; card/connector/CRTC/plane topology, brightness and qualified modes
  enumerate; calibration parses.
- **R2:** atomic test/real commits advance both-eye page-flip/vblank without underrun or drift.
- **R3:** left/right identity, orientation, supplier/revision, scan direction, timing, brightness,
  distortion/FOV/IPD and calibration binding are physically verified.
- **R4:** ordinary Monado service acquires direct/lease target and an OpenXR app reaches both eyes
  on glass with correct session ownership.
- **R5:** cold boot, suspend/resume, doff/panel-off, mode switching, revoke/reacquire, thermal load
  and recovery pass.

## Contract/preflight consequences

P2 must bind calibration to exact board/panel revision and reject unrelated fallback. P3 currently
refers to a contract-named connector although no connector-identity option exists. P6 first frame
needs a dedicated probe compositor/client and KMS/vblank progression for every eye; a single frame
alone is not readiness. Scalar `panel.refresh` cannot describe qualified mode sets. Galaxy's
dual-lease target also does not fit the current compositor-backend enum. These are open contract/
implementation questions, not option changes licensed by this audit.

## Contradictions / deciders

- Monterey 1440×1600 product nodes versus a 960×1600 common AUO node: retail boot/sysfs.
- Lynx JDI default versus BOE fleet and inconsistent rate fields: panel ID/runtime modes.
- Galaxy stock 60 Hz versus broken native 60 Hz: kernel/display update plus on-glass run.
- Frame public refresh set versus absent audited mode table: source or `modetest`.
- Eureka Sharp default versus BOE/JDI/Sharp fleet: runtime panel ID.
- P3 “named connector” versus absent option: first target spike/contract maintainer.
