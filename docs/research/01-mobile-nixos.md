# 01 — Mobile NixOS (and Tow-Boot): build-system architecture study

**Sources studied (local shallow clones):**

- `references/mobile-nixos` — HEAD `2c132754` (2026-07-12, *"Merge pull request #884 from samueldr-wip/fix/nixpkgs-2026-07"*)
- `references/tow-boot` — HEAD `59259b3` (`development` merged with `released`)

All paths below are relative to the respective repo root. Unless prefixed with `tow-boot/`, paths refer to the mobile-nixos tree.

---

## 1. Project purpose

**Mobile NixOS** is a *superset* of NixOS — not a fork (`README.adoc:5-9`, `doc/about.adoc`). It layers a `mobile.*` option namespace, a replacement stage-1 init, a kernel builder, and a disk-image builder onto stock nixpkgs, so that one NixOS configuration can be built for many ARM phones and Chromebooks. The thesis it sells: *the differences between mobile devices are configuration data, not forks.*

**Tow-Boot** is "an opinionated distribution of U-Boot" (`tow-boot/README.md`) by the same author. It solves the layer *below*: turn a board's idiosyncratic vendor boot process into uniform, EBBR-ish, installable platform firmware with a consistent UX (same baud rate everywhere, same boot menu, same installer flow). Mobile NixOS assumes that firmware exists and boots a generic disk image off it — `devices/pine64-pinephone/README.adoc:8-11` ("It is recommended to install Tow-Boot to dedicated storage... Mobile NixOS is tested with Tow-Boot").

For spatial-os the pairing matters more than either project individually: **the boot-chain problem and the OS-image problem are deliberately separated into two repos with a narrow contract between them.**

---

## 2. Repository / build architecture

```
default.nix            entry point; --argstr device foo
pkgs.nix               pinned nixpkgs instantiation (npins)
npins/sources.json     the pin
release.nix            Hydra job matrix
lib/                   eval plumbing (3 files, 212 lines total)
modules/               60 NixOS modules defining `mobile.*`
devices/               12 device dirs + devices/families/
overlay/               nixpkgs overlay: packages + image-builder + kernel-builder
boot/                  the stage-1 init, written in mruby (70 .rb files)
bin/                   dev helpers (menuconfig, kernel-normalize-config, ...)
examples/              example systems (hello, phosh, installer, target-disk-mode)
doc/                   asciidoc, rendered to the website
```

**Evaluation chain:** `default.nix` → `lib/eval-with-configuration.nix` → `lib/release-tools.nix`'s `evalWith` → nixpkgs' own `nixos/lib/eval-config.nix`. The single most architecturally significant lines in the repo:

```24:27:references/mobile-nixos/lib/release-tools.nix
    , baseModules ? (
      (import ../modules/module-list.nix)
      ++ (import "${toString pkgs.path}/nixos/modules/module-list.nix")
    )
```

Mobile NixOS modules are injected into NixOS' **`baseModules`**, not imported as user modules. Consequence: `mobile.*` options are as "built in" as `networking.*`, `extendModules` picks them up automatically (used heavily — §6.4), and device files need no import boilerplate. The cost is that `nixos-rebuild` against a stock channel needs a *second* integration path, `lib/configuration.nix`, which imports device + `module-list.nix` into the user's own `imports`. Two entry points with subtly different semantics is a wart worth avoiding.

**Outputs are options.** Every artifact lives under `mobile.outputs.*` (`modules/outputs.nix`). System types register sub-attrsets (`mobile.outputs.android.*`, `.depthcharge.*`, …) and set `mobile.outputs.default`. The CLI surface is flattened at the very end:

```66:66:references/mobile-nixos/lib/eval-with-configuration.nix
  outputs = eval.config.mobile.outputs // eval.config.mobile.outputs.${eval.config.mobile.system.type};
```

So `nix-build -A outputs.default` works identically for every device, while `-A outputs.android-bootimg` exists only on Android devices. Building the whole attrset is booby-trapped with a `throw` (`lib/eval-with-configuration.nix:99-109`). `release.nix` drives Hydra with a `device × system` matrix (`release.nix:167-183`), example systems, cross-canaries, and two aggregates `tested`/`testedPlus` (`release.nix:289-364`).

---

## 3. Device abstraction model

### 3.1 The taxonomy

`doc/in-depth/devices.adoc` defines four terms; this taxonomy is the part most worth stealing.

| Term | What it is | Where it lives |
|---|---|---|
| **System (type)** | the *boot chain* — how a kernel gets loaded | `modules/system-types/{android,depthcharge,u-boot,uefi}` |
| **Platform / SoC** | SoC-level facts and quirks | `modules/hardware-<vendor>.nix` |
| **Family** | an "incomplete device"; shared config for near-identical models | `devices/families/<name>/` |
| **Device** | a model (or set of SKUs booting the same build) | `devices/<oem>-<codename>/` |

The doc is explicit about the discriminator — *"following 'the same kernel build boots them' is likely a good differentiator"* (`doc/in-depth/devices.adoc:78`) — and about families deliberately **not** being a module: *"families are an implementation detail of a device, and not an intrinsic part of the architecture"* (`:151-158`). A family is just a path you `imports`. Naming is `$oem-$codename` because those codenames are what Android ROM communities use, so they're searchable.

### 3.2 Case study A — `oneplus-enchilada` (OnePlus 6, SDM845, Android boot)

The entire device file is 25 lines:

```1:25:references/mobile-nixos/devices/oneplus-enchilada/default.nix
{ config, lib, pkgs, ... }:

{
  imports = [
    ../families/sdm845-mainline
  ];

  mobile.device.name = "oneplus-enchilada";
  mobile.device.identity = {
    name = "OnePlus 6";
    manufacturer = "OnePlus";
  };
  mobile.device.supportLevel = "supported";

  mobile.hardware = {
    ram = 1024 * 8;
    screen = { width = 1080; height = 2280; };
  };

  mobile.device.firmware = pkgs.callPackage ./firmware {};

  mobile.system.android.device_name = "OnePlus6";
}
```

Everything else comes from `devices/families/sdm845-mainline/default.nix` (73 lines), which declares:

