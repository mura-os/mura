# spatial-os architecture: overview

**Status:** draft, derived from [docs/research/00-synthesis.md](../research/00-synthesis.md).
This document defines the layers, the boundaries between them, and the vocabulary the rest of
`docs/architecture/` uses. It is deliberately technology-specific where the research settled a
decision and deliberately open where a hardware spike is still required.

## What spatial-os is

A Nix-built, NixOS-based, Wayland-based Linux XR distribution that targets many standalone VR
headsets by turning **pinned vendor firmware ("donors") + device definitions + pinned sources** into
**reproducible flashable artifacts**. The distribution itself is device-independent; each supported
headset is a versioned hardware-adaptation bundle plus a small typed contract. The whole system is
deterministic: same inputs → same artifacts, with reproducibility treated as tested release
evidence rather than an assumed property of using Nix.

The central design rule, from the synthesis:

> Build the distribution independently of the device, but qualify and deploy it together with an
> explicit, versioned hardware dependency set.

## The four boundaries

```mermaid
flowchart TB
    subgraph app [Application boundary]
        shell["XR shell (StardustXR)"]
        apps["OpenXR apps + 2D Wayland apps"]
        openxr["OpenXR interface"]
    end
    subgraph common [Common distribution - device independent]
        runtime["Monado runtime + compositor (out-of-process)"]
        services["Common services: audio, net, updates, logging, session"]
    end
    subgraph adapt [Hardware boundary - per device adaptation bundle]
        native["Native backend: Mesa/Freedreno, V4L2, IIO, PipeWire, mac80211"]
        android["Android-compat backend (optional, per subsystem): libhybris / HAL bridges / late LXC"]
        xrdrv["Device XR driver: monado-rev + patch series"]
    end
    subgraph hw [Kernel + firmware]
        kernel["Kernel + modules + DTB (buildLinux, contract-gated)"]
        fw["Donor firmware + vendor blobs"]
    end
    subgraph build [Build boundary - Nix]
        donor["Donor pipeline: acquire to identify to parse to extract to qualify"]
        images["Image assembly + update bundles"]
    end

    app --> common --> adapt --> hw
    build -. produces .-> hw
    build -. produces .-> images
    adapt -->|"standard interfaces + device adapters"| hw
```

1. **Build boundary.** Nix turns pinned sources, donor firmware, and device definitions into
   reproducible artifacts. `nix build` never touches hardware; flashing is a separate, human-driven
   step with its own verification. (See [donor-pipeline.md](donor-pipeline.md),
   [images-and-updates.md](images-and-updates.md).)
2. **Hardware boundary.** A per-device adaptation bundle binds kernel + modules + DTB + donor
   firmware + per-subsystem backends + the device's XR driver + hardware config into one qualified
   compatibility unit. (See [device-contract.md](device-contract.md).)
3. **Application boundary.** OpenXR and ordinary Wayland/Linux interfaces keep the shell and
   applications independent of any individual device.
4. **(Implicit) release boundary.** Signing, redistribution gating, and reproducibility evidence
   sit around the build boundary; keys live outside the Nix store.

## The layers, top to bottom

### Application layer (device-independent)
The XR shell/compositor, OpenXR applications, and 2D Wayland applications. Talks only OpenXR and
Wayland. Knows nothing about specific hardware.

The shell is the spatial-os XR compositor — a Wayland-native, client-renders / compositor-composites
design continuing the wxrc `zxr` protocol lineage as `zxr-shell-v2`, itself an OpenXR client of
Monado, serving `xdg-shell` for 2D apps and `zxr-shell-v2` for 3D apps in one depth-tested space.
This resolves the previously-open "2D apps in a headset session" question from
[docs/research/05-xr-userspace.md](../research/05-xr-userspace.md) §11. The decision, its
alternatives (StardustXR and WayVR are packaged as optional sessions, not the backbone), the Vulkan
renderer choice, and the ship-the-2D-tier-first sequencing are recorded in
[adr/0006-compositor-strategy.md](adr/0006-compositor-strategy.md), grounded in research docs
[08](../research/08-wxrc.md), [09](../research/09-wxrc-ecosystem-gap-2026.md), and
[10](../research/10-xr-wayland-protocol-comparison.md). Build-system-wise this layer is packaging +
session policy; the compositor engineering program itself is tracked in the ADR.

### Common distribution layer (device-independent)
Owns the **packaging and session policy** for the shell, the application environment, networking
policy, user management, update policy, logging, diagnostics, security policy, and the **XR
runtime**: Monado running out-of-process and socket-activated, with
`/etc/xdg/openxr/1/active_runtime.json` declared by the module system. The shell *process* runs at
the application boundary (below) as an OpenXR/Wayland client; the common layer decides how it is
packaged, wired into the session, and configured — it does not embed hardware knowledge. "Common"
means a shared package closure per CPU architecture and runtime family, plus a small device-specific
configuration closure — not a byte-identical filesystem on every device.

The **session/login model** is part of this layer: an appliance profile that auto-logs the owner
straight into the XR session with a compositor-integrated lock as the only auth surface, and a
multi-user profile using greetd with the zxr compositor run in a restricted `--greeter` mode.
Because the greeter and lock need the full XR display path (panel, distortion, IPD, IMU tracking)
before any user session exists, per-unit calibration is system state (not `$HOME`) and tracking is
tiered (IMU-only pre-auth, full 6DoF with the session). The decision, the doff/don/idle re-auth
policy, and the lock-as-composition-policy model are in
[adr/0007-session-greeter-lock.md](adr/0007-session-greeter-lock.md), selected via
`spatial.xr.session.*`. On devices whose USB-C can drive a monitor, the same session also offers
**docked desktop mode** — flat presentation on the external display with the XR stack quiesced
while doffed ([adr/0015-docked-desktop-mode.md](adr/0015-docked-desktop-mode.md)).

