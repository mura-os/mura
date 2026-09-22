# Architecture red-team review
## Verdict

**With changes, but not yet sound enough to implement the advertised scaffold literally.** The
common-distribution/adaptation boundary, donor pinning posture, and build/flash separation are good
foundations. However, the documents skip the load-bearing path from boot ROM to a mounted NixOS
closure, leave AVB and per-unit state as slogans rather than protocols, and claim more board-level
mainline readiness than the research supports. A skeletal module tree and simulated VM can start,
but the contract and flake checks need the blocking corrections below during that scaffold. Do not
build the generic donor/update/backend machinery before a Lynx boot/display/tracking spike proves
the assumptions it is meant to encode.

## Blocking issues

1. **The stage-1 and rootfs handoff do not exist as an architecture.**
   - **Where:** `overview.md` §Kernel + firmware; `images-and-updates.md` §Android boot chains;
     `00-synthesis.md` §4 only says to use `boot.initrd.systemd`.
   - **Problem:** Packing `boot`, `vendor_boot`, DTB and an ext4/EROFS payload does not specify how
     ABL starts Linux, which ramdisk owns stage 1, how the initrd finds the target slot/rootfs/Nix
     store, when firmware and modules load, or how it switches root. The rootfs's physical location
     on an Android device is itself unresolved.
   - **Why it matters:** This is the actual boot design. Header-correct images can still be entirely
     unbootable, and neither the VM target nor an image builder can be implemented against the
     current prose.
   - **Suggested resolution:** Add an early-boot contract covering boot artifact ownership,
     kernel command line, initrd composition, slot/root selection, rootfs format/location, dm-verity
     or equivalent, required early modules/firmware, failure shell, and `switch_root`. Define one
     concrete Lynx path before generalizing image families.

2. **AVB/secure-boot policy is deferred even though it determines every Android image.**
   - **Where:** `donor-pipeline.md` §Open questions; `images-and-updates.md` §Android boot chains and
     §reference-free flashing bundle; `overview.md` invariants 5 and 8.
   - **Problem:** The packer promises AVB metadata while the flasher may disable verification, and
     signing is described only as “outside the store.” There is no model for chained vbmeta,
     descriptors, key enrollment, unlock-state requirements, rollback indexes, test-key rejection,
     or device-specific trust roots.
   - **Why it matters:** “Unlocked” does not imply all targets accept disabled verification, and
     rollback-index mistakes can permanently prevent boot or downgrade. A generic `flash_vbmeta`
     template is unsafe.
   - **Suggested resolution:** Define a per-device verified-boot policy with explicit modes
     (`unlocked-disable`, `unlocked-custom-key`, `vendor-signed-only`), vbmeta chain, signing inputs,
     rollback-index source and monotonicity rules, key custody/rotation, and preflight checks. Make
     unsupported modes produce no flashable output.

3. **The unlock-to-flash workflow is not represented by the flasher table.**
   - **Where:** `device-contract.md` §`spatial.deployment.*`; `images-and-updates.md` §Build never
     flashes; `07-device-landscape.md` device-specific boot-chain sections.
   - **Problem:** `flashMethod = fastboot|heimdall|edl-qdl|…` models a command, not the stateful
     workflows described by the evidence: Quest inactive-slot ABL manipulation, Samsung
     firmware/CSC and Download Mode constraints, Lynx backup/recovery, or PFDM fuse uncertainty.
   - **Why it matters:** The dangerous work is preflight, backup, unlock verification, reboot
     transitions, anti-rollback checks, and recovery—not argv interpolation.
   - **Suggested resolution:** Specify a reviewed per-device install state machine with read-only
     probes, required firmware/build/unlock states, backup outputs, human confirmation boundaries,
     permitted writes, post-reboot verification, and recovery steps. Keep EDL/authenticated
     Firehose outside a generic “flash method.”

4. **Per-unit calibration is simultaneously protected and admitted as donor/pass-through content.**
   - **Where:** `overview.md` invariant 4; `donor-pipeline.md` §metadata-preservation and manifest
     example (`persist` in `keepVerbatim`); `images-and-updates.md` §Build never flashes.
   - **Problem:** “Keep verbatim” conflates immutable vendor partition blobs with live per-unit
     state. A factory `persist` image can enter the Nix store or flashing bundle and then be copied
     to another unit—the exact outcome the invariants prohibit.
   - **Why it matters:** This can destroy optical calibration, radio identity, attestation, and NV
     state. It is a safety and privacy failure, not merely a packaging bug.
   - **Suggested resolution:** Split partition classes into `vendorPayload`,
     `preserveInPlace`, `backupOnlySensitive`, and `spatialManaged`. Forbid per-unit classes from
     donor derivations, caches, images, and install manifests by assertion. Model backup/restore as
     device-local installer operations with encrypted handling and explicit redaction.