- `mobile.hardware.soc = "qualcomm-sdm845"` — which, via `modules/hardware-soc.nix` → `modules/hardware-qualcomm.nix:116-120`, sets `mobile.system.system = "aarch64-linux"` and defaults `mobile.boot.boot-control.enable` on.
- The kernel: `pkgs.callPackage ./kernel {}`, an `sdm845-mainline/linux` fork at tag `sdm845-6.4-r1` plus one `fetchpatch` (`.../sdm845-mainline/kernel/default.nix`).
- `mobile.system.type = "android"`, the mkbootimg offset table (`offset_base = "0x00000000"`, `pagesize = "4096"`, …), and `ab_partitions = lib.mkDefault true` — *"Assumed all SDM845 devices use A/B"* (`:38-52`).
- A DTB name *template* resolving per device: `appendDTB = lib.mkDefault [ "dtbs/qcom/sdm845-${config.mobile.device.name}.dtb" ]`.
- Firmware surgery for stage-1: copy the firmware package, delete `qcom/sdm845/*/modem.mbn` because it is *"Big file, fills and breaks stage-1"*, then graft `a630_sqe.fw`/`a630_gmu.bin` in from `linux-firmware` (`:24-33`).
- USB gadget identity, `mobile.quirks.qualcomm.sdm845-modem.enable = true`, a udev rule tagging the haptics device.

**Declares vs implements:** the device declares identity, screen geometry, RAM, one firmware derivation, and one Android product string. It *implements* nothing. All executable logic lives in the family, the SoC module, and the system-type module. `oneplus-fajita` (OnePlus 6T) has the same shape.

### 3.3 Case study B — `motorola-potter` (Moto G5 Plus, MSM8953, Android boot via `lk2nd`)

The counter-example: no family, so it carries everything itself (`devices/motorola-potter/default.nix`, 94 lines). It is marked `supportLevel = "broken"` with the comment *"The boot image is currently too big to fit."* Knobs it must set that `enchilada` never touches: `flashingMethod = "lk2nd"`; a per-device kernel structured-config fragment forcing `CC_OPTIMIZE_FOR_SIZE`; a modular kernel with an explicit initrd module list (`rmi_i2c`, `qcom-pon`, two alternative panel drivers, `msm`); `compression = "xz"` because "the boot partition on this phone is 16MB"; different mkbootimg offsets (`pagesize = "2048"`, `offset_base = "0x80000000"`); and `mobile.device.enableFirmware = false` because the firmware derivation needs a user-supplied dump of their own `modem` partition (§4d).

It also ships `devices/motorola-potter/partitions_32GB.gdisk` — a *comment file* recording the donor's stock GPT verbatim from `fdisk -l`. Not consumed by the build; pure archaeology, and cheap.

### 3.4 Case study C — `acer-lazor` (Chromebook Spin 513, SC7180, depthcharge)

19 lines: `imports = [ ../families/mainline-chromeos-sc7180 ]`, identity, screen size. The family chain is two deep. `devices/families/mainline-chromeos/default.nix` sets `mobile.system.type = "depthcharge"`, a modular kernel, and three `sbs-*` modules deliberately deferred to udev ("Breaks udev if builtin or loaded before udev runs"). `devices/families/mainline-chromeos-sc7180/default.nix` sets the SoC, the kernel (stock `torvalds/linux` v6.5 + two `fetchpatch` reverts), the DTB directory for the kpart, the serial console `ttyMSM0,115200n8`, the firmware, and a `nixpkgs.overlays` entry exposing a non-redistributable firmware package (§4b).

`devices/asus-dumo/default.nix` is the un-familied depthcharge equivalent (Rockchip OP1) and shows the escape hatch for device-specific *runtime* behaviour — two mruby task files injected into the stage-1 init:

```58:61:references/mobile-nixos/devices/asus-dumo/default.nix
  mobile.boot.stage-1.tasks = [
    ./fixup_sdhci_arasan_task.rb
    ./usb_role_switch_task.rb
  ];
```

### 3.5 Adding a new device, concretely

1. `devices/<oem>-<codename>/default.nix` with `mobile.device.{name,identity,supportLevel}`.
2. `mobile.hardware.soc = "<vendor>-<soc>"`; if new, add an option and arch mapping in `modules/hardware-<vendor>.nix`. The SoC registry is a flat set of `enable` booleans checked by an assertion in `modules/hardware-soc.nix:19-22`.
3. `mobile.system.type` plus its per-type parameters.
4. `devices/<name>/kernel/{default.nix,config.aarch64}` calling `mobile-nixos.kernel-builder`.
5. `devices/<name>/firmware/default.nix`.
6. Optionally `README.adoc` (rendered into the website device page) and stage-1 tasks.

Device discovery is by directory listing (`lib/release-tools.nix:10-14` filters `devices/*/default.nix`) — no registry file to update. There is a semi-automated porting helper, `overlay/mobile-nixos/autoport` (a pinned external Ruby tool that inspects a donor `boot.img`), but `doc/porting-guide.adoc` is four sentences and marked *"This subject needs to be expanded upon."*

---

## 4. Vendor blob / donor firmware handling

There is no single mechanism; there are **five**, chosen per device by redistributability. All five are relevant to a headset distro.

**(a) Pinned third-party firmware repo — the happy path.** `devices/oneplus-enchilada/firmware/default.nix` does a `fetchFromGitLab` of `sdm845-mainline/firmware-oneplus-sdm845` at a fixed rev + sha256, then reshuffles `lib/firmware/postmarketos/*` up one level. Always tagged `meta.license = lib.licenses.unfree` with the comment *"We make no claims that it can be redistributed."*

**(b) Extraction from a pinned vendor OS image** — the donor-image pattern spatial-os needs, in ~25 lines (`devices/families/mainline-chromeos-sc7180/firmware/non-redistributable.nix`): `fetchzip` a specific ChromeOS recovery `.bin.zip` from `dl.google.com`, then

```26:39:references/mobile-nixos/devices/families/mainline-chromeos-sc7180/firmware/non-redistributable.nix
  echo ":: Extracting $part from ChromeOS image"

  eval "$(
      sfdisk --dump "$disk_image" | grep "$part" | sed -e 's/,\s*/;/g' -e 's/\s*=\s*/=/g' -e 's/^.*\s:\s//'
  )"

  echo ":: Extracting firmware files from $part"

  (
  PS4=" $ "
  set -x
  dd bs=512 if="$disk_image" of="$part" skip="$start" count="$size"
  7z x -o"$out" "$part" "lib/firmware/qcom/sc7180-trogdor"
  )
```

The vendor image is a fixed-output derivation; the extraction is an ordinary derivation; the specific subtree is named explicitly. It is exposed via `nixpkgs.overlays` rather than enabled by default, so the user opts in.

**(c) Minimal redistributable subset + fakes.** The sibling `.../firmware/default.nix` copies an explicit allowlist out of `linux-firmware`, then fabricates the modem filesystem with `dd if=/dev/zero` (*"Add bogus firmware files for the modem, which is unused on non-LTE devices"*). Fabricating plausible blobs to satisfy a driver is a legitimate move.

**(d) User-supplied partition dump.** When neither redistribution nor URL fetching is defensible, the derivation's default argument is a `throw` that prints instructions:

