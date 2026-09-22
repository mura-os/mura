# 00 — Synthesis: what the references tell us to build

**Date:** 2026-09-22. This document reads across the six sibling research docs
([01-mobile-nixos](01-mobile-nixos.md), [02-postmarketos](02-postmarketos.md),
[03-android-compat](03-android-compat.md), [04-nix-imaging](04-nix-imaging.md),
[05-xr-userspace](05-xr-userspace.md), [06-donor-pipeline](06-donor-pipeline.md),
[07-device-landscape](07-device-landscape.md)) and extracts the decisions the spatial-os
architecture must make. It is the bridge from research to `docs/architecture/`.

The reference projects were studied as *examples of how to target many mobile/XR devices*, not as
code to import. spatial-os imports none of them wholesale; it borrows patterns. Where a pattern
recurs across independent projects, that convergence is treated as strong evidence.

---

## 1. The one boundary every project draws

Every multi-device system studied — postmarketOS, Mobile NixOS, Sailfish/droid-hal, UBports/Halium,
Yocto/meta-qcom, even robotnix — draws the **same primary boundary**: a device-independent
distribution on one side, a per-device hardware-adaptation bundle on the other, joined by a
**typed device descriptor**.

| Project | Device-independent side | Per-device descriptor | Adaptation bundle |
|---|---|---|---|
| postmarketOS | Alpine userspace + UI metapackages | `deviceinfo` (typed TOML schema) | `device-*` + `linux-*` + `firmware-*` + `soc-qcom-*` |
| Mobile NixOS | NixOS + `mobile.*` modules | `devices/<oem>-<codename>/default.nix` | family + SoC module + kernel + firmware |
| Sailfish | Sailfish Core (RPM) | `droid-hal-<device>.spec` macros | droid-hal / droid-configs / droidmedia RPMs |
| UBports | Ubuntu Touch rootfs (downloaded) | `deviceinfo` (shell) | kernel + boot images + overlay tarball |
| Yocto/meta-qcom | distro layer + image recipes | `conf/machine/<board>.conf` | SoC `.inc` + kernel + firmware + boot recipes |
| robotnix | AOSP + module system | `device` string + flavor JSON | device source repos + vendor blobs |

The lesson is unambiguous: **spatial-os's central artifact is a typed device contract**, and its
central discipline is keeping the common distribution genuinely common. This is confirmed by the
smallest device files in the corpus — a 25-line Mobile NixOS `oneplus-enchilada`
([01](01-mobile-nixos.md) §3.2), a 3-package pmaports Lynx R1 port ([02](02-postmarketos.md) §3.3),
a 17-line droid-hal spec template ([03](03-android-compat.md) §3.3). When the abstraction is right,
adding a device is mostly data.

The corollary, equally strong: the three-layer decomposition **device → SoC-family → vendor/common**
appears independently in pmaports (`device-* → soc-qcom-sdm845 → soc-qcom`), meta-qcom
(`board.conf → qcom-sm8250.inc → qcom-common.inc`), and Mobile NixOS (`device → family → SoC module`).
spatial-os should adopt it directly.

---

## 2. Where the projects disagree, and how spatial-os should decide

### 2.1 Build engine: imperative orchestration vs. pure derivations

postmarketOS (`pmbootstrap`, imperative chroots + QEMU) and Sailfish (Android build inside an RPM
inside an OBS chroot) are the imperative extreme; robotnix and Mobile NixOS are the Nix extreme.
The Nix projects win decisively on the property spatial-os cares most about — reproducibility — and
the imperative projects' own maintainers treat their statefulness as a liability (`pmbootstrap zap`
exists because chroots rot, [02](02-postmarketos.md) §8).

**Decision:** Nix derivations for everything, no imperative chroot lifecycle. But keep two things
from the imperative world: pmbootstrap's *decision enum* for cross-compilation strategy per package
(evidence that one global strategy doesn't fit all, [02](02-postmarketos.md) §6) and its
`bootimg_analyze` donor-introspection idea ([02](02-postmarketos.md) §6, generate the descriptor
*from* the donor). Where a foreign build system is unavoidable (AOSP for the bionic half of
libhybris compat layers), wrap it with robotnix's recipe — `unshare -m -r` + `mount --bind`
tree assembly + `buildFHSEnv` + `fakeuser` ([04](04-nix-imaging.md) §9 item 9) — rather than
rewriting it.

### 2.2 NixOS-as-runtime vs. Nix-as-builder-only

This is the sharpest tension in the corpus, and the device landscape forces it into the open.
Mobile NixOS ships NixOS; the brick appliance deliberately did *not* (it built BusyBox+musl init
with Nix because the vendor kernel was Linux 4.9 and current systemd needs ≥5.10). The Halium
research shows the same kernel-version cliff from the other side: systemd ≥217 needs ≥3.10, and
Ubuntu Touch's apparmor kernel patches propagate all the way into kconfig ([03](03-android-compat.md)
§5.2, §10.2).

