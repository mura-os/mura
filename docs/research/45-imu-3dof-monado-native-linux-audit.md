# 45 — IMU/3DoF to Monado native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
**Boundary:** headset orientation only; SLAM/VIO remains in [20](20-slam-stacks-for-xr.md),
buttons in [42](42-input-bootstrap.md).

## Verdict

| Device/profile | Native chain | State |
|---|---|---|
| Quest 1 / `monterey:base` | downstream SyncBoss blueprint; firmware/packet/calibration/Monado joins absent | A0 |
| Lynx R1 / `lynx-r1:base` | stock SLPI/SSC; mainline remoteproc only, no sensor endpoint | A0 |
| Galaxy XR / `sm-i610:base` | SSC→QRTR/QMI→calibration→Monado source verified; runtime reported | S1; R1–R4 community-reported |
| Play For Dream / `pfdm-mr:base` | physical accel/gyro only | A0 |
| Steam Frame / `deckard:base` | stock IMU reported; no located transport or open consumer | A0 |
| Quest 3 / `eureka:base` | stock SyncBoss/Android path; no board source/native driver | A0 |

No accessory/mod profile meets doc 44's domain inclusion bar. Arcturus changes cameras, not IMU.
Galaxy is not formally R4: the matching downstream kernel/SSC firmware hashes, licenses and Mura
packaging are not pinned.

## Shared endpoint

