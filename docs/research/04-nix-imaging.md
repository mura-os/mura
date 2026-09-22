# Nix image-building and foreign-build-system orchestration

**Research date:** 2026-09-22
**Scope:** code-level study of robotnix, nixos-generators (and its nixpkgs replacement), Jovian-NixOS, nixos-apple-silicon, and mkosi, as precedents for the spatial-os multi-device image build system.
**Path conventions:** file paths are relative to each repo root under `references/`; `nixpkgs:` paths are relative to a nixpkgs checkout (nixos-unstable, rev `35e2127`, the locally-registered flake). The consumer of this document is designing a new Nix-based multi-device build system; sections 9–11 distill the transferable patterns.

---

## 1. Project purpose

**robotnix** builds complete Android (AOSP) device images with Nix.

- Wraps the entire AOSP build — repo-manifest source assembly, soong/ninja, vendor blob extraction, AVB/APK/APEX signing, OTA generation — in a NixOS-style module system evaluated by `lib.evalModules` (`default.nix`).
- Currently maintained flavors: LineageOS and GrapheneOS (the `flavor` enum in `modules/base.nix` lists exactly those two; vanilla/anbox/waydroid are commented out of the module list in `default.nix`).
- Closest existing precedent for "Nix orchestrates a giant vendor build system and ingests vendor blobs," and also a cautionary tale: per its `README.md`, it was "unmaintained for three years" and "many components are still in disrepair."

**nixos-generators** was for eight years the standard front-end for turning one NixOS configuration into ~30 image formats (`README.md` format table).

- Since NixOS 25.05 it is deprecated: "most of nixos-generators has been upstreamed into nixpkgs" and `nixos-generate` is replaced by `nixos-rebuild build-image` (`README.md`; the deprecation warning is emitted from `format-module.nix`).
- Its README's format table is the authoritative old-name → new-variant migration map (e.g. `sd-aarch64` → `sd-card` with `system = aarch64-linux`; `install-iso` → `iso-installer`).

**nixpkgs image framework** (the replacement) consists of:

- `nixpkgs:nixos/modules/image/images.nix` — declares `image.modules` and `system.build.images`;
- `nixpkgs:nixos/modules/image/file-options.nix` — standard `image.baseName`/`extension`/`fileName`/`filePath` options;
- the systemd-repart builder — `nixpkgs:nixos/modules/image/repart.nix`, `repart-image.nix`, `repart-verity-store.nix`;
- older imperative builders in `nixpkgs:nixos/lib/` — `make-disk-image.nix`, `make-ext4-fs.nix`, `make-squashfs.nix`, `erofs-store-image.nix`, `make-iso9660-image.nix`, `make-system-tarball.nix`, plus the sd-image modules under `nixos/modules/installer/sd-card/`.

**Jovian-NixOS** makes "NixOS become SteamOS" on the Steam Deck.

- A NixOS module set plus a nixpkgs overlay reproduce SteamOS behavior (gamescope session, vendor kernel, forked Mesa RADV, firmware updaters, Steam client bootstrap) on top of a user's ordinary NixOS system.
- Ships no images and no installer — the value is entirely in modules and packages.
- Architecturally, this is what spatial-os on the Valve Steam Frame would look like.

**nixos-apple-silicon** is a serious NixOS port to unlocked consumer ARM hardware (Apple Silicon Macs).

- Custom boot chain (m1n1 + U-Boot packaged in Nix), downstream kernel, a shrinking overlay, on-device donor-firmware extraction from the vendor ESP, and a cross-buildable installer ISO.
- Demonstrates the full lifecycle of a device port, including the endgame: upstreaming forks until almost nothing downstream remains (`docs/release-notes.md`).

**mkosi** (benchmark only, non-Nix) is systemd's reproducible image builder: declarative repart partitioning, verity/SecureBoot/PCR signing, A/B via systemd-sysupdate, SOURCE_DATE_EPOCH discipline, hermetic tool trees. Studied here as a checklist of image-assembly concerns spatial-os's `lib/images/` must cover, not as a candidate backend.

---

## 2. Repository/build architecture

### 2.1 robotnix

**Evaluation.**

- `default.nix` runs `lib.evalModules` over a fixed module list: `modules/base.nix`, `modules/source.nix`, `modules/signing.nix`, `modules/release.nix`, `modules/kernel.nix`, per-Android-version modules (`modules/9`, `modules/13`, `modules/15`, `modules/16`), app modules (`modules/apps/*.nix`), and the two live flavors.
- It deliberately does **not** reuse `nixosSystem`; it is a parallel module universe whose "build products" all live in an untyped escape hatch: `build = mkOption { internal = true; type = types.attrs; }` (`modules/base.nix`). Failed assertions are re-implemented by hand, copied from NixOS's `top-level.nix` (`default.nix`).
- `flake.nix` exposes `lib.robotnixSystem = configuration: import ./default.nix { … }` — same call shape as `lib.nixosSystem`, so a downstream flake pins robotnix and builds `.#exampleSystem.img` (`README.md` flake example).
- Top-level convenience outputs re-exported from `config.build`: `targetFiles`, `signedTargetFiles`, `ota`, `incrementalOta`, `img`, `factoryImg`, `bootImg`, `recoveryImg`, `otaDir`, `releaseScript`, `generateKeysScript`, `verifyKeysScript` (`default.nix`).

**Source layer** (`modules/source.nix`).

- The AOSP tree is modeled as `source.dirs.<relpath>`, an attrset of submodules with `src`, `patches`, `gitPatches` (for patches GNU patch can't fuzz), `postPatch`, `nativeBuildInputs`, `linkfiles`/`copyfiles` (implementing git-repo's `<linkfile>`/`<copyfile>` tags), `groups`, and `enable`.
- Patching is done per-directory in small `runCommand` derivations (the `apply` function on the `src` option), so patched dirs are individually cached.
- When `source.manifest.enable = true`, the entire attrset is populated from a JSON lockfile (`source.manifest.lockfile`): each entry becomes `manifestSrc = pkgs.fetchgit { url = entry.project.repo_ref.repo_url; rev = entry.lock.commit; hash = entry.lock.nix_hash; fetchLFS; fetchSubmodules; }` — one fixed-output derivation per git-repo project (1000+ for a full AOSP tree).
- Entries are filtered by `active` flag and by *categories* (`source.manifest.categories`, defaulting to `["Default"]`); LineageOS adds `{ DeviceSpecific = config.device; }` so device repos are selected at eval time (`flavors/lineageos/default.nix`). Group-based `excludeGroups = ["darwin" "mips"]` / `includeGroups` mirror `repo sync -g`.
- An eval-time assertion requires `fetch_completed` in the lockfile (`modules/source.nix`), catching truncated lock generation.

**Lockfile generation** is out-of-band, by the Rust tool in `pkgs/repo2nix` (crates `repo-manifest` + `repo-tool`; the older Python `scripts/mk_repo_file.py` is legacy). Per `pkgs/repo2nix/README.md`:

- `repo-tool fetch` produces the lockfile from a manifest URL + branch/tag, resolves LineageOS `lineage.dependencies` device-dependency chains, and optionally fetches TheMuppets proprietary-vendor repos (`--muppets`).
- Auxiliary subcommands feed the flavor metadata: `get-build-id` (reads `core/build_id.mk` from locked `build/make` into `flavors/grapheneos/build_ids.json`), `get-lineage-devices` (scrapes `LineageOS/hudson` + `git ls-remote` into `devices.json`), `get-graphene-devices` (queries `releases.grapheneos.org` channel API into `channel_info.json`), `ensure-store-paths` (pre-substitutes locked projects for updater scripts).
- Every flavor has an `update.sh` that re-runs this pipeline (`flavors/{lineageos,grapheneos}/update.sh`).

**Build layer** (`modules/base.nix`, `build.mkAndroid`). One `stdenvNoCC.mkDerivation` that:

- writes a custom `builder` script which re-execs the generic build under `unshare -m -r` (new mount + user namespace, fake root);
- assembles the source tree with **bind mounts, not copies**: each enabled dir's `unpackScript` does `mkdir -p ${relpath}; mount --bind ${src} ${relpath}`, with parent-directory mountpoints pre-created by computing the directory tree of all relpaths (`dirsTree` in `modules/source.nix`) — a ~45 GB tree assembled in seconds;
- drops fake root back to the original uid/gid via a bundled `fakeuser` C shim (`modules/fakeuser/`) and enters a `buildFHSEnv` called `robotnix-build`, so AOSP's prebuilt toolchains see an FHS layout; for Android ≥ 12 the FHS env uses a patched `bashInteractive` because AOSP spawns bash with an empty PATH and nixpkgs bash hardcodes `/no-such-path` (`modules/base.nix`, `env` definition);
- sources `build/envsetup.sh`, runs `lunch ${productName} ${release} ${variant}` (or `breakfast` for LineageOS), then `m target-files-package otatools-package`, with `NINJA_ARGS="-j$NIX_BUILD_CORES"`, `TERM=dumb` (to keep soong from emitting ANSI codes into Nix logs), and `requiredSystemFeatures = ["big-parallel"]`;
- exports **`target_files.zip` + `otatools.zip`**, not a flashable image — signing and imaging are downstream derivations (§6, §7). Variants: `checkAndroid` (ninja `-n` dry run), `moduleInfo`, and `mkAndroidComponents targets` which uses `module-info.json` to copy out only the installed files of named build targets — the cheap-partial-build mechanism (`components.nix` at repo root uses it).
- Debug affordances: `debugUnpackScript`/`debugPatchScript` (`modules/source.nix`) and `debugEnterEnv` (`modules/base.nix`) reproduce the exact build environment interactively outside Nix.

**Version drift absorption.** The honest cost of tracking AOSP, all in code:

- `apiLevel` mapping table for Android 7–16 and `targetFilesName` suffix change at Android 14 (`modules/base.nix`);
- avbtool filename churn documented across Android 10→14 (`modules/signing.nix`, `build.generateKeysScript` comment);
- per-version APEX package lists ~70 entries long (`modules/signing.nix` `signing.apex.packageNames`);
- per-version product-makefile injection points (`modules/base.nix` `source.dirs."build/make".postPatch`);
- LineageOS-specific toggles like APEX flattening removal at Android 14 (`flavors/lineageos/default.nix` `OVERRIDE_TARGET_FLATTEN_APEX`) and the LOS 23.0 otatools path exception (`otatoolsOutPath` mkForce).

### 2.2 nixos-generators and the nixpkgs image framework

**Old model.** Each format is a tiny module in `formats/*.nix` setting two options declared in `format-module.nix`: `formatAttr` (which `system.build.<attr>` to realize) and `fileExtension`.

- Examples: `formats/raw-efi.nix` imports `formats/raw.nix`, configures GRUB removable-EFI, mounts `/boot` by `ESP` label, and overrides `system.build.raw` with a `make-disk-image.nix` call (`partitionTableType = "efi"`); `formats/sd-aarch64.nix` merely imports `nixpkgs:nixos/modules/installer/sd-card/sd-image-aarch64.nix` and sets `formatAttr = "sdImage"`.
- `all-formats.nix` fan-outs one base config into every format via `extendModules { modules = [ ./format-module.nix formatModule ]; }` and exposes `config.formats.<name>`; the CLI path (`nixos-generate.nix`) does the same for flake configs via `flakeSystem.extendModules`.
- The flake also exposes a `nixosGenerate { format, modules, customFormats, … }` function (`flake.nix`) that resolves the format module by name — the API spatial-os users would have reached for in 2023.

**New model** (`nixpkgs:nixos/modules/image/images.nix`).

- `image.modules :: attrsOf deferredModule` is pre-populated with ~25 variants (raw/qemu-efi/sd-card/iso/iso-installer/kexec, all clouds); `system.build.images.<variant>` is computed per variant with `extendModules { modules = [ module ]; }`, and each variant module **must** define `system.build.image` (`throw` otherwise).
- The builder derivation is `recursiveUpdate`d with `passthru = { inherit config filePath; }` so callers can introspect the evaluated variant. The `sd-card` variant dispatches on `pkgs.targetPlatform.qemuArch` to pick `sd-image-<arch>.nix`.
- `file-options.nix` standardizes `image.baseName` (default `nixos-image-${label}-${system}`), `image.extension`, `image.fileName`, `image.filePath` so artifacts are locatable generically.
- `nixos-rebuild build-image --image-variant <v>` (implemented in `nixpkgs:pkgs/by-name/ni/nixos-rebuild-ng`) is a thin wrapper over these attributes.
- A warning fires if a config defines the singular `system.build.image` at toplevel while also using `system.build.images` — the framework wants image-specific config confined to variant modules.

**Two builder generations coexist.**
- *Legacy imperative*: `nixpkgs:nixos/lib/make-disk-image.nix` — builds the store image with LKL `cptofs`, then boots a QEMU VM to run `nixos-install` and the bootloader; its own header comment points at nixpkgs issue #324817 ("work towards the effort of unifying our image builders… before adding more"). Still used by `nixpkgs:nixos/modules/virtualisation/disk-image.nix`, which backs the `raw`/`raw-efi`/`qemu`/`qemu-efi` variants (options `image.format`, `image.efiSupport`).
- *Modern declarative*: `image.repart` (§6.2).

### 2.3 Jovian-NixOS

Module set + overlay, no image product.

- `modules/default.nix` imports `steam/`, `steamos/`, `devices/`, `hardware/`, `jovian/` (which injects `overlay.nix` into `nixpkgs.overlays`), and `decky-loader.nix`.
- `overlay.nix` defines ~40 packages: `linux_jovian`/`linuxPackages_jovian`, `linux-firmware-jupiter`, `mesa-radv-jupiter`, `mesa-radeonsi-jupiter`, Jovian-patched `gamescope`/`gamescope-wsi`, `gamescope-session`, `steamos-manager`, `jupiter-hw-support` + derived `steamdeck-firmware`/`steamdeck-hw-theme`/`steamdeck-bios-fwupd`, `pipewire-jupiter`/`wireplumber-jupiter` (vendor DSP configs), `steam`/`steam-unwrapped` overrides, `jovian-stubs`, and more.
- Pinning: `flake.nix` tracks nixos-unstable; non-flake users get the identical rev because `nixpkgs.nix` parses `flake.lock` and `fetchTarball`s `nodes.nixpkgs.locked.rev` with `narHash` — a one-file flake/non-flake bridge worth copying.
- Options are namespaced (`jovian.steam.*`, `jovian.steamos.*`, `jovian.devices.steamdeck.*`, `jovian.hardware.has.*`) and vendor behavior is reproduced as *documented* NixOS config: e.g. `modules/steamos/boot.nix` reproduces Valve's kernel cmdline (`amdgpu.lockup_timeout=5000,10000,10000,5000`, `ttm.pages_min=2097152`, `amdgpu.sched_hw_submission=4`, …) with a comment per flag citing the upstream `jupiter-hw-support` grub config and explaining intentional deviations (e.g. skipping `fbcon=vc:4-6` because of LUKS prompts).
- Session integration: `modules/steam/steam.nix` installs a capability wrapper for gamescope (`cap_sys_nice+pie`), a setuid `galileo-mura-extractor` (per-unit OLED mura-correction data), gamescope-session + steamos-manager as systemd packages, and inputplumber/scx/orca services; `modules/steam/autostart.nix` adds a display-manager-less auto-start with a validated `desktopSession` option.
- `pkgs/gamescope-session/default.nix` rewrites Valve's session shell scripts with `resholve`, pinning every external binary to a store path — the rigorous way to adopt vendor shell scripts.