The device landscape ([07](07-device-landscape.md)) resolves most of this favorably: every priority
target has a **mainline-capable path**. Quest 1 (MSM8998) and Lynx R1 (SM8250) already have
postmarketOS mainline kernels; Steam Frame (SM8650) runs mainline-ish SteamOS today; Galaxy XR / PFDM
(XR2+ Gen 2, ~SM8550) have strong generic SM8550 upstream foundations. So the *default* can be a
current NixOS userspace on a mainline-or-near-mainline kernel.

**Decision:** NixOS is the default runtime. But the architecture must treat "NixOS userspace" and
"kernel meets a version/feature contract" as **separately verifiable**, with a kconfig-style contract
gate (§3.3 below) as the enforcement mechanism, and a documented (not yet built) escape hatch for a
Nix-built non-NixOS userspace should a strategically important device be stuck on an ancient vendor
kernel. Freezing the whole distro on an old nixpkgs for one headset is explicitly rejected.

### 2.3 Android compatibility: foundation vs. optional per-subsystem backend

Halium/UBports/Sailfish make Android a *boot dependency* (rootfs mounts `/system` + `/vendor` before
`local-fs.target`, Android `init` runs as PID 1 of a mandatory LXC container —
[03](03-android-compat.md) §6.1). Waydroid inverts it (Android in a late-starting container on a
mainline host). The XR device landscape adds a decisive complication: **DSP-based tracking has no
compatibility precedent anywhere** ([03](03-android-compat.md) §9.6, §11 item 1), and libhybris has
no hwcomposer Vulkan WSI and no linker newer than Android 10 ([03](03-android-compat.md) §11 items 2, 4).

**Decision:** Android compat is an **optional, per-subsystem, late-starting backend**, never the
foundation. Each subsystem (display/GPU, camera, sensors/IMU, audio, Wi-Fi/BT, DSP tracking)
independently selects `native` | `android-backed` | `device-specific`. Given the mainline kernel
paths above, the *default* posture is native (Mesa/Freedreno, V4L2, IIO, PipeWire, mac80211), with
Android-backed as a fallback per subsystem where a mainline driver doesn't yet exist. This matches
the direction the whole ecosystem is moving and sidesteps the unproven libhybris paths. The Android
compat subsystem is designed in but proven last, on one device, per subsystem.

### 2.4 Image assembly: what to build on

nixos-generators is deprecated ([04](04-nix-imaging.md) §2.2); its one durable idea (`extendModules`
fan-out) now lives in nixpkgs `image.modules` / `system.build.images`. Mobile NixOS's bespoke
image-builder modules system is capable but carries an empirical ext4 "fudge factor" table and a
2020-pinned mkbootimg that emits only legacy boot headers ([01](01-mobile-nixos.md) §6.1, §6.3) —
exactly the wrong starting point for XR2 header-v3/v4 + `vendor_boot` donors. systemd-repart
(rootless, VM-less, cross-clean, deterministic UUIDs) is the modern consensus for GPT/disk images
([04](04-nix-imaging.md) §6.2).

**Decision:** build `lib/images/` on nixpkgs `image.modules` + systemd-repart for
disk/UEFI/RAUC-class targets. For Android boot chains, write a Nix-native declarative wrapper over
AOSP's *current* `mkbootimg.py`/`unpack_bootimg.py`/`avbtool`/`lpmake` (all in nixpkgs
`android-tools`), driven by header-version and offset data extracted from the donor — never the
2020 mkbootimg fork. UBports' `make-bootimage.sh` ([03](03-android-compat.md) §6.2) is the
functional spec for header v0–v4 + `vendor_boot` + `init_boot` + AVB behavior.

### 2.5 Updates: one plane or two

The device landscape splits cleanly. Steam Frame already uses RAUC + casync A/B
([06](06-donor-pipeline.md) §2.2, [07](07-device-landscape.md)); the donor-pipeline doc shows all
the tooling is in nixpkgs and recommends RAUC+casync for spatial-os's own updates. Android-boot
devices need boot/dtbo/vbmeta/super slot management that a generic partition writer can't safely do
([02](02-postmarketos.md) §11 item 1, [04](04-nix-imaging.md) §7).