Realized as NixOS modules under `modules/os/` (distro policy) and `modules/xr/` (runtime + session).

### Hardware-adaptation layer (per device)
Where hardware knowledge lives, below the OpenXR/Wayland interfaces. Three independently selectable
backend kinds **per subsystem** (display/GPU, camera, sensors/IMU, audio, Wi-Fi/BT, DSP tracking):

- **native** — the function is provided by the mainline Linux stack (default posture given the
  device landscape: Mesa/Freedreno, V4L2, IIO, PipeWire, mac80211/BlueZ).
- **android-backed** — an Android HAL/vendor library/service provides it, via libhybris `dlopen`,
  `libgbinder` IPC, or a late-starting, optional LXC container. Never a boot dependency.
- **device-specific** — a dedicated implementation, typically for tracking or display control.

The device's XR driver is part of this layer, expressed as a pinned Monado revision plus a patch
series (Monado has no stable out-of-tree driver ABI). Realized under `modules/adaptation/`, `soc/`,
`families/`, and `devices/<vendor-model>/`.

### Kernel + firmware layer (per device)
A standalone `buildLinux` derivation per device/family (vendor tag + `structuredExtraConfig` with
per-option provenance), a kconfig **contract** gate that verifies the kernel meets the userspace's
requirements, DTBs, and the donor-derived firmware/vendor-blob closure. Kernel builds are decoupled
from image builds so a shell change never rebuilds a kernel.

## The desktop environment: five runtime planes

Orthogonal to the layers above (which sort by *device dependence*), the running system is also
organized into **planes** sorted by *authority*: a **system plane** (greetd, seat brokering,
session lifecycle — ADR 0007), the **authority plane** (the zxr compositor — the only process with
knowledge/control over arbitrary clients), an XR-specific **perception plane** (Monado + the
camera/pose services of ADRs 0008–0011, one clock/calibration domain, never client-visible), a
**shell plane** (presentation clients on privileged protocols), and a **service plane** (D-Bus
session services, portals). Each feature further splits into mechanism / policy / presentation.
The model, the XR redefinitions of desktop vocabulary, and the component dependency graph (hard
edges only — no build order has been chosen) are in
[desktop-environment.md](desktop-environment.md); the per-component inventory with
evidence-based status (specified / partial / missing) is
[component-registry.md](component-registry.md); which components spin out onto standard Wayland
seams versus stay compositor-internal is decided in
[adr/0012-de-modularity-spinout-seams.md](adr/0012-de-modularity-spinout-seams.md).

## Where each concern is documented

| Concern | Document |
|---|---|
| What a device must declare (typed options) | [device-contract.md](device-contract.md) |
| The desktop-environment plane model + dependency graph | [desktop-environment.md](desktop-environment.md) |
| Component inventory: what exists / what's missing | [component-registry.md](component-registry.md) |
| Build order: the boot-forward rung ladder to the XR greeter and session | [implementation-path.md](implementation-path.md) |
| Turning donor firmware into pinned artifacts | [donor-pipeline.md](donor-pipeline.md) |
| Building flashable images and shipping updates | [images-and-updates.md](images-and-updates.md) |
| Monorepo layout and patch management | [repo-structure.md](repo-structure.md) |
| Contested decisions and their rationale | [adr/](adr/) |

## Non-negotiable invariants

These fall out of the research and hold across every document:

1. **`nix build` never flashes a device.** Tools refuse to write to block devices; flashing is a
   separate stage that independently verifies model, firmware prerequisites, slot state, and
   bootloader conditions.
2. **Every donor input is hash-pinned** (`fetchurl` public, `requireFile` non-redistributable,
   explicit path option for on-device extraction) with recorded provenance.
3. **Donor-containing outputs never reach a *public* cache.** Cache eligibility is a three-way
   policy on every artifact: `publicRedistributable` (public cache OK), `privateSubstitutable`
   (a private, access-controlled cache only), or `localOnly` (`allowSubstitutes = false;
   preferLocalBuild = true`). Any closure containing non-redistributable donor bytes inherits the
   strictest label of its inputs. `redistributable = false` in a donor manifest maps to
   `localOnly` for outputs embedding those bytes, and at most `privateSubstitutable` for the private
   device cache — never `publicRedistributable`. (Resolves the apparent
   [invariant-3 vs. private-cache](repo-structure.md) contradiction the review flagged.)
4. **Per-unit calibration/identity is sacred** — never copied between units, protected across flashes.
5. **Signing keys live outside the Nix store**; in-store artifacts use test keys and are cacheable.
6. **The common distribution is device-independent** — device modules select and configure, they do
   not fork services or ship replacement rootfilesystems.
7. **Reproducibility is tested** — releases carry independent-rebuild byte-comparison evidence, not
   an assumption.
8. **A kernel meeting the contract is a precondition for the default NixOS userspace** — an old
   vendor kernel does not freeze the whole distribution; see
   [adr/0002-nixos-vs-nix-built-userspace.md](adr/0002-nixos-vs-nix-built-userspace.md).
9. **Budgets are architecture.** One smartphone-class SoC serves perception, composition,
   clients, and services under hard frame deadlines and a fanless thermal envelope; the
   partition is owned by [budgets.md](budgets.md), every new ADR/design doc carries a
   budget-impact statement (its standing rule), and the frame path is a whitelist — this is
   near-embedded development and efficiency is an invariant, not a pass.
