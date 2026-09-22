# spatial-os architecture: image assembly and updates

**Status:** draft. Derived from [00-synthesis](../research/00-synthesis.md) §2.4/§2.5,
[04-nix-imaging](../research/04-nix-imaging.md) §6/§7, [01-mobile-nixos](../research/01-mobile-nixos.md)
§6, and [07-device-landscape](../research/07-device-landscape.md).

## Two image families, one interface

The device landscape forces (at least) two image families, selected by
`spatial.deployment.bootScheme`:

- **`uefi-rauc`** — Steam Frame (SM8650, SteamOS-class): a GPT/UEFI disk with A/B system partitions,
  built with nixpkgs `image.modules` + systemd-repart, updated with RAUC + casync.
- **`android-bootimg`** — Quest 1, Lynx R1, Galaxy XR, PFDM (Qualcomm Android donors): Android boot
  images (`boot`/`vendor_boot`/`init_boot` per verified header version), AVB metadata, DTBO, and
  rootfs targeted at the donor's partition scheme (including dynamic `super`).
- **`abl-uboot`** (future) — where an unlocked ABL chainloads U-Boot; a variant of the Android family
  on the boot side with a UEFI-like OS side (Tow-Boot's dedicated-storage model,
  [01](../research/01-mobile-nixos.md) §12).

All families implement the same `lib/images/` interface: they are **deferred modules injected into
nixpkgs `image.modules`**, each defining `system.build.image` and the standard
`image.baseName`/`filePath` ([04](../research/04-nix-imaging.md) §9 item 6). This gains
`nixos-rebuild build-image` and per-variant introspection for free, and matches the `extendModules`
fan-out that nixos-generators pioneered and nixpkgs now implements natively.

## Assembly: build on systemd-repart, not VMs or legacy tools

For disk/UEFI/RAUC targets, use nixpkgs `image.repart` ([04](../research/04-nix-imaging.md) §6.2):
rootless, VM-less, binfmt-less, cross-clean (`buildPackages.systemd`, `--architecture=`),
deterministic partition-UUID seed, `split = true` for per-partition artifacts, `mkfsOptions` for fs
tuning, and `verityStore` for measured images. Explicitly **not** the VM-based `make-disk-image.nix`
(slow, KVM-dependent, cross-hostile) and **not** Mobile NixOS's bespoke image-builder with its
empirical ext4 fudge table ([01](../research/01-mobile-nixos.md) §6.1).

For **Android boot chains**, write a Nix-native declarative wrapper over AOSP's *current*
`mkbootimg.py` / `unpack_bootimg.py` / `avbtool` / `lpmake` (all in nixpkgs `android-tools`) — never
the 2020 mkbootimg fork Mobile NixOS pins. The functional spec is UBports'
`make-bootimage.sh` behaviour ([03](../research/03-android-compat.md) §6.2): header v0–v2 single
image; v3/v4 split kernel/ramdisk + `vendor_boot` with offsets and DTB; v4 vendor-ramdisk fragments
and `--vendor_bootconfig`; `init_boot` for Android-13-launch devices; AVB hash-footer with
`SEANDROIDENFORCE` tail or `avbtool add_hash_footer`. Header version and offsets come from the donor
via `unpack_bootimg`, recorded in the device contract, never hard-coded.

Bare filesystem payloads (a `system.img`-style ext4/erofs for fastboot into a dynamic partition) use
nixpkgs `make-ext4-fs` / `erofs-store-image` as leaf tools. Dynamic `super` handling (regenerate LP
metadata vs. overwrite whole `super`) is an open question carried from
[02](../research/02-postmarketos.md) §11 item 1.

## The reference-free flashing bundle