### 2.4 nixos-apple-silicon

- `apple-silicon-support/` is both a module tree (`modules/{kernel,peripheral-firmware,boot-m1n1,sound,video}`) and an overlay (`packages/overlay.nix`, now reduced to `linux-asahi`, `uboot-asahi`, `libva-v4l2_request-sofus13`, and a *conditional* mesa pin that only overrides mesa when the nixpkgs version equals a known-broken `26.0.5`).
- `flake.nix` exposes `nixosModules.apple-silicon-support`, `nixosModules.apple-silicon-installer`, the overlay, and per-system packages including `installer-bootstrap`: a `nixosSystem` evaluation of `iso-configuration/` with `nixpkgs.hostPlatform.system = "aarch64-linux"` and `nixpkgs.buildPlatform.system = <builder>` — i.e. the installer ISO cross-builds from x86_64.
- The module supports the same trick post-install via `hardware.asahi.pkgsSystem`, which re-imports nixpkgs with `crossSystem` inside the module so "major Asahi packages" (kernel, U-Boot) can be substituted from a cross build (`modules/default.nix`).
- Repo history is a deliberate shrink: release 2025-08-23 dropped the Mesa fork ("since mesa 25.1, support for asahi is enabled by default") and the `useExperimentalGPUDriver`/`experimentalGPUInstallMode` options (now `mkRemovedOptionModule` stubs in `modules/kernel/default.nix`); release 2025-11-18 "drops every custom package except the kernel and U-Boot… cannot be dropped due to upstream nixpkgs' policy," and switched to a full NixOS kernel build for standard hardware support, offset by CI + a nix-community Cachix cache (`docs/release-notes.md`).

### 2.5 mkosi

Single Python tool configured by INI.

- Config layering: base `mkosi.conf`, drop-ins `mkosi.conf.d/` (per-distro/arch conf files in this repo: `arch.conf`, `debian.conf`, `x86-64.conf`, even `postmarketos.conf`), and `mkosi.profiles/<name>/` for named variants.
- Content comes from distro package managers into a rootfs tree inside mkosi's own sandbox; partitioning/imaging is delegated to systemd-repart (`RepartDirectories=`, `Seed=`, `SectorSize=`, `Overlay=`+`BaseTrees=` for overlayfs-based extension images).
- `ToolsTree=` makes builds hermetic against the host by executing all tools from a purpose-built tools image; `Incremental=` caches the package-install stage; `History=` and `CacheDirectory=` appear in the repo's own `mkosi.conf`.
- Output formats include `disk`, `directory`, `sysext`, ISO with `ElTorito=` hybrid boot (`mkosi/resources/man/mkosi.1.md`).

---

## 3. Device abstraction model

**robotnix: device = string + flavor metadata.**

- `device`, `arch`, `variant`, `productName`/`productNamePrefix` are plain options (`modules/base.nix`).
- All per-device knowledge lives in flavor-committed JSON: `flavors/lineageos/devices.json` (codename → default branch) + per-branch `lineage-<ver>/repo.lock` + `missing_dep_devices.json` (eval-time assertion for unsupported devices); `flavors/grapheneos/devices.json`, `channel_info.json` (channel → device → git_tag/build_time), `build_ids.json` (tag → Android build ID, from which `androidVersion` is *derived* by decoding the first letter of the build ID — `flavors/grapheneos/default.nix` `buildIDCodenameInitialToPlatformRelease`), and per-release `vendor_imgs/<device>.json`.
- Device-specific source repos are pulled in by lockfile categories (`{ DeviceSpecific = config.device; }`).
- Adding a device is a metadata regeneration exercise, not new Nix code. Weakness: nothing modular about per-device *hardware* config — it's all upstream's problem (AOSP device trees).

**Jovian: device = module directory + capability flags.**

- `modules/devices/steamdeck/` splits concerns into `kernel.nix`, `firmware.nix`, `controller.nix`, `fan-control.nix`, `graphical.nix`, `hw-support.nix`, `perf-control.nix`, `sdgyrodsu.nix`, `sound.nix`, `workarounds.nix`, all gated on a single `jovian.devices.steamdeck.enable`.
- The device module then asserts capabilities — `jovian.hardware.has.amd.gpu = true` (`modules/devices/steamdeck/default.nix`) — which generic modules (`modules/hardware/amd/`) consume. Two levels: device modules declare *what the hardware is*; capability modules implement *what that implies*.
- Everything uses `mkDefault` so users can override (e.g. `enableKernelPatches` defaults to `cfg.enable` but is independently settable, `modules/devices/steamdeck/kernel.nix`).
- This is the cleanest device model of the five projects and scales naturally to a device matrix.

**nixos-apple-silicon: device = hardware family, differentiated by DTB.**

- One `hardware.asahi.enable` gate for all Macs; per-machine variation is resolved at boot because *all* `apple/*.dtb` files from the kernel build are concatenated into the m1n1 payload and m1n1 selects the right one (`modules/boot-m1n1/default.nix`).
- No per-model Nix code at all. Applicable to spatial-os wherever one kernel tree covers several headset SKUs.

**nixos-generators/nixpkgs: no device model** — variants are *output formats*, orthogonal to hardware. spatial-os must therefore compose two axes itself: device modules (Jovian-style) × image variants (`image.modules`), evaluated as a matrix.

**mkosi: profiles as variant axis** (`mkosi.profiles/`, `--profile=`), no hardware model.

---

## 4. Vendor blob / donor firmware handling

**robotnix + adevtool** (`modules/adevtool/default.nix`) — donor = vendor factory image, extraction at build time:
- Metadata-driven acquisition: `adevtool.vendorImgMetadata` (list of `{fileName, url, sha256}`, imported from `flavors/grapheneos/<release>/vendor_imgs/<device>.json`) is mapped to `pkgs.fetchurl` FODs and exposed as `adevtool.vendorImgs` and a `linkFarm` (`build.vendorImgDir`).
- The extraction tool is itself a pinned source dir (`source.dirs."vendor/adevtool"` from the repo lockfile) whose node_modules come from `fetchYarnDeps` keyed by a per-release hash (`adevtool.yarnHash`, values in `flavors/grapheneos/yarn_hashes.json`) applied via `yarnConfigHook` in the dir's `postPatch`. **Second-order pinning: the blob extractor's own dependency lockfile must be re-hashed every release.**
- Invocation happens *inside the main build derivation, before `lunch`* (`modules/base.nix` buildPhase): a throwaway `lunch sdk_phone64_x86_64 cur user`, symlink the fetched images into `ADEVTOOL_IMG_DOWNLOAD_DIR=/tmp/vendor_imgs`, run `vendor/adevtool/bin/run generate-all --noVerify -d <device>` with a `fakeGit` shim on PATH (adevtool wants `git describe`-ish output; the source dir has no `.git`, so a one-line script echoes the locked rev). Then a sed-hack renames the generated vendor RROs (`auto_generated_rro` → `auto_generated_vendor_rro`) to avoid collisions with robotnix's own generated RROs — the kind of deep vendor-tool coupling to expect.
- Extraction reads vendor ext4 images with e2fsprogs `debugfs` (no mounts, `envPackages` comment). Patches to adevtool itself are version-gated (`flavors/grapheneos/default.nix`: `adevtool-ignore-EINVAL-upon-chown.patch` only for releases < 2025090300; several `adevtool-*.patch` variants in the flavor dir).
- LineageOS path: proprietary blobs come pre-extracted as git repos from TheMuppets, pinned in the same repo lockfile via `repo-tool fetch --muppets` (`pkgs/repo2nix/README.md`) — blobs as ordinary FOD sources, no extraction step.