**Decision:** two update backends behind one transactional interface. RAUC+casync A/B for
UEFI/SteamOS-class devices; an Android-slot backend (A/B where present, droid-hal's "flash on
upgrade via pre-init oneshot" semantics where not — [03](03-android-compat.md) §7) for Android-boot
devices. Mark an update successful only after a hardware-aware health check reaches XR readiness,
not merely "kernel booted."

---

## 3. Patterns to adopt verbatim (convergent across projects)

These recurred independently and should be treated as settled.

### 3.1 Typed device descriptor with a deprecation lifecycle
pmaports moved from stringly-typed shell `deviceinfo` to a typed TOML schema with datatypes, enums,
mandatory flags, `fate`/`epitaph` deprecation, and first-class renames ([02](02-postmarketos.md) §3.1);
UBports' `deviceinfo.sample` is the most complete Android-donor field list
([03](03-android-compat.md) §3.2, §9.1). In Nix this is a NixOS-module option set with
`mkRenamedOptionModule`. Keep the mandatory minimum tiny (pmOS requires 6 fields) so a bring-up port
is a 30-line file. **Start schema-first; never have an untyped intermediate format.**

### 3.2 Hash-pinned donor inputs with provenance, blobs as data not code
robotnix (`fetchurl` + committed per-device JSON hashes for public donors), the brick appliance
(`requireFile` + instructive message for non-redistributable donors), nixos-apple-silicon
(on-device firmware extraction with an explicit path option + pure escape hatch), and Waydroid
(sha256-against-manifest) all converge ([04](04-nix-imaging.md) §4, [06](06-donor-pipeline.md) §2).
Adopt `fetchurl` for public (Lynx portal, Steam Frame, Google-style), `requireFile` for
authenticated/mirror/device-dumped (Samsung FUS, Meta archives, PFDM captures), on-device extraction
for per-unit firmware. Every derived artifact records which donor it came from.

### 3.3 Kernel: literal `.config` as source of truth + a validated contract
Mobile NixOS ships a literal vendor `.config` and uses structured config as a *version-aware
validator* emitted as a standalone shell script ([01](01-mobile-nixos.md) §5); pmaports enforces a
`kconfigcheck.toml` contract as a CI merge gate per tier ([02](02-postmarketos.md) §5); Halium ships
an authoritative ~190-symbol config requirement list ([03](03-android-compat.md) §5.1). Synthesis:
per-device kernels are standalone `buildLinux` derivations (Jovian/Asahi style: vendor tag +
`structuredExtraConfig` with per-option provenance comments — [04](04-nix-imaging.md) §5), and a
**kconfig contract** — composed per subsystem-backend and per tier — is checked against the built
`.config`, distinguishing errors from warnings, stripping toolchain-derived symbols before diffing.
This contract is also the §2.2 kernel gate.

### 3.4 Signing outside the store, test keys inside
robotnix ([04](04-nix-imaging.md) §7, §9 item 8): every in-store artifact signed with committed test
keys (fully cacheable); a generated `releaseScript` re-signs final artifacts against `$KEYSDIR` or a
PKCS#11 token. **Key rotation must never trigger a rebuild.** Adopt directly.

### 3.5 Donor metadata mechanically generates adaptation
droid-hal's three generators — `ueventd.rc`→udev, Android fstab→systemd mount units, Android ids→group
policy ([03](03-android-compat.md) §9.5) — plus UBports' `unpack_bootimg`-derived offsets
([03](03-android-compat.md) §9.1) and pmOS's `bootimg_analyze` ([02](02-postmarketos.md) §6) all say
the same thing: **derive the device descriptor and adaptation glue from the pinned donor, don't
hand-transcribe them.** This removes the two most error-prone manual steps in every Halium port.

### 3.6 Reference-free flashing bundle + build/flash separation
Mobile NixOS's Android output is a deliberately reference-free directory of `.img` + a script using
bare tool names, tar-able and flashable from a non-Nix laptop ([01](01-mobile-nixos.md) §6.3). The
brick appliance's tools *refuse to write to block devices* — flashing is a separate human act
([06](06-donor-pipeline.md) §2.1.5). meta-qcom ships a full "factory restore bundle"
([02](02-postmarketos.md) §9 item 11). **`nix build` must never flash a device.** Adopt all three.

### 3.7 Support tiers encoded in-tree and CI-enforced
postmarketOS's directory-as-tier with CI gates ([02](02-postmarketos.md) §3.6) and Mobile NixOS's
`supportLevel` enum surfaced in generated docs ([01](01-mobile-nixos.md) §9 item 11). spatial-os
needs booting / XR-functional / release-supported tiers gating which checks are mandatory, with
tier requirements encoded as evaluatable checks (fixing pmOS's wiki-only requirements gap).

---

## 4. Anti-patterns to avoid (learned from the corpus)

- **Legacy-only Android boot assembly** — Mobile NixOS's 2020 mkbootimg, no `--header_version`, no
  `vendor_boot` ([01](01-mobile-nixos.md) §6.3, §10 item 3). Every XR2 donor is v3/v4 + `vendor_boot`.
- **Floating refs / CI-artifact URLs / mutable tags** — pervasive in Halium/UBports build tools
  (`lastSuccessfulBuild`, `continuous` tags, branch clones — [03](03-android-compat.md) §8). Pin
  everything to commits+hashes.
- **The mega-derivation as a general pattern** — robotnix's 10-hour rebuild-the-world on any 1-of-1000
  source change ([04](04-nix-imaging.md) §10 item 1). Keep package/kernel/rootfs/partition/image as
  separate derivations with clean cache boundaries; accept a monolith only where a foreign build
  system leaves no choice, and carve out sub-products.
- **Runtime probing as the source of configuration** — Waydroid computes its whole config on the live
  host ([03](03-android-compat.md) §10.4). Keep the *knowledge* (its mainline-fallback table is
  excellent), move the *evaluation* to build time driven by the donor + device module.
- **Config smuggled through `nixpkgs.overlays`** — Mobile NixOS defeats pkgs sharing across devices
  ([01](01-mobile-nixos.md) §10 item 2). Pass config explicitly via `callPackage`.
- **Bespoke stage-1 in a custom language** — Mobile NixOS's 70-file mruby init
  ([01](01-mobile-nixos.md) §10 item 1). Use `boot.initrd.systemd`.
- **Build-at-runtime XR stacks** — Envision clones branch heads into `$HOME`
  ([05](05-xr-userspace.md) §10). Keep its profile *schema*, reject its mechanism.
- **Manual hash-bump maintenance at fleet scale** — Jovian's constants-in-file is fine for one
  device; with 6+ headsets × {kernel, firmware, blobs} it drowns you. Automate with a lockfile +
  scheduled bump PRs (nixpkgs-xr's nvfetcher cron is the template — [05](05-xr-userspace.md) §2.4).
- **Re-compression in the donor pipeline; mtools FAT writes** — nondeterministic
  ([06](06-donor-pipeline.md) §6).
- **Pushing donor-containing outputs to public caches** — a redistribution event; a
  `redistributable=false` flag must mechanically force `allowSubstitutes=false`
  ([06](06-donor-pipeline.md) §6, §2.1.3).

---

## 5. XR-specific findings that bound the design

The XR layer is the point of the project, but the research confirms it should sit *above* the device
boundary and be kept out of the build-system weeds ([05](05-xr-userspace.md) framing).

- **Monado has no stable out-of-tree driver ABI.** Drivers are compiled into a target via a static
  list; the realistic device-port mechanism is a pinned `monado-rev` + a small patch series, exactly
  as WiVRn does with 11 patches ([05](05-xr-userspace.md) §3, §9 item 2). A device's XR driver is
  therefore part of its adaptation bundle, expressed as a Monado pin+patches, not a plugin.
- **Runtime = Monado, out-of-process, socket-activated**, with `/etc/xdg/openxr/1/active_runtime.json`
  declared by the module system, never symlink-flipped at runtime ([05](05-xr-userspace.md) §9 item 1).
  The service↔client IPC has no ABI guarantee, so the whole closure must update atomically — which
  Nix does for free ([05](05-xr-userspace.md) §7).
- **nixpkgs-xr is the update architecture to reuse** (nvfetcher pins incl. the WiVRn→Monado cross-pin,
  daily cron, cachix) — pull it in as a flake input ([05](05-xr-userspace.md) §9 item 3).
- **Display path is the first feasibility test.** `comp_window_vk_display` (VK_KHR_display, Monado
  owns DRM directly) is the natural appliance choice but must be proven on the target SoC's Vulkan
  driver; the fallback is a minimal DRM-lease Wayland compositor under Monado
  ([05](05-xr-userspace.md) §5, §11 item 1). This is the decisive early experiment.
- **`modules/xr/` option surface** mirrors Monado's CMake flags: runtime selection, per-driver
  `XRT_BUILD_DRIVER_*`, compositor backend, systemd env-var config, udev rules, SLAM/Basalt plugin,
  hand-tracking-model data path, calibration paths ([05](05-xr-userspace.md) §9 item 4).
- StardustXR is the shell layer (an OpenXR client), but note the current tree has no in-tree Wayland
  compositor, so the 2D-app-in-headset story is an open packaging question ([05](05-xr-userspace.md)
  §11 item 2).

---

## 6. The device landscape's hard constraints on the architecture

From [07](07-device-landscape.md), the facts that the architecture must accommodate:

- **All Qualcomm, three kernel eras:** MSM8998 (Quest 1) and SM8250 (Lynx R1) share mature linux-msm
  mainline; SM8550-class XR2+ Gen 2 (Galaxy XR, PFDM) needs newer GKI/mainline; SM8650 (Steam Frame)
  tracks modern upstream. The SoC-family layer must span all three.
- **Two image families minimum:** Android boot-chain (Quest/Lynx/Galaxy XR/PFDM — boot.img packing
  per verified header version, AVB policy, A/B, `vendor_boot`/dynamic-partition) and UEFI/RAUC
  (Steam Frame).
- **Donor acquisition differs per vendor:** Lynx = official versioned ZIP (cleanest); Steam Frame =
  official RAUC/casync (also clean, publicly reconstructable); Samsung = FUS with launch-firmware
  pinning; Meta = latest-only official + user-supplied historical archives; PFDM = capture from owned
  device only. The donor manifest schema ([06](06-donor-pipeline.md) §5.6) must express all five.
- **Unlock preservation is a real, per-device constraint:** some updates patch unlocks (Galaxy XR
  Dec-2025, PFDM OTA-burns-fuse risk, Quest patched builds). The device contract must pin
  known-good donor build IDs and the installer must verify prerequisite state — building correct
  images never bypasses bootloader/AVB policy.
- **Per-unit calibration is sacred:** every device has factory calibration (Lynx `persist/qvr` CSVs,
  the 11-camera PFDM array, Quest `persist/vision`) that must be preserved as per-unit state and
  never copied between units. The flash plan must protect these partitions.
- **Read-only donor collection where recovery is unsafe** (Quest 3): a hard build-system safety
  constraint, matching the brick appliance's refuse-to-write-block-devices posture.

---

## 7. Consolidated decisions handed to the architecture

1. **Nix builds everything; NixOS is the default runtime**, with a separately-verified kernel
   contract and a documented non-NixOS-userspace escape hatch (§2.1, §2.2).
2. **Typed device contract as the central artifact**, three-layer device→SoC-family→common,
   schema-first, generated from the donor where possible (§1, §3.1, §3.5).
3. **Per-subsystem adaptation backend** (`native` | `android-backed` | `device-specific`), native by
   default, Android-compat optional and late-starting (§2.3).
4. **Donor pipeline** = acquire→identify→parse→extract→qualify, hash-pinned, blobs-as-data,
   contract-gated, never flashes ([06](06-donor-pipeline.md) §5; §3.2, §3.6).
5. **Images** on nixpkgs `image.modules` + systemd-repart, plus a modern Android-boot packer;
   **updates** via two backends (RAUC+casync / Android-slot) behind one transactional interface,
   gated on XR-readiness health checks (§2.4, §2.5).
6. **XR layer above the boundary:** Monado out-of-process runtime pinned via nixpkgs-xr, per-device
   driver as monado-rev+patches, `modules/xr/` option surface, display path as the first feasibility
   experiment (§5).
7. **Reproducibility as engineered evidence, not assumed:** pinned graph + deterministic assembly +
   independent-rebuild byte comparison; signing outside the store; donor-containing outputs off
   public caches (§3.4, §4).
8. **Support tiers** (booting / XR-functional / release-supported) encoded in-tree and CI-enforced
   (§3.7).
9. **Implementation order:** prove the two critical runtime paths first (common shell + OpenXR on a
   native reference platform; full graphics+tracking on one strategically chosen Android headset —
   Lynx R1 is the strongest candidate given its open bootloader, official firmware, and existing
   pmaports mainline port), make that port reproducible, then add a genuinely different device
   before extracting shared families.
