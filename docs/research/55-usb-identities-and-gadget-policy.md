# 55 — USB identities, gadget compositions, and target modes

**Research date:** 2026-09-25.
**Question:** which USB identities do Mura, its comparable distributions, and the six target
headsets use; why were those identities chosen; what evidence can be recovered without hardware;
and which identity strategies remain available to Mura?
**Method:** pinned-source archaeology, exact donor/driver/firmware inspection, vendor documentation,
community descriptor reports, and host class-driver/specification review. No target was attached.

This is evidence and analysis, not the shipping VID/PID decision. Reusing Linux Foundation,
Google, OEM, or unallocated community values is established practice in comparable projects; USB
assignment/certification material describes a different governance posture. The user decides which
trade-off Mura accepts.

## 1. Evidence and mode schema

Evidence labels:

- **VERIFIED** — exact build artifact, pinned source, hash-bound donor, or complete descriptor
  capture.
- **VENDOR-DOCUMENTED** — target mode/function documented, descriptors not captured.
- **COMMUNITY-REPORTED** — target host observation without exact build/artifact closure.
- **ARTIFACT-CONFIGURED** — exact firmware configures the identity, but no target enumeration was
  captured.
- **CANDIDATE** — adjacent/family/driver evidence only.
- **CONFLICTING** — credible evidence disagrees or mode/build is unresolved.
- **UNKNOWN** — no defensible evidence.

Every record is keyed by `(device, exact build, USB mode)`. Modes are never collapsed:

1. charge/no-data;
2. normal MTP/PTP and ADB composites;
3. USB networking (NCM/RNDIS/ECM);
4. Android Open Accessory/audio/MIDI or proprietary wired streaming;
5. recovery/sideload and userspace fastbootd;
6. bootloader/ABL/Download/U-Boot fastboot;
7. Qualcomm EDL/Sahara/Firehose.

Record VID:PID, full configuration/functions, IAD and interface class/subclass/protocol, strings,
serial/MAC source, mode condition, speed, artifact hash/path and host driver. Per-unit descriptor
values cannot be recovered from generic firmware.

## 2. Current Mura gadget

Mura implements one initrd configfs gadget for out-of-band NCM→IP→SSH/web access
(`modules/os/oob.nix:82-144`):

| Field | Current value |
|---|---|
| gadget/function | `mura`, one config `c.1`, `ncm.usb0` |
| VID:PID | `1d6b:0104` |
| strings | manufacturer `Mura`, product `<codename>`, serial `mura-<codename>` |
| configuration | `USB Ethernet (NCM)`, MaxPower 250 |
| UDC | first entry in `/sys/class/udc` |
| network | device `172.16.42.1/24`, DHCP `.2–.20`; NM-unmanaged |

`1d6b:0104` is the Linux Foundation legacy multifunction-gadget identity associated with Linux
`g_multi`; it is **not** postmarketOS's default and does not describe Mura's NCM-only composition.
It remains Mura's implemented development/example identity while the shipping choice is open.

Three descriptor gaps are already visible statically:

- `usb_f_ncm` emits an IAD/control/data function, but Mura does not set device-level
  `bDeviceClass/bDeviceSubClass/bDeviceProtocol = ef/02/01`;
- the model-wide serial collides when two identical headsets are attached;
- configfs chooses random NCM `host_addr`/`dev_addr` each boot.

The VM proves Linux `cdc_ncm`, DHCP, SSH and HTTP behavior through `dummy_hcd`
(`tests/vm/oob.nix`). It does not prove a real UDC/Type-C controller, non-Linux host, suspend,
per-unit identity, or simultaneous-device routing.

Planned but separate compositions:

- Windows compatibility investigation: corrected NCM plus optional `WINNCM`; RNDIS only if measured
  coverage requires it;
- per-unit serial/MAC derivation after persisted identity is available in initrd;
- FunctionFS bulk transport for waypipe only after NCM latency measurement.

## 3. Why the comparable projects use their values

### postmarketOS

Pinned pmaports globally defaults to Google `18d1:d001`, labels it “Google Inc.” / “Nexus 4
(fastboot),” and creates `ncm.usb0`; if creating NCM fails locally it creates `rndis.usb0`
(`references/pmaports/main/postmarketos-initramfs/init_functions.sh:795-873`). The fallback tests
gadget-kernel function availability, not the attached host OS.