5. **There is no per-unit state schema or migration/rollback protocol.**
   - **Where:** `device-contract.md` §XR exposes only `calibration.paths`;
     `images-and-updates.md` §Updates mentions “state-schema expectations” and mutable migrations.
   - **Problem:** Paths do not define ownership, mount timing, schema version, validation, backup,
     migration atomicity, downgrade compatibility, or what happens after a failed readiness check.
   - **Why it matters:** An A/B rootfs rollback is not a rollback if the new slot irreversibly
     migrated calibration or runtime state.
   - **Suggested resolution:** Define versioned state domains and migration edges, backup/checksum
     rules, forward/backward compatibility, slot-independent storage, and a no-mark-success rule
     until migration and calibration validation pass. Ban destructive migrations without an
     explicit recovery artifact.

6. **The claimed mainline-capable target set overstates the evidence.**
   - **Where:** `adr/0002` §Context/Decision rationale; `overview.md` §adaptation defaults;
     `device-contract.md` §Design principles.
   - **Problem:** The ADR says every priority target has a mainline-or-near-mainline path. The
     research says Quest 1 has no public Monterey mainline DTS, Galaxy XR/PFDM have only adjacent
     SoC support and unverified silicon/board mappings, and Steam Frame's alternate-OS boot policy
     is unknown.
   - **Why it matters:** SoC enablement is not headset enablement. Native-by-default and current
     NixOS are strategic bets, not established compatibility facts.
   - **Suggested resolution:** Replace the blanket claim with per-device evidence levels:
     `soc-supported`, `board-boots`, `display-proven`, `tracking-proven`, `release-capable`.
     Gate NixOS/native assumptions on measured board evidence and keep unsupported devices out of
     concrete package outputs.

7. **`nix flake check` and the kernel contract have incompatible semantics.**
   - **Where:** `device-contract.md` §Kernel and §Support tiers; `repo-structure.md` §flake entry
     point; `adr/0002` §Decision; `adr/0005` §Decision.
   - **Problem:** The contract is described as rejecting bad configurations before anything builds,
     but its authoritative input is the generated `.config` from `buildLinux`. Evaluation cannot
     inspect that output without import-from-derivation, and a check derivation still builds the
     kernel. “Build/eval check” hides this distinction.
   - **Why it matters:** A naive implementation either enables IFD, evaluates impurely, or makes
     every `nix flake check` build every device kernel.
   - **Suggested resolution:** Split checks into (a) eval-time option/cross-field checks against
     declared config intent, and (b) realization-time checks against the kernel's final `.config`.
     Disable IFD, expose per-device kernel checks lazily, and run the expensive checks in a sharded
     CI job rather than the default flake check.

8. **The device × variant × check evaluation strategy is unspecified.**
   - **Where:** `device-contract.md` §deployment; `repo-structure.md` and `adr/0005` flake outputs.
   - **Problem:** `packages.<system>.<device>-<variant>` plus repo-wide checks suggests eager
     evaluation of every donor, kernel, image and tier test. `system` is also ambiguous: build host
     or target platform, especially with native-aarch64 release builds and x86_64 evaluation.
   - **Why it matters:** Fleet-scale evaluation can become the bottleneck before builds start, and
     x86_64 developers may not be able to discover or request aarch64 device outputs consistently.
   - **Suggested resolution:** Define build/host platform parameters, lazy per-device output
     constructors, a cheap eval-only inventory, and CI-generated shards. Specify which small subset
     default `nix flake check` realizes and how release CI expands the matrix.

9. **Support tiers claim CI-enforced facts that ordinary CI cannot observe.**
   - **Where:** `device-contract.md` §Support tiers; `repo-structure.md` §flake checks.
   - **Problem:** Physical display output, tracking quality, thermals, interrupted update recovery,
     and controller reconnect are presented as evaluatable checks. No hardware-lab runner,
     attestation format, freshness rule, or manual-evidence model is defined.
   - **Why it matters:** A tier can become a self-asserted enum while CI appears to guarantee more
     than it does.
   - **Suggested resolution:** Separate static, build, VM, hardware-automated, and manual evidence.
     Define signed qualification records bound to device revision, donor hash, output hash and test
     version, with expiry/freshness policy. CI should validate evidence, not pretend to run absent
     hardware.

