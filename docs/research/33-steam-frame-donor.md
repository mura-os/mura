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

The DTS is the per-device adaptation bundle's requirements document (camera buses, panels,
regulators, DSP nodes) — to be mined further when adaptation work starts. Doc 07's "no public
Galaxy-class DTS" caveat does not apply here: **the Frame's production DTS is in the donor.**

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
| Image | `packages.aarch64-linux.frame-image`: 33 GiB sparse GPT (zstd artifact 1.9 GiB); remote build ~23 min end-to-end |
| Boot | UEFI → systemd-boot → slot A; `multi-user.target` + `graphical.target` active; autologin session; sshd up |
| Layout | `lsblk` partlabels exactly `esp / rootfs_a / rootfs_b / syspersist / home`; cmdline `root=PARTLABEL=rootfs_a rauc.slot=A` (the donor's slot-cmdline contract, §3) |
| RAUC | `compatible=Mura-deckard`, booted `rootfs.0 (A)`, custom backend (`mura-bootconf`) reporting primary correctly |
| XR wiring | `monado.socket` active (user unit); `/etc/mura-device.json` correct |
| Update round-trip | test-signed 2.1 GiB `.raucb` → `rauc install` into slot B (**4 m 42 s** with the VM disk on fast local SSD; effectively unbounded on slow storage — see lessons), backend flipped primary to B, reboot → **booted `rootfs.1 (B)`** with `root=PARTLABEL=rootfs_b rauc.slot=B`, `rauc status mark-good` → both slots good |
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
- nixbuild caches *failures* per drv-hash (a rebuilt-unchanged failing drv returns the cached
  failure instantly — good for cost, surprising the first time), and build logs need the
  build-key's permissions (plan key permissions accordingly).
**Justified generalization (the standing-rule gate, satisfied for this family):** the uefi-rauc
family is now spike-proven; extracting shared pieces (mark-good service, bundle builder, the
bootconf backend) into reusable modules is licensed *for this family*. The Android-family
machinery remains gated on the Lynx spike as before.
