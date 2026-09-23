# ADR 0001: Single flake monorepo, boot firmware as the only seam

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [00-synthesis](../../research/00-synthesis.md),
[01-mobile-nixos](../../research/01-mobile-nixos.md) §2/§10,
[04-nix-imaging](../../research/04-nix-imaging.md) §2.

## Context

Mura spans a device/SoC/family module system, a donor pipeline, image builders, an XR stack,
and many patches. Should this be one repository or several? The user explicitly flagged "sound
project structure (git mono/super project?)" as a requirement, expecting many customisations and
patches.

The reference projects split differently: postmarketOS uses two repos (`pmbootstrap` tool +
`pmaports` data); Mobile NixOS + Tow-Boot are two repos by the same author with a thin contract, but
they **duplicate the image-builder** between them ([01](../../research/01-mobile-nixos.md) §10 item 9);
robotnix and Jovian are each single repos.

## Decision

**One flake-based monorepo.** All layers share one pinned nixpkgs and one evaluation. Large upstream
sources (nixpkgs, Monado, kernels, AOSP) enter as flake inputs or hash-pinned fetches, never vendored.
Reference study clones live under a git-ignored `references/` reproducible from a tracked
`clone.sh` + `MANIFEST.json`.

The **single deliberate seam is boot firmware**: if an `abl-uboot` boot component grows enough to
warrant it, it becomes a separate flake input with a thin contract ("provide a boot environment that
loads kernel+initrd+DTB from a known partition"), mirroring the clean Mobile-NixOS↔Tow-Boot boundary
— but shared infrastructure (image builder, donor pipeline, contract lib) is never duplicated across
that seam.

## Rationale

- The device→SoC→family→common decomposition is a single module-system evaluation; splitting it
  across repos adds cross-repo version coordination for no isolation benefit at this scale.
- Nix's cache boundaries already give the isolation that multi-repo splits are often reached for:
  changing the shell doesn't rebuild the kernel; changing a kernel patch doesn't re-extract a donor
  ([00](../../research/00-synthesis.md) §7 recommendation §6).
- The concrete failure mode of the two-repo approach in the corpus is duplication
  ([01](../../research/01-mobile-nixos.md) §10 item 9), which a monorepo avoids by construction.

## Consequences

- One `flake.lock`, one `nix flake check`, one place to reason about pins.
- Patch management, update automation, and CI are all repo-wide (see
  [repo-structure.md](../repo-structure.md)).
- If boot firmware is split later, the image builder and donor pipeline stay in the monorepo and the
  boot component consumes them as inputs — not the reverse.

## Alternatives considered

- **pmOS-style tool + data split:** rejected; the "tool" is Nix itself, and the "data" is typed
  modules that must co-evaluate with the tool's libraries.
- **Per-device repos:** rejected; defeats the shared common distribution and family reuse, and
  multiplies pin/CI overhead by device count.