**nixos-apple-silicon** (`modules/peripheral-firmware/default.nix`) — donor firmware from live hardware:
- The Asahi installer (running under the stock OS) harvests Apple's non-redistributable peripheral firmware (Wi-Fi, webcam, ALS…) into `vendorfw/firmware.cpio` on the ESP.
- The NixOS module ingests it with a documented eval-time impurity: `hardware.asahi.peripheralFirmwareDirectory` defaults to `lib.findFirst (path: builtins.pathExists (path + "/firmware.cpio")) null [ /boot/vendorfw /mnt/boot/vendorfw ]` (normal boot vs installer mount), and a plain `stdenv.mkDerivation` cpio-extracts into `$out/lib/firmware`, added to `hardware.firmware`. The option description explicitly instructs flake/pure/remote-build users to copy the file elsewhere and set the path manually, and notes a possible future switch to boot-time loading per the Asahi open-OS-interop docs. An assertion fires if extraction is enabled but the directory is missing. `extractPeripheralFirmware = false` opts out.
- This is exactly spatial-os's situation for per-headset DSP/GPU/sensor firmware that cannot be redistributed: extract on-device at install time, ingest via an explicit path option (or a `requireFile`-style pinned copy), never fetch from vendor servers.

**Jovian** — three grades of vendor consumption, all hash-pinned:
1. *Redistributable binary bundle*: the Steam Deck client bootstrap tarball, `fetchurl` from `https://steamdeck-packages.steamos.cloud/misc/steam-snapshots/steam_jupiter_stable_bootstrapped_<ver>.tar.xz`, injected into the standard nixpkgs steam as `bootstraplinux_ubuntu12_32.tar.xz` (`pkgs/steam-jupiter/unwrapped.nix`).
2. *Vendor firmware + updater blobs*: `jupiter-hw-support` fetched from Valve's GitHub tag (`pkgs/jupiter-hw-support/src.nix`, with four Jovian patches for path/user assumptions), then adapted in `firmware.nix`: `autoPatchelfHook` for the Insyde BIOS flasher, `sed`-rewrites of `/usr/…` paths, a wrapper (`h2offtWrapper`) around `h2offt`'s hardcoded `/boot/efi` ESP path that relocates `isflash.bin` when the flasher exits 194, deletion of unusable GTK2 binaries, and rewritten systemd units. Licensed `unfreeRedistributableFirmware`. `bios-fwupd.nix` alternatively routes BIOS updates through fwupd.
3. *Vendor source forks*: kernel and Mesa (§5); Mesa RADV is built with the exact meson flags from Valve's PKGBUILD, including a pinned `radv-build-id` (`pkgs/mesa-radv-jupiter/default.nix`).

**nixpkgs / nixos-generators / mkosi**: no blob story. mkosi's nearest knobs are `ExtraTrees=`/`BaseTrees=` (inject arbitrary prebuilt content) and `SkeletonTrees=`.

**Transferable decomposition.** Keep three phases separable:

- (a) *acquisition* — `fetchurl`/`fetchgit` FOD from committed metadata, or impure on-device path option;
- (b) *extraction tooling* — pinned like any other input including its language-ecosystem lock hash;
- (c) *placement* — module options feeding `hardware.firmware`, a vendor partition image, or the foreign build tree.

robotnix fuses (b)+(c) into the mega-derivation (rebuild-the-world coupling); apple-silicon keeps them as a cheap standalone derivation.

---

## 5. Kernel strategy

**Jovian** (`pkgs/linux-jovian/default.nix`) — the canonical vendor-kernel pattern:
- `buildLinux` over `fetchFromGitHub` of Valve's tree (`Jovian-Experiments/linux`, tag `7.2.4-valve1`; version string composed as `${kernelVersion}-${vendorVersion}`).
- The vendor config delta is expressed as `structuredExtraConfig`, ~150 lines, with **provenance comments on every option**: which vendor decision it mirrors, what was renamed/removed upstream ("Jovian: renamed", "Jovian: removed in 7.2"), and `lib.mkForce (option no)` fixups where vendor-set options collide with nixpkgs common-config (the `DRM_AMD_DC_SI`/`HYPERV`/`KVM_GUEST` block). Notably it already carries Steam-Frame-adjacent options: `RTW89_*_USB` Realtek dongle modules ("Enable realtek dongle for Steam Frame") and `LEDS_VALVE`.
- Vendor sloppiness is absorbed in `postFetch`: sed-fixes the stale `EXTRAVERSION` in the Makefile, with the comment that `postPatch` on kernels "doesn't compose in buildLinux" — fix sources at fetch time, keep the FOD hash stable.
- The overlay pairs it with `linuxPackagesFor` so out-of-tree module packages keep working, and a separate `linux-firmware-jupiter` overrides linux-firmware (`overlay.nix`).

**nixos-apple-silicon** (`packages/linux-asahi/default.nix`):

- Same pattern against `AsahiLinux/linux` tag `asahi-7.1.13-3`; minimal `structuredExtraConfig` (`ARM64_16K_PAGES` for the GPU, Apple SoC drivers as `yes` where module-loading order matters, `features.rust = true`).
- User `boot.kernelPatches` are honored by re-plumbing them through an `_kernelPatches` argument (`modules/kernel/default.nix`: `boot.kernelPackages = pkgs'.linux-asahi.override { _kernelPatches = config.boot.kernelPatches; }` — note `pkgs'` is the possibly-cross `hardware.asahi.pkgs`).
- Since 2025-11-18 it is a *full* NixOS kernel build (all modules) — worse build time, zero hardware-support surprises, made tolerable by CI + public cache (`docs/release-notes.md`).
- The module also carries the vendor-derived `boot.initrd.availableKernelModules` list (copied from asahi-scripts initcpio, with citation) and boot-critical `kernelParams` (`earlycon`, the `nvme_apple.flush_interval=0` tradeoff, documented inline).

**robotnix** (`modules/kernel.nix`):

