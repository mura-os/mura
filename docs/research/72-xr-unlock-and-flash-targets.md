# 72 — XR bootloader unlock and flash targets

**Research date:** 2026-09-27. **Method:** pinned source and artifact inspection plus dated vendor
and community web sources. **No target hardware was available:** no USB descriptor, partition,
unlock, write, reboot or restore result in this document is first-hand Mura evidence.
**Scope:** owner-authorized bootloader unlock, unit backup, stock recovery and the prerequisites
for eventually writing Mura images. Linux hardware enablement is out of scope. Installer
mechanisms are in [71](71-unlock-installer-precedents.md); boot/donor provenance remains indexed
by [51](51-android-boot-donor-extraction-audit.md).

## 1. Evidence vocabulary and verdicts

- **SOURCE-VERIFIED** — inspected pinned source or an exact hash-bound artifact.
- **VENDOR-DOCUMENTED** — first-party instructions or release record.
- **COMMUNITY-REPRODUCED** — a complete public flow tied to an exact target/build, but not run by
  Mura.
- **COMMUNITY-REPORTED** — an observation without a complete reproducible evidence chain.
- **INFERRED** — adjacent platform behavior; never enough for a write.
- **UNKNOWN** — no defensible evidence.

“Unlock” means a cleanly rebooted bootloader accepts non-vendor images. Root after the vendor
kernel has booted is not unlock. “Restore” means a documented path with accepted stock artifacts
and protected-state rules, not merely the existence of a recovery mode or EDL USB identity.

| Target | Static verdict | Best public path | Mura install status |
|---|---|---|---|
| Galaxy XR `SM-I610`, launch era | unlock was observed; AYIA is separately reported as launch build | OEM unlock + Samsung boot mode, exact event/build binding and transport incomplete | hardware-unqualified |
| Galaxy XR `AYKE` and later | AYKE removal reported; no later public unlock path; AYKE→AYIA unknown, U2→U1 normally SWREV-blocked | no complete public custom-image path or SM-I610 failure transcript | unsupported for writes |
| Play For Dream MR | one unburnt-eFuse unit reported | no retail recovery/install closure | hardware-unqualified |
| PICO 4 / 4 Pro `phoenix` | community unlock | model-specific EDL programmer + engineering ABL/devinfo → OEM fastboot token | source-documented, hardware-unqualified |
| PICO 4 Enterprise `phoenix` | tool claim conflicts with vendor memory specification | no safe programmer selection established | unsupported for EDL writes |
| PICO Neo 3 | maintainer reports support through 5.11.2 | same family, different entry/buttons/artifacts; original discoverer tested only PICO 4 | maintainer-reported, hardware-unqualified |
| Steam Frame `deckard` | administrator access, cryptographic unlock unknown | SSH/RAUC/U-Boot evidence, no alternate-OS procedure | hardware-unqualified |
| Lynx R1 | vendor says bootloader is open | fastboot for custom images; QDL/QFIL stock restore | vendor-documented |
| Oculus Go `pacific` | official unlock | Meta unlock ZIP → `fastboot oem unlock` | vendor-documented |
| Quest 1 `monterey` | current community unlock | inactive-slot v29 boot-chain downgrade + ABL exploit | source-documented, hardware-unqualified |
| Quest 2 `hollywood` | old v29 unlock; one newer start build claimed | direct vulnerable ABL or deployment-claimed inactive-slot flow for `50670960048600150` | build-bounded, hardware-unqualified |
| Quest Pro / 3 / 3S | temporary root only | no public persistent retail unlock | unsupported for writes |
| HTC Vive XR Elite `kyoto` | one guide author reports unlock on displayed release `1.0.999.738` | standard `fastboot flashing unlock`; exact fingerprint/SKU/bootloader unknown | community-reported, hardware-unqualified |

No row says a Mura image is flashable. It says what an eventual installer would have to prove.

## 2. Common pre-write evidence

Every target needs all of these before a Mura write plan can be enabled:

1. exact retail model, SKU/region, hardware revision, stock build and bootloader revision;
2. mode-specific USB descriptors and host drivers;
3. persistent unlock verified after a clean bootloader restart;
4. all physical LUNs, primary/backup GPTs, slots and dynamic partitions mapped;
5. AVB chain, secure-boot trust root and every observed rollback floor;
6. protected per-unit state identified and read twice with matching hashes;
7. exact stock artifacts and a restore path whose limitations are stated;
8. a write boundary that preserves an independently bootable recovery route;
9. target-specific confirmation, readback/postconditions and interruption handling.

The common future read-only capture set is:

```text
adb shell getprop ro.product.manufacturer
adb shell getprop ro.product.model
adb shell getprop ro.product.device
adb shell getprop ro.build.fingerprint
adb shell getprop ro.build.id
adb shell getprop ro.build.version.incremental
adb shell getprop ro.build.version.security_patch
adb shell getprop ro.boot.hardware.sku
adb shell getprop ro.boot.boot_devices
adb shell getprop ro.boot.slot_suffix
adb shell getprop ro.boot.flash.locked
adb shell getprop ro.boot.vbmeta.device_state
adb shell getprop ro.boot.vbmeta.digest
adb shell getprop ro.boot.verifiedbootstate
adb shell cat /proc/cmdline

fastboot getvar product
fastboot getvar version-bootloader
fastboot getvar hw-revision
fastboot getvar variant
fastboot getvar max-download-size
fastboot getvar current-slot
fastboot getvar slot-count
fastboot getvar unlocked
fastboot getvar secure
fastboot flashing get_unlock_ability
fastboot getvar is-userspace
fastboot getvar snapshot-update-status
fastboot getvar super-partition-name
fastboot getvar battery-soc-ok
```

Only run the fastboot probes on a target already known to expose that mode. Do not publish
`fastboot getvar all`: it commonly includes serials and unit identifiers. Host descriptor capture
uses `lsusb` followed by `lsusb -v -d <vid>:<pid>` for each independently entered mode; serials
and MACs are redacted.

