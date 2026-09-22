# 02 — postmarketOS (pmbootstrap + pmaports) and meta-qcom

Research for spatial-os build-system design. Sources: local shallow clones under
`references/`: `pmbootstrap` (build tool), `pmaports` (device/package ports),
`meta-qcom` (Yocto BSP layer for Qualcomm). File paths below are relative to each
repo root.

---

## 1. Project purpose (brief)

**postmarketOS** is an Alpine-Linux-derived distribution for phones/tablets/odd
ARM hardware. Its build system is split in two repositories:

- **pmbootstrap** — a Python 3 CLI (no daemon, no containers) that orchestrates
  Alpine chroots on the developer machine to build packages and assemble flashable
  images. Entry point `pmbootstrap.py`, all logic in `pmb/`.
- **pmaports** — a git repository of Alpine `APKBUILD` packaging recipes:
  device ports (`device/`), shared packages (`main/`), cross toolchains
  (`cross/`), plus two policy files that matter greatly for spatial-os:
  `deviceinfo_schema.toml` (typed schema for device descriptors) and
  `kconfigcheck.toml` (kernel-config contract).

**meta-qcom** is Qualcomm's/Linaro's Yocto/OpenEmbedded BSP layer for Qualcomm
SoCs (Dragonboards, RB-series robotics kits, IQ EVKs, automotive Ride boards). It
is the "machine abstraction" counterpart: machine configs in `conf/machine/`,
SoC-family includes in `conf/machine/include/`, recipes for kernel, boot
firmware, GPU userspace, partition tables.

Both target the same silicon families spatial-os cares about: pmaports contains a
**Lynx R1 port (SM8250/XR2 Gen 1)** in `device/testing/device-lynx-r1/`, an
archived **MSM8998** family (`device/archived/soc-qcom-msm8998/` — Quest 1
silicon), and a flagship-quality **SDM845** family in community; meta-qcom covers
**QRB5165/RB5 (SM8250)** via `conf/machine/include/qcom-sm8250.inc` and
`recipes-bsp/packagegroups/packagegroup-rb5.bb`.

---

## 2. Repository/build architecture

### pmbootstrap + pmaports

- pmbootstrap creates a work directory with several Alpine chroots
  (`pmb/core/chroot.py`): one `native` chroot, one `buildroot_<arch>` per foreign
  architecture, and one `rootfs_<device>` per target device. Chroots are
  bootstrapped with a static `apk` and pre-shipped repository signing keys
  (`pmb/chroot/init.py::init_keys`, keys in `pmb/data/keys/`).
- Foreign-arch chroots run through **QEMU user-mode emulation** registered via
  binfmt_misc (`pmb/chroot/binfmt.py`).
- Package builds happen with Alpine's `abuild` inside chroots; pmbootstrap picks a
  cross-compilation strategy per package (`pmb/build/autodetect.py`, §6 below).
- **Channel/branch pinning**: `pmaports/channels.cfg` maps each release channel to
  a pmaports branch *and* an Alpine aports branch/mirror
  (e.g. `[edge] branch_pmaports=main / mirrordir_alpine=edge`,
  `[v26.06] branch_aports=3.24-stable / mirrordir_alpine=v3.24`). A pmaports
  checkout therefore transitively selects the whole Alpine binary package
  universe. `pmaports/pmaports.cfg` gates tool compatibility
  (`version=7`, `pmbootstrap_min_version=3.11.0`) and encodes distro-wide policy
  (supported root filesystems, `supported_fastboot_depends=android-tools-fastboot,...`).
- pmaports layout:
  - `device/{main,community,testing,downstream,archived}/` — tiered device ports (§3).
  - `main/` — shared infrastructure packages (`devicepkg-dev`,
    `postmarketos-mkinitfs`, `boot-deploy`, `msm-firmware-loader`, `lk2nd`,
    `mkbootimg-osm0sis`, `dtbloader`, ...).
  - `extra-repos/systemd/` — an overlay repo enabling systemd variants
    (`[repo:systemd]` in `pmaports.cfg`).

### meta-qcom

Single BSP layer (`conf/layer.conf`: collection `qcom`, priority 6, depends only
on `core`, recommends `meta-arm`). Structure:

- `conf/machine/*.conf` — one file per physical board (§3).
- `conf/machine/include/qcom-<soc>.inc` — SoC-family includes
  (`qcom-sm8250.inc`, `qcom-sdm845.inc`, `qcom-qcs6490.inc`, ...), all pulling
  `qcom-common.inc` → `qcom-base.inc`.
- `classes/` — `linux-qcom-bootimg.bbclass` (Android boot.img generation),
  `linux-qcom-dtbbin.bbclass`, `uki-esp-image.bbclass`;
  `classes-recipe/image_types_qcom.bbclass` (the `qcomflash` image type),
  `qcom-capsule.bbclass` (UEFI FMP capsule updates).
- `recipes-kernel/linux/` — kernel recipes; `recipes-bsp/firmware-boot/` —
  pre-Linux boot stack blobs (XBL etc.); `recipes-bsp/partition/` — GPT binaries;
  `recipes-graphics/adreno/` — proprietary GPU userspace;
  `dynamic-layers/` — content activated only when other layers are present.
- Vendor blobs can come from Qualcomm's artifactory
  (`recipes-graphics/adreno/qcom-adreno_1.877.3.bb` fetches
  `https://qartifactory-edge.qualcomm.com/.../prebuilt_yocto/...tar.gz`), and
  `INHERIT += "qli-mirrors"` in `conf/layer.conf` routes fetches through
  Qualcomm-Linux mirrors.

