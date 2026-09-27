# 71 — Unlock and installer precedents: one guarded plan, not a flash script

**Research date:** 2026-09-27. **Scope:** host-side bootloader unlock, backup, stock restore and
image installation mechanisms. This is static source/web research; no headset was connected and
no command was run against hardware. Per-target findings are in
[72](72-xr-unlock-and-flash-targets.md); the resulting normative contract is
[`specs/install-target-manifest.md`](../../specs/install-target-manifest.md).

## Verdict

No one comparable supplies Mura's whole answer:

- AOSP defines fastboot, fastbootd, dynamic-partition and verified-boot mechanisms.
- GrapheneOS has the strongest narrow WebUSB fastboot implementation and host guidance, but its
  image order, AVB custom-key and relock assumptions are Pixel-specific.
- UBports has the most useful heterogeneous, declarative device-flow shape, but its YAML is an
  installer configuration rather than release-authenticated evidence.
- the Quest 1/2 WebUSB unlocker has the strongest target-specific backup/write/readback/recovery
  gates, but it depends on a firmware-specific exploit and a still-booting untouched slot.
- the PICO tool proves that some XR unlock paths are multi-protocol EDL→fastboot procedures which
  cannot honestly be flattened into `flashMethod = "fastboot"`.

Mura therefore uses one authenticated, typed install plan as the source of truth for a future
native CLI and any WebUSB-capable subset. Shell commands and prose are generated views. Unknown
identity, build, mode, slot, unlock state, partition map, rollback floor, artifact digest or
recovery path fails closed.

## 1. Problem and evaluation criteria

The existing flasher table models one command, while the dangerous part is the state around it
([architecture review](../architecture/REVIEW.md) lines 45–55): device/build eligibility, a
unit-bound backup, transition between several USB identities, anti-rollback, user confirmation,
write verification, and recovery after each interruption point.

Each comparable is evaluated for:

1. device and build identity;
2. artifact authentication;
3. explicit boot states and transitions;
4. slot, dynamic-partition and rollback handling;
5. destructive confirmation;
6. readback/postcondition checks;
7. reconnect identity;
8. recovery and resumability;
9. structured diagnostics;
10. assumptions that transfer to heterogeneous XR hardware.

## 2. AOSP — protocol authority, not a complete installer

AOSP's fastboot README makes `fastboot-info.txt` the build-produced ordered operation list
(`references/aosp-system-core/fastboot/README.md:170-188`). It distinguishes bootloader fastboot
from userspace fastbootd with `getvar:is-userspace`
(`references/aosp-system-core/fastboot/README.md:205-234`) and defines the logical-partition
operations and `is-logical:<partition>` probe
(`references/aosp-system-core/fastboot/README.md:239-267`). The optimized path may construct a
whole-super task and avoid a fastbootd reboot, but only from a recognized operation sequence
(`references/aosp-system-core/fastboot/README.md:190-203`).

**Problem solved.** AOSP needs one host protocol to install physical boot firmware and logical
Android partitions whose ownership differs between bootloader and userspace.

**Why it chose this.** Boot-critical firmware belongs below Android; dynamic-partition metadata
and filesystems need userspace facilities. A build artifact, rather than host heuristics, describes
the order.

**Assumptions.** The device implements AOSP fastboot faithfully and the factory package describes
the exact product. It says nothing about Samsung Download, authenticated Firehose, exploit-time
root, vendor token acquisition or a unit-specific calibration backup.

**Transfer.** Fastboot variables and state names are normative wherever a target actually exposes
them. Mura adopts a typed operation list and separate bootloader/fastbootd states. It does not
assume fastboot exists, that every target supports every variable, or that `super` is safe to
overwrite.

## 3. GrapheneOS WebUSB and fastboot.js — strong narrow mechanism

The site gates on a supported Chromium-class browser, host OS, storage, cable and SKU constraints,
and documents Linux udev and `fwupd` interference
(`references/grapheneos-org/static/install/web.html:83-218`). Its page separates connect, unlock,
download, flash and lock, with device-side confirmation for both wiping transitions
(`references/grapheneos-org/static/install/web.html:264-341`).

The JavaScript:

- queries `unlocked` before issuing `flashing unlock`
  (`references/grapheneos-org/static/js/web-install.js:212-241`);
- queries `product` against a whitelist before choosing a release
  (`references/grapheneos-org/static/js/web-install.js:243-255`);
- caches the image in IndexedDB and exposes progress
  (`references/grapheneos-org/static/js/web-install.js:258-278`);
- exposes a reconnect callback during factory flashing
  (`references/grapheneos-org/static/js/web-install.js:280-336`);
- uses a wake lock and a before-unload guard during long operations
  (`references/grapheneos-org/static/js/web-install.js:23-50,493-500`).

