# Install target manifest: one guarded plan, multiple frontends

**Status:** draft data-model rev 0 (2026-09-27). This specifies the host-side semantics intended
for a future Mura installer, but it is not yet a machine-validatable JSON Schema. No installer
implements it, no Mura Android-family image exists, and no target is hardware-qualified by this
document. Rev 0 cannot authorize a hardware write.
**Design sources:** [research/71](../docs/research/71-unlock-installer-precedents.md),
[research/72](../docs/research/72-xr-unlock-and-flash-targets.md),
[images-and-updates](../docs/architecture/images-and-updates.md), and review disposition
[design-backlog #3](../docs/architecture/design-backlog.md).
**Grounding:** Android terms (`fastboot`, `fastbootd`, slots, dynamic partitions and AVB) have
their AOSP meanings. `Download` means Samsung's bootloader protocol, not an HTTP transfer. `EDL`
means Qualcomm Sahara/Firehose Emergency Download mode. XDG is not applicable: this is a release
artifact and host/device protocol contract, not desktop configuration, a Wayland protocol, or a
portal API.
**Budget impact** (overview invariant 9): none on the headset frame path. A host installer parses
one bounded plan and streams artifacts only during installation. The future CLI should not need a
resident service or interpreter on the target; the WebUSB frontend has browser-only memory and
storage costs which each plan declares.

## 1. Purpose and boundary

The install plan is the sole machine-readable authority for an unlock, backup, install, restore,
or inspection flow. A WebUSB frontend and a native host CLI interpret the same plan. A generated
shell transcript or human guide is a view of the plan, never an input to it.

The plan describes what may be attempted; its evidence and hardware qualification fields describe
what has actually been demonstrated. A plan with `qualification.hardwareStatus = "unqualified"`
MUST NOT expose write actions in a release installer. Research-only examples may include those
actions with `enabled = false` so the remaining evidence is explicit.

`nix build` may produce a plan and its referenced artifacts. It MUST NOT connect to or modify
hardware. Installation is a separate, explicitly invoked host operation
([overview invariant 1](../docs/architecture/overview.md)).

## 2. Trust and serialization

The filename is `install-target.json`. It is UTF-8 JSON with:

- no duplicate object keys;
- integers only where the schema says integer;
- no non-finite numbers;
- artifact digests written as lowercase hexadecimal;
- operation and state identifiers matching `[a-z][a-z0-9-]{0,63}`.

The exact plan bytes MUST be authenticated with the release artifact set:

1. when inside a signed install bundle, the bundle's signed manifest names the plan's SHA-256;
2. when distributed alone, a detached signature and signer identity accompany it;
3. the installer verifies authentication before reading any operation as executable.

The signing algorithm and release-key lifecycle belong to the release-signing policy, which is
not yet specified. Until that policy exists, generated plans are development-only and MUST carry
`authentication.status = "test-only"`. A frontend MUST show that status and refuse release mode.

## 3. Top-level object

Required fields:

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | exactly `org.mura.install-target/v1` |
| `planId` | string | stable identifier for this target/build/flow |
| `revision` | positive integer | monotonic revision of this plan |
| `purpose` | enum | `inspect`, `unlock`, `install`, `restore-stock`, or `combined` |
| `qualification` | object | §4 |
| `target` | object | §5 |
| `host` | object | §6 |
| `authentication` | object | §2 |
| `artifacts` | array | §8 |
| `partitions` | array | §9 |
| `backups` | array | §9a |
| `states` | array | §10 |
| `operations` | array | §11 |
| `flows` | array | §12 |
| `recovery` | array | §15 |
| `redaction` | object | §16 |

Optional `notes` are display-only. A frontend MUST NOT derive a command, guard, or default from
prose.

Unknown values are represented as absent fields or explicit `null` where the field permits it.
They are never guessed from a SoC family, neighboring model, Android version, or similar product.

## 4. Evidence and hardware qualification

`qualification` separates two axes:

- `evidenceLevels`: a non-empty set containing any of:
  - `source-documented` — reconstructed from pinned source or exact artifacts;
  - `vendor-documented` — documented by the target vendor;
  - `community-reproduced` — exact target/build reproduction is reported with inspectable tooling
    or transcripts;
  - `community-reported` — a public observation lacking a complete reproducible source/transcript
    chain;
- `hardwareStatus`: exactly one of:
  - `unqualified` — Mura has no bound hardware qualification record;
  - `qualified` — the named record passes the target's complete flow and recovery matrix;
  - `unsupported` — evidence shows the flow does not apply or lacks a safe recovery boundary;
- `evidence`: citations or evidence-record identifiers;
- `testedBuilds`: exact builds covered by cited evidence (not implied to be Mura-tested);
- `excludedBuilds`: exact builds or predicates known not to transfer;
- `unknowns`: stable identifiers linked to future capture instructions;
- `hardwareQualification`: `null` unless qualified, otherwise an object containing record URL/path,
  SHA-256, plan digest, device revision, artifact-set digest and test-suite version;
- `writesEnabled`: boolean.

`writesEnabled` MUST be false unless `hardwareStatus = "qualified"`, `hardwareQualification` is
present and its plan digest matches the loaded bytes. A frontend does not infer this boolean from
an evidence level. The static research documents in this revision produce no write-enabled plan.

## 5. Target identity

`target` contains:

| Field | Requirement |
|---|---|
| `vendor`, `product`, `model` | human and vendor identity |
| `codenames` | artifact-local aliases; aliases never join devices without another exact guard |
| `hardwareRevisions` | accepted revisions or `[]` when unknown |
| `regions` / `skus` | accepted region, carrier, enterprise or storage variants |
| `stockBuilds` | exact accepted build identifiers and security patch levels |
| `bootloaderRevisions` | accepted bootloader/SWREV values where observable |
| `usbModes` | mode-specific identities from §7 |
| `targetSelectionProbes` | sufficient target-specific conjunction of read-only model/build/SKU facts |
| `sameUnitProbes` | stable unit facts observable on both sides of every automatic reconnect |

A generic codename such as `anorak`, SoC name such as `sm8550`, or USB vendor ID alone is
insufficient. Each target defines a sufficient conjunction and explains why its facts exclude
nearby models; this specification invents no universal fact count.

Target selection and same-unit continuity are separate. USB topology/path commonly changes after
re-enumeration and is not unit identity. Before any reconnect-spanning write flow, at least one
target-defined stable unit fact MUST be observed in both source and destination states. The
installer derives an ephemeral `sessionDeviceId` as a keyed hash of that fact and rechecks it
after reconnect. If no stable cross-mode fact exists, the plan MUST stop before the reconnect or
require a new manually confirmed session; it cannot automatically continue a write flow. Raw
serials, IMEI, chip IDs and unlock tokens never enter the plan or ordinary transcript.

## 6. Host requirements

`host` declares:

- supported OS and minimum versions;
- frontend capabilities: `webusb`, `native-cli`, `manual`;
- browser and secure-context requirements;
- required native tools and minimum versions;
- USB driver/udev requirements for each mode;
- conflicting services known to claim an interface;
- minimum free storage and memory;
- whether a direct port and external power are required or merely recommended.

A frontend MUST reject an unmet hard requirement. It may warn for a recommendation. A VM, USB
hub, cable type, or browser is not rejected unless the plan has target-specific evidence for the
restriction.

## 7. USB mode record

Each `target.usbModes[]` item contains:

- `mode`: state identifier;
- `vid`, `pid` as four-digit lowercase hex when known;
- configuration and interface class/subclass/protocol;
- endpoint count and transfer type when known;
- serial behavior (`stable`, `changes`, `absent`, `unknown`);
- host driver;
- maximum observed or required speed;
- `permissionScope`: whether a new WebUSB chooser or native driver bind is expected;
- `evidence`.

Runtime ADB/MTP, recovery ADB, bootloader fastboot, fastbootd, Download and EDL identities are
separate records. One mode's identity MUST NOT be inferred for another.

## 8. Artifacts

Each artifact has:

| Field | Meaning |
|---|---|
| `id` | operation-stable name |
| `role` | e.g. `mura-boot`, `stock-abl`, `firehose`, `factory-zip` |
| `source` | URL template, user-supplied selector, or bundle-relative path |
| `size` | exact bytes |
| `sha256` | expected digest |
| `signature` | signature reference when independently signed |
| `format` | `raw`, `android-sparse`, `zip`, `raucb`, `elf`, etc. |
| `targetPartition` | partition identifier or `null` |
| `buildBinding` | exact stock/Mura build set |
| `redistribution` | `bundled` or `user-supplied`; this is operational, not a legal opinion |
| `sensitive` | whether normal logs and caches must hide the path/content |

Downloads are staged before the first write. Size, digest and signature are checked after download
and immediately before first use. A cached artifact is rehashed. A redirect or filename never
changes its identity.

## 9. Partition and per-unit state records

Each partition record contains:

- stable `id` and every exact on-device name;
- physical LUN and GPT identity where known;
- physical versus dynamic-logical;
- slot policy (`none`, `current`, `inactive`, `both`, or an explicit suffix map);
- expected size/range and filesystem, if relevant;
- legal access modes;
- AVB/vbmeta chain and rollback-index location;
- one of:
  - `vendor-payload`;
  - `preserve-in-place`;
  - `backup-only-sensitive`;
  - `mura-managed`;
- read, write, erase and restore permissions per flow.

`preserve-in-place` and `backup-only-sensitive` content MUST NOT enter an install bundle, cache,
ordinary diagnostic export, or another unit. A write or erase operation naming either class is
invalid unless a separate, explicitly reviewed recovery flow names the same unit-bound backup and
post-restore validation.

An unknown partition map makes all writes invalid. A whole-LUN, GPT, `super`, `userdata`, XBL,
ABL, modem/NV, EFS, persist, calibration or identity write requires an exact partition record; a
wildcard is forbidden.

## 9a. Unit backup contracts and runtime records

`backups[]` defines every backup that guards a write:

| Field | Meaning |
|---|---|
| `id` | stable backup-set identifier |
| `sourceStates` | states in which acquisition is permitted |
| `unitBindingProbes` | stable unit facts required at capture and restore |
| `items` | exact partition/LUN/range records, expected sizes and sensitivity classes |
| `readsRequired` | at least two independent device reads for boot/protected state |
| `hash` | runtime digest algorithm, initially `sha256` |
| `completeness` | all required items and GPT copies which must be present |
| `storage` | allowed local store, persistence requirement and confidentiality requirement |
| `export` | whether portable export is required |
| `restoreStates` | states from which restore is permitted |
| `restoreValidation` | post-write readback and semantic checks |

A plan cannot carry a newly captured backup's digest in `artifacts[]`. Instead the interpreter
creates an authenticated sidecar `org.mura.install-backup-result/v1` containing:

- plan digest, backup ID and redacted `sessionDeviceId`;
- exact target/build/slot/source state and capture timestamp;
- each item's source partition/LUN/range, expected size, both read sizes/hashes and their
  agreement;
- container/store digest and persistence status;
- completeness and verification timestamps;
- `restoreEligible`, initially false and set true only after all checks pass.

The sidecar is authenticated by the running installer and bound to its backup bytes. Restore
requires the same stable unit fact, plan/backup ID, source geometry and item set. A filename,
matching model, non-empty file or plausible total size is never backup validation.

`backup-only-sensitive` content requires confidential storage and an explicit user-controlled
export before the guarded write. Rev 0 does not define a portable encrypted-container format;
therefore a flow requiring portable sensitive backup cannot become `qualified` under rev 0.
Host-disk encryption alone may protect a temporary store but does not satisfy portable export.

## 10. Observable states

Each state declares:

- `id` and human label;
- transport (`adb`, `fastboot`, `webusb-fastboot`, `samsung-download`, `sahara-firehose`,
  `ssh`, `rauc`, or `manual`);
- mode probes and expected normalized results;
- USB mode record;
- commands permitted in the state;
- whether the normal OS, a recovery, or boot firmware is trusted for the probes;
- safe stopping instructions.

The common vocabulary is:

- `stock-os-adb`;
- `stock-recovery` / `adb-sideload`;
- `bootloader-fastboot`;
- `userspace-fastbootd`;
- `samsung-download`;
- `qualcomm-edl`;
- `uboot-shell` / `uefi-boot-manager`;
- `stock-os-booted`;
- `mura-first-boot`.

A target uses only observed states. Merely defining the vocabulary does not claim a state exists.

## 11. Operations

Every operation contains:

| Field | Requirement |
|---|---|
| `id`, `kind` | stable identity and one kind from below |
| `from`, `to` | accepted source states and expected destination |
| `capabilities` | permitted frontend(s) |
| `enabled` | false for research-only/unqualified writes |
| `guards` | all conditions that must pass immediately before execution |
| `command` | typed command object, never a shell string |
| `artifacts` / `partitions` | exact referenced records |
| `confirmation` | §13 identifier or `null` |
| `timeout` | operation and reconnect bounds |
| `postconditions` | probes required for success |
| `failureEdges` | failure class to recovery action |
| `idempotence` | `read-only`, `retry-safe`, `resume-only`, or `not-retry-safe` |

Kinds:

- `probe`;
- `download`;
- `verify-artifact`;
- `backup`;
- `load-programmer`;
- `transition`;
- `unlock`;
- `relock`;
- `flash`;
- `erase`;
- `format`;
- `reset-rollback-index`;
- `set-active-slot`;
- `reboot`;
- `verify-target`;
- `restore`.

Typed command families include `adb`, `fastboot`, `rauc`, `qdl`, `samsung-download`, `ssh` and
HTTP fetch. Their argument arrays contain no shell expansion. `load-programmer` records the
programmer artifact as authenticated code execution on the target, its accepted hardware/storage
identity and the commands it exposes. Samsung Download operations name PIT selection and exact
partition/archive members. Rollback-index operations name the index/location, before/after probes
and irreversibility. `restore` names either a static recovery artifact or a §9a unit backup result.

`manual` is restricted to physical actions and observations (hold buttons, confirm a device
prompt, reconnect a cable, read displayed state). It cannot wrap an arbitrary host command.
Vendor-specific commands carry a vendor namespace and an exact response grammar.

`format` names the exact partition and filesystem and is destructive. `relock` requires an
installed trust-root probe, compatible rollback state, wipe disclosure and confirmation, a clean
bootloader restart and authoritative lock-state readback. Disconnect or command success alone is
not proof.

An unknown or malformed response fails closed. In particular, an absent `unlocked`, product,
slot, partition, rollback, battery or mode result is not success.

## 12. Flows and graph rules

A flow names:

- entry states;
- ordered operation IDs or an explicit branch on a normalized probe;
- terminal success and safe-stop states;
- required backup set;
- recovery matrix;
- whether user data is wiped and at which transition;
- whether the stock OS remains bootable after each write.

The graph validator rejects:

1. an operation whose state is unreachable;
2. every destructive or state-changing operation—including unlock, relock, flash, erase, format,
   reset-rollback-index, set-active-slot and destructive restore—without an immediately preceding
   identity/build/mode revalidation;
3. an operation that consumes an artifact without an artifact revalidation;
4. a destructive operation without a target-specific confirmation;
5. a write to an unknown or protected partition;
6. a transition whose reconnect does not re-identify the same unit;
7. a success terminal lacking all postconditions;
8. a failure edge with no safe stopping or recovery instruction;
9. relocking without a proven installed trust root and compatible rollback state.

Parallel writes are forbidden. Downloads and independent read-only probes may run concurrently.

## 13. Human confirmations

A confirmation record states:

- the exact effect in plain language;
- affected partitions and whether user data is wiped;
- whether interruption can make stock boot unavailable;
- the available recovery path and its limits;
- the physical action or exact phrase required to proceed.

Unlock, erase, relock, boot-firmware write, GPT/LUN write, rollback-index change and first write to
an inactive slot each require a separate confirmation unless one physical device confirmation
unambiguously covers the same atomic action.

Defaults never select a destructive action. Relocking is an administrator choice and is offered
only when §12 rule 9 passes.

## 14. Slots, snapshots, rollback and battery

Before an A/B write the plan records and rechecks:

- current, inactive, successful and unbootable slots;
- slot suffix mapping;
- snapshot/update status;
- boot-attempt and mark-success mechanism;
- which firmware is shared, paired, or slotless.

A pending Virtual A/B snapshot must be handled by a target-specific documented operation; a
frontend may not silently cancel it.

Rollback data records each AVB index location, bootloader/SWREV floor, TrustZone or RPMB floor
known to matter, its source, and when it advances. An unmeasured downgrade is represented as
unknown, never as allowed.

Where the bootloader exposes `battery-soc-ok`, the guard must pass. Otherwise a target-specific
battery/charger requirement is required for writes; no universal percentage is invented.

## 15. Recovery matrix

For every write boundary, `recovery[]` states:

- failure detection;
- whether the normal OS, alternate slot, stock recovery, Download, EDL, U-Boot or external media
  remains reachable;
- exact recovery artifact and its digest;
- operations permitted;
- protected state that must survive;
- what cannot be recovered by this method.

“EDL exists” is not a recovery path. A qualified EDL recovery names the exact programmer,
storage geometry, rawprogram/patch plan or typed equivalent, and a proven way to preserve
per-unit state. A recovery requiring a booting ADB shell says so and cannot cover loss of both
bootable slots.

Stock/vendor recovery remains distinct from Mura's own recovery image
([recovery-menu](recovery-menu.md)).

## 16. Transcript and redaction

The installer emits JSON Lines. Each record contains:

- wall-clock and monotonic timestamp;
- plan digest and operation ID;
- normalized state before and after;
- command family and redacted arguments;
- normalized responses;
- duration, byte count and artifact digest;
- disconnect/reconnect events;
- result and failure class;
- host, frontend and tool versions.

The following never appear in a normal transcript: raw serials, IMEI/MEID, chip IDs, unlock
tokens, account credentials, URLs containing credentials, partition contents, calibration,
attestation material, Wi-Fi MACs or encryption keys. `redaction` declares target-specific response
fields and regular expressions in addition to this fixed set.

A diagnostic export is previewed before saving. The user may separately export an encrypted,
unit-bound backup; it is not a diagnostic attachment.

## 17. Frontend behavior

Both frontends:

- render the same states, confirmations, progress and failures;
- expose the structured transcript;
- permit cancel only at operation-defined safe boundaries;
- inhibit accidental exit during non-interruptible work;
- never substitute a manual command after a typed operation failed;
- resume only from a re-probed state allowed by the plan.

The WebUSB frontend additionally declares browser storage quota, wake-lock behavior, chooser and
reconnect prompts, interface filters and a CLI handoff for unsupported operations. WebUSB support
for fastboot does not imply browser support for ADB, Download or Firehose.

The native CLI additionally reports exact external-tool versions and captures stdout/stderr
without treating process exit zero as sufficient when postconditions exist.

## 18. Rev-0 review checklist and conformance fixtures

Rev-0 document review includes:

- example JSON syntax and unique IDs;
- graph rules in §12;
- artifact size/digest formatting;
- all operation references resolved;
- no enabled write under an unqualified target;
- all destructive operations confirmed;
- all write boundaries represented in recovery;
- all sensitive response fields redacted;
- generated human guide and both frontend views preserving operation order.

Transport fixtures cover:

- success, informational and failure responses;
- malformed length/status and timeout;
- disconnect before, during and after a write;
- wrong-device reconnect;
- same device with a changed USB identity;
- bootloader-fastboot ↔ fastbootd transition;
- slot and partition mismatch;
- bad hash/signature, truncated sparse image and payload over `max-download-size`;
- low-battery refusal;
- unrecognized vendor response.

These are host-side fixtures, not hardware qualification. Device-side fastboot conformance and
interrupted-write recovery remain separate hardware evidence.

## 19. Illustrative research-only fragment

This fragment is intentionally non-executable:

```json
{
  "schema": "org.mura.install-target/v1",
  "planId": "example-device-inspect",
  "revision": 1,
  "purpose": "inspect",
  "qualification": {
    "evidenceLevels": ["source-documented"],
    "hardwareStatus": "unqualified",
    "evidence": ["research-only-example"],
    "testedBuilds": [],
    "excludedBuilds": [],
    "unknowns": ["usb-bootloader-id"],
    "hardwareQualification": null,
    "writesEnabled": false
  },
  "target": {
    "vendor": "Example",
    "product": "Example Headset",
    "model": "EX-1",
    "codenames": [],
    "hardwareRevisions": [],
    "regions": [],
    "skus": [],
    "stockBuilds": ["EX1.20260927"],
    "bootloaderRevisions": [],
    "usbModes": [],
    "targetSelectionProbes": ["probe-product", "probe-build"],
    "sameUnitProbes": []
  },
  "host": {
    "capabilities": ["native-cli"]
  },
  "authentication": {
    "status": "test-only"
  },
  "artifacts": [],
  "partitions": [],
  "backups": [],
  "states": [
    {
      "id": "stock-os-adb",
      "transport": "adb",
      "probes": ["probe-product", "probe-build"],
      "commands": ["probe"],
      "safeStop": "Disconnect without changing the device."
    }
  ],
  "operations": [
    {
      "id": "probe-product",
      "kind": "probe",
      "from": ["stock-os-adb"],
      "to": "stock-os-adb",
      "capabilities": ["native-cli"],
      "enabled": true,
      "guards": [],
      "command": {
        "family": "adb",
        "argv": ["shell", "getprop", "ro.product.model"]
      },
      "artifacts": [],
      "partitions": [],
      "confirmation": null,
      "timeout": {"operationSeconds": 10, "reconnectSeconds": 0},
      "postconditions": [{"equals": "EX-1"}],
      "failureEdges": [{"class": "mismatch", "action": "stop"}],
      "idempotence": "read-only"
    }
  ],
  "flows": [
    {
      "id": "inspect",
      "entryStates": ["stock-os-adb"],
      "operations": ["probe-product"],
      "successState": "stock-os-adb",
      "safeStopStates": ["stock-os-adb"],
      "requiredBackups": [],
      "recovery": [],
      "wipesUserData": false
    }
  ],
  "recovery": [],
  "redaction": {
    "fields": ["serialno", "imei", "chip-id", "unlock-token"],
    "patterns": []
  }
}
```

## 20. Open deciders and reserved hooks

1. **Release authentication algorithm and key lifecycle.** Decider: the release-signing design;
   until then §2 mandates `test-only`.
2. **Machine-validatable schema and generated language types.** Rev 0 deliberately reserves
   `$schema` but is a prose data model and cannot be consumed as a release plan. Decider: owner
   ratification of a complete schema; no installer implementation or qualified write plan may
   precede it.
3. **Portable encrypted unit-backup container.** §9a specifies acquisition and runtime results,
   but not portable encryption. Decider: owner after a cryptographic-storage comparable pass. A
   target requiring exported sensitive backup cannot be `qualified` until this is specified.
4. **Browser support per non-fastboot transport.** Decider: an implementation audit plus host
   fixtures; the manifest capability remains `native-cli` or `manual` until then.

