# Donor manifest: Valve Steam Frame (deckard), SteamOS "vr" channel.
# First real donor through the pipeline. Facts verified in docs/research/33-steam-frame-donor.md.
#
# This is a data-only manifest (lib/donor remains a typed stub until the pipeline
# machinery is built against real needs — design-backlog standing rule). Every
# byte source is hash-pinned; the payload itself is never fetched by `nix build`
# from Valve (acquire is the out-of-store archive script), and outputs embedding
# donor bytes are localOnly.
{
  name = "deckard-steamos-vr";
  version = "20260921.6090922";
  variant = "0.5.0";
  compatible = "steamos-aarch64"; # RAUC compatible string (Valve's; ours differs deliberately)

  # acquire — reproducible out-of-store recipe; original distribution form retained.
  acquire = {
    bundleUrl = "https://holo-images.steamos.cloud/vr/20260921.6090922/deckard-20260921.6090922-0.5.0.raucb";
    chunkStoreUrl = "https://holo-images.steamos.cloud/vr/20260921.6090922/deckard-20260921.6090922-0.5.0.castr";
    recipe = "references/archive-steam-frame/archive-steam-frame.sh";
    # requireFile-shaped local path for the reconstructed payload (sparse btrfs image):
    localPath = "references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/images/rootfs.img";
    sha256 = "5c53ff2ed7dc78f313a19fc9224aa07e7fb63271b811a4ada295441a0361e6a8"; # == manifest.raucm [image.rootfs]

    # Valve's official first-install/repair release — the layout, boot-chain and flash donor
    # (docs/research/74-steam-frame-recovery-image.md). Userspace inside is 0.3.0 build
    # 20260922.6152327 (older than the payload above); it is NOT the userspace donor. Whether it
    # becomes a second full donor entry is an owner decision (74 §10 Q5).
    recovery = {
      release = "20260922.5153644-0.3.0"; # packaging job id; OS build inside = 20260922.6152327 (vr, branch rc)
      storePage = "https://store.steampowered.com/steamos/download/?ver=steamframe-qdl"; # EULA click-through
      latestAlias = "https://steamdeck-images.steamos.cloud/recovery/steamframe-repair-qdl-latest.tar.gz";
      qdlTarballUrl = "https://steamdeck-images.steamos.cloud/recovery/steamframe-oobe-repair-qdl-20260922.5153644-0.3.0.tar.gz";
      qdlTarballSha256 = "d3323bfa8efe9ece1954948421cdf5f705e8942eb50c960e2916d935d1b850ab";
      usbImageUrl = "https://steamdeck-images.steamos.cloud/recovery/steamframe-oobe-repair-20260922.5153644-0.3.0.img.bz2";
      usbImageSha256 = "3a4a077f1b1f40688ab3279affcb56776bd97c54db1573e7c65fc52a97106676";
      recipe = "references/archive-steam-frame/archive-steam-frame-recovery.sh";
      localPath = "references/archive-steam-frame/frame-recovery-deckard-20260922.5153644-0.3.0";
      # Vendor-documented entry procedures (Steam Support 65B4-2AA3-5F37-4227, retrieved 2026-09-27).
      edlChord = "power off, wait 10 s, hold Power + Volume Up + Volume Down 10 s, connect USB-C";
      bootMenuChord = "hold Aux (Select), press Power; Volume Up/Down navigate, Aux selects";
    };
  };

  # identify — facts read from the payload (doc 33 §2–§6).
  facts = {
    filesystem = "btrfs";
    sizeBytes = 10737418240;
    kernel = {
      version = "6.18.0-gbfea53e51a5d";
      pkgbase = "linux-618-deckard";
      binaryRepo = "https://holo-packages.steamos.cloud/archlinux-deckard-hotfixes/";
      sourceTarball = null; # not publicly located 2026-09-23; GPL-request/watch item (doc 33 §4)
      ikconfigExtracted = true; # extracted/config-6.18.0-deckard beside the archive
      virtioBlkNet = false; # donor kernel cannot boot QEMU virtio machines
    };
    boot = {
      loader = "u-boot"; # 2025.07-rc3-00634-gbb0a2a01cb6d, per-board images in /boot
      slotCmdline = "rauc.slot="; # A|B token on the kernel cmdline
      efiPartCmdline = "steamos.efi=PARTUUID="; # selects the efi-X partition whose partsets the initrd loads (doc 74 §4)
      raucBootloader = "custom"; # steamos-bootconf backend
      bootfwAB = true; # splctl / GPT-attribute / XBL_SC-parttype selection (doc 33 §3)
      # Chain as evidenced by the recovery release (doc 74 §4): PBL -> xbl_{a,b} (LUN 1, Qualcomm +
      # SecTools *test* OEM chain) -> uefi_{a,b} = U-Boot SPL (LUN 1, no certificates) ->
      # uboot_{a,b} FIT (LUN 2, crc32 only) -> /boot/Image + initrd.uImage + maindtb.dtb.
      spl = "u-boot-spl"; # lives in the Qualcomm `uefi_a/b` slots; no Qualcomm UEFI/ABL, keymaster/uefisecapp empty
      kernelPayload = "raw-image"; # /boot/Image loaded unsigned; uboot.env has verify=n
      efiBootmethPresent = true; # bootmeth_efi + BOOTAA64.EFI strings, steamcl.efi shipped; order vs native path unknown (74 §11)
      efiVarStore = "uefivarstore"; # raw partition on LUN 2, zero in the image; runtime persistence unproven
      usbBoot = "spl-loads-esp:/uboot/u-boot.img"; # "Boot from USB": SPL loads a U-Boot FIT from the stick's first FAT partition
      edl = {
        programmer = "loader.melf"; # Lanai devprg BOOT.MXF.2.1-01643.1-LANAI-1, RISC-V MELF
        host = "linux-msm qdl (BSD)"; # x86_64/aarch64 Linux + Windows builds shipped by Valve
        writes = [ "lun0" "lun1" "lun2" ]; # sparse extent map for LUN 0; whole-LUN for 1 and 2; no patch/erase XML
      };
    };
    slots = {
      # /etc/rauc/system.conf + fstab (doc 33 §2) + the recovery release's GPT (doc 74 §3).
      scheme = "by-partsets"; # A/ B/ shared/ self/ other/ — udev symlinks generated from efi-X:/SteamOS/partsets
      rootfs = [ "A" "B" ]; # type=raw, desync in-place install with seed
      # Vendor partlabels are `-A`/`-B` (rootfs-A), not `_a`; Mura's own image uses `rootfs_a` (owner call, 74 §10 Q3).
      partitions = [ "esp" "efi-A" "efi-B" "rootfs-A" "rootfs-B" "var-A" "var-B" "home" ];
      # UFS logical units as the artifacts and stock scripts name them (74 §3). Sizes are the
      # recovery image's; `home` is grown at first boot by systemd-repart (Type=home, no maximum).
      luns = {
        lun0 = {
          device = "/dev/sda";
          role = "os";
          gptTemplateBytes = 34359738368;
          sectorSize = 4096;
          partitions = [ "esp" "efi-A" "efi-B" "rootfs-A" "rootfs-B" "var-A" "var-B" "home" ];
        };
        lun1 = {
          device = "/dev/sdb";
          role = "boot-firmware";
          sizeBytes = 268435456;
          sectorSize = 4096;
          partitions = [ "xbl" "xblconfig" "shrm" "aop" "aopconfig" "cpucp" "cpucpconfig" "tz" "devcfg" "hyp" "uefi" "uefisecapp" "keymaster" "qupfw" ]; # each _a/_b; content == /boot/bootfw.tar.xz
          slotAttribute = "LegacyBIOSBootable";
        };
        lun2 = {
          device = "/dev/sdc";
          role = "u-boot";
          sizeBytes = 268435456;
          sectorSize = 4096;
          partitions = [ "bootenv" "bootenvb" "uboot_a" "ubootenv_a" "ubootfw_a" "uboot_b" "ubootenv_b" "ubootfw_b" "uefivarstore" "cdt" "ddr" "xblramdump" "toolsfv" "adpd" ];
        };
        lun3 = {
          device = "/dev/sdd";
          role = "syspersist";
          sizeBytes = null; # not in the recovery package; ext4 label syspersist, /persist ro
          source = "calibration EEPROM via deckard-eeprom/restore-calibration";
        };
      };
      growth = "systemd-repart:/usr/lib/repart.d/90-home.conf Type=home + fstab x-systemd.growfs";
    };
    deviceTree = {
      production = "sm8650-mp.dtb"; # == /boot/maindtb.dtb; model "SM8650 MP rev1 4slam 2et"
      revisions = [ "dv1" "dv2" "ev1" "ev2" "ev3" "mp" ];
    };
    xrStack = "steamvr-gamescope"; # proprietary; no Monado (doc 33 §6)
  };

  # qualify — cache/redistribution policy (overview invariant 3).
  redistributable = false; # mixed GPL/blob/proprietary content -> localOnly
  signature = {
    payloadHashVerified = true; # against manifest.raucm
    bundleChainVerified = false; # open item, doc 33 §1
  };
}