For every partition named by the exact target plan, capture supported
`has-slot:<partition>`, `partition-size:<partition>`, `partition-type:<partition>` and
`is-logical:<partition>` variables individually. For every reported slot, capture
`slot-successful:<slot>` and `slot-unbootable:<slot>`. An unsupported variable is recorded as
unsupported rather than replaced with a neighboring device's answer.

With root or recovery read access, future capture adds `/dev/block/*/by-name`, `/proc/partitions`,
`ls -l /sys/class/block`, duplicate GPT/LUN reads, and offline:

```text
unpack_bootimg --boot_img copied-boot.img --out boot-unpacked
avbtool info_image --image copied-vbmeta.img
lpdump copied-super.img
sgdisk --print copied-disk-or-lun.img
sha256sum first-read.img second-read.img
```

These are capture instructions, not results from this research.

## 3. Samsung Galaxy XR (`SM-I610`)

### 3.1 Build matrix

| Build | Public position | Unlock / rollback verdict |
|---|---|---|
| `I610UEU1AYIA` | separately reported launch firmware | no preserved capture binds the successful launch unlock event to this full string; exact package/PIT/transport absent |
| `I610UEU1AYKE` | official ledger: 2025-12-09 | unlock removal remains community-reported; AYIA and AYKE are both `U1`, so an irreversible AYKE→AYIA floor is not established |
| `I610UEU2AZCI` | official ledger: 2026-04-08 | first ledgered `U2`; strong target-specific SWREV explanation for ordinary Samsung-signed return to `U1`, but no SM-I610 failure transcript |
| `I610UEU2AZD8` | official ledger: 2026-05-06 | `U2`; no restoration of OEM unlock reported |
| `I610UEU2AZF3` | official ledger: 2026-07-07 | `U2`; no restoration of OEM unlock reported |

Launch-era reporting showed an OEM Unlock toggle and a completed unlock, but did not preserve the
full build identity, bootloader revision or transcript. Separate reports identify AYIA as the
launch build; the two facts are not a captured build binding. Samsung published no SM-I610 unlock
guide: **[external]** Android Authority, “Samsung allows bootloader unlocking on Galaxy XR,”
October 2025; SamMobile, “Galaxy XR bootloader unlock is possible,” 2025-10-26.

Samsung's official SM-I610 build ledger establishes AYKE, AZCI, AZD8 and AZF3 with the dates in
the table. The pinned community survey says AYKE removed unlocking and the 2026-04-08 update
“seemingly” prevented downgrade
(`references/bootloader-unlock-wall-of-shame/brands/samsung/README.md:38-40`). The transition from
Samsung bootloader binary revision `U1` to `U2` is a strong target-specific SWREV explanation for
why normal Samsung-signed flashing would reject a return to `U1`. It does not prove every downgrade
from every `U2` build is blocked: no SM-I610 `SW REV CHECK FAIL` screen, Odin transcript, package
comparison or attempted `U2`→older-`U2` downgrade is public.

### 3.2 What is documented and what is not

**COMMUNITY-REPORTED at launch:** enable Android developer options and OEM unlocking, enter the
device's bootloader confirmation flow, accept the data wipe, then observe unlocked state. Public
coverage does not preserve the exact key sequence, fastboot/Odin command, Download screen fields,
USB identity or post-clean-reboot transcript. It proves eligibility, not an automatable plan.

**COMMUNITY-REPORTED after launch:** AYKE removes eligibility; April firmware blocks the old
firmware; an update of an already-unlocked unit may relock it. No exact already-unlocked
AYIA→AYKE/AZCI controlled transcript was found. Generic One UI 8 phone behavior in the same
survey (`references/bootloader-unlock-wall-of-shame/brands/samsung/README.md:8-14`) is a warning,
not Galaxy XR evidence.

**SOURCE-VERIFIED protected-state classes:** the pinned Galaxy XR Monado driver identifies this
unit's authoritative display/camera calibration at
`/mnt/vendor/efs/device_profile.textproto`, per-unit panel color calibration under
`/mnt/vendor/persist/display/`, separate QVR calibration under `/data/vendor/qvr/`, and the SSC
sensor registry under `/mnt/vendor/persist/sensors/`
(`references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:259-345`). They are backup-only,
unit-bound state; never transplant them.

**UNKNOWN:** PIT/GPT and LUN layout; A/B status; `boot`, `init_boot`, `vendor_boot`, DTBO and
`super`; Android boot header; AVB descriptors/indexes; whether SM-I610 exposes fastboot,
fastbootd or only Samsung Download; FUS model/CSC package availability for AYIA; stock restore;
partition labels containing the EFS/persist/QVR classes above; exact Knox consequences.

### 3.3 Static flow and hard stops

The only defensible static flow is:

1. identify `SM-I610`, CSC/SKU, full build and bootloader binary revision in the running OS;
2. archive and hash the exact matching FUS/OTA and all available source packages without flashing;
3. photograph and transcribe the complete Download/bootloader screen;
4. on AYIA only, document the device-side OEM-unlock interaction and wipe;
5. restart the bootloader and independently read lock state;
6. inspect PIT/images/AVB offline and capture protected state before defining any write.

An installer MUST refuse every build except an exact AYIA specimen whose full identity and unlock
eligibility have been independently captured; this also excludes unknown intermediate `U1`
builds. It MUST NOT offer AYIA downgrade merely because the Samsung binary field matches: a
within-`U1` AVB/TrustZone floor may exist. It MUST NOT update an unlocked unit, relock it, or
infer Odin semantics from Samsung phones.

### 3.4 Future hardware capture

In addition to §2:

- record Settings' OEM-unlock eligibility before and after network setup;
- photograph Download Mode fields `PRODUCT NAME`, `CURRENT BINARY`, `SYSTEM STATUS`, `OEM LOCK`,
  `FRP LOCK`, `KG STATUS`, `RP SWREV`, `WARRANTY VOID` and `SECURE DOWNLOAD`;
