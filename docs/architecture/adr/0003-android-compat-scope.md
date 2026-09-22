# ADR 0003: Android compatibility is an optional, per-subsystem, late-starting backend

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [03-android-compat](../../research/03-android-compat.md) (whole doc),
[00-synthesis](../../research/00-synthesis.md) §2.3.

## Context

Most targets ship Android firmware whose proprietary HALs (GPU, camera, sensors, DSP tracking) might
need an Android-compatibility layer. Halium/UBports/Sailfish make Android a **boot dependency** (rootfs
mounts `/system`+`/vendor` before `local-fs.target`; Android `init` is PID 1 of a mandatory LXC
container — [03](../../research/03-android-compat.md) §6.1). Waydroid inverts it (Android in a
late-starting container on a mainline host). The XR-specific complications are decisive:
DSP-based tracking has **no compatibility precedent anywhere** in the seven projects studied
([03](../../research/03-android-compat.md) §9.6, §11 item 1); libhybris has **no hwcomposer Vulkan
WSI** and **no linker newer than Android 10** ([03](../../research/03-android-compat.md) §11 items 2, 4),
while XR2 donors ship Android 12–14.

## Decision

Android compatibility is an **optional, per-subsystem, late-starting backend** — never the
foundation. Each subsystem (display/GPU, camera, sensors/IMU, audio, Wi-Fi/BT, DSP tracking)
independently selects `native` | `android-backed` | `device-specific` (see
[device-contract.md](../device-contract.md) §adaptation). Given the mainline kernel paths in the
device landscape, the **default posture is native** (Mesa/Freedreno, V4L2, IIO, PipeWire,
mac80211/BlueZ). Where an `android-backed` subsystem is selected:

- it pulls a **per-subsystem blob closure** from the donor (the HAL `.so` + `NEEDED` closure +
  `.rc`/VINTF manifests + ueventd entries), not the whole `/system` (rejecting droid-hal's
  undifferentiated monolith — [03](../../research/03-android-compat.md) §10.5);
- it builds the matching libhybris variant via an `android-headers-<gen>-<device>` derivation +
  `android-config.h` defines ([03](../../research/03-android-compat.md) §9.2);
- it runs as a **late, optional systemd unit** (LXC container or `libgbinder` bridge), **never a
  `local-fs.target` prerequisite** ([03](../../research/03-android-compat.md) §10.2).

Donor-derived udev rules, mount units, and group policy are **generated mechanically** from the
pinned donor (droid-hal's three generators — [03](../../research/03-android-compat.md) §9.5), not
hand-transcribed.

## Rationale

- The whole ecosystem is moving toward native; the mainline paths for MSM8998/SM8250/SM8550/SM8650
  make native the realistic default and sidestep libhybris's unproven Vulkan/hwcomposer and
  Android-12+ linker gaps.
- Per-subsystem selection lets proprietary dependencies be replaced incrementally without
  redesigning the distribution, and confines the one genuinely hard case (DSP tracking) to a
  `device-specific` backend proven per device.
- Keeping compat late and optional means a compat failure degrades one subsystem, not the boot.

## Consequences

- The Android-compat building blocks (`modules/adaptation/android-compat/`) are designed in but
  proven **last**, on one device, per subsystem — after the native reference platform and the first
  Android-headset graphics+tracking spike.
- DSP-based tracking is flagged as the single highest-risk feasibility question and needs a hardware
  spike before any architecture commitment ([03](../../research/03-android-compat.md) §11 item 1).
- The bionic half of any libhybris compat layer is the one place an AOSP build is unavoidable; it is
  scoped to exactly the layers a chosen backend needs (see [adr/0004](0004-cross-vs-native-builds.md)
  for how that build runs).

## Alternatives considered

- **Halium-style Android-as-foundation:** rejected; makes Android a boot dependency, drags apparmor
  kernel policy into kconfig, and still doesn't solve DSP tracking.
- **Global "this device needs libhybris" flag:** rejected; too coarse — a device may need native
  networking, android-backed audio, and device-specific tracking simultaneously.