```4:15:references/mobile-nixos/devices/motorola-potter/firmware/default.nix
, modem ? builtins.throw ''

    Your attention is required:
    ---------------------------

    You will need to provide the content of the modem partition this way:

      hardware.firmware = [
        (config.mobile.device.firmware.override {
          modem = ./path/to/copy/of/modem;
        })
      ];
```

paired with `mobile.device.enableFirmware = false` (`modules/mobile-device.nix:45-55`) so evaluation still succeeds for anyone not building that device. "Throw-with-instructions as the default argument" is an excellent, cheap UX pattern.

**(e) Don't extract at all — mount the donor partition at runtime.** `modules/initrd-vendor.nix` mounts the stock `/vendor` read-only and points the kernel firmware loader at it:

```25:37:references/mobile-nixos/modules/initrd-vendor.nix
  config = lib.mkIf (vendor.partition != null) {
    boot.kernelParams = [
      "firmware_class.path=/vendor/firmware"
    ];

    boot.specialFileSystems = {
      "/vendor" = {
        device = vendor.partition;
        fsType = "ext4";
        options = [ "ro" "nosuid" "noexec" "nodev" ];
      };
    };
  };
```

Zero redistribution risk, zero fetching, perfectly matched to the donor — at the cost of a non-reproducible runtime dependency on whatever the vendor left on flash. **No device in the current tree uses it**, but for XR headsets, where the blob set is large (camera/DSP/tracking) and legally radioactive, this is arguably the primary mechanism.

Aggregation is a `buildEnv` over `/lib/firmware` with `ignoreCollisions = true` (`modules/initrd-firmware.nix:17-22`), and a single `mobile.device.firmware` option auto-appended to `hardware.firmware` unless opted out (`modules/mobile-device.nix:72-76`).

---

## 5. Kernel strategy

mobile-nixos does **not** use nixpkgs' kernel infrastructure. It has its own `mobile-nixos.kernel-builder` (`overlay/mobile-nixos/kernel/builder.nix`, 700 lines) because *"many kernels will be of older vintages, not supported by NixOS' own kernel build infrastructure"* (`builder.nix:1-13`), and it disables the NixOS kernel-config checker outright: `system.requiredKernelConfig = lib.mkForce []` (`modules/initrd-kernel.nix:253`).

### The config model — the best single idea in the repo

A device ships a **literal, complete `.config`** (`devices/*/kernel/config.aarch64`), which is the source of truth. Structured config is *not* used to generate it; it is used as a **validator** and an optional merge source.

- `modules/kernel-config.nix` accumulates `mobile.kernel.structuredConfig` — a list of `helpers: { ... }` functions (universal options, systemd requirements, nftables, HID sensors, Waydroid) all guarded by `whenAtLeast`/`whenOlder`/`whenBetween`, so one fragment set covers Linux 3.18 → 6.17.
- `overlay/mobile-nixos/kernel/eval-config.nix` reuses nixpkgs' own `nixos/modules/system/boot/kernel_config.nix` module and emits **two** things: a `configfile` string, and a `validatorSnippet` — a standalone shell script that greps the real `.config` and reports errors vs warnings, with `optional` items downgrading errors to warnings (`eval-config.nix:47-143`).
- At build time (`builder.nix:337-436`): copy the device `.config`; optionally append the structured config and run `oldconfig`; optionally diff pre/post-`oldconfig` to enforce a *normalized* config; then run the validator.

Two toggles control strictness: `updateConfigFromStructuredConfig` (on for users — "allows implicitly tracking revision updates") vs `forceNormalizedConfig` (on in CI via `mobile.boot.stage-1.kernel.useStrictKernelConfig`, set by `examples/common-configuration.nix`). Before diffing, ~25 toolchain-derived symbols are stripped (`CONFIG_CC_VERSION_TEXT`, `CONFIG_GCC_PLUGIN_*`, `CONFIG_AS_VERSION`, …) because otherwise cross-compilation and minor version bumps break the comparison (`builder.nix:376-411`).

Developer ergonomics are first class: `passthru.normalizedConfig`, `passthru.validatedConfig`, and `passthru.menuconfig` — the last builds an `nconf` binary wrapped in a script that copies the kernel tree to a tmpdir and edits your `config.aarch64` in place (`builder.nix:597-697`), driven by `bin/menuconfig $device` and `bin/kernel-normalize-config $device`.

### Vendor/downstream kernel accommodation

The builder signature is a catalogue of Android-kernel pathologies, each a boolean:

| Flag | Purpose | `builder.nix` |
|---|---|---|
| `isQcdt` / `qcdt_dtbs` | build a Qualcomm `dt.img` with `dtbTool` | 98-99, 503-509 |
| `isExynosDT` | Exynos `dt.img` with platform/subtype magic | 102-105, 510-518 |
| `isImageGzDtb` | Android appended-DTB image target | 108, 519-522 |
| `dtboImg` | `mkdtboimg.py create` | 113, 523-528 |
| `enableRemovingWerror` | strip all `-Werror` from every Makefile/Kbuild | 126, 318-327 |
| `enableCompilerGcc6Quirk` | drop in a `compiler-gcc6.h` shim | 129, 328-331 |
| `enableCombiningBuildAndInstallQuirk` | auto-enabled for < 4.4 (rebuild-on-install bug) | 133 |
| `enableDefaultYYLOCPatch` | fix `multiple definition of 'yylloc'` on < 4.0 | 140, 250 |
| `enableForceLogoPatch`, `enableCenteredLinuxLogo` | boot-logo hacks incl. `sed` on `fbmem.c` | 117-121, 292-316 |

Reproducibility handling: randstruct seed derived deterministically from `sha256(src + configfile)` (`:273-282`), `KBUILD_BUILD_TIMESTAMP` from `SOURCE_DATE_EPOCH` (`:447`), `KBUILD_BUILD_VERSION=1-mobile-nixos`, `rm -vf *.x509` ("Removing default OEM-provided certificates"), FHS path stripping in Makefiles.

Kernel sources in tree span the spectrum: stock `torvalds/linux` v6.5 + two reverts (sc7180); community SoC forks (`sdm845-mainline` @ `sdm845-6.4-r1`, `msm8953-mainline` @ 5.16); and a personal tree (`codeberg.org/megi/linux` @ `orange-pi-6.17-20251026-1441` for PinePhone). **No genuinely downstream 3.x/4.x Android kernel remains in the current tree**, even though the builder was built for them (§11).

One misfeature: the boot logo is rasterized per device from an SVG at the exact screen resolution and injected into the kernel via the nixpkgs overlay (`modules/initrd-kernel.nix:169-190`). Two devices differing only in panel size cannot share a kernel build.

---

## 6. Image assembly and flashing

