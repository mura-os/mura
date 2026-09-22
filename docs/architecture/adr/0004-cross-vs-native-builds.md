# ADR 0004: Native aarch64 builders primary, cross for kernels/leaf tools, binfmt as fallback

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [04-nix-imaging](../../research/04-nix-imaging.md) §8 (cross-cutting),
[01-mobile-nixos](../../research/01-mobile-nixos.md) §11,
[00-synthesis](../../research/00-synthesis.md) §7.

## Context

Targets are mostly aarch64 (all Qualcomm headsets) plus one x86_64-adjacent aarch64 case (Steam
Frame is also aarch64). The aarch64 NixOS closures must be built somehow. The corpus shows every
option and its cost: Mobile NixOS cross-builds kernel+initrd+base but **neuters parts of the
graphics-rich userspace under cross** (`btrfs-progs` disabled on armv7l, NetworkManager plugins
force-disabled — [01](../../research/01-mobile-nixos.md) §11); nixos-apple-silicon proves
installer-grade cross (x86_64→aarch64) works but pays ongoing maintenance
([04](../../research/04-nix-imaging.md) §8); repart assembles foreign-arch images **without** binfmt
because nothing target-arch executes ([04](../../research/04-nix-imaging.md) §8).

## Decision

- **Native aarch64 builders are the primary path** for the graphics-rich common userspace. Candidates:
  Apple Silicon machines running nixos-apple-silicon, aarch64 cloud builders, or the target hardware
  itself once bootstrapped.
- **Cross-compilation** (nixpkgs `crossSystem`/`pkgsCross`) for components where it is well supported
  — kernels especially (Jovian/Asahi `buildLinux` patterns cross cleanly), and leaf image tools.
- **systemd-repart image assembly needs neither** — build-platform systemd assembles target-arch
  filesystems with `--architecture=` and nothing target-arch runs
  ([04](../../research/04-nix-imaging.md) §6.2, §9 item 7).
- **binfmt qemu-user on x86_64 farms is a fallback** for the occasional package that neither crosses
  nor has a native builder available — accepted as slow and occasionally wrong, never the default for
  release builds.
- **The one unavoidable foreign build** (bionic half of libhybris compat layers, when an
  `android-backed` subsystem is enabled) runs via robotnix's recipe: `unshare -m -r` +
  `mount --bind` + `buildFHSEnv` + `fakeuser`, using AOSP's own prebuilt (x86_64-hosted) toolchains
  that emit ARM — so Nix does not cross-compile it at all ([04](../../research/04-nix-imaging.md)
  §9 item 9).

## Rationale

- Cross-building the entire graphics-rich userspace is explicitly *not* assumed to be the
  lowest-maintenance route (Mobile NixOS's cross workarounds are the evidence). Native-target
  evaluation with a real cache is more reliable for the big closure.
- Kernels and leaf tools cross well and are where cross pays off (decoupled, cacheable).
- repart removing the image-assembly arch problem is a major simplification worth building on.

## Consequences

- CI needs at least one native aarch64 builder; the release protocol assumes it.
- A public binary cache for the aarch64 common closure is effectively required (apple-silicon and
  robotnix both needed one).
- The Android-compat foreign build is isolated and only triggered when a device actually selects an
  `android-backed` subsystem.

## Alternatives considered

- **Full cross for everything:** rejected as primary; high maintenance for the userspace, per the
  Mobile NixOS evidence. Kept for kernels/tools.
- **binfmt for everything:** rejected as primary; slow and occasionally miscompiles.