10. **The implementation sequence does not contain stop/go experiments for the fatal risks.**
    - **Where:** `00-synthesis.md` §7 item 9; `adr/0003` §Consequences;
      `device-contract.md` §adaptation.
    - **Problem:** “Full graphics+tracking on Lynx” is correctly early in prose, but it is one large
      milestone after a common-shell exercise, while the architecture offers substantial generic
      pipeline/backend work to do first. Android compatibility is “proven last,” despite a selected
      device possibly needing it for a critical subsystem.
    - **Why it matters:** The project can spend months implementing abstractions before learning
      that direct display, synchronized camera capture, calibration access, or tracking is
      infeasible.
    - **Suggested resolution:** Make four pre-framework spikes with explicit kill criteria:
      boot current Linux on Lynx; present via Turnip/`VK_KHR_display` (or prove the lease fallback);
      acquire timestamped synchronized IMU/camera data with calibration; run a minimal Monado pose
      path. Use throwaway scripts if necessary, then encode only the proven path.

11. **The three backend values are too abstract to be executable contracts.**
    - **Where:** `overview.md` §Hardware-adaptation; `device-contract.md`
      §`spatial.adaptation.*`; `adr/0003` §Decision.
    - **Problem:** `android-backed` may mean in-process libhybris, binder service bridge, or LXC,
      which have different kernels, lifecycle, security and filesystem needs. “Optional” also means
      only non-boot-critical; an Android-backed display/tracker may still be mandatory for XR
      readiness.
    - **Why it matters:** One enum cannot select packages, units, kernel contract or failure policy
      deterministically.
    - **Suggested resolution:** Make each subsystem select a concrete implementation module with a
      declared interface, dependencies, startup target, health probe and criticality. Keep the
      three labels as documentation categories, not the executable API.

12. **The donor acquisition model omits authenticated network and secret boundaries.**
    - **Where:** `donor-pipeline.md` §acquire; `device-contract.md` §donor.
    - **Problem:** Samsung FUS, authenticated portals and account-bound OTA URLs are collapsed into
      `requireFile`. There is no rule for credential use, networked prefetch tooling, URL/token
      redaction, secret logging, expiry, or conversion into an offline hash-pinned input.
    - **Why it matters:** Putting credentials in Nix expressions, derivation environments or logs
      leaks them into world-readable store metadata and breaks reproducibility.
    - **Suggested resolution:** Define an out-of-store acquisition CLI that reads secrets from an
      agent/keyring, writes a local content-addressed artifact, emits a redacted lock record, and
      hands off to `requireFile`. Derivations must be networkless and secret-free.

13. **Cache policy contradicts itself.**
    - **Where:** `overview.md` invariant 3; `donor-pipeline.md` §Reproducibility and safety;
      `repo-structure.md` §Caches.
    - **Problem:** `redistributable=false` mechanically forces `allowSubstitutes=false`, while
      `repo-structure.md` proposes a private cache for qualified donor outputs. Those cannot both
      be true.
    - **Why it matters:** Implementers will either lose the intended private cache or weaken the
      legal safety gate ad hoc.
    - **Suggested resolution:** Define separate `publicRedistributable`, `privateSubstitutable`,
      and `localOnly` policies, including who may upload/download and whether derived closures
      containing donor bytes inherit the strictest label.

14. **The update abstraction is asserted before atomicity is defined.**
    - **Where:** `images-and-updates.md` §Updates.
    - **Problem:** RAUC multi-image updates, Android A/B, dynamic/Virtual A/B, AVB indexes, and
      non-A/B pre-init flashing are called one transaction without a common state machine or crash
      model. Dynamic `super` handling is still an open question.
    - **Why it matters:** The least capable backend cannot provide the same atomicity or rollback
      promise. A power loss while updating boot metadata can brick a device.
    - **Suggested resolution:** Specify backend-specific guarantees and transaction phases. Do not
      expose one undifferentiated promise. Exclude non-A/B devices from `release-supported` until a
      tested recovery design exists, and resolve rootfs/dynamic-partition ownership for Lynx.