Monado's stable driver-side sample is `xrt_imu_sample { timestamp_ns, accel_m_s2,
gyro_rad_secs }` (`references/monado/src/xrt/include/xrt/xrt_tracking.h:138-142`), consumed by
`m_imu_3dof_update` (`references/monado/src/xrt/auxiliary/math/m_imu_3dof.c:230`). Android, WMR,
Vive and Rift drivers prove orientation-only fallback mechanics; none is a target driver. A search
of pinned Monado found no IIO adapter and no target-native driver other than the Galaxy fork.

## Per-target L0–L7

### Quest 1

- **L0:** camera+IMU fusion is vendor-documented; exact IMU chip is **UNKNOWN**.
- **L1/L2 — VERIFIED [external]:** `oculus,syncboss` uses SPI12 at 10 MHz, reset/SWD/wakeup
  GPIOs and a 1.808 V rail; Meta's downstream misc driver transports MCU packets
  ([external] [DT](https://github.com/facebookincubator/oculus-linux-kernel/blob/589280fc40ddbcc2287024c8b672568a0fdd68e7/arch/arm/boot/dts/qcom/vs1.dtsi#L1010-L1042),
  [driver](https://github.com/facebookincubator/oculus-linux-kernel/blob/589280fc40ddbcc2287024c8b672568a0fdd68e7/drivers/misc/oculus/syncboss.c#L126-L148)).
- **L3:** final-v50 MCU firmware, calibration schema, hashes and license are unknown; the source
  commit belongs to build `333700.3780.0`.
- **L4:** downstream misc FIFOs include `/dev/syncboss_stream0`, but public source does not close
  HMD packet semantics/timestamps.
- **L5–L7:** stock HAL permissions, decoder, Monado driver, calibration declaration and
  transport-specific readiness probe are absent. Backend must be `device-specific`, not assumed
  IIO/native.

### Lynx R1

- **L0:** factory IMU/gyro calibration is vendor-documented; exact production chip remains unknown.
- **L1/L2:** stock uses SLPI/SSC, GLINK and QRTR. The pinned kernel enables QRTR/Q6V5 PAS
  (`references/pmaports/device/testing/linux-lynx-r1/config-lynx-r1.aarch64:1598-1601,6475-6495`)
  but disables `QCOM_SSC_BLOCK_BUS` and ICM42600 IIO (`:1935,7037-7038`).
- **L3:** proprietary `slpi.mbn` is pinned by
  `references/pmaports/device/testing/firmware-lynx-r1/APKBUILD:15-22,82-87`; exact calibration
  record/ABI is uninspected.
- **L4–L7:** no verified service-400 SUID, native IIO ABI, daemon, Monado driver or readiness probe.
  The public ORB-SLAM demo explicitly runs without IMU
  ([external] [LynxOrbSlam3](https://github.com/Lynx-MR/LynxOrbSlam3)).

### Samsung Galaxy XR

- **L0:** Samsung specifies five IMUs; the fork sees five ST LSM6DSV accel/gyro instances and uses
  `hw_id == 0` (`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:65-75`).
- **L1/L2:** userspace discovers SSC QMI service 400 over `AF_QIPCRTR`, resolves SUIDs/attributes
  and enables streams (`galaxyxr_ssc.c:20-76,240-519,695-779`). Physical buses remain hidden.
- **L3:** per-unit EFS `imus{id:"0"}` supplies scale/bias; SSC `gyro_cal` supplies live bias.
  Another unit's `/product` fallback is rejected (`README.md:80-99,255-319`).
- **L4:** QTimer ticks at 19.2 MHz become nanoseconds by `ticks * 625 / 12`; samples already use
  SI units and are tracked against `CLOCK_MONOTONIC`.
- **L5/L6:** the fork fuses x-io AHRS and serves orientation/angular velocity as an orientation-only
  HMD (`galaxyxr_hmd.c:283-308,630-699,950-953`).
- **L7:** no Mura device, firmware/source pin, service permission, calibration path or preflight
  result. Treat runtime success as reported reference evidence, not qualification.

### Play For Dream MR

Vendor docs list accelerometer/gyro; every L1–L6 join—chip, bus, DT, SSC/IIO, firmware,
calibration, timestamp domain and Monado driver—is unknown. Shared `anorak` strings transfer
nothing from Galaxy. Owned-device read-only capture is the S1 prerequisite.

### Valve Steam Frame

The core-module IMU is community-reported, but the inspected MP DT has no located HMD IMU/SSC
route and common candidate IIO drivers are disabled
(`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/config-6.18.0-deckard:2032,6395-6405,6682-6751`).
This is artifact-scoped negative evidence, not proof that no route exists. Tracking firmware,
calibration, device ABI and userspace consumer likely sit outside the reconstructed rootfs, but
remain unknown. `mura.adaptation.sensors.backend = "native"` in the current Frame declaration is
not qualification evidence.

### Meta Quest 3

A community SyncBoss log names ICM45688, but Meta artifacts do not corroborate it
([external] [WiVRn issue 598](https://github.com/WiVRn/WiVRn/issues/598)). Stock Android exposes fused/high-rate
sensor data; no Eureka board DT/source, SyncBoss firmware/packet ABI, calibration closure, safe
native boot or Monado driver exists.

## Artifact and calibration boundaries

Meta and PFDM MCU/sensor firmware and calibration remain proprietary/unavailable. Lynx SLPI is a
pinned proprietary donor input; its per-unit calibration must be preserved. Galaxy's Monado code
is inspectable, but matching Samsung kernel/SSC firmware and EFS per-unit calibration remain one
versioned local closure. Frame tracking state outside the RAUC donor is unknown. No calibration
record may be copied between units.

## Runtime evidence bundle

- **R1:** exact IIO channels, SSC service/SUID/instance, SyncBoss nodes/firmware ABI or HID report
  enumerates; Monado creates orientation-only HMD.
- **R2:** hardware timestamps and gyro samples advance under known rotation without loss/reset;
  capture-clock conversion is measured.
- **R3:** axes/handedness, SI units, rate, saturation, noise/bias/temperature, gravity and
  per-unit calibration/extrinsics are physically verified exactly once.
- **R4:** greeter user starts Monado without root; cameras stay off; head aim works; greeter/session
  handoff reacquires transport; ordinary `xrLocateViews` returns predicted orientation.
- **R5:** cold boot, service restart, suspend/resume, user switch, XR load, DSP/MCU crash, clock
  restart and calibration corruption recover safely.

## Contract and preflight consequences

F4 is a native-driver requirement, not a physical/stock-hardware fact. B1b P5 must be
transport-aware—an IIO device, QRTR service+SUID, SyncBoss ABI or HID report—and P6/readiness must
also prove advancing samples and a Monado pose. Galaxy's open QRTR client is a
`device-specific` native GNU/Linux implementation, not an Android-backed path. No new contract
option follows from this audit.

## Contradictions / deciders

- Frame `sensors.backend=native` versus no identified producer: retail node/service inventory.
- Lynx ICM4X6XX/BMM150 claim versus absent public artifact strings: stock sensorservice/SSC dump.
- Galaxy “ADSP sensor core” nomenclature versus SSC/SLPI terminology: matching Samsung DT.
- Universal physical head aim versus absent native target drivers: per-target R4.
- Quest 1 ICM-20689 web claims: board marking or trusted SyncBoss log.
