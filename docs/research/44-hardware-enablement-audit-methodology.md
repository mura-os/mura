# 44 — Hardware-enablement audit methodology

**Status:** canonical method for per-target hardware-chain research.
**Date:** 2026-09-24.
**Scope:** Quest 1 (`monterey`), Lynx R1, Samsung Galaxy XR (`SM-I610`),
Play For Dream MR, Valve Steam Frame (`deckard`), and Meta Quest 3 (`eureka`).

This method generalizes the physical/stock/native separation and static/runtime discipline from
[43-microphone-native-linux-capture-audit.md](43-microphone-native-linux-capture-audit.md).
It governs docs 45–53 and future hardware audits. It does not turn consumer-platform policy into
Mura policy, create a device-contract option, or promote a physical feature to supported status.

## 1. Audit unit: device, profile, domain

Every claim is keyed by:

```
(device codename, hardware profile, audit domain)
```

`base` is the shipping configuration and is always the first profile. A named profile is included
only when an attachable accessory, hardware revision overlay, or reproducible community-adopted mod
changes at least one chain link: physical topology, bus/power, DT/kernel, firmware, calibration,
device-node ABI, userspace service, or runtime consumer.

Board revisions remain `mura.device.skuConstraints`; they are not accessories. This research pass
defines profile vocabulary but deliberately does **not** invent `mura.hardware.profiles.*` or
`activeProfiles` contract fields.

### 1.1 Profile identifiers

Use `<unique-device-id>:<class>:<slug>`:

- `deckard:base` — Steam Frame without an attachment; MP is a `skuConstraints` fact;
- `deckard:acc:arcturus-vision` — the Arcturus Vision Camera attached;
- `<codename>:mod:<slug>` — a qualifying community mod.

Classes are:

- `base` — shipping configuration;
- `acc` — vendor-supported or near-official accessory;
- `mod` — reproducible, maintained, community-adopted hardware/enablement modification.

Profiles may state `requiresAttachment`, `exclusiveWith`, `minStockBuild`, and the base links they
share. These are audit fields, not implemented Nix options.

### 1.2 Inclusion bar

Include a profile when either:

1. the vendor or an official partner sells/documents it and the stock OS has a named hardware or
   software path; or
2. a maintained public recipe/repository is independently reused and changes an audited chain.

Label the latter **COMMUNITY-ADOPTED**. A single private hack, destructive exploit, cosmetic
strap/pad/lens, software-only mode switch on identical nodes, or hypothetical accessory is not a
profile. Reserve a hook for announced-but-unshipped expansion hardware; do not add a matrix row.
WiVRn/ALVR are alternate runtime boundaries, not hardware profiles.

### 1.3 Canonical profile ledger

Doc 44 owns profile identity so domain audits never independently invent aliases:

