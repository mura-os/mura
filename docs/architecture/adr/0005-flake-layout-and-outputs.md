# ADR 0005: One flake integration path, typed named outputs, nixpkgs-xr as input

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [04-nix-imaging](../../research/04-nix-imaging.md) §9,
[01-mobile-nixos](../../research/01-mobile-nixos.md) §2/§10,
[05-xr-userspace](../../research/05-xr-userspace.md) §9.

## Context

How does the flake expose itself? Mobile NixOS ships **two** divergent integration entry points
(modules-into-`baseModules` via `default.nix`, and import-into-user-config via
`lib/configuration.nix`) with subtly different semantics — a wart the research flags explicitly
([01](../../research/01-mobile-nixos.md) §10 item 4). robotnix exposes a clean
`lib.robotnixSystem`-shaped entry but buries outputs in an untyped `build = types.attrs` grab-bag,
making them undiscoverable ([04](../../research/04-nix-imaging.md) §10 item 9). nixpkgs-xr already
solves XR-stack pinning and cross-pinning ([05](../../research/05-xr-userspace.md) §2.4).

## Decision

- **One integration path.** `nixosModules.default` (the `spatial.*` module set) plus
  `spatialSystem = { device, ... }: …` (mirrors `lib.robotnixSystem`,
  [04](../../research/04-nix-imaging.md) §9 item 1). No second entry point; any convenience alias is
  thin.
- **Typed, named, discoverable outputs:** `packages.<system>.<device>-<variant>` and
  `checks.<system>.*`. No untyped output grab-bag.
- **`checks` wire `nix flake check`:** treefmt formatting, contract assertions (typed-option +
  cross-field), the `virtual-headset` build, per-donor golden-hash tests, and device-tier consistency
  checks.
- **nixpkgs-xr is a flake input**, reused for the XR stack (its nvfetcher pins including the
  WiVRn→Monado cross-pin, and its cachix cache) rather than re-packaging Monado/WiVRn from scratch
  ([05](../../research/05-xr-userspace.md) §9 item 3).
- **Flake/non-flake bridge** (parse `flake.lock`, `fetchTarball` by rev+narHash — Jovian's
  `nixpkgs.nix`, [04](../../research/04-nix-imaging.md) §9 item 13) keeps both consumption modes on
  identical pins.
- **`main` tracks nixos-unstable; `release-YY.MM` branches pin NixOS stable**
  ([04](../../research/04-nix-imaging.md) §9 item 11).

## Rationale

- A single entry point removes the class of bug Mobile NixOS demonstrates (config that behaves
  differently depending on how it was integrated).
- Typed named outputs make `nix flake show` truthful about what a device can build — the brick
  appliance's null-propagation gating already proves the value of "outputs reflect what's authorized"
  ([06](../../research/06-donor-pipeline.md) §2.1.2).
- Reusing nixpkgs-xr avoids re-solving XR pinning and inherits its daily-bump automation.

## Consequences

- Device outputs appear/disappear based on donor+contract availability (null-propagation), matching
  the brick pattern.
- CI is the enforcement point for contract validity, reproducibility golden hashes, and tier claims.
- The XR stack's update cadence is largely delegated to nixpkgs-xr; spatial-os pins it and layers
  per-device Monado patches on top.

## Alternatives considered

- **Two entry points (Mobile NixOS):** rejected; the documented wart.
- **Repackage the XR stack from scratch:** rejected; nixpkgs-xr already does it with better update
  automation than we would start with.
