# ADR 0002: NixOS is the default runtime; kernel contract gates it; non-NixOS userspace is a documented escape hatch

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [00-synthesis](../../research/00-synthesis.md) §2.2,
[03-android-compat](../../research/03-android-compat.md) §5.2/§10.2,
[07-device-landscape](../../research/07-device-landscape.md), and the local brick appliance prior art.

## Context

"Nix as builder" and "NixOS as runtime" are separable choices. The brick appliance deliberately built
a BusyBox+musl init with Nix (not NixOS) because its vendor kernel was Linux 4.9 and current systemd
needs ≥5.10. Halium shows the same cliff from the other side (systemd ≥217 needs kernel ≥3.10, and
distro security policy propagates into kconfig — [03](../../research/03-android-compat.md) §5.2,
§10.2). The user prefers NixOS. Can NixOS be the runtime on these headsets?

The device landscape ([07](../../research/07-device-landscape.md)) is *SoC-level* favorable, but SoC
enablement is not headset enablement — a distinction the research is careful about and this ADR must
not blur. Per-device **evidence levels** (not a blanket "mainline-capable" claim):

| Device | SoC-supported | Board boots | Display proven | Tracking proven |
|---|---|---|---|---|
| Lynx R1 (SM8250) | yes (mature linux-msm) | yes (pmOS debug shell on 6.13) | no | no |
| Quest 1 (MSM8998) | yes (mature linux-msm) | no public Monterey DTS | no | no |
| Steam Frame (SM8650) | yes (mainline-ish SteamOS) | vendor OS boots; alt-OS policy unknown | vendor only | vendor only |
| Galaxy XR / PFDM (~SM8550) | adjacent only; silicon/board mapping unverified | no | no | no |

The mainline path is real at the SoC level and demonstrated only to a debug shell, only on Lynx R1.
Everything above "board boots" is an open bet to be settled by a hardware spike, not an established
fact.

## Decision

1. **NixOS is the default runtime**, on a kernel that meets an explicit **kconfig + feature
   contract** (`mura.kernel.contract`).
2. **"NixOS userspace" and "kernel meets the contract" are separately verified.** The contract gate
   (Mobile NixOS's validator-script idea + pmOS's `kconfigcheck` CI gate,
   [00](../../research/00-synthesis.md) §3.3) is a build/eval check that distinguishes errors from
   warnings and fails a device that cannot meet its userspace's requirements.
3. **A Nix-built non-NixOS userspace is a documented escape hatch**, not built now — reserved for a
   strategically important device stuck on an ancient vendor kernel, following the brick appliance's
   BusyBox+musl-init-via-Nix precedent.
4. **Freezing the whole distribution on an old nixpkgs for one headset is explicitly rejected.**

## Rationale

- Putting a newer userspace in a container does not supply missing kernel functionality; replacing
  systemd alone does not address every userspace dependency. A legacy profile is a real maintenance
  obligation, so it must be an exception, not a default.
- The mainline paths in the device landscape mean the exception is unlikely to be needed for the
  priority targets — but the architecture must not assume it away, because vendor kernel provenance
  is uneven ([07](../../research/07-device-landscape.md): Lynx/PFDM/Quest-3 kernel sources are
  unverified or missing).
- Making the contract a first-class gate means "this kernel can run this userspace" is a checked
  fact, not a hope, and it doubles as the tier-`booting` requirement.

## Consequences

- Every device declares `mura.kernel.contract`; CI runs it against the built `.config`.
- The default image pipeline targets NixOS closures (see [images-and-updates.md](../images-and-updates.md)).
- The escape-hatch profile is scoped and documented before it is ever built; it is not a compatibility
  flag.

## Alternatives considered

- **Nix-as-builder-only, never NixOS:** rejected; throws away the module ecosystem, `image.modules`,
  sysupdate, and the update tooling for a problem the device landscape mostly doesn't have.
- **NixOS unconditionally:** rejected; ignores the real kernel-version cliff the brick appliance and
  Halium both hit.