The underlying library flashes bootloader and radio packs from bare-metal fastboot, reboots after
each bootloader update, checks package requirements, enters fastbootd for `super_empty`, returns to
the bootloader before AVB-key and wipe operations, then optionally installs the custom AVB key
(`references/grapheneos-fastboot-js/src/factory.ts:220-357`). This is the clearest source example
of mode-shaped partition ownership.

**Problem solved.** Install one OS on a tightly controlled Pixel set without requiring a native
binary on the host.

**Why it chose this.** Pixels share a known factory-package format, standards-conforming fastboot,
large browser support and AVB custom-key support. Reconnects are normal because bootloader
firmware and fastbootd re-enumerate.

**Assumptions that do not transfer.**

- Pixel partition order and custom-key/relock policy are not generic.
- `waitForConnect()` explicitly accepts the next connection regardless of whether it is the same
  device (`references/grapheneos-fastboot-js/src/fastboot.ts:197-213`); its USB connect listener
  replaces the current device with any matching connection
  (`references/grapheneos-fastboot-js/src/fastboot.ts:247-277`). That is unacceptable around
  heterogeneous or simultaneous headsets.
- The web page fetches the factory ZIP but does not reproduce the CLI guide's detached image
  signature verification. Post-install AVB-key verification is valuable but does not replace
  pre-write artifact authentication.
- The state bitmask only prevents conflicting page actions; it is not a durable recovery journal.

**Transfer.** Mura adopts browser capability checks, bounded storage, progress, wake lock,
before-unload guard, explicit reconnect prompts, mode-shaped operations and post-install trust-key
verification where a target supports it. It adds full identity/build/slot revalidation after every
reconnect and keeps the native CLI as the complete frontend.

## 4. GrapheneOS Python flasher — useful shape, unsafe defaults

The independent Python flasher verifies the factory archive with `ssh-keygen -Y verify`
(`references/grapheneos-flasher/grapheneos_flasher/core.py:168-238`), queries unlock state
(`references/grapheneos-flasher/grapheneos_flasher/core.py:297-315`), and waits with a bounded
timeout after an unlock
(`references/grapheneos-flasher/grapheneos_flasher/core.py:318-339,341-396`). Its small
orchestrator, result enum, UI separation and test-injected file handler are useful CLI structure.

It is not a safety authority:

- an unknown unlock state only warns and proceeds
  (`references/grapheneos-flasher/grapheneos_flasher/core.py:494-528`);
- after relock, disappearance is accepted as lock confirmation
  (`references/grapheneos-flasher/grapheneos_flasher/core.py:434-447`);
- loss of fastboot after flashing still returns success and leaves a manual lock reminder
  (`references/grapheneos-flasher/grapheneos_flasher/core.py:558-564`);
- actual partition semantics are delegated to an archive shell script.

**Transfer.** Use subprocess argument arrays, tool-version checks, bounded waits, test injection
and detached signature verification. Reject every unknown guard; process exit zero and disconnect
are never postconditions by themselves.

## 5. UBports Installer configs — heterogeneous state-plan precedent

The Fairphone 4 plan begins with model and exact stock-OS confirmations, supplies manual fallbacks
for bootloader/fastbootd/recovery entry, and asserts `unlocked = yes` before writing
(`references/ubports-installer-configs/v2/devices/FP4.yml:1-42,69-86`). It fixes the selected A/B
slot, downloads per-file SHA-256-bound boot artifacts, flashes named physical partitions, enters
fastbootd, deletes/resizes explicit logical partitions, returns to the bootloader, conditionally
wipes data and enters recovery
(`references/ubports-installer-configs/v2/devices/FP4.yml:87-197`).

**Problem solved.** One installer supports devices whose preconditions, manual actions, transports
and layouts differ.

**Why it chose this.** Device maintainers know the flow; a declarative plan lets the installer
provide common execution, fallback UI and downloads without hard-coding every product.

**Assumptions.** Maintainer-reviewed YAML and download hashes are enough for its release process;
the schema does not itself prove hardware behavior or bind a qualification record.

**Transfer.** Mura adopts explicit user actions, conditional branches, manual fallback states,
per-file hashes and typed transport actions. It adds release authentication, protected-partition
classes, reconnect identity, recovery edges for every write and separate static versus hardware
qualification.

## 6. Quest 1/2 WebUSB unlocker — strongest destructive-flow precedent

