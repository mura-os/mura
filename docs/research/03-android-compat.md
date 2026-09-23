# 03 — Android Compatibility Ecosystem (Halium, libhybris, droid-hal, Droidian, Waydroid)

Research target: evaluate the existing Android-compatibility stack as **one selectable backend per
subsystem** for a Nix/NixOS-based Wayland XR distribution ("Mura"), not as the foundation of
the distribution. All claims below are cited to files in the locally cloned repositories
(`references/<repo>/<path>`), pinned at the commits recorded in `references/MANIFEST.json`.

Repositories studied, with pinned commits from `references/MANIFEST.json`:

| Repo | Upstream | Commit | Role |
|---|---|---|---|
| `halium-docs` | github.com/Halium/docs | `4bf1a71` | Halium architecture + porting guide (source `.rst`) |
| `halium-boot` | github.com/Halium/halium-boot | `499ea58` | boot.img generator built inside an Android tree |
| `halium-generic-adaptation-build-tools` | gitlab.com/ubports/porting/community-ports/… | `5112934` | UBports' current standalone-kernel port method |
| `libhybris` | github.com/libhybris/libhybris | `7079712` | bionic→glibc loader + HAL wrappers |
| `droid-hal-device` | github.com/mer-hybris/droid-hal-device | `775f6c0` | Sailfish: adaptation as versioned RPMs |
| `droidian` | github.com/droidian/droidian | `6aaea06` | Debian consumer of Halium (**meta-repo is a stub**) |
| `waydroid` | github.com/waydroid/waydroid | `5a51271` | Inverse: Android in LXC on a mainline host |

**Caveat on `droidian`:** the cloned `droidian/droidian` repository contains only
`droidian/README.md` and `droidian/FUNDING.yml` (`git ls-tree -r HEAD` returns exactly those two
files). It is an issue-tracker/landing repo, not the build system. Every Droidian claim in this
document is therefore either quoted from `droidian/README.md` or cross-referenced from
`halium-docs/project/Related-projects.rst`; the actual Droidian packaging (`adaptation-*` packages,
`debos` image recipes, `droidian-*` metapackages) was **not available for code-level study** and is
recorded as an open question in §11.

---

## 1. Project purpose

**Halium** — "a collaborative project to unify the Hardware Abstraction Layer for projects which run
GNU/Linux on mobile devices with pre-installed Android" (`halium-docs/index.rst:5`). Halium's scope
is explicitly two things and nothing more (`halium-docs/project/Scope.rst:4-12`):

1. a minimal Android distribution designed to run inside an LXC container, exposing Android HALs
   through three interface classes (native Android interfaces, libhybris compat layers, custom
   interfaces);
2. a "middleware" set of libraries/kernel modules that talk to those interfaces, "mostly distro and
   toolkit agnostic".

Halium is "primarily a source-only distribution" (`Scope.rst:14`). Deployment tools, initramfs
tooling, and boot-sequence scripts are **reference implementations only** and explicitly not
enforced on downstreams (`Scope.rst:18-31`).

**halium-boot** — a `boot.img` generator that lives as an Android build module
(`halium-boot/Android.mk:80` defines `LOCAL_MODULE := halium-boot`; built with `mka halium-boot`,
`halium-boot/README.md`). It pairs a device kernel with a *downloaded, distro-provided* initramfs.

**halium-generic-adaptation-build-tools** — UBports' current "minimal port" method. It builds a
kernel + boot images + a device overlay tarball from a `deviceinfo` file, **without a local Android
build tree at all**; the Android userspace comes from a prebuilt "Halium GSI" artifact
(`halium-generic-adaptation-build-tools/prepare-fake-ota.sh:54-89`).

**libhybris** — "a way to load drivers compiled for Android from regular linux processes … allows
you to load drivers that link against the bionic c library inside processes whose native c library is
e.g. glibc, musl" (`libhybris/README.md:1-10`).

**droid-hal-device** (Sailfish/mer-hybris) — RPM packaging machinery that converts a *built Android
tree* into a set of versioned, per-device RPMs (`droid-hal-device/droid-hal-device.inc`). It is the
most mature model of "hardware adaptation as a versioned artifact bundle".

**Droidian** — "a GNU/Linux distribution based on Debian for mobile devices … accomplished by using
well-known technologies such as libhybris and halium"; supports "devices released with Android 8.1 or
later (devices with a vendor partition also known as treble compatible)", with experimental support
for older devices that have a Halium 9 port (`droidian/README.md:1-13`).

**Waydroid** — the inverse: "uses Linux namespaces (user, pid, uts, net, mount, ipc) to run a full
Android system in a container … The Android system inside the container has direct access to any
needed hardware", shipping a LineageOS-based image (currently Android 13)
(`waydroid/README.md:7-19`).

---

## 2. Repository / build architecture

### 2.1 Halium classic (Android-tree-centric)

Source acquisition is `repo`-based against `github.com/Halium/android` with a per-generation branch:
`halium-12.0`, `halium-11.0`, `halium-10.0`, `halium-9.0`, `halium-7.1` (LineageOS 14.1 base),
`halium-5.1` (CyanogenMod 12.1 base) — `halium-docs/porting/get-sources.rst:17-43`. A port adds a
**local manifest** at `halium/devices/manifests/[manufacturer]_[device].xml` listing the device tree,
its `lineage.dependencies` repos, and vendor blob repos (`get-sources.rst:65-113`); `./halium/devices/setup DEVICE`
symlinks it to `.repo/local_manifests/device.xml` and syncs (`get-sources.rst:179-181`).

Build is the Android build system: `source build/envsetup.sh`, `breakfast <codename>` (7.1+) or
`lunch` (5.1), then `hybris-patches/apply-patches.sh --mb`, then
`export USE_HOST_LEX=yes; mka hybris-boot; mka systemimage`
(`halium-docs/porting/build-sources.rst:11-49,113-131`).

`halium-boot/Android.mk` shows the boot-image assembly contract precisely: it discovers fstabs by
globbing `device/*/$(TARGET_DEVICE)` then `device/$(PRODUCT_MANUFACTURER)` then `device/$(PRODUCT_BRAND)`
(lines 32-45); it derives `mkbootimg` arguments from `BOARD_KERNEL_BASE`, `BOARD_KERNEL_PAGESIZE`,
`BOARD_KERNEL_CMDLINE`, `BOARD_KERNEL_SEPARATED_DT`, `BOARD_MKBOOTIMG_ARGS`,
`BOARD_CUSTOM_MKBOOTIMG`, `BOOT_RAMDISK_SEANDROIDENFORCE` (lines 53-105). The ramdisk is **not
built** — it is downloaded from a GitHub release:
`https://github.com/halium/initramfs-tools-halium/releases/download/continuous/initrd.img-touch-{armhf,arm64,i386}`
(`halium-boot/get-initrd.sh:18-33`), unless `BOARD_USE_LOCAL_INITRD` points at
`device/*/$(TARGET_DEVICE)/initramfs.gz` (`halium-boot/Android.mk:110-115`).

### 2.2 UBports standalone-kernel method (current minimal port)

Entry point `halium-generic-adaptation-build-tools/build.sh`. Flow (lines 46-122):

```
source ./deviceinfo                         # port-tree config, the only required input file
source setup_repositories.sh  <downloads>   # fetch toolchains, mkbootimg, avb, ramdisks, kernel
[build-ufdt-apply-overlay.sh]               # build ufdt_apply_overlay from dtc + libufdt
build-kernel.sh <downloads> <tmp/system>    # defconfig; make; modules_install into tmp/system
[make-dtboimage.sh]                         # mkdtboimg.py create -> tmp/partitions/dtbo.img
make-bootimage.sh …                         # boot.img [+ init_boot/vendor_boot/recovery]
build-tarball-mainline.sh                   # -> out/device_<codename>.tar.xz + .tar.build
```

Everything Android is prebuilt and downloaded (`setup_repositories.sh`): GCC 4.9 and clang from
`android.googlesource.com/platform/prebuilts/...` (lines 57-149), `external/dtc` + `system/libufdt`
at `pie-gsi` (155-156), `external/avb` at `android13-gsi` (159),
`LineageOS/android_system_tools_mkbootimg` at `lineage-20.0` (201), the halium-boot ramdisk from the
`initramfs-tools-halium` releases (lines 214-235), and a UBports unified recovery ramdisk from a
Jenkins `lastSuccessfulBuild` artifact URL (lines 237-265).

The CI contract is a single include (`gsi-port-ci.yml:1-6`): a port's `.gitlab-ci.yml` becomes
`include: [ …/gsi-port-ci.yml ]`. Three stages: `build` (apt-install a toolchain set on
`ubuntu:22.04`, clone the tools into `./build`, run `./build/build.sh`), `flashable` (real OTA), and
`devel-flashable` (fake OTA) — `gsi-port-ci.yml:18-85`.

### 2.3 libhybris

Autotools, rooted at `libhybris/hybris/configure.ac`, with an out-of-tree Android-side component
directory `libhybris/compat/` containing `Android.mk` files (`compat/hwc2/Android.mk`,
`compat/camera/Android.mk`, `compat/media/Android.mk`, `compat/ui/Android.mk`,
`compat/input/Android.mk`, `compat/surface_flinger/Android.mk`). **This split is the central
packaging fact**: libhybris is two build systems producing two halves of each bridge — the glibc half
(autotools) and the bionic half (built inside an Android tree).

Top-level subdir order (`libhybris/hybris/Makefile.am:1-17`): `include common properties hardware ui
gralloc [libsync] platforms egl glesv1 glesv2 sf input camera vibrator media opencl wifi hwc2
[vulkan] libnfc_nxp libnfc_ndef_nxp utils tests`.

### 2.4 droid-hal-device

A single included spec fragment `droid-hal-device.inc` (1335 lines) plus a per-device spec generated
from `droid-hal-@DEVICE@.spec.template` (which is 17 lines: `%define device`, `%define vendor`,
pretty names, `%define installable_zip 1`, `%include rpm/dhd/droid-hal-device.inc`). The Android
build itself is executed *inside the RPM's `%build`*, in an Ubuntu chroot, on OBS:

```
ubu-chroot -r /srv/mer/sdks/ubu ... "cd %android_root; source build/envsetup.sh;
   lunch %{?lunch_device}%{!?lunch_device:%{device}}%{?device_variant};
   rm -f .repo/local_manifests/roomservice.xml; make %{?_smp_mflags} %{?hadk_make_target}%{!?hadk_make_target:hybris-hal}"
```
(`droid-hal-device/droid-hal-device.inc:463`). Locally, `droid-hal-device/build-from-android` just
runs `mb2 -s rpm/droid-hal-$DEVICE.spec build` in a tree that already contains `out/`.