| Profile | Product identity | Inclusion evidence | Revision constraint | Affected domains |
|---|---|---|---|---|
| `monterey:base` | Oculus Quest 1 | shipping target | retail revisions recorded per claim | all base domains |
| `lynx-r1:base` | Lynx R1 | shipping target | panel/board variants remain SKU facts | all base domains |
| `sm-i610:base` | Samsung Galaxy XR | exact model `SM-I610` | build/revision recorded per artifact | all base domains |
| `pfdm-mr:base` | Play For Dream MR | exact vendor/product identity | `anorak` is artifact-local only | all base domains |
| `deckard:base` | Valve Steam Frame | shipping target | MP donor evidence is a SKU constraint | all base domains |
| `deckard:acc:arcturus-vision` | Arcturus Vision Camera on Frame | vendor-supported sold accessory **[external]** ([source](https://arcturus.vision/)) | attachment/build requirements unresolved | world camera; donor and power deltas |
| `eureka:base` | Meta Quest 3 | shipping target | retail revisions recorded per claim | all base domains |

Additional profiles enter this ledger only after satisfying §1.2. `anorak` is never a profile or
device identity: it occurs in Galaxy, PFDM, and Quest 3 artifacts and is qualified only by exact
product/build provenance.

### 1.4 Arcturus worked example

Steam Frame `base` has four outward monochrome cameras for tracking and monochrome passthrough.
Arcturus is a vendor-documented sold color-passthrough accessory and therefore qualifies as an
**A0** profile. Its exact donor `arcimx616` attribution, firmware, calibration, hotplug and native
capture path remain audit questions rather than inherited facts. Therefore:

- `deckard:base` owns the stock mono sensor/media/runtime chain;
- `deckard:acc:arcturus-vision` owns its added RGB sensor, expansion-bus, power, driver/firmware,
  calibration, stock package, and native ingestion delta;
- color capability and its runtime qualification apply only while that profile is attached;
- shared SoC/ISP links are inherited only when explicitly cited.

No base-device table may say “Steam Frame has color passthrough” because the accessory can provide
it.

## 2. Three axes

Every profile is described independently on three axes:

1. **Physical:** BOM, placement, connector, bus, electrical/power topology, and revision.
2. **Stock:** vendor kernel/DT, firmware, HAL/service, calibration, modes, and observed behavior.
3. **Native Mura/Linux:** upstream/downstream kernel path, device ABI, standard userspace or
   explicitly classified donor bridge, session/runtime consumer, and contract consequence.

Stock functionality is engineering evidence. It never proves a native path.

## 3. L0–L7 chain schema

| Level | Required join |
|---|---|
| **L0 — identity** | exact device, SKU/revision, profile, physical components and provenance |
| **L1 — electrical/bus** | supplies, clocks, resets, pinctrl, bus/lanes, interrupts and bandwidth |
| **L2 — kernel description** | board DT/ACPI, machine/glue driver, component drivers and kconfig |
| **L3 — opaque closure** | firmware, topology, board data, calibration, licenses, hashes and ABI pairing |
| **L4 — kernel ABI** | device nodes/sysfs/debugfs/net/media/DRM/ALSA interfaces and permissions |
| **L5 — system userspace** | udev, standard daemon or donor bridge, configuration and service ordering |
| **L6 — runtime consumer** | Monado, PipeWire, NetworkManager/BlueZ, power service, perception intake, or flasher |
| **L7 — Mura contract** | declared fact, adaptation backend, preflight/readiness check and qualification record |

A chain is statically closed only if producer and consumer identifiers join at every level. “Driver
exists,” “same SoC,” and “stock Android works” are not joins.

## 4. Evidence vocabulary

- **VERIFIED** — inspected source, donor extract, firmware dump, package, or runtime log directly
  supports the claim.
- **VENDOR-DOCUMENTED** — first-party specification/support/developer material.
- **COMMUNITY-REPORTED** — credible third-party observation without a reproducible adaptation.
- **COMMUNITY-ADOPTED** — maintained, reproducible public mod used independently.
- **INFERRED** — adjacent platform or generic reference-board evidence.
- **UNKNOWN** — no defensible evidence.

Pinned clones cite `references/<clone>/path:line`. Other sources are marked **[external]** and
linked. A git-ignored donor archive cites its hash-bound manifest/recipe and exact internal path.
Absence claims state search scope, revision/date, and whether absence means “not present in this
artifact” rather than “hardware absent.”

### 4.1 Artifact classes

For each artifact record:

- source URL/repository and exact revision/build;
- path, package, partition or service name;
- hash where downloadable;
- license/redistribution status;
- immutable firmware versus per-unit calibration/identity;
- profile and hardware revisions to which it applies;
- expected kernel/userspace/DSP ABI.

Per-unit calibration, identity, NV, EFS and persist-class data is never transplanted or committed.

## 5. Qualification ladder

- **A0 — inventory:** physical/stock evidence exists; native links are incomplete.
- **S1 — source coverage:** exact board/profile, component drivers, opaque closure and userspace
  route are identified and pinned.
- **S2 — static closure:** all L0–L6 identifiers join; L7 names the intended existing contract
  mapping and test definitions (not a passing runtime record); schemas/builds pass, licenses are
  recorded, and no unexplained Android-only dependency remains. This is the maximum without
  runtime evidence.
- **R1 — enumeration:** firmware loads; components bind; expected nodes/interfaces and service
  objects exist.
- **R2 — transport:** data/control advances under a known stimulus without stalls, faults or
  resets.
- **R3 — physical semantics:** physical identity, axes/channels/views/connectors, units, timing and
  calibration are verified.
- **R4 — session/runtime integration:** the ordinary intended Mura consumer works under correct
  seat/session ownership and profile detection.
- **R5 — robustness/quality:** cold boots, suspend/resume, hotplug/profile switching, sustained XR
  load, recovery, and domain-specific quality bounds pass.

Each profile has its own state. An accessory R4 does not promote `base`; base R4 does not prove the
accessory. Audit docs define domain-specific evidence bundles without silently inventing numeric
thresholds.

Profile-dependent checks remain qualification procedures until attachment detection and profile
policy are designed. They must not become boot/preflight gates through an undeclared
`activeProfiles` mechanism.

## 6. Standard per-audit document shape

Every canonical audit contains:

1. metadata, boundaries and relation to existing algorithm/design docs;
2. a summary matrix with `base` first and profile rows immediately after their device;
3. per-device sections following L0–L7;
4. profile deltas that state which base links are shared and which diverge;
5. artifact/license/calibration inventory;
6. static verdict and explicit runtime-only gaps;
7. a domain-specific R1–R5 evidence bundle;
8. Mura contract/preflight consequences;
9. adopt/reject/open questions with named deciders;
10. contradiction register entries.

Common matrix:

| Device | Profile | Physical | Stock | Native static | Native runtime | Contract impact |
|---|---|---|---|---|---|---|

Do not copy algorithm detail, UX policy, or generic pipeline design from its canonical owner.

## 7. Contradiction register

Record disputes as:

| Claim key | Evidence A | Evidence B | Current label/verdict | Decider |
|---|---|---|---|---|

The decider is a named source acquisition, source-code comparison, or runtime test. Do not resolve
a conflict by majority vote or by preferring a consumer vendor.

High-risk cross-audit collisions include:

- Galaxy XR, Play For Dream, and Quest 3 artifacts using `anorak`;
- raw versus processed audio channels;
- world-facing versus inward cameras;
- proximity Hall sensors versus IPD-position Hall sensors;
- stock base versus accessory color/depth capabilities;
- one firmware blob serving several DSP domains;
- boot/donor partitions versus subsystem-owned calibration schemas.

### 7.1 Shared artifact ownership

Doc 51 owns immutable donor provenance: build, hash, license, ABI, protected partitions and
consumer list. Each domain audit owns how that artifact is consumed. Persistent-state architecture
owns per-unit calibration lifecycle. A shared atomic family such as Galaxy
`device_profile.textproto` is indexed once and referenced by display, IMU, camera and eye audits;
it is never split into independently versioned copies. Existing `calibration.paths` and
`keepVerbatim` limitations remain open architecture gaps, not solved by these audits.

## 8. Parallel research-agent output contract

Domain agents are read-only. They inspect pinned/local artifacts first and then external sources.
They return Markdown plus a compact ledger shaped as:

```yaml
domain: world-camera
device: deckard
profile: deckard:acc:arcturus-vision
evidence_state: A0
shared_base_through: L1
links:
  L0: []
  L1: []
  L2: []
  L3: []
  L4: []
  L5: []
  L6: []
  L7: []
artifacts: []
contradictions: []
runtime_gates: []
contract_implications: []
```

Each report must cover all six devices, list qualifying profiles and rejected profile candidates,
separate facts/inference, state searches that returned nothing, and propose only thin integration
links. Agents never edit shared files.

## 9. Review and integration

After the nine domain reports:

1. evidence reviewers check exact citations, identity/profile binding, negative evidence and
   static/runtime claims;
2. a cross-domain reviewer checks shared buses, firmware, calibration, profile names and contract
   implications;
3. disputed findings return to the responsible domain agent;
4. the parent integrator writes canonical docs and adjudicates contradictions;
5. landscape/design documents receive links or compact indices, not duplicate chains.

No device option is added from research alone. A later policy decision may define supported
profile declarations and runtime attachment detection.

## 10. Domain ownership

- **45:** IMU/3DoF to Monado; SLAM algorithms stay in doc 20.
- **46:** HMD panel/DRM path; compositor architecture stays in docs 31/39 and composition design.
- **47:** outward and attachable world cameras; warp/depth algorithms stay in docs 13/14/16/22.
- **48:** Wi-Fi/Bluetooth hardware closure; provisioning/pairing UX stays in doc 42/onboarding.
- **49:** battery/charging/thermal hardware; budget and idle policy stay in their architecture docs.
- **50:** speaker/playback sink; microphone capture stays in doc 43, spatial-audio policy remains a
  registry gap.
- **51:** boot/donor facts and profile deltas; pipeline design stays in doc 06 and
  `architecture/donor-pipeline.md`; Frame reconstruction stays in doc 33.
- **52:** inward eye cameras, illuminators and IPD sensing/actuation; algorithms stay in docs 28/29.
- **53:** proximity/presence hardware; lock/doff policy stays in doc 12 and ADR 0007.

Docs 52 and 53 supersede only the detailed per-target enablement paths in docs 29 and 42
respectively; those older docs retain their hardware matrix/algorithm and bootstrap-policy roles.
Doc 51 owns provenance, not every subsystem's profile delta, and doc 33 remains the canonical
Frame reconstruction record.
