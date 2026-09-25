# The VM's stand-in for the `syspersist` partition.
#
# On hardware /persist is a GPT partition by PARTLABEL beside the A/B slots
# (families/uefi-rauc). The NixOS VM runner builds a single root image with no partition
# table of ours, so a second blank virtual disk plays the part — and since the recovery
# environment (modules/os/recovery.nix) exists, it plays it the same way as hardware: a GPT
# `syspersist` partition created by systemd-repart in stage 1 on the first boot and marked
# `FactoryReset=yes`, so the recovery menu's factory reset (`systemd-repart --factory-reset`)
# deletes and re-creates it exactly as it would on a device. The A/B slot switch itself is not
# VM-testable; it stays with the uefi-rauc image proof (docs/research/33 §9).
#
# Imported by the interactive VM (virtualisation.vmVariant) and by tests/vm/lib.nix;
# `virtualisation.*` options exist only where the qemu-vm module is loaded.
{ ... }:
{
  virtualisation.emptyDiskImages = [ 512 ];

  boot.initrd.systemd.repart = {
    enable = true;
    device = "/dev/vdb";
    empty = "allow"; # a fresh emptyDiskImages disk has no partition table yet
  };
  systemd.repart.partitions."30-syspersist" = {
    Type = "linux-generic";
    Label = "syspersist";
    Format = "ext4";
    FactoryReset = true;
  };

  virtualisation.fileSystems."/persist" = {
    device = "/dev/disk/by-partlabel/syspersist";
    fsType = "ext4";
    neededForBoot = true;
  };
}