### 6.1 The image builder

`overlay/image-builder/` is a self-contained ~1700-line *modules system* (`lib.evalModules`, not NixOS) for filesystem and disk images, exposed as option types: `mobile.generatedFilesystems` and `mobile.generatedDiskImages` are `types.attrsOf pkgs.image-builder.types.{filesystem-image,disk-image}` (`modules/generated-filesystems.nix:16-19`, `modules/generated-disk-images.nix:12-17`).

Filesystem images have fixed phases (`populate → allocate → mkfs → copy → check → additional`, `filesystem-image/basic.nix:197-221`), auto-computed sizes, and backends for ext4/btrfs/fat32/squashfs. Disk images take a list of partition submodules with explicit `partitionUUID`, `partitionType`, `offset`, `length`, `bootable`, `requiredPartition`, `isGap`, and either a nested `filesystem` submodule or a `raw` path (`disk-image/partitions.nix`).

Determinism is taken seriously: every content-producing step runs under `libfaketime -f "1970-01-01 00:00:01"`, `make_ext4fs` gets an explicit `-U` UUID and `-L` label, and every partition UUID in the tree is a hardcoded constant (`modules/rootfs.nix:42`, `modules/disk-image.nix:20-31`). Each image derivation emits a second `metadata` output with `tree.xz` and `ncdu.xz` (`filesystem-image/builder.nix:25,63-67`).

The ugly part is ext4 sizing: an *empirically measured lookup table* of "fudge factors" per image size, with the comment *"This table was built using a script that built an image with `make_ext4fs` for the given size in MiB, and recorded the available size according to `df`"* (`filesystem-image/filesystem/ext4.nix:26-48`).

### 6.2 The rootfs

`modules/rootfs.nix` produces one ext4 image labelled `NIXOS_SYSTEM`, populated by walking `closureInfo`'s `store-paths` and copying each path, plus a `nix-path-registration` file. First boot re-hydrates the Nix database:

```100:113:references/mobile-nixos/modules/rootfs.nix
    boot.postBootCommands = mkIf (config.mobile.rootfs.rehydrateStore) ''
      # On the first boot do some maintenance tasks
      if [ -f /nix-path-registration ]; then
        # Register the contents of the initial Nix store
        ${config.nix.package.out}/bin/nix-store --load-db < /nix-path-registration

        # nixos-rebuild also requires a "system" profile and an /etc/NIXOS tag.
        touch /etc/NIXOS
        ${config.nix.package.out}/bin/nix-env -p /nix/var/nix/profiles/system --set /run/current-system

        # Prevents this from running on later boots.
        rm -f /nix-path-registration
      fi
    '';
```

Root is mounted `by-label` with `autoResize = true` and `boot.growPartition = true` — build minimal, grow on first boot. `modules/shared-rootfs.nix` produces a *device-agnostic* rootfs by nulling `kernelFile`/`initrdFile` and `rm`-ing `dtbs initrd kernel kernel-modules kernel-params` from the toplevel; this is what the Hydra example systems use so one closure serves all devices.

### 6.3 Per-system-type assembly

**Android** (`modules/system-types/android/`). `bootimg.nix` is a thin `mkbootimg` wrapper:

```37:48:references/mobile-nixos/modules/system-types/android/bootimg.nix
  mkbootimg \
    --kernel  $kernel \
    ${optionalString (bootimg.dt != null) "--dt ${bootimg.dt}"} \
    --ramdisk ${initrd} \
    --cmdline       "${cmdline}" \
    --base           ${bootimg.flash.offset_base   } \
    --kernel_offset  ${bootimg.flash.offset_kernel } \
    --second_offset  ${bootimg.flash.offset_second } \
    --ramdisk_offset ${bootimg.flash.offset_ramdisk} \
    --tags_offset    ${bootimg.flash.offset_tags   } \
    --pagesize       ${bootimg.flash.pagesize      } \
    -o $out
```

**There is no `--header_version`.** The tool is `osm0sis/mkbootimg` pinned at `2020.05.18` (`overlay/mkbootimg/default.nix`), and the wrapper emits only legacy boot images, using either the old `--dt` mechanism or DTBs *concatenated onto the kernel* (`appendDTB`, `bootimg.nix:26-34`). **Nothing in the tree produces header v2/v3/v4 or a `vendor_boot` image.** For XR2 / Android-11+ donors this is a hard gap.

The default Android output is a deliberately **reference-free** directory:

```29:33:references/mobile-nixos/modules/system-types/android/default.nix
  # Note:
  # The flash scripts, by design, are not using nix-provided paths for
  # either of fastboot or the outputs.
  # This is because this output should have no refs. A simple tarball of this
  # output should be usable even on systems without Nix.
```

— `system.img`, `boot.img`, optionally `recovery.img`, and a `flash-critical.sh` calling bare `fastboot`/`heimdall`. Flashing method is an enum (`fastboot` | `lk2nd` | `odin`) driving both the script and which documentation fragment renders (`android/default.nix:110-119, 222`). There is also an Android recovery-`.zip` generator built from a static mruby binary (`system-types/android/flashable-zip.nix`, `overlay/mobile-nixos/android-flashable-zip-binaries/`).

A/B is modelled as three booleans with derived defaults — `ab_partitions`, `boot_as_recovery` (defaults to `ab_partitions`), `has_recovery_partition` (defaults to `!boot_as_recovery`) — plus `boot_partition_destination` and `system_partition_destination` string escape hatches for OEMs that name partitions oddly, or where the system image must go to `userdata` (`android/default.nix:89-140`).

`recovery.img` is *the same build* re-evaluated with one config line flipped (`modules/recovery.nix`), giving what the docs call "recovery as pseudo-A/B": a second, always-bootable stage-1 to fall back to (`doc/in-depth/android/boot.adoc:24-35`).

**Depthcharge** (`modules/system-types/depthcharge/default.nix`): FIT image via `mkimage`, `futility vbutil_kernel` signed with nixpkgs' **vboot devkeys**, a GPT with a `CHROMEOS_KERNEL`-typed first partition, finalized with `cgpt add -i 1 -S 1 -T 5 -P 10`. The `.its` generator is fetched from a raw `githubusercontent.com` URL belonging to a third project (`:40-44`).

**U-Boot** (`modules/system-types/u-boot/default.nix`): an ext4 `boot` partition holding `mobile-nixos/{boot,recovery}/{kernel,stage-1,dtbs/}` plus a generated `boot.scr` that hunts for the partition by partlabel, falls back to the bootable flag, and tries normal then recovery kernel. Mobile NixOS **does not build U-Boot** — `mobile.outputs.u-boot.u-boot` is declared as an option but never assigned anywhere in the tree. That is Tow-Boot's job.

