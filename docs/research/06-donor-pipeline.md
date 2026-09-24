# Donor firmware ingestion pipeline for Mura

**Research date:** 2026-09-22

## 1. Purpose

Mura targets standalone VR headsets whose boot chains, kernels, and hardware-enablement
blobs are only available inside official vendor firmware images. The build system must therefore
treat those vendor images — "donors" — as pinned build inputs: acquired reproducibly, verified by
hash, dissected into named artifacts by pure derivations, and consumed by image-assembly
derivations that are gated behind explicit human review. This document surveys three concrete
prior-art sources available locally (the project owner's TRIMUI Brick appliance, a Valve Steam
Frame RAUC/casync release archive, and robotnix's Pixel vendor-blob ingestion), catalogs the
Android image-format zoo a donor importer must handle, analyzes the metadata-preservation
problem the Nix store creates, and proposes a concrete donor-import contract. It feeds directly
into the donor-pipeline part of the Mura build architecture; section 5 is the
implementable core.

Terminology: a **donor** is a complete official vendor firmware artifact (factory image, OTA,
RAUC bundle, SD-card image) pinned by hash; an **artifact** is a named output extracted from a
donor (partition image, kernel `Image`, DTB, firmware file, vendor library); a **contract** is
reviewed, human-authored data recording verified facts about a donor that authorize its use in
flashable outputs (from the brick appliance's "boot contract").

## 2. Prior art studied

### 2.1 The brick appliance (local, detailed walkthrough)

Path: a local, unpublished reconstruction checkout (`m8tracker-re/reconstruction/frontends/brick/appliance`, not in this repository). A Nix-built
appliance for the TRIMUI Brick (Allwinner A133) handheld that uses a published KNULLI SD image as
donor. It is the most directly transferable prior art because it solves the same problem shape:
a non-redistributable vendor image supplies the boot chain, and Nix supplies the userspace.

#### 2.1.1 `profile.nix`: all trust decisions in one reviewed data file

`profile.nix` is a plain attrset holding board parameters and — critically — every hash and every
approval gate:

- Donor pinning: `donorName = "knulli-a133-trimui-brick-scarab-20260511.img"` with an SRI
  `donorHash`, plus the *publisher's* compressed-file SHA-256 recorded separately
  (`donorCompressedSha256`), so the provenance chain from the vendor download to the imported
  uncompressed file is explicit (`profile.nix` lines 10–15).
- A separate `runtimeArchiveName`/`runtimeArchiveHash` for a locally-assembled, audited vendor
  runtime subset (see 2.1.5).
- Fail-closed gates: `bootContract = null` with the comment "Set to ./boot-contract.json ONLY
  after reviewing the donor initramfs", `deviceProfile = null`, and `cleanImageApproved = false`
  (`profile.nix` lines 17–24). Image outputs simply do not exist until these are set.
- Source-derived defaults are still verified against the donor: `compression = "zstd"` is chosen
  because KNULLI's A133 board config selects zstd, but the comment notes "the image tool still
  verifies the actual donor rootfs before authorizing an image, so this source-derived default
  cannot silently approve a mismatch" (`profile.nix` lines 5–8).
- The clean-image path pins every reproducibility-relevant identifier as data: partition sizes,
  `cleanDiskUuid`, and a `cleanPartitionUuids` attrset with one fixed UUID per partition
  (`profile.nix` lines 25–33).

#### 2.1.2 `nix/default.nix`: requireFile pinning and staged output gating

Non-redistributable inputs enter through `requireFile`, never `fetchurl`:

```nix
donor = host.requireFile {
  name = profile.donorName;
  hash = profile.donorHash;
  message = ''
    Import the reviewed, uncompressed Brick donor image with:
      nix-store --add-fixed sha256 /path/to/${profile.donorName}
  '';
};
```

(`nix/default.nix` lines 22–31; the runtime archive uses the same pattern at lines 5–14.)
The `message` doubles as operator documentation: the build fails with instructions rather than
attempting a download that would be legally or technically impossible.

Output staging is expressed as null-propagation: `sdImage` evaluates to `null` unless donor,
`bootContract`, and `deviceProfile` are all present; `cleanImage` additionally requires
`sdImage != null && profile.cleanImageApproved` (`nix/default.nix` lines 59–74). The flake's
package set then only exposes `brick-sd-image` / `brick-clean-image` attributes when non-null
(lines 86–90). The effect: `nix flake show` truthfully reflects which outputs are authorized.

#### 2.1.3 `nix/sd-image.nix`: guarded transplant derivation

The first bootable image is a *transplant*: copy the donor byte-for-byte, replace only the rootfs
file inside its FAT boot partition. The derivation runs three validators
(`validate_device_profile.py`, `validate_mapping_coverage.py`, `validate_performance_gate.py`)
before invoking `image_tool.py transplant`, and sets

```nix
preferLocalBuild = true;
allowSubstitutes = false;
```

with the comment "Keep private donor-containing outputs off public caches unless rights are
audited" (`nix/sd-image.nix` lines 4–6). This is a load-bearing detail for Mura: any
derivation whose output embeds donor bytes must not be pushed to a public binary cache.

#### 2.1.4 `nix/clean-image.nix`: from transplant to assembled image

The second stage assembles a fresh image from *extracted, individually-selected* donor artifacts
plus the Nix rootfs, using `genimage`:

- Extraction uses the same guarded tool: `image_tool.py extract ${donor} boot ...`,
  `env`, `env-redund`, and two raw `dd` reads at documented fixed offsets for the Allwinner
  `boot0` SPL region (offset 131072) and the packed boot package (offset 16793600)
  (`nix/clean-image.nix` lines 33–40).
- The donor's boot logo is pulled out of the FAT partition with `mcopy` at an offset *measured*
  from the donor's own GPT (`image_tool.py inspect` output), not assumed (lines 42–52).
- Reproducibility is engineered, not hoped for: `SOURCE_DATE_EPOCH`, `E2FSPROGS_FAKE_TIME`,
  ext4 `-U <fixed uuid> -E hash_seed=<fixed>,lazy_itable_init=0,lazy_journal_init=0`, fixed FAT
  volume ID `-i 4D385442`, fixed GPT `disk-uuid`, and `gpt-location = 81920` matching the vendor
  layout (lines 61–125).
- The output is re-inspected by the same tool to produce `build-report.json` (lines 131–136).

#### 2.1.5 `tools/image_tool.py`: paranoid parsing and verified preservation

The image tool (324 lines) is the core safety component:

- **Refuses devices**: `require_regular()` rejects anything but regular files — "Only regular
  image files are accepted, not devices" (`tools/image_tool.py` lines 52–55). The build system
  literally cannot write to a block device; flashing is a separate human act.
- **CRC-checked GPT parsing**: validates primary *and* backup GPT header CRCs and entry-array
  CRCs, cross-checks that both copies agree, checks the backup header is at the final LBA,
  rejects out-of-bounds/overlapping/duplicate-named partitions (lines 74–142). Unknown formats
  fail instead of being guessed.
- **Contract enforcement**: `check_contract()` requires exact values for a fixed key set
  (`schema: 2`, `donorInitramfsReviewed: true`, `handoffEntrypoint: "/sbin/init"`,
  `updatePolicy: "initramfs-renames-update-over-rootfs"`, …), requires the contract's
  `donorSha256` to match the actual donor being used, and requires `reviewNotes` of at least 40
  characters — review evidence is structurally mandatory (lines 152–186). It also parses the
  SquashFS superblock of the replacement rootfs and rejects a compressor mismatch against the
  contract (lines 183–198).
- **Preservation verified by read-back**, not by construction: after staging the new rootfs via
  `mcopy` into a copy of the donor, the tool (a) reads the written rootfs back out and compares
  SHA-256 with the input, (b) reads the *preserved donor* rootfs back and compares with the
  pre-copy extraction, (c) re-parses the output GPT and requires equality with the donor's,
  (d) hashes every byte before and after the FAT partition and requires equality with the donor
  (lines 250–268). Any failure deletes the output (lines 269–271). The build report explicitly
  says `"status": "UNTESTED_ON_HARDWARE"` (line 272) — the derivation never overstates what it
  proved.
- Output paths must be *new* files opened with `open('xb')` (exclusive create), never existing
  paths (lines 225–227, 243–245).

#### 2.1.6 `tools/runtime_archive.py`: allowlisted vendor-runtime extraction

Vendor userspace files (kernel modules, firmware, ALSA config, the vendor input daemon) are
imported as a locally-assembled tar, extracted under a strict policy: path-prefix allowlist
(`lib/`, `usr/lib/`, `usr/trimui/`, `etc/alsa/`, …) plus an exact-file allowlist; member-count,
per-file, and total size limits; only dirs/files/symlinks permitted; symlink targets resolved and
required to stay inside the allowlist; refusal to write through symlink parents; and
`os.chmod(out, m.mode & 0o777)` with the comment "Never preserve setuid/setgid from donor"
(`tools/runtime_archive.py` lines 12–20, 27–46, 83–105). The docstring states the intent:
"Extract an audited vendor runtime subset, not a whole third-party rootfs."

#### 2.1.7 Documentation as part of the pipeline

- `docs/boot-contract.example.json` is a filled but deliberately unapproved example
  (`boardRuntimeReviewed: false`) whose `reviewNotes` record the actual initramfs behavior
  discovered by audit: mounts `mmcblk0p4`, renames `/boot/knulli.update` over `/boot/knulli`,
  mounts the zstd SquashFS as lower root under a tmpfs overlay, `switch_root`s to `/sbin/init`,
  and does *not* read `firmware.sig`.
- `docs/sources.md` separates navigation URLs from pinned facts (exact donor asset name +
  publisher SHA-256, pinned KNULLI source tag and commit), and states what each source does and
  does not establish ("A source file named boot.cmd elsewhere is not sufficient proof that this
  is the active path", `docs/architecture.md` lines 64–68).
- `docs/architecture.md` records the ownership boundary as a table (donor BSP components on the
  left, Nix-built responsibilities on the right, lines 20–30), notes that vendor binaries split
  into three independent axes — source availability, ability to rebuild, redistribution
  permission (lines 32–35) — and lists the clean-assembler derivation ladder (`vendorBoot` →
  `bootAdapter` → `runtimeClosure` → `rootfs` → `bootFat` → `dataExt4` → `sdImage` →
  `releaseManifest`, lines 87–100). Its reproducibility guidance is blunt: "rebuild independently
  and compare actual bytes; derivation identity alone is not a proof of reproducible filesystem
  images" (lines 108–111).
- `README.md` documents the operator workflow (`nix hash file`, `nix-store --add-fixed sha256`)
  and honestly flags what is *not* yet reproducible: "mtools may introduce FAT metadata
  timestamps" (README "Reproducibility boundaries" section).

**Reusable patterns extracted**: (1) `requireFile` with instructive `message` for
non-redistributable inputs; (2) all hashes and approval gates in one reviewed data file;
(3) contract-as-data with mandatory review notes, hash-bound to the exact donor; (4) staged
image strategy — byte-preserving transplant first, clean assembly second, each behind its own
gate; (5) preservation properties verified by read-back rather than asserted; (6) tools that
refuse block devices and existing outputs; (7) `preferLocalBuild`/`allowSubstitutes = false` on
donor-containing outputs; (8) allowlisted, size-bounded, setuid-stripping vendor-file extraction;
(9) build reports that state their own limits (`UNTESTED_ON_HARDWARE`).

### 2.2 Steam Frame / SteamOS RAUC+casync archive (local, observed layout)

Path: `references/archive-steam-frame/`. This is an
offline archive of one Valve `deckard` SteamOS VR release, produced by the accompanying
`archive-steam-frame.sh` inside a pinned dev shell (`flake.nix` provides `curl`, `squashfs-tools`,
`desync`, `rauc`, `cacert`).

Observed layout of `frame-archive-deckard-20260921.6090922-0.5.0/`:

- `deckard-20260921.6090922-0.5.0.raucb` — the RAUC bundle, only **2.2 MiB**. It is a SquashFS
  (the script extracts it with plain `unsquashfs`, or `rauc extract --keyring=…` when signature
  verification is requested, `archive-steam-frame.sh` lines 96–111).
- `bundle/` — the extracted bundle: `manifest.raucm`, `rootfs.img.caibx` (3.5 MiB), `UUID`.
- `deckard-20260921.6090922-0.5.0.castr/` — the casync chunk store: **14,701** `*.cacnk`
  compressed chunk files sharded into four-hex-digit prefix directories, 1.2 GiB total.
- `metadata/` — sidecar downloads: `<stem>.manifest.json`, `<stem>.chunks_details.json`,
  `stable_keyring.pem`, `insecure_keyring.pem`, `STORE_URL.txt`, `INDEXES.txt`,
  `SIGNATURE_STATUS.txt` ("Signature authentication not checked."), `build-directory.html`.
- `SOURCE_URL.txt` — `https://holo-images.steamos.cloud/vr/20260921.6090922/deckard-20260921.6090922-0.5.0.raucb`.
- `images/` — empty in this archive; the reconstructed 10 GiB `rootfs.img` was not retained
  (the script's final status/checksum files `ARCHIVE_STATUS.txt` and `SHA256SUMS` are also
  absent, so this run stopped after chunk caching).

The actual `bundle/manifest.raucm`:

```ini
[update]
compatible=steamos-aarch64
version=20260921.6090922

[bundle]
format=plain

[image.rootfs]
sha256=5c53ff2ed7dc78f313a19fc9224aa07e7fb63271b811a4ada295441a0361e6a8
size=10737418240
filename=rootfs.img.caibx
```

Key observations:

- **The bundle contains exactly one image slot: `rootfs`.** No boot, ESP, kernel, or vbmeta
  images are shipped in this update path; the manifest's `sha256`/`size` describe the full
  10 GiB uncompressed rootfs image, while `filename` points at a casync *index*, not the image
  itself. Whatever manages the bootloader/firmware partitions on the device is outside this
  bundle.
- **How reconstruction works**: the `.caibx` index lists chunk IDs; the adjacent `.castr` store
  (URL convention: same stem, `.castr` suffix — `metadata/STORE_URL.txt`) holds
  zstd-compressed chunks addressed by content hash. `desync cache -s <remote> -c <local>
  <indexes>` fetches exactly the referenced chunks; `desync extract -s <local> <index> <image>`
  rebuilds the image; `desync verify-index` re-checks the result against the index
  (`archive-steam-frame.sh` lines 118–149). Per `metadata/…chunks_details.json`, chunks have a
  uniform 262,144-byte uncompressed size and highly variable compressed sizes (a 40-byte
  compressed chunk repeats many times — deduplicated zero/filler blocks). 10 GiB of image
  compresses to 1.2 GiB of unique chunks.
- **Version metadata is a first-class sidecar**: `metadata/…manifest.json` records
  `product: steamos`, `release: holo`, `variant: vr`, `arch: aarch64`, a `gitsha`, `version:
  0.5.0`, `steamvr_version`, and `buildid: 20260921.6090922` — a vendor-published, machine-
  readable donor identity.
- **Signing**: RAUC bundles are CMS-signed; Valve publishes `stable_keyring.pem` and an
  `insecure_keyring.pem` next to the images. The archive script makes verification opt-in and
  refuses to silently promote an unverified extraction to verified status
  (`archive-steam-frame.sh` lines 89–115).

**Implications for Mura**: (1) SteamOS-style donors are the *easiest* class to ingest —
public HTTPS URLs suitable for `fetchurl`, a tiny signed manifest, and chunk-level fetching that
allows caching only what changed between releases; the parse stage is `unsquashfs` (in nixpkgs)
plus `desync` (in nixpkgs) with no Android tooling at all. (2) RAUC+casync is a strong candidate
for Mura's *own* update mechanism: A/B slots via RAUC, `format=plain` bundles carrying
`.caibx` indexes, chunks served from a dumb HTTPS store, device-side seeding from the currently
installed slot. All required tools (`rauc`, `desync`, `casync`) are in nixpkgs, and the bundle
format is simple enough to generate from a Nix-built rootfs image derivation. The main open cost
is deterministic chunking if Mura wants reproducible chunk stores (section 6).

### 2.3 robotnix vendor ingestion (local)

Path: `references/robotnix`. Only the donor/vendor
ingestion machinery is covered here.

- **Acquisition is `fetchurl`, not `requireFile`**: Google factory images are publicly
  downloadable at stable URLs, so `modules/adevtool/default.nix` maps a list of
  `{ fileName, url, sha256 }` records into fixed-output `pkgs.fetchurl` calls and aggregates
  them with `pkgs.linkFarm "vendor-imgs"` (lines 102–111). Redistribution rights are not needed
  to *fetch*; the hash pin makes the fetch reproducible.
- **Per-device, per-release hash records are committed JSON**: e.g.
  `flavors/grapheneos/2026052400/vendor_imgs/cheetah.json` contains
  `[{"fileName":"cheetah-bp4a.251205.006-factory-1577f43b.zip","url":"https://dl.google.com/dl/android/aosp/…","sha256":"1577f43b…"}, …]`
  — one entry per required factory/OTA zip. The flavor imports these with `lib.importJSON`
  (`flavors/grapheneos/default.nix` line 124). Note Google's convention of embedding the SHA-256
  prefix in the factory-zip filename, which robotnix records in full.
- **Extraction is driven by the vendor's own tool (adevtool), pinned like source**: the
  `vendor/adevtool` repo comes from the source manifest lockfile; its yarn dependency tree is
  pinned via `fetchYarnDeps` with a per-release `yarnHash` recorded in `yarn_hashes.json`
  (`modules/adevtool/default.nix` lines 16–22, 86–98; `flavors/grapheneos/default.nix` line 122).
- **Extraction runs inside the main AOSP build derivation, offline**: `modules/base.nix`
  (lines 471–483) symlinks each prefetched image into `/tmp/vendor_imgs`, sets
  `ADEVTOOL_IMG_DOWNLOAD_DIR` to point there, and runs
  `vendor/adevtool/bin/run generate-all --noVerify -d <device>`. `--noVerify` skips adevtool's
  own download verification because Nix already verified the fixed-output hashes. There is no
  separate "vendor blobs" derivation output; the blobs materialize as `vendor/google_devices/…`
  inside the build tree.
- **The extraction toolchain betrays the format handling**: the module adds `e2fsprogs` to
  `envPackages` with the comment "adevtool uses e2fsprogs `debugfs` to extract the vendor ext4
  images" (`modules/adevtool/default.nix` lines 73–79) — i.e. root-less ext4 extraction via
  `debugfs`, the same technique proposed for Mura in section 3/4.
- **Metadata maintenance is automated**: `flavors/grapheneos/update.sh` regenerates lockfiles per
  GrapheneOS tag, prefetches yarn hashes, and runs a patched adevtool
  (`adevtool-show-metadata-json.patch`) in a metadata-only mode that *prints* the
  `{fileName,url,sha256}` JSON for each device instead of downloading (lines 64–84). The pin
  records are thus produced by the same tool that will consume them.

**Reusable patterns**: `fetchurl` + committed per-device JSON hash records for public donors;
pinning the extractor (and its language-ecosystem deps) with the same rigor as the donor;
verify-once-in-Nix then `--noVerify` inside the sandbox; `debugfs`-based unprivileged ext4
extraction. **Anti-pattern for Mura**: extraction buried inside a monolithic build
derivation — robotnix can afford this because the AOSP build consumes the blobs in place, but
Mura wants separately cacheable, separately reviewable artifact derivations.

## 3. The format zoo

Donor artifacts Mura must expect, given the device landscape (Quest 1 `monterey`, Lynx R1,
Galaxy XR, Play For Dream MR — Android-based; Steam Frame — SteamOS; see
`docs/research/07-device-landscape.md`):

| Format / container | Where it appears | Extraction tool(s) | In nixpkgs? |
|---|---|---|---|
| Factory-image ZIP (bootloader.img, radio.img, inner `image-<build>.zip`) | Google-style factory images; Lynx firmware portal | `unzip` / `p7zip` | yes (`unzip`, `p7zip`) |
| OTA ZIP with `payload.bin` (update_engine payload v2: protobuf `DeltaArchiveManifest`, per-partition ops REPLACE/REPLACE_XZ/ZERO; incremental adds source ops) | Full and incremental OTAs on all Android headsets; Meta Quest updates | `payload-dumper-go`; `magiskboot extract`; AOSP `ota_extractor` | yes (`payload-dumper-go` 1.3.0) |
| Android sparse image (magic `0xed26ff3a`) | `super.img`, `system.img`, `userdata.img` in factory zips | `simg2img` | yes (`android-tools` 36.0.1 bundles it; standalone `simg2img` pkg also exists) |
| `super` dynamic-partition image (liblp `LpMetadata`; contains system/vendor/product/odm logical partitions, possibly A/B slots) | All Android ≥10 devices, i.e. every Android headset target | `lpdump` (inspect), `lpunpack` (split) | yes (in `android-tools`) |
| `boot.img` v0–v2 (kernel + ramdisk + optional second/dtb, appended DTBs) | Quest 1 (Android 10-era) | `unpack_bootimg`, `mkbootimg`; `magiskboot unpack`; `extract-dtb` for appended DTBs | yes (`android-tools`); `magiskboot` **no** (NUR `xddxdd/nur-packages#magiskboot`; needs first-class packaging) |
| `boot.img` v3/v4 + `vendor_boot.img` (GKI split; v4 adds vendor-ramdisk fragments, boot signature) | Android 11+ headsets (Lynx, Galaxy XR, PFDM) | `unpack_bootimg` (handles vendor_boot and fragments), `mkbootimg` | yes (`android-tools`) |
| `init_boot.img` (generic ramdisk split out of boot.img) | Android 13+ devices | `unpack_bootimg`, `magiskboot` | yes / NUR |
| `dtbo.img`, `dtb.img` (DTBO table format) | All Qualcomm Android targets | `mkdtboimg dump`; `extract-dtb`; `dtc` to decompile | yes (`mkdtboimg` in `android-tools`; `extract-dtb` 1.2.3; `dtc`) |
| `vbmeta.img` and chained AVB descriptors (hash / hashtree descriptors, rollback indexes) | All AVB2 devices | `avbtool info_image`, `avbtool verify_image`; `avbroot` for re-signing workflows | yes (`avbtool` in `android-tools`; `avbroot` packaged) |
| ext4 partition images (system/vendor/odm on older builds; APEX payloads) | Quest 1, Lynx; robotnix precedent | `debugfs -R "rdump / <dir>"` (no root); loop-mount ro (needs privileges — avoid in builds); `e2fsdroid`/`mke2fs.android` to rebuild | yes (`e2fsprogs`; `android-tools` for the Android mkfs variants) |
| EROFS partition images (system/vendor on modern builds) | Galaxy XR, PFDM, likely Quest 3-era images | `fsck.erofs --extract=<dir>` (supports `--preserve-owner`/xattr dump), `dump.erofs`; `erofsfuse` | yes (`erofs-utils` 1.9.2; **note**: nixpkgs builds with `selinuxSupport ? false` — override needed to preserve/inspect SELinux xattrs) |
| f2fs images (userdata; rarely needed by the importer) | Modern Android userdata | `dump.f2fs` (`f2fs-tools`) | yes |
| APEX packages (`.apex`/`.capex` = ZIP containing `apex_payload.img` ext4/erofs + manifest; capex adds outer compression) | Vendor APEXes carrying firmware/HALs | `unzip` + `debugfs`/`fsck.erofs`; AOSP `deapexer` | partial (`unzip`+fs tools yes; `deapexer` **not packaged**) |
| Samsung FUS artifacts (`.tar.md5` containing `.img.lz4` per partition) | Galaxy XR stock firmware via Frija/SamFirm | `tar`, `lz4`, then the standard per-partition tools above | yes (`tar`, `lz4`; FUS *download* tooling like `samloader` not in nixpkgs) |
| Qualcomm EDL/Firehose packages (`rawprogram*.xml` + `patch*.xml` + partition blobs; MBN loaders) | Lynx authenticated Firehose recovery; low-level rescue paths | `qdl`; `edl` (bkerler) for research | partial (`qdl` yes; `bkerler/edl` **not packaged**) |
| RAUC bundle `.raucb` (SquashFS: `manifest.raucm` + payload or casync index) | Steam Frame | `unsquashfs`; `rauc extract --keyring` (verified); `rauc info` | yes (`squashfs-tools`, `rauc`) |
| casync index + chunk store (`.caibx`/`.caidx` + `.castr` of `.cacnk`) | Steam Frame rootfs delivery | `desync cache/extract/verify-index`; `casync` | yes (`desync`, `casync`) |
| Raw GPT whole-disk images | SD/eMMC dumps; brick-style donors | project GPT tool (brick `image_tool.py` pattern); `sgdisk`/`gdisk` for interactive work | project code + yes (`gptfdisk`) |
| Android sparse *chunked* factory payloads (`system.img_sparsechunk.N`) | Some vendors (Motorola-style; unlikely for headsets) | `simg2img` (concatenating) | yes |

Summary: **nixpkgs already covers nearly the entire zoo** via `android-tools` (which bundles
`simg2img`, `lpdump`/`lpunpack`, `mkbootimg`/`unpack_bootimg`/`repack_bootimg`, `avbtool`,
`mkdtboimg`), `payload-dumper-go`, `extract-dtb`, `erofs-utils`, `e2fsprogs`, `squashfs-tools`,
`rauc`, `desync`, `qdl`, `avbroot`. The gaps Mura would need to package or vendor:
`magiskboot` (exists in NUR; useful as a robust one-tool fallback but not strictly required given
`unpack_bootimg` + `payload-dumper-go`), AOSP `deapexer` (trivially replaced by `unzip` +
filesystem tools), and `bkerler/edl` (research/rescue only, not a build input). An
`erofs-utils.override { selinuxSupport = true; }` is needed wherever SELinux xattrs must be read.

## 4. The metadata preservation problem

### 4.1 What the Nix store destroys

Nix store objects are NAR-serialized: a NAR records only file contents, the executable bit,
symlink targets, and directory structure. Everything else is normalized or dropped —
**ownership (uid/gid), non-executable mode bits, setuid/setgid/sticky, all extended attributes
(hence POSIX capabilities `security.capability` and SELinux labels `security.selinux`), and
timestamps**. Android system/vendor filesystems depend on exactly this metadata: AOSP's
`fs_config` assigns uid/gid/mode/capabilities per path, and `file_contexts` assigns SELinux
labels; a vendor partition re-created without them will not boot an Android userspace.

Consequence: **any extraction derivation whose output is a file tree silently loses this
metadata**, even if the extraction tool faithfully wrote it into the build sandbox. The loss
happens at store-serialization time and no tool choice avoids it. The brick appliance never hits
this because its donor artifacts are either opaque blobs (boot.img, env, boot0) or files whose
metadata it deliberately normalizes (`runtime_archive.py` strips setuid; the SquashFS build sets
`-all-root -no-xattrs`, `nix/appliance.nix` line 119–121).

### 4.2 Representation (a): intact image blobs

Keep each extracted partition as a single verbatim image file in the store (`vendor.img`,
`modem.img`, `vbmeta.img`). All metadata lives *inside* the blob's bytes, so the store's
normalization is harmless. Consumers either (i) copy the blob byte-for-byte into an assembled
disk image / flash script (the brick clean-image pattern, `nix/clean-image.nix` lines 33–40,
97–123), or (ii) mount it read-only at runtime on the device (loop or dm-verity), or (iii) read
individual files out of it on demand with `debugfs`/`fsck.erofs` in later derivations.

Pros: lossless by construction; trivially hashable and diffable against vendor manifests
(`avbtool verify_image` still works); matches what flashing tools want; AVB metadata remains
valid for pass-through partitions. Cons: coarse granularity (a one-file change re-stores
gigabytes); opaque to Nix-level composition; runtime mounting costs loop devices and prevents
cherry-picking single blobs into the Mura rootfs closure.

### 4.3 Representation (b): contents + explicit metadata manifest

Extract the file tree *and* generate, in the same derivation, a manifest file recording per path:
uid, gid, full mode (including setuid), capabilities, SELinux label, file type, symlink target,
and content SHA-256. Sources for the metadata: `fsck.erofs --extract` with xattr support,
`debugfs` `stat`/`ea_list` output, or parsing the donor's own `fs_config`/`file_contexts`
artifacts when present. Consumers that need to *rebuild* an Android-valid filesystem feed the
manifest to `mke2fs.android` + `e2fsdroid` (which accept fs_config/file_contexts inputs) or a
SELinux-enabled `mkfs.erofs --file-contexts=…`.

Pros: fine-grained — individual blobs become first-class store paths usable in the Mura
rootfs closure; enables allowlisting (brick `runtime_archive.py` pattern) and per-file license
tracking; small rebuild deltas. Cons: the manifest generator becomes trust-critical (a bug loses
metadata invisibly); re-assembly reintroduces filesystem-image nondeterminism; two artifacts
(tree + manifest) must never drift apart.

### 4.4 Recommendation: hybrid, blob-default

Mura is **not** rebuilding Android, which changes the calculus: for the vast majority of
consumed artifacts (kernel `Image`, DTBs, `lib/firmware` files, GPU/DSP blobs copied into a
Wayland Linux rootfs), Android's fs_config/SELinux metadata is *irrelevant at destination* —
Mura assigns its own ownership and labels. What matters is byte-exact content plus a
*record* of the original metadata for audit. Therefore:

1. **Default: representation (a).** Every parse-stage output is a verbatim partition blob,
   content-addressed. Partitions that get flashed back unchanged (modem, dsp, persist, vbmeta on
   locked-adjacent flows, `boot0`-analogue raw regions) *only* ever exist as blobs.
2. **Extract stage: representation (b) with normalized metadata + manifest.** File-level
   extraction (the brick `runtime_archive.py` analogue) emits a normalized tree (0644/0755,
   no setuid, no xattrs — matching what the store keeps anyway) plus `metadata.json` capturing
   the original uid/gid/mode/caps/SELinux label/hash per file. The manifest is informational and
   audit-supporting, not load-bearing for boot.
3. **Full Android-filesystem re-assembly (b→image) is out of scope** unless a future flow needs
   to write a modified vendor partition back to a stock-Android slot; if that arises, use
   `mke2fs.android`/`e2fsdroid` with the recorded manifest, and treat it as its own gated stage.

## 5. Proposed donor-import contract for Mura

Five stages, each a separate derivation (or fixed-output derivation) with declared inputs and
outputs. Stage boundaries are chosen so that everything after `acquire` is pure and offline, and
everything before `qualify` is mechanical (no human judgment embedded in code).

```
acquire ──▶ identify ──▶ parse ──▶ extract ──▶ qualify ──▶ (image assembly, out of scope here)
 (FOD)      (pure)       (pure)     (pure)      (data + check derivation)
```

### 5.1 Stage: acquire

- **Takes**: an entry from the donor manifest (section 5.6): `name`, `hash`, and either `urls`
  (public) or an import `message` (non-redistributable).
- **Produces**: the verbatim vendor artifact as a single store path (flat/SRI hash).
- **Policy** (hash pinning):
  - `fetchurl` for artifacts at stable public URLs with no click-through or authentication:
    Google-style factory/OTA zips (robotnix precedent,
    `references/robotnix/modules/adevtool/default.nix` lines 102–109), Lynx portal downloads,
    Steam Frame `.raucb` + sidecar manifests from `holo-images.steamos.cloud`. Record mirror
    URLs; upstreams delete old releases.
  - `requireFile` for anything behind authentication, click-through licensing, device-extracted
    dumps, or community mirrors of dubious longevity (Meta Quest firmware archives, Samsung FUS
    output, SD/eMMC dumps): brick pattern with an instructive `message` naming the exact
    file and `nix-store --add-fixed sha256` command
    (`brick/appliance/nix/default.nix` lines 22–31).
  - For casync-backed donors, `acquire` covers the bundle (`.raucb`) and, as a separate
    fixed-output derivation, a chunk-store snapshot produced by `desync cache` against the
    pinned index — hashed as a NAR of the `.castr` directory. The remote store URL is recorded
    but never trusted at build time after the snapshot exists.
  - Record the *publisher's* stated hash separately from our computed hash when both exist
    (brick `donorCompressedSha256` vs `donorHash` pattern, `profile.nix` lines 12–14), so
    provenance survives recompression.

### 5.2 Stage: identify

- **Takes**: the acquired artifact + the donor manifest entry's expected identity.
- **Produces**: `identity.json` — machine-read facts: container type detected by magic bytes
  (never by extension), device codename, build ID / version, per-member table of
  `{path, size, sha256}` for top-level members, and — where the vendor provides it — the
  vendor's own metadata verbatim (Steam Frame `manifest.json` fields `product/variant/arch/
  buildid`; Android `ro.build.fingerprint` from the payload's build props; AVB
  `avbtool info_image` output for vbmeta).
- **Behavior**: *asserts* the detected identity against the manifest's declared `device` and
  `buildId` and fails on mismatch. This is the guard against "right hash, wrong expectations"
  (e.g. a Smart Pro image where a Brick image is required — `brick/appliance/docs/sources.md`
  explicitly warns the assets are not interchangeable). Identification failure is fail-closed:
  unknown formats are errors, not warnings (the `image_tool.py` philosophy: "Unknown image
  formats fail rather than being guessed", `brick/appliance/README.md`).

### 5.3 Stage: parse

- **Takes**: the artifact + `identity.json` + the manifest's `container` declaration.
- **Produces**: a normalized **partition set**: one directory of verbatim partition blobs named
  by canonical partition name (`boot_a.img`, `vendor.img`, `vbmeta.img`, `rootfs.img`, …) plus
  `parse-report.json` recording, per blob: source container member, extraction tool + version,
  size, sha256, detected filesystem/format, and AVB descriptor summary where applicable.
- **Per-container recipes** (each its own small derivation so caching is granular):
  - Factory zip → `unzip` outer + inner image zip → `simg2img` each sparse member.
  - OTA zip → `payload-dumper-go -o <out> payload.bin` (full payloads only; refuse incremental
    payloads that reference source partitions we do not have).
  - `super.img` → `simg2img` if sparse → `lpdump --json` (recorded) → `lpunpack`.
  - `boot.img`/`vendor_boot.img`/`init_boot.img` → `unpack_bootimg --format=mkbootimg` with the
    argument dump recorded (needed for byte-exact repack later); appended-DTB kernels
    additionally pass through `extract-dtb`.
  - `dtbo.img` → `mkdtboimg dump`; DTBs decompiled with `dtc -I dtb -O dts` *for the report
    only* — the blob remains canonical.
  - `vbmeta.img` → `avbtool info_image` into the report; chained-partition digests recorded.
  - `.raucb` → `unsquashfs` (plus `rauc info --keyring` when a trusted keyring is pinned in the
    manifest) → `desync extract -s <chunk snapshot> rootfs.img.caibx rootfs.img` →
    `desync verify-index`.
  - Samsung `.tar.md5` → `tar` → `lz4 -d` per member → standard per-partition handling.
  - Whole-disk GPT images → a Mura port of brick `image_tool.py inspect`/`extract`
    (CRC-verified primary+backup GPT, bounds/overlap/duplicate checks, regular-files-only;
    `tools/image_tool.py` lines 74–150) — this tool should be adopted nearly verbatim.
- All parse derivations verify what they produce: sizes and hashes go into the report; where the
  donor carries its own digests (RAUC manifest `sha256`, AVB hash descriptors, payload.bin
  per-op hashes verified by payload-dumper-go), the recipe checks them and records the result.

### 5.4 Stage: extract

- **Takes**: the partition set + the manifest's `extract` rules.
- **Produces**: named **artifact sets** — e.g. `kernel/` (Image, DTBs, ramdisk, mkbootimg args),
  `firmware/` (files destined for `/lib/firmware`), `blobs/<domain>/` (GPU, camera, DSP
  userspace libs), each with a per-file `metadata.json` (section 4.4: original uid/gid/mode/
  capabilities/SELinux label/sha256) and normalized store-safe permissions.
- **Mechanics**: filesystem reads are unprivileged — `debugfs -R "rdump <path> <out>"` for ext4
  (robotnix/adevtool precedent, `modules/adevtool/default.nix` lines 73–79),
  `fsck.erofs --extract` for EROFS (with the SELinux-enabled `erofs-utils` override to read
  labels into the manifest). No loop mounts, no root, no FUSE inside derivations.
- **Policy, inherited from `runtime_archive.py`**: extraction rules are allowlists (explicit
  path prefixes and exact files), never "everything except…"; setuid/setgid are always dropped;
  symlink targets are validated against the allowlist; member-count and size ceilings apply
  (`tools/runtime_archive.py` lines 12–20, 27–46, 105). A rule that matches nothing is an error
  (silent partial extraction is how blob drift starts).

### 5.5 Stage: qualify

- **Takes**: artifact sets + a human-authored, reviewed **donor contract** file (per donor,
  hash-bound like the brick boot contract).
- **Produces**: the *qualified donor* attrset that image-assembly derivations accept; plus a
  check derivation that re-validates the contract against the actual artifacts on every build.
- **Contract contents** (following `docs/boot-contract.example.json`): schema version; device;
  `donorSha256` binding it to the exact acquired artifact; the reviewed boot-chain facts this
  donor is trusted for (e.g. "ABL accepts unsigned boot images when unlocked", "vendor_boot
  ramdisk mounts vendor via fstab entry X"); which artifact sets are approved for which uses
  (`kernelForBoot = true`, `firmwareRedistributable = false`); and mandatory free-text
  `reviewNotes` with a minimum-length check (`image_tool.py` lines 168–170 enforce ≥40 chars —
  keep this; it converts "review theater" into recorded evidence).
- **Gating**: exactly the brick null-propagation pattern — flashable image outputs evaluate to
  `null`/absent until the contract exists and validates (`nix/default.nix` lines 59–74). Interim
  outputs (artifact sets, reports) build without a contract so research is never blocked.

### 5.6 Donor manifest schema sketch

One Nix attrset per donor, committed to the repo (the analogue of brick `profile.nix` donor
fields + robotnix `vendor_imgs/<device>.json`, unified):

```nix
{
  # Identity
  device = "lynx-r1";                 # Mura device codename
  vendor = "lynx";
  buildId = "1.1.2-20250114";         # vendor's release identifier
  class = "android-factory";          # android-factory | android-ota | rauc-casync |
                                      # samsung-fus | disk-image

  # Acquisition (one artifact entry per file; hash is authoritative)
  artifacts = {
    firmware-zip = {
      name = "lynx-r1-firmware-1.1.2.zip";
      hash = "sha256-…";                       # our computed SRI hash
      publisherSha256 = "…";                   # vendor-stated hash, if published (else null)
      source = {
        kind = "fetchurl";                     # fetchurl | requireFile
        urls = [ "https://portal.lynx-r.com/…" ];
        # kind = "requireFile" entries instead carry:
        # message = "Download … from … while logged in, then nix-store --add-fixed sha256 …";
      };
    };
    # rauc-casync donors add: bundle = { … }; chunkStore = { storeUrl, indexName, narHash };
  };

  # Identification assertions (checked by the identify stage)
  expect = {
    fingerprintContains = "lynx/…";            # or raucCompatible = "steamos-aarch64";
    abScheme = true;
    dynamicPartitions = true;
  };

  # Parse plan
  container = {
    outer = "zip";
    payload = "payload.bin";                   # or superImg = "super.img"; or raucb = …;
    partitions = [ "boot" "vendor_boot" "dtbo" "vbmeta" "super" "modem" "persist" ];
    keepVerbatim = [ "modem" "persist" "vbmeta" ];   # blob-only, never file-extracted
  };

  # Extract rules (allowlists; empty match = build failure)
  extract = {
    kernel = { from = "boot"; items = [ "kernel" "ramdisk" "mkbootimg-args" ]; };
    dtbs   = { from = "dtbo"; items = [ "*" ]; };
    firmware = {
      from = "vendor";
      allowPrefixes = [ "etc/firmware/" "firmware/" "lib/firmware/" ];
      allowFiles = [ ];
      maxTotalBytes = 1073741824;
    };
    gpuBlobs = { from = "vendor"; allowPrefixes = [ "lib64/egl/" ]; … };
  };

  # Licensing / redistribution flags (consulted by cache-push and release tooling;
  # any donor-derived output with redistributable=false must set allowSubstitutes=false)
  licensing = {
    fetchIsPublic = true;         # may CI download it?
    redistributable = false;      # may artifacts appear in public caches/releases?
    gplComponents = [ "kernel" ]; # corresponding-source obligations to track
    notes = "Vendor EULA §3 prohibits redistribution of firmware blobs.";
  };

  # Qualification
  contract = null;                # ./contracts/lynx-r1-1.1.2.json once reviewed; null = ungated
}
```

Evaluation of this attrset produces the acquire/identify/parse/extract derivation graph
mechanically; only `contract` requires human action.

## 6. Reproducibility properties and risks

- **Tool version pinning**: all extraction tools come from the locked nixpkgs revision, so parse
  and extract stages are as pinned as any Nix build. Risk: tool *behavior* changes across nixpkgs
  bumps (e.g. `payload-dumper-go` or `erofs-utils` output naming) invalidate extract-stage
  caches and can change bytes. Mitigation: `parse-report.json` records tool names + versions;
  golden-hash tests per donor (assert the sha256 of each extracted partition) turn a silent
  behavior change into a loud diff. robotnix pins even the extractor's yarn dependency tree
  (`yarn_hashes.json`) — the equivalent discipline applies if Mura ever vendors adevtool-
  style tooling.
- **Extraction determinism**: blob-level operations (`dd`-style GPT extraction, `simg2img`,
  `lpunpack`, `desync extract`) are deterministic — output bytes are fully determined by input
  bytes. File-tree extraction is riskier: directory iteration order, timestamps, and umask can
  differ. Mitigations: NAR serialization already normalizes timestamps/ordering for store
  outputs; manifests must be emitted in sorted order; extraction code sets `TZ=UTC LC_ALL=C`
  (brick does this in every image-touching derivation, `nix/sd-image.nix` line 9,
  `image_tool.py` line 202).
- **Compression nondeterminism**: never re-compress in the donor pipeline. Store extracted
  artifacts uncompressed (or verbatim as delivered); decompression (`lz4 -d`, xz in
  payload ops, zstd in `.cacnk` chunks) is deterministic, re-compression (multithreaded xz/zstd,
  gzip headers) is not. Where Mura *builds* compressed artifacts (its own SquashFS/EROFS
  rootfs), copy brick's normalization: `mksquashfs … -noappend -all-root -no-xattrs -mkfs-time
  $SOURCE_DATE_EPOCH -all-time $SOURCE_DATE_EPOCH -processors 1` (`nix/appliance.nix`
  lines 118–122) — note `-processors 1`, because parallel compressors can produce
  block-layout differences.
- **Filesystem-image nondeterminism**: if the pipeline ever rebuilds FAT/ext4/EROFS images, pin
  UUIDs, volume IDs, hash seeds, and fake time (`E2FSPROGS_FAKE_TIME`, `-U`, `-E hash_seed=…`,
  `-i <volid>` — `nix/clean-image.nix` lines 17, 63–77); and verify by independent rebuild +
  byte comparison, per `docs/architecture.md` lines 108–111. mtools FAT writes carry timestamp
  nondeterminism (brick README flags this as an open item) — avoid mtools writes in Mura's
  pipeline or normalize afterwards.
- **Fixed-output-derivation trust**: an FOD's hash pins bytes but not availability. Public
  vendor URLs rot (Meta publishes latest-only; Google deletes old factory images eventually).
  Mitigation: the manifest allows mirror URL lists, and `requireFile` remains the fallback for
  anything an operator archived privately. The Steam Frame archive script demonstrates the
  archival posture: fetch once, verify, then "No remote source appears in any command below this
  point" (`archive-steam-frame.sh` line 130).
- **Cache-leak risk**: donor-derived outputs on a public cache are a redistribution event. The
  `licensing.redistributable` flag must mechanically force `preferLocalBuild = true;
  allowSubstitutes = false` (brick `nix/sd-image.nix` lines 4–6) and exclusion from cache-push
  scripts.
- **Incremental OTAs are a trap**: delta payloads reference source-partition state; ingesting
  them requires the previous full image and bit-exact application. Policy: full images only;
  the identify stage rejects payloads whose manifest contains source ops.

## 7. What Mura should adopt / reject

**From the brick appliance — adopt (most of it):** `requireFile` with instructive messages;
single reviewed data file per donor holding all hashes and gates; contract-as-data hash-bound to
the donor with mandatory review notes; null-propagated output gating; transplant-before-clean-
image staging (for Mura: "replace one payload inside a copied stock image" as the first
boot-attempt strategy on each device, graduating to assembled images); read-back preservation
verification; regular-files-only tooling with exclusive-create outputs; `allowSubstitutes =
false` on donor-containing outputs; the GPT inspector nearly verbatim. **Reject/adapt:** the
single-donor, single-board scale — Mura needs the manifest schema of section 5.6 and a
stage-per-derivation graph rather than brick's two hand-written image derivations; and brick's
allowlists are hardcoded in the tool (`runtime_archive.py` lines 12–17) where Mura should
move them into per-donor manifest data.

**From the Steam Frame archive — adopt:** the archive-first posture (snapshot bundle + chunk
store, then operate fully offline); `desync`-based chunk handling; treating vendor sidecar
metadata (`manifest.json`) as the identity source; opt-in signature verification that never
silently downgrades. Strongly consider RAUC+casync for Mura's own updates: single-slot
`format=plain` bundles with `.caibx` indexes are simple, all tooling is in nixpkgs, and chunk
stores give cheap delta updates without delta-payload complexity. **Reject:** shipping only a
rootfs slot — Mura on Android-boot devices must also manage boot/dtbo/vbmeta slots, so its
RAUC manifests will be multi-image; and the archive script's bash monolith should become
derivations.

**From robotnix — adopt:** `fetchurl` + committed per-device/per-release JSON hash records for
public donors; automated metadata regeneration (`update.sh` pattern) so pin records are produced
by tooling, not typed; verify-once-in-Nix then trust inside the sandbox; `debugfs`-based
unprivileged ext4 extraction. **Reject:** running extraction inside the consuming build
derivation — Mura wants artifact sets as independent, cacheable, inspectable store paths;
and adevtool itself (Pixel-specific device knowledge; Mura's per-device knowledge lives in
the donor manifest instead).

## 8. Open questions

1. **Quest/Meta donor acquisition path**: Meta serves latest-only updates via its updater;
   historical images exist only on community mirrors (see `07-device-landscape.md`). Is
   `requireFile` against operator-archived images the permanent posture, or should Mura
   maintain its own private archival store keyed by the manifest hashes?
2. **Chunk-store snapshot hashing**: hashing a `.castr` directory as a NAR-FOD works but couples
   the hash to desync's on-disk layout. Is a content-defined alternative (hash of the sorted
   chunk-ID list + per-chunk hashes, i.e. the `chunks_details.json` shape) worth the custom
   fetcher complexity?
3. **AVB re-signing scope**: for unlocked devices Mura can disable verification or sign
   with its own keys (`avbtool`/`avbroot` are packaged). Which target devices require a valid
   self-signed vbmeta chain versus tolerating `vbmeta` with the disable-verification flag, and
   does that belong in the donor contract or the (separate) boot-chain architecture document?
4. **Manifest metadata fidelity**: is recording SELinux labels/capabilities in `metadata.json`
   ever load-bearing for Mura (e.g. a future containerized-Android compatibility layer
   running vendor HALs), which would upgrade representation (b) from audit-only to
   boot-critical and demand test coverage of the manifest generator?
5. **Incremental re-qualification**: when a donor is bumped to a new `buildId`, which contract
   facts carry over and which must be re-reviewed? Splitting device-invariant facts (partition
   scheme) from build-specific facts (initramfs behavior) reduces review load but risks stale
   approvals.
6. **GPL corresponding-source tracking**: donor kernels are GPL; the manifest's
   `licensing.gplComponents` records the obligation, but where does the pipeline verify that a
   pinned kernel-source tree actually corresponds to the extracted `Image` (vermagic/config
   comparison), and is that an identify-stage check or a qualify-stage contract fact?
7. **Steam Frame boot/firmware slots**: this bundle updates only `rootfs`. What updates the
   ESP/bootloader/firmware partitions on the Frame, and does Mura need to ingest a second,
   different Valve artifact class to control the full boot chain?