The current source describes Quest 1 build `16476800119700000` and Quest 2 build
`16476800118700000` as separate archives and patches
(`references/quest1-bootloader-unlocker-web/README.md:1-18`). Quest 1 has the complete final-build
downgrade path. Quest 2's profile comment says its current-build path is unusable until an ionstack
binary is supplied
(`references/quest1-bootloader-unlocker-web/src/data/profiles.ts:200-214`), but the same revision's generated manifests
pin both the Quest 2 archive and ionstack
(`references/quest1-bootloader-unlocker-web/binaries/EXPECTED.sha256:8-9`,
`references/quest1-bootloader-unlocker-web/src/data/asset-hashes.json:7-8`), and the deployed assets matched those hashes when retrieved
2026-09-27. The repository is internally stale: treat the inactive-slot path as a deployment claim
for exact starting build `50670960048600150`, not as independently reproducible source or support
for later firmware. A Quest 2 already on the vulnerable v29 bootloader can use the direct fastboot
path. The 13-step procedure identifies
the device, derives the payload, reads slots, gets temporary root, backs up and rehashes every
partition it will overwrite, writes only the inactive boot chain, verifies writes, boots the old
ABL, unlocks, restores the original active slot and inactive-slot backup, wipes data and lets
Android mark the slot successful
(`references/quest1-bootloader-unlocker-web/README.md:38-58`).

Source-level safeguards include:

- profile/build gates before a write
  (`references/quest1-bootloader-unlocker-web/src/lib/device.ts:150-290`);
- live-state-derived typed confirmations
  (`references/quest1-bootloader-unlocker-web/src/lib/flow.ts:270-333`);
- refusal if any intended partition is absent
  (`references/quest1-bootloader-unlocker-web/src/lib/partitions.ts:94-114`);
- browser-stored backup checked against both its recorded hash and a fresh live-partition hash
  (`references/quest1-bootloader-unlocker-web/src/lib/flow.ts:690-894`,
  `references/quest1-bootloader-unlocker-web/src/lib/partitions.ts:292-332`);
- push hash before `dd`, `sync`, and partition readback after each write
  (`references/quest1-bootloader-unlocker-web/src/lib/partitions.ts:232-290`);
- build-number check immediately before the exploit and authoritative unlock verification only
  after a clean bootloader restart
  (`references/quest1-bootloader-unlocker-web/src/lib/flow.ts:1080-1229`);
- original-slot and unlock-state rechecks after another clean restart
  (`references/quest1-bootloader-unlocker-web/src/lib/flow.ts:1237-1352`).

The recovery claim is deliberately narrow: browser backup restoration still needs a booting ADB
shell and exploit-time root; the untouched slot, not the backup, is what handles a failed
inactive-slot boot (`references/quest1-bootloader-unlocker-web/README.md:190-216`).

**Problem solved.** Temporarily execute a vulnerable ABL without permanently leaving old boot
firmware and without touching the running slot.

**Why it chose this.** Quest has no public signed Firehose recovery. A/B gives one recoverable
place to experiment, while live readback catches corrupt writes before activation.

**Assumptions.** At least one slot boots far enough for ADB and the build-specific root chain; the
bootloader honors retry fallback; the user preserves browser backup data; fuse rollback floors
have not advanced beyond the payload.

**Transfer.** Mura adopts exact-write-set backups, independent readback, live-state confirmations,
inactive-slot preference, post-clean-reboot verification, explicit recovery limits and refusal to
guess. It does not generalize the exploit, partition set or retry behavior to another target.

## 7. PICO tooling — why EDL is a separate transport

`more-picohaxx-tool` claims PICO 4/4 Pro through 5.13.8 and Neo 3 through 5.11.2
(`references/more-picohaxx-tool/README.md:16-21`). It offers physical-LUN, userdata and
per-partition backup modes (`references/more-picohaxx-tool/README.md:29-58`), then derives a headset-specific OEM command from the
Qualcomm chip serial, writes engineering ABL/devinfo in EDL, issues OEM/critical/normal unlock
commands, confirms persistence after a bootloader reboot, restores stock ABL/devinfo and factory
resets (`references/more-picohaxx-tool/README.md:65-80`). It reports that the state lives in RPMB
and may require repeated unlock attempts; restoring stock ABL keeps the unlocked state while
removing engineering-ABL boot instability
(`references/more-picohaxx-tool/README.md:120-133`).

The implementation is weaker than that summary in three safety-relevant ways. It writes both
`abl` and `devinfo` and restores `devinfo` only if its backup file exists
(`references/more-picohaxx-tool/picounlock.ps1:275-313,360-407`); Mura requires both originals.
Its whole-LUN loops cover 0–5 while comments/UI claim 0–6
(`references/more-picohaxx-tool/modules/qfilhelper.ps1:30-58,545-565`), and its backup verifier
checks presence/non-empty/total size rather than duplicate read hashes
(`references/more-picohaxx-tool/modules/backuprestore.ps1:843-921`). PICO 4 Enterprise is not a
consumer-PICO-4 alias: the tool routes it to its DDR4-labelled programmer while PICO specifies
LPDDR5, and no Enterprise reproduction is supplied. It remains unsupported for EDL writes.