`droid-hal-device/helpers/build_packages.sh` orchestrates the whole adaptation: `-d` droid-hal, `-c`
droid-configs, `-m` middleware, `-g` droidmedia, `-v` droid-hal-version, `-i` image via `mic`
(lines 40-46, 413-420).

### 2.5 Waydroid

A pure-Python host tool (`waydroid/waydroid.py`, `waydroid/tools/`) with no compilation step
(`waydroid/Makefile:32-33`: "Nothing to build, run 'make install' to copy the files!"). It composes
an LXC config from versioned snippets, probes the host for HALs, and mounts downloaded
`system.img`/`vendor.img`. Runtime deps: `lxc`, `python3-gbinder`, `python3-dbus`, `polkitd`,
`pipewire-pulse | pulseaudio`, `iptables` (`waydroid/debian/control:14-27`).

---

## 3. Device abstraction model — what a port declares vs. implements

### 3.1 Halium classic: a port declares a repo manifest; implements an Android device tree

The declaration is XML: `halium/devices/manifests/[manufacturer]_[device].xml` with
`<project path="device/[manufacturer]/[device]" name=… remote=… revision=…/>` plus one line per entry
in the device repo's `lineage.dependencies`, plus `vendor/` blob repos
(`halium-docs/porting/get-sources.rst:65-113`). Everything else — kernel defconfig, `BoardConfig.mk`,
fstab, `ueventd*.rc`, `init*.rc` — is implemented as a normal Android device tree.

Two device-specific artifacts must be hand-edited *outside* the device tree:

- `halium/hybris-boot/fixup-mountpoints`: per-codename `sed` rules that rewrite
  `/dev/block/by-name/*` aliases to literal `/dev/block/*` nodes, "because `by-name` is not populated
  by systemd" (`halium-docs/porting/build-sources.rst:82-107`). The required procedure is: for each
  fstab line whose type isn't `auto`/`emmc`/`swap`, run `readlink -f <src>` on the device over ADB and
  record the mapping.
- udev rules, mechanically derived from the Android ueventd config:
  ```
  cat out/target/product/[codename]/root/ueventd*.rc | grep ^/dev | sed -e 's/^\/dev\///' \
    | awk '{printf "ACTION==\"add\", KERNEL==\"%s\", OWNER=\"%s\", GROUP=\"%s\", MODE=\"%s\"\n",$1,$3,$4,$2}'
  ```
  (`halium-docs/porting/debug-build/udev.rst:22`). These are device-specific but Halium asks porters
  to upstream them into `Halium/lxc-android` (`udev.rst:36-39`).

Device *documentation* has its own schema (`halium-docs/supplementary/devices/devicetemplate.rst`,
instantiated e.g. in `supplementary/devices/laurel_sprout.rst`: codename, Halium status, kernel
version, SoC, GPU, sensors, per-distribution "what works / what doesn't") — but it is prose RST, not
machine-readable. There is no Halium-wide device database in a parseable format.

### 3.2 UBports standalone-kernel: a port declares `deviceinfo` + overlay directories

This is the cleanest input contract in the ecosystem and the most directly relevant model for
Mura. The complete declared surface is `deviceinfo` (documented exhaustively in
`halium-generic-adaptation-build-tools/deviceinfo.sample`, 246 lines) plus a handful of
convention-named directories in the port repo root.

Declared identity and generation:

```
deviceinfo_name / _manufacturer / _codename / _arch / _kernel_arch
deviceinfo_halium_version="10"           # 9|10|11|12 documented; 9..16 accepted by scripts
deviceinfo_ubuntu_touch_release="24.04-2.x"
```
(`deviceinfo.sample:14-50`.) `deviceinfo_halium_version` is the single knob that selects the whole
Android generation: clang branch/revision (`setup_repositories.sh:99-134`), kernel build-tools branch
(`setup_repositories.sh:171-191`), and the prebuilt Halium GSI tarball
(`prepare-fake-ota.sh:56-90`).

Declared kernel:

```
deviceinfo_kernel_source / _kernel_source_branch / _kernel_defconfig
deviceinfo_kernel_cmdline / _kernel_vendor_cmdline
deviceinfo_kernel_clang_compile / _kernel_clang_branch / _kernel_clang_revision
deviceinfo_kernel_llvm_compile / _kernel_use_lld / _kernel_gcc_toolchain_source
deviceinfo_kernel_image_name="Image.lz4" / _kernel_disable_modules
```
(`deviceinfo.sample:58-103`.) Note `deviceinfo_kernel_source_branch` — a *branch*, cloned
`--depth 1` (`setup_repositories.sh:29`), not a commit.

Declared boot-image geometry (extractable from a stock `boot.img` via `unpack_bootimg.py`, per
`deviceinfo.sample:113`):

```
deviceinfo_bootimg_header_version=0..4
deviceinfo_flash_pagesize / _offset_{base,kernel,ramdisk,second,tags,dtb}
deviceinfo_bootimg_os_version / _os_patch_level / _partition_size
deviceinfo_bootimg_tailtype="SEAndroid" / _append_vbmeta
deviceinfo_bootimg_has_init_boot_partition        # Android 13+ launch devices
deviceinfo_bootimg_has_vendor_kernel_boot_partition
deviceinfo_{dtbo,prebuilt_dtbo,dtbo_ids,bootimg_prebuilt_dtb,bootimg_dt,kernel_apply_overlay}
```
(`deviceinfo.sample:116-189`.)

Implemented (not declared) by the port as filesystem overlays, consumed by name:

| Path in port repo | Consumer | Effect |
|---|---|---|
| `overlay/` | `build-tarball-mainline.sh:24` (`cp -av overlay/* "${dir}/"`) | contents of the device tarball: `system/` files, `/etc`, halium-overlay init/props |
| `ramdisk-overlay/` | `make-bootimage.sh:131-141` | appended cpio segment onto the downloaded halium-boot ramdisk |
| `vendor-ramdisk-overlay/` | `make-bootimage.sh:150-199` | vendor_boot ramdisk; `lib/modules/modules.load` drives module selection |
| `ramdisk-recovery-overlay/` | `make-bootimage.sh:81` | overrides files in the unified UBports recovery ramdisk |
| `<release>-overlay/system/` | `build-tarball-mainline.sh:26-28` | per-Ubuntu-Touch-release overlay |
| `rsa4096_<partition>.pem` | `make-bootimage.sh:61-63` | AVB signing key for that partition's hash footer |

The module-selection logic in `make-bootimage.sh:156-194` is worth noting for a Nix reimplementation:
it reads `vendor-ramdisk-overlay/lib/modules/modules.load`, resolves each entry plus its
`modules.dep` closure against the `modules_install` tree, copies the flat set into the vendor ramdisk,
and **rewrites `modules.dep` to the flat GKI `/lib/modules/*.ko` layout**. That is a genuine
build-system responsibility, not a packaging detail.

### 3.3 droid-hal-device: a port declares RPM macros; inherits ~1300 lines of spec

Per-device spec = macro definitions + `%include rpm/dhd/droid-hal-device.inc`
(`droid-hal-@DEVICE@.spec.template`). The macro vocabulary is documented in the header of
`droid-hal-device.inc:1-101`:

- identity: `device`, `vendor`, `rpm_device`, `rpm_vendor`, `lunch_device`, `device_variant`,
  `device_target_cpu`, `droid_target_aarch64` / `droid_target_armv7hl`;
- capabilities: `installable_zip`, `enable_kernel_update`, `enable_bootloader_update`,
  `enable_dtbo_update`, `enable_init_boot_update`, `enable_vendor_boot_update`;
- build control: `hadk_make_target` (default `hybris-hal`), `pre_actions`, `custom_build_cmds`,
  `custom_install_cmds`, `custom_prep_cmds`, `custom_build_requires`, `dhd_sources`;
- adaptation content: `android_config` (a multi-line macro of `#define`s injected into
  `android-config.h`, e.g. `#define WANT_ADRENO_QUIRKS 1`, `#define MALI_QUIRKS 1` — lines 94-99),
  `makefstab_skip_entries`, `straggler_files`, `additional_ha_groups`,
  `additional_ha_groups_remove`, `header_patches`, `provides_own_headers`;
- artifact overrides: `have_custom_img_boot`, `have_custom_img_recovery`, `have_custom_init_boot`,
  `have_custom_vendor_boot`, `bootloader_binary`, `dtbo_binary`.

Everything else — subpackage layout, udev generation, mount-unit generation, header extraction,
user/group creation — is **shared** and version-gated by `android_version_major`, which the spec
itself extracts from three different locations depending on Android generation
(`droid-hal-device.inc:476-486`: `build/release/flag_values/*/RELEASE_PLATFORM_VERSION_LAST_STABLE.textproto`,
then `build/release/build_flags.scl`, then `build/core/version_defaults.mk`).

### 3.4 Waydroid: nothing is declared; the host is *probed*

Waydroid has no device description. `waydroid/tools/actions/initializer.py:22-41` derives a
`vendor_type` from host properties:

```python
vndk_str = helpers.props.host_get(args, "ro.vndk.version")
vendorapi_str = helpers.props.host_get(args, "ro.vendor.build.version.sdk")
ret = "MAINLINE"
if vndk_str != "":
    vndk = int(vndk_str)
    if vndk > 19:
        halium_ver = vndk - 19
        if vndk > 31: halium_ver -= 1     # 12L -> Halium 12
        ret = "HALIUM_" + str(halium_ver)
```

HAL discovery is filesystem + binder probing (`waydroid/tools/helpers/lxc.py:223-257`):
`find_hal(name)` scans `ro.hardware.<name>`, `ro.hardware`, `ro.product.board`, `ro.arch`,
`ro.board.platform` and tests `{/odm,/vendor,/system}/lib[64]/hw/<name>.<prop>.so`; `find_hidl()` /
`find_aidl()` call `gbinder.ServiceManager("/dev/hwbinder")` / `("/dev/binder")` and check
`list_sync()`. The resulting decisions are emitted as Android properties into
`waydroid_base.prop` (`lxc.py:259-370`) — e.g. gralloc falls back to `gbm` with
`ro.hardware.egl=mesa` and `gralloc.gbm.device=<renderD*>` when no vendor gralloc HAL is found
(`lxc.py:271-287`), and Vulkan driver is mapped from the host DRM kernel driver
(`waydroid/tools/helpers/gpu.py:37-60`: `msm`/`msm_dpu` → `freedreno`, `panfrost` → `panfrost`,
`i915`/`xe` → `intel`, …).

