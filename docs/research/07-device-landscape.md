# Device landscape for Mura

**Research date:** 2026-09-22

The six targets divide into three practical classes. Oculus Quest 1 and Lynx R1 are the best near-term Android-boot-image targets because their bootloaders are now usable and their MSM8998/SM8250-family SoCs have substantial upstream Linux support; Lynx already reaches a postmarketOS debug shell. Samsung Galaxy XR and Play For Dream MR expose much newer XR2+ Gen 2 hardware, but their exact board support, vendor kernels, firmware layouts, and long-term unlock behavior are less public. Steam Frame is structurally different and unusually attractive: it already runs aarch64 SteamOS/Linux, publishes RAUC/casync update artifacts, and exposes SSH, while Quest 3 remains an aspirational locked target where temporary root is not equivalent to a bootloader unlock.

## Comparison

| Device | SoC | Stock OS | Unlock status (2026-09-22) | Kernel source | Mainline status | Firmware source |
|---|---|---|---|---|---|---|
| Oculus Quest 1 (`monterey`) | Snapdragon 835 / MSM8998 | Meta/Oculus Android; final release is Android 10-based | Community unlock works on final build `49845030443200410` using a vulnerable v29 ABL in the inactive slot | Meta's archived [Oculus kernel tree](https://github.com/facebookincubator/oculus-linux-kernel), Linux 4.4-era | MSM8998 is substantially upstream; no public Monterey DTS/complete port found | Unofficial [Quest firmware archive](https://cocaine.trade/Quest_firmware); no official historical archive |
| Lynx R1 | Snapdragon XR2 Gen 1; SM8250-family, downstream board platform `kona` | Lynx AOSP 12 | Vendor-documented open bootloader; fastboot and authenticated Firehose recovery available | **Unverified:** no public Lynx-R1 vendor kernel tree found; do not confuse Lynx's GPL ORB-SLAM release with kernel source | Active; postmarketOS/mainline boots to a debug shell | Official [Lynx firmware portal](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/) |
| Samsung Galaxy XR (`SM-I610`, board reported as `anorak`) | Snapdragon XR2+ Gen 2; public Qualcomm part-number mapping is unverified | Android XR / “XR One UI,” Android 14-based | Launch firmware unlockable; Dec. 2025 update reportedly removes unlock; Apr. 2026 update reportedly adds rollback barrier | Official model-specific release through [Samsung Open Source](https://opensource.samsung.com/uploadSearch?searchValue=SM-I610) | Generic SM8550/QCS8550 support is strong, but the XR2+ mapping and Galaxy XR board port are unverified; community port is reported in progress | Samsung FUS via Frija/SamFirm ecosystem; Samsung support site does not publish full images |
| Play For Dream MR (`anorak`) | Snapdragon XR2+ Gen 2; exact silicon ID unverified | DreamOS, Android 14-based | FreeXR reports an unlocked unit with vendor-left-unburnt eFuse; scope and update durability are unknown | No public vendor GPL kernel release located | No public device port; only adjacent SM8550-family upstream work | OTA only; official downloads page exposes manuals, not firmware |
| Valve Steam Frame (`deckard`) | Snapdragon 8 Gen 3 / SM8650 | SteamOS, Arch-derived aarch64 Linux; **not Android** | Developer Mode exposes SSH/RDP and root-capable stock Linux; alternate-OS/secure-boot policy is not yet documented | **Kernel identity donor-verified** ([33 §4](33-steam-frame-donor.md)): 6.18 LTS, pkgbase `linux-618-deckard`, config extracted via IKCONFIG, production DTBs in-image; binary packages public, source tarball still unlocated (GPL acquisition task) | SM8650 has broad upstream support; Valve uses upstream Mesa Turnip | Official public [SteamOS VR image index](https://holo-images.steamos.cloud/vr/) with `.raucb` and `.castr` — **reconstructed, audited, and mirrored by a VM-boot-proven Mura image** ([33 §9](33-steam-frame-donor.md)) |
| Meta Quest 3 (`eureka`) | Snapdragon XR2 Gen 2, package marking `SXR2230P`; SM8550-derived | Meta Horizon OS; Android 12.1L at launch, Android 14 currently | Locked; temporary root exists on selected firmware, but no public bootloader unlock | No verified Quest 3 GPL source drop found; older Quest source repository does not contain Eureka | SM8550 is well-supported upstream; no public Eureka DTS/bootable mainline port | Official latest-only [Meta update tool](https://www.meta.com/help/quest/software_update/); unofficial [historical archive](https://cocaine.trade/Quest_3_firmware) |

Status terms above are deliberately narrow: “root” means control after the vendor kernel has booted, while “unlocked” means the boot chain can accept a non-vendor OS image.

## Oculus Quest 1 (`monterey`)

### 1. SoC and key hardware

- Qualcomm Snapdragon 835, platform ID MSM8998, with Adreno 540 GPU and 4 GB RAM. Meta's own engineering overview confirms the [Snapdragon 835, Adreno 540, and 4 GB memory](https://developers.meta.com/horizon/blog/down-the-rabbit-hole-w-oculus-quest-the-hardware-software/).
- Two 1440 × 1600 PenTile OLED panels, one per eye, at 72 Hz. Meta describes them as [two 1600 × 1440 OLED screens](https://developers.meta.com/horizon/blog/down-the-rabbit-hole-w-oculus-quest-the-hardware-software/); the reversed dimensions are only an orientation convention.
- Four wide-angle monochrome fisheye cameras provide inside-out headset and controller tracking. Camera pose is fused with a headset IMU (accelerometer and gyroscope); Meta's tracking description covers the [four cameras and inertial fusion](https://www.aiacceleratorinstitute.com/the-oculus-insight-positional-tracking-system-2/).
- Storage SKUs are 64 GB and 128 GB UFS. Wi-Fi is 802.11ac/Wi-Fi 5, and controllers communicate through the headset's radio stack.

### 2. Stock OS

- The stock system is Oculus/Meta's Android-derived VR OS, not a conventional Android UI. It launched on Android 7.1.1 and the final v50 release is Android 10-based; archived build fingerprints show `oculus/vr_monterey/monterey:10/...`.
- Final system build: `49845030443200410`, version `50.0.0.198.257.455910822`. QuestStack identifies this as the [latest and required build](https://github.com/starseed12345/QuestStack).
- Vendor kernel: Linux `4.4.21` is reported by early device research and the published source lineage. The old XDA investigation records [Quest's 4.4.21 kernel and fastboot/EDL modes](https://xdaforums.com/t/mods-customization-snapdragon-835.3932649/).
- Quest 1 is end-of-life; v50 was its final feature release and Meta ended security fixes in August 2024. This makes the firmware stable for reproducible work, but unsafe as an Internet-facing stock system.

### 3. Kernel situation

- Meta published the GPLv2-derived [Oculus Linux kernel repository](https://github.com/facebookincubator/oculus-linux-kernel). The Quest commit includes `monterey_defconfig`, Quest device trees, Qualcomm camera/display/audio changes, and is tagged to build [`333700.3780.0`](https://github.com/facebookincubator/oculus-linux-kernel/commit/589280fc40ddbcc2287024c8b672568a0fdd68e7).
- The tree is only a kernel source drop, not a complete tracking stack or reproducible OS BSP. A Meta maintainer explicitly said the repository [contains only the kernel](https://github.com/facebookincubator/oculus-linux-kernel/issues/6).
- MSM8998 has substantial upstream Linux support: clocks, pinctrl, UART, SPMI, UFS, PCIe, GPU, and other blocks are tracked by [linux-msm's MSM8998 status](https://linux-msm.github.io/mainline-status/soc/msm8998). postmarketOS still packages a [dedicated MSM8998 mainline fork](https://pkgs.postmarketos.org/package/v25.12/postmarketos/aarch64/linux-postmarketos-qcom-msm8998).
- No public, bootable Monterey mainline DTS or postmarketOS device package was found. The SoC baseline is mature, but panel, camera topology, IMU, audio, power, and headset-specific reserved-memory work remain.
- FreeXR lists Monterey as active research in its [target overview](https://github.com/FreeXR/FreeXR/tree/init), and the new community unlock removes the most important prerequisite for mainline bring-up.

### 4. Boot chain

- The Qualcomm chain is PBL → XBL → ABL/UEFI `LinuxLoader` → AVB-verified Android boot image. `xbl_a/b`, `abl_a/b`, `boot_a/b`, `modem_a/b`, and `bluetooth_a/b` are present in the published [Quest partition research](https://github.com/QuestEscape/research).
- “USB Update Mode” is Qualcomm fastboot exposed by ABL. EDL also exists, but there is no supported public unbrick path for a retail unit with damaged XBL/ABL.
- The device is A/B. The WebUSB unlocker explicitly reads the active slot, backs up 13 inactive-slot partitions, writes the downgrade images, and returns to the original slot; its [procedure and failure conditions](https://github.com/darknight1050/quest1-bootloader-unlocker-web) are the best current boot-layout documentation.
- Boot image header version is **unverified from a current image**. Because Quest launched before Android 9, a legacy v0 Android boot image is plausible, but Mura must inspect the actual selected firmware with `unpack_bootimg` rather than encode that assumption.
- `vendor_boot` was introduced with Android 11 and is not expected on this Android 10 device; it is absent from the early published partition map. Dynamic partitions are also **not verified** for the final firmware.
- Unlock method: start from final build `49845030443200410`, obtain temporary root, put Quest 1 build `16476800119700000` (v29.0.0.66, 2021-05-10) into the inactive slot, exploit CVE-2021-1931 in its ABL fastboot implementation, request an unlock token, clear rollback indexes, and restore the original slot. The full sequence is documented by the [WebUSB implementation](https://github.com/darknight1050/quest1-bootloader-unlocker-web).
- Unlocking wipes userdata. The unlock is persistent across clean reboots, but no source guarantees behavior across every manually flashed firmware. There are no newer official Quest 1 releases to test.

### 5. Stock firmware sources and version policy

- Meta does not provide a public historical full-firmware directory. Its official web recovery tool supplies only the current supported image and no longer meaningfully serves this end-of-life model.
- The community [Quest firmware archive](https://cocaine.trade/Quest_firmware) lists builds, fingerprints, dates, and SHA-256 hashes. QuestStack links the final full ZIP from its [supported-build instructions](https://github.com/starseed12345/QuestStack).
- An older mirror, [QuestEscape/updates](https://github.com/QuestEscape/updates), contains factory and early full OTAs. Some historical packages are incremental, so the manifest and hash must be checked before use.
- Pin two artifacts: final `49845030443200410` as the known root/unlock entry point and vulnerable `16476800119700000` only as the inactive-slot unlock payload.
- Do not substitute a Quest 2 image or a neighboring Quest 1 build. The patched ABL payload and 13-partition set are Monterey/build-specific.
- These mirrors redistribute Meta binaries without an explicit redistribution grant. The build system should download from user-configured URLs, verify hashes, and never vendor the ZIPs into Mura.

### 6. Donor suitability

- High-value donor artifacts are the downstream DTB/DTBO, `modem_a/b`, `bluetooth_a/b`, Adreno firmware, ADSP/CDSP firmware, camera and sensor firmware/HAL metadata, and board-specific regulator/reserved-memory descriptions.
- Back up `persist`, `private`, and `vision` before any destructive flash. The Quest research partition table identifies all three; the ABL command named `oem read-persist` actually reads [`private`, not `persist`](https://github.com/QuestEscape/research).
- Calibration location and format are not fully documented. Treat `persist`, `private`, `vision`, modem NV partitions, and controller/radio identity data as per-unit state and never copy them from another headset.
- The old GPL source tree is valuable for DTS archaeology, but proprietary Oculus Insight tracking and camera calibration code are not donor components that can simply be reused under NixOS.

## Lynx R1

### 1. SoC and key hardware

- Qualcomm Snapdragon XR2 Gen 1, an XR derivative of Snapdragon 865. Its downstream Qualcomm board platform is `kona`, placing it in the SM8250 family with Adreno 650.
- 6 GB LPDDR5 RAM and 128 GB UFS 3.1 storage. Community specifications and contemporary reporting agree on [6 GB/128 GB with XR2](https://www.uploadvr.com/lynx-r1-standalone-passthrough/).
- Two 1600 × 1600 LCD panels at 90 Hz using Lynx's folded catadioptric optics.
- Six cameras: two 640 × 400 grayscale tracking cameras, two 400 × 400 hand-tracking cameras, and two RGB passthrough cameras. Lynx publishes stream formats and resolutions in its [Video Capture documentation](https://portal.lynx-r.com/documentation/view/video-capture).
- A community hardware dump identifies OV9282 and OV4689 camera sensors, ICM4X6XX accelerometer/gyro, Bosch BMM150 magnetometer, STK3X3X proximity sensor, JDI/BOE R63455-class panels, QCA6174 PCIe Wi-Fi, and 128 GB WD UFS in [lynx-mainline](https://github.com/ellyq/lynx-mainline).

### 2. Stock OS

- Lynx OS on R1 is AOSP Android 12 with a custom launcher, Qualcomm OpenXR runtime, and Ultraleap hand tracking. The official firmware page identifies [AOSP 12 and the bundled XR stack](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/).
- This is Android, despite Lynx's unusually open recovery path. It is not the newer Lynx R2 software platform.
- Shipped kernel version is **unknown/unverified** from public vendor documentation. Android version alone is not enough to infer it.

### 3. Kernel situation

- **Vendor kernel source availability is unverified.** Extensive searches of Lynx's portal and public GitHub organization found firmware, SDK, and ORB-SLAM sources but no Lynx-R1 kernel tree.
- Lynx's GPLv3 release is its [ORB-SLAM3 library and device demo](https://portal.lynx-r.com/blog/view/open-sourcing-lynx-6dof/), not the GPLv2 Linux kernel. A 2025 postmarketOS status note still said the project [needed kernel sources from the vendor](https://wiki.postmarketos.org/wiki/Lynx_R1_(lynx-r1)).
- Community mainlining has nevertheless progressed using dumps and existing SM8250 support. A published boot log shows a [postmarketOS debug shell on Linux 6.13-rc7](https://nostr.ae/nevent1qqs8fp6rj2trcqzzfvje00yrcdl5vag0qr3craqvrd6f7nqdd85srtszyrcxn3xy22funpm0yq92gccm4n8nt4yrpxt8qzgeppcskxtd3pyr2rl80fx).
- postmarketOS now carries `device-lynx-r1`, and its [SM8250 mainline kernel package](https://pkgs.postmarketos.org/package/master/postmarketos/aarch64/linux-postmarketos-qcom-sm8250) has advanced beyond that initial boot.
- Generic SM8250 mainline coverage is substantial, including UFS, USB, PCIe, clocks, interconnects, and GPU foundations; see [linux-msm's status matrix](https://linux-msm.github.io/mainline-status/soc/sm8250).
- Headset completeness is still far from SoC completeness: dual-panel timing, camera synchronization, sensor fusion, low-latency display, audio, suspend, and XR calibration remain device-specific.

### 4. Boot chain

- Lynx officially states that the [bootloader is open](https://portal.lynx-r.com/documentation/view/getting-started) and alternate system images can be flashed.
- The chain is Qualcomm PBL/XBL → ABL/fastboot → Android boot/recovery. Fastboot is available, and full recovery uses Qualcomm EDL with Lynx-provided Firehose programmer and rawprogram/patch XML files.
- The official Linux recovery instructions support `qdl` and show `adb reboot edl`, `prog_firehose_ddr.elf`, and UFS rawprogram XMLs in the [update/restore guide](https://portal.lynx-r.com/documentation/view/updating-your-device).
- It is A/B: the known dump has `xbl_a/b`, `abl_a/b`, `boot_a/b`, `recovery_a/b`, `dtbo_a/b`, `vbmeta_a/b`, and paired firmware partitions. Lynx's root guide also explicitly flashes [`boot_a` and `boot_b`](https://portal.lynx-r.com/documentation/view/orb-slam-3).
- A physical `super` partition and `vbmeta_system_a/b` are present in the [community partition dump](https://github.com/ellyq/lynx-mainline/blob/main/dumps/partitions.log), so dynamic logical Android partitions are expected.
- No `vendor_boot` partition appears in that dump. Boot header version and the exact placement of DTB/vendor ramdisk remain **unverified** and must be inspected per release.
- Unlock persistence is not exploit-dependent: this is a vendor-supported open state. Lynx's portal allows restoring any listed firmware, but a future policy change cannot be ruled out.

### 5. Stock firmware sources and version policy

- Lynx publishes complete firmware downloads and checksums on the official [R1 firmware page](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/). Release 1.4.1 is the latest listed at the research date.
- Packages support both ADB sideload and complete QFIL/QDL restore. Lynx says users may [restore to a specific version](https://portal.lynx-r.com/documentation/view/updating-your-device), making this the cleanest Android donor workflow in the set.
- No firmware version is known to patch the open bootloader. Pinning is still required because system/vendor interfaces, panel behavior, and firmware blobs change.
- Start with 1.4.1 for current userspace and separately preserve the version used by the mainline developer. Do not assume DT or calibration compatibility across hardware revisions.
- The downloads are publicly served by Lynx. Redistribution terms are not stated; Mura should record URL, MD5/SHA-256, and extraction recipe rather than mirror the ZIP.

### 6. Donor suitability

- Extract `boot.img`, `dtbo`, the downstream DTB, `vendor`/`odm` logical partitions, Wi-Fi/BT firmware, Adreno firmware, ADSP/CDSP blobs, camera firmware, audio DSP data, and UFS rawprogram XML partition metadata.
- Preserve device-specific calibration. Lynx documents `device_calibration.xml`, `svrapi_lens_left.csv`, and `svrapi_lens_rigth.csv` under [`/mnt/vendor/persist/qvr`](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/).
- Never select QFIL's “Erase all before download”: Lynx explicitly warns that this can destroy distortion/calibration data in the [restore guide](https://portal.lynx-r.com/documentation/view/updating-your-device).
- The community dump is especially useful as a machine-readable donor manifest because it identifies paired boot firmware and the `super`/persist layout.

## Samsung Galaxy XR (`SM-I610`)

### 1. SoC and key hardware

- Qualcomm Snapdragon XR2+ Gen 2 with Adreno 740-class graphics, 16 GB LPDDR5 RAM, and 256 GB storage. Samsung's official specification lists [XR2+ Gen 2 and 16 GB/256 GB](https://news.samsung.com/global/introducing-galaxy-xr-opening-new-worlds).
- The public board name is reported as `anorak` by an [SM-I610 Geekbench result](https://browser.geekbench.com/v6/cpu/16721756). This name is also used by Play For Dream, so it may be a Qualcomm platform label rather than a unique product codename.
- Qualcomm has not publicly mapped “XR2+ Gen 2” to an exact SM/QCS part number. Its CPU/GPU resemble SM8550/QCS8550, but treating that mapping as proven would be unsafe.
- Two 3552 × 3840 Micro-OLED displays, 60/72/90 Hz, 109° horizontal × 100° vertical field of view.
- Sensor array: two high-resolution passthrough cameras, six world-facing tracking cameras, four eye-tracking cameras, five IMUs, a depth sensor, and a flicker sensor; all are listed in [Samsung's official table](https://news.samsung.com/global/introducing-galaxy-xr-opening-new-worlds).

### 2. Stock OS

- Galaxy XR is the first shipping Android XR headset. The platform is Android 14-based and Samsung brands the shell “XR One UI”; launch and first-update reporting confirms [Android XR built on Android 14](https://sammyguru.com/galaxy-xr-gets-first-update-with-new-travel-mode-feature/).
- It is Android-based, with Google Play, OpenXR, Gemini integration, and Samsung Knox. It is not a mainline GNU/Linux distribution.
- Shipped Linux kernel version is **unknown**: no reliable `uname -r` or source-package metadata was found in public indexed material.

### 3. Kernel situation

- Samsung released an SM-I610 source package through its official [Open Source Release Center search](https://opensource.samsung.com/uploadSearch?searchValue=SM-I610). This is an official GPL-compliance release, although the portal's dynamic UI makes exact archive metadata difficult to cite.
- The package should be treated as the Android vendor/GKI source for its matching build, not evidence that Android XR userspace or proprietary XR drivers are open.
- The exact XR2+ Gen 2 Linux platform identifier is unverified. If it is close to SM8550/QCS8550, generic support is strong: core clocks, pinctrl, UFS, USB, PCIe, GPU, and reference boards appear in the [SM8550 mainline status](https://linux-msm.github.io/mainline-status/soc/sm8550).
- No public Galaxy XR DTS, boot log, or reproducible mainline tree was located. The FreeXR community status supplied for this research says “mainline Linux in progress,” but a public implementation could not be independently verified.
- **Update (2026-09-23): a working Linux bring-up now exists in public code.** The
  [lightofmysoul Monado fork, branch `galaxyxr`](https://gitlab.freedesktop.org/lightofmysoul/monado/-/tree/galaxyxr)
  (pinned at `references/monado-galaxyxr`, studied in [31-kwin-vr §5](31-kwin-vr.md)) runs
  Kubuntu 26.04 + KDE VR + native Steam on the device and implements: a dual-DRM-lease
  direct-mode display backend (two SDE devices, plane-sliced Sony ECX344A micro-OLED panels,
  UBWC scanout, 90/72 Hz), blob-free 3DoF via Qualcomm SSC QMI-over-QIPCRTR with per-unit `efs`
  factory calibration decoding and live motorized-IPD readout, titan-server stereo NV12
  passthrough fused into the distortion pass, `XR_EXT_eye_gaze_interaction` eye tracking via the
  OEM QNN library, and gaze-driven `VK_KHR_fragment_shading_rate` foveation. 6DoF/SLAM is not
  implemented. It presupposes launch firmware (or root): per the unlock notes below, current
  retail units cannot reach it. Disposition for Mura: [ADR 0013](../architecture/adr/0013-kwin-vr-disposition.md) §4.
- XR-specific gaps will include both display pipelines, multi-camera synchronization, iris/eye tracking, depth sensing, calibration, audio, and power/thermal policy — the fork above now provides working reference code for the display, IMU-tier sensor, passthrough, and eye-tracking slices of that list.

### 4. Boot chain

- Launch units exposed a working OEM Unlock toggle in Android Developer Options; multiple outlets confirmed that it actually completed the [bootloader unlock](https://www.androidauthority.com/samsung-galaxy-xr-bootloader-unlocking-3609841/).
- As a modern Qualcomm Samsung device, the expected low-level chain is PBL → XBL → Samsung/Qualcomm ABL/UEFI → AVB/GKI. This is a **platform inference**; no Galaxy XR-specific ABL/XBL dump was located.
- Samsung normally uses Download Mode/Odin rather than a user-facing Qualcomm fastboot flashing workflow. Exact Galaxy XR fastboot commands and recovery key sequence remain **unknown**.
- The exact partition map, A/B status, `init_boot`, `vendor_boot`, dynamic `super`, and boot header version are **unverified**. Android 14/GKI makes v4 `boot` plus `vendor_boot` and dynamic partitions plausible, but Mura must derive these from the SM-I610 package or a device dump.
- Community tracking reports that the first update on 2025-12-09 removed the working unlock and that the 2026-04-08 update prevents downgrading to launch firmware; see the carefully qualified [Samsung unlock status notes](https://github.com/zenfyrdev/bootloader-unlock-wall-of-shame/blob/main/brands/samsung/README.md).
- Therefore an already-unlocked launch-firmware unit is materially different from a current retail-updated unit. Whether an unlocked unit remains unlocked after each Samsung update is **unverified**.

### 5. Stock firmware sources and version policy

- Samsung's consumer support page offers manuals but [no full firmware downloads](https://www.samsung.com/uk/support/model/SM-I610NZSAEUB/).
- The normal acquisition route is Samsung FUS using model `SM-I610` plus the unit's CSC. Frija/SamFirm-compatible tools fetch official encrypted Samsung packages and produce BL/AP/CP/CSC archives; the [Frija workflow](https://samupdater.com/use-frija-tool-to-download-samsung-stock-firmware/) documents this ecosystem.
- FUS generally exposes the latest firmware for a CSC, not a durable archive of every historical build. Archive the launch package immediately from an eligible unit/account and record CSC, bootloader binary revision, and hashes.
- Pin pre-2025-12-09 launch firmware for unlock work. The first update is reported as build suffix `AYKE`, about 925 MB, in [first-update reporting](https://sammyguru.com/galaxy-xr-gets-first-update-with-new-travel-mode-feature/); avoid it when unlockability is the goal.
- Avoid the 2026-04-08 enterprise/security update and later if downgrade capability matters. Current July 2026 firmware `I610UEU2AZF3` is explicitly a newer security build in [Samsung update reporting](https://www.sammobile.com/news/galaxy-xr-gets-a-mysterious-1-5gb-update/).
- Samsung firmware is proprietary. FUS retrieval for an owned device is preferable to third-party mirrors; Mura should never redistribute AP/BL archives.

### 6. Donor suitability

- Desired donor content: the SM-I610 kernel source matching the pinned image, DTB/DTBO, GKI vendor modules, Qualcomm GPU/ADSP/CDSP firmware, Wi-Fi/BT blobs, camera/depth/eye-tracking firmware, and Android vendor/odm manifests.
- Dump and preserve Samsung per-unit `EFS`/NV and any `persist`-class partition before unlocking. Their exact Galaxy XR labels and calibration schema are **unknown**, so never import another unit's identity or calibration partitions.
- Preserve panel, camera, IMU, iris, and lens calibration separately from redistributable firmware blobs. With twelve-plus optical sensors, stock calibration is likely as important as driver availability.
- Odin/FUS archives may provide partition images but not all per-unit factory state. The donor pipeline must distinguish immutable vendor payloads from per-device secrets and calibration.

## Play For Dream MR (`anorak`)

### 1. SoC and key hardware

- Qualcomm Snapdragon XR2+ Gen 2, 16 GB LPDDR5X, and 512 GB or 1 TB UFS 3.1. Contemporary specifications report [both memory/storage tiers](https://www.uploadvr.com/play-for-dream-mr-xr2-plus-gen-2-headset/).
- The Android board/motherboard name is `anorak`, confirmed by a [Play For Dream Geekbench result](https://browser.geekbench.com/v6/compute/3216615).
- Exact SM/QCS silicon ID is **unknown**. The benchmark reports six 2.36 GHz CPU cores and Adreno 740, but that does not prove an SM8550-compatible device-tree binding.
- Two BOE 3840 × 3552 Micro-OLED panels at 90 Hz, with pancake lenses and a claimed 103° field of view.
- The vendor advertises [11 cameras, seven sensor types, and 22 IR emitters](https://pfdm.ai/blogs/press-release/play-for-dream-technology-enters-asia-pacific-market-with-the-play-for-dream-mr-headset-world-s-first-android-based-spatial-computer), covering color passthrough, world tracking, hand tracking, eye tracking, and depth.

### 2. Stock OS

- DreamOS is Play For Dream's proprietary Android-derived XR system, not Google's Android XR.
- The shipping MR headset is Android 14-based; both the independent hands-on and the [`anorak` benchmark](https://browser.geekbench.com/v6/compute/3216615) identify Android 14.
- DreamOS 4.5 was announced in June 2026, but the vendor did not publish base Android or kernel changes in the [public announcement](https://www.linkedin.com/posts/play-for-dream_were-excited-to-roll-out-dream-os-45-bringing-activity-7468486714676920320-TNai).
- Shipped kernel version is **unknown**.

### 3. Kernel situation

- No official Play For Dream GPL kernel source archive or repository was located. The public developer GitHub material contains SDK samples, not a kernel tree.
- The [enterprise camera sample](https://github.com/PlayForDreamDevelopers/CameraSample-Unity) documents access to tracking, eye, and passthrough streams, but only through proprietary DreamOS enterprise APIs.
- Generic mainline prospects depend on the unverified SoC mapping. SM8550/QCS8550 has strong upstream foundations, but no `anorak` headset DTS, boot log, or public Linux port was found.
- FreeXR's target page reports the device as [“Bootloader unlocked, efuse unburnt by vendor”](https://github.com/FreeXR/FreeXR/blob/init/targets/anorak/README.md). No public mainline implementation accompanies that status.

### 4. Boot chain

- The only concrete public unlock evidence located is the FreeXR target note and its linked [ShinyQuagsire recovery photo](https://mastodon.social/@ShinyQuagsire/114027425822225840). It is community evidence, not vendor documentation.
- “Unburnt eFuse” suggests a development/factory-like unit that does not enforce the production Qualcomm root of trust. It does **not** prove that every retail unit is unlocked.
- Expected chain is Qualcomm PBL → XBL → ABL/UEFI → AVB Android boot images, but all Play For Dream-specific partition names, fastboot commands, A/B behavior, and boot image versions are **unknown**.
- No public Firehose programmer, EDL restore package, `boot.img`, `vendor_boot.img`, or partition dump was found.
- It is unknown whether the open state survives DreamOS updates, whether OTA burns fuses, or whether only an early batch was affected. Treat auto-update as unsafe until a unit's QFPROM and boot state are archived.

### 5. Stock firmware sources and version policy

- The official [downloads page](https://pfdm.ai/pages/downloads) exposes manuals, not full firmware or OTA ZIPs.
- DreamOS updates are delivered over the air. No stable public OTA URL, full recovery image, checksums, or version archive was found.
- Pin the exact factory firmware on an unlocked unit and block updates until bootloader state, anti-rollback, partition layout, and a recovery path are verified.
- DreamOS 4.5 is the latest publicly announced version at the research date, but it is neither recommended nor known-safe for unlock retention.
- Firmware obtained by intercepting an authenticated OTA remains proprietary and may include account-bound URLs. Mura should support user-supplied extraction only.

### 6. Donor suitability

- Highest-priority captures are a complete GPT, XBL/ABL metadata, `boot`/`vendor_boot`/DTBO if present, vendor/odm partitions, Qualcomm firmware, GPU microcode, Wi-Fi/BT data, and all camera/depth/eye-tracking service firmware.
- Factory calibration locations are **unknown**. Preserve every `persist`, `calib`, `factory`, `fsg`, `modemst`, and vendor-NV-like partition before experimentation.
- The 11-camera array makes synchronized intrinsics/extrinsics, lens distortion, IMU alignment, and display warp calibration essential; these cannot be reconstructed from generic XR2+ firmware.
- The enterprise SDK can help validate extracted camera topology, but its proprietary API is not itself a reusable Linux donor.

## Valve Steam Frame (`deckard`)

### 1. SoC and key hardware

- Qualcomm Snapdragon 8 Gen 3, exact Linux platform SM8650, with Adreno 750 and 16 GB unified LPDDR5X. Valve's Steamworks documentation officially confirms [Snapdragon 8 Gen 3 Arm64](https://partner.steamgames.com/doc/steamhardware/steamframe/compatibility); launch specifications list [16 GB LPDDR5X](https://www.gamingonlinux.com/2025/11/valve-reveal-the-new-steam-frame-steam-controller-and-steam-machine-with-steamos/).
- Storage options are 256 GB and 1 TB UFS, plus microSD expansion.
- Two 2160 × 2160 LCD panels, 72–144 Hz, with pancake optics.
- Four outward-facing monochrome cameras handle headset/controller tracking and monochrome passthrough; two inward cameras provide eye tracking. Valve's published specification summary is reproduced in the [hardware announcement](https://www.gamingonlinux.com/2025/11/valve-reveal-the-new-steam-frame-steam-controller-and-steam-machine-with-steamos/).
- Dual Wi-Fi 7 radios separate normal network traffic from a dedicated 6 GHz PC-streaming link.

### 2. Stock OS

- Steam Frame runs SteamOS: an Arch-derived, aarch64 GNU/Linux distribution. It does **not** boot Android.
- Windows/x86 games use Proton plus FEX, while Android APKs run in Valve's Lepton container; Valve documents all three layers in [Steam Frame compatibility](https://partner.steamgames.com/doc/steamhardware/steamframe/compatibility).
- The public image manifest identifies product `steamos`, variant `vr`, architecture `aarch64`, and version/build metadata; for example [build `20260921.6090922`](https://holo-images.steamos.cloud/vr/20260921.6090922/deckard-20260921.6090922-0.5.0.manifest.json).
- A reviewer fastfetch screenshot is reported to show Linux 6.18 on SM8650, but this is [community observation](https://fed.amazonawaws.com/notes/ar6mmrcngxj24aj4), not an official production-kernel statement. Mark the exact shipped release **unverified** until read from a retail unit or reconstructed rootfs.

### 3. Kernel situation

- Holo Core is Valve/Collabora's published pure-aarch64 Arch base; Collabora describes it as the [basis for Steam Frame's OS](https://www.collabora.com/news-and-blog/news-and-events/building-an-arch-linux-aarch64-port-for-holo-core.html).
- Holo Core's generic Arch source snapshot is not necessarily the patched Deckard production kernel. A current, clearly labeled Valve kernel source package was not located; GPL corresponding-source tracking is therefore an open acquisition task.
- **Update (2026-09-23, donor-verified — [33](33-steam-frame-donor.md)):** the production kernel is `6.18.0-gbfea53e51a5d`, pkgbase **`linux-618-deckard`**; binary packages are public (`holo-packages.steamos.cloud/archlinux-deckard-hotfixes/`); the full config was extracted via IKCONFIG and all eight deckard board DTBs (dv1→mp, model `"SM8650 MP rev1 4slam 2et"`) ship in the image; the boot chain is PBL → XBL(A/B) → **U-Boot 2025.07-rc3+valve** → `/boot`. The source tarball remains unpublished in the indexed mirrors (no `deckard` sources dir; checked root + holo-main) — the GPL acquisition task stands, now with the exact package name to request.
- SM8650 has broad upstream support. The [linux-msm SM8650 matrix](https://linux-msm.github.io/mainline-status/soc/sm8650) covers UFS, USB-C, WLAN, Bluetooth, GPU, DSI, DSPs, camera, video, and power foundations across Linux 6.8–6.19.
- Valve and Igalia use the open Mesa Turnip Vulkan driver for Adreno 750 and have upstreamed much of the work; Igalia describes its [Frame-specific Turnip effort](https://www.igalia.com/2025/11/helpingvalve.html).
- The remaining Mura work is primarily Deckard board description, dual-display/camera integration, Valve tracking services, and packaging—not a from-zero Qualcomm Linux port.

### 4. Boot chain

- The exact Frame boot chain and alternate-OS policy are **not publicly documented**. Generic current Qualcomm Linux uses PBL → XBL → UEFI → systemd-boot/UKI, described in Qualcomm's [boot-flow documentation](https://docs.qualcomm.com/doc/80-80022-3/topic/boot-flow-and-architecture-overview.html), but Frame-specific use of systemd-boot/UKI is unverified.
- It does not use Android `boot.img`, `vendor_boot`, fastbootd, or dynamic Android `super` as its OS update abstraction.
- SteamOS uses atomic A/B system partitions with RAUC and casync/desync. Collabora documents the [A/B RAUC update design](https://www.collabora.com/news-and-blog/news-and-events/steamos-3-6-how-the-steam-deck-atomic-updates-are-improving.html).
- A Frame-specific GPT and ESP/bootloader map were not found. Do not copy the Steam Deck's eight-partition GRUB layout into the build until a Frame image or device confirms it.
- Developer Mode officially enables SSH, RDP, and ADB; ADB addresses the native debugging bridge and Lepton instances, not an Android host OS. Valve documents [`ssh steamos@frame`, ADB, and `steamos-readonly disable`](https://partner.steamgames.com/doc/steamhardware/steamframe/debugging).
- Stock root access is therefore operationally available through the `steamos` user and `sudo`. This is different from a cryptographic bootloader unlock; Secure Boot keys and external-boot behavior remain **unknown**.

### 5. Stock firmware sources and version policy

- Valve publicly indexes stock VR builds at [`holo-images.steamos.cloud/vr/`](https://holo-images.steamos.cloud/vr/). This is an official vendor source.
- A current directory contains a tiny signed RAUC bundle plus adjacent casync chunk store, manifest, and chunk details; see [`deckard-20260921.6090922-0.5.0`](https://holo-images.steamos.cloud/vr/20260921.6090922/).
- Goldmaster and MR channels are also openly indexed. `latest-oobe-test-image.txt` points to a specific goldmaster RAUC bundle, so “latest” is channel-dependent.
- Reconstruction is offline-capable: extract the SquashFS-based `.raucb`, obtain `rootfs.img.caibx`, and use casync or desync against the `.castr/` store. The equivalent SteamOS workflow is documented in the [RAUC reconstruction example](https://iliana.fyi/blog/build-your-own-steamos-updates/).
- Pin build ID, manifest hash, RAUC bundle hash, keyring, and chunk-store namespace. Do not silently follow `latest` in reproducible builds.
- Valve makes these files public, but that does not imply every proprietary firmware file inside may be separately redistributed. Prefer fetch-and-hash derivations.

### 6. Donor suitability

- The Frame should use stock SteamOS primarily as a reference and firmware donor, not as an opaque Android BSP.
- Extract `/lib/firmware`, board DTBs/overlays, initramfs, kernel config/modules, Mesa/Turnip version, camera and tracking firmware, Wi-Fi board data, Bluetooth firmware, audio DSP payloads, and RAUC slot configuration.
- Preserve any `persist`/factory/calibration partition outside the RAUC rootfs. Its names and schema are **unknown**, and RAUC bundles may intentionally omit per-unit data.
- The public rootfs should make dependency provenance easier than on Android targets, but Valve's closed tracking/calibration components may remain the limiting donor.

## Meta Quest 3 (`eureka`) — secondary/aspirational

### 1. SoC and key hardware

- Qualcomm Snapdragon XR2 Gen 2. iFixit's board inspection identifies the exact package marking [`SXR2230P-100-AB`](https://www.ifixit.com/Guide/Meta+Quest+3+Chip+ID/165932), an SM8550/8 Gen 2-derived XR part with Adreno 740.
- 8 GB LPDDR5 RAM and 128 GB or 512 GB UFS storage.
- Two 2064 × 2208 LCD panels, 90/120 Hz, with pancake optics and roughly 110° horizontal × 96° vertical field of view.
- Two 4 MP RGB passthrough cameras, four 400 × 400 IR tracking cameras, headset IMU, and an IR depth projector. Meta's comparison page confirms [XR2 Gen 2, 8 GB, and 4 MP passthrough](https://www.meta.com/quest/compare/).
- Board inspection identifies Qualcomm WCN6856 Wi-Fi 6E and a Nordic nRF52833 BLE device in the [Quest 3 chip ID](https://www.ifixit.com/Guide/Meta+Quest+3+Chip+ID/165932).

### 2. Stock OS

- Meta Horizon OS is AOSP-derived Android with Meta's proprietary shell, compositor, tracking, and service stack.
- Quest 3 launched on Android 12.1L and current firmware fingerprints are Android 14 (`UP1A.231005.007.A1`), visible in the [firmware archive metadata](https://cocaine.trade/Quest_3_firmware).
- A June 2026 community root adaptation reports `Linux 5.10.240-g69827d40d782` for incremental `52168470043600520`; this is a direct runtime string in [IonStackQuest3](https://github.com/F-19-F/IonStackQuest3), not a vendor kernel release statement.

### 3. Kernel situation

- Meta's public Oculus kernel repository contains original Quest and Quest 2 drops, but no verified Eureka/Quest 3 branch was found.
- A Quest 3 owner publicly raised the missing corresponding-source problem in Meta's [GPL support forum](https://communityforums.atmeta.com/discussions/dev-general/meta-quest-and-the-gpl/1095133). Until an exact source archive is located, mark Quest 3 GPL source availability as **unverified/missing**, not “published.”
- Generic SM8550 support is mature: clocks, pinctrl, UFS, USB, PCIe, GPU, DSP, and reference devices are tracked in [linux-msm's SM8550 status](https://linux-msm.github.io/mainline-status/soc/sm8550).
- FreeXR's work currently provides kernel memory read/write, root, dumps, and bootloader research—not a bootable mainline port. Its [Eureka/Panther exploit](https://github.com/FreeXR/eureka_panther-adreno-gpu-exploit-1) explicitly warns that writing bootloader partitions can hard-brick the headset.
- No public Eureka mainline DTS or complete port was found.

### 4. Boot chain

- Quest 3 uses Qualcomm XBL and ABL/UEFI with AVB and factory-programmed `OEM_PK_HASH` fuses. ABL exposes fastboot in USB Update Mode, but retail unlock authorization is cryptographically blocked.
- Closely related Quest 3S analysis documents `xbl_a/b`, `abl_a/b`, `vbmeta_a/b`, `devinfo`, and `unlock_token`, plus the fused signature barrier in [panther-bootloader-analysis](https://github.com/darkening-otter624/panther-bootloader-analysis). Applying every Panther detail to Eureka is **not yet verified**.
- Modern Horizon OS uses A/B updates. `vendor_boot`, GKI boot images, and dynamic `super` are expected for an Android 12 launch device, but an authoritative Eureka GPT and boot-header dump were not located.
- Boot header v4 is plausible for Android 12 GKI, yet remains **unverified for Eureka**. Build tooling must inspect the actual images rather than hard-code v4.
- FreeXR/CVE-2025-21479 provides temporary kernel root, not bootloader unlock. For Quest 3, the last known vulnerable v79 build is `51154110129000520`; [`51154110134200520` and newer are patched](https://github.com/zhuowei/cheese).
- A later IonStack adaptation demonstrates temporary root on a June 2026 build, but likewise warns not to modify system/boot partitions. Root after boot cannot patch ABL before it verifies Android.
- There is no public software bootloader unlock as of the research date. Unlock state therefore does not survive reboot because no unlock is achieved; only specific root chains can be rerun.

### 5. Stock firmware sources and version policy

- Meta's official [Software Update Tool](https://www.meta.com/help/quest/software_update/) uses WebUSB to install the latest signed release. It is recovery-friendly but not a historical archive.
- The community [Quest 3 firmware archive](https://cocaine.trade/Quest_3_firmware) supplies historical full/incremental ZIPs, fingerprints, dates, and SHA-256 values.
- For the older FreeXR/`cheese` research path, pin v79 incremental `51154110129000520` or an explicitly supported earlier build, and avoid `51154110134200520` onward.
- Do not “update to v79” without checking the complete incremental number: vulnerable and patched v79 builds both exist.
- For IonStack, pin the exact incremental and generated exploit configuration; a nearby kernel build is not interchangeable.
- Meta ZIPs are proprietary and historical mirrors are unofficial. Store only URLs, expected hashes, and user-facing acquisition instructions.

### 6. Donor suitability

- If temporary root permits safe read-only capture, acquire the full GPT, DTB/DTBO, boot/vendor_boot metadata, vendor modules, GPU firmware, ADSP/CDSP/SLPI payloads, WCN6856 firmware, camera/depth firmware, and vendor/odm manifests.
- Factory tracking, passthrough, depth, display warp, IPD, and IMU calibration locations are **unknown**. Preserve all persist/factory/vision/sensor-NV partitions without modifying them.
- Never transplant `devinfo`, `unlock_token`, identity, attestation, or calibration data between units.
- Because there is no safe flash/recovery path after a boot-chain mistake, donor collection must be read-only and versioned. The FreeXR warning that [EDL reflashing is unavailable without authenticated keys](https://github.com/FreeXR/eureka_panther-adreno-gpu-exploit-1) should be treated as a hard build-system safety constraint.

## External video-out capability (docked-mode fact, verified 2026-09-23)

Per-device evidence for the `mura.hardware.externalDisplay` contract fact
([ADR 0015](../architecture/adr/0015-docked-desktop-mode.md) — mirror tier and docked desktop
mode both gate on it):

| Device | Video out over USB-C | Evidence |
|---|---|---|
| Quest 3 | **Yes — DP alt-mode, vendor-supported.** Wired mirroring auto-starts on connect; tested cable combos published; HDMI needs an *active* DP→HDMI adapter; no audio; DRM content blanked; **passthrough is included in the mirror** | [Meta's official casting-by-cable doc](https://www.meta.com/help/quest/1561768654489777/) |
| Samsung Galaxy XR | **Yes — community-verified out**, officially undocumented. USB-C→HDMI adapter mirroring confirmed by a tester; Samsung's own docs describe the port as data/peripherals-only (and no charging) | [user test report](https://www.reddit.com/r/Galaxy_XR/comments/1t3kjoy/is_a_hardwire_video_connection_possible_on_galaxy/), [Samsung port doc](https://www.samsung.com/us/support/troubleshoot/TSG10007584/), [port discovery](https://www.androidauthority.com/samsung-galaxy-xr-usbc-3610592/) |
| Lynx R1 | **Reported DP alt-mode** (USB-C 3.1 Gen1), direction unconfirmed; official docs only document scrcpy screen-sharing | [VR/AR wiki spec](https://vrarwiki.com/wiki/Lynx_R1), [Lynx screen-sharing doc](https://portal.lynx-r.com/documentation/view/sharing-your-screen?version=1) |
| Valve Steam Frame | **No.** Rear USB-C is USB 2.0 (data + 45 W charge) — no alt-mode; front expansion is MIPI/PCIe, not display | [spec digest](https://steamhardware.io/steam-frame/specs/), [UploadVR announcement](https://www.uploadvr.com/valve-steam-frame-official-announcement-features-details/) |
| Oculus Quest 1 | **No** — Meta scopes wired external-display mirroring to Quest 3-class headsets; Quest 1 has no DP alt-mode path | [Meta doc scope](https://www.meta.com/help/quest/1561768654489777/) |
| Play For Dream MR | **Unknown** — no public port capability documentation located | — |

Notes for ADR 0015: Meta's behaviour is the mirror-tier prior art (hotplug → auto-mirror, DRM
blanking, no audio) *except* that Meta mirrors passthrough by default — Mura's capture
taxonomy default is the opposite (passthrough excluded unless consented,
[spatial-sharing.md §2.2](../architecture/spatial-sharing.md)). The Galaxy XR result means the
flagship docked-mode target has working silicon for it; Steam Frame, the strongest near-term
Linux target, can never dock over its port — docked mode must remain an optional, fact-gated
feature, never assumed.

## Implications for the build system

Mura needs at least two image families. Quest 1, Lynx, Galaxy XR, Play For Dream, and eventually Quest 3 require Android/Qualcomm-aware artifacts: raw `Image`/DTB assembly where possible, Android boot-image packing for each verified header version, AVB metadata policy, A/B slot handling, and optional `vendor_boot`/dynamic-partition support. Steam Frame instead needs an EFI/UEFI and RAUC-oriented target capable of producing signed A/B rootfs updates and, once its actual boot map is confirmed, the appropriate ESP/UKI or Valve-specific boot payload.

Kernel packaging must separate SoC support from board support. MSM8998 and SM8250 can share mature linux-msm foundations, SM8550-like XR2 Gen 2/XR2+ devices need newer GKI/mainline branches, and Steam Frame's SM8650 can track a modern upstream kernel. Each headset still needs its own DTS, panel/camera topology, firmware manifest, calibration preservation rules, and hardware enablement status; “SoC boots” must not be represented as “XR headset works.”

Donor acquisition must be a first-class, reproducible input rather than committed blobs. Lynx can use an official versioned ZIP; Samsung needs model/CSC-aware FUS acquisition and careful launch-firmware pinning; Meta requires official latest-only recovery plus user-supplied historical archives; Play For Dream currently requires capture from an owned device; Steam Frame can reconstruct official RAUC/casync images directly. Nix derivations should record URL, cryptographic hash, license/redistribution status, extraction recipe, and a strict boundary between redistributable firmware and per-unit calibration/identity data.