**Problem solved.** Retail ABL requires a signed token which the operator does not have; an older
signed engineering path accepts a serial-derived command.

**Why it chose this.** Fastboot cannot install the enabling ABL while locked. A model-specific
Firehose can reach UFS below ABL; RPMB then carries the durable state.

**Assumptions.** The programmer accepts the exact memory/storage variant, old signed components
are not blocked by a lower-stage rollback floor, EDL entry is reliable, and original per-unit data
is recoverable.

**Transfer.** EDL/Sahara/Firehose is its own state and capability, never a fastboot argument
template. A plan names programmer, memory variant, LUN geometry, exact write set and recovery.
Browser support is not assumed; current evidence is a Windows/native tool. The firmware ceiling,
consumer/Pro programmer mapping and the Enterprise conflict remain target evidence questions in
[72](72-xr-unlock-and-flash-targets.md).

## 8. Existing distro precedents

postmarketOS's flasher table demonstrates the value of a device-declared action vocabulary, but
does not encode the pre-write state machine; the earlier audit and exact source map are
[research/02 §6](02-postmarketos.md). Mobile NixOS/Tow-Boot's installer checks board identity
before writing and deliberately invalidates a partially written install target so interruption
cannot leave plausible corrupt firmware ([research/01 §12](01-mobile-nixos.md)). Those reasons
transfer to raw media that has a safe alternate boot source; they do not license erasing the first
blocks of an internal headset LUN.

## 9. Derived Mura model

The normative result is [`specs/install-target-manifest.md`](../../specs/install-target-manifest.md):

1. one authenticated typed plan, interpreted by both frontends;
2. exact target/build/SKU/bootloader identity, rechecked after every reconnect;
3. observable states, each with mode-specific USB/interface evidence;
4. typed operations with immediate guards, artifacts, confirmations, postconditions and failure
   edges;
5. explicit partition classes and an exact permitted write set;
6. separate bootloader fastboot, fastbootd, Download, EDL and U-Boot/RAUC transports;
7. artifact signatures, hashes and sizes checked before use;
8. A/B, snapshot and rollback state as guards, not prose warnings;
9. a recovery matrix at every write boundary;
10. structured redacted transcripts and host-side fault fixtures;
11. qualification separate from syntax; this static pass enables no writes.

## 10. Rejected shapes

- **Parse `flash-all.sh`.** Unrecognized syntax, environment expansion and ignored lines turn a
  release procedure into an ad-hoc interpreter. Generate scripts from the typed plan instead.
- **One `flashMethod` enum.** It cannot express ADB exploit→EDL→fastboot or mode-specific
  partition ownership.
- **Disconnect means success.** Unlock, reboot and write commands routinely disconnect on both
  success and failure; a fresh probe is the postcondition.
- **Unknown means permissive.** Several community tools warn and continue when a variable is
  absent. Mura refuses a write.
- **Relock as a universal final step.** It is safe only with a proven installed trust root and
  compatible rollback state, and remains the administrator's choice.
- **EDL means recovery.** Recovery requires a model-specific accepted programmer, known geometry,
  complete stock artifacts and protected-state preservation.
- **Root means unlock.** Root after vendor boot says nothing about the next boot's signature
  enforcement.

## 11. Contradictions and deciders

| Claim | Evidence conflict | Current verdict | Decider |
|---|---|---|---|
| WebUSB can be the universal installer | GrapheneOS proves fastboot; PICO evidence needs native EDL and drivers | WebUSB is a per-operation capability, not the architecture | implementation and host fixtures for each transport |
| A reconnect identifies the same device | GrapheneOS fastboot.js accepts the next matching connection | Mura re-identifies target/build/unit after every reconnect | manifest conformance fixture with two devices |
| Package hash is enough | UBports hashes files; GrapheneOS CLI also authenticates the release signer | Mura authenticates the plan/artifact set and hashes each payload | release-signing design |
| Backup means unbrick | Quest backup restore needs a booting ADB shell | every recovery claim states its required surviving state | per-target recovery evidence |
| Unlock implies relock is desirable | GrapheneOS has Pixel AVB custom keys; XR targets usually do not | no generic relock step | target trust-root and rollback proof plus administrator choice |

## 12. Source boundary

Pinned source revisions are in [`references/MANIFEST.json`](../../references/MANIFEST.json).
External vendor pages and current firmware reports used for target facts are indexed in
[72 §External sources](72-xr-unlock-and-flash-targets.md). This document cites consumer XR
platforms only for mechanisms and failure evidence, never as Mura policy authorities.