- Kernels are built as separate Nix derivations (flavor-provided `config.build.kernel`) and then **spliced into the AOSP tree as prebuilts**: `source.dirs.${kernel.relpath}.postPatch` copies `${config.build.kernel}/*` over the checked-in prebuilt kernel dir, warning about files it does not replace. This respects AOSP's prebuilt-kernel workflow rather than fighting soong.
- Options: `kernel.src/patches/postPatch/clangVersion` (pointing at AOSP's prebuilt clang per-version README).
- Status: custom kernels are **unmaintained** (`README.md` status table) — Android kernel packaging (per-release clang prebuilts, LTO/CFI memory blowups, see `NEWS.md` 2021-09-09 note on redfin) churns too fast.
- LineageOS sidesteps this: kernels build in-tree during the main build, needing only `envPackages` additions (`flavors/lineageos/default.nix` adds `openssl.dev`, gcc, glibc.dev for Android ≥ 11) plus a determinism patch (`0003-kernel-Set-constant-kernel-timestamp-*.patch`).

**Takeaway for spatial-os:** per-device kernels should be standalone `buildLinux` derivations (Jovian/Asahi style: vendor tag + structuredExtraConfig-with-provenance + DTB outputs), never built inside image or foreign-build derivations, so kernel ↔ image rebuilds are decoupled. Where a donor Android build must be produced, feed the Nix-built kernel in as prebuilts (robotnix `modules/kernel.nix` splice pattern).

---

## 6. Image assembly and flashing

### 6.1 robotnix (Android artifact pipeline)

All imaging is delegated to AOSP's own `otatools` operating on `target_files.zip` in *small* `runCommand` derivations (`modules/release.nix`):
- `img` — `img_from_target_files` → fastboot-flashable `img.zip`; flashing is manual `fastboot update -w <img.zip>` (`README.md`).
- `factoryImg` — drives AOSP's `generate-factory-images-common.sh` with `BOOTLOADER`/`RADIO` versions parsed out of `OTA/android-info.txt` inside the target-files zip.
- `bootImg`/`recoveryImg` — single-file `unzip -p` extractions for fastboot flashing.
- `ota`, `incrementalOta` — §7.
- otatools binaries are made runnable outside the FHS env by patchelf-ing the interpreter (`fixOtaTools` in `modules/base.nix` + `scripts/patchelf-prefix.sh`), keeping embedded-python executables unstripped, and symlinking script entry points; `pkgs.robotnix.unpackImg` unpacks built images for inspection (`build.unpackedImg`).
- All of these run with **test keys** in-store; the signed equivalents are produced outside the store by `releaseScript` (§8 impurities / §7).

### 6.2 nixpkgs repart path (the modern extension point)

`image.repart` (`nixpkgs:nixos/modules/image/repart.nix`):
- Partitions are declared as `image.repart.partitions."NN-name" = { storePaths; contents."/dest".source; nixStorePrefix; repartConfig = { Type; Label; Format; Minimize = "guess"; SizeMinBytes; … }; }` — `repartConfig` is passed through to `repart.d(5)` INI verbatim (`pkgs.formats.ini` with duplicate-key lists).
- Store closures: `pkgs.closureInfo { rootPaths = storePaths; }` per partition; `amend-repart-definitions.py` injects `CopyFiles=` lines for every closure path into the generated repart.d files.
- Assembly (`repart-image.nix`): `unshare --map-root-user fakeroot systemd-repart --empty=create --seed=<uuid> --architecture=<systemd-arch> --split=<bool> --definitions=repart.d --json=pretty` in a plain `stdenvNoCC` derivation — **no VM, no root, no binfmt**; mkfs tools selected per declared filesystem (vfat/ext4/squashfs/erofs/btrfs/xfs/swap map in `fileSystemToolMapping`); optional in-derivation zstd/xz/zstd-seekable compression; `--split=true` additionally emits one file per `SplitName=` partition — the hook for per-partition fastboot artifacts.
- `image.repart.mkfsOptions` → `SYSTEMD_REPART_MKFS_OPTIONS_<FSTYPE>` env vars for per-fs tuning.
- Cross: `image.repart.package` defaults to `buildPackages.systemd` explicitly "allowing for cross-compiled systems to work" (`repart.nix`).
- Verity: `image.repart.verityStore` (`repart-verity-store.nix`) builds a dm-verity-protected store/`/usr` partition pair with arch-correct GPT types (`usr-arm64`/`usr-arm64-verity`) and a build-time check that the UKI's embedded roothash matches the built verity partition (`assert_uki_repart_match.py`).
- GPT label-length assertions and an explicit warning to keep label headroom for sysupdate version-in-label schemes (`repart.nix` warnings).

Legacy path: `nixpkgs:nixos/lib/make-disk-image.nix` (LKL `cptofs` staging + QEMU VM to run `nixos-install`/bootloader; formats raw/qcow2/vdi/vpc; partition layouts none/legacy/efi/hybrid) — still behind `image.modules.{raw,raw-efi,qemu,qemu-efi}` via `nixpkgs:nixos/modules/virtualisation/disk-image.nix`. Special-purpose builders: `make-ext4-fs.nix`, `make-squashfs.nix`, `erofs-store-image.nix`, `make-iso9660-image.nix`, `make-system-tarball.nix` — useful as leaf tools when spatial-os must produce a bare `system.img`-style filesystem for fastboot rather than a GPT disk.

### 6.3 nixos-generators

Pure dispatch to the above (see §2.2). Nothing to port beyond confirmation that the `extendModules` fan-out pattern (one config → many image evals) is the right multi-variant mechanism — which nixpkgs `images.nix` now implements natively.

### 6.4 nixos-apple-silicon

No disk image for the installed system. Deliverables:

- The installer ISO (`iso-configuration/`, standard `config.system.build.isoImage`, cross-buildable; `boot.postBootCommands` copies the module tree into `/etc/nixos` on first boot for the clone-config workflow).
- The boot payload: `m1n1/boot.bin = cat m1n1.bin + all apple/*.dtb + u-boot-nodtb.bin.gz [+ extra m1n1 options appended as text]` (`modules/boot-m1n1/default.nix`), installed by the *standard NixOS bootloaders* via `boot.loader.{grub,systemd-boot,limine}.extraFiles` — the custom boot chain rides the existing bootloader-install machinery instead of replacing it.
- `packages/uboot-asahi/default.nix` shows the same concatenation trick inside the U-Boot package (`preInstall` gzip + cat, "so that m1n1 knows U-Boot's size and can find things after it").
- Disk partitioning of the target is done by the vendor-side Asahi installer (macOS), not by Nix. `system.extraDependencies` forces m1n1/U-Boot into the installer closure (`modules/boot-m1n1/default.nix`).

### 6.5 Jovian

None. Installation is stock NixOS; SteamOS's real A/B image machinery (rauc/casync — used on Steam Frame per sibling doc `07-device-landscape.md`) is not reproduced anywhere in the repo.

### 6.6 mkosi (benchmark checklist)

All in `mkosi/resources/man/mkosi.1.md`; the A/B + verity end-to-end walkthrough is `docs/root-verity.md`:

- GPT via repart with `Verity=` (`signed`/`hash`/`defer`/`auto` modes) producing hash + signature partitions; `VerityKey=`/`VerityCertificate=` with pluggable key sources (incl. PKCS#11 via `*KeySource=`).
- UKI building with `SecureBoot=`, `SecureBootAutoEnroll=`, shim support; `SignExpectedPcr=` for systemd-measure PCR policy signatures.
- `SplitArtifacts=` (incl. split-out pcrs); `Overlay=`+`BaseTrees=` for sysext layering.
- `Seed=`/`mkosi.seed`; `SectorSize=`; `ElTorito=` hybrid ISO boot; GPG-signed `SHA256SUMS` (`Key=`).

---

## 7. Update mechanism

**robotnix — Android-native OTA** (`modules/release.nix`):
- `ota_from_target_files` produces full OTAs; incremental OTAs via `-i prevTargetFiles` with `prevBuildDir`/`prevBuildNumber`/`prevTargetFiles` options; a `retrofit` option adds `--retrofit_dynamic_partitions` for pre-dynamic-partition devices; LineageOS adds `--backup=true` for addon.d survival (`flavors/lineageos/default.nix` `otaArgs`).
- `otaDir` assembles a static updater-server directory: OTA zip + target-files + flavor-specific metadata — GrapheneOS one-line format (`<buildNumber> <buildDateTime> <device> <channel>`) or LineageOS Updater JSON (datetime/filename/id/romtype/size/url/version), with `ROM_SIZE` patched in by `du` at assembly (`otaMetadata`/`writeOtaMetadata`). Root-level `ota.nix` shows the intended use: `symlinkJoin` several devices' `otaDir`s into one nginx-served tree. The device-side updater app is configured via `apps.updater.url` and included per flavor (`modules/apps/updater.nix`, set `includedInFlavor` in both flavors).
- OTA monotonicity is an explicit contract: `buildDateTime` "needs to be monotonically increasing… if you use the over-the-air (OTA) update mechanism" (`modules/base.nix` option doc). Seamless A/B semantics come from AOSP itself.
- The signed release path runs outside Nix: `releaseScript` signs target-files, then builds OTA + incremental + img + factory from the *signed* target-files, then writes updater metadata (`modules/release.nix`).

**Jovian — NixOS generations + vendor-UI stubs.** Updates are `nixos-rebuild switch`. Valve's update plumbing is defused by `pkgs/jovian-stubs/`:

- `holo-update` implements the vendor updater's exit-code contract (0 = updated, 7 = no update, 8 = needs reboot) by comparing `readlink /run/booted-system/kernel` against `/nix/var/nix/profiles/system/kernel` — the gamescope UI's "check for updates" button thus correctly reports "reboot required" after a rebuild that changed the kernel.
- Other stubs neutralize `holo-select-branch`, `jupiter-biosupdate` (the real flasher is separately packaged), `steamos-wifi-set-backend`, `holo-factory-reset-config`, etc. (`pkgs/jovian-stubs/`).
- `modules/steam/updater.nix` separately manages the *Steam client* updater's splash image (systemd service writing to `/run`, `jovian-updater-logo-helper`).

**nixos-apple-silicon — NixOS generations + disciplined channel tracking.** No image updates; the port itself follows `docs/release-process.md`:

- `main` tested only against nixos-unstable; `release-YY.MM` branches pinned to each NixOS stable; dated `release-YYYY-MM-DD` tags produce CI-built installer images.
- A written test protocol (installer boots on both build platforms, install completes, DE/network/GPU/audio checks) gates each release; U-Boot/m1n1-updating releases get extra reboot testing because those flash to the ESP.

**nixpkgs — systemd-sysupdate.** `nixpkgs:nixos/modules/system/boot/systemd/sysupdate.nix` configures `sysupdate.d` transfers; combined with `image.repart` + `verityStore` + UKIs this is the current upstream answer for image-based A/B "appliance NixOS." The repart module's GPT-label warning (§6.2) exists precisely for sysupdate's version-in-label scheme.

**mkosi.** `mkosi sysupdate` wraps `systemd-sysupdate --transfer-source=` (`mkosi/resources/man/mkosi.1.md`); `docs/root-verity.md` supplies complete transfer definitions for a root+verity+sig+UKI A/B scheme (partition triplets + ESP UKI files, `InstancesMax=2`).

**Spatial-os implication:** two genuinely different update planes. Android-boot-chain donors ⇒ robotnix model (OTA payloads, monotonic build numbers, updater metadata dir). Open Linux devices (Steam Frame) ⇒ either mutable NixOS (`nixos-rebuild`, Jovian model) or sysupdate A/B appliance images (nixpkgs/mkosi model). Don't force one mechanism across both.

---

## 8. Reproducibility properties

Distinguish **input pinning** (same inputs every eval) from **bit-reproducibility** (same output bits). Findings per project, with concrete impurities enumerated.

### robotnix

*Pinning:* strong.

- Every AOSP project is a `fetchgit` FOD from the lockfile (`modules/source.nix`).
- Vendor images are `fetchurl` FODs (`modules/adevtool/default.nix`); adevtool's node deps via `fetchYarnDeps` hash.
- nixpkgs + android-nixpkgs pinned by `flake.lock`; per-release metadata (build IDs, channels, yarn hashes) committed as JSON.

*Bit-reproducibility:* actively engineered, not guaranteed. Evidence:
- `useReproducibilityFixes` option (default true, `modules/base.nix`).
- `buildDateTime` defaults to the **max commit timestamp across all locked source dirs** (`modules/base.nix`) — deterministic, not `now`; `BUILD_NUMBER`/`BUILD_DATETIME`/`BUILD_USERNAME`/`BUILD_HOSTNAME` env pinning (base + `flavors/grapheneos/default.nix` matching upstream's `grapheneos`/`grapheneos`).
- Determinism patches: `flavors/lineageos/0002-bootanimation-Reproducibility-fix*.patch` (imagemagick output), `0003-kernel-Set-constant-kernel-timestamp*.patch`, `modules/userdata-cache-uuid-reproducible.patch`, `0004-dont-run-repo-during-build*.patch` (kills a build-time `repo` invocation — a network/VCS impurity in upstream LineageOS).
- A packaged `pkgs/diffoscope` for output comparison; `check.nix`/`scripts/check.sh` for CI evaluation.
- Known upstream mutability hazard: LineageOS force-pushes its prebuilt webview repo and GCs old revs, breaking even FOD re-fetches — robotnix disables `external/chromium-webview` and documents the incident (`flavors/lineageos/default.nix` comment; `NEWS.md` 2021-03-02).

*Impurities (all deliberate and scoped):*
1. `ccache.enable` bind-mounts host `/var/cache/ccache` through a Nix **sandbox exception** (`extra-sandbox-paths`), with a fail-fast permission probe in the builder (`modules/base.nix`). Opt-in dev accelerator; breaks purity by design (`CCACHE_COMPILERCHECK=content` mitigates staleness).
2. **Release signing runs outside the store**: `releaseScript` takes `$KEYSDIR` at runtime, copies keys to a `mktemp -d /dev/shm/…` (because AOSP's `ZipWrite` chmods its inputs), verifies them with `verifyKeysScript`, and signs with `sign_target_files_apks` (`modules/release.nix` `wrapScript`). Keys never enter the store; in-store builds use AOSP test keys (`runWrappedCommandWithTestKeys` points `keysDir` at `build/make/target/product/security`). PKCS#11/YubiKey signing keeps even runtime keys off disk (`modules/pkcs11.nix`, `NEWS.md` 2026-03-26).
3. Environmental requirements: user namespaces (`CONFIG_USER_NS`), sandbox enabled for the signing-adjacent features, non-tmpfs `/tmp` sized ~100 GB+, `big-parallel` feature (`README.md`, `modules/base.nix`).
4. `fakeGit` feeds adevtool a fixed rev instead of real VCS metadata (`modules/base.nix`) — a *controlled* answer to tools demanding `.git`.

### nixpkgs image framework

- repart: fixed default partition-UUID seed `0867da16-f251-457d-a9e8-c31f9a3c220b` — "Random but fixed to improve reproducibility," settable to `random` (`repart.nix` `seed`); systemd-repart derives per-partition UUIDs from it deterministically.
- `unsafeDiscardReferences.out = true` treats the image as a boundary artifact (`repart-image.nix`).
- mtimes inside filesystems inherit Nix's epoch discipline; mkfs invocations run under fakeroot with fixed inputs. Compression is in-derivation (threaded zstd — note: multithreaded xz/zstd *can* introduce nondeterminism in some tools; zstd output is deterministic for fixed level/threads).
- `make-disk-image.nix`: VM-based; more historical nondeterminism surface (timestamps, UUIDs, ext4 randomness) — a reason it's being sunset for appliance work.
- NixOS closures are pinned by construction; bit-reproducibility of the *contents* is nixpkgs' broader r13y effort, not enforced here.

### Jovian

- *Pinning:* everything by tag + hash:
  - kernel `7.2.4-valve1` (`pkgs/linux-jovian/default.nix`);
  - Mesa `steamos-26.05.18` with pinned `radv-build-id` meson flag (`pkgs/mesa-radv-jupiter/default.nix`);
  - steam bootstrap `20251031.0` (`pkgs/steam-jupiter/unwrapped.nix`);
  - `jupiter-hw-support` `jupiter-20260914.1` (`pkgs/jupiter-hw-support/src.nix`).
  - No lock automation — bumps are manual commits.
- *Impurities:* none at build time. Runtime is necessarily impure (Steam self-updates into `$HOME`; BIOS/controller flashers write hardware + ESP). Vendor URLs are mutable hosts but versioned filenames; no archival mirror exists in-repo.

### nixos-apple-silicon

- *Pinning:* kernel/U-Boot by tag + hash (`packages/linux-asahi/default.nix`, `packages/uboot-asahi/default.nix`); nixpkgs by `flake.lock`; releases document known-good nixpkgs revs (`docs/release-process.md`).
- *Impurities:* (1) `peripheralFirmwareDirectory`'s default does `builtins.pathExists` against `/boot` and `/mnt/boot` at **eval time** (`modules/peripheral-firmware/default.nix`) — acknowledged in the option docs, with a pure escape hatch; breaks remote/CI evaluation unless overridden. (2) `hardware.asahi.pkgs` re-imports nixpkgs inside the module (`modules/default.nix`) — pure but doubles eval cost and bypasses the host's overlay set except for its own.

### mkosi

Explicit artifact-determinism toolkit, but unpinned acquisition:

- `SourceDateEpoch=` clamps all file mtimes and propagates to repart and scripts (resolution order: setting → `--environment` → host env, `mkosi/config.py`).
- `Seed=`/`mkosi.seed` fixes partition UUIDs; `ToolsTree=` fixes tool versions independent of host.
- `CacheOnly=always` forbids network and is described as providing "a minimal level of reproducibility, as long as the package cache is already fully populated" (`mkosi/resources/man/mkosi.1.md`).
- Package installation from distro repos is a mutable-network operation by default — snapshot-repo pinning is the user's job. Nix strictly dominates on input pinning; mkosi's UUID/mtime/tool-version discipline is the part worth copying.

### Cross-cutting: architectures, binfmt, cache boundaries

- **robotnix:** x86_64 build hosts only (`flake.nix` outputs `x86_64-linux`; `devShells.x86_64-linux`). Nix never cross-compiles — AOSP's own prebuilt toolchains emit the ARM artifacts. No binfmt anywhere.
- **Jovian:** native x86_64 only.
- **nixos-apple-silicon:** real nixpkgs cross (x86_64→aarch64) for the installer ISO and for `pkgsSystem`-selected packages; native aarch64 after install. No binfmt.
- **nixpkgs images:** repart images cross-build cleanly — build-platform systemd assembles target-arch filesystems (`--architecture=` flag; `buildPackages.systemd` default) and nothing target-arch executes. `make-disk-image`/sd-image paths execute target code (VM or activation scripts) ⇒ need binfmt emulation or native builders for foreign arch.
- **Cache/rebuild boundaries:**
  - robotnix: the AOSP mega-derivation means *any* source dir change (1 repo of 1000) rebuilds the whole ~10 h build (`README.md` timing figures). Separate derivations: each fetched repo, each patched dir, kernel, vendor image fetches, APKs, and everything post-target-files (img/ota/factory are seconds-long `runCommand`s). Signing being outside the store means **key rotation costs zero rebuilds**. `mkAndroidComponents` carves out sub-targets. Historical caches: robotnix.cachix.org for kernels/Chromium (`NEWS.md` 2021-01-05).
  - nixpkgs framework: package → toplevel closure → per-partition filesystem (`Minimize=guess` re-runs on closure change) → image; the last two are minutes, not hours. Variant evals share the base config via `extendModules`, so eval cost is shared too.
  - Jovian / apple-silicon: nixpkgs granularity; the kernel is the big unit. apple-silicon deliberately *enlarged* the kernel build (full module set) and paid for it with CI + Cachix rather than keeping a bespoke trimmed config (`docs/release-notes.md` 2025-11-18) — cacheability beats cleverness.
  - mkosi: `Incremental=` caches the installed base tree; `ToolsTree` cached across builds; invalidation heuristics in `CacheOnly=auto`.

---

## 9. What spatial-os should adopt

1. **`nixosSystem`-shaped per-device entry point; parallel module system only where the product is a foreign image.** robotnix proves `evalModules` scales to a non-NixOS build product (`default.nix`), but everything NixOS-based (Steam Frame class) should stay on real `nixosSystem` to inherit `system.build.images`, sysupdate, and the module ecosystem. Expose `spatialSystem = configuration: …` mirroring `robotnix/flake.nix lib.robotnixSystem`.
2. **Lockfile-driven source/donor ingestion with a dedicated out-of-band fetch tool** (`robotnix/modules/source.nix` + `pkgs/repo2nix`): JSON lockfiles → attrsets of `fetchgit`/`fetchurl` FODs; eval-time completeness assertions (`fetch_completed`); category/group filters for per-device source selection; and an `update.sh`-grade automation for *every* donor input class (git manifests, firmware URLs, extractor tool hashes). The robotnix maintenance gap (`NEWS.md` 2023-10→2026-02; `README.md` status table) shows un-automated pins rot within one vendor release cycle — only the flavors with full update automation survived.
3. **Donor blobs as data, not code** (`robotnix/modules/adevtool/default.nix`): per-device/per-release JSON metadata (filename/url/hash) → `fetchurl` FODs → `linkFarm` handed to the extractor; the extractor pinned *including its yarn/cargo lock hash* (`yarn_hashes.json` pattern). For firmware that cannot be downloaded, adopt apple-silicon's on-device extraction with an explicit `…FirmwareDirectory` path option, documented impurity, and a pure escape hatch (`nixos-apple-silicon/apple-silicon-support/modules/peripheral-firmware/default.nix`).
4. **Jovian's device-module layout**: `modules/devices/<codename>/{kernel,firmware,graphical,workarounds,…}.nix` gated on one `spatial.devices.<codename>.enable`, asserting capability flags (`jovian.hardware.has.*` pattern in `modules/devices/steamdeck/default.nix`) consumed by generic XR modules; `mkDefault` everywhere so users can override. This is the axis robotnix lacks and spatial-os needs for 6+ headsets.
5. **Vendor kernel pattern**: `buildLinux` + vendor tag + `structuredExtraConfig` with per-option provenance comments + `postFetch` source fixups (`jovian-nixos/pkgs/linux-jovian/default.nix`); accept `boot.kernelPatches` passthrough (`nixos-apple-silicon/…/packages/linux-asahi/default.nix`); prefer full-config builds + cache over trimmed configs (apple-silicon 2025-11-18 lesson). Where a donor AOSP build is required, splice Nix-built kernels in as prebuilts (`robotnix/modules/kernel.nix`).
6. **`image.modules` / `system.build.images` as the image-variant extension point** (`nixpkgs:nixos/modules/image/images.nix`): spatial-os's `lib/images/` should be deferred modules (`fastboot-images`, `ab-sysupdate-disk`, `installer-usb`, `dev-vm`, …) injected into `image.modules`, each defining `system.build.image` and the standard `image.baseName`/`filePath` (`file-options.nix`) — gaining `nixos-rebuild build-image` and per-variant `passthru.config` introspection for free, and matching the `extendModules` fan-out that nixos-generators pioneered (`all-formats.nix`).
7. **systemd-repart as primary image assembler** (`nixpkgs:nixos/modules/image/repart.nix`): rootless, VM-less, binfmt-less, cross-clean (`buildPackages` systemd, `--architecture=`), deterministic UUID seed, `split = true` for per-partition fastboot artifacts, `mkfsOptions` for fs tuning, `verityStore` for measured images. Reserve `make-ext4-fs`/`make-squashfs` (`nixpkgs:nixos/lib/`) as leaf tools for bare Android partition payloads.
8. **Signing outside the store, test keys inside** (`robotnix/modules/release.nix`): every in-store artifact signed with committed test keys ⇒ fully cacheable CI; a generated `releaseScript` re-signs final artifacts against `$KEYSDIR` (dir or PKCS#11 token, `modules/pkcs11.nix`) with `generateKeysScript`/`verifyKeysScript` companions and key-size assertions (`modules/signing.nix`). Key rotation must never trigger rebuilds.
9. **The foreign-build execution recipe** (`robotnix/modules/base.nix` `mkAndroid`): `unshare -m -r` + per-directory `mount --bind` for zero-copy tree assembly; `buildFHSEnv` for vendor prebuilts; `fakeuser` de-escalation; `TERM=dumb`; build-ID env pinning; `stdenvNoCC` to avoid env contamination; `requiredSystemFeatures = ["big-parallel"]`; `debugEnterEnv`-style reproduction scripts. Reuse nearly verbatim for any AOSP/Qualcomm build spatial-os must run.
10. **Deterministic-by-derivation timestamps**: default build time = max commit date of locked sources (`robotnix/modules/base.nix` `buildDateTime`), never `now`; monotonicity documented as an OTA contract.
11. **Release/tracking discipline** (`nixos-apple-silicon/docs/release-process.md`): `main` on nixos-unstable, `release-YY.MM` per NixOS stable, dated tags with CI-built installer artifacts and a written on-hardware test protocol; plus the explicit strategy of upstreaming forks (Mesa 2025-08-23, everything-but-kernel-and-U-Boot 2025-11-18, `docs/release-notes.md`) to shrink the maintained surface.
12. **Vendor-updater/UI stubbing** (`jovian-nixos/pkgs/jovian-stubs/holo-update`): honor vendor update-UI exit-code contracts with thin shims over NixOS generations; `resholve` vendor shell scripts to store-pinned dependencies (`pkgs/gamescope-session/default.nix`).
13. **Flake/non-flake bridge**: `jovian-nixos/nixpkgs.nix` (parse `flake.lock`, `fetchTarball` by rev+narHash) — one file, keeps both consumption modes on identical pins.
14. **mkosi-derived assembly checklist** for `lib/images/`: fixed partition-UUID seed; SOURCE_DATE_EPOCH/mtime clamping; verity hash+sig partitions with pluggable key sources; UKI + SecureBoot + `SignExpectedPcr` PCR policies; split artifacts; A/B ESP+/usr layout with sysupdate transfer definitions (`mkosi/docs/root-verity.md`); compression policy; hermetic tools (native in Nix).

---

## 10. What spatial-os should reject and why

1. **The mega-derivation as a general pattern** (robotnix `build.android`, `modules/base.nix`). Unavoidable when wrapping soong, but for NixOS-based targets package/kernel/rootfs/partition/image must stay separate derivations. A one-repo change costing a 10-hour rebuild (`README.md` build times) is only acceptable where the foreign build system leaves no choice — and even then, carve out sub-products (`mkAndroidComponents`).
2. **Per-vendor-branch patch forests** (`robotnix/flavors/lineageos/0001-Remove-LineageOS-keys{,-19,-20,-21}.patch`, `0002-…{,-21,-22_2}.patch`, per-version signing patch stacks in `modules/signing.nix`). Every branch-specific patch multiplies rebase work at each vendor release. Prefer config-level overrides (Jovian's `structuredExtraConfig` + `mkForce`, cmdline flags in `modules/steamos/boot.nix`) over source patches; where patches are unavoidable, isolate per-donor-release directories with automated rebase checking in the update pipeline.
3. **Building on nixos-generators** — deprecated, warning-emitting (`format-module.nix`), and its one durable idea (`extendModules` fan-out) already lives in `nixpkgs:nixos/modules/image/images.nix`. Its `formatAttr` indirection is obsolete; target `system.build.image`/`system.build.images` directly.
4. **VM-based image builders** (`nixpkgs:nixos/lib/make-disk-image.nix`): slow, KVM-dependent, cross-hostile (binfmt or nested VM), and discouraged by its own maintainers' header comment. Use repart.
5. **Host-cache sandbox exceptions as defaults** (robotnix `ccache.enable` + `/var/cache/ccache`, `modules/base.nix`): acceptable as an opt-in developer accelerator, never in release builds — it silently couples outputs to host state and requires host nix.conf changes.
6. **Trusting vendor-hosted mutable sources without archival**: LineageOS force-pushed prebuilt repos broke robotnix builds (`NEWS.md` 2021-03-02; `flavors/lineageos/default.nix` webview comment); Quest-class donors have no official firmware archive (sibling doc `07-device-landscape.md`). Every donor artifact needs a content-addressed mirror under spatial-os control before it is depended upon.
7. **mkosi as a build backend**: its acquisition layer is unpinned by design (`CacheOnly=` freezes a cache, doesn't pin inputs); adopting it would re-solve problems Nix already solves and un-solve the ones Nix solves better. Benchmark only.
8. **Manual hash-bump maintenance at fleet scale** (Jovian's constants-in-file pinning, `pkgs/linux-jovian/default.nix`): fine for one device/one vendor; with 6+ headsets × {kernel, firmware, blobs, extractor}, adopt lockfile automation (adopt-item 2) or drown in bump commits.
9. **Untyped `build = types.attrs` output grab-bags** (robotnix `modules/base.nix`): made robotnix outputs undiscoverable/undocumented. Expose typed, documented options and named flake outputs instead.
10. **Eval-time filesystem probing as the default donor-firmware path** (apple-silicon's `builtins.pathExists /boot/vendorfw` default): keep the mechanism, but spatial-os should default to the pure path (explicit option) and make the impure autodetection the opt-in installer convenience, not vice versa — remote builds and CI break otherwise.

---

## 11. Open questions

1. **Android-boot-chain image assembly without an AOSP build.** robotnix gets `boot.img`/`super.img`/`vbmeta` from otatools *inside* an AOSP build. spatial-os wants mainline-Linux payloads in Android boot formats (Quest 1 / Lynx class): is a repart-style declarative Nix wrapper over standalone `mkbootimg`/`avbtool`/`lpmake` the right shape for `lib/images/android-bootchain.nix`? (Non-Nix precedent to study next: `references/pmbootstrap` / `pmaports` boot-image machinery; Halium's `halium-generic-adaptation-build-tools`.)
2. **A/B + verity vs. mutable `/nix/store`.** `repart-verity-store.nix` assumes an immutable-store appliance, conflicting with on-device `nixos-rebuild`. Which spatial-os targets are appliances (sysupdate A/B + verity + UKI) vs. mutable NixOS (Frame dev image)? Can one device offer both channels from one module tree without divergence?
3. **Steam Frame donor specifics.** Jovian consumes Deck channels (`steamdeck-packages.steamos.cloud`) and Deck hw-support repos; Frame equivalents exist (`holo-images.steamos.cloud/vr/` rauc/casync per `07-device-landscape.md`) — is there a Frame analog of `jupiter-hw-support`, and is Valve's Frame kernel tree the same `Jovian-Experiments/linux`-mirrored lineage (its config already carries `RTW89_*` "for Steam Frame")? Does Jovian intend Frame support, and should spatial-os build on Jovian's modules or fork the layout?
4. **Lockfile tool scope.** Extend repo2nix (Rust, git-repo-manifest-specific) or build a spatial-os fetch tool covering git manifests *and* firmware URL sets *and* OTA/rauc payload indexes? What single lockfile schema covers donor types as different as a Pixel factory zip, a rauc bundle, Samsung FUS firmware, and a GPL kernel drop?
5. **Impurity policy for on-device donor extraction.** Mandate a two-phase flow (extract on device → register hash/`requireFile` → pure build) even at installer-UX cost, or allow apple-silicon-style eval-time path probing on the device itself? Related: can extraction be a *runtime* (boot-time) step, as Asahi's docs foreshadow (`modules/peripheral-firmware/default.nix` option description), keeping images pure and firmware out of the store entirely?
6. **Cache topology and redistribution boundaries.** apple-silicon and robotnix both needed public caches (Cachix) for kernels/Mesa/Chromium-class builds. Which spatial-os artifacts are cacheable publicly vs. legally non-redistributable (extracted vendor firmware, adevtool-style outputs) and must be excluded from caches — and does that force a private-cache tier per device?
7. **Cross vs. native aarch64 builders.** repart assembles foreign-arch images without binfmt, but the aarch64 NixOS closures themselves must be built. Commit to full cross-compilation (apple-silicon proves installer-grade cross works; ongoing maintenance cost) or require native aarch64 builders (Frame itself? Apple Silicon machines running nixos-apple-silicon?) — or binfmt qemu-user on x86_64 farms (slow, occasionally wrong)?
8. **Upstreaming as exit strategy.** Which spatial-os components have a credible nixpkgs/mesa/kernel upstream path (the apple-silicon endgame: everything but kernel+U-Boot dropped, `docs/release-notes.md` 2025-11-18) vs. permanent downstream (vendor blobs, per-device quirks)? Repo layout should separate overlay-per-component from day one so pieces can be deleted as they land upstream.
9. **Multi-device eval cost.** robotnix re-evaluates its whole module system per device; nixpkgs `images.nix` shares a base eval across variants via `extendModules`. For a device × variant matrix, where should spatial-os put the shared-eval boundary (per-device base config extended per image variant?), and does `specialArgs`-injected device metadata (Jovian style) or lockfile-category selection (robotnix style) scale better?
