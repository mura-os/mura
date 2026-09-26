# 33 — Steam Frame donor: reconstruction audit, system inventory, pre-hardware inference

**Status:** research complete (first real donor through the pipeline). **Date:** 2026-09-23.
**Subject:** the reconstructed `deckard-20260921.6090922-0.5.0` update payload at
`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/` (git-ignored;
recipe: `references/archive-steam-frame/archive-steam-frame.sh`). Feeds
[devices/valve-steam-frame/donor.nix](../../devices/valve-steam-frame/donor.nix),
[07-device-landscape](07-device-landscape.md), and the uefi-rauc image family. Extracted small
artifacts (kconfig, DTS, scripts, inspection transcripts) persist beside the image under
`extracted/` in the archive directory.

## 1. Reconstruction audit

| Fact | Value |
|---|---|
| Source bundle | `https://holo-images.steamos.cloud/vr/20260921.6090922/deckard-20260921.6090922-0.5.0.raucb` (+ `.castr` chunk store) |
| Payload | one casync index `rootfs.img.caibx` → `images/rootfs.img`, 10 GiB logical, ~4.4 GiB used, sparse |
| Filesystem | **btrfs** (UUID `8d9f6ea7-24ef-40ee-a697-f3e1a5599148`) — not ext4 |
| Integrity | `sha256(rootfs.img) = 5c53ff2ed7dc…a0361e6a8` — **exact match** with `manifest.raucm [image.rootfs] sha256` (byte-verified reconstruction) |
| RAUC manifest | `compatible=steamos-aarch64`, `version=20260921.6090922`, bundle `format=plain` |
| Identity | os-release: SteamOS `holo`, `VARIANT_ID="vr"`, `VERSION_ID=0.5.0`, `BUILD_ID=20260921.6090922` |
| Redistribution | mixed content (GPL kernel + Qualcomm blobs + Valve userspace): **`localOnly`** per overview invariant 3 |

**Open item — bundle signature.** `rauc info --keyring …` fails with *"Verify error: self-signed
certificate"* against all three available trust anchors: the archived `stable_keyring.pem`, the
archived `insecure_keyring.pem`, and the device's own trust root
`/etc/rauc/trusted_keys/ba942c60.0` (= the 2021 Valve `steamdeck-images` CA, also used on Deck).
The payload hash match makes reconstruction integrity independent of this; the signature question
matters only for *consuming* Valve updates (their chain may involve an intermediate or
channel-specific signer). Recorded, not blocking.

## 2. Partition/slot layout — what "flashable" means (authoritative)

`/etc/rauc/system.conf` (verbatim in `extracted/system.conf`):

- `[system] compatible=steamos-aarch64`, **`bootloader=custom`** with handlers
  `bootloader-custom-backend.sh` (= steamos-customizations' `steamos-bootconf`, Collabora/Valve
  LGPL — same as Deck), `pre-install.sh`, `post-install.sh`.
- `[casync] use-desync=true`, in-place install with `--seed /var/lib/steamos-atomupd/rootfs.caibx`
  — updates are desync-seeded in-place writes into the inactive slot, `type=raw`.
- Slots: `[slot.rootfs.0] bootname=A device=/dev/disk/by-partsets/A/rootfs type=raw`; likewise
  `rootfs.1 → B`. **`by-partsets`** is SteamOS's udev partition-grouping scheme (groups `A/`,
  `B/`, `shared/`, `self/`).

`/etc/fstab` adds the rest of the GPT: `syspersist` (PARTLABEL, ext4, **ro**), per-slot
`efi` (vfat, `by-partsets/self/efi`), shared `esp` (vfat), shared `home` (ext4,
`x-systemd.growfs`); `/var` and `/etc` mounts are handled in the initrd (SteamOS overlay
pattern). So the full disk is: `esp` (shared) + per-slot `efi`×2 + `rootfs`×2 + `syspersist` +
`home` — the Steam Deck layout on aarch64.

## 3. Boot chain — U-Boot, not stock UEFI

`/boot` contents (list in `extracted/batch1.txt`): kernel `Image` (+ versioned copy),
`initrd.uImage`, `maindtb.dtb`, full `dtbs/` tree, `bootfw.tar.xz`, **U-Boot images per board
revision** (`uboot_{dv1,dv2,ev3,mp}.img` + `ubootfw_*`), `uboot.env`, `ubootrelease.txt` =
**`2025.07-rc3-00634-gbb0a2a01cb6d`** (U-Boot 2025.07-rc3 + 634 Valve/vendor commits), and
`kernelsetup.sh`.