Every device produces a **reference-free flashing bundle**: a directory of `.img` files + a
machine-readable install manifest (hashes, provenance, recovery instructions) + a flash script that
uses **bare tool names** so it tars up and runs from a non-Nix laptop
([01](../research/01-mobile-nixos.md) §6.3, meta-qcom's "factory restore bundle"
[02](../research/02-postmarketos.md) §9 item 11). A declarative flasher table (pmOS pattern,
[02](../research/02-postmarketos.md) §6) maps `spatial.deployment.flashMethod` → argv templates
resolved from the contract, including `flash_vbmeta` (avbtool verification-disable where the device is
unlocked) and `flash_dtbo`.

## Build never flashes

Non-negotiable ([overview.md](overview.md) invariant 1): `nix build` produces images and bundles; it
never writes to a block device. The **installer** is a separate stage that independently verifies
model, firmware prerequisites, partition layout, slot state, and bootloader conditions before
writing, and:
- refuses to run on a mismatched board (Tow-Boot identity check,
  [01](../research/01-mobile-nixos.md) §12);
- protects `spatial.deployment.protectedPartitions` (persist/calib/NV/identity) unless a separately
  reviewed operation explicitly touches them;
- for single-slot devices, documents a different recovery guarantee than an A/B device — it does not
  advertise the same atomicity.

Unlocking and verified-boot policy are device constraints producing correct images never bypasses.

## Updates: two backends behind one transaction

The release transaction covers the compatible set of **boot artifacts + kernel/modules + rootfs +
hardware adaptation + state-schema expectations** — not just the rootfs. Two backends implement it:

### RAUC + casync (uefi-rauc devices)
Steam Frame already uses this ([07](../research/07-device-landscape.md),
[06](../research/06-donor-pipeline.md) §2.2), all tooling is in nixpkgs, and the bundle format is
simple to generate from a Nix-built rootfs derivation. A/B slots via RAUC; `format=plain` bundles
carrying `.caibx` indexes; chunks served from a dumb HTTPS store; device-side seeding from the
installed slot for cheap deltas. Unlike Valve's rootfs-only bundle, spatial-os RAUC manifests are
multi-image where the device also has spatial-os-managed boot/ESP partitions. Deterministic chunking
is the one reproducibility cost to design for.

### Android-slot backend (android-bootimg devices)
A/B where the device has slots (drive slot switching: write inactive, flip, confirm — plus a
mark-successful unit, pmOS `qbootctl` / Mobile NixOS `boot-control` pattern). Where there is no A/B,
droid-hal's "flash on package upgrade via a pre-init oneshot, upgrade-only" semantics
([03](../research/03-android-compat.md) §7) — but treated as a distinct, lower recovery guarantee.
Android's dynamic-partition / Virtual-A/B constraints mean a generic partition writer is unsafe here;
the backend is slot-scheme-aware.

### Health-gated success
An update is marked successful only after a **hardware-aware readiness check**: the intended kernel
booted, the adaptation services started, and the XR path passed `spatial.qualification.readinessCheck`
— not merely "the kernel booted." Rollback must account for mutable-data migrations and AVB
rollback-protection (arbitrary downgrades cannot be promised).

## Development vs. release

For development, ordinary NixOS generations (`nixos-rebuild`, Jovian's model) are the fast path —
including the `virtual-headset` VM smoke target. For a consumer headset release, the default is an
immutable A/B image with a boot-integrated update transaction. One device may offer both channels
from one module tree; keeping them from diverging is an open question
([04](../research/04-nix-imaging.md) §11 item 2).

## Reproducibility of images

Distinguish pinned-graph / reproducible-assembly / reproducible-source-build / runtime-equivalence
([00-synthesis](../research/00-synthesis.md) §7 of the recommendation, and
[06](../research/06-donor-pipeline.md) §6). For images specifically: fixed partition-UUID seed,
`SOURCE_DATE_EPOCH`/mtime clamping, single-threaded compression, fixed filesystem UUIDs/labels/hash
seeds, and — as release evidence — an independent rebuild with byte comparison, not a re-download of
the same cached output. Signing happens in a separate controlled stage with keys outside the store;
test-key-signed artifacts remain cacheable.