- capture VID:PID/interfaces in normal, ADB, recovery, Download and fastboot/fastbootd if present;
- obtain AYIA, AYKE and first-U2 packages without flashing; hash every BL/AP/CP/CSC/PIT member;
- compare PIT and `avbtool info_image` output offline;
- on separate eligible specimens, record AYKE→AYIA signed downgrade and already-unlocked
  AYIA→AYKE behavior. Those are destructive later tests, not part of this task.

**Static verdict:** hardware-unqualified. AYIA eligibility is credible; no complete safe Mura write
or stock-restore state machine exists.

## 4. Play For Dream MR / YVR / QWR XRone (`MRD3A0`)

FreeXR contains one target note: “Bootloader unlocked, efuse unburnt by vendor,” linked to a
single photograph (`references/freexr/targets/anorak/README.md:1-9`). The image visibly shows
**FastBoot Mode**, not recovery, with `PRODUCT_NAME - anorak`, `VARIANT - SXR UFS`,
`SECURE BOOT - no`, and `DEVICE STATE - unlocked`. This proves the displayed state of one unit,
not an unlock method, retail policy, update durability or a wholly unfused chain. The parent
matrix repeats that status (`references/freexr/README.md:7-17`).

**VENDOR-DOCUMENTED:** Android/DreamOS development and APK installation over ADB; the vendor
provides a Windows ADB package. **SOURCE-VERIFIED:** the vendor driver identifies an ADB interface
at `34e2:4f07`/`MI_01` (see [research/55 §4.4](55-usb-identities-and-gadget-policy.md)).

**COMMUNITY-REPORTED on that unit:** A/B boot behavior and a `boot.img` per slot; returning to
boot slot A looped, locally built images failed with `fastboot boot`, OTAs were delta-only, and
writing a recovery image over slot A left `misc` selecting recovery repeatedly. A custom SXR2250
EDL program later changed the reboot reason to bootloader and regained Fastboot; it did not restore
the damaged partitions. Follow-up analysis found that XBL-SC still verified a QTI signature even
on the apparently unfused unit. The observed chain is therefore selectively open, not
wholly unverified.

**UNKNOWN:** retail fuse state; OEM unlock authorization or method; bootloader/fastboot/fastbootd
USB IDs; stock recovery and button entry; an accepted general-purpose Firehose; complete GPT and
protected-state map; boot headers; AVB and rollback; full OTA/firmware archive; stock restore;
regional/enterprise differences; calibration partitions. The QWR ADB enabler independently binds
the exact Windows WinUSB ADB interface `34e2:4f07` / `MI_01`; it says nothing about Fastboot or
recovery. There is no public QFIL package or complete unlock procedure.

### Static flow and future capture

There is no write flow. The future zero-write sequence is:

1. record `MRD3A0`, region, storage SKU, consumer/enterprise identity and DreamOS build;
2. capture normal/ADB descriptors and §2 properties;
3. archive the exact OTA URL/headers/package when the stock updater downloads it;
4. inspect its metadata, payload manifest, boot images and AVB data offline;
5. record fuse/lock state only through already-exposed read-only properties.

Mode-changing probes such as `adb reboot bootloader`, `adb reboot fastboot`, `adb reboot recovery`
or `adb reboot edl` are not zero-risk inventory. A future owner should attempt them one at a time
only after the physical key exit/recovery path is known, capturing VID:PID and read-only variables.
No generic Firehose should be sent to the sole specimen.

**Static verdict:** hardware-unqualified, `writesEnabled = false`.

## 5. PICO 4 family (`phoenix`)

### 5.1 Product and firmware boundary

The pico4.wiki guide explicitly covers PICO 4, PICO 4 Pro and PICO 4 Enterprise under `phoenix`
and excludes PICO 4 Ultra/Ultra Enterprise `sparrow`: **[external]** pico4.wiki “Rooting,”
retrieved 2026-09-27. The pinned tool claims:

- PICO 4 and PICO 4 Pro: 5.13.8 and below;
- PICO Neo 3: 5.11.2 and below
  (`references/more-picohaxx-tool/README.md:16-21`).

This newer source claim appears to conflict with the earlier warning supplied to this research
(“do not update to 5.13.8”), but the warning may concern a separate kernel-root exploit while
more-picohaxx uses EDL plus an old engineering ABL. The mechanism mismatch means either claim can
be true without proving or disproving the other. The tool now links two region-specific
`5.13.8` Phoenix archives dated 2026-09-02/03
(`references/more-picohaxx-tool/modules/backuprestore.ps1:480-527`). There is no public
per-device transcript proving all three Phoenix variants on those exact builds. Keep the
contradiction open until upstream supplies logs or a later hardware task reproduces it.