---

## 3. Device abstraction model

### 3.1 deviceinfo: the device descriptor

A device is described by a flat shell-sourceable key/value file `deviceinfo`. The
authoritative variable list is now a **typed schema**:
`pmaports/deviceinfo_schema.toml` (582 lines, `schema_version = "0.1"`), parsed by
`pmbootstrap/pmb/parse/deviceinfo.py::deviceinfo_schema()`. Schema features worth
copying:

- **Categories**: `metadata`, `flash`, `android_multiboot`, `usb`, `cros`
  (depthcharge Chromebooks), `of` (Open Firmware), plus obsolete `splash`/`weston`.
- **Per-variable metadata**: `description`, `datatype`
  (`string|integer|boolean|enumeration`), `mandatory`, `default_value`,
  `integer_interval` (e.g. `year` must be in `"[1978, 9999)"`), `enum_values`
  (e.g. `flash_method` ∈ `["0xffff","fastboot","fastboot-bootpart","heimdall-bootimg","heimdall-isorec","mtkclient","none","rkdeveloptool","sp-flash-tool","uuu"]`),
  `allow_device_variant_suffix` (per-kernel-variant overrides),
  `behaviour_if_unset`.
- **Deprecation lifecycle encoded in the schema**: obsolete variables carry
  `fate = "obsolete"` and an `epitaph` explaining the replacement (e.g.
  `kernel_cmdline`: *"Replaced by kernel-cmdline.d pmaports!7708"*). Renames are
  first-class: `[rename.metadata.gpu_accelerated] new_name = "drm"`.
- Mandatory variables are only: `format_version`, `name`, `manufacturer`,
  `codename`, `year`, `arch`.

The runtime parser (`pmb/parse/deviceinfo.py::Deviceinfo`) is a Python class with
~80 attributes covering: identity (name/codename/year/arch/chassis), display
(`screen_width/height`, touchscreen dev + calibration), boot image parameters
(`flash_offset_{base,kernel,ramdisk,second,tags,dtb}`, `flash_pagesize`,
`header_version` 0–4, `bootimg_qcdt`, `bootimg_custom_args`, `dtb`,
`append_dtb`), partition targets per flash method
(`flash_{fastboot,heimdall,rk,mtkclient}_partition_{kernel,rootfs,vbmeta,dtbo,vendor_boot}`),
filesystem/partition policy (`boot_filesystem` default ext2, `root_filesystem`
default ext4, `partition_type` default gpt, `rootfs_image_sector_size` for
4096-byte UFS, `super_partitions` for Android dynamic partitions), USB gadget
identity (`usb_idVendor/idProduct`, `usb_network_function` default `ncm.usb0`),
and initramfs policy (`initfs_compression`, `create_initfs_extra`).

**Multiple kernels per device**: variables may be suffixed per kernel variant
(`deviceinfo_dtb_mainline=...`, `_downstream=...`); the parser strips the suffix
for the selected kernel (`_parse_kernel_suffix`), and the device APKBUILD exposes
the variants as `linux-<x>` dependencies. This is how a device offers mainline
and vendor kernels simultaneously.

### 3.2 Device package: what a device *implements*

Every device is a metapackage `device/<tier>/device-<vendor>-<codename>/` with an
`APKBUILD` whose build/package functions are entirely generic — they delegate to
`main/devicepkg-dev/devicepkg_build.sh` and `devicepkg_package.sh`, which:

- source `deviceinfo` and generate `/etc/machine-info`
  (PRETTY_HOSTNAME/CHASSIS/HARDWARE_VENDOR from deviceinfo);
- generate udev touchscreen calibration rules from
  `deviceinfo_dev_touchscreen_calibration` (pixel→libinput matrix conversion);
- install declarative sidecar files with fixed semantics:
  `modules-initfs` → `/usr/share/mkinitfs/modules/00-<pkg>.modules` (initramfs
  module list), `kernel-cmdline.conf` → `/usr/lib/kernel-cmdline.d/50-<pkg>.conf`,
  `modules-load.conf`, `modprobe.conf`, `initfs-hook.sh`;
- install `deviceinfo` itself into the image at
  `/usr/share/deviceinfo/deviceinfo`, where on-device tools (mkinitfs,
  boot-deploy) read it. **The same descriptor drives build-time and run-time.**

### 3.3 Case study A — Lynx R1 (SM8250 XR headset), `device/testing/`

`device/testing/device-lynx-r1/deviceinfo` (excerpt):

```
deviceinfo_arch="aarch64"
deviceinfo_chassis="embedded"
deviceinfo_flash_method="fastboot"
deviceinfo_generate_bootimg="true"
deviceinfo_flash_pagesize="4096"
deviceinfo_header_version="2"
deviceinfo_dtb="qcom/sm8250-lynx-r1"
deviceinfo_flash_offset_dtb="0x01f00000"
deviceinfo_flash_fastboot_partition_rootfs="super"
deviceinfo_super_partitions="/dev/sda6"
deviceinfo_rootfs_image_sector_size="4096"
```

