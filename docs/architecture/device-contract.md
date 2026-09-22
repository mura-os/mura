# spatial-os architecture: the device contract

**Status:** draft. Derived from [00-synthesis](../research/00-synthesis.md) §3.1, and the deviceinfo
schemas surveyed in [02-postmarketos](../research/02-postmarketos.md) §3.1 and
[03-android-compat](../research/03-android-compat.md) §3.2/§9.1.

The device contract is spatial-os's central artifact. It is a **typed NixOS-module option set** that
a device declares; evaluation turns it into the acquire/parse/extract derivation graph, the kernel
build, the adaptation bundle, the image variants, and the qualification checks. Typed options give
type-checking, defaults, documentation, and `mkRenamedOptionModule`-based deprecation for free, and
let assertions reject inconsistent combinations before anything builds.

## Design principles

1. **Schema-first, never a stringly-typed intermediate.** pmOS had to bolt a TOML schema onto its
   shell `deviceinfo` after the fact ([02](../research/02-postmarketos.md) §10 item 4). We start
   with typed options.
2. **Tiny mandatory minimum.** Like pmOS's 6 required fields, a bring-up device should be a ~30-line
   file. Everything else has a default or is derived from the donor.
3. **Declare vs. implement.** A device *declares* facts (identity, geometry, donor, backends) and
   *selects* implementations; it does not *implement* services. Executable logic lives in the
   SoC-family and common modules. The proof this works: 25-line Mobile NixOS devices.
4. **Three layers.** `device` imports a `family`, which sets a `soc`, which pulls `common`. Values
   flow device → family → SoC → common with normal module merging (no Yocto-style override games).
5. **Generated from the donor where possible.** Boot-image header version, flash offsets, partition
   layout, udev rules, and mount units are derivable from the pinned donor
   ([00](../research/00-synthesis.md) §3.5) — the contract records them, tooling drafts them.
6. **Typed assertions reject impossible combinations** before building. Prefer a small set of
   explicitly supported combinations over a flexible matrix.

## Option namespace

Everything lives under `spatial.*`. The option groups below are the contract; each maps to a module
under `modules/` or `devices/`.

### `spatial.device.*` — identity and support (mandatory minimum)

| Option | Type | Notes |
|---|---|---|
| `spatial.device.codename` | str | e.g. `lynx-r1`; directory name, searchable |
| `spatial.device.vendor` | str | e.g. `lynx` |
| `spatial.device.name` | str | human-readable, e.g. "Lynx R1" |
| `spatial.device.arch` | enum `aarch64`\|`x86_64` | build/host platform |
| `spatial.device.supportTier` | enum `booting`\|`xr-functional`\|`release-supported` | gates mandatory checks (see §Qualification) |
| `spatial.device.maintainers` | listOf str | empty ⇒ cannot exceed `booting` tier |
| `spatial.device.skuConstraints` | attrs | hardware revision constraints this port is valid for (defaults to "all revisions") |

