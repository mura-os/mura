# 51 — Android boot and donor-extraction audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Per-target facts only; pipeline design remains in [06](06-donor-pipeline.md) and
`architecture/donor-pipeline.md`. Frame remains the case study in [33](33-steam-frame-donor.md).
The complete static unlock/backup/restore flows, including additional installer targets not in
doc 44's six-device hardware-enablement scope, are now canonical in
[72](72-xr-unlock-and-flash-targets.md); the host installer mechanism is [71](71-unlock-installer-precedents.md).
This audit continues to own donor/build provenance and protected-state facts rather than duplicate
those procedures.

## Verdict

| Device/profile | Boot/donor state | Mura state |
|---|---|---|
| Quest 1 / `monterey:base` | final-v50 hash known; exact header uninspected | A0 |
| Quest 1 / `monterey:mod:v29-abl-unlock` | hash-bound v29 header-v0 unlock payload; persistent unlock reported | A0; S1-ready after source pin |
| Lynx / `lynx-r1:base` | vendor-open and QDL documented; 1.4.1 download currently 404, stock header unresolved | A0 |
| Lynx / `lynx-r1:mod:postmarketos-mainline` | header-v2 packaging, hashes and debug-shell boot | R1 observed, not Mura-qualified |
| Galaxy XR / `sm-i610:base` | launch unlock; no exact donor/GPT/header/recovery closure | A0 |
| PFDM / `pfdm-mr:base` | one unlocked-unit FastBoot capture; partial A/B/EDL reports, no donor/map/restore | A0 |
| Frame / `deckard:base` | rootfs reconstruction verified; official recovery release gives the full LUN 0–2 GPT, EDL programmer, USB repair image and vendor install script ([74](74-steam-frame-recovery-image.md)); fuse/enforcement unread | donor S1; hardware A0 |
| Quest 3 / `eureka:base` | exact root-build hashes; root is not unlock; no safe flash path | A0 |

Software/build states are not profiles except where a maintained modification changes the durable
boot chain. Arcturus changes no boot chain; accessory package deltas remain references in its
domain audit.

## Per-target chain

### Quest 1

Final build `49845030443200410` is hash-bound externally, but its `boot.img` header/DTB/AVB layout
has not been inspected. Published Monterey maps establish paired boot firmware and per-unit
partitions but are not a final-v50 recheck. Protect `persist`, `private`, `vision`, identity,
security and modem-NV state.

