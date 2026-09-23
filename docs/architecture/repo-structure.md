# spatial-os architecture: repository structure and patch management

**Status:** draft. Derived from [00-synthesis](../research/00-synthesis.md) and the layout precedents
in [01-mobile-nixos](../research/01-mobile-nixos.md) §2/§3, [02-postmarketos](../research/02-postmarketos.md)
§2, and [04-nix-imaging](../research/04-nix-imaging.md) §2/§9.

## Monorepo, with vendored sources kept out

spatial-os is a **single flake-based monorepo**. The device/SoC/family decomposition, the module
system, and the image builders all share one evaluation and one pinned nixpkgs; splitting them across
repos would reproduce the Mobile-NixOS/Tow-Boot duplication (their image-builder is copied between
repos — [01](../research/01-mobile-nixos.md) §10 item 9) with no benefit at this scale. The one
deliberate seam is **boot firmware**: if an `abl-uboot` boot component grows, it becomes a separate
flake input (the thin Mobile-NixOS↔Tow-Boot contract, [01](../research/01-mobile-nixos.md) §12), not
a second copy of shared infrastructure. See [adr/0001](adr/0001-monorepo-vs-subprojects.md).

Large upstream sources (nixpkgs, Monado, kernels, AOSP trees) are **never vendored into the tree**;
they enter as flake inputs or hash-pinned fetches. Reference clones for study live under
`references/` and are git-ignored, reproducible from `references/clone.sh` + `references/MANIFEST.json`.

## Layout

```text
flake.nix                      # pinned inputs, dev shell, checks, per-device outputs
flake.lock
lib/
  images/                      # image-variant deferred modules (image.modules), repart + android packer
  donor/                       # acquire/identify/parse/extract/qualify derivation builders
  contract/                    # device-contract option types + assertions
modules/
  os/                          # common distribution policy (device-independent)
  xr/                          # Monado runtime, session, StardustXR shell wiring
  adaptation/                  # per-subsystem backend options: native | android-backed | device-specific
    android-compat/            # libhybris/android-headers/late-LXC building blocks (optional)
soc/
  msm8998/  sm8250/  sm8550/  sm8650/   # shared per-SoC integration (kernel base, firmware paths, DSP stack)
families/                      # shared definitions for near-identical models (plain imports)
devices/
  virtual-headset/             # x86_64 VM smoke target (no hardware)
  <vendor>-<model>/            # device contract, donor manifest, kernel cfg, patches, tests, contract file
pkgs/                          # overlay: XR components, kernels, tools (nixpkgs-xr pulled as input)
patches/                       # patch sets, organized per upstream + per donor build (see below)
protocols/                     # spatial-os Wayland protocol XMLs (zxr-shell-v2, the zspatial
                               # shell-integration family incl. zspatial-toplevel-export) + governance
                               # notes + CONVENTIONS.md; CI: wayland-scanner + xmllint (tests/protocols.nix)
specs/                         # normative non-Wayland contracts (IPC framings, storage formats,
                               # D-Bus/PipeWire interfaces) — the peer of protocols/
tests/                         # eval assertions, VM tests, reproducibility + hardware tests
contracts/                     # reviewed, hash-bound donor contracts (the qualify-stage gate)
docs/
  research/                    # the seven research docs + synthesis
  architecture/                # this set
references/                    # git-ignored study clones (clone.sh + MANIFEST.json tracked)
```

This mirrors the convergent device→SoC→family→common decomposition ([00](../research/00-synthesis.md)
§1) and Jovian's device-module-with-capability-flags layout ([04](../research/04-nix-imaging.md) §3),
which was the cleanest of the five Nix projects studied.

## The flake entry point

One integration path (avoiding Mobile NixOS's two-entry-point wart —
[01](../research/01-mobile-nixos.md) §10 item 4):

- `nixosModules.default` — the `spatial.*` module set, usable in any NixOS config.
- `spatialSystem = { device, ... }: …` — mirrors robotnix's `lib.robotnixSystem`
  ([04](../research/04-nix-imaging.md) §9 item 1); evaluates a device into its artifacts.
- `packages.<system>.<device>-<variant>` — named, discoverable flake outputs (no untyped
  `build = types.attrs` grab-bag — [04](../research/04-nix-imaging.md) §10 item 9).