**UEFI** (`modules/system-types/uefi/default.nix`): a hand-rolled UKI — `objcopy --add-section` of `.osrel/.cmdline/.initrd/.linux` onto systemd's EFI stub, with manual section-alignment arithmetic parsed out of `objdump` output.

### 6.4 Stage-1

`modules/initrd.nix` sets `boot.initrd.enable = false` and builds its own initrd, because the NixOS stage-1 is a bash script with assumptions that don't hold on phones. The replacement is **mruby bytecode**: `boot/init/*.rb` plus selected `boot/lib/**` are compiled by `mrbc` into a single `init.mrb` (`boot/init/default.nix:43-60`) and executed by a small native loader (`/loader`, `boot/script-loader/`). Device configuration reaches it as JSON at `/etc/boot/config` (`modules/initrd.nix:79,88`).

The init is a **dependency-resolved task graph**, not a script — 15 built-in tasks in `boot/init/tasks/` (`mount`, `udev`, `modules`, `luks`, `splash`, `auto_resize`, `switch_root`, …), with devices contributing more via `mobile.boot.stage-1.tasks`. There is a full LVGL GUI in the initrd (`boot/lib/lvgui/`, `boot/recovery-menu/`, `boot/splash/`): recovery menu, generation selection, progress bars.

`boot/init/tasks/switch_root.rb` is the bootloader-replacement logic: enumerate `/nix/var/nix/profiles/system-*` as generations, resolve `init=` / `mobile-nixos.generation=` from the cmdline, rehydrate from `nix-path-registration` on first boot, and — in **stage-0** mode — `kexec` into the *selected generation's own kernel*, forwarding bootloader-provided FDT nodes (`/memory`, `/ serial-number`) into the new DTB (`switch_root.rb:123-151`, `modules/stage-0.nix:34-52`, `overlay/mobile-nixos/fdt-forward/`). DTB choice is driven by a build-time `dtb-mapping.json` keyed on `/proc/device-tree/compatible` (`modules/system-build.nix:8-14`).

Both `stage-0` and `recovery` use the same trick — re-evaluate the whole configuration with a small override:

```69:78:references/mobile-nixos/modules/stage-0.nix
    mobile.outputs.stage-0 = (extendModules {
      modules = [
        (
          { config, ... }:
          lib.mkIf supportsStage-0 {
            mobile.boot.stage-1.stage = 0;
            mobile.boot.stage-1.extraUtils = [
              { package = pkgs.kexec-tools; }
              { package = fdt-forward; }
            ];
```

The system types then consume `config.mobile.outputs.stage-0.mobile.boot.stage-1.kernel` rather than the top-level one (`android/default.nix:10`, `depthcharge:18`, `u-boot:11`, `uefi:15`). Elegant and cheap to express, but it multiplies evaluation cost and the indirection is invisible unless you know to look.

---

## 7. Update mechanism

**There isn't one beyond `nixos-rebuild switch`.** Stated plainly:

```17:19:references/mobile-nixos/modules/bootloader.nix
            Installation and management of the Mobile NixOS bootloading
            components is not implemented at this point in time.
            (e.g. flashing boot.img on android, boot partition on other systems.)
```

What exists:

- **Rootfs updates:** ordinary NixOS. The rootfs grows into its partition and `nixos-rebuild` works on-device or via `--target-host`.
- **Kernel/initrd updates without reflashing:** only via `stage-0` kexec. The flashed `boot.img` carries a stage-0 initrd that chain-loads whatever kernel the selected generation has. Gated on `mobile.quirks.supportsStage-0`, which **no device in the current tree sets** (default `false`, `modules/stage-0.nix:24-32`). Without it, a kernel change means a manual `fastboot flash boot`.
- **A/B:** `mobile.boot.boot-control.enable` (default on for SDM845) installs a one-shot systemd unit running `boot-control --mark-successful` (`modules/boot-control.nix:24-38`). The tool (`overlay/mobile-nixos/boot-control/`) is a small Ruby wrapper around `sgdisk` manipulating GPT attribute bits. It only *confirms* a boot — there is no slot-switching updater, no download/verify/stage pipeline.
- **Rollback:** the initrd's generation-selection GUI, plus `recovery.img` as a known-good second stage-1.

Honest summary: mobile-nixos solves *first install* and *in-place userspace updates*, and punts on *boot-artifact lifecycle*. Tow-Boot is the project that took firmware update seriously (§12).

---

## 8. Reproducibility properties

**Good:**

- Nixpkgs pinned with `npins` (`npins/sources.json`, currently `nixos-unstable` @ `nixos-26.11pre1031299.0bb7ec54c848`), with a trace announcing the pin (`pkgs.nix:28`).
- Every vendor blob source is a fixed-output derivation with an explicit hash (`fetchFromGitLab`, `fetchFromGitHub`, `fetchzip`, `fetchurl`) — including the ChromeOS recovery image and the TheMuppets blob URLs.
- `libfaketime` + fixed UUIDs + fixed labels throughout the image builder; `SOURCE_DATE_EPOCH`-derived `KBUILD_BUILD_TIMESTAMP`; deterministic randstruct seed; OEM certificates deleted from kernel trees.
- Kernel `.config` files are checked in verbatim and CI-enforced to be `oldconfig`-stable (`useStrictKernelConfig`, `passthru.validatedConfig`).
- Release evaluation sets `nixpkgs.config.allowAliases = false` (`release.nix:88-91`) to catch deprecated attribute use.

**Impurities and hazards found:**

1. `builtins.currentSystem` is the default `system` (`lib/eval-with-configuration.nix:21-25`) — the standard invocation is impure.
2. `default.nix:8-18` silently imports `./local.nix` if present, with only a `builtins.trace` warning. Evaluation depends on untracked working-tree state.
3. `release.nix:8-9`: *"by design it still relies on NIX_PATH being used for the input Nixpkgs."* Pin and channel mechanism coexist.
4. **Configuration is smuggled into the package set via `nixpkgs.overlays`** in at least three places: `systemBuild-structuredConfig` (`modules/kernel-config.nix:284-294`), `__mobile-nixos-useStrictKernelConfig` and `linuxLogo224PPMFile` (`modules/initrd-kernel.nix:162-173`). Correct-by-construction in Nix, but it means `pkgs` is not a shared, cacheable instantiation across devices, and packages become secretly config-dependent.
5. Third-party raw-URL fetches for *build logic*, not just data: the depthcharge `make-kernel-its.sh` from `raw.githubusercontent.com/thefloweringash/kevin-nix` (`depthcharge/default.nix:40-44`), and mruby's `shellwords.rb` from `raw.githubusercontent.com/ruby/ruby` (`boot/init/default.nix:11-17`). Hashed, but link-rot-prone.
6. Depthcharge images are signed with the **public vboot devkeys** from nixpkgs (`depthcharge/default.nix:81-83`). Fine for dev-mode Chromebooks; there is no real signing story.
7. `motorola-potter`'s modem firmware is a user-supplied path literal — an irreducible impurity, handled as gracefully as it can be.
8. Unfree licensing on blob derivations is asserted by hand on each one; nothing enforces it.