The qualifying `monterey:mod:v29-abl-unlock` profile uses exact build
`16476800119700000`, stages a reduced 13-partition inactive-slot set, and has two maintained
implementations. The hash-pinned repack's `boot.img` is definitively header **v0**; that fact does
not transfer to final v50
([external] [unlock manifest](https://raw.githubusercontent.com/darknight1050/quest1-bootloader-unlocker-web/main/binaries/EXPECTED.sha256)).
Temporary root enables the process; clean-reboot persistent ABL unlock is the separate result.
Never touch both XBL/ABL slots, and do not expose a Mura image until final-donor header inspection
and recovery qualification.

### Lynx R1

Official open bootloader and QDL/Firehose recovery are vendor-documented. Public dumps show
multiple UFS LUNs, paired XBL/ABL/boot/recovery/DTBO/vbmeta, physical `super`, firmware and
protected state. The postmarketOS profile—not stock 1.4.1 inspection—sets header v2, 4096 pages,
DTB offset, DTBO and `super`
(`references/pmaports/device/testing/device-lynx-r1/deviceinfo:18-32`).
Official 1.4.1 provides MD5 only; acquisition must compute SHA-256/SRI.
On 2026-09-27 the portal still advertised MD5 `3c0e60e393d899403a4037c5c9fcb1d3`,
but its `ota-lynx-user-v1-4-1-6942b6a7eb266.zip` redirect and tested older links returned
object-store 404. The metadata remains vendor evidence; stock 1.4.1 artifact inspection is
currently blocked.

The mainline mod pins kernel/firmware packages and has external debug-shell R1 evidence. Preserve
QVR calibration and modem/security state; never use QFIL “Erase All.” Rev-0
`specs/install-target-manifest.md` can represent fastboot deployment plus QDL recovery and
unit-bound backups, but the vendor documents calibration restore and destructive QDL/QFIL—not a
read-only Firehose backup route. Flashing both boot slots to gain root cannot satisfy the pre-write
backup gate. No concrete Lynx plan or hardware qualification exists.

### Samsung Galaxy XR

Launch-era unlock was publicly demonstrated, and `I610UEU1AYIA` is separately reported as the
launch build; no preserved capture binds the successful unlock event to that full build string.
The community record says first update `I610UEU1AYKE` (2025-12-09) removed unlock and the
2026-04-08 update blocked downgrade
(`references/bootloader-unlock-wall-of-shame/brands/samsung/README.md:38-40`). Samsung's official
ledger establishes AYKE on 2025-12-09, `I610UEU2AZCI` on 2026-04-08, AZD8 on 2026-05-06 and AZF3
on 2026-07-07. AYIA and AYKE are `U1`; AZCI and later are `U2`. That U1→U2 transition strongly
explains why ordinary Samsung-signed flashing would reject a return to U1, but no SM-I610
`SW REV CHECK FAIL`/Odin transcript exists and not every U2 downgrade is thereby proven blocked.
Updating an already-unlocked unit and AYKE→AYIA behavior remain hardware deciders.

No exact launch FUS artifact/hash, PIT/GPT, LUN map,
boot/vendor_boot/init_boot/DTBO/super/A-B/header report or tested stock restore is pinned. Model
`SM-I610` and CSC—not `anorak`—must key acquisition. EFS/persist-class state is protected, but
the pinned Monado driver specifically identifies this unit's
`/mnt/vendor/efs/device_profile.textproto`, persist display calibration, QVR calibration and SSC
sensor registry classes
(`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:259-345`). Preserve them as
unit-bound state; never transplant them. Exact partition labels still need a dump. No flashable
contract exists.

### Play For Dream MR

FreeXR's linked image is visibly FastBoot Mode, not recovery, and shows one unit as
`PRODUCT_NAME - anorak`, `VARIANT - SXR UFS`, `SECURE BOOT - no`, `DEVICE STATE - unlocked`
(`references/freexr/targets/anorak/README.md:1-9`). It proves that unit's displayed state, not an
unlock method, retail policy or wholly unfused chain. Community reports establish A/B behavior,
one `boot.img` per slot, failed `fastboot boot`, delta-only OTA and a `misc`-selected recovery
loop. A custom SXR2250 EDL payload changed reboot reason to regain Fastboot without restoring
partitions; XBL-SC still verified a QTI signature, so the chain is selectively open.

No exact OTA build/hash, complete GPT, boot header, AVB chain, accepted general-purpose Firehose,
stock recovery, full firmware/restore or calibration map exists. The exact Windows ADB interface
is `34e2:4f07` / `MI_01`; Fastboot's USB ID is unknown. `flashMethod=none` remains the only safe
conclusion.

### Valve Steam Frame contrast

Doc 33 proves a hash-matching 10-GiB btrfs rootfs, A/B RAUC semantics, and PBL→XBL→U-Boot→rootfs
`/boot`. Its post-install source also proves per-slot `var-A/B`, reformats inactive `var`, and
syncs `/var` including the `/etc` overlay
(`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/batch2.txt:851-916`);
the `donor.nix` partition list omitted that pair until 2026-09-27. Doc 74 closes the physical map
from Valve's recovery release: LUN 0 = the eight-partition OS disk (no `syspersist` there),
LUN 1 = 28 A/B boot-firmware partitions byte-equal to `bootfw.tar.xz` (`uefi_a/b` is Valve's
U-Boot SPL), LUN 2 = U-Boot FIT/env/`uefivarstore`, LUN 3 = `syspersist` regenerated from the
calibration EEPROM. The vendor's first-install paths (QDL LUN images; USB `repair_device.sh`) and
its bootfw update transaction are source-verified. Bundle signature authentication, fuse state and
boot-ROM enforcement, the EFI-vs-native U-Boot order and any Mura hardware boot remain open.
VM R4 does not promote hardware flashing.

### Meta Quest 3

Exact vulnerable/patched builds and hashes support read-only acquisition through temporary root.
Cheese/IonStack explicitly do not unlock ABL and forbid boot/system writes
([external] [Cheese README](https://raw.githubusercontent.com/zhuowei/cheese/main/README.md)).
No authoritative Eureka GPT/header/recovery/programmer exists; Panther evidence cannot substitute.
This is acquisition-only with `flashMethod=none`.

## Additional unlock/install targets

[Research 72](72-xr-unlock-and-flash-targets.md) adds static installer records without promoting
them into doc 44's hardware-enablement matrix:

- Oculus Go: vendor-documented official unlock ZIP and `fastboot oem unlock`;
- Quest 2: direct unlock on exact vulnerable build `16476800118700000`; the newer-build WebUSB
  profile is bounded to starting build `50670960048600150`. Its source comment still calls
  ionstack absent, while the same revision's generated manifests pin the deployed binary
  (`references/quest1-bootloader-unlocker-web/src/data/profiles.ts:200-214`,
  `references/quest1-bootloader-unlocker-web/binaries/EXPECTED.sha256:8-9`), so it is a
  deployment claim rather than independently reproducible source;
- PICO 4/Pro `phoenix`: EDL engineering ABL/devinfo plus a serial-derived OEM command, with current
  source claiming ≤5.13.8. A warning about 5.13.8 may concern a separate kernel-root exploit and
  neither mechanism proves the other. Enterprise remains unsupported for EDL writes because the
  tool's DDR4-labelled selection conflicts with PICO's LPDDR5 specification and pinned recovery
  guidance; its LUN0–5 backups are bounded sector dumps with non-cryptographic validation, and
  engineering `devinfo` provenance/hash/source are absent;
- PICO Neo 3: maintainer-reported ≤5.11.2; the original discoverer personally tested only PICO 4;
- HTC Vive XR Elite: one community-reported standard unlock on `1.0.999.738`.

Their donor provenance, partition protection and recovery closure remain unknown where doc 72 says
so; none gains a write-enabled Mura plan from static research.

## Safe extraction and runtime gates

Before any write:

1. exact model/SKU/build/profile and full-versus-incremental artifact identified;
2. donor hash verified;
3. primary/backup GPT and all LUNs captured twice and CRC/hash checked;
4. boot headers, DTB/DTBO, AVB descriptors, rollback and slots inspected;
5. protected/per-unit partitions captured twice and stored separately;
6. unlock verified after clean reboot, not merely exploit-time root;
7. exact stock recovery demonstrated;
8. only a proven inactive slot is written and read back;
9. fallback/boot attempts/readiness/mark-successful behavior tested.

R1 enumerates exact storage/partitions/protocol endpoints; R2 proves read/hash and gated inactive-
slot transport; R3 verifies partition semantics, calibration ownership and revision; R4 boots/
updates/recovers without ambient exploit privilege; R5 covers interrupted update, fallback,
anti-rollback and repeat recovery.

## Contract consequences

`bootimg.headerVersion` is set only from the exact selected donor: v29 Quest 1 repack is v0;
postmarketOS Lynx is v2; all other audited donor claims remain `null`. Root never satisfies unlock.
`protectedPartitions` must fail closed when GPT is unknown. Frame `syspersist` and per-slot
`var-A/B` are definite, the current donor manifest omits `var-A/B`, and full physical boot-LUN
classification remains open. Profiles stay audit vocabulary.

## Contradictions / deciders

- Quest vulnerable full OTA and reduced unlock archive hashes: different artifacts, both valid.
- Lynx pmOS header v2 versus stock 1.4.1: restore official artifact availability, then inspect
  stock `boot.img`.
- Galaxy launch/update unlock durability: exact FUS packages and already-unlocked unit.
- Frame evidenced root/efi/var slot set plus separate boot LUN: complete GPT/LUN capture and
  reconcile `donor.nix`.
- PFDM one eFuse observation versus retail fleet: retail-unit fuse/recovery study.
- Quest 3 temporary root versus unlock claims: ABL state after clean reboot.