`device/testing/device-lynx-r1/APKBUILD` depends on:
`firmware-qcom-adreno-a650` (generic Adreno blob from Alpine's linux-firmware
packaging), `firmware-lynx-r1-{adreno,adsp,cdsp,cvpss,slpi,venus}` (device blobs,
§4), `linux-lynx-r1` (device kernel), `mkbootimg`, `postmarketos-base`. The whole
port is 3 packages + 1 external firmware repo. `kernel-cmdline.conf` is 7 lines
(`pd_ignore_unused clk_ignore_unused` etc.).

`device/testing/linux-lynx-r1/APKBUILD`: mainline fork
(`gitlab.postmarketos.org/ellyq/linux-lynx-r-1`, v6.13.0), built with
`LLVM=1`, full committed kconfig `config-lynx-r1.aarch64`, one patch, and options
`pmb:cross-native pmb:kconfigcheck-nftables pmb:kconfigcheck-community`.

### 3.4 Case study B — OnePlus 6 "enchilada" (SDM845), `device/community/`

`device/community/device-oneplus-enchilada/deviceinfo` adds
`deviceinfo_flash_sparse="true"`, `deviceinfo_boot_filesystem="fat32"`,
`deviceinfo_append_dtb="true"`, `deviceinfo_flash_kernel_on_update="true"`,
`deviceinfo_initfs_compression="zstd:fast"`. Its APKBUILD dependency chain shows
the SoC-family sharing model:

```
alsa-ucm-conf-sdm845          # family audio profiles
firmware-oneplus-sdm845       # device firmware (pinned commit)
hexagonrpcd>=0.3.2-r3         # DSP RPC daemon (sensors)
linux-postmarketos-qcom-sdm845 # SHARED family kernel
soc-qcom + soc-qcom-modem + soc-qcom-qbootctl   # SoC-family userspace
systemd-boot / unl0kr-fbforcerefresh
```

### 3.5 SoC-family packages

`device/community/soc-qcom/APKBUILD` — a vendor-wide umbrella: depends
`bootmac swclock-offset tqftpserv`, subpackages `-modem` (rmtfs etc.),
`-qbootctl` (A/B slot ack daemon), `-vulkan`, `-pulseaudio`, `-gstreamer`;
ships udev rule `51-qcom.conf`, `UPower.conf`, workarounds. Narrower family
packages layer on top: `soc-qcom-sc7180`, `soc-qcom-msm8953`, ... and archived
`soc-qcom-msm8998` (which `depends="soc-qcom"` and adds a `-nonfree-firmware`
subpackage pulling `soc-qcom-modem qcom-diag`). Family kernels are separate
shared packages consumed by many devices:
`device/community/linux-postmarketos-qcom-sdm845` (fork
`gitlab.com/sdm845-mainline/linux`), `device/testing/linux-postmarketos-qcom-sm8250`
(fork `gitlab.postmarketos.org/soc/qualcomm-sm8250/linux`, v7.2.0, options
include `pmb:kconfigcheck-uefi`), archived `linux-postmarketos-qcom-msm8998`.

So a device port *declares* (deviceinfo + dependency list) and only *implements*
what is unique to it: kernel fork/config if not shared, firmware package, cmdline
and initramfs module lists. Everything mechanical is in `devicepkg-dev`,
`postmarketos-mkinitfs`, `boot-deploy`, and the soc-* packages.

### 3.6 Device tiers

Directory placement *is* the tier; `pmbootstrap/pmb/helpers/devices.py::DeviceCategory`
defines the semantics:

- `main` — "ports where mostly everything works";
- `community` — "often mostly usable, may lack important functionality";
- `testing` — "anything from 'just boots in some sense' to almost fully functioning";
- `downstream` — vendor-kernel ports, "very limited functionality. Not recommended";
- `archived` — "ports that have a better alternative available" (hidden; each
  APKBUILD carries a header comment, e.g. `# Archived: unmaintained` in
  `device/archived/soc-qcom-msm8998/APKBUILD`).

`DeviceCategory.allows_downstream_ports()` hard-codes that **main/community/testing
must be mainline-kernel ports**; downstream kernels are quarantined. Tier
promotion requirements are enforced by CI in-tree:
`pmaports/.ci/testcases/test_kernel.py` fails if a `device/community` or
`device/main` kernel lacks `pmb:kconfigcheck-community` in its options. Maintainer
commitment is encoded as the mandatory `maintainer=` field in the APKBUILD
(empty maintainer ⇒ archived).

### 3.7 meta-qcom machine abstraction

A board conf, e.g. `conf/machine/rb3gen2-core-kit.conf`, declares:

```
require conf/machine/include/qcom-qcs6490.inc
MACHINE_FEATURES += "efi kvm m2connector pci tpm2 phone"
KERNEL_DEVICETREE ?= "qcom/qcs6490-rb3gen2.dtb  ...dtbo overlays..."
MACHINE_ESSENTIAL_EXTRA_RRECOMMENDS += "packagegroup-rb3gen2-firmware ..."
QCOM_CDT_FILE = "cdt_core_kit"            # board config-data-table blob
QCOM_BOOT_FILES_SUBDIR = "qcm6490"        # which XBL blob set
QCOM_PARTITION_FILES_SUBDIR ?= "partitions/qcs6490-rb3gen2/ufs"
QCOM_BOOT_FIRMWARE = "firmware-qcom-boot-qcs6490"
UBOOT_CONFIG = "qcs6490-rb3gen2"
```