At pinned commit `72704b7`, 52 of 667 deviceinfo files override VID and 51 override PID. Comments
commonly mirror stock Google/Xiaomi/Samsung/PineTab identities so existing host recognition/rules
may match; they do not prove every override was necessary. pmOS's use case is developer/recovery
networking on Android-derived devices.

### Mobile NixOS

There is no global VID/PID default (`references/mobile-nixos/modules/initrd-usb.nix:34-44`).
The SDM845 family uses `mkDefault 18d1:d001` as a “compatible well-known identifier”
(`devices/families/sdm845-mainline/default.nix:55-58`). Other devices use `18d1:d002` for
well-known default udev rules.

Motorola Potter deliberately uses `18d1:4ee7` rather than `d001` to distinguish NixOS runtime
from fastboot/lk2nd (`devices/motorola-potter/default.nix:75-82`). PinePhone Pro says Pine64 lacks
a VID and selects `1209:0069` from pid.codes' generic testing/CDC range
(`devices/pine64-pinephonepro/default.nix:36-46`); no current registry entry proves that PID was
formally allocated.

### Transfer to Mura

The comparables establish a shape—stable runtime identity, per-device override, and composition/
boot-mode collision avoidance—not a forced number. Mura's NCM uses class matching; SSH and HTTP
need no ADB udev/Windows INF match. Borrowed Android IDs may improve recognition in developer
environments but can misidentify the composition or collide with boot/recovery. These are trade-
offs, not prohibitions.

## 4. Six-target identity matrix

| Target/mode | Identity | Evidence |
|---|---:|---|
| Quest 1 v50 MTP | `2833:0082` | ARTIFACT-CONFIGURED |
| Quest 1 v50 ADB | `2833:0086` | ARTIFACT-CONFIGURED + Meta INF |
| Quest 1 v50 MTP+ADB | `2833:0083` | ARTIFACT-CONFIGURED |
| Quest 1 v50 XRSP / XRSP+ADB / MTP+XRSP+ADB | `2833:0137` / `0186` / `0183` | exact HAL; runtime 0183/0186 build-unspecified |
| Quest 1 v50 recovery / fastbootd / ABL | `2833:0086` / `0081` / `0081` | exact firmware/ABL |
| Lynx stock normal/recovery/fastboot | unknown | functions documented, descriptors absent |
| Lynx EDL | generic `05c6:9008` | vendor says QDLoader 9008; no target descriptor capture |
| Lynx pmOS networking | `18d1:d001` | exact pmOS static composition, not stock Lynx |
| Galaxy XR MTP/ADB | unknown | target functions confirmed; no SM-I610 descriptor |
| PFDM developer composite | `34e2:4f07`, ADB `MI_01` | driver artifact + community corroboration |
| Steam Frame Developer Mode | `28de:2460` | donor-configured FunctionFS ADB + conditional NCM |
| Quest 3 normal | `2833:0186` | target runtime observation |
| Quest 3 v77 NCM / NCM+ADB | `2833:5009` / `500a` | ARTIFACT-CONFIGURED |
| Quest 3 v77 recovery / fastbootd | `2833:0086` / `0081` | ARTIFACT-CONFIGURED |
| Quest 3 ABL | `2833:0081` | CANDIDATE, not Eureka-captured |
| Quest 3 EDL | `05c6:9008` | COMMUNITY-REPORTED |

### 4.1 Oculus Quest 1 (`monterey`)

Exact final build `49845030443200410` (OTA SHA-256
`7c1f75ecd807b59d5215a67ff69e8e8c6da4b966eb08464d48e00d2d5a8b6aaa`)
maps compositions as follows:

| Mode | VID:PID | Function/interface |
|---|---:|---|
| no-data | none | gadget torn down |
| charger boot | `2833:0083` | mass storage; PID reused |
| MTP / ADB / MTP+ADB | `2833:0082` / `0086` / `0083` | ADB `ff/42/01` |
| PTP / PTP+ADB | `2833:0089` / `0090` | PTP `06/01/01` |
| XRSP / XRSP+ADB | `2833:0137` / `0186` | exact v50 XRSP `ff/89/01` |
| MTP+XRSP / +ADB | `2833:0182` / `0183` | composition-specific interfaces |
| recovery ADB / fastbootd | `2833:0086` / `0081` | `ff/42/01` / `ff/42/03` |
| ABL USB Update Mode | `2833:0081` | one fastboot `ff/42/03` interface |