`kernelsetup.sh` (full text in `extracted/`) is the bootloader/firmware update hook and encodes
the boot contract:

- The booted slot is read from the kernel cmdline: **`rauc.slot=A|B`** — the exact cmdline token
  our images must reproduce for slot-aware userspace.
- Boot firmware is itself A/B: active bootfw slot via `splctl get-bootfw-slot`, falling back to
  the GPT *legacy BIOS bootable* attribute on partitions 1/2 of **`/dev/sdb`** (the boot LUN —
  UFS exposes multiple LUNs as separate block devices), and finally to a Qualcomm-specific
  parttype search (`dea0ba2c-cbdd-4805-b4f9-f428251c3e98` = the parttype the SM8650 ROM PBL
  scans for XBL_SC), with `_a`/`_b` partlabel suffixes.
- Chain: Qualcomm PBL → XBL/XBL_SC (A/B, on the boot LUN) → **U-Boot** (per-board image) →
  kernel `Image` + `maindtb.dtb` + `initrd.uImage` from the rootfs `/boot`. There is a per-slot
  `efi` partition, but the primary path is U-Boot script/env driven, not systemd-boot.

Implication for the image family: the name "uefi-rauc" is imprecise for Frame — the *slot and
update semantics* (RAUC, A/B raw slots, desync) transfer exactly, but the boot payload
convention is U-Boot + `/boot`-in-rootfs + per-slot `efi`. Our VM image keeps a clean
UEFI/systemd-boot path (QEMU) while matching the GPT/slot/partlabel/cmdline contract; the
on-device boot payload (U-Boot env/scripts) is a later, device-side step.

## 4. Kernel — verified facts