The SoC include (`conf/machine/include/qcom-sm8250.inc`) is tiny: sets
`SOC_FAMILY = "sm8250"`, CPU tune (`armv8-2a-crypto`), and boot packagegroups; it
requires `qcom-common.inc`, which sets the defaults everything inherits:
`PREFERRED_PROVIDER_virtual/kernel`, `QCOM_BOOTIMG_KERNEL_BASE ?= "0x80000000"`,
`QCOM_BOOTIMG_PAGE_SIZE ?= "4096"`, `SERIAL_CONSOLES ?= "115200;ttyMSM0"`,
`EFI_PROVIDER ?= "systemd-boot"`, UKI naming, XBL config selection keyed off
`MACHINE_FEATURES` (kvm ⇒ `xbl_config_kvm.elf`). `SOC_FAMILY` feeds
`MACHINEOVERRIDES`, so any recipe can specialize per SoC (`:sm8250`) or per
machine. Notably, the generic `conf/machine/qcom-armv8a.conf` is a *multi-board*
machine: one image with a DTB list covering db410c→RB5
(`qcom/qrb5165-rb5.dtb` included) and per-board overrides via **varflags**:
`QCOM_BOOTIMG_ROOTFS[qcs6490-rb3gen2] ?= "PARTLABEL=system"`.

RB5/QRB5165 specifics: `recipes-bsp/packagegroups/packagegroup-rb5.bb` pulls
`linux-firmware-qcom-adreno-a650`, `linux-firmware-qcom-sm8250-{audio,compute}`,
`linux-firmware-ath11k-qca6390`, RB5 sensor DSP blobs, and
`hexagon-dsp-binaries-thundercomm-rb5-{adsp,cdsp,sdsp}` — i.e. XR2 Gen 1-class
firmware is *already upstreamed into linux-firmware* and only board-specific DSP
userspace comes from `github.com/linux-msm/dsp-binaries`
(`recipes-bsp/hexagon-dsp-binaries/hexagon-dsp-binaries.inc`).

---

## 4. Vendor blob / donor firmware handling

### pmaports

Three coexisting mechanisms:

1. **Per-device firmware packages** pinned to a commit of an external
   "extracted-from-stock" repo. `device/testing/firmware-lynx-r1/APKBUILD`:
   `license="proprietary"`, `options="!check !strip !archcheck !tracedeps pmb:cross-native"`,
   `source=https://github.com/ellyq/lynx-firmware-mainline/archive/$_commit...`,
   one subpackage per IP block (`-adreno` = `a650_zap.mbn`, `-adsp`, `-cdsp`,
   `-cvpss`, `-slpi`, `-venus`), each installing into
   `/lib/firmware/postmarketos/` (a pmOS-priority firmware search path). The
   sha512 in the APKBUILD pins the donor content. `firmware-oneplus-sdm845` adds
   list-file-driven installs (`firmware.files`, `sensor.files`) and a
   `firmware-initramfs.files` manifest so early-boot firmware lands in the
   initramfs.
2. **Runtime extraction from donor partitions**: `main/msm-firmware-loader` —
   "Set of init services to automatically load firmware from device partitions".
   Instead of redistributing blobs, it mounts the stock Android modem/vendor
   partitions at boot and symlinks firmware into place. This sidesteps
   redistribution licensing entirely and guarantees per-unit-correct
   (DRM-provisioned) blobs.
3. **Generic upstream firmware** from Alpine's `linux-firmware-*` split packages
   (e.g. `firmware-qcom-adreno-a650`, `linux-firmware-ath10k` as depends of
   `firmware-oneplus-sdm845`).

The `-nonfree-firmware` subpackage convention (see
`device/archived/soc-qcom-msm8998/APKBUILD`) lets users opt out at install time;
`pmb/install/_install.py::get_nonfree_packages` wires this into `pmbootstrap install`.

### meta-qcom

- `recipes-bsp/firmware/firmware-qcom.inc` is a reusable framework: given
  `FW_QCOM_NAME`, it defines split packages per IP block with glob rules
  (`FILES:linux-firmware-qcom-${FW_QCOM_NAME}-adreno = "${FW_QCOM_PATH}/*_zap.mbn*"`,
  `-audio` = adsp, `-compute` = cdsp, `-sensors` = slpi, `-venus`, `-modem`,
  `-wifi`...), optional zstd/xz compression, and QA-check exemptions
  (`INSANE_SKIP += "arch already-stripped"`).
- **Licensing gates** are done with license texts in-layer
  (`licenses/LICENSE.qcom`, `LICENSE.qcom-2` — QTI redistributable-binary
  agreements) referenced as `LicenseRef-Firmware-qcom` via
  `NO_GENERIC_LICENSE` (`recipes-bsp/hexagon-dsp-binaries/hexagon-dsp-binaries.inc`).
  The proprietary Adreno userspace (`recipes-graphics/adreno/qcom-adreno_1.877.3.bb`)
  even checksums the license PDF (`LIC_FILES_CHKSUM = file://NO.LOGIN.BINARY.LICENSE.QTI.pdf;md5=...`).
  No `LICENSE_FLAGS_ACCEPTED` gating exists in the current layer — acceptance is
  implicit in using the layer.
- Some firmware is deliberately **absent**: `recipes-bsp/firmware/firmware-qcom-rb3gen2.bb`
  is a "Placeholder recipe, actual modem firmware is provided in a separate
  layer" — the split between open BSP layer and NDA blob layer is architectural.