This resolves the historical 0183/0186 conflict: they are different active compositions, not
different headset models. Android's mandated AOA/audio-source IDs use Google `18d1:2d00–2d05`;
MIDI uses `18d1:4ee8`. V29 recovery instead used AOSP `18d1:d001`/`4ee0`. Network gadget PIDs and
Quest-specific EDL identity remain unknown. Exact sources are the final HAL/ramdisk and hash-bound
ABL described in [51 §Quest 1](51-android-boot-donor-extraction-audit.md); external archive/driver
artifacts are local-only.

### 4.2 Lynx R1

Vendor docs prove MTP, ADB, sideload, userspace fastboot and QDL. The modified 1.2 dump proves
ADB-only configfs state and manufacturer/model properties but its attempted `18d1:4ee7` legacy
writes failed; it is not a host enumeration. Stock normal/recovery/fastboot identities remain
unknown. Vendor “QDLoader 9008” aligns with generic Qualcomm `05c6:9008` but lacks target capture.

The postmarketOS profile has no overrides, so its own initramfs—not stock Lynx—statically uses
`18d1:d001`, strings `Lynx` / `Lynx R1` / `postmarketOS`, and NCM with local RNDIS creation
fallback. Official 1.4.1 archive acquisition is currently broken/404; recover/hash it before
extracting descriptors.

### 4.3 Samsung Galaxy XR (`SM-I610`)

Samsung documents MTP and live-headset ADB; no build-tagged SM-I610 VID/PID/interface capture
exists. Generic Samsung `04e8:*`, Download/Odin and Qualcomm EDL values are candidates only.
Host-mode hubs/storage/Ethernet/audio enumerate the accessory, not Galaxy gadget identity.

Acquire exact FUS launch/current packages using the unit's real CSC, both SM-I610 source releases
and Samsung's signed generic driver; inspect AP/vendor/recovery/BL/PIT artifacts. Capture normal
locked/unlocked, file transfer, ADB, recovery/sideload, fastbootd if present, Download/Odin, and
only independently reached EDL. Do not use generic button/EDL procedures merely to obtain IDs.

### 4.4 Play For Dream MR

The official `PFDM_ADB_Driver.rar` maps `USB\VID_34E2&PID_4F07&MI_01` to WinUSB “YVR Composite
ADB Interface” (archive SHA-256
`eb3516b090a57782acc73e11e191315635bea5d38065119812cb965d957d3139`).
This binds ADB interface 1 but not MI_00/MTP/strings/endpoints. Official wired PCVR is ALVR-derived
TCP/UDP tunneled over ADB; it is not proof of a separate USB class or UAC mode.

Fastboot UI is community-observed on one unlocked unit, but generic INF candidates are not target
IDs. Recovery, fastboot PID, EDL, network gadget and exact DreamOS build remain unknown.

### 4.5 Valve Steam Frame

Hash-bound donor configuration sets `28de:2460`, manufacturer `Valve`, product/configuration
`Steam Frame`, serial from hostname, one configfs configuration, FunctionFS ADB and conditional
CDC-NCM. Normal mode conditions produce no gadget functions; Developer Mode enables ADB and NCM.
This is **VERIFIED static donor configuration**; live full descriptors still need capture.

NCM MACs are stable per unit/interface, derived from EEPROM Wi-Fi MAC—not random like Mura's
current gadget. Valve's USB subnet is also interface-indexed. U-Boot contains Fastboot/EDL/recovery
menu paths but no verified fastboot VID/PID; DFU is not evidenced. Arcturus uses the front camera
expansion path and is not known to enumerate as USB.

### 4.6 Meta Quest 3 (`eureka`)

Runtime evidence supports normal `2833:0186`; exact v77 firmware identifies Oculus/Quest 3/
`${ro.serialno}` and configures:

- MTP-only / MTP+ADB `2833:0082` / `0083`;
- NCM-only / NCM+ADB `2833:5009` / `500a`, with `WINNCM`;
- recovery ADB `2833:0086`;
- fastbootd `2833:0081`.

These are build-bound configuration facts until captured. ABL `0081` remains a candidate because
no Eureka ABL descriptor was obtained. Community EDL `05c6:9008` does not imply recoverability.
Panther/Quest 3S evidence is excluded.

## 5. Host class binding and descriptor correctness

