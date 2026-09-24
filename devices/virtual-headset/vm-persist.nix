# The VM's stand-in for the `syspersist` partition.
#
# On hardware /persist is a GPT partition by PARTLABEL beside the A/B slots
# (families/uefi-rauc). The NixOS VM runner builds a single root image with no partition
# table of ours, so a second blank virtual disk plays the part: the same property under
# test — a filesystem that outlives the root image. The A/B slot switch itself is not
# VM-testable; it stays with the uefi-rauc image proof (docs/research/33 §9).
#
# Imported by the interactive VM (virtualisation.vmVariant) and by tests/vm/lib.nix;
# `virtualisation.*` options exist only where the qemu-vm module is loaded.
{ ... }:
{
  virtualisation.emptyDiskImages = [ 512 ];
  virtualisation.fileSystems."/persist" = {
    device = "/dev/vdb";
    fsType = "ext4";
    autoFormat = true;
    neededForBoot = true;
  };
}