- **Pre-Linux boot stack** is packaged separately in `recipes-bsp/firmware-boot/`
  (`firmware-qcom-boot-<soc>_<build>.bb` deploying XBL/NHLOS binaries;
  `firmware-qcom-cdt-*.bb` for board config-data tables), consumed only by the
  flashable-image class, never installed in the rootfs.

---

## 5. Kernel strategy

- **Mainline-first is structurally enforced**, not aspirational: non-downstream
  tiers cannot contain downstream kernels
  (`pmb/helpers/devices.py::allows_downstream_ports`), and vendor kernels live in
  `device/downstream/` built with compatibility shims
  (`main/devicepkg-dev/downstreamkernel_prepare.sh` swaps in a GCC-compatible
  `compiler-gcc.h` for ancient trees).
- **Per-SoC shared mainline forks** with small patch deltas: `sdm845-mainline`
  (7.1-rc1 + 2 patches), `soc/qualcomm-sm8250` (7.2.0), each consumed by many
  device packages. The full kernel `.config` is committed per arch
  (`config-postmarketos-qcom-sdm845.aarch64`, ~9.7k lines) — no fragment
  composition; kernels are built with `LLVM=1`.
- **kconfig contract** (the pattern spatial-os most wants):
  `pmaports/kconfigcheck.toml` defines rule categories as
  `["category:<name>".">=KVER"."<arches>"]` sections with tristate/list/string
  value semantics, e.g. default category requires
  `DM_CRYPT=y`, `DEVTMPFS=y`, `ANDROID_PARANOID_NETWORK=n`, version-scoped rules
  like `MODULE_COMPRESS_ZSTD=y` only for `>=6.2.0`. Aliases bundle categories:
  `community = [default, containers, debug, filesystems, hardening, input, iwd,
  netboot, nftables, usb_gadgets, waydroid, wireguard, zram, ...]`. Kernel
  APKBUILDs opt in via `options="pmb:kconfigcheck-community pmb:kconfigcheck-uefi ..."`;
  `pmbootstrap kconfig check` (parser: `pmb/parse/kconfigcheck.py`,
  checker: `pmb/parse/kconfig.py`) validates the committed config, and pmaports
  CI (`.ci/lib/kconfig.py`, `.ci/testcases/test_kernel.py`) makes the check a
  merge gate tied to tier.
- **Multiple kernels per device** via APKBUILD subpackage naming plus deviceinfo
  variant suffixes (§3.1), letting a port carry `-mainline` and `-downstream`
  simultaneously during bring-up.

meta-qcom mirrors the same idea at layer level: default kernel is
`linux-qcom-next` (`qcom-base.inc`), a rolling upstream-tracking tree from
`github.com/qualcomm-linux/kernel` pinned by `SRCREV` with config assembled from
in-tree `defconfig` + `arch/arm64/configs/qcom.config` + fragment
`files/configs/bsp-additions.cfg` (`recipes-kernel/linux/linux-qcom-next_git.bb`)
— fragment composition rather than committed monolithic configs. RT variants
(`linux-qcom-rt`) get machine-tunable cmdline isolation parameters
(`QCOM_RT_CPU`, `QCOM_RCU_NOCBS` in `qcom-common.inc`).

---

## 6. Image assembly and flashing

### pmbootstrap build flow

1. **Cross strategy per package** (`pmb/build/autodetect.py::crosscompile`),
   an explicit 5-state enum: `UNNECESSARY` (native arch), `CROSS_NATIVE`
   (kernel-style: full cross toolchain in native chroot, opted in by
   `options="pmb:cross-native"`), `CROSS_NATIVE2` (build in native chroot against
   a bind-mounted foreign sysroot), `CROSSDIRECT` (foreign chroot under QEMU, but
   compiler invocations transparently redirected to native cross tools via
   `/native/usr/lib/crossdirect/<arch>` in PATH — `pmb/build/backend.py`), and
   `QEMU_ONLY` fallback. Kernels and firmware use `cross-native`; most packages
   use crossdirect.
2. **Rootfs assembly** (`pmb/install/_install.py::create_device_rootfs` →
   `install_system_image`): install `device-<codename>` + UI + base metapackages
   with apk into the `rootfs_<device>` chroot; then create a raw image
   (`pmb/install/blockdevice.py` losetup), partition it (GPT default, boot +
   root, `pmb/install/partition.py`), format per deviceinfo
   (`pmb/install/format.py`, LUKS supported), run `mkinitfs` once inside the
   chroot, rsync/copy the chroot into the mounted image, then optionally
   `embed_firmware` (`sd_embed_firmware` writes bootloader blobs at raw offsets
   for SD-boot devices).
3. **boot.img** is generated *inside the rootfs* by
   `postmarketos-mkinitfs`/`boot-deploy` (separate upstream project,
   `main/boot-deploy/`), driven entirely by the installed
   `/usr/share/deviceinfo/deviceinfo` (`deviceinfo_generate_bootimg`,
   offsets, `header_version`, qcdt options) — the device package only has to
   depend on `mkbootimg`. The on-device apk trigger
   (`main/postmarketos-mkinitfs/postmarketos-mkinitfs.trigger`) regenerates it on
   every kernel/module upgrade, skipping inside pmbootstrap (`/in-pmbootstrap`
   marker).