Linux `usb_f_ncm` emits NCM IAD/control/data descriptors; Linux `cdc_ncm` class-matches
`02/0d/00`, not a Mura VID/PID. Because an IAD is used, USB-IF/Microsoft guidance calls for
device-level `ef/02/01`; Mura currently leaves `00/00/00`.

- **Linux:** class binding is expected and VM-proven; real UDC/Type-C/suspend remains untested.
- **macOS:** inbox NCM exists; recent interoperability and stable network-service behavior need
  physical testing.
- **Android hosts:** kernel driver presence does not prove OEM module loading, host role or
  Ethernet framework acceptance. Never claim universal support.
- **Windows 11:** Microsoft documents inbox `UsbNcm.sys` for NCM.
- **Windows 10:** Microsoft documentation and field behavior conflict. Treat plug-and-play as
  conditional; test `WINNCM` OS descriptors on every supported build.

NCM-only remains the simplest primary composition. Simultaneous NCM+RNDIS creates two NICs and
needs explicit MAC/IP/DHCP policy; pmOS's NCM→RNDIS creation fallback is not host detection.
If broad legacy Windows support requires RNDIS, a separately selectable descriptor-stable
composition/PID is cleaner than two unmanaged interfaces.

## 6. Identity, collision, and multi-device facts

VID/PID identifies a product/composition, not a unit. Mura also needs:

- stable unique, privacy-preserving per-unit serial;
- stable distinct locally administered host/device MACs;
- descriptor stability under one PID;
- collision-safe simultaneous-device subnets/routes and per-unit mDNS suffixes;
- explicit UDC identity per target rather than first-entry selection.

Current `mura-<codename>` serial duplicates units; random configfs MACs create new host network
profiles after reboot; fixed `172.16.42.0/24` is ambiguous with two attached Muras.

## 7. Identity strategies — discretionary scorecard

| Strategy | Precedent/benefit | Costs/risks | Evidence verdict |
|---|---|---|---|
| keep `1d6b:0104` | Linux gadget example familiarity; zero process | Linux Foundation identity, g_multi composition mismatch, collisions/caching | established development practice; shipping posture discretionary |
| borrow Google/OEM ID | pmOS/Mobile NixOS precedent; possible existing raw-USB rules | misidentification, vendor INF/quirk binding, boot/runtime collisions, no project-owned namespace | technically demonstrated; rationale only partly applies to class NCM |
| request pid.codes PID | unique open-project namespace, low process/cost | not USB-IF endorsed; allocation/range rules and project eligibility | viable only after accepted allocation; never self-select |
| OEM-authorized PID | exact hardware relationship may aid vendor hosts | per-OEM agreements, USB-IF/OEM scope, fragmented identity across targets | investigate authorization in writing |
| Mura USB-IF VID | strongest namespace/control/certification path | fee/administration/testing; logo is separate | technically clean; cost/posture are user decisions |

Separate PIDs are appropriate when the descriptor/function layout materially changes (for example
NCM-only versus RNDIS-only or FunctionFS composite). A service toggle with identical descriptors
does not need another PID. Donor bootloader/recovery modes keep their own vendor identities.

USB-IF materials describe exclusive assignment and certification practice; comparable projects
show de-facto reuse. This research does not issue a legal conclusion and does not rule out a
strategy. The user decides the acceptable governance/collision/certification posture.

## 8. Runtime qualification matrix

Capture `lsusb -v`/USBView plus build/mode trigger and raw descriptors for every mode. Test:

- Linux current + LTS; macOS current/recent; Android 12/14/15+ and OEM devices; Windows 10 22H2,
  Windows 11 current, optionally Server 2022;
- current NCM, corrected `ef/02/01`+stable identity, corrected+`WINNCM`; RNDIS-only only if needed;
- clean driver binding, DHCP, bidirectional ping, SSH/HTTP, throughput/boundary traffic;
- cable-at-boot, reconnect/port changes, host and headset suspend/resume, Type-C role/speed;
- two Muras simultaneously: separate containers, serials, MACs, leases/routes/subnets/mDNS;
- descriptor upgrade/cache behavior on clean and previously paired Windows hosts.

Per target capture normal/developer/file-transfer/network/recovery/fastboot/Download/EDL modes
without writes. Redact serials, MACs and identity/calibration before sharing.

## 9. Contradictions and deciders