Only `codename`, `vendor`, and `name` have no default and are strictly mandatory; `arch`,
`supportTier`, `maintainers`, and `skuConstraints` have defaults (`aarch64`, `booting`, `[]`, "all
revisions"). So a bring-up device sets ~3–5 fields. Everything below has defaults, is set by the
family, or is derived from the donor. (The minimal example at the end of this document omits
`skuConstraints` for exactly this reason.)

### `spatial.soc.*` and families

`spatial.soc` is an enum (`msm8998`, `sm8250`, `sm8550`, `sm8650`, …) set by the family, not the
device. The SoC module provides the shared kernel base, firmware search paths, the DSP/sensor
userspace stack, A/B slot ack, and default kconfig fragments — mirroring pmOS `soc-qcom-<family>`
and meta-qcom `qcom-<soc>.inc`. Families (`families/<name>/`) are plain module imports for
near-identical models; per Mobile NixOS guidance, implement real devices first and extract families
later rather than designing a deep hierarchy up front.

### `spatial.donor.*` — the donor manifest

The full schema is specified in [donor-pipeline.md](donor-pipeline.md) §manifest. From the device's
perspective it declares: accepted firmware build IDs and their hashes, the acquisition method
(`fetchurl` / `requireFile` / on-device), container type and partition list, extraction allowlists,
and licensing/redistribution flags. Known-good build IDs matter for unlock preservation
([07](../research/07-device-landscape.md)): e.g. Galaxy XR launch firmware before the Dec-2025
unlock-removing update.

### `spatial.kernel.*` — kernel build + contract

| Option | Type | Notes |
|---|---|---|
| `spatial.kernel.source` | pinned src | vendor tag or mainline rev+hash |
| `spatial.kernel.structuredExtraConfig` | attrs | with per-option provenance comments (Jovian style) |
| `spatial.kernel.configFile` | path | literal `.config` as source of truth (Mobile NixOS style) |
| `spatial.kernel.dtbs` | listOf str | DTB name templates, resolved per device |
| `spatial.kernel.bootimg.headerVersion` | enum 0–4 | derived from donor `unpack_bootimg`; **no legacy-only assumption** |
| `spatial.kernel.bootimg.offsets` | attrs | base/kernel/ramdisk/second/tags/dtb; from donor |
| `spatial.kernel.bootimg.hasVendorBoot` / `hasInitBoot` / `hasDtbo` | bool | from donor |
| `spatial.kernel.contract` | enum alias set | which kconfig contract categories apply |

The kernel is a standalone `buildLinux` derivation. `spatial.kernel.contract` composes named
categories (container/systemd prerequisites, per-subsystem-backend prerequisites, distro security
policy, XR requirements). It is checked in **two phases**, and **import-from-derivation is
forbidden**:
- **eval-time** (`nix flake check` default): typed/cross-field assertions against the *declared*
  config intent (`structuredExtraConfig` + the literal `.config`'s recorded symbols), cheap and
  building no kernel.
- **realization-time** (sharded CI, not the default flake check): the composed contract checked
  against the built kernel's final `.config`, errors vs. warnings distinguished, toolchain-derived
  symbols stripped before diffing. Exposed as a per-device lazy check so `nix flake check` never
  builds every device kernel.

Source-of-truth precedence: the literal `spatial.kernel.configFile` is authoritative;
`structuredExtraConfig` is applied on top and the realization-time check verifies the merged result
(Mobile NixOS's validator model). This contract is the mechanism that lets NixOS be the default
runtime safely (see [adr/0002](adr/0002-nixos-vs-nix-built-userspace.md)).

### `spatial.adaptation.*` — per-subsystem backend selection

Each subsystem independently selects its backend. This is the core of the hardware boundary.

```nix
spatial.adaptation = {
  display   = { backend = "native"; };          # native | android-backed | device-specific
  gpu       = { backend = "native"; };          # Mesa/Freedreno by default
  camera    = { backend = "native"; };          # V4L2
  sensors   = { backend = "native"; };          # IIO
  audio     = { backend = "native"; };          # PipeWire
  wifiBt    = { backend = "native"; };          # mac80211 / BlueZ
  tracking  = { backend = "device-specific"; }; # the hard one; see below
  eyes      = { backend = "none"; };            # eye tracking; most targets lack the hardware (ADR 0011)
};
```

- **native** (default): the subsystem uses the mainline driver stack. Requires the corresponding
  kconfig contract category.
- **android-backed**: pulls a per-subsystem blob closure from the donor (the HAL `.so`, its `NEEDED`
  closure, `.rc`/VINTF manifests, ueventd entries — [03](../research/03-android-compat.md) §10.5),
  builds the matching libhybris variant (`android-headers-<gen>-<device>` derivation +
  `android-config.h` defines — [03](../research/03-android-compat.md) §9.2), and starts a
  late, optional LXC unit or a `libgbinder` bridge. **Never a `local-fs.target` prerequisite.**
- **device-specific**: a dedicated implementation. DSP-based tracking has no ecosystem precedent
  ([03](../research/03-android-compat.md) §9.6, §11 item 1) and must be prototyped per device.

Assertions: an `android-backed` subsystem requires `spatial.donor` to expose the needed partitions
and the kconfig contract to include the Android-HAL prerequisite category; a `native` subsystem
requires its mainline kconfig category.

### `spatial.xr.*` — XR runtime and device driver

Mirrors Monado's build/runtime surface ([05](../research/05-xr-userspace.md) §9 item 4):

| Option | Type | Notes |
|---|---|---|
| `spatial.xr.runtime` | enum `monado`\|`wivrn`\|`none` | Monado is the only initial on-device runtime; `wivrn` models the optional streaming-**server** role (its headset side is an Android app on the vendor runtime, per [05-xr-userspace](../research/05-xr-userspace.md) §2.2) and is not a drop-in appliance runtime; `none` for headless bring-up |
| `spatial.xr.monado.rev` + `.patches` | rev + listOf patch | per-device driver as monado-rev + patch series (WiVRn pattern) |
| `spatial.xr.monado.drivers.<name>.enable` | bool | → `XRT_BUILD_DRIVER_*`, minimal per-device runtime |
| `spatial.xr.compositor.backend` | enum `vk-display`\|`wayland-direct`\|`window` | vk-display for appliance; **first feasibility test** |
| `spatial.xr.environment` | attrs | → systemd unit env (the proven config channel) |
| `spatial.xr.tracking.slam.package` | pkg | provides `libbasalt.so`, sets `VIT_SYSTEM_LIBRARY_PATH` |
| `spatial.xr.calibration.paths` | attrs | per-device calibration data locations (per-unit state) |

### `spatial.deployment.*` — partitions, images, flashing

| Option | Type | Notes |
|---|---|---|
| `spatial.deployment.bootScheme` | enum `android-bootimg`\|`uefi-rauc`\|`abl-uboot` | selects image family + update backend |
| `spatial.deployment.partitions` | listOf submodule | layout; `keepVerbatim` list for pass-through blobs |
| `spatial.deployment.abSlots` | bool | drives slot handling and the mark-successful unit |
| `spatial.deployment.flashMethod` | enum | `fastboot`\|`heimdall`\|`edl-qdl`\|`rauc`\|… (declarative flasher table) |
| `spatial.deployment.protectedPartitions` | listOf str | persist/calib/NV — never touched without a separately-reviewed op |
| `spatial.deployment.imageVariants` | listOf str | which `lib/images/` variants to build |

### `spatial.hardware.ipd.*` and the `eyes` subsystem (ADR 0011)

`spatial.hardware.ipd.source` ∈ `fixed | manual | manual-sensed | stored | motorized-auto` declares
where the rendering-IPD value comes from, with `ipd.defaultMeters` as the safe pre-auth default
(greeter/lock render with it per [adr/0007](adr/0007-session-greeter-lock.md)). `motorized-auto`
(an eye-tracked lens servo, Galaxy XR / Play For Dream class) requires
`spatial.adaptation.eyes.backend != "none"` — asserted. The eyes backend selects the session-scoped
Monado-side eye-tracking service (gaze via `XR_EXT_eye_gaze_interaction`, rotation-center IPD into
`eye_relation`, event-gated motor proposals); see
[adr/0011-eye-tracking-ipd.md](adr/0011-eye-tracking-ipd.md). The qualification matrix gains
per-device rows — *IPD source*, *eye-camera access class*, *ET capability*, *iris auth* — with
current values in [research/29](../research/29-eye-hardware-ipd-per-target.md).

### `spatial.qualification.*` — required functionality and tests

| Option | Type | Notes |
|---|---|---|
| `spatial.qualification.acceptanceTests` | listOf test | automated + manual, gated by tier |
| `spatial.qualification.readinessCheck` | test | the XR-readiness health check an update must pass |

## Support tiers (encoded and CI-enforced)

Following postmarketOS's directory-as-tier + CI gate ([02](../research/02-postmarketos.md) §3.6) and
Mobile NixOS's `supportLevel` enum, but with tier *requirements* fully encoded as evaluatable checks
(fixing pmOS's wiki-only gap):

- **`booting`** — kernel boots, serial/adb reachable. Requires: valid device contract, kconfig
  contract passes, donor pinned. No maintainer required.
- **`xr-functional`** — a real OpenXR app renders on the physical display with working tracking.
  Requires everything in `booting` + a passing `readinessCheck` + named maintainers.
- **`release-supported`** — reproducible build, documented donor combination, recovery tested,
  update/rollback tested, hardware regression coverage (sustained rendering, sensor timestamps,
  tracking, controller reconnect, thermal, suspend/resume, interrupted update), named maintainers.

CI asserts that a device's declared tier is consistent with the checks it actually passes.

## What a minimal device file looks like (target)

```nix
# devices/lynx-r1/default.nix
{ ... }:
{
  imports = [ ../../families/xr2-gen1 ];

  spatial.device = {
    codename = "lynx-r1";
    vendor = "lynx";
    name = "Lynx R1";
    arch = "aarch64";
    supportTier = "booting";
    maintainers = [ ];
  };

  spatial.hardware = {
    displays = 2;
    panel = { width = 1600; height = 1600; refresh = 90; };
  };

  spatial.donor = import ./donor.nix;   # hashes, build IDs, extraction rules
  # kernel, adaptation, xr, deployment all default from the xr2-gen1 family + sm8250 SoC module
}
```

Everything mechanical lives in `families/xr2-gen1/` and `soc/sm8250/`; the device supplies identity,
geometry, and its donor manifest.