4. **Flasher abstraction**: a declarative table
   `pmbootstrap/pmb/config/__init__.py::flashers` mapping method → actions →
   argv lists with `$VARIABLE` placeholders, resolved by
   `pmb/flasher/variables.py` from deviceinfo. Methods: `fastboot`,
   `fastboot-bootpart` (split boot/root images), `heimdall-isorec`,
   `heimdall-bootimg`, `adb` (recovery-zip sideload), `uuu`, `rkdeveloptool`,
   `mtkclient`, `0xffff`. Cross-cutting actions include `flash_vbmeta`
   (generates a verification-disabled vbmeta with
   `avbtool make_vbmeta_image --flags 2` — the AVB unlock story) and
   `flash_dtbo`. Host USB paths are bind-mounted into the native chroot
   (`flash_mount_bind`) so flashing tools run from the chroot, not the host.
5. **Donor introspection**: `pmbootstrap bootimg_analyze`
   (`pmb/parse/bootimg.py`) unpacks a stock boot.img with `unpackbootimg`,
   detects header version, QCDT type (`qcom|exynos|sprd`), MTK labels, and emits
   the exact `deviceinfo_flash_offset_*` values a new port needs.

### meta-qcom

- `classes/linux-qcom-bootimg.bbclass`: for every DTB in `KERNEL_DEVICETREE`,
  concatenates `Image.gz`+dtb and runs skales `mkbootimg` with
  `QCOM_BOOTIMG_{PAGE_SIZE,KERNEL_BASE}` and a cmdline assembled from
  `SERIAL_CONSOLES` + `KERNEL_CMDLINE_EXTRA` + `root=${QCOM_BOOTIMG_ROOTFS}`,
  producing `boot-<dtb>-<kernel>.img` (+ initramfs-bundled and SD variants).
  Per-DTB overrides via varflags.
- `classes-recipe/image_types_qcom.bbclass` defines `IMAGE_FSTYPES += qcomflash`:
  a directory/tarball combining GPT binaries (from
  `recipes-bsp/partition/qcom-partition-conf_git.bb`, prebuilt
  `gpt_main*.bin` per platform + QDL scripts), boot firmware
  (`QCOM_BOOT_FIRMWARE`), CDT, U-Boot or UKI ESP image, and the 4096-aligned
  ext4 rootfs — everything QDL/EDL needs to flash a blank device
  (`qcom-base.inc`: "QDL expects 4096 aligned ext4 image").
- Modern boards boot via **UEFI + systemd-boot + UKI**
  (`EFI_PROVIDER ?= "systemd-boot"`, `uki-esp-image.bbclass`), Android bootimg is
  the legacy path.

---

## 7. Update mechanism

- postmarketOS updates are **package-based** (`apk upgrade` on device). Kernel
  updates are made safe by the mkinitfs trigger + `boot-deploy`, which rebuilds
  initramfs/boot.img and — only if the device sets
  `deviceinfo_flash_kernel_on_update="true"` (schema warns: *"dangerous, verify
  per device"*) — writes it to the boot partition. On Android A/B devices,
  `soc-qcom-qbootctl` (`device/community/soc-qcom/APKBUILD`) marks the current
  slot successful at boot so the bootloader doesn't roll back.
- Release cadence is inherited from Alpine: channels in `channels.cfg` pair a
  pmaports branch with an Alpine stable branch; `edge` is rolling. There is no
  image-based OTA, no rollback of the rootfs.
- meta-qcom: rootfs updates are out of scope for the BSP layer (that's a distro
  concern), but **boot-firmware updates** are first-class via UEFI FMP capsules —
  `classes-recipe/qcom-capsule.bbclass` builds signed capsules with version and
  anti-rollback floor (`CAPSULE_FW_VERSION`, `CAPSULE_FW_LSV`, OEM PKI required,
  test keys only for CI).

---

## 8. Reproducibility properties

- **pmbootstrap is imperative and stateful**: chroots are mutated by sequences of
  apk/abuild commands; `pmbootstrap zap` exists precisely because state rots.
  Builds pull binary packages from Alpine/pmOS mirrors at whatever index state
  the mirror has; only the *channel* (branch pair) is pinned, not package
  versions. Two `pmbootstrap install` runs weeks apart on the same pmaports
  commit can produce different images.
- What *is* pinned: source tarballs by sha512 in every APKBUILD; firmware donors
  by git commit (`firmware-lynx-r1: _commit="5ee6f8..."`); kernel configs are
  committed in full, so kernel builds are effectively input-complete; repository
  signing keys ship with the tool (`pmb/data/keys/`).
- Determinism boundary worth noting for spatial-os: the deviceinfo parser does
  naive quote-stripping (`value.replace('"', "")` in `pmb/parse/deviceinfo.py`)
  on a bash-sourceable file — the format is convenient but weakly specified; the
  schema TOML is the corrective move (the parser even carries a FIXME to make the
  schema the source of truth).
- **meta-qcom/Yocto is closer to hermetic**: every recipe pins `SRCREV`/sha256,
  task signatures gate sstate reuse, `LIC_FILES_CHKSUM` pins license text, and
  the layer explicitly manages signature-safe exceptions
  (`SIGGEN_EXCLUDE_SAFE_RECIPE_DEPS` in `conf/layer.conf`). But network fetches
  from Qualcomm artifactory and multiple mutable-branch kernels
  (`linux-qcom-next-upstream` uses `AUTOREV`) mean full bit-reproducibility is
  not claimed either.

---

## 9. What spatial-os should adopt

1. **A typed, versioned device schema with a deprecation lifecycle**
   (`deviceinfo_schema.toml`). In Nix this maps almost 1:1 to a NixOS module
   option set (`options.spatial.device.*` with types, defaults, enums,
   `mkRenamedOptionModule` for renames). Keep pmOS's category split
   (identity / boot-image / flash-partitions / usb-gadget) and its discipline of
   *schema-recorded* obsolescence (`fate`/`epitaph`) — invaluable when many
   headset ports evolve at different speeds. Also copy "mandatory minimum"
   being tiny (6 fields) so a bring-up port is a 30-line file.
2. **Three-layer device model: device → SoC family → vendor** as seen in
   `device-oneplus-enchilada → soc-qcom-sdm845(kernel/audio/fw) → soc-qcom`.
   spatial-os targeting msm8998 (Quest 1), sdm845/850, sm8250 (Quest 2/Lynx),
   XR2 Gen 2 should make `soc-qcom-<family>` a Nix module providing the shared
   kernel, firmware search paths, qbootctl, hexagonrpcd, sensor stack; devices
   contribute only DTB name, cmdline, blobs, module lists. meta-qcom's
   `qcom-common.inc → qcom-<soc>.inc → <board>.conf` is the identical shape and
   confirms the pattern is vendor-endorsed; note how small the per-SoC include is
   when the common layer has good defaults.
3. **kconfig contract checks as a merge/eval gate** (`kconfigcheck.toml` +
   `pmbootstrap kconfig check`). In Nix: a derivation assertion that parses the
   built kernel's `.config` against category rule sets (versioned ranges,
   arch-scoped, tristate semantics), with categories composed per device tier.
   This is cheap to implement (pure text check) and is postmarketOS's single most
   effective mechanism for keeping N kernels compatible with one userspace. XR
   additions would be categories like `category:xr` (e.g. `DRM=y`, high-res
   timers, sched deadline) and `category:waydroid`-style optional sets.