- Mura/pmOS `1d6b:0104` attribution: false; pmOS default is `18d1:d001`.
- Quest 1 `0183` versus `0186`: composition difference, not model.
- Quest 3 exact v77 configured modes versus current runtime: capture current build.
- Lynx `18d1:4ee7` failed legacy writes: not enumeration.
- PFDM `4f07`: ADB MI_01 proven, remaining composition unknown.
- Frame `28de:2460`: donor configuration verified; live descriptors/bootloader IDs pending.
- Windows 10 NCM support: conflicting Microsoft/field evidence; `WINNCM` test decides.
- pid.codes `1209:0069`: comparable rationale verified, formal allocation not verified.
- first UDC and fixed subnet: per-target enumeration and simultaneous-device lab decide.

## 10. Decision handoff

Before changing the configured pair or adding contract fields, the user needs:

1. chosen identity strategy and governance posture;
2. supported host/version matrix;
3. descriptor compositions that merit distinct PIDs;
4. per-unit serial/MAC source and privacy rule;
5. simultaneous-device addressing/discovery design;
6. whether Windows 10 merits `WINNCM` and/or RNDIS;
7. certification/logo goal.

Only after that decision should a separate implementation change add identity options, corrected
device descriptors, stable serial/MAC derivation, explicit UDC selection and matching tests.

## 11. External source index

Target/build artifacts:

- Quest 1 firmware archive and build hashes: **[external]**
  [Quest firmware archive](https://cocaine.trade/Quest_firmware); exact ABL fastboot corroboration:
  [Quest 1 unlocker](https://github.com/darknight1050/quest1-bootloader-unlocker-web/blob/7f34276ecef9021771e7bf4a2d53f10b26243334/src/lib/fastboot.ts).
- Lynx stock modes/recovery: **[external]**
  [firmware portal](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/) and
  [update/QDL guide](https://portal.lynx-r.com/documentation/view/updating-your-device).
- Galaxy XR target MTP behavior: **[external]**
  [Samsung backup guide](https://www.samsung.com/us/support/answer/ANS10007552/); generic signed
  host driver is a candidate artifact, not SM-I610 identity evidence.
- PFDM ADB identity corroboration: **[external]**
  [xrOne ADB Enabler](https://github.com/QWR-Interactive-Solutions-Pvt-Ltd/xrone-adb-enabler/commit/1f1d074079abb43bcc22418eec5eacb82fa57a2a);
  vendor packages originate at the
  [PFDM support center](https://pfdm.ai/pages/play-for-dream-mr-support-service-center).
- Steam Frame Developer Mode/ADB: **[external]**
  [setup](https://partner.steamgames.com/doc/steamhardware/steamframe/setup) and
  [debugging](https://partner.steamgames.com/doc/steamhardware/steamframe/debugging); descriptor
  composition itself is from the hash-bound local donor.
- Quest 3 normal runtime `2833:0186`: **[external]**
  [SteamVR issue 719](https://github.com/ValveSoftware/SteamVR-for-Linux/issues/719); exact v77
  configured modes: [Eureka dump commit](https://dumps.tadiphone.dev/dumps/oculus/eureka/-/commit/a784742539a3d06aa2b113b8fd95ad68edc63d1d);
  NCM community mechanism:
  [Wired Steam Link VR](https://github.com/UbootVRC/Wired-Steam-Link-VR/commit/1362a1f76ae641ae53df7f271dbd1039a2c58281).

Specifications/governance/host behavior:

- **[external]** [USB NCM 1.1](https://www.usb.org/sites/default/files/NCM11.pdf) and
  [IAD class-code guidance](https://usb.org/sites/default/files/iadclasscode_r10.pdf).
- **[external]** Linux [`usb_f_ncm`](https://github.com/torvalds/linux/blob/master/drivers/usb/gadget/function/f_ncm.c)
  and [`cdc_ncm`](https://github.com/torvalds/linux/blob/master/drivers/net/usb/cdc_ncm.c).
- **[external]** Microsoft
  [supported USB classes](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/supported-usb-classes),
  [IAD behavior](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/usb-interface-association-descriptor),
  and [Windows composite enumeration](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/enumeration-of-the-composite-parent-device).
- **[external]** pid.codes [allocation procedure](https://pid.codes/howto/),
  [VID 1209 rules](https://pid.codes/1209/), and [FAQ](https://pid.codes/faq/).
- **[external]** USB-IF [developer policy](https://usb.org/developers) and
  [VID information](https://www.usb.org/getting-vendor-id). These describe governance practice;
  this audit does not provide legal advice or adjudicate the project's risk posture.