- `checks.<system>.*` — `nix flake check` wiring: formatting, contract assertions, the
  `virtual-headset` build, per-donor golden-hash tests, tier-consistency checks.

A flake/non-flake bridge (Jovian's `nixpkgs.nix`, [04](../research/04-nix-imaging.md) §9 item 13)
keeps both consumption modes on identical pins.

## Patch management (many patches expected)

The project will carry many patches — kernel trees, Monado per-device drivers, occasional vendor
fixups. The corpus is emphatic about how this goes wrong: robotnix's per-vendor-branch patch forests
multiply rebase work at every upstream release ([04](../research/04-nix-imaging.md) §10 item 2).
Rules:

1. **Prefer config over patches.** Jovian reproduces almost all of Valve's kernel via
   `structuredExtraConfig` + `mkForce` and cmdline flags rather than source patches
   ([04](../research/04-nix-imaging.md) §5, §9 item 5). Reach for a patch only when config cannot
   express the change.
2. **Patches are pinned data with provenance.** Each patch series lives under `patches/<upstream>/`
   or `patches/<donor-buildid>/`, applied via `applyPatches`/`FetchContent`-equivalent at build time.
   The XR per-device driver is a `monado-rev` file + `patches/monado/<device>/*.patch` — exactly
   WiVRn's proven 11-patch pattern ([05](../research/05-xr-userspace.md) §9 item 2).
3. **Isolate per-donor-release patch directories** with automated rebase checking in the update
   pipeline, so a donor bump surfaces broken patches loudly
   ([04](../research/04-nix-imaging.md) §10 item 2).
4. **Ship the diff as a release artifact.** Following Tow-Boot, emit `savedefconfig`, the original
   and final `.config`, and a unified diff of every patch applied to a source tree
   ([01](../research/01-mobile-nixos.md) §9 item 12) — outstanding practice for a project that
   patches vendor trees.
5. **Upstream as exit strategy.** Track which patches are candidates for upstreaming (Monado
   explicitly invites driver upstreaming; nixos-apple-silicon shrank to kernel+U-Boot by upstreaming
   — [04](../research/04-nix-imaging.md) §9 item 11, §11 item 8). Layout separates overlay-per-
   component so a piece can be deleted when it lands upstream.

## Input pinning and update automation

At fleet scale (6+ headsets × {kernel, firmware, blobs, XR components}), manual hash bumps drown you
(Jovian's constants-in-file is fine for one device — [04](../research/04-nix-imaging.md) §10 item 8).
The design:

- **Lockfile-driven ingestion** for every donor input class, produced by tooling not typed by hand
  (robotnix `repo2nix` + `update.sh`; the un-automated parts of robotnix are exactly what rotted
  during its 3-year gap — [04](../research/04-nix-imaging.md) §2.1, §9 item 2).
- **nixpkgs-xr as a flake input** for the XR stack, reusing its nvfetcher daily-cron pin architecture
  (including the WiVRn→Monado cross-pin scrape) and cachix cache
  ([05](../research/05-xr-userspace.md) §2.4, §9 item 3).
- **Scheduled bump PRs** (nvfetcher/Renovate) for spatial-os's own pins.
- **Release/tracking discipline** (nixos-apple-silicon, [04](../research/04-nix-imaging.md) §9 item
  11): `main` on nixos-unstable, `release-YY.MM` per NixOS stable, dated tags with CI-built artifacts
  and a written on-hardware test protocol.

## Caches and redistribution boundaries

- Public artifacts (common userspace, XR components, kernels) are cacheable and should have a public
  binary cache (apple-silicon and robotnix both needed one for kernel/Mesa-class builds).
- Donor-derived artifacts (extracted vendor firmware) are legally non-redistributable: the
  `licensing.redistributable = false` flag mechanically forces `allowSubstitutes = false;
  preferLocalBuild = true` and exclusion from cache-push ([06](../research/06-donor-pipeline.md) §6).
  This likely means a private cache tier per device for its qualified-donor outputs.
- Signing keys live outside the store; a separate controlled stage signs qualified artifacts
  ([04](../research/04-nix-imaging.md) §9 item 8).