---

## 9. What spatial-os should adopt

1. **The four-term taxonomy (system-type / SoC / family / device), with families as plain `imports`.** It maps onto the headset landscape: system-type ∈ {`android-bootimg`, `abl-chainload-uboot`, `steamos-uefi`}, SoC ∈ {XR2 Gen1, XR2+ Gen2, SD845, Van Gogh…}, family ∈ {Quest-family, Pico-family}, device ∈ individual headsets. A 25-line `oneplus-enchilada` and a 19-line `acer-lazor` are the proof the decomposition works (`doc/in-depth/devices.adoc`).

2. **Build artifacts as typed options with a per-system-type namespace flattened at the CLI boundary.** One command shape (`-A outputs.default`) for every device; per-type extras discoverable; the whole-set build explicitly rejected (`modules/outputs.nix`, `lib/eval-with-configuration.nix:66`).

3. **Literal kernel `.config` as source of truth + version-aware structured config as a *validator*.** This is the right answer for donor kernels, where the vendor defconfig is the only thing that boots. Ship the validator as a standalone shell snippet so it runs outside Nix, distinguish errors from warnings, strip toolchain-derived symbols before diffing, and provide `menuconfig`/`normalize-config` developer commands (`overlay/mobile-nixos/kernel/eval-config.nix`, `modules/kernel-config.nix`, `builder.nix:337-436`, `bin/`).

4. **Per-device booleans for vendor-kernel pathologies on the kernel builder.** `isQcdt`, `enableRemovingWerror`, `enableCombiningBuildAndInstallQuirk` etc. are unglamorous but exactly the shape the problem has (`builder.nix:96-141`). Add `headerVersion`, `hasVendorBoot`, `hasVendorRamdisk`, `hasDtbo` for the modern Android era.

5. **The tiered donor-blob strategy, especially (b) and (e).** Keep the pinned-vendor-image extraction derivation (fetch → parse partition table → `dd` → extract named subtree) *and* make `initrd-vendor.nix`'s "mount the donor `/vendor` read-only, set `firmware_class.path`" a first-class supported mode. For XR blobs (camera, CV, DSP, display tuning), the runtime mount is likely the only legally and practically viable default.

6. **`throw`-with-instructions as a derivation's default argument**, plus an `enableFirmware`-style opt-out so unrelated evaluations still succeed (`devices/motorola-potter/firmware/default.nix:5-18`, `modules/mobile-device.nix:45-55`).

7. **A declarative partition/filesystem image builder as a modules system**, with explicit UUIDs, `faketime`, per-partition `raw`-or-`filesystem` submodules, `isGap`, and a `metadata` output (`overlay/image-builder/`). Do not reuse the ext4 fudge table; use `mke2fs -d` or `mkfs.erofs` and compute sizes honestly.

8. **The reference-free flashing bundle.** A directory of `.img` files plus a generated script that deliberately uses bare tool names, so a user can tar it up and flash from a non-Nix laptop. This matters enormously for headset users (`android/default.nix:29-75`).

9. **Recovery / fallback as a re-evaluation of the same configuration.** One flag flips a build into "recovery" mode and you get a second known-good boot path essentially free. Combined with A/B slots where they exist, this is the cheapest bricking insurance available (`modules/recovery.nix`, `doc/in-depth/android/boot.adoc:24-35`).

10. **Record the donor's stock partition table as a checked-in artifact.** `devices/motorola-potter/partitions_32GB.gdisk` costs nothing and is the first thing anyone porting or unbricking needs.

11. **Support-level enum on every device, surfaced in generated docs and CI.** `supported | best-effort | broken | vendor | unsupported | abandoned` (`modules/mobile-device.nix:57-63`), with `modules/devices-metadata.nix` emitting per-device JSON the docs build consumes. A distro with many half-working targets needs this from day one.

12. **From Tow-Boot: the `composeConfig` variant pattern and the build-diff artifact.** Re-evaluating one board config into N firmware variants (`tow-boot/modules/build.nix:14-24`) is the cleanest multi-artifact idiom in either repo. Shipping `savedefconfig`, the original defconfig, the final `.config`, *and* a unified diff of every patch applied to the source tree (`tow-boot/modules/tow-boot/builder.nix:141-150, 221-238`) is outstanding practice for a project that patches vendor trees.

13. **From Tow-Boot: `knownHashes` guard-rails.** An attrset of hashes per upstream version that `throw`s on an unknown value, so bumping a version without a hash fails at eval, loudly (`tow-boot/modules/tow-boot/src.nix:65-109`).

14. **From Tow-Boot: identity-checked, self-hardening flashers.** Bake the board identifier into the firmware environment at build time and have the installer refuse to run on a mismatched board (`tow-boot/modules/tow-boot/installer.nix:133-137, 272-277`); deliberately erase the first 8 KiB *before* writing the payload so an interrupted flash degrades to "won't boot from here" rather than "won't boot at all" (`installer.nix:167-184`). Directly applicable to ABL-chainloaded U-Boot (§12).

---

## 10. What spatial-os should reject

1. **Do not replace stage-1 with a bespoke language runtime.** The mruby init (`boot/`, 70 `.rb` files + LVGL GUI + a custom mruby builder overlay + a native loader) is the largest maintenance liability in the repo, and it exists mostly to get a pretty splash and a touch recovery menu. Modern NixOS has `boot.initrd.systemd`, which gives generation selection, Plymouth, fsck, LUKS, and kexec for free and is maintained by someone else. Use it; add device-specific units where mobile-nixos uses tasks.

2. **Do not use `nixpkgs.overlays` as an eval-time configuration side channel.** `systemBuild-structuredConfig`, `__mobile-nixos-useStrictKernelConfig`, `linuxLogo224PPMFile` (`modules/kernel-config.nix:284-294`, `modules/initrd-kernel.nix:162-173`) defeat pkgs sharing across devices and make package definitions secretly config-dependent. Pass config explicitly into `callPackage`.

3. **Do not inherit the legacy-only Android boot image assumption.** `overlay/mkbootimg/default.nix` pins a 2020 fork; `system-types/android/bootimg.nix` never passes `--header_version`, has no `vendor_boot`/`vendor_ramdisk`/`bootconfig` handling, and no `dtbo` flashing. Every Qualcomm XR donor of interest is header v3/v4 with a separate `vendor_boot`, and some have `init_boot`. Design for that from the start — ideally by vendoring AOSP's own `mkbootimg.py`/`unpack_bootimg.py` rather than a community fork.