Enterprise is named in the tool's scope but omitted from its “confirmed working” status table.
More seriously, the tool selects its “DDR 4” programmer for Enterprise
(`references/more-picohaxx-tool/modules/utils.ps1:343-364`), while PICO's official Enterprise
specification says LPDDR5: **[external]**
[PICO 4 Enterprise specifications](https://www.picoxr.com/sg/products/pico4e/specs), retrieved
2026-09-27. The pinned PICO documentation independently warns that
`prog_firehose_ddr.elf` initializes DDR4 and must not be used on Pro/Enterprise DDR5 variants
(`references/pico-documentation/recovery/README.md:38-43`). This unresolved programmer-selection
conflict makes Enterprise **unsupported for EDL operations**, not merely unconfirmed.

### 5.2 Mechanism

The source-described flow is:

1. enable USB debugging and read the Qualcomm SoC serial from
   `/sys/devices/soc0/serial_number` (sensitive; never publish it);
2. make duplicate bounded LUN-sector and exact-partition backups before a write;
3. derive `fastboot oem pico<token> unlock` from the serial;
4. enter Qualcomm EDL and use the matching programmer to stage old engineering ABL and `devinfo`;
5. enter bootloader fastboot and run the target-derived OEM command, then
   `fastboot flashing unlock_critical`, `fastboot flashing unlock`, and
   `fastboot oem setenforce 0`;
6. `fastboot reboot-bootloader` and read `fastboot oem device-info`;
7. repeat only the unlock commands if the RPMB bits did not persist;
8. restore and read back the unit's original `abl` **and `devinfo`,** not another unit's;
9. boot recovery and accept the required factory reset.

The token algorithm and rationale are inspectable at
`references/more-picohaxx-tool/more-picohaxx.py:1-15,25-95`. The operational sequence is also
summarized at `references/more-picohaxx-tool/README.md:65-80`. The source says unlock state is
stored in protected RPMB, may not stick on the first attempt, and survives restoring stock ABL
(`references/more-picohaxx-tool/README.md:120-133`). That last property is why the engineering
ABL should be temporary.

### 5.3 Memory/programmer split

For the consumer models, PowerShell selects and labels:

- `prog_firehose_ddr.elf`, “DDR 4,” for PICO 4 and Neo 3;
- `prog_firehose_lite.elf`, “DDR 5,” for PICO 4 Pro
  (`references/more-picohaxx-tool/picounlock.ps1:33-34,93-94`;
  `references/more-picohaxx-tool/modules/utils.ps1:343-364,481-489`).

This proves the tool's model→loader mapping and its own DDR labels, not the physical memory
topology or loader interchangeability independently. A Mura plan records both the source labels
and exact filenames/hashes, and selecting the wrong programmer is a hard stop.
PICO 4 Enterprise inherits neither consumer verdict: no programmer is selected until the conflict
in §5.1 is resolved by accepted-loader evidence.

### 5.4 Backup, recovery and edge cases

The tool offers LUN 0–6, partition, full-userdata and used-userdata modes, but marks the
used-sector userdata mode experimental (`references/more-picohaxx-tool/README.md:29-58`). The
implementation actually loops LUN 0–5 despite comments/UI saying 0–6 and creates bounded
sector-range dumps for each LUN; these are not literal complete physical UFS images
(`references/more-picohaxx-tool/modules/qfilhelper.ps1:30-58,545-565`). Its `Verify-Backup`
checks required filenames, non-empty files and a minimum total size, not cryptographic hashes or
duplicate device reads (`references/more-picohaxx-tool/modules/backuprestore.ps1:843-921`).
For boot-chain safety only duplicate required-range/partition reads with matching hashes qualify.
Backup metadata
must bind model, build, UFS geometry and unit hash; calibration/identity content stays encrypted
and unit-bound.

The unlock staging code separately reads original `abl` and `devinfo`, then writes engineering
versions of both (`references/more-picohaxx-tool/picounlock.ps1:275-313`). Its restore writes
`abl` and writes `devinfo` only when that file exists
(`references/more-picohaxx-tool/picounlock.ps1:360-407`). Mura therefore requires both
originals to be present, independently hashed, restored and read back; an ABL-only restore is
incomplete. The engineering `devinfo` payload's provenance, source and cryptographic hash are
absent from the pinned clone, so source visibility of the write sequence does not authenticate
that input.

Known edge cases from source:

- unlocking wipes user data (`references/more-picohaxx-tool/README.md:10-14`);
- an engineering ABL can boot slowly or fall into EDL; restore stock ABL/devinfo
  (`references/more-picohaxx-tool/README.md:125-133`);
- downgrade can create keystore/SELinux mismatch and boot loops, requiring a reset
  (`references/more-picohaxx-tool/README.md:105-117`);
- USB 2.0 is suggested for unstable EDL (`references/more-picohaxx-tool/README.md:135-138`);
- relocking wipes data and is not a generic Mura final step
  (`references/more-picohaxx-tool/README.md:179-185`);
- “custom image via EDL” is asserted (`references/more-picohaxx-tool/README.md:101-103`) but no
  partition-safe Mura flow is specified.

EDL is not automatically recovery. A valid record must identify the exact accepted programmer,
UFS LUN geometry, original GPTs, complete stock payload and which per-unit partitions the restore
plan excludes.

### 5.5 Future capture

Zero-write while Android boots:

```text
adb shell getprop ro.product.device
adb shell getprop ro.product.model
adb shell getprop ro.build.version.incremental
adb shell getprop ro.boot.slot_suffix
adb shell cat /sys/devices/soc0/serial_number   # private capture; redact the value
adb shell ls -l /dev/block/bootdevice/by-name  # may require root
```

After separately proving the exact programmer, a future EDL **read** capture may use its tool's
equivalent of `printgpt --lun <n>`, duplicate LUN reads, and two independent
`read-part abl` / `read-part devinfo` captures. Exact syntax must come from the hash-bound
programmer/tool revision. Loading a Firehose executes OEM-signed code and is risk-bearing even if
the requested operation is read-only. Do not test a generic loader, run `erase`, restore a whole
LUN, or publish RPMB/serial/calibration data.

Capture normal ADB, recovery, fastboot and 9008 descriptors; programmer `getstorageinfo`; every
LUN size/block size; both GPT headers; partition hashes; AVB indexes; clean-reboot unlock state;
and stock-ABL-restored unlock state.

**Static verdict:** source-documented, hardware-unqualified. No Mura write action is enabled.

## 6. PICO Neo 3

Neo 3 is a separate record because button entry, supported firmware and image set differ. The
pinned tool maintainer reports 5.11.2 and below
(`references/more-picohaxx-tool/README.md:18-21`) and documents:

- recovery: Volume Up + Power + Home;
- fastboot: Volume Down + Power + Home;
- EDL: enter fastboot, then use the tool's reboot menu
  (`references/more-picohaxx-tool/README.md:162-169`).

The underlying early Neo 3 ABL and standard Firehose are also the components described by the
manual method (`references/more-picohaxx-tool/more-picohaxx.py:48-69`). Its original discoverer
explicitly says only PICO 4 was personally tested and Neo 3 was assumed
(`references/more-picohaxx-tool/more-picohaxx.py:29-41`). This does not confirm Neo 3 or prove one
universal partition map across consumer, Pro and Enterprise variants.

Use the §5 flow only after exact SKU/build/programmer and partition closure. Capture model,
regional firmware, button modes, descriptors, UFS geometry, GPT, protected calibration, A/B
status, unlock persistence and stock restore independently from Phoenix.

**Static verdict:** maintainer-reported through 5.11.2, exact variants unknown and
hardware-unqualified.

## 7. Valve Steam Frame (`deckard`)

Valve documents Developer Mode with SSH and a `steamos` administrator who can use `sudo` and
disable the read-only root: **[external]** Valve Steamworks, “Steam Frame Debugging,” retrieved
2026-09-27. This is administrator access after the vendor boot chain; it is not cryptographic
bootloader unlock.

Pinned donor inspection proves:

- RAUC A/B raw rootfs slots and `/dev/disk/by-partsets/{A,B}/rootfs`, plus per-slot `var`
  partitions that the post-install handler reformats and synchronizes, including the `/etc`
  overlay
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/batch2.txt:851-916`);
- a separate `/dev/sdb` boot LUN with paired XBL/boot firmware;
- Qualcomm PBL→XBL→Valve U-Boot→kernel/initrd/DTB in rootfs `/boot`
  ([research/33 §3](33-steam-frame-donor.md));
- public RAUC/casync update artifacts, but incomplete bundle-signature trust-chain reconstruction
  ([research/33 §1](33-steam-frame-donor.md)).

**UNKNOWN on hardware:** secure-boot fuse policy; U-Boot console/menu and verified-boot policy;
external-media boot; accepted unsigned EFI/Linux payloads; EDL programmer; complete physical-LUN
map; rescue image; boot-firmware update transaction; persistent UEFI variables; factory/calibration
partitions.

The current device manifest's `flashMethod = rauc` is only a coarse capability label; the typed
install plan would have to describe the in-system inactive-root update after SteamOS is already
booted. Neither establishes first installation onto blank or replaced storage. There is no unlock
flow. An eventual first-install flow must first choose
between a vendor-supported alternate boot, a U-Boot boot target, or an internal-slot write with an
independently bootable rescue path. Root access alone cannot decide that.

The locally audited 2026-09-21 donor is a pinned specimen, not a channel-independent “latest”:
the public `vr/` index already contained builds through 2026-09-25 when checked on 2026-09-27.
This caveat does not change the pinned donor or its static partition evidence.

Future read-only capture:

```text
uname -a
cat /proc/cmdline
lsblk -o NAME,MAJ:MIN,SIZE,TYPE,FSTYPE,PARTTYPE,PARTLABEL,PARTUUID,MOUNTPOINTS
findmnt
rauc status --detailed
bootctl status
efibootmgr -v
mokutil --sb-state
fwupdmgr get-devices
fw_printenv
```

Commands absent from the stock image are recorded as “tool unavailable,” not inferred. Also copy
and hash `/boot`, RAUC configuration, U-Boot environment/scripts and boot updater scripts without
modifying them. Redact UUIDs, serials, MACs and keys.

**Static verdict:** hardware-unqualified. Developer Mode is useful for capture and stock-slot
inspection, not proof that Mura can replace SteamOS.

## 8. Lynx R1

Lynx is the strongest Android target because the vendor states: “As the bootloader is open, it is
possible to flash a different system image,” and provides system/calibration restore
**[external]** Lynx Getting Started v30, retrieved 2026-09-27.

The vendor's update/restore page documents:

- version-selected ADB sideload, including reversing an update;
- QFIL/QDLoader 9008 full restore on Windows;
- a hard warning not to enable “Erase All Before Download,” because it loses distortion and
  calibration files;
- Power + Volume Up + Volume Down to expose 9008 when needed;
- QDL on Linux/macOS with the firmware's `prog_firehose_ddr.elf`, `rawprogram*.xml` and
  `patch*.xml`, entered with `adb reboot edl`;
- temporary ModemManager disablement on Linux.

Source: **[external]** Lynx “Updating your device” v15, retrieved 2026-09-27. The vendor command is
a destructive stock-restore operation; it is evidence for the artifact set and recovery transport,
not a command Mura ran.

On 2026-09-27 the official portal still listed 1.4.1 with MD5
`3c0e60e393d899403a4037c5c9fcb1d3`, but the tested official download redirect named
`ota-lynx-user-v1-4-1-6942b6a7eb266.zip` and returned object-store 404; tested older firmware links
also returned 404. The portal metadata is vendor evidence, but acquisition is operationally
blocked and stock 1.4.1 artifact inspection cannot currently be performed.

Execute only the exact QDL/QFIL command supplied for a hash-bound official firmware package after
inspecting every rawprogram/patch XML and acquiring the unit's matching calibration profile. It is
a full vendor write, not a discovery command; no generic rawprogram substitution is permitted.

Public partition evidence has paired XBL/ABL/boot/recovery/DTBO/vbmeta, firmware partitions and a
physical `super`; postmarketOS records the current community packaging facts at
**[external]** ellyq/lynx-mainline `dumps/partitions.log`. postmarketOS independently records
fastboot, header-v2 community packaging, DTBO and `super` at
`references/pmaports/device/testing/device-lynx-r1/deviceinfo:18-32`; it does not prove the stock
partition inventory or stock boot header. Stock 1.4.1 boot header and exact protected partition set
remain unresolved.

Pre-write protected backup is also unresolved. The vendor documents restoring the three exact QVR
calibration files and destructive QDL/QFIL writes, not a read-only Firehose backup route.
Obtaining root by flashing both `boot_a` and `boot_b` as in the vendor ORB-SLAM guide has already
performed writes and therefore cannot satisfy a pre-write backup gate.

### Static flow

1. when the vendor artifact is obtainable, download the exact official firmware and independently
   compute SHA-256; the current 1.4.1 404 is a hard stop;
2. inspect rawprogram/patch XML, GPT and every boot/AVB artifact offline;
3. capture the running build, slot, descriptors and calibration paths;
4. verify persistent open state after a clean fastboot restart;
5. back up primary/backup GPT, protected partitions, and the unit's vendor calibration profile
   twice with matching hashes;
6. use temporary boot if supported; otherwise write only a proven inactive-slot boot artifact;
7. read back, switch slot and boot with retry fallback; stock restore uses vendor-documented ADB
   sideload or the exact official QDL/QFIL package.

Steps 4–7 are requirements, not completed Mura evidence. Never select QFIL erase-all or generate a
generic rawprogram from an adjacent SM8250 device.

Future capture adds `fastboot oem device-info`, exact stock USB identities, per-partition
`partition-type:*`/`is-logical:*`, AVB indexes, boot-control flags and rawprogram
protected-state coverage. Separately pull and hash each of
`/mnt/vendor/persist/qvr/device_calibration.xml`,
`/mnt/vendor/persist/qvr/svrapi_lens_left.csv`, and the vendor-spelled
`/mnt/vendor/persist/qvr/svrapi_lens_rigth.csv` twice; acquire the matching vendor calibration
profile needed by the documented restore service. Calibration files never enter an installer
bundle.

**Static verdict:** vendor-documented open bootloader and stock recovery, but the current 1.4.1
artifact is unavailable; exact Mura write plan is hardware-unqualified.

## 9. Oculus Go (`pacific`)

Meta released an official, Go-only software-unlock package in October 2021. The vendor states that
it disables `boot.img` signature enforcement and dm-verity, permits replacement of `boot.img` and
then `system.img`, permanently ends OTA service, and does not apply to another product:
**[external]** Meta, “Unlocking Oculus Go,” October 2021, and “Oculus Go SW Unlock” download
version 1.0, updated 2021-10-18.

Vendor flow:

1. enable developer mode/ADB on stock Go;
2. enter sideload from the boot menu—the official guide says Volume Up from power-off, while owner
   reports say the working chord is Volume Down;
3. extract the downloaded outer `Oculus_Go_SW_Unlock_v1.zip`, then
   `adb sideload unlocked_build.zip` from inside it;
4. reboot into the bootloader;
5. `fastboot oem unlock` and confirm the wipe;
6. verify that `fastboot flash` and `adb root` are accepted.

Unlocking permanently ends the device's OTA-update lifecycle according to the official guide,
even though `fastboot oem lock` exists. Relocking wipes again and only an Oculus-signed
`boot.img` will boot.
Therefore Mura must not offer relock until it can build and enroll a target-supported trust root;
no such Go mechanism is documented.

**UNKNOWN:** official package's durable current URL/hash in a reproducible Mura manifest, exact
partition map and protected state, USB descriptors, tested stock restoration after the unlock,
and whether all retail revisions behave identically.

Future capture uses §2, hashes the official outer and inner ZIP, inventories its updater-script
and images offline, captures before/after lock state over a clean restart, and takes duplicate
per-unit backups before custom writes.

**Static verdict:** vendor-documented unlock; Mura artifact/partition closure remains
hardware-unqualified.

## 10. Quest 1 (`monterey`)

The current WebUSB project supports a headset already running final Quest 1 build
`49845030443200410` and rolls only the inactive boot chain to `16476800119700000`
(v29.0.0.66), not the Android system
(`references/quest1-bootloader-unlocker-web/README.md:1-18`). Its exact procedure, backup and
recovery properties are audited in [71 §6](71-unlock-installer-precedents.md).
It does not supply authenticated final-stock acquisition or recovery after loss of both bootable
slots; final-stock packages still come from community mirrors.

Important target facts:

- the running slot is never written;
- 13 profile-declared boot-chain/kernel/modem partitions are backed up and checked;
- the target slot is activated with retry fallback;
- the ABL exploit and unlock token request occur without a reboot between them;
- only a cleanly rebooted bootloader's state is authoritative;
- rollback indexes are reset only after unlock; fuse floors cannot be reset;
- the original slot is reselected and the downgraded slot restored;
- userdata is erased; Android must boot fully to mark the original slot successful;
- restoration still requires one bootable slot and an ADB shell.

Source: `references/quest1-bootloader-unlocker-web/README.md:38-139,190-216` and
`references/quest1-bootloader-unlocker-web/src/lib/flow.ts:1080-1374`.

The tool warns but allows an unlisted root-exploit build
(`references/quest1-bootloader-unlocker-web/src/lib/device.ts:270-287`). A Mura release plan
cannot: it requires exact exploit/build
evidence. Community firmware mirrors are not authenticated by Meta; the pinned tool's embedded
hashes protect bytes against accidental substitution but not a compromised source bundle
(`references/quest1-bootloader-unlocker-web/README.md:164-188`).

Future capture: exact final build fingerprint, both GPT copies, all LUNs, slot flags before/after,
13-partition sizes/hashes, protected `persist`/`private`/`vision`/NV state, USB Update Mode
descriptors, post-clean-reboot unlock state, rollback command result, both restored slot hashes,
and official/latest stock sideload recovery.

**Static verdict:** source-documented and unusually complete, but still hardware-unqualified by
Mura.

## 11. Quest 2 (`hollywood`)

Quest 2 is not simply “locked.” The original unlocker states that Quest 1/2 can be unlocked when
already on old firmware and identifies Quest 2's latest vulnerable build as
`16476800118700000` (29.0.0.65.370.289987413, 2021-05-09)
(`references/quest-bootloader-unlocker/README.md:1-10`). The current WebUSB repository carries a
separate Quest 2 profile/archive/patch. Its comment says the full newer-build downgrade flow lacks
ionstack (`references/quest1-bootloader-unlocker-web/src/data/profiles.ts:200-214`), while the
same commit's generated manifests pin a Quest 2 ionstack and firmware archive
(`references/quest1-bootloader-unlocker-web/binaries/EXPECTED.sha256:8-9`,
`references/quest1-bootloader-unlocker-web/src/data/asset-hashes.json:7-8`). The deployed assets
matched those hashes when retrieved 2026-09-27. This is an internally stale source comment and a
deployment claim, not an independently reproducible payload or Mura hardware result. The direct
fastboot path for a headset already on the vulnerable bootloader does not need that post-boot
root binary.

The distinction is:

- **exact vulnerable v29 bootloader:** direct exploit unlock is source-documented;
- **newer build `50670960048600150`:** the inactive-slot flow is source-bounded and deployed with
  hash-pinned assets, but remains community/deployment evidence rather than a Mura-qualified path;
- **ordinary current retail firmware with no matching root chain:** no entry path, therefore
  locked.

Do not generalize Quest 1's 13-partition set, by-name path, firmware archive or rollback behavior.
The current source itself treats partition lists and paths as device profiles
(`references/quest1-bootloader-unlocker-web/src/lib/partitions.ts:21-66`).

Future capture mirrors §10 on a dedicated unit and additionally verifies storage/revision variants,
Quest 2-specific profile completeness, root ceiling, exact partition set and stock recovery.

**Static verdict:** build-bounded source path, no current-firmware blanket unlock claim.

## 12. Quest Pro, Quest 3 and Quest 3S

These devices have public temporary-root research, not public persistent retail bootloader
unlock. `fuguquest` is a build-specific post-boot kernel exploit; it does not run ABL or change the
next boot's trust root. Its pinned source identifies `52433670036000520` / security patch
2026-06-03 as the last vulnerable build and `52433670048800520` / 2026-06-04 as the first patched
build (`references/fuguquest/README.md:1-17`), supports build-specific Quest 2/Pro/3/3S targets,
and says the root/SELinux state is RAM-only and fully cleared by reboot
(`references/fuguquest/README.md:40-77`). Those are
acquisition boundaries, not unlock boundaries. FreeXR lists Quest 3/3S as locked
(`references/freexr/README.md:11-16`).

For Quest 3/3S, [research/07](07-device-landscape.md) records a previously public Panther ABL
analysis describing a fused OEM key hash and signed unit-bound unlock tokens. Its source URL
returned 404 on the final 2026-09-27 check, so it is unavailable secondary evidence rather than a
pinned proof. Writing modified XBL/ABL without an accepted signature risks an unrecoverable brick
because no matching public Firehose recovery is known.

Temporary root may support read-only donor capture on an exact vulnerable build. It does not
enable a Mura flash action. Future capture is limited to safe running-OS properties and copied
artifacts until an independently recoverable persistent unlock exists.

**Static verdict:** `unsupported`, `writesEnabled = false`.

## 13. HTC Vive XR Elite (`kyoto`)

A community guide author reports standard `fastboot flashing unlock` while running the displayed
software release `1.0.999.738`; the Android fingerprint, bootloader revision, hardware revision,
SKU and transcript were not captured:
**[external]** XDA, “HTC Vive XR Elite Bootloader Unlock Guide,” created 2026-02-03, edited
2026-03-07. HTC's release note confirms that software release exists, not that every unit on it
unlocks: **[external]**
[VIVE XR Elite 1.0.999.738 release notes](https://www.vive.com/release-notes/vive-xr-elite/vive-xr-elite-software-10999738/).
The reported prerequisites and flow are:

1. enable USB debugging;
2. force Android provisioning so full Android settings are reachable;
3. enable OEM unlocking;
4. `adb reboot-bootloader`, verify fastboot;
5. `fastboot flashing unlock`, select the on-device unlock option;
6. complete the required recovery factory reset;
7. return to fastboot and observe `Device State: Unlocked`.

The same report warns that downgrading beta firmware through Download Mode corrupted both A/B
boot partitions and left only bootloader access. That is a failure report, not a supported
downgrade path.

HTC's official release note records `2.0.999.960` as FOTA 8.0, released 2026-03-11, and an
upgrade to Android 12. The same single community guide author reports that updating to that release
resets bootloader unlock and that attempting a Download Mode downgrade corrupts both A/B boot
partitions. The release identity and Android version are vendor facts; relock and corruption remain
single-author community reports.

No HTC vendor unlock page for XR Elite, exact factory package, partition map, AVB chain,
anti-rollback data, protected calibration map or local full stock restore was found. Other
firmware builds are UNKNOWN.

Future capture: complete stock identity and descriptors, `fastboot flashing
get_unlock_ability`, before/after lock state across clean restart, slot and snapshot state,
partition/GPT maps, AVB data, OTA/factory artifacts, recovery key behavior, and a
vendor-supported restore path. Do not attempt Download downgrade as discovery.

**Static verdict:** community-reported on one displayed software release; exact build identity
unbound; hardware-unqualified.

## 14. Screened devices not promoted to write targets

| Device | Public mechanism | Reason not promoted |
|---|---|---|
| PICO 4 Ultra / Ultra Enterprise `sparrow` | root claims only | Phoenix guide explicitly excludes it; no persistent unlock |
| HTC Vive Focus 3 | vendor boot menu/factory reset | no reproducible retail unlock or local full custom-image path |
| Magic Leap 2 | official fastboot scripts install accepted vendor OS images | signed flashing is not owner-controlled custom-image unlock |
| Magic Leap 1 | historical official images / EOL | current artifact availability and custom trust policy incomplete |
| HoloLens 2 | official FFU recovery | vendor-signed recovery, no owner unlock |
| Apple Vision Pro | Configurator/IPSW restore | vendor signing window, no owner unlock |
| Google Glass Enterprise Edition 2 | official factory images and Android flashing | community OEM-unlock evidence needs exact retail/build closure |
| Lenovo Mirage Solo | mirrored service/EDL reports | weak provenance; no reproducible protected-state-safe flow |
| Pimax Portal / Crystal | vendor update tools and development-unit reports | no retail bootloader-unlock proof |
| Simula One | PC-class NixOS concept | ordinary PC replacement, shipping target status uncertain; not this bootloader problem |

This is a bounded discovery result, not a claim that no private/vendor service path exists.

## 15. Contradiction register

| Claim key | Evidence A | Evidence B | Current verdict | Future decider |
|---|---|---|---|---|
| `pico-phoenix-5.13.8` | earlier warning may concern kernel-root exploit | current tool uses EDL/old ABL and claims ≤5.13.8 | mechanisms do not resolve each other; no independent 5.13.8 unlock log | model/build-specific transcript or hardware capture |
| `pico-enterprise` | tool routes Enterprise to DDR4-labelled programmer | PICO specifies LPDDR5; pinned recovery doc says standard DDR loader must not be used | unsupported for EDL writes | accepted-loader evidence plus Enterprise log and partition comparison |
| `pico-pro-memory` | notes call Pro DDR5/Lite | source proves only Pro→`prog_firehose_lite.elf` | use standard/lite labels | Firehose storage-info + board memory evidence |
| `galaxy-ayke-downgrade` | AYKE reportedly removed unlock | AYIA and AYKE are both `U1` | downgrade floor unknown | packages + Download screen + signed downgrade result |
| `galaxy-u1-u2` | official ledger establishes AYKE `U1`, then AZCI/AZD8/AZF3 `U2` | no SM-I610 `SW REV CHECK FAIL`/Odin transcript | strong explanation for signed U2→U1 rejection; not a claim that all U2 downgrades fail | package comparison plus signed U2→U1 and U2→U2 attempts |
| `galaxy-update-relock` | Samsung-family community warning | no controlled SM-I610 transcript | community-reported | already-unlocked AYIA specimen |
| `pfdm-retail-unlock` | one unburnt-eFuse unit photo | no retail matrix/recovery package | one-unit report only | retail captures across SKU/build |
| `frame-open` | root-capable stock Linux | lower boot-chain policy unknown | admin access ≠ unlock | secure-boot/U-Boot/external-boot capture |
| `quest2-locked` | current retail firmware has no entry | old v29 and profile-specific WebUSB source exist | build-bounded, not universally locked/unlocked | exact build/profile matrix |
| `vive-xr-elite` | one guide tied only to displayed release 1.0.999.738 | no vendor procedure/factory package | community-reported | second reproduction + stock restore |

## 16. External sources

All retrieved 2026-09-27 unless dated above:

- Samsung: [official SM-I610 build ledger](https://doc.samsungmobile.com/SM-I610/036371251209/eng.html);
  [launch unlock report](https://www.androidauthority.com/samsung-galaxy-xr-bootloader-unlocking-3609841/);
  [launch corroboration](https://www.sammobile.com/news/galaxy-xr-bootloader-unlock-is-possible-at-least-for-now/);
  [AYIA launch-era community/build report](https://www.reddit.com/r/GalaxyXR/comments/1oyc7v5/no_software_updates_since_july_5th/);
  [December 2025 update report](https://sammyguru.com/galaxy-xr-gets-first-update-with-new-travel-mode-feature/);
  [April 2026 platform update](https://blog.google/products-and-platforms/platforms/android/android-xr-immersive-features-update-april-2026/);
  [AZD8 May community/build announcement](https://www.reddit.com/r/Galaxy_XR/comments/1t5fdaj/update_patch_version_i610ueu2azd8_is_rolling_out/);
  [July 2026 build report](https://www.sammobile.com/news/galaxy-xr-gets-a-mysterious-1-5gb-update/).
- Play For Dream:
  [developer build guide](https://developer.yvrdream.com/yvrdoc/unity/UserManual/GetStartedXR/BuildXRScene.html);
  [support center](https://pfdm.ai/pages/play-for-dream-mr-support-service-center);
  [FastBoot image/API](https://mastodon.social/api/v1/statuses/114027425822225840);
  community reports [A/B, failed `fastboot boot`, delta OTA](https://mastodon.social/@ShinyQuagsire/114102888472792684),
  [`misc` recovery loop](https://mastodon.social/@ShinyQuagsire/114103602501189178),
  [custom SXR2250 EDL](https://mastodon.social/@ShinyQuagsire/114119219012750767),
  [QTI-verified XBL-SC](https://mastodon.social/@ShinyQuagsire/114136842701377922), and
  [Fastboot-regaining reboot reason](https://mastodon.social/@ShinyQuagsire/114146942883232103);
  [SXR2250 EDL commit](https://github.com/shinyquagsire23/sxr2250_edl_prog/commit/418bfef3c45caf578248191915074acd35cdd9f6);
  [QWR ADB-enabler commit](https://github.com/QWR-Interactive-Solutions-Pvt-Ltd/xrone-adb-enabler/commit/1f1d074079abb43bcc22418eec5eacb82fa57a2a).
- PICO: [rooting guide](https://pico4.wiki/guides/root/01-root/);
  [PICO 4 Enterprise specifications](https://www.picoxr.com/sg/products/pico4e/specs).
- Valve: [Steam Frame debugging](https://partner.steamgames.com/doc/steamhardware/steamframe/debugging).
- Lynx: [Getting Started v30](https://portal.lynx-r.com/documentation/view/getting-started?version=30);
  [update/restore v15](https://portal.lynx-r.com/documentation/view/updating-your-device?version=15);
  [firmware downloads](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/);
  [community partition dump](https://raw.githubusercontent.com/ellyq/lynx-mainline/main/dumps/partitions.log).
- Meta: [Oculus Go unlock package](https://developers.meta.com/horizon/downloads/package/oculus-go-sw-unlock/);
  [official Go article](https://developers.meta.com/horizon/blog/unlocking-oculus-go/);
  [Quest software update tool](https://www.meta.com/help/quest/software_update/).
- HTC: [Vive XR Elite community guide](https://xdaforums.com/t/htc-vive-xr-elite-bootloader-unlock-guide.4777618/);
  [VIVE XR Elite 1.0.999.738 release notes](https://www.vive.com/release-notes/vive-xr-elite/vive-xr-elite-software-10999738/);
  [VIVE XR Elite 2.0.999.960 release notes](https://www.vive.com/release-notes/vive-xr-elite/vive-xr-elite-software-20999960/).
- Normative Android background:
  [bootloader locking/unlocking](https://source.android.com/docs/core/architecture/bootloader/locking_unlocking),
  [fastbootd](https://source.android.com/docs/core/architecture/bootloader/fastbootd), and
  [AVB](https://source.android.com/docs/security/features/verifiedboot/avb).