4. **Tier policy encoded in the tree and enforced by CI**
   (`device/{main,community,testing,downstream,archived}` +
   `.ci/testcases/test_kernel.py` + `allows_downstream_ports`). For spatial-os:
   tiers should gate which checks are mandatory (kconfig categories, boot test,
   maintainer set), and "downstream kernel allowed" should be an explicit,
   quarantined tier — important early when Quest-class devices may need
   downstream kernels before mainline catches up.
5. **Declarative flasher table** (`pmb/config/__init__.py::flashers` +
   `variables.py`). A tiny interpreter over argv templates supports 9 flash
   protocols with ~200 lines. spatial-os needs at minimum fastboot (Qualcomm
   headsets) and whatever Steam Frame uses; model it as data, including the
   `flash_vbmeta`/`flash_dtbo` auxiliary actions and the avbtool
   verification-disable trick.
6. **Donor introspection tooling** (`pmbootstrap bootimg_analyze`,
   `pmb/parse/bootimg.py`). For a distro whose inputs are pinned stock firmware
   images, an equivalent Nix tool that ingests a donor boot.img and emits the
   device descriptor fragment (offsets, header version, pagesize) removes the
   most error-prone step of porting.
7. **Firmware split by IP block with pinned donors** — both ecosystems converge
   on the same decomposition (adreno/adsp/cdsp/slpi/venus/modem/wifi;
   `firmware-lynx-r1` subpackages ≙ `firmware-qcom.inc` split packages). In Nix,
   each blob set is a fixed-output derivation keyed on the donor commit/hash,
   exposed as per-block outputs so images can omit e.g. modem. Adopt also
   pmOS's **priority firmware dir** (`/lib/firmware/postmarketos` analog) so
   device blobs shadow linux-firmware.
8. **msm-firmware-loader's runtime-extraction option** (`main/msm-firmware-loader`)
   as the answer for blobs that cannot be redistributed: mount donor partitions,
   symlink firmware at boot. For consumer headsets with intact stock partitions
   this may be the *default* strategy, with pinned-donor packages as fallback.
9. **Single descriptor consumed at build-time and run-time** — deviceinfo is
   installed into the image and drives on-device mkinitfs/boot-deploy. In Nix
   the natural analog is the evaluated device module surfacing both in the image
   builder and as `/etc/spatial-device.json` for runtime tools (A/B ack, flash
   scripts, XR runtime probing).
10. **A/B slot acknowledgement daemon** (`soc-qcom-qbootctl` pattern) and
    `flash_kernel_on_update` semantics as explicit, per-device opt-in flags.
11. From meta-qcom specifically: **separating the pre-Linux boot stack**
    (`recipes-bsp/firmware-boot/*`) from the rootfs, and shipping a
    "full flash bundle" image type (`image_types_qcom.bbclass` qcomflash:
    GPT bins + boot firmware + rootfs + flash scripts) — spatial-os will need the
    same "factory restore bundle" artifact per headset; and **in-layer license
    texts with per-recipe LicenseRef gating** for QTI blobs.

---

## 10. What spatial-os should reject and why

1. **Imperative chroot builds and QEMU-emulated packaging**
   (`pmb/chroot/*`, binfmt QEMU, crossdirect). Nix derivations already give
   hermetic, cacheable, per-package sandboxes; reproducing pmbootstrap's chroot
   lifecycle (zap, in-pmbootstrap markers, resolv.conf copying) would be adopting
   its weakest part. Cross-compilation should use Nix's `crossSystem`/pkgsCross,
   not emulation. The *decision enum* (which strategy per package) is worth
   remembering only as evidence that one global strategy doesn't fit all.