4. **Do not ship two integration entry points.** `default.nix` + `lib/eval-with-configuration.nix` (project path, modules into `baseModules`) and `lib/configuration.nix` (import-into-your-config path, modules into user `imports`) diverge. Pick one — a flake exposing `nixosModules.default` plus `packages.<system>.<device>-<artifact>` — and make the other a thin alias.

5. **Do not adopt the empirical ext4 fudge-factor table** (`image-builder/filesystem-image/filesystem/ext4.nix:26-48`). It is measured constants working around `make_ext4fs`, an abandoned AOSP tool.

6. **Do not depend on raw third-party URLs for build logic** (`depthcharge/default.nix:40-44`, `boot/init/default.nix:11-17`). Vendor the script into the repo.

7. **Do not bake screen-resolution-derived assets into the kernel.** The per-device boot logo (`modules/initrd-kernel.nix:169-190`) forces a distinct kernel build for otherwise-identical devices. On headsets, where one XR2 kernel could plausibly serve several SKUs, this is pure cost. Put splashes in the initrd.

8. **Do not leave the update story as "`nixos-rebuild` and good luck."** For devices that boot from `fastboot`-flashed partitions, with users who aren't Nix experts, boot-artifact lifecycle is a feature, not an afterthought. mobile-nixos' own answer (`stage-0` kexec) is enabled by exactly zero in-tree devices.

9. **Do not duplicate infrastructure across sibling repos.** `tow-boot/support/image-builder/` is a copy of `mobile-nixos/overlay/image-builder/`. If spatial-os grows a boot-firmware sibling, factor the image builder into a shared flake input on day one.

10. **Do not treat `nixos-unstable` as the only supported base** unless prepared to pay the tax. `README.adoc:19` — *"Mobile NixOS is only expected to build successfully against the **unstable** branch of Nixpkgs"* — plus a HEAD commit titled `fix/nixpkgs-2026-07` tells you the steady-state maintenance load.

---

## 11. Maintenance reality check

Worth stating plainly, because it calibrates how much of the above to trust.

- **The tree has shrunk.** `devices/` currently holds 12 entries: `acer-juniper`, `acer-lazor`, `asus-dumo`, `lenovo-krane`, `lenovo-wormdingler`, `motorola-potter`, `oneplus-enchilada`, `oneplus-fajita`, `pine64-pinephone`, `pine64-pinephonepro`, `pine64-pinetab`, `uefi-x86_64`. Only **three** are Android-boot donor devices, and one (`motorola-potter`) is `supportLevel = "broken"`. What survived is Chromebooks and PINE64 — mainline kernels, cooperative bootloaders. **The Android-donor path is the least-exercised part of the codebase, and it is exactly the path spatial-os needs.**
- **CI covers very little.** `release.nix`'s `tested` aggregate is `uefi-x86_64`, `motorola-potter`, `asus-dumo`, the `hello`/`phosh` examples, and the cross canaries (`release.nix:289-329`). `README.adoc:47-50`: *"There is no published artifacts for the time being."*
- **But it is not dead.** HEAD is 2026-07, the pin is current, PinePhone tracks a 2025-10 kernel. Active maintenance, narrow device set.
- **Cross-compilation is real but partial.** `modules/system-target.nix` auto-derives `nixpkgs.buildPlatform` when host ≠ local (with a `tryEval` on `config.nixpkgs.localSystem` to tolerate pure/flake evals), `release.nix:101-113` builds aarch64 and armv7l from x86_64, and dedicated "cross-canary" derivations are aggregated into a Hydra job (`release.nix:264-278`). Where it breaks, it breaks in userspace: `modules/cross-workarounds.nix` force-disables all NetworkManager plugins under cross and *neuters* `btrfs-progs` entirely on armv7l with a `lib.warn`. Expect cross to cover kernel + initrd + base system and to need native aarch64 builders (or binfmt) for a full desktop stack. For spatial-os — mostly aarch64, one x86_64 target — plan native aarch64 builders as the primary path, cross as the fast-iteration path.

---

## 12. Tow-Boot: the boot-chain half

### Architecture

28 boards in `boards/<vendor>-<board>/default.nix`, same modules-system shape as mobile-nixos but with `Tow-Boot.*` / `device.*` / `hardware.*` namespaces. A board declares (`boards/pine64-pinephonePro/default.nix`, 68 lines):

- `device.{manufacturer,name,identifier,productPageURL,supportLevel}` (`modules/device.nix`; `supportLevel` has **no default** — "the eval should fail if unset").
- `hardware.soc`, plus the two capability facts that drive everything: `hardware.SPISize` (non-null ⇒ build the SPI variant) and `hardware.mmcBootIndex` (non-null ⇒ build the eMMC-boot variant) (`modules/hardware/default.nix:38-53`, `modules/build.nix:10-12`).
- `Tow-Boot.defconfig` — the *upstream* U-Boot defconfig name.
- `Tow-Boot.config` — a list of structured-config functions, identical in shape to `mobile.kernel.structuredConfig` (`modules/kconfig.nix`).
- Documentation fragments.

The SoC module (`modules/hardware/rockchip/default.nix`) supplies what a board author shouldn't have to know: the ATF `BL31` blob, firmware partition offset/length arithmetic, SPI-specific kconfig, and crucially the **`installPhase`** — the actual byte layout of the firmware image:

```99:105:references/tow-boot/modules/hardware/rockchip/default.nix
            (mkIf (variant != "spi") ''
              echo ":: Preparing single file firmware image for shared storage..."
              (PS4=" $ "; set -x
              dd if=idbloader.img of=Tow-Boot.$variant.bin conv=fsync,notrunc bs=$sectorSize seek=$((partitionOffset - partitionOffset))
              dd if=u-boot.itb    of=Tow-Boot.$variant.bin conv=fsync,notrunc bs=$sectorSize seek=$((secondOffset - partitionOffset))
              cp -v Tow-Boot.$variant.bin $out/binaries/
              )
            '')
```

That is the right factoring: **board = facts, SoC = byte layout.**

### Variants and the `composeConfig` pattern

`Tow-Boot.variant ∈ { noenv, spi, mmcboot, boot-installer }` (`modules/tow-boot/options.nix:104-111`, `doc/variants.md`). `modules/build.nix` re-evaluates the *same board config* once per variant via `config.helpers.composeConfig` — a thin wrapper over NixOS `evalConfig` reusing the parent's `baseModules` and `modules` (`modules/helpers.nix:33-62`) — then assembles them into one output directory with binaries, configs, diffs, `shared.disk-image.img`, `spi.installer.img`, `mmcboot.installer.img`.

