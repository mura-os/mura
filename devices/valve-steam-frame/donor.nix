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
      raucBootloader = "custom"; # steamos-bootconf backend
      bootfwAB = true; # splctl / GPT-attribute / XBL_SC-parttype selection (doc 33 §3)
    };
    slots = {
      # /etc/rauc/system.conf + fstab (doc 33 §2): the layout our image family mirrors.
      scheme = "by-partsets"; # A/ B/ shared/ self/
      rootfs = [ "A" "B" ]; # type=raw, desync in-place install with seed
      partitions = [ "esp" "efi_a" "efi_b" "rootfs_a" "rootfs_b" "syspersist" "home" ];
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