**This probe-based model is the wrong fit for a Nix build system** (it is runtime, impure, and
unreproducible), but the *decision table it encodes* is directly reusable as build-time
per-device configuration.

---

## 4. Vendor blob / donor firmware handling

Four distinct strategies exist in these repos, and the differences matter enormously for Mura's
"pinned donor firmware image" plan.

### 4.1 Halium classic: blobs as git repos in the Android tree

"Vendor blobs go in the `vendor/` folder in your Halium source tree … Check if your device's vendor
is listed in TheMuppets' GitHub organization" (`halium-docs/porting/get-sources.rst:110-114`).
Remotes `them` (`github.com/TheMuppets`) and `them2` (`gitlab.com/the-muppets`) are predeclared in
the halium-7.1 manifest (`get-sources.rst:145-155`). Blobs are thus consumed as **third-party git
repositories of extracted proprietary libraries**, then compiled/packaged into `system.img` by the
Android build. There is no provenance link back to a specific stock firmware image, and no
extraction step in-tree.

An escape hatch is documented for devices that keep blobs on a dedicated firmware partition: "no need
for a vendor blobs repository in the manifest … reflash android to ensure the blobs are in the
partition … reflash halium" (`halium-docs/porting/debug-build/debug-android-userspace.rst:89-99`) —
i.e. the stock firmware partition is left in place and treated as a donor.

### 4.2 UBports standalone-kernel: the stock `vendor`/`system` partitions are left *in situ*

This is the most important architectural finding in the adaptation build tools: **they never ingest
blobs at all.** Grepping the full script set for vendor-blob handling yields only
`halium-overlay`/`android` overlay paths (`build-tarball-mainline.sh:30-56`) and no extraction,
`simg2img`, or blob-copy step. The port's outputs are (a) a kernel, (b) boot images, (c) an overlay
tarball. The Android userspace is a *generic* prebuilt:

```
DEVICE_GENERIC_URL="$DEVICE_GENERIC_URL_BASE/halium-14.0/lastSuccessfulBuild/artifact/halium_halium_arm64.tar.xz"
```
(`prepare-fake-ota.sh:80-85`, with per-halium-version cases for 9, 10 (arm/arm64), 11, 12, 13
(arm/arm64), 14, 15|16). The device-specific proprietary HALs come from the **stock vendor partition
already flashed on the device**, reached through `androidboot.*` cmdline properties and
`systempart=/dev/mapper/system` dynamic-partition mapping (`deviceinfo.sample:69,72`).

The port only *overrides* pieces of the donor's Android config, via overlay trees that the
build script normalizes permissions for (`build-tarball-mainline.sh:30-56`):

```
system/opt/halium-overlay/{system,vendor}/etc/init      # .rc overrides, chmod 644
system/usr/share/halium-overlay/{system,vendor}/etc/init
system/android/{system,vendor}/etc/init
…/{prop.halium,build.prop}                              # chmod 600
```

Consequence for Mura: with this method the "donor firmware image" is not an *input to the
build* — it is a *precondition of the device state*. That is unacceptable for a reproducible Nix
build system, but the overlay mechanism (a small, declarative, per-device set of init/prop overrides
layered on an opaque vendor partition) is exactly the right shape.

### 4.3 droid-hal-device: blobs are repackaged into versioned RPMs

The Sailfish model ingests a fully built Android tree and *repackages* the results. From
`droid-hal-device.inc`:

- Android `/system/bin` and `/system/lib[64]` are copied wholesale into
  `%{_libexecdir}/droid-hybris/system/` (lines 722-730) — i.e. the entire Android userspace becomes
  file payload of an RPM, living at a non-Android path.
- A curated `dev/alog` subset (`liblog.so`, `libcutils.so`) is symlinked into
  `%{_libexecdir}/droid-hybris/lib{,64}-dev-alog/` for "trying to run pure android apps"
  (lines 880-886, 1242-1248).