| Fact | Value |
|---|---|
| Version | `6.18.0-gbfea53e51a5d` (6.18 LTS base) |
| Package (`pkgbase`) | **`linux-618-deckard`** |
| Binary package repo (public) | `https://holo-packages.steamos.cloud/archlinux-deckard-hotfixes/mr-1102/` — `linux-618-deckard-6.18.0+g0406308f87f6-1-aarch64.pkg.tar.zst` (+headers, +v4l2loopback; newer commit than the donor's) |
| Source tarball | **not located publicly** (2026-09-23): no `deckard` directory under `steamdeck-packages.steamos.cloud/archlinux-mirror/sources/` (checked root + `holo-main`); the evlaV community mirror carries jupiter/neptune only. The Deck precedent (`linux-neptune-*` src tarballs with full git history) says Valve publishes per-release — a GPL source request / repo-appearance watch is the follow-up. Adjacent: `jupiter-3.8` has `linux-neptune-618-*` (x86 Deck, same 6.18 lineage). |
| Config | **extracted via IKCONFIG** from `/boot/Image` → `extracted/config-6.18.0-deckard` (8,350 options; GCC 15.1.1) |
| Storage | `CONFIG_SCSI_UFS_QCOM=y`, btrfs built-in |
| **Virtio** | core built-in (`virtio`, `virtio_ring`, virtiofs, vsock) but **`CONFIG_VIRTIO_BLK` and `CONFIG_VIRTIO_NET` are not set** and no loadable virtio modules exist |

The virtio finding kills the "boot the donor kernel in QEMU" mode (plan acceptance item 6):
Valve's kernel cannot see a virtio disk or NIC. Interesting corollary: virtiofs+vsock present
suggests Valve's own dev workflow runs the *userspace* under virtualization with a different
kernel, exactly the pattern we adopt.

## 5. Devicetree — the board map, pre-hardware

The donor ships the full qcom DTB set for 6.18 including **eight deckard board revisions**:
`sm8650-dv1`, `dv2(+power)`, `ev1(+mte/psci)`, `ev2(+mte/power)`, `ev3(+power/3d/3dplano)`,
`mp(+dbg/power)` — dev/EV/mass-production generations. `/boot/maindtb.dtb` is byte-identical to
`sm8650-mp.dtb` (mass production). Decompiled (`extracted/sm8650-mp.dts`):

> `model = "SM8650 MP rev1 4slam 2et"` — **4 SLAM cameras + 2 eye-tracking cameras declared in
> the model string**; `compatible = "qcom,sm8650-dv1", "qcom,sm8650"` (MP retains the dv1
> compatible); a `simple-framebuffer` chosen node exists (boot splash path).

The DTS is the per-device adaptation bundle's requirements document. Its panel/DRM, world-camera,
radio, power/thermal and speaker paths are now mined in
[46](46-display-panel-drm-native-linux-audit.md)–[50](50-speaker-output-audio-native-linux-audit.md);
those audits preserve static/runtime boundaries rather than promoting the donor to support. Doc
07's "no public Galaxy-class DTS" caveat does not apply here: **the Frame's production DTS is in
the donor.**

## 6. Userspace inventory

- **No pacman database** in the deployed image (`/var/lib/pacman` absent) — the image is a
  composed artifact, not a package-managed system; the SBOM must come from Valve's package repos
  per-release rather than the image. (Adjusts the plan's assumption.)
- **XR stack:** `steamvr` + `gamescope`/`start-gamescope-session` binaries; SteamVR-specific
  units (`steamvr-program-ble`, `steamvr-set-kernel-thread-priorities`, `steamvr-v4l2loopback`,
  `steamvr-web-debug-portforward`). No Monado. The XR runtime is Valve-proprietary — reinforcing
  that our value-add is the open stack, and that `monado-galaxyxr`-style bring-up work has no
  Valve-side equivalent to reuse.
- **Update/system services:** `rauc.service` (+drop-in), `atomupd.service`
  (steamos-atomupd + desync config), `steamos-manager`, `steamos-boot`, OOBE/devkit/log-submitter
  units — the Deck service architecture.
- **Firmware:** standard linux-firmware tree plus `qcom/sm8650/` (SoC blobs) and `qcom/vpu/`;
  `CAMERA_ICP.mbn` at top level (camera ICP firmware).

### 6.1 Audio capture chain — static donor evidence

The reconstructed donor contains the only end-to-end, product-specific Linux microphone
configuration currently available for a Mura target. This is **static closure**, not a
headset-generated capture result; the qualification states and cross-target comparison are in
[43 §7 and §10](43-microphone-native-linux-capture-audit.md).

**Hardware/kernel join:**

- Valve specifies a dual-microphone array. The production DT uses
  `qcom,sm8650-lpass-va-macro` at a 2.4 MHz DMIC clock
  (`extracted/sm8650-mp.dts:3734-3747`).
- The board sound card is `qcom,lpass-sndcard`, model **`SM8250 LPASS`** — a compatibility card
  name on an SM8650 device, not evidence of an SM8250 SoC. Its `VA Capture` link terminates at the
  VA macro (`extracted/sm8650-mp.dts:9434-9468`).
- The extracted config enables QDSP6, Q6V5 ADSP remoteproc, Qualcomm SoundWire, and LPASS
  VA/RX/TX/WSA macros (`extracted/config-6.18.0-deckard:4872-4890,5175-5180,6182-6214`).
  WCD937x/938x/939x is disabled: the built-in digital microphones do not use an external WCD
  capture codec. MAX98390 is the playback amplifier.

**Firmware/topology in the rootfs** (read-only `guestfish` inspection):

```
/usr/lib/firmware/qcom/sm8650/adsp.mbn
/usr/lib/firmware/qcom/sm8650/adsp_dtb.mbn
/usr/lib/firmware/qcom/sm8650/SM8650-MTP-tplg.bin
/usr/lib/firmware/qcom/sm8650/SM8650-QRD-tplg.bin
```

**ALSA/UCM:**

- `/usr/share/alsa/ucm2/conf.d/SM8250_LPASS/{SM8250_LPASS,HiFi}.conf` maps speaker playback to
  `hw:${CardId},0` and the `Mic` device to capture `hw:${CardId},1`.
- `/usr/share/alsa/ucm2/codecs/sm8250-lpass/VAEnableSeq.conf` routes DEC0/DEC1 to DMIC0/DMIC1,
  enables those two AIF capture mixers, and zeros DMIC2/DMIC3.
- `deckard-audio-setup.service` runs
  `/usr/share/deckard-audio-config/soundsetup.sh`, clears stale WirePlumber mic-route state,
  chooses product speaker tuning, and links the microphone filter chain.

**PipeWire/WirePlumber:** Valve's public
[external] [`deckard-audio-config-20260914.1-1`](https://holo-packages.steamos.cloud/archlinux-deckard-hotfixes/deckard-audio-config-20260914.1-1-any.pkg.tar.zst)
defines a 48 kHz stereo S16LE built-in capture source. The current product chain applies a
two-channel Deckard LV2 EQ, downmixes to mono, applies WebRTC AEC3/gain control, and then uses
SteamVR's proprietary `audiofilter.so` for noise suppression. The kernel/UCM/PipeWire/WebRTC
portion is reusable open mechanism; the Valve EQ and noise-suppression binaries remain
donor/reference components with a separate redistribution decision.

SteamOS 0.3.0 later fixed a vendor-described “garbled audio” microphone bug
([external] [Valve patch notes](https://store.steampowered.com/news/app/4165890/view/711161056325533925)).
That proves the stock path was exercised, but the archive contains no `arecord -l`,
`/proc/asound/pcm`, `wpctl status`, successful WAV, physical channel map, or suspend/resume
result. Mura therefore records two physical/raw channels and one stock processed application
channel as separate facts and leaves `mura.xr.sensing.micChannels = 0` until runtime R4.

## 7. Reconciliation verdicts (donor-pipeline stages)

| Stage | Verdict |
|---|---|
| acquire | Done and reproducible (`archive-steam-frame.sh`, public URLs, original `.raucb`+`.castr` retained beside the reconstruction) |
| identify | Done — manifest + os-release + system.conf (this doc) |
| parse/extract | Done for phase-1 needs (configs, kernel, DTS, inventories); deeper extraction (firmware closure, initrd contents) deferred to adaptation work |
| qualify | Payload hash verified against Valve's manifest; bundle-signature chain **open** (§1); `redistributable=false → localOnly` |
| gaps | No GPT/ESP/recovery *bytes* in the payload (expected — it's a slot image); the *layout* is fully specified by §2–§3, so image structure is not blocked. Flash procedure (Download/recovery path) remains device-side future work. |

## 8. What this changes in the plan

1. Filesystem is btrfs (not ext4) — our slot payloads should also be btrfs for maximal
   layout fidelity (and `type=raw` slots don't care beyond that).
2. "uefi-rauc" family: slot/update semantics confirmed; boot payload is U-Boot-shaped on device.
   VM image boots UEFI/systemd-boot (QEMU-clean) while matching GPT partlabels/partsets, A/B raw
   slots, `rauc.slot=` cmdline, and the RAUC compatible-string convention.
3. Donor-kernel-in-QEMU: **infeasible** (no virtio blk/net). VM proof runs on a Nix-built kernel;
   the device-kernel path waits on the `linux-618-deckard` source (watch/GPL-request).
4. First-donor lessons for the pipeline are collected in §7 and §10.

## 9. The VM proof (executed 2026-09-23) — acceptance evidence

Built via nixbuild.net remote aarch64 builders (ADR 0004), run in `qemu-system-aarch64` (TCG,
edk2 firmware) on the x86_64 dev host via `nix run .#frame-vm-run`:

| Check | Result |
|---|---|
| Image | `packages.aarch64-linux.frame-image`: 33.6 GiB sparse GPT (zstd artifact ~2 GiB after the dedicated recovery partition); remote build via `nix run .#frame-build` |
| Boot | UEFI → systemd-boot → slot A; `multi-user.target` + `graphical.target` active; autologin session; sshd up |
| Layout | `esp / mura_recovery / rootfs_a / rootfs_b / syspersist / home`; `mura_recovery` is a separate 260 MiB XBOOTLDR image (63.8 MiB compressed) — systemd-repart's VFAT/4K-sector minimum and a hard build cap — carrying stable `recovery.conf` + one measured 103 MiB self-contained recovery UKI (kernel+initrd+cmdline, including USB/hotspot services), registered as readonly RAUC `rescue.0` so ordinary A/B bundles leave it untouched; cmdline `root=PARTLABEL=rootfs_a rauc.slot=A` (the donor's slot-cmdline contract, §3) |
| RAUC | `compatible=Mura-deckard`, booted `rootfs.0 (A)`, custom backend (`mura-bootconf`) reporting primary correctly |
| XR wiring | `monado.socket` active (user unit); `/etc/mura-device.json` correct |
| Update round-trip | test-signed 2.1 GiB `.raucb` → `rauc install` into slot B (**4 m 42 s** with the VM disk on fast local SSD; effectively unbounded on slow storage — see lessons), backend flipped primary to B, reboot → **booted `rootfs.1 (B)`** with `root=PARTLABEL=rootfs_b rauc.slot=B`, `rauc status mark-good` → both slots good |
| Recovery escalation (2026-09-25) | Test-only image forces P2 hard + threshold 2: first failure cycles, second writes LoaderEntryOneShot, third boot selects `recovery.conf` → recovery UKI from XBOOTLDR; stage 1 reports `RECOVERY_PROOF_OK`, five recovery units active, the selected entry, and embedded `rd.systemd.unit=mura-recovery.target`. edk2 proof only; Frame U-Boot runtime-variable persistence remains hardware-only |
| Donor-kernel boot mode | **not executed — infeasible by evidence** (§4: no `VIRTIO_BLK/NET` in Valve's kernel) |

Precise scope of the "technically flashable" claim: the artifact reproduces the donor's GPT
partlabel scheme, A/B raw-slot RAUC semantics, and slot-cmdline contract, and demonstrably
updates A→B. On-device flash additionally requires the U-Boot boot payload (§3), the device
kernel (§4), and the not-yet-obtained flash/recovery procedure — the hardware-only residual.

## 10. Lessons learnt (what the first device port teaches the next five)

**Pipeline (donor stages):**
- *Reconstruction verification is two distinct facts*: payload-hash-vs-manifest (achieved,
  load-bearing) and bundle-signature-chain (open; single-cert keyrings + `rauc info` were not
  enough — budget time for CMS chain archaeology per vendor, and don't block on it).
- *Root-free inspection works end-to-end*: `libguestfs-with-appliance` (guestfish batches) +
  IKCONFIG extraction + `dtc` decompile covered everything; no sudo, no loop mounts. One sharp
  edge: guestfish aborts the whole batch on a failed glob — keep batches small, transcripts
  persisted (`extracted/batch*.txt`).
- *Deployed images may carry no package DB* (no `/var/lib/pacman`): plan SBOMs from vendor
  package repos per-release, not from the image.
- *The maximal-inference checklist generalizes*: system.conf/fstab (layout) → boot scripts
  (chain + cmdline contract) → `pkgbase`+IKCONFIG (kernel identity/config) → DTBs (`dtc`; the
  model string alone encoded camera counts) → unit graph + firmware tree. For Android-family
  donors (Lynx/Quest/Galaxy XR) the equivalents are `boot.img`/`vendor_boot` headers, fstab +
  `by-name` partitions, DTBO, and the vendor manifest — same questions, different containers.
**Image family:**
- nixos-unstable's `image/repart.nix` is **not** in the default module list (docs-only
  `extraModules`) and is gated on `image.repart.enable` — both bit us.
- **ESP mount-point correction, from the recovery-entry proof.** The first family draft copied
  the donor's shared ESP mount at `/esp`, but not the donor's reason: SteamOS has *both* the
  current slot's separate `efi` partition at `/efi` and a shared `esp` at `/esp`
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/batch1.txt:32-35`);
  its updater reads the current slot's partsets through `/efi` and writes shared
  `steamos-bootconf` records through `/esp` (the same archive's
  `extracted/batch2.txt:779-806`). Mura has one ESP carrying systemd-boot, entries, kernel and
  initrd — no per-slot EFI partition — so that split does not transfer. Comparable positions:
  pinned systemd discovers ESPs only at `/efi`, `/boot`, `/boot/efi`
  (`references/systemd/src/shared/find-esp.c:479-497`) and auto-mounts a lone ESP at `/boot`,
  or at `/efi` when `/boot` is XBOOTLDR
  (`src/gpt-auto-generator/gpt-auto-generator.c:678-699`); NixOS and Jovian retain `/boot`
  ([external, pinned `nixpkgs` flake input]
  `nixos/modules/system/boot/loader/efi.nix:11-15`;
  `references/jovian-nixos/pkgs/jupiter-hw-support/firmware.nix:18-41`). The systemd project's
  image-builder mkosi is the exact comparable: it deliberately creates `/efi` for the ESP,
  reserves `/boot` for XBOOTLDR, and passes those paths to bootctl
  (`references/mkosi/mkosi/__init__.py:283-290`, `bootloader.py:749-760`) — no nested mounts and
  a clean `/boot` XBOOTLDR boundary, now used by Mura Recovery. That reason transfers, so Mura uses `/efi`; the choice is
  systemd/mkosi's semantic layout, not a Mura convention. `/esp` also made
  `systemctl --boot-loader-entry` unable to enumerate `recovery.conf`.
- **Boot assessment needs `preferred`, not `default`.** systemd-boot deliberately checks
  `tries_left != 0` for `preferred`, but resolves `default` without assessment
  (`references/systemd/src/boot/boot.c:1851-1863,1904-1945`). A `default a*.conf` therefore
  reselected exhausted A instead of falling back. NixOS's pinned builder uses the upstream
  pattern when boot counting is enabled: assessment-aware `preferred <primary>`, then a broad
  `default` fallback ([external, pinned nixpkgs]
  `nixos/modules/system/boot/loader/systemd-boot/systemd-boot-builder.py:297-314`). Mura's
  two-slot translation writes `preferred a*.conf` + `default b*.conf` (or the reverse).
  `packages.aarch64-linux.frame-bootconf-test` exercises both a plain entry and an exhausted
  `+0-N` entry, asserting re-arm plus the opposite-slot fallback.
- **Recovery is one UKI, not loose boot files.** systemd-boot scans Type #2 UKIs on XBOOTLDR
  (`references/systemd/man/systemd-boot.xml:31-49`), and NixOS's pinned `system.build.uki`
  uses ukify to bind kernel, initrd, cmdline and OS metadata into one PE artifact ([external,
  pinned nixpkgs] `nixos/modules/system/boot/uki.nix:78-116`). This removes the mixed-version
  kernel/initrd window and makes the dedicated-image boundary real. RAUC's Additional Rescue
  Slot is readonly: normal bundles cannot target it. Mutable recovery is a later design requiring
  versioned whole-UKI staging; raw overwrite of the sole rescue partition is not atomic.
- **Recovery partition size is 260 MiB, derived rather than copied.** systemd-repart enforces a
  260 MiB minimum for VFAT ESP/XBOOTLDR at 4 KiB filesystem sectors
  (`references/systemd/src/repart/repart.c:115-122,1194-1197`). The built UKI is 103 MiB, leaving
  157 MiB growth budget. `SizeMaxBytes=260M` makes growth beyond that budget fail at build time;
  readonly recovery needs no in-place update staging space. The recovery-specific extended
  module evaluation also keeps its hostapd/sshd/menu/web closure out of normal boot: the normal
  initrd measured 37 MiB versus 41 MiB before the split.
- Closure size is the slot-size driver: the VM-proof userspace closed at **12.1 GiB** (16 GiB
  slots as a result). Before any real update channel, closure slimming is mandatory
  (docs/man pages, firmware pruning, sway-vs-zxr) — follow-up, not blocking.
- Sparse rawness does not survive NAR copies from remote builders: enable
  `image.repart.compression` (zstd) or pay a 33 GiB transfer for a 1.9 GiB image.
- `rauc bundle` needs `squashfsTools` explicitly; heredocs inside `runCommand` strings are a
  trap (indented terminators) — use `writeText`.
**Update flow:**
- RAUC health-gating needs a *mark-good service* on boot (we ran `rauc status mark-good`
  manually); wire it to the `mura.qualification.readinessCheck` per
  [images-and-updates.md](../architecture/images-and-updates.md) — now a concrete TODO with a
  proven substrate.
- RAUC refuses block devices as bundles ("not a regular file") and qemu pads attached raw files
  to block size — copy the exact byte count out before installing when sideloading via a disk.
- Slot-write speed is storage-bound under TCG: the same install was >60 min on slow media and
  4m42s on local NVMe — keep VM working copies on fast disk (`FRAME_VM_DIR`).
**Build workflow:**
- nixbuild.net remote-builder mode works as designed for this (outputs needed locally); 100
  parallel SSH connections hit drops — 16 is stable; `builders-use-substitutes` is essential so
  the builder pulls aarch64 closures from cache.nixos.org directly.
- The repository-default invocation is `nix run .#frame-build`: a host-side wrapper around the
  standard Nix remote-builder flags above. It uses the invoking user's SSH key and `known_hosts`;
  it needs no `/etc/nix`, nix-daemon or root SSH configuration. Its default target is
  `packages.aarch64-linux.frame-image`; another target and ordinary `nix build` flags follow
  `--`.
- nixbuild caches *failures* per drv-hash (a rebuilt-unchanged failing drv returns the cached
  failure instantly — good for cost, surprising the first time), and build logs need the
  build-key's permissions (plan key permissions accordingly).
**Justified generalization (the standing-rule gate, satisfied for this family):** the uefi-rauc
family is now spike-proven; extracting shared pieces (mark-good service, bundle builder, the
bootconf backend) into reusable modules is licensed *for this family*. The Android-family
machinery remains gated on the Lynx spike as before.
