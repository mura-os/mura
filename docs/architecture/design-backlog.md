# Design backlog: disposition of the architecture review

This triages [REVIEW.md](REVIEW.md) (the cross-model red-team pass). The review's own verdict is
that a skeletal module tree + simulated VM can start now, but the generic donor/update/backend
machinery must not be built before a **Lynx R1 boot/display/tracking spike** proves the assumptions
it would encode. This document records what was fixed immediately and what is gated on that spike
or on pre-release design — so nothing is silently dropped.

> **Order authority:** [implementation-path.md §5](implementation-path.md) is the sole deferral
> register; this document is its satellite — it records *what each gate must prove*, not when
> work happens.

## Fixed now (in this pass)

| # | Review item | Resolution |
|---|---|---|
| 13 | Cache policy self-contradiction | [overview.md](overview.md) invariant 3 now defines three cache classes (`publicRedistributable` / `privateSubstitutable` / `localOnly`), strictest-input-wins; the private device cache is `privateSubstitutable`, never public. |
| 6 | Mainline readiness overstated | [adr/0002](adr/0002-nixos-vs-nix-built-userspace.md) replaced the blanket claim with a per-device evidence-level table (SoC-supported / board-boots / display-proven / tracking-proven). |
| 7 | kconfig contract vs. `nix flake check` / IFD | [device-contract.md](device-contract.md) §kernel now splits eval-time (declared-intent) from realization-time (final `.config`) checks, forbids IFD, makes per-device kernel checks lazy/sharded, and states configFile-over-structuredExtraConfig precedence. |
| 16 | WiVRn treated as a drop-in runtime | [device-contract.md](device-contract.md) §xr clarifies Monado is the only initial on-device runtime; `wivrn` is the optional streaming-server role. The scaffold already warns it is unimplemented. |
| NB1 | Shell ownership inconsistency | [overview.md](overview.md) §layers: common owns shell *packaging + session policy*; the shell *process* runs at the application boundary as an OpenXR/Wayland client. |
| NB2 | "seven mandatory fields" vs. six | [device-contract.md](device-contract.md): only `codename`/`vendor`/`name` are strictly mandatory; the rest default. Matches the implemented `lib/contract`. |
| — | Virtual-device exemption | Implemented directly: `lib/contract` assertions require `headerVersion` only for `bootScheme = "android-bootimg"` and a donor only for an `android-backed` subsystem, so `bootScheme = "vm"` is exempt. `devices/virtual-headset` builds and passes `nix flake check`. |

## Deferred to the Lynx R1 spike (do NOT build generic machinery first)

These are the review's blocking items that encode hardware-dependent facts. Per the review's own
guidance and [00-synthesis](../research/00-synthesis.md) §7 item 9, they are settled by throwaway
spikes on real hardware, then the *proven* path is encoded — not designed speculatively now.

- **#1 Early-boot / stage-1 / rootfs-handoff contract.** How ABL starts Linux, which ramdisk owns
  stage 1, how the initrd finds slot/rootfs/store, module/firmware load order, `switch_root`, and
  the rootfs's physical location on an Android device. Define one concrete Lynx path first.
- **#2 AVB / verified-boot policy.** Per-device modes (`unlocked-disable` / `unlocked-custom-key` /
  `vendor-signed-only`), vbmeta chain, rollback-index rules, key custody. Unsupported modes produce
  no flashable output.
- **#3 Unlock-to-flash install state machine.** Read-only probes, required firmware/build/unlock
  state, backups, human-confirmation boundaries, permitted writes, post-reboot verification,
  recovery — per device. Replaces the argv-template flasher enum for real devices.
- **#10 Stop/go spikes for the fatal risks.** Four kill-criteria spikes before framework work: boot
  current Linux on Lynx; present via Turnip/`VK_KHR_display` (or prove the DRM-lease fallback);
  acquire timestamped synchronized IMU/camera with calibration; run a minimal Monado pose path.
- **#11 Concrete adaptation-backend interface.** Turn the three labels
  (`native`/`android-backed`/`device-specific`) into a per-subsystem implementation module with a
  declared interface, dependencies, startup target, health probe, and criticality — once one real
  backend exists.
- **#14 Update atomicity per backend.** Backend-specific transaction phases and crash model; resolve
  Lynx dynamic-`super`/rootfs ownership; exclude non-A/B devices from `release-supported` until a
  tested recovery design exists.