15. **Reproducibility evidence has no responsible actor or protocol.**
    - **Where:** `overview.md` invariants 2/7; `images-and-updates.md` §Reproducibility;
      `device-contract.md` §release-supported.
    - **Problem:** “Independent rebuild” does not say who rebuilds, on which platform, with donor
      access, with cache substitution disabled, what unsigned object is compared, or how evidence
      is signed and published. Per-unit acquisition cannot produce globally identical artifacts.
    - **Why it matters:** The release gate is not implementable and may accidentally compare two
      downloads of one cached result.
    - **Suggested resolution:** Define a two-builder protocol on independent native-aarch64
      builders, forced local realization for compared paths, canonical unsigned artifacts, donor
      hash equivalence, diffoscope on mismatch, and signed attestations. Scope reproducibility
      claims away from per-unit backup/state and post-build signatures.

16. **The runtime model incorrectly treats WiVRn as a drop-in standalone runtime.**
    - **Where:** `device-contract.md` §`spatial.xr.*`; `05-xr-userspace.md` §2.2 and §10.
    - **Problem:** The contract offers `runtime = monado|wivrn`, but the research says WiVRn's
      headset client is an Android OpenXR application using the vendor runtime; its server role is
      a PC/host runtime. That does not establish WiVRn as the native appliance runtime on a
      spatial-os headset.
    - **Why it matters:** The option implies service and image configurations that have not been
      defined or demonstrated.
    - **Suggested resolution:** Keep Monado as the only initial on-device runtime. Model WiVRn
      separately as an optional streaming role with explicit client/server placement after a real
      spatial-os use case is proven.

## Non-blocking concerns

1. **The common/application ownership boundary is internally inconsistent.**
   - `overview.md` §Application puts the shell in the application layer, while §Common says the
     common layer owns the shell. Pick one owner; preferably common packages/session policy with
     the shell itself consuming only OpenXR/Wayland interfaces.

2. **The “seven mandatory fields” example supplies only six.**
   - `device-contract.md` §identity requires `skuConstraints`, but §minimal device omits it. Either
     make it default to a typed “unknown/all revisions” value or include it in the example.

3. **The schema-first claim is undercut by untyped attribute bags.**
   - `device-contract.md` uses `attrs` for SKU constraints, boot offsets, environment and
     calibration paths, and `listOf str` for image variants. Replace safety-relevant bags with
     submodules/enums; keep arbitrary environment only as an explicit escape hatch.

4. **The kernel configuration source of truth is ambiguous.**
   - `device-contract.md` §kernel declares both literal `.config` and
     `structuredExtraConfig`; `repo-structure.md` favors structured config. Define precedence,
     generated artifacts, and how conflicts fail.

5. **The five donor stages are premature as five derivations.**
   - `donor-pipeline.md` §Five stages gives caching/review as rationale, but identify/parse/extract
     boundaries may duplicate large images and I/O. Preserve the conceptual stages, initially
     combine cheap adjacent stages, and split only from measured cache/review value.

6. **A qualification derivation cannot literally produce a Nix attrset.**
   - `donor-pipeline.md` §qualify should distinguish the derivation that emits a report from the
     Nix function that gates downstream attrs. Specify the boundary to avoid IFD.

7. **Output disappearance depends on more than the documents admit.**
   - `adr/0005` §Consequences says outputs disappear based on donor+contract availability.
     Evaluation can gate on a committed contract, but cannot reliably gate on whether a
     `requireFile` payload happens to exist without impurity. Expose unavailable builds with clear
     failure instructions rather than probing local state at eval.

8. **The “reference-free bundle” is not distribution-safe by default.**
   - `images-and-updates.md` §bundle may include donor-derived `.img` files that policy says cannot
     be redistributed. Call it a self-contained local install bundle and mark/export only outputs
     whose closure policy permits it.

9. **Development generations and immutable release images may diverge structurally.**
   - `images-and-updates.md` §Development vs. release assumes `nixos-rebuild` is a fast path without
     defining a writable store/profile, boot entry switching, persistence, or garbage collection
     on Android partitions. Define a development image/storage layout or limit the promise to VM
     and remote rootfs deployment.

## Things done well

- The common-distribution versus per-device adaptation boundary is clear and worth preserving.
- Build/flash separation and explicit refusal to write hardware during `nix build` are correct.
- Donor hash pinning, provenance, allowlist extraction, and rejection of incremental OTAs are
  appropriately conservative.
- The documents correctly treat SoC, kernel, firmware, DTB, userspace and XR driver as one qualified
  compatibility set rather than independently “latest” components.
- The Monado pin-plus-patch model accurately reflects the lack of a stable out-of-tree driver ABI.
- The architecture explicitly identifies direct display and tracking as feasibility risks instead
  of presenting packaging as the hard part.
- Separating canonical test-key artifacts from controlled release signing is the right reproducible
  build boundary, once the AVB policy is specified.