2. **Alpine/musl coupling**: APKBUILD shell recipes, abuild, apk triggers,
   `-openrc`/`-systemd` subpackage duplication, `install_if` magic. These are
   Alpine mechanics, not architecture. (Note the schema literally says arch
   "must be supported by Alpine Linux" — a reminder that pmOS inherits its
   platform matrix from its parent distro; spatial-os inherits nixpkgs' instead.)
3. **Mutable on-device package updates as the primary update path** (apk upgrade
   + regenerate boot.img on device). For sealed consumer XR devices, image-based
   A/B updates (systemd-sysupdate/OSTree-style, or Nix generations with A/B
   boot.img) are safer; the mkinitfs-trigger design exists *because* the rootfs
   is mutable. Keep qbootctl, drop the trigger machinery.
4. **Bash-sourceable stringly-typed deviceinfo files** — the parser's naive
   quote handling and "everything is a string" attributes
   (`pmb/parse/deviceinfo.py`) caused enough pain that pmOS bolted a TOML schema
   on afterwards. Start schema-first (Nix module types), never have an untyped
   intermediate format.
5. **Committed 9.7k-line monolithic kernel configs** per SoC
   (`config-postmarketos-qcom-sdm845.aarch64`). They make diffs unreviewable and
   sharing impossible. Prefer meta-qcom's fragment composition
   (`defconfig` + `qcom.config` + `bsp-additions.cfg` in
   `recipes-kernel/linux/linux-qcom-next_git.bb`) expressed as Nix
   `structuredExtraConfig` layers (base + soc-family + device + tier-contract),
   with the kconfigcheck contract validating the *result*.
6. **Yocto's global-namespace machine configuration** — `MACHINEOVERRIDES`,
   varflag-per-dtb overrides (`QCOM_BOOTIMG_ROOTFS[qrb2210-rb1]`), `??=`/`?=`
   precedence games in `qcom-common.inc`. This is exactly the class of
   spooky-action-at-a-distance the Nix module system's explicit merging and
   priorities are designed to replace. Adopt the *layering shape*, not the
   override mechanics.
7. **Tier semantics living partly outside the repo** — pmOS device-category
   *requirements* (what community tier demands beyond kconfig) are on the wiki,
   not in-tree; only fragments are CI-enforced. spatial-os should encode tier
   requirements fully as evaluatable checks.
8. **Placeholder/out-of-band blob layers without in-tree stubs being explicit
   about provenance** (`firmware-qcom-rb3gen2.bb` placeholder): acceptable for
   Qualcomm's NDA world, but spatial-os's "pinned donor firmware" premise should
   make every blob's origin (donor image + extraction path + hash) a first-class,
   evaluatable input instead.

---

## 11. Open questions

1. **Super/dynamic partitions**: `deviceinfo_super_partitions="/dev/sda6"` (Lynx
   R1) and `main/make-dynpart-mappings` show pmOS handling Android dynamic
   partitions by flashing the rootfs *into* `super`. How should a Nix image
   builder target super partitions — regenerate the LP metadata, or always
   overwrite the whole super? (Quest-family devices use dynamic partitions on
   stock; the answer constrains dual-boot viability.)
2. **AVB/secure boot on retail headsets**: pmOS's answer is
   "flash verification-disabled vbmeta" (`flashers` `flash_vbmeta` action), which
   presumes an unlockable bootloader. Quest devices are not fastboot-unlockable
   without exploits — what is the spatial-os equivalent of the flasher table for
   exploit-initiated boot chains, and does the deviceinfo schema need a
   "boot-chain method" axis beyond `flash_method`?
3. **Where does the kconfig contract live** in a Nix design — as derivation
   assertions (fail at build), as NixOS module assertions (fail at eval), or as
   a separate `nix run .#checks` gate? pmOS runs it both interactively and in CI;
   eval-time is attractive but requires parsing `.config` of an already-built
   kernel (IFD or a two-stage check).
4. **Firmware licensing posture**: pmaports ships `license="proprietary"`
   packages from third-party GitHub repos of extracted blobs (e.g.
   `ellyq/lynx-firmware-mainline`) — legally grey. meta-qcom ships QTI
   license texts but the real modem blobs live in NDA layers. For Meta/Quest
   donors, is msm-firmware-loader-style runtime extraction the *only* clean
   distribution story, and what does that imply for image reproducibility
   (per-unit firmware ≠ fixed-output derivation)?
5. **Multi-device images**: meta-qcom's `qcom-armv8a` machine builds one rootfs
   with a DTB list covering a dozen boards + dtbloader; pmOS builds strictly
   per-device images. Should spatial-os aim for a shared aarch64 rootfs with
   per-device boot artifacts (closer to qcom-armv8a + UKI), which Nix's
   content-addressed store makes cheap, or per-device images (simpler A/B
   story)?
6. **SoC-family kernel governance**: pmOS family kernels (sdm845-mainline,
   qualcomm-sm8250) are community forks with their own release tags. Does
   spatial-os track these existing forks as flake inputs (free-riding on their
   XR-adjacent enablement — Lynx R1's kernel is already a 6.13 mainline fork) or
   maintain its own tree per family?
7. **Steam Frame / SteamOS donor**: nothing in either ecosystem models an
   x86-style A/B-image donor with its own updater. The deviceinfo schema's
   `flash_method` enum and the flasher table have no SteamOS entry; a new method
   plus a different update posture will be needed — worth validating the schema's
   extensibility against that case early.