## Deferred to pre-release design (not needed for first boot, needed before a release)

- **#4 / #5 Per-unit state model.** Split partition classes (`vendorPayload` / `preserveInPlace` /
  `backupOnlySensitive` / `spatialManaged`); forbid per-unit classes from donor derivations, caches,
  images, and manifests by assertion; define versioned state domains, migration edges, and a
  no-mark-success-until-validated rule. (The current `keepVerbatim` conflation is a known gap; the
  contract's `protectedPartitions` is the placeholder.)
- **#8 Device × variant × check evaluation strategy.** Lazy per-device output constructors, a cheap
  eval-only inventory, CI shards, and an explicit build-vs-host-vs-target platform model in
  `spatialSystem`.
- **#9 / tiers evidence.** Separate static / build / VM / hardware-automated / manual evidence
  classes; signed qualification records bound to device revision + donor hash + output hash + test
  version, with freshness policy. CI validates evidence; it does not pretend to run absent hardware.
- **#12 Authenticated donor acquisition.** An out-of-store acquisition CLI reading secrets from an
  agent/keyring, emitting a redacted lock record + content-addressed artifact handed to
  `requireFile`. Derivations stay networkless and secret-free.
- **#15 Reproducibility protocol.** Two-builder protocol on independent native-aarch64 builders,
  forced local realization for compared paths, canonical unsigned artifacts, donor-hash equivalence,
  diffoscope on mismatch, signed attestations. Reproducibility claims scoped away from per-unit
  state and post-build signatures.
- **Non-blocking NB3–NB9.** Tighten typed submodules over `attrs` bags; `.config` vs.
  `structuredExtraConfig` precedence (partially fixed in #7); combine cheap donor stages initially;
  separate the qualify report-derivation from the gating function; stop probing local state at eval
  for output presence; rename the "reference-free bundle" to a local install bundle with closure
  policy applied; define the dev-image storage layout or scope the `nixos-rebuild` promise to
  VM/remote.

## Desktop-environment gaps live in the component registry

The backlog above covers the base build/donor/update architecture. The *desktop-environment*
gaps — the 29 missing components (launcher, notifications, settings, polkit agent, input methods,
audio policy, spatial-workspace model, session restoration, colour pipeline, …) — are enumerated
with evidence in
[component-registry.md §8](component-registry.md), dependency-mapped by
[desktop-environment.md §6](desktop-environment.md), with their modularity/seam decisions in
[adr/0012-de-modularity-spinout-seams.md](adr/0012-de-modularity-spinout-seams.md). They are not
duplicated here.

## Scope decisions

- **App distribution/installation is out of scope** (2026-09-23). spatial-os installs NixOS onto
  the device; after that the user owns a PC and brings whatever medium they want (nix profiles,
  Flatpak, plain binaries) — the OS does not ship an app store or bless a distribution channel.
  What *is* in scope is the launcher (desktop-entry + icon-theme consumption; registry §5) and
  the portal/security seams any medium plugs into. Future exploration preserved: a small local
  LLM (llama.cpp-class) that edits the user's Nix configuration conversationally — an
  installation *interface*, not a distribution channel; revisit after the settings model
  (research/35) lands.

## Standing rule from the review — status update (2026-09-23)

The **uefi-rauc family's spike is done**: the Steam Frame donor was reconstructed, byte-verified,
and inventoried, and a spatial-os image mirroring its slot architecture boots and A/B-updates in a
VM ([33 §9–§10](../research/33-steam-frame-donor.md)). Generalizing uefi-rauc machinery
(mark-good service, bundle builder, bootconf backend) is therefore now licensed *for that family*.
The Android-family machinery (`lib/donor` automation, android-bootimg) remains gated on the Lynx
spike exactly as below.

## Standing rule from the review

> Do not build the generic donor/update/backend machinery before a Lynx boot/display/tracking spike
> proves the assumptions it is meant to encode.

The scaffold deliberately reflects this: `lib/donor` and the real image/update backends are typed
stubs that `throw`/`warn` rather than pretend to work, so an incomplete port fails loudly. The
`virtual-headset` VM exercises the contract + module composition + XR runtime wiring end-to-end
without any of that machinery.