- The plan to implement real devices before extracting family layers is sound and should override
  pressure to design a complete taxonomy up front.

## Specific under-specifications for the scaffold

Before `modules/` and `devices/virtual-headset/` can be implemented as more than placeholders, pin
down the following:

- [ ] Define the exact Nix option type for a pinned source (`path`, `fetchTree` input, derivation, or
  a typed source submodule); “pinned src” is not a type.
- [ ] Define `spatial.kernel.contract` as a list/set of named categories, including valid names,
  merge behavior, severity, and unknown-category failure.
- [ ] Decide whether `.config` or `structuredExtraConfig` is authoritative and how the other is
  derived/validated.
- [ ] Split eval-time kernel assertions from realization-time final-`.config` checks; explicitly
  forbid IFD.
- [ ] Define which checks default `nix flake check` evaluates/builds and which are CI shards.
- [ ] Define build platform versus host/target platform in `spatialSystem` and flake output names.
- [ ] Add the missing `spatial.hardware.*` schema used by the sample.
- [ ] Type display topology: panel count, resolution per panel, physical size, refresh modes,
  orientation, DRM connector mapping, and stereo layout.
- [ ] Type camera and sensor topology sufficiently to describe simulated and physical devices,
  including clocks/timestamp domain and calibration references.
- [ ] Define controller/input capabilities or explicitly defer them from the first scaffold.
- [ ] Replace `skuConstraints = attrs` with typed revision/SKU predicates and define unknown
  hardware behavior.
- [ ] Reconcile the seven mandatory fields with the minimal example's omitted `skuConstraints`.
- [ ] Define a virtual-device exemption from donor, flash, protected-partition and physical-kernel
  requirements.
- [ ] Define the virtual target's kernel: stock nixpkgs VM kernel versus a project `buildLinux`
  derivation, and which contract it exercises.
- [ ] Define the virtual XR path: Monado simulated driver, software or virtio Vulkan, compositor
  backend, expected OpenXR test app, and pass condition.
- [ ] Define whether the VM must launch StardustXR; if so, specify session user, D-Bus, XDG runtime,
  socket activation, and headless/windowed display setup.
- [ ] Define `spatial.xr.runtime` initial scope; remove or separately model WiVRn.
- [ ] Type Monado revision and patches, including whether family/device values may override each
  other and how patch order is fixed.
- [ ] Define driver option names and map them to known `XRT_BUILD_DRIVER_*` values with rejection of
  unknown names.
- [ ] Define compositor backend dependencies and whether `wayland-direct` includes/provides the
  DRM-lease compositor.
- [ ] Define `spatial.xr.environment` merge and secret policy; environment values must never be a
  credential channel.
- [ ] Define calibration path entries: source class, mount, ownership, permissions, requiredness,
  schema version, and whether content is per-unit.
- [ ] Define the adaptation implementation interface: packages, modules, units, kernel categories,
  health probe, criticality, and readiness contribution.
- [ ] Replace backend defaults with either explicit selections or capability-driven defaults that
  fail closed when family evidence is absent.
- [ ] Specify `spatial.deployment.partitions` fields: stable identifier, source class, slot, size,
  filesystem, update owner, preservation class, and permitted operations.
- [ ] Remove `persist`/calibration/NV from image-source and donor-pass-through types.
- [ ] Define Android stage-1, rootfs location and slot-selection options even if the VM does not use
  them, so the initial schema does not freeze the wrong image model.
- [ ] Define AVB policy options and make them mandatory for any `android-bootimg` output.
- [ ] Replace the open-ended flash-method enum and argv templates with a typed installer workflow
  reference; the virtual target should select `none`.
- [ ] Define image variant declarations as typed deferred modules, not arbitrary strings, including
  device compatibility and output naming.
- [ ] Define `acceptanceTests` and `readinessCheck` option types: derivation, executable package,
  systemd unit, VM test, hardware protocol, or evidence record.
- [ ] Define support-tier evidence classes and prevent the VM's simulated tests from satisfying
  physical `xr-functional` requirements.
- [ ] Specify the NixOS module import/merge order; the prose alternates between
  device→family→SoC→common and device→SoC-family→common.
- [ ] Define what `spatialSystem` returns internally before projecting named flake outputs; avoid an
  untyped public grab-bag without making implementation impossible.
- [ ] Define the smallest scaffold success test: evaluation, VM boot, Monado socket activation,
  active-runtime manifest correctness, and one simulated OpenXR frame/health result.