- `%define __requires_exclude ^.*$` and
  `%define __provides_exclude_from ^%{_libexecdir}/droid-hybris/.*$` (lines 118-119) disable RPM's
  automatic dependency extraction over the blob tree — necessary because bionic `.so`s would
  otherwise generate unsatisfiable `NEEDED` deps. **Any Nix packaging of a donor blob tree needs the
  exact analogue** (`dontPatchELF`, no `autoPatchelf`, and exclusion from `stdenv`'s RPATH fixups).
- `%define __strip /bin/true` and `%define _missing_build_ids_terminate_build 0` (lines 168-172) —
  blobs must not be stripped and have no build-ids.
- Bionic core libs are *replaced* from APEX content: for Android 10–14 it hunts
  `out/soong/.intermediates/art/build/apex/com.android.runtime.release/.../image.apex/lib/bionic/lib{dl,c,m}.so`,
  falling back to three other paths, and for Android 15+ takes `system/lib64/bootstrap/lib*.so`
  (lines 730-787). Empty `/apex` and `/bootstrap-apex` directories are created for `apexd` to mount
  into, and `linkerconfig` is shipped (lines 730-792).
- Two subsystems are deliberately *excluded* and packaged separately as "localbuild" specs:
  `libaudioflingerglue.so`/`miniafservice` and `libdroidmedia.so`/`libminisf.so`/
  `minimediaservice`/`minisfservice` (lines 798-806, with
  `helpers/droidmedia-localbuild.spec`, `helpers/audioflingerglue-localbuild.spec` and
  `helpers/pack_source_*.sh`).

### 4.4 Waydroid: donor images as signed OTA payloads, plus host `/vendor` passthrough

`waydroid/tools/helpers/images.py:25-82` downloads `system.img` and `vendor.img` from two independent
OTA channels, validates `sha256sum` against the channel manifest's `id` field, and extracts. Channel
defaults: `https://ota.waydro.id/system` and `https://ota.waydro.id/vendor`, `rom_type=lineage`,
`system_type=VANILLA|GAPPS` (`waydroid/tools/config/__init__.py:76-82`). Preinstalled images may be
dropped at `/etc/waydroid-extra/images` or `/usr/share/waydroid-extra/images`
(`config/__init__.py:37-40`), which disables the updater (`initializer.py:74-81`).

On a *Halium* host (non-MAINLINE `vendor_type`), Waydroid bind-mounts the host's own vendor
partition into the container as `/vendor_extra` and rewrites property paths accordingly:

```python
if args.vendor_type != "MAINLINE":
    if not make_entry("/dev/hwbinder", "dev/host_hwbinder"):
        raise OSError('Binder node "hwbinder" of host not found')
    make_entry("/vendor", "vendor_extra", options="rbind,optional 0 0")
```
(`waydroid/tools/helpers/lxc.py:79-82`; path rewriting at `lxc.py:291-306`,
`images.py:140`). Host EGL directories (`/vendor/lib{,64}/egl`) and `/odm` are bind-mounted over the
container image (`images.py:187-197`), and host `/vendor/etc/permissions/android.hardware.nfc.*` etc.
are copied into a `host-permissions` directory that is mounted at
`vendor/etc/host-permissions` (`lxc.py:94-96, 372-398`).

---

## 5. Kernel strategy

### 5.1 Halium's authoritative kernel-config requirements

Two config checkers exist and they disagree in scope:

1. **`halium-boot/check-kernel-config`** — self-contained, in-repo, and therefore the concrete
   authoritative list. It defines three sets and can auto-fix with `-w` (lines 287-360).
   - `CONFIGS_ON` (~190 symbols, lines 16-212): namespaces and cgroups (`CONFIG_NAMESPACES`,
     `UTS_NS`, `IPC_NS`, `USER_NS`, `PID_NS`, `NET_NS`, `CGROUPS`, `CGROUP_FREEZER`,
     `CGROUP_DEVICE`, `CGROUP_PERF`, `BLK_CGROUP`, `CPUSETS`, `CGROUP_MEM_RES_CTLR{,_SWAP,_KMEM}`),
     systemd prerequisites (`DEVTMPFS`, `DEVTMPFS_MOUNT`, `FHANDLE`, `EPOLL`, `SIGNALFD`,
     `TIMERFD`, `INOTIFY_USER`, `FANOTIFY`, `FANOTIFY_ACCESS_PERMISSIONS`, `AUTOFS4_FS`,
     `TMPFS_XATTR`, `TMPFS_POSIX_ACL`, `SECCOMP`, `SYSVIPC`, `IKCONFIG`, `IKCONFIG_PROC`), audit
     (`AUDIT`, `AUDITSYSCALL`, `AUDIT_TREE`, `AUDIT_WATCH`), LSM scaffolding
     (`SECURITY`, `SECURITYFS`, `SECURITY_NETWORK`, `SECURITY_PATH`, `SECURITY_APPARMOR`,
     `SECURITY_APPARMOR_HASH`, `SECURITY_APPARMOR_UNCONFINED_INIT`, `SECURITY_SELINUX`,
     `SECURITY_SELINUX_BOOTPARAM`, `SECURITY_SELINUX_DISABLE`, `SECURITY_YAMA`), plus a very large
     netfilter/xtables/IPsec/IPv6 block and `CONFIG_VT`, `CONFIG_VT_CONSOLE`, `CONFIG_SWAP`.
   - `CONFIGS_OFF` (lines 214-254): `NETPRIO_CGROUP`, `NET_CLS_CGROUP`, `FW_LOADER_USER_HELPER`,
     **`ANDROID_LOW_MEMORY_KILLER`**, **`ANDROID_PARANOID_NETWORK`**, all `DEFAULT_SECURITY_*`
     except apparmor, **`FRAMEBUFFER_CONSOLE`**, `VT_HW_CONSOLE_BINDING`, `RT_GROUP_SCHED`,
     `ARM_UNWIND`, `DEVKMEM`, `IP_SET`, `IP_VS`, `KGDB`, and the entire `BT_HCI*` set.
   - `CONFIGS_EQ` (lines 255-268): `CONFIG_DEFAULT_SECURITY="apparmor"`,
     `SECURITY_APPARMOR_BOOTPARAM_VALUE=1`, `SECURITY_SELINUX_BOOTPARAM_VALUE=0`,
     `SECURITY_SELINUX_CHECKREQPROT_VALUE=1`, `DEFAULT_MMAP_MIN_ADDR=32768`,
     `DEFAULT_IOSCHED="deadline"`, `EVM_HMAC_VERSION=2`.
2. **`mer-kernel-check`** — the porting guide's recommended tool
   (`halium-docs/porting/build-sources.rst:58-62`), and the one droid-hal-device actually runs in
   `%build`: `hybris/mer-kernel-check/mer_verify_kernel_config out/target/product/%{device}/obj/*/.config`
   (`droid-hal-device.inc:472-473`). Not vendored in any cloned repo.

Three caveats that a config-generating build system must encode:

- `# CONFIG_X is not set` is **not a comment** — it unsets a previously set symbol; the docs warn
  explicitly and point at Halium docs PR #85 (`build-sources.rst:65-72`).
- `CONFIG_IKCONFIG` and `CONFIG_IKCONFIG_PROC` must be `y` "otherwise Halium wont boot"
  (`build-sources.rst:74-75`).
- Apparmor-vs-SELinux is a *distro-level* conflict, not a Halium-level decision: "Ubuntu touch needs
  apparmor patches in kernel, while Sailfish doesn't" (`halium-docs/project/Planning.rst:48`). The
  `check-kernel-config` list above is the Ubuntu-Touch-flavoured variant. For Mura this means
  the kernel-config fragment set must be *composed per distro policy*, not copied.

`lxc-checkconfig` is the runtime verification ("All option except `User namespace` need to be the
green word enabled" — `halium-docs/porting/debug-build/debug-android-userspace.rst:13-17`), and the
docs record a counter-intuitive fix for Linux 3.4: disable `CONFIG_NET_CLS_CGROUP` and
`CONFIG_NETPRIO_CGROUP` to stop `lxc-start` failing on `/sys/fs/cgroup/net_cls//lxc/android`
(`debug-android-userspace.rst:19-27`).

### 5.2 Kernel version constraints

- Halium requires ≥ 3.10 because systemd ≥ v217 requires it; "Some Halium distributions may use a
  kernel as old as 3.4, such as Ubuntu Touch" (`halium-docs/porting/first-steps.rst:26-29`).
- Kernel 3.4 + systemd ≥ 233 needs a one-line kernel patch (a `fstat` return-value bug that breaks
  `tmpmnt` creation) — `build-sources.rst:77-80`.
- Kernel 3.10 devices need three `SECURITY_ANDROID_GID_CAPABILITIES` commits or `ping` fails with
  `socket: Permission denied` (`halium-docs/porting/debug-build/wifi.rst:69-81`) — this is the
  interaction between `CONFIG_ANDROID_PARANOID_NETWORK` being off and Android's gid-based network
  gating.
- Device minimums otherwise: ≥ 1 GB RAM (2 GB recommended), ≥ 16 GB storage
  (`first-steps.rst:30-33`).

### 5.3 UBports standalone-kernel build

`build-kernel.sh` is the whole story (76 lines):

```
make O=$OUT $MAKEOPTS $deviceinfo_kernel_defconfig
[make O=$OUT $MAKEOPTS menuconfig]         # only with -m
make O=$OUT $MAKEOPTS -j$(nproc --all)
[make O=$OUT INSTALL_MOD_STRIP=1 INSTALL_MOD_PATH=$TMP/system modules_install]
```
Toolchain selection: `CLANG_TRIPLE` is set only if the kernel Makefile still mentions it, else
`CROSS_COMPILE=<arch>-linux-gnu-` (lines 23-30); default `CROSS_COMPILE=<arch>-linux-android-`;
arm64 also exports `CROSS_COMPILE_ARM32=arm-linux-androideabi-` (lines 32-38);
`deviceinfo_kernel_llvm_compile` adds `LLVM=1 LLVM_IAS=1` and
`HOSTLDFLAGS="-fuse-ld=lld --rtlib=compiler-rt"` (lines 46-50).

Two things stand out as good practice worth importing:

- **PATH hermeticity for LLVM builds.** `build.sh:59-84` restricts `PATH` to
  `ALLOWED_HOST_TOOLS="bash git perl sh sync tar yes"` symlinked into a scratch dir, plus AOSP
  `prebuilts/build-tools` and `kernel/prebuilts/build-tools`, plus toybox shims for
  `dd expr nproc tr`, "to make builds less susceptible to host differences". This is a hand-rolled
  approximation of what Nix gives for free.
- **Overlaystore module handling.** When `deviceinfo_use_overlaystore` is set, the build touches
  `${INSTALL_MOD_PATH}/lib/modules/.halium-override-dir` so the whole modules directory is
  bind-mounted from the device overlay and "rootfs won't ship any device-specific kernel module"
  (`build-kernel.sh:72-76`). That is precisely the separation Mura needs between a generic
  rootfs closure and a per-device kernel/module artifact.

Kernel↔system.img coupling is a documented hazard: module signing must match, or `insmod` fails with
`Required key is not found` / `Invalid module format`, and both `hybris-boot` and `system.img` must be
rebuilt together (`halium-docs/porting/debug-build/wifi.rst:31-33`).

### 5.4 Waydroid: what the host kernel must provide

- **Binder, three nodes.** `waydroid/tools/helpers/drivers.py:14-31` recognizes four naming
  conventions per node (`binder`/`bonder`/`puddlejumper`/`anbox-binder`, and `vnd*`/`hw*` variants).
  If nodes are missing, `probeBinderDriver()` runs `modprobe binder_linux devices="..."`, mounts
  `-t binder binder /dev/binderfs`, and allocates nodes via the `BINDER_CTL_ADD` ioctl
  (`drivers.py:43-109`). On a `HALIUM_*` host it refuses to allocate and requires the pre-existing
  nodes (`drivers.py:147-167` searches `BINDER_DRIVERS[:-1]`, i.e. excludes plain `binder`).
- **ashmem or memfd.** `probeAshmemDriver()` does `modprobe -q ashmem_linux`
  (`drivers.py:111-119`); if `/dev/ashmem` still doesn't exist, the container is told
  `sys.use_memfd=true` (`lxc.py:261-262`).
- **Device nodes.** `generate_nodes_lxc_config()` (`lxc.py:40-127`) bind-mounts a long explicit list:
  `/dev/{zero,null,full,ashmem,fuse,ion,tty,char,uhid,pmsg0,sw_sync,net/tun}`, GPU nodes
  `/dev/{kgsl-3d0,mali0,pvr_sync,dxg}` and the DRI render node, globs `/dev/fb*`,
  `/dev/graphics/fb*`, `/dev/video*`, `/dev/dma_heap/*`, plus `/sys/kernel/debug` (rbind),
  `/sys/module/lowmemorykiller`, `/sys/class/leds/vibrator`,
  `/sys/devices/virtual/timed_output/vibrator`, and MediaTek codec nodes.
- **LSM/seccomp.** apparmor profile `lxc-waydroid` if apparmor is active, else `unconfined`
  (`lxc.py:129-141, 172-174`); seccomp profile at
  `/var/lib/waydroid/lxc/waydroid/waydroid.seccomp`; `lxc.seccomp.allow_nesting = 1` on LXC ≥ 4
  (`waydroid/data/configs/config_4`); a large `lxc.cap.keep` list including `sys_admin`,
  `sys_ptrace`, `wake_alarm`, `block_suspend`, `sys_time`, `net_admin`, `net_raw`, `mknod`, `syslog`
  (`waydroid/data/configs/config_base:8`).

---

## 6. Image assembly and flashing

### 6.1 Halium classic

Deployables (`halium-docs/Distribution.rst:22-33`): `boot.img`, rootfs, udev rules, LXC container
configuration, `system.img`, `vendor.img` (newer devices), Android ramdisk.

The boot flow is deliberately trivial (`Distribution.rst:38-52`): the initramfs mounts the data
partition, loop-mounts `/data/rootfs.img` at `/target`, then
`exec switch_root /target $INIT --log-target=kmsg &> /target/init-stderrout`.

Full startup sequence (`Distribution.rst:128-141`):
fastboot/bootloader → initrd mounts userdata + `rootfs.img` → systemd from the rootfs → rootfs mounts
`/system`, `/vendor` and other Android mountpoints **before `local-fs.target`** → LXC `android`
container starts → the pre-start hook bind-mounts the mounted Android partitions inside the Android
rootfs → host udev and daemons start → "At this point the scope of halium is over".

The Android container's contents and configuration: `lxc.rootfs`, `lxc.network.type`,
`lxc.devttydir`, `lxc.tty`, `lxc.pts`, `lxc.arch`, `lxc.cap.drop`, `lxc.pivotdir` (deprecated),
`lxc.hook.pre-start`, `lxc.init_cmd`, `lxc.aa_profile`, `lxc.autodev`, sourced from
`Halium/lxc-android` at `var/lib/lxc/android/` (`Distribution.rst:86-112`). The container's root
filesystem is the *Android boot ramdisk*, extracted by the pre-start hook from
`/system/boot/android-ramdisk.img` (`Distribution.rst:118-124`) — i.e. the container runs Android's
`init` (`lxc.init_cmd`) against the Android initramfs, with `/system` and `/vendor` bind-mounted in.

What is *not* in the container: "Android is also patched to remove zygote, surfaceflinger, bootanim
and other processes, that are not needed when running a regular Linux distribution or would cause
problems, from Android init" (`libhybris/README.md:14-18`). Combined with `Scope.rst:6-12`, the
surviving Android userspace is: `init`, `servicemanager`/`hwservicemanager`, `rild`, the HAL service
daemons (`android.hardware.*@x.y-service`), `logd`, and in newer generations `apexd` and
`android.system.suspend` (the latter two visible in droid-hal-device's init-file handling,
`droid-hal-device.inc:702-708`).

Flashing: `fastboot flash boot hybris-boot.img`, or Samsung "Download Mode" + `heimdall flash --BOOT`
(the docs warn explicitly against using downloaded PIT files) —
`halium-docs/porting/install-build/reference-rootfs.rst:8-48`. Rootfs+system install via
`halium-install -p halium <rootfs.tar.gz> <system.img>` from recovery
(`reference-rootfs.rst:50-59`). The reference rootfs itself is Ubuntu 16.04 + systemd + lxc +
lxc-android + libhybris + ofono, built with live-build/`rootfs-builder`
(`Distribution.rst:5-17`); minimum requirements for any rootfs are "systemd as the main init" and
"lxc" (`Distribution.rst:59-63`).

### 6.2 UBports standalone-kernel: boot images, then OTA-shaped system images

`make-bootimage.sh` (349 lines) is the most detailed mkbootimg wrapper in the ecosystem. Behaviour by
`deviceinfo_bootimg_header_version`:

- **v0-v2**: single `boot.img` with `--base/--kernel_offset/--ramdisk_offset/--second_offset/
  --tags_offset/--pagesize`; v0 may take `--dt`, v2 takes `--dtb --dtb_offset` (lines 256-275,
  289-290).
- **v3-v4**: kernel and ramdisk split; `vendor_boot.img` carries offsets and DTB; v4 uses
  `--ramdisk_type platform --ramdisk_name '' --vendor_ramdisk_fragment` while v3 uses
  `--vendor_ramdisk`; v4 can take `--vendor_bootconfig` (lines 258-266, 300-315).
- **`init_boot`** (Android 13+ launch devices): ramdisk goes into a separate image
  (lines 292-295).
- **recovery**: either a separate `recovery.img` (lines 331-348), or appended as a second cpio
  segment onto the boot ramdisk when there is no recovery partition (lines 143-147), or as a
  `--ramdisk_type recovery` vendor ramdisk fragment (lines 308-310).
- **AVB**: `avb_add_hash_footer()` either appends the literal string `SEANDROIDENFORCE` (when
  `deviceinfo_bootimg_tailtype=SEAndroid`) or runs `avbtool add_hash_footer --image … --partition_name
  … --partition_size …`, optionally with `--key rsa4096_<part>.pem --algorithm SHA256_RSA4096`
  (lines 52-64); `append_vbmeta_image` against a downloaded Google GSI `vbmeta.img` is also supported
  (lines 322-324, `setup_repositories.sh:193-199`).
- Recovery `prop.default` is *synthesized* from `deviceinfo` under `fakeroot`
  (lines 76-102): `ro.product.{brand,device,manufacturer,model,name}`,
  `ro.boot.dynamic_partitions`, `ro.build.ab_update` (detected by grepping the recovery fstab for
  `slotselect`), `ro.recovery.usb.{vid,adb.pid,fastboot.pid}`.

The deliverable of the `build` stage is **not** an image: it is
`out/device_<codename>.tar.xz` containing only `partitions/` and `system/`, tarred with
`--owner=root --group=root`, plus `out/device_<codename>.tar.build` containing
`date --utc '+%Y%m%d-%H%M%SZ'` (`build-tarball-mainline.sh:76-84`). Hardlinked compatibility aliases
`device_<codename>_usrmerge.tar.{xz,build}` are created for old pipelines (`build.sh:117-119`).

Image *assembly* is a separate deploy stage that reuses Ubuntu Touch's OTA format. Both
`fetch-and-prepare-latest-ota.sh` (real channel) and `prepare-fake-ota.sh` (CI artifacts) emit an
`ubuntu_command` script:

```
format system
load_keyring image-master.tar.xz image-master.tar.xz.asc
load_keyring image-signing.tar.xz image-signing.tar.xz.asc
mount system
update <tarball> <tarball>.asc      # rootfs, halium GSI, device tarball, version
unmount system
```
(`fetch-and-prepare-latest-ota.sh:29-46`; `prepare-fake-ota.sh:121-208`.) `system-image-from-ota.sh`
interprets that script: `format system` truncates `rootfs.img` to
`${deviceinfo_system_partition_size:-3584M}` and `mkfs.ext4`s it — then, notably, disables the
`orphan_file` feature via `tune2fs -O '^orphan_file'` because host e2fsprogs ≥ 1.47 produces
filesystems that UBports recovery's e2fsck 1.45 cannot check, breaking 20.04 OTAs (lines 179-191);
`mount system` loop-mounts it (with a `mknod` fallback for containers lacking loop devices, lines
211-229); each `update` xz-extracts a tarball over the mount, honouring a `removed` manifest for
delta updates (lines 252-298); `unmount system` runs `img2simg` to produce a fastboot-flashable
`system.img` (line 243). `verify_signature()` is stubbed to `return 0` (lines 58-60) — signatures are
structurally present but unchecked in this tool.

CI artifacts: `out/{boot.img,init_boot.img,vendor_boot.img,system.img,dtbo.img}` for the manual
`flashable` job; `out/{*boot.img,dtbo.img,recovery.img,ubuntu.img.zst}` for `devel-flashable`
(`gsi-port-ci.yml:53-85`). The devel path also patches the rootfs to auto-start sshd with
`PasswordAuthentication=yes -o PermitEmptyPasswords=yes`, enable usb-tethering, and set
`ADBD_SECURE=0` (`prepare-fake-ota.sh:143-154`) — a debug-only image variant, cleanly separated.

### 6.3 droid-hal-device: images via subpackages + `mic`

Images are RPM payload. `droid-hal-device.inc` puts `hybris-boot.img`, `hybris-recovery.img`,
`vendor_boot.img`, `hybris-init_boot.img`, `dtbo.img`, `dt.img`/`dtb.img`, the bootloader binary, and
`kernel-$kernel_release` + `boot-initramfs.gz` into `/boot` or `/img` inside dedicated subpackages
(lines 1018-1097, 1279-1335). `kernel_release` is read from
`out/target/product/%{device}/*/*/include/config/kernel.release` (line 1018). Kernel modules are
harvested from six possible Android output locations — `system/lib/modules`,
`system/vendor/lib/modules`, `system_dlkm`, `vendor/lib/modules`, `vendor_dlkm`, plus
`vendor_ramdisk/lib/modules` for the vendor_boot case — and flattened into
`/lib/modules/$kernel_release` (lines 1059-1081).

The final OS image is produced by `mic create fs` / `mic create loop` from kickstart files in
`hybris/droid-configs/installroot/usr/share/kickstarts`
(`droid-hal-device/helpers/build_packages.sh:354, 413-420`).

An `installable_zip` variant packages Android recovery updater bits (`update-binary`,
`hybris-updater-script`, `hybris-updater-unpack.sh`) so the adaptation can be flashed as a recovery
zip (`droid-hal-device.inc:663-667, 1290-1294`).

### 6.4 Waydroid

No image assembly. `system.img` is loop-mounted at `/var/lib/waydroid/rootfs`, `vendor.img` at
`rootfs/vendor`, each optionally with an **overlayfs** on top (lower = `/var/lib/waydroid/overlay`,
upper = `overlay_rw`, work = `overlay_work`), with automatic fallback to disabling overlays if
`mount_overlay` raises (`waydroid/tools/helpers/images.py:162-201`). A per-session
`waydroid.prop` is bind-mounted over `rootfs/vendor/waydroid.prop` (lines 199-201). Session mounts
bind the host Wayland socket to `/run/xdg/wayland-0` and PulseAudio's `native` socket into the
container (`lxc.py:186-220`).

---

## 7. Update mechanism

| Project | Mechanism | Evidence |
|---|---|---|
| Halium | None of its own; explicitly delegated. "Deployment tools" are reference-implementation-only (`halium-install`). Downstreams provide OTA: "Ubuntu Touch … provides Android images as part of its OTA update model" | `halium-docs/project/Scope.rst:14,18-31` |
| UBports | Ubuntu Touch system-image OTA. Channel + device are baked into the image as `/etc/system-image/channel.ini` (`channel: 24.04-2.x/arm64/android9plus/daily`, `device: <codename>`) in a "version tarball" | `prepare-fake-ota.sh:181-205`; `fetch-and-prepare-latest-ota.sh:19-22` |
| UBports (delta) | `update` entries can carry a `removed` manifest processed before extraction, so OTAs are delta-capable over the same system partition | `system-image-from-ota.sh:270-279` |
| droid-hal | RPM/zypper, with an explicit "flash-on-upgrade" hook: `%post img-boot` etc. register `add-preinit-oneshot /var/lib/platform-updates/flash-bootimg.sh` guarded by `if [ $1 -ne 1 ]` (upgrade only, not first install). Same pattern for `aboot`, `dtbo`, `init_boot`, `vendor_boot` | `droid-hal-device.inc:1127-1178` |
| droid-hal (identity) | `hw-release` (an os-release analogue) records `MER_HA_DEVICE`, `MER_HA_VENDOR`, `MER_HA_VERSION`, `MER_HA_VERSION_ID`, `MER_HA_SAILFISH_BUILD`, `MER_HA_SAILFISH_FLAVOUR`; shipped both on-device and as `hw-release.vars` in `-devel` for build-time consumption | `droid-hal-device.inc:586-599, 967` |
| Waydroid | Dual OTA channels with sha256 pinning and monotonic `datetime` gating; the in-container `IHardware.upgrade()` call stops the container, replaces images, remounts, and restarts | `tools/helpers/images.py:25-121`; `tools/services/hardware_manager.py:33-41` |
| Waydroid (validation) | `validate()` re-fetches the channel manifest and requires the local zip's sha256 to appear in it, warning and refusing otherwise | `tools/helpers/images.py:84-120` |

Key structural observation: **only droid-hal-device treats bootloader-adjacent partitions as
package-managed, updatable artifacts.** Everything else treats `boot.img` as a manual flash step.
For an XR headset fleet this matters: Mura will need a package→partition flashing hook with
exactly droid-hal's "upgrade only, deferred to pre-init oneshot" semantics, or A/B slots.

---

## 8. Reproducibility properties

Assessed against what a Nix build would require (fixed-output inputs, no network in build, no
timestamps, no floating refs).

### Serious problems

- **Floating branch clones everywhere.** `clone_if_not_existing()` does
  `git clone "$url" -b "$branch" … --depth 1 --recursive`
  (`halium-generic-adaptation-build-tools/setup_repositories.sh:17-31`) — branches, never commits,
  and shallow so history can't pin retroactively. This applies to the *device kernel itself*
  (`deviceinfo_kernel_source_branch`, `setup_repositories.sh:204-212`).
- **`lastSuccessfulBuild` artifact URLs.** The recovery ramdisk
  (`setup_repositories.sh:263`), the Ubuntu Touch rootfs, and the Halium GSI tarball
  (`prepare-fake-ota.sh:28-85`) are all fetched from Jenkins `lastSuccessfulBuild/artifact/…`
  URLs. There is no hash, no version, and the content changes under you.
- **GitHub `continuous` release tag** for the boot ramdisk
  (`halium-boot/get-initrd.sh:18`; `deviceinfo.sample:197`; default `dynparts` tag at
  `setup_repositories.sh:232`) — a mutable tag.
- **Timestamps in outputs.** `date --utc '+%Y%m%d-%H%M%SZ' > $output_name.tar.build`
  (`build-tarball-mainline.sh:84`); recovery `prop.default` gets
  `ro.build.version.incremental=ci.ubports.$(date …)` (`make-bootimage.sh:94`); droid-hal's local
  `Release: %(date +'%Y%m%d%H%M')` (`droid-hal-device.inc:198-199`).
- **Host toolchain from `apt` at build time** in CI (`gsi-port-ci.yml:23-29`), including
  `python2` and `libtinfo5`; `make-dtboimage.sh:38` and `make-bootimage.sh:231` invoke
  `python2 mkdtboimg.py` unconditionally.
- **Root/loop/sudo in the image path.** `system-image-from-ota.sh` uses `sudo losetup`, `sudo mknod`,
  `sudo mount`, `sudo sh -c "xzcat … | tar -xf -"` (lines 215-292). Unsandboxable as-is.
- **Signature verification stubbed.** `verify_signature() { return 0; }` and
  `check_filesystem() { return 0; }` (`system-image-from-ota.sh:53-60`); `.asc` files are created with
  `touch` in the fake-OTA path (`prepare-fake-ota.sh:162, 172, 178, 204`).
- **droid-hal builds Android inside an RPM `%build` inside an `ubu-chroot`, after a repo tarball is
  unpacked by an OBS service** (`droid-hal-device.inc:392-424, 463`), with a
  "dummy tarball for rpm checks" hack and `ln -s ../SOURCES rpm`. This is the opposite of hermetic.
- **libhybris configuration is a function of *which Android tree the headers came from*.** The
  `android-headers` package carries `ANDROID_VERSION_{MAJOR,MINOR,PATCH}` and an `android-config.h`
  whose content is device-specific (see §9.2). Two devices on the same Android version can require
  different libhybris builds (`--enable-adreno-quirks`, `MALI_QUIRKS`, `QCOM_BSP`).
- **Waydroid is entirely runtime-resolved**: binder node names, HAL presence, GPU driver, and
  therefore the container's property set are computed on the running host each `waydroid init`
  (`tools/actions/initializer.py:43-128`).

### Genuinely good properties worth keeping

- **Content-addressed image distribution.** Waydroid validates sha256 against a channel manifest and
  refuses mismatches (`tools/helpers/images.py:41-46, 84-97`). This is the right primitive for pinned
  donor firmware; Nix's `fetchurl` with `sha256` is the same idea with better ergonomics.
- **Declarative single-file device config.** `deviceinfo` is a flat shell-sourced key/value file with
  a documented sample; converting it to a Nix attrset is nearly mechanical.
- **PATH restriction for hermeticity** (`build.sh:59-84`), as discussed in §5.3.
- **Pinned AOSP prebuilt *branches* per Android generation** — the mapping
  halium-version → clang branch/revision (`setup_repositories.sh:99-134`) and
  → build-tools branch (lines 171-191) is explicit table-driven data, not guesswork. Table:
  9→`pie-gsi`/`4691093`, 10→`android10-gsi`/`r353983c`, 11→`android11-gsi`/`r383902`,
  12→`android12L-gsi`/`r416183b`, 13→`master-kernel-build-2022`/`r450784e`,
  14→`main-kernel-build-2023`/`r487747c`, 15|16→`main-kernel-build-2024`/`r510928`.
- **Separation of generic rootfs from per-device overlay** (`build-kernel.sh:72-76`,
  `build-tarball-mainline.sh:67-74`) — the "overlaystore" design means the rootfs closure is
  device-independent and a small overlay carries the device delta. This maps well onto a Nix
  `system` closure + device-specific overlay derivation.
- **`%define __requires_exclude` / `__provides_exclude_from` / `__strip /bin/true`**
  (`droid-hal-device.inc:118-119, 168-172`) — the exact set of packaging escape hatches needed for a
  blob tree.

---

## 9. What Mura should adopt

### 9.1 Adopt `deviceinfo` as the device declaration schema — as a typed Nix module

`deviceinfo.sample` is, in effect, a complete specification of "what a port declares" for a
standalone-kernel Android-donor device: identity, arch, Android generation, kernel source+defconfig,
cmdline (both boot and vendor_boot), boot-image header version and offsets, AVB policy, DTB/DTBO
policy, ramdisk compression, partition sizes. Turning this into a NixOS-module-style options set gets
type checking, defaults, and documentation for free, and the existing 40+ UBports community ports
become a corpus of real values to validate against.

Specific fields that must survive translation because they encode hardware truth that cannot be
derived: `deviceinfo_bootimg_header_version`, the six `flash_offset_*` values,
`deviceinfo_bootimg_tailtype`, `deviceinfo_bootimg_has_init_boot_partition`,
`deviceinfo_bootimg_has_vendor_kernel_boot_partition`, `deviceinfo_kernel_image_name`,
`deviceinfo_ramdisk_compression`. All of these are extractable from a donor `boot.img` with
`unpack_bootimg.py` (`deviceinfo.sample:113`) — so Mura can generate a first draft of a device
module *from the pinned donor image*, which is strictly better than UBports' manual transcription.

### 9.2 Adopt the two-artifact model: `android-headers` + `android-config.h` as a derivation

libhybris cannot be built generically. `configure.ac:183-184` hard-requires `android-config.h` and
`android-version.h`; `configure.ac:213-262` parses `ANDROID_VERSION_{MAJOR,MINOR,PATCH}` out of the
headers with an awk-over-cpp hack and derives ~12 `HAS_ANDROID_x_y_z` conditionals from them;
`configure.ac:165-197` toggles whole subsystems on *header presence*
(`libnfc-nxp/phLibNfc.h`, `hardware_legacy/wifi.h`, `hardware/hwcomposer2.h`,
`hardware/gralloc1.h`, `hardware_legacy/vibrator.h`), and `configure.ac:355-363` builds the Vulkan
wrapper only if `vulkan/vulkan.h` exists.

The extractor is `libhybris/utils/extract-headers.sh` (388 lines), duplicated as
`droid-hal-device/helpers/extract-headers.sh`. It takes `<ANDROID_ROOT> <HEADER_PATH>`, auto-detects
the version from `build/core/version_defaults.mk`, and copies a specific list of header directories:
`hardware/libhardware/include/hardware`, `system/core/include/{cutils,log,system,android,private}`,
`system/media/audio/include/system`, `bionic/libc/kernel/uapi/linux/{sync,sw_sync,sync_file}.h`,
`bionic/libc/private`, `frameworks/native/libs/{nativewindow,arect,nativebase}/…`, and the
`vndk/window.h` + `apex/window.h` variants (lines 191-292). It emits `android-version.h`,
a templated `android-config.h` containing the literal marker `/* CONFIG GOES HERE */`, an
`android-headers.pc`, and — importantly — `git-revisions.txt` plus a copy of `.repo/manifest.xml`
"in order to make it easier to trace back the origins of headers" (lines 294-348).

droid-hal-device then *fills in* that marker (`droid-hal-device.inc:824-867`) with:
the per-device `%{android_config}` macro; auto-detected `#define QCOM_BSP 1` / `#define QTI_BSP 1`
when `TARGET_USES_QCOM_BSP := true` or `BOARD_USES_QCOM_HARDWARE := true` appears in the device
`BoardConfig*.mk`; and Android-generation fixups — `#define DISABLED_FOR_HYBRIS_SUPPORT` (≥15),
`#define __BIONIC_VERSIONER` + `#define _Nonnull` + `#define _Nullable` +
`#include <android/versioning.h>` (≥13), `#include <android/api-level.h>` (≥15).

**Adopt this verbatim as a Nix derivation**: `android-headers-<generation>-<device>` with the
donor/AOSP tree as a fixed-output input, the config defines as a Nix attrset, and the
`git-revisions.txt`/manifest provenance retained. Then libhybris becomes a normal function of that
derivation, and per-device libhybris variants (`--enable-adreno-quirks`, `--enable-mali-quirks`,
`--with-default-egl-platform`, `--with-default-hybris-ld-library-path`) are just module options.

### 9.3 Adopt libhybris' runtime-pluggable linker and EGL-platform layout

Both plugin systems are directory-based with an environment override, which is ideal for Nix:

- Linkers: one shared object per Android generation installed to
  `LINKER_PLUGIN_DIR = $(libdir)/libhybris/linker`
  (`libhybris/hybris/common/Makefile.am:53-57`), named `jb.so`, `mm.so`, `n.so`, `o.so`, `q.so`
  (`hooks.c:3430-3434`), built conditionally: `jb` only for non-arm64/non-x86-64, `mm` at Android ≥6,
  `n` ≥7, `o` ≥8, `q` ≥10 (`common/Makefile.am:6-26` and the matching `-DWANT_LINKER_*` flags on
  lines 75-92). Selection at runtime reads `ro.build.version.sdk` via `my_property_get`
  (`hooks.c:3458-3478`) and picks: sdk ≤29→`q`, ≤27→`o`, ≤25→`n`, ≤23→`mm`, <21→`jb`
  (`hooks.c:3642-3661`), overridable with `HYBRIS_ANDROID_SDK_VERSION` (line 3495) and
  `HYBRIS_LINKER_DIR` (line 3668, guarded by `getauxval(AT_SECURE)` so setuid processes can't be
  hijacked). The symbol-hook tables are layered the same way: `hooks_p` for sdk >27, `hooks_n` >23,
  `hooks_mm` >21, `hooks_properties` only when sdk <27 (because Android ≥8 reads properties through
  bionic `libc.so` directly), then `hooks_common` (`hooks.c:3539-3571`).
- EGL platforms: `eglplatform_<name>.so` loaded lazily from `PKGLIBDIR`, overridable with
  `HYBRIS_EGLPLATFORM_DIR`, again `AT_SECURE`-guarded (`libhybris/hybris/egl/ws.c:51-90`).
  Available platforms: `null`, `fbdev`, `hwcomposer` (Android ≥4.2), `wayland` (with
  `--enable-wayland`) — `libhybris/hybris/egl/platforms/Makefile.am`. Compile-time default via
  `--with-default-egl-platform` (`configure.ac:158-163`), runtime via `EGL_PLATFORM`
  (used throughout the docs, e.g. `EGL_PLATFORM=hwcomposer test_hwcomposer`,
  `halium-docs/porting/debug-build/graphics.rst:14`).

Adopt: a `libhybris` package whose linker set and EGL-platform set are Nix-selectable, with
`HYBRIS_LINKER_DIR`/`HYBRIS_EGLPLATFORM_DIR`/`HYBRIS_LD_LIBRARY_PATH` set by a wrapper rather than
baked in. Note that the correct default `HYBRIS_LD_LIBRARY_PATH` is version-dependent: Android ≥7
wants `/system/lib64:/odm/lib64:/vendor/lib64` (system first), older wants
`/vendor/lib64:/system/lib64:/odm/lib64` (vendor first) — `configure.ac:264-281`.

### 9.4 Adopt the "overlaystore" separation of generic closure vs. device delta

`build-kernel.sh:72-76` (`.halium-override-dir` marker) plus `build-tarball-mainline.sh:67-74`
(everything under `system/` relocated into `system/opt/halium-overlay/`) implement: *generic rootfs
never contains device-specific files; a per-device overlay is bind-mounted over it at boot.* This is
the same shape as a NixOS system closure plus a device-specific derivation, and it is what makes one
rootfs artifact serve many devices. Adopt it, but implement the overlay as a proper Nix derivation
rather than a tarball.

### 9.5 Adopt droid-hal-device's generators as build-time, pure transformations

Three mechanical conversions from donor Android metadata are implemented in perl/C and are trivially
portable to Nix build steps:

- `helpers/makeudev` — `ueventd*.rc` → udev rules, including `/sys` rules (emitting
  `DEVPATH==…, TEST==…, RUN+="/bin/chmod …", RUN+="/bin/chown …"`) and a
  subsystem-symlink table for `graphics`, `block`, `dri`/`drm`, `oncrpc`, `adsp`, `msm_camera`,
  `mtd`, `dvb` (`makeudev:14-56`). Invoked over all non-goldfish `ueventd*.rc` in
  `out/.../{system,root,vendor}` (`droid-hal-device.inc:563-568`).
- `helpers/makefstab` — Android fstab and `init*.rc` → systemd `.mount` units with
  `Before=local-fs.target systemd-modules-load.service`, `WantedBy=local-fs.target`,
  `TimeoutSec=10`, and SELinux `context=` options stripped into a comment
  (`makefstab:28-60`). The call site passes a ~30-entry `--skip` list (`/acct`, `/data`,
  `/data_mirror/*`, `/sys/fs/{bpf,cgroup,pstore,fuse/connections}`, `/dev/{cpuctl,cpuset,stune}`,
  `/dev/usb-ffs/*`, …) extensible per device via `%{makefstab_skip_entries}`
  (`droid-hal-device.inc:581`), then post-processes with
  `hybris/hybris-boot/fixup-mountpoints %{device} tmp/units/*` (line 583).
- `helpers/usergroupgen.c` → `droid.ids`, and a policy file
  `%{_prefix}/lib/droid/hw-group.d/default.list` listing the groups a user needs for hardware access:
  `audio bluetooth camera graphics input media media_rw mtp`, with `system` in `default.list.remove`,
  plus conditional `inet.list` when the kernel config actually has
  `CONFIG_ANDROID_PARANOID_NETWORK=y` (`droid-hal-device.inc:969-1014`).

For Mura these three become: `donorUdevRules`, `donorMountUnits`, `donorGroupPolicy` — pure
functions of the pinned donor image. This directly removes the two most error-prone manual steps in
the Halium workflow (§3.1).

### 9.6 Adopt per-subsystem backend selection — the decision framework

The evidence supports a per-subsystem matrix rather than a single global choice. Halium itself
classifies interfaces into three tiers (`halium-docs/project/Scope.rst:6-12`) and explicitly wants to
minimize tier 3. Waydroid's `find_hal`/`find_hidl`/`find_aidl` probing (`lxc.py:223-257`) plus its
mainline fallbacks (`lxc.py:271-320`, `gpu.py:37-60`) enumerate exactly where a mainline path exists.

| Subsystem | Mainline / native backend | Android-compat backend | What the compat choice costs |
|---|---|---|---|
| **Display + GPU** | Mesa + `msm`/`msm_dpu` KMS (Freedreno; `gpu.py:43-44` maps `msm`→`freedreno`) | libhybris `libEGL` + `eglplatform_hwcomposer.so` + `libhwc2_compat_layer.so` | Needs `hardware/hwcomposer2.h` in android-headers (`configure.ac:170`); needs the *bionic* half built in an Android tree with HIDL composer 2.1–2.4 or AIDL composer3 per version (`compat/hwc2/Android.mk:75-129`); Android-side deps include `libhidlbase`, `libhwbinder`, `libgui`, `libui`, `libfmq`, `android.hardware.graphics.{allocator@2.0,composer@2.1..2.4}`, `libbinder_ndk`+`composer3-V{1,4}-ndk` at ≥13/≥14. **Vulkan has no hwcomposer WSI** in libhybris — only `null` and `wayland` platforms exist (`hybris/vulkan/platforms/`), so a Vulkan-based XR compositor on the hwcomposer path is unproven. Also: CAF-vs-AOSP mismatch produces `EGL_BAD_SURFACE`/`IOCTL_KGSL_TIMESTAMP_EVENT` failures requiring a differently-built userspace (`graphics.rst:34-53`). |
| **Buffer allocation** | GBM on the DRI render node (Waydroid: `gralloc=gbm`, `egl=mesa`, `gralloc.gbm.device=<renderD*>` — `lxc.py:277-282`) | libhybris `hybris_gralloc_*`, auto-detecting gralloc0 module / gralloc1 (`HAS_GRALLOC1_HEADER`) / "version 2" via `graphic_buffer_allocator_allocate` in `hybris/ui` at Android ≥10 (`hybris/gralloc/gralloc.c:104-120`) | Three distinct ABIs to support; the ≥10 path additionally requires `libhybris/hybris/ui`. `GrallocUsageConversion.cpp` is needed for gralloc1 usage-bit translation. |
| **Camera** | V4L2 (Waydroid sets `ro.hardware.camera=v4l2` on MAINLINE — `lxc.py:313-320`) | Halium tier 3: `compat/camera/camera_compatibility_layer.cpp` + `compat/media/` + "deep modifications to Android's libmedia" (`Scope.rst:10` and footnote 33) | The single worst case in the ecosystem. Halium says it wants to reduce this class "as much as possible". Sailfish routes it through separately-packaged `droidmedia` (`droid-hal-device.inc:801-806`, `helpers/droidmedia-localbuild.spec`). For XR (multiple global-shutter tracking cameras at high frame rate) this path is both the highest-effort and the least likely to expose the needed controls. |
| **Sensors / IMU** | Kernel IIO drivers | `hybris/hardware` (`hw_get_module`) + the sensors HAL, exercised by `hybris/tests/test_sensors.c` (`tests/Makefile.am:211-217`); Sailfish wraps it in `sensorfw-qt5-hybris` (`build_packages.sh:285-286`); Waydroid has an out-of-tree `waydroid-sensord` and stubs the HAL when absent (`images.py:151-152`) | There is **no** libhybris sensors *wrapper library* — only the generic `libhardware` shim. Every consumer writes its own. Sensor sample latency/timestamps through a HAL + container boundary is the critical unknown for XR head tracking. |
| **Audio** | ALSA/PipeWire | Audio HAL "dlopen'ed directly" (`Scope.rst:8`), i.e. via `hybris/hardware`; consumers are `pulseaudio-modules-droid` / `-droid-hidl` / `-droid-glue` / `-droid-jb2q` (`build_packages.sh:276-281`), with `audioflingerglue` needed only for the `-glue` variant | Four mutually exclusive PulseAudio module variants by generation; `-glue` additionally needs an Android-side `libaudioflingerglue.so` + `miniafservice`. No PipeWire module exists in any of these repos. |
| **Wi-Fi / BT** | mac80211 + BlueZ | Wi-Fi: a standard libhybris wrapper (`hybris/wifi`, gated on `hardware_legacy/wifi.h` — `configure.ac:169`). BT: not in libhybris at all; the documented route is binder-IPC from native code via `libgbinder`/`bluebinder` (`libhybris/README.md:53-59`) | Halium's kernel config *disables* every `CONFIG_BT_HCI*` backend (`check-kernel-config:228-234`), implying BT goes through the Android stack. Device-specific bring-up incantations are real: `echo 1 > /dev/wcnss_wlan; echo sta > /sys/module/wlan/parameters/fwpath` for Qualcomm, `insmod` of `wlan.ko`/`bcmdhd.ko` from `init.rc` (`wifi.rst:11-47`). |
| **DSP-based tracking (aDSP/cDSP, FastRPC)** | No evidence of any mainline path in any repo studied | No wrapper exists. The only DSP-adjacent artifact is the `adsp` subsystem entry in the udev symlink table (`makeudev:19`) and `/dev/adsp` nodes coming from `ueventd.rc` | **This is the biggest gap for Mura.** None of the seven projects wraps FastRPC/`libadsprpc`/`libcdsprpc`. Whatever exists must either be reached through libhybris `dlopen` of the vendor `.so` (plausible: it is a userspace library over an ioctl device, similar to the "Audio HAL dlopen'ed directly" case) or by running the vendor's DSP service daemon inside a container and talking to it over binder. Both need prototyping before any architecture is committed. |

### 9.7 Adopt Waydroid's mainline-first fallback table as *build-time* configuration

`lxc.py:259-370` encodes, in one place, every "if no vendor HAL then use the mainline thing" decision:
gralloc→gbm, egl→mesa, vulkan→(DRM-driver→driver-name map), camera→v4l2, sensors→stub,
ashmem absent→`sys.use_memfd=true`, `ro.vndk.lite=true` on MAINLINE. Adopt the *content* of this
table; reject the mechanism (runtime probing — see §10.4).

### 9.8 Adopt content-addressed donor pinning with provenance

Waydroid's sha256-against-manifest validation (`images.py:41-46, 84-97`) and
`extract-headers.sh`'s retention of `git-revisions.txt` + `.repo/manifest.xml` (lines 294-348) are the
two provenance mechanisms worth keeping. For Mura: every donor firmware image is a
`fetchurl`-style fixed-output derivation with a recorded hash, model, firmware version, and extraction
date; every derived artifact (headers, udev rules, mount units, blob closure) records which donor it
came from.

---

## 10. What Mura should reject

### 10.1 Reject the Android build tree as a build input

Halium classic requires a full `repo` tree (`halium-docs/porting/get-sources.rst:45-47`: "several
GBs"), a specific i386-multilib host environment
(`first-steps.rst:55-91` — `libncurses5-dev:i386`, `libgl1-mesa-glx:i386`, `g++-multilib`,
`mingw-w64-i686-dev`, …), and Python 2 in places. droid-hal-device goes further and runs that build
*inside an RPM inside an Ubuntu chroot on OBS* (`droid-hal-device.inc:463`). The maintenance surface
is enormous: `halium-docs/porting/common-kernel-build-errors.rst` and
`common-system-build-errors.rst` exist purely to catalogue its failure modes.

The UBports standalone-kernel method already proves the Android tree is unnecessary for the *port*:
kernel from a plain git repo + defconfig, Android userspace as a prebuilt artifact
(`build.sh`, `prepare-fake-ota.sh:56-90`). Mura should go one step further and take the Android
userspace from the *pinned donor image* rather than from a third-party GSI CI artifact. The one
thing that genuinely needs an Android tree is the *bionic half* of libhybris' compat layers
(`libhybris/compat/*/Android.mk`) — which should therefore be scoped to exactly the layers a chosen
backend requires, and ideally avoided by preferring donor-provided HAL services over compat layers.

### 10.2 Reject "Android-compat as the foundation"

Halium's own architecture makes the Android container a *hard boot dependency*: the rootfs must mount
`/system` and `/vendor` before `local-fs.target`, and the LXC container must start before udev and
other host daemons (`halium-docs/Distribution.rst:132-141`). Ubuntu Touch's kernel needs apparmor
patches that Sailfish does not (`Planning.rst:48`), i.e. the compat layer's requirements propagate all
the way into the kernel config. For Mura, where Android-compat is one selectable backend,
the container must be an *optional, late-starting, per-subsystem* unit, not a `local-fs.target`
prerequisite. Concretely: no `switch_root`-from-Android-initramfs boot design
(`Distribution.rst:38-52`), no Android `init` as PID 1 of a mandatory container, and no Android
mountpoints in the critical boot path.

### 10.3 Reject floating refs, CI-artifact URLs, and tarball-shaped artifacts

Every input identified in §8 as a floating ref must be replaced by a pinned, hashed input:
`deviceinfo_kernel_source_branch` → commit; `initramfs-tools-halium` `continuous`/`dynparts` releases
→ built from source or pinned by hash; `lastSuccessfulBuild/artifact/*.tar.xz`
(`prepare-fake-ota.sh:54-85`, `setup_repositories.sh:263`) → not used at all. Likewise reject the
`.tar.xz` + `ubuntu_command` artifact format (`build-tarball-mainline.sh:81-84`,
`system-image-from-ota.sh`): a tarball applied over a mounted ext4 image with `sudo tar -xf`, gated by
a stubbed `verify_signature()`, is strictly worse than a Nix closure.

### 10.4 Reject runtime probing as the source of configuration

Waydroid's entire configuration is derived by executing probes on the live host
(`initializer.py:43-128`, `lxc.py:223-257`, `drivers.py:67-109`, `gpu.py:20-60`) and writing the
results into a mutable `/var/lib/waydroid` state tree plus a generated `.prop` file. That is
diametrically opposed to a Nix build system: the same hardware can yield different containers on
different boots, and nothing is auditable. Keep the *knowledge*; move the *evaluation* to build time,
driven by the pinned donor image and the device module.

Relatedly, reject Waydroid's LXC-config-by-string-substitution
(`lxc.py:143-184`: `cat` snippets together, then `sed` `LXCARCH` and `LXCPOSTSTOP`, then `sed -E` the
apparmor profile name) and its version-conditional snippet selection (`config_base`, `config_1`,
`config_3`, `config_4` picked by `lxc-info --version`). If a container is needed, generate the full
config declaratively for a known LXC version.

### 10.5 Reject undifferentiated blob packaging

droid-hal-device ships the *entire* Android `/system/bin` and `/system/lib[64]` as one RPM payload
(`droid-hal-device.inc:722-730`) with dependency extraction disabled wholesale
(`__requires_exclude ^.*$`, line 118). It works, but it makes the adaptation a monolith: you cannot
select camera without also shipping the media stack, and the `-detritus`/`straggler_files` mechanism
(lines 328-335, 907-931) exists precisely because nobody knows what the extra files are. Mura
should instead derive a *per-subsystem* blob closure from the donor image (the HAL `.so`, its
`NEEDED` closure, its `.rc` and `*.xml` VINTF manifests, its `ueventd` entries), so that enabling the
camera backend pulls camera blobs and nothing else.

### 10.6 Reject the manual per-device steps

`fixup-mountpoints` (hand-transcribed `readlink -f` output per device —
`build-sources.rst:82-107`) and hand-generated udev rules (`udev.rst`) are manual, device-specific,
and duplicated across every distro. Both are mechanically derivable from the donor image (§9.5).
Similarly reject the Halium device-documentation model (`supplementary/devices/*.rst` prose tables):
device support status must be machine-readable and derived from the device modules themselves.

### 10.7 Reject apparmor-flavoured kernel policy as a default

`halium-boot/check-kernel-config` forces `CONFIG_DEFAULT_SECURITY="apparmor"`,
`SECURITY_APPARMOR_BOOTPARAM_VALUE=1`, `SECURITY_SELINUX_BOOTPARAM_VALUE=0`,
`SECURITY_APPARMOR_UNCONFINED_INIT`, and disables `DEFAULT_SECURITY_SELINUX` (lines 68-80, 214-268) —
a Ubuntu Touch policy choice, not a Halium requirement (`Planning.rst:48`). It also disables
`CONFIG_FRAMEBUFFER_CONSOLE` and `VT_HW_CONSOLE_BINDING` (lines 248-249), which is hostile to
bring-up on a headset with no serial console. Take the list as a *superset of candidate symbols* and
partition it into: (a) container/systemd prerequisites (adopt), (b) Android-HAL prerequisites
(adopt only when an Android backend is enabled), (c) distro security policy (decide independently).

---

## 11. Open questions

1. **DSP-based tracking has no precedent here.** No repo studied wraps FastRPC / `libadsprpc` /
   `libcdsprpc` / the SLPI sensor DSP. Is the vendor tracking stack on XR2-class parts reachable by
   `dlopen`ing the vendor `.so` under libhybris (like the audio HAL, `Scope.rst:8`), or does it require
   a vendor daemon plus binder (the post-Android-8 pattern noted in `libhybris/README.md:53-59`)?
   This single question probably determines whether Halium-style compat is viable for Mura at
   all. Needs a hardware spike.
2. **Vulkan through libhybris on a hwcomposer display path is unproven.** `hybris/vulkan/platforms/`
   contains only `common`, `null`, and `wayland` — there is no hwcomposer WSI, and the libvulkan
   wrapper is built only if `vulkan/vulkan.h` was extracted (`configure.ac:355-363`). An XR
   compositor wanting Vulkan + direct-to-display + explicit sync needs this path characterized, or
   needs Mesa/Freedreno instead.
3. **Sensor timestamp fidelity across the container boundary.** For head tracking, IMU sample
   timestamps must be in a clock domain the compositor can use with sub-millisecond confidence.
   Nothing in these repos measures that. Waydroid's `waydroid-sensord` and Sailfish's
   `sensorfw-qt5-hybris` are both "does it report values" quality, not "is it tight enough for
   reprojection".
4. **Which Halium generation applies per target?** `deviceinfo_halium_version` is documented as 9-12
   (`deviceinfo.sample:38-43`) but the scripts handle 9-16 (`setup_repositories.sh:99-134`,
   `prepare-fake-ota.sh:56-90`), while libhybris' newest linker is `q` (Android 10, SDK 29)
   (`hooks.c:3430-3434`) with no `r`/`s`/`t` linker. For Snapdragon 835 devices (Android 8/9 era)
   through XR2+ Gen 2 (Android 12/13 era), does the `q` linker actually load Android 12/13 bionic
   libraries correctly, and what do the `/apex` workarounds
   (`common/q/linker.cpp:121-153, 260-296, 2262-2266`) cover vs. miss?
5. **APEX handling for Android ≥13 donors.** droid-hal-device has three different strategies by
   generation (apex image extraction for 10-14, `system/lib64/bootstrap` for ≥15, plus shipping
   `linkerconfig` and empty `/apex`, `/bootstrap-apex` — `droid-hal-device.inc:730-792`). Which is
   correct for a *donor-image* workflow where no Android build output exists, and can the required
   bionic core libs be extracted from a stock `system.img` reliably?
6. **Droidian's actual packaging model is unknown.** The cloned meta-repo is a stub (see the caveat at
   the top). The specific questions worth answering from the real Droidian repos: what is the
   granularity of an `adaptation-<device>` package; how is the device tarball from
   `halium-generic-adaptation-build-tools` converted into `.deb`s; what does their `debos`/image
   recipe look like; and what exactly did they automate that Halium leaves manual (the README's claim
   of Android 8.1+/treble-only support implies a materially simpler adaptation than Halium classic —
   `droidian/README.md:9-11`).
7. **Does the donor's vendor partition need to be mounted read-only in place, or can it be a Nix
   store input?** UBports relies on the stock partition being present on the device (§4.2); Sailfish
   copies everything into RPM payload (§4.3). For a Nix closure the second is required, but it needs
   a lawful extraction path from a pinned `super.img`/`vendor.img` (dynamic partitions, possibly
   `erofs`, possibly AVB-protected), plus a decision about whether the donor's `vendor` is mounted at
   `/vendor` (breaking NixOS's FHS assumptions) or at a Nix path with `HYBRIS_LD_LIBRARY_PATH`
   pointing there (`configure.ac:277-281` allows the latter — this looks promising and should be
   tested early).
8. **VINTF / HIDL-vs-AIDL service manifests.** `compat/hwc2/Android.mk:107-129` shows the HAL
   interface ABI changing at Android 13 (AIDL `composer3-V1-ndk`) and 14 (`V4-ndk`). If Mura
   runs donor HAL services in a container, the VINTF matching logic (`hwservicemanager` /
   `servicemanager` + `vintf` manifests) must be satisfied or bypassed. No repo studied documents
   doing this outside a full Android userspace.
9. **A/B slots and boot-image flashing policy.** droid-hal's "flash on package upgrade via pre-init
   oneshot" (`droid-hal-device.inc:1127-1178`) is the only package-managed partition-update model
   here, and it is not slot-aware; `make-bootimage.sh:71-74` detects A/B by grepping the recovery
   fstab for `slotselect`. What does Mura do for boot/dtbo/vendor_boot updates on A/B devices,
   and can it avoid writing partitions from a package at all?
10. **Kernel provenance for XR targets.** The whole Halium/UBports model assumes a publishable
    vendor kernel source tree (`first-steps.rst:24-25` requires kernel source plus a LineageOS
    12.1/14.1-era base). Standalone VR headsets frequently ship GPL kernel source dumps of varying
    completeness and no device tree in mainline. Which of the intended targets have (a) buildable
    kernel source, (b) a matching defconfig, (c) a usable `boot.img` header description? Until that
    is enumerated per device, the choice between "Linux on the Android kernel via Halium" and
    "Android in a container on a mainline kernel via Waydroid-style inversion" cannot be made per
    target — and §9.6 suggests it may have to be made *per subsystem* per target anyway.
