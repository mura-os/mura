# 51 — Android boot and donor-extraction audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Per-target facts only; pipeline design remains in [06](06-donor-pipeline.md) and
`architecture/donor-pipeline.md`. Frame remains the case study in [33](33-steam-frame-donor.md).

## Verdict

| Device/profile | Boot/donor state | Mura state |
|---|---|---|
| Quest 1 / `monterey:base` | final-v50 hash known; exact header uninspected | A0 |
| Quest 1 / `monterey:mod:v29-abl-unlock` | hash-bound v29 header-v0 unlock payload; persistent unlock reported | A0; S1-ready after source pin |
| Lynx / `lynx-r1:base` | vendor-open, QDL recovery, exact partitions; stock header unresolved | A0 |
| Lynx / `lynx-r1:mod:postmarketos-mainline` | header-v2 packaging, hashes and debug-shell boot | R1 observed, not Mura-qualified |
| Galaxy XR / `sm-i610:base` | launch unlock; no exact donor/GPT/header/recovery closure | A0 |
| PFDM / `pfdm-mr:base` | one unlocked-unit report; no donor/map/recovery | A0 |
| Frame / `deckard:base` | rootfs reconstruction verified; physical boot-LUN/full-disk/recovery incomplete | donor S1; hardware A0 |
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
(`references/pmaports/device/testing/device-lynx-r1/deviceinfo:18-31`).
Official 1.4.1 provides MD5 only; acquisition must compute SHA-256/SRI.

The mainline mod pins kernel/firmware packages and has external debug-shell R1 evidence. Preserve
QVR calibration and modem/security state; never use QFIL “Erase All.” Current contract cannot
express fastboot normal deployment plus QDL recovery without a later design decision.

### Samsung Galaxy XR

Launch firmware was unlockable; later lockout/downgrade barriers are community-reported. No exact
launch FUS artifact/hash, PIT/GPT, LUN map, boot/vendor_boot/init_boot/DTBO/super/A-B/header report
or tested stock restore is pinned. Model `SM-I610` and CSC—not `anorak`—must key acquisition.
EFS/persist-class state is protected, but exact labels need a dump. No flashable contract exists.

### Play For Dream MR

FreeXR records one unit with unburnt eFuse
(`references/freexr/targets/anorak/README.md:1-9`); that is not retail/update-durable unlock.
No exact OTA build/hash, GPT, boot images/header, AVB chain, Firehose, recovery package, firmware
or calibration map exists. `flashMethod=none` is the only safe conclusion until owned-unit
read-only capture and restore proof.

### Valve Steam Frame contrast

Doc 33 proves a hash-matching 10-GiB btrfs rootfs, A/B RAUC semantics, and PBL→XBL→U-Boot→rootfs
`/boot`. It does **not** prove a complete disk map: `kernelsetup.sh` separately identifies a
`/dev/sdb` boot LUN with A/B XBL/U-Boot/bootfw. Bundle signature authentication, physical install,
U-Boot payload integration, bootfw handling and hardware recovery remain open. VM R4 does not
promote hardware flashing.

### Meta Quest 3

Exact vulnerable/patched builds and hashes support read-only acquisition through temporary root.
Cheese/IonStack explicitly do not unlock ABL and forbid boot/system writes
([external] [Cheese README](https://raw.githubusercontent.com/zhuowei/cheese/main/README.md)).
No authoritative Eureka GPT/header/recovery/programmer exists; Panther evidence cannot substitute.
This is acquisition-only with `flashMethod=none`.

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
`protectedPartitions` must fail closed when GPT is unknown. Frame `syspersist` is definite but full
boot-LUN classification remains open. Profiles stay audit vocabulary.

## Contradictions / deciders

- Quest vulnerable full OTA and reduced unlock archive hashes: different artifacts, both valid.
- Lynx pmOS header v2 versus stock 1.4.1: inspect stock `boot.img`.
- Galaxy launch/update unlock durability: exact FUS packages and already-unlocked unit.
- Frame “full disk” versus root/data slot subset plus separate boot LUN: complete GPT/LUN capture.
- PFDM one eFuse observation versus retail fleet: retail-unit fuse/recovery study.
- Quest 3 temporary root versus unlock claims: ABL state after clean reboot.