Installer images are themselves Tow-Boot disk images running the `noenv` variant, containing a `boot.scr` menu, a `flash.scr`, and the payload binary for the *target* variant (`modules/tow-boot/installer.nix:436-460`). Bootstrapping handled cleanly.

### Installation strategies, and the mapping to Qualcomm ABL

`doc/getting-started.md` and `doc/in-depth/firmware-storage-map.md` define two.

**Dedicated storage** — SPI flash or eMMC hardware boot partitions. Menu-driven installer; the firmware owns the whole medium; the OS never has to keep a magic byte range intact.

**Shared storage** — a *protective GPT partition* on the same medium as the OS, `requiredPartition = true`, with a Tow-Boot-owned type GUID, citing EBBR chapter 4.1.1:

```64:89:references/tow-boot/modules/tow-boot/disk-image.nix
      firmwarePartition = {
        name = "${config.Tow-Boot.outputName}.${config.device.identifier}.bin";
        partitionLabel = "Firmware (Tow-Boot)";
        # > Protective partitions are entries in the partition table that cover
        # > the LBA region occupied by firmware and have the 'Required Partition'
        # > attribute set.
        # — EBBR chapter 4.1.1
        requiredPartition = true;
        # ...
        partitionType = lib.mkDefault (
          if config.Tow-Boot.diskImage.partitioningScheme == "gpt"
          then "67401509-72E7-4628-B1AF-EDD128E4316A"
          else "F8"
        );
        raw = lib.mkIf config.Tow-Boot.writeBinaryToFirmwarePartition "${config.Tow-Boot.outputs.firmware}/binaries/Tow-Boot.noenv.bin";
```

**Mapping to an unlocked Qualcomm headset chainloading U-Boot from ABL:** the donor's eMMC/UFS already has a GPT full of vendor partitions, and ABL will load a boot-format image from a named partition. That is structurally *dedicated storage* — a fixed, named, size-bounded region owned by the firmware, distinct from the OS filesystem. What transfers directly: (i) a `hardware.*` capability fact naming the target partition, analogous to `SPISize`/`mmcBootIndex`; (ii) the variant split between "firmware installed to its own home" and "firmware retrofitted into shared space"; (iii) the installer-as-a-bootable-image with a board-identity check; (iv) the erase-head-first write ordering. What does *not* transfer is the environment-storage scheme (`doc/variants.md`) — on a headset the U-Boot environment lives in its own small partition, or is `noenv`.

### Division of responsibility with mobile-nixos

Tow-Boot owns SoC bring-up, DRAM init, ATF/TF-A, boot menu, boot-source priority, USB mass-storage/DFU exposure, the *firmware* update mechanism, and presenting a uniform (roughly EBBR / distro-boot) interface upward. Mobile NixOS owns everything from `boot.scr` onward: kernel, DTB, initrd, rootfs, generation selection, OS updates.

The contract is thin and legible: mobile-nixos' `u-boot` system type writes `boot.scr` and `mobile-nixos/{boot,recovery}/…` into an ext4 partition labelled `boot` and assumes some U-Boot will find it. Mobile NixOS never builds U-Boot. Tow-Boot never builds a kernel for the OS — though it does build a tiny Celun-based Linux for its graphical touch installer (`tow-boot/embedded-linux-os/`, `tow-boot/modules/tow-boot/installer.nix:414-431`).

**spatial-os should copy this seam:** a separate, small, independently-versioned "spatial-boot" component handling ABL chainloading, U-Boot/EDK2, and firmware installation/update, with the OS image builder depending on nothing but "a boot environment that can load a kernel + initrd + DTB from a known partition."

---

## 13. Open questions

1. **Boot image header v3/v4 and `vendor_boot`.** Neither repo handles it. Does spatial-os vendor AOSP's `mkbootimg.py`/`unpack_bootimg.py`, or write a Nix-native packer? How do `vendor_ramdisk` fragments and `bootconfig` interact with a NixOS initrd? (Gap relative to `overlay/mkbootimg/`, `system-types/android/bootimg.nix`.)

2. **Blob acquisition for XR-specific hardware.** The donor-image pattern assumes a downloadable vendor image with a stable URL. Quest/Pico firmware is neither freely downloadable nor stably hosted. Does the runtime-`/vendor`-mount approach become the *primary* mechanism, and if so, how do we characterise reproducibility when a key input lives on the user's device?

3. **Where does `stage-0`-style kexec fit?** It is the only mechanism either repo offers for updating a kernel without reflashing, and it is used by no device. Is chainloading U-Boot from ABL (Tow-Boot's model) a better answer, or do we need both — kexec as the fallback where ABL chainloading isn't achievable?

4. **A/B slots.** mobile-nixos models A/B as three booleans plus a mark-successful unit (`system-types/android/default.nix:89-126`, `modules/boot-control.nix`). Headsets that ship A/B give us real rollback — but does spatial-os drive slot switching itself (write inactive, flip, confirm) or defer to the vendor bootloader? What counts as a *successful* boot on a device with no display until compositor start?

5. **Kernel sharing across a family.** Can one XR2 kernel + per-device DTB serve multiple headsets, as the family model assumes? Or do vendor forks diverge enough per device that `family` degenerates to "shared metadata only"? This determines whether the abstraction pays for itself.

6. **Growable ext4 vs immutable squashfs/EROFS + overlay.** mobile-nixos assumes a growable ext4 rootfs (`modules/rootfs.nix:115-121`) and has `rootfs-squashfs` plus an `examples/hello-but-squashfs` as the alternative. For fixed-hardware, appliance-like headsets, is an immutable, verity-able image the better default?

7. **Steam Frame's place in the taxonomy.** x86_64, SteamOS-derived, UEFI. Does it become a fourth system type alongside the Android/U-Boot ones, or is the boot-chain diversity great enough that "system type" should be replaced by a pluggable boot-artifact interface?

8. **Signing.** Depthcharge images here are signed with public devkeys (`system-types/depthcharge/default.nix:81-83`); Android images are unsigned. Unlocked headsets mostly don't verify — but for AVB-enforcing bootloaders, or user-owned keys, where does key management live in the build?

9. **Who owns the display/tracking hardware description?** mobile-nixos has `mobile.hardware.screen.{width,height}` and a pile of framebuffer quirks (`modules/quirks/framebuffer.nix`, `modules/quirks/qualcomm/msm-fb-notify.nix`). An XR distro needs per-device lens/panel/IMU/tracking metadata as a first-class concern. Is that a `spatial.hardware.hmd.*` namespace in the module system, or external data (à la a Monado device database) that the build merely packages?
