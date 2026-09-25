# Evaluation-time assertions on the persistent-state layout (first-run-onboarding.md §2,
# multi-user.md §1.1; D1 in implementation-path §3c).
#
# Pure evaluation of the flake's NixOS configurations — no build, so the aarch64 Steam Frame
# image is checked here on x86_64 too. Pins the flashable image's persist layout (the
# `syspersist` partition, stage-1 mount, no `nofail`, the binds, the /etc overlay's upper
# layer on /persist) so the VM stand-in and the real partition cannot drift apart without a
# failing check.
{ nixpkgs, system, configurations }:
let
  lib = nixpkgs.lib;

  frame = configurations.valve-steam-frame.config;
  vm = configurations.virtual-headset.config;
  vmMulti = configurations.virtual-headset-multiuser.config;

  hasBind = fs: builtins.elem "bind" fs.options;
  hasNofail = fs: builtins.elem "nofail" fs.options;
  pullsInSetup = fs: builtins.elem "x-systemd.requires=mura-persist-setup.service" fs.options;

  # What both a real image and the VM must agree on (the module's mounts).
  commonChecks = name: c: {
    "${name}-varlibmura-binds-persist" =
      c.fileSystems."/var/lib/mura".device == "/persist/mura"
      && hasBind c.fileSystems."/var/lib/mura"
      && pullsInSetup c.fileSystems."/var/lib/mura"
      && !hasNofail c.fileSystems."/var/lib/mura";
    # /etc is a mutable overlay whose upper layer is bound from /persist in stage 1 — this is
    # what makes passwd/useradd/machine-id/NetworkManager state survive a slot switch.
    "${name}-etc-overlay-mutable" =
      c.system.etc.overlay.enable && c.system.etc.overlay.mutable;
    "${name}-etc-upper-on-persist-stage1" =
      c.fileSystems."/.rw-etc".device == "/persist/etc-rw"
      && hasBind c.fileSystems."/.rw-etc"
      && c.fileSystems."/.rw-etc".neededForBoot;
    # userborn keeps its default location: the files are real files inside the overlay
    # (symlinks into /persist would be replaced by shadow-utils' rename — D1 finding).
    "${name}-userborn-in-etc" =
      c.services.userborn.enable
      && c.services.userborn.passwordFilesLocation == "/etc";
    "${name}-mutable-users" = c.users.mutableUsers;
    "${name}-systemd-initrd" = c.boot.initrd.systemd.enable;
    "${name}-ssh-hostkey-in-identity" =
      !c.services.openssh.enable
      || lib.all (k: lib.hasPrefix "/var/lib/mura/identity/ssh/" k.path) c.services.openssh.hostKeys;
    "${name}-f1-seed-marker-gated" =
      c.systemd.services.mura-f1-seed-state.unitConfig.ConditionPathExists
      == "!/var/lib/mura/state/provisioning/seed-state";
  };

  results = commonChecks "frame" frame // commonChecks "vm" vm // commonChecks "vm-multiuser" vmMulti // {
    # The flashable image: /persist IS the syspersist partition, stage 1, never nofail.
    frame-persist-is-syspersist-partition =
      frame.fileSystems."/persist".device == "/dev/disk/by-partlabel/syspersist"
        && frame.fileSystems."/persist".fsType == "ext4";
    frame-persist-stage1-no-nofail =
      frame.fileSystems."/persist".neededForBoot && !hasNofail frame.fileSystems."/persist";
    frame-repart-has-syspersist =
      let p = frame.image.repart.partitions."30-syspersist".repartConfig; in
      p.Label == "syspersist" && p.Format == "ext4";
    # Bluetooth: the Frame has an adapter → pairing/ bound; the VM has none → no bind.
    frame-pairing-bound =
      frame.fileSystems."/var/lib/bluetooth".device == "/persist/mura/pairing";
    vm-no-pairing-bind = !(vm.fileSystems ? "/var/lib/bluetooth");
    # The VM stand-in disk (vmVariant): a GPT `syspersist` partition on /dev/vdb created by
    # systemd-repart in stage 1 and marked FactoryReset=yes — the same shape as the image, so the
    # recovery environment's factory reset is exercised the same way (recovery.nix).
    vm-persist-standin-disk =
      let
        vmv = configurations.virtual-headset.config.virtualisation.vmVariant;
        fs = vmv.virtualisation.fileSystems."/persist";
        part = vmv.systemd.repart.partitions."30-syspersist";
      in
      fs.device == "/dev/disk/by-partlabel/syspersist" && fs.neededForBoot
        && vmv.boot.initrd.systemd.repart.enable && vmv.boot.initrd.systemd.repart.device == "/dev/vdb"
        && part.Label == "syspersist" && part.FactoryReset == true;
    # The image's state partitions are the ones a factory reset deletes and re-creates; Mura
    # Recovery is a separate XBOOTLDR boot image, not a file on the normal ESP.
    frame-factory-reset-partitions =
      frame.systemd.repart.partitions."30-syspersist".FactoryReset == true
        && frame.systemd.repart.partitions."40-home".FactoryReset == true
        && frame.fileSystems."/efi".device == "/dev/disk/by-partlabel/esp"
        && frame.fileSystems."/boot".device == "/dev/disk/by-partlabel/mura_recovery"
        && frame.mura.recovery.rebootCommand == "systemctl reboot --boot-loader-entry=recovery.conf"
        && !(frame.image.repart.partitions."10-esp".contents ? "/loader/entries/recovery.conf")
        && frame.image.repart.partitions."15-mura-recovery".repartConfig.Type == "xbootldr"
        && frame.image.repart.partitions."15-mura-recovery".repartConfig.Label == "mura_recovery"
        && frame.image.repart.partitions."15-mura-recovery".contents ? "/loader/entries/recovery.conf"
        && frame.image.repart.partitions."15-mura-recovery".contents ? "/EFI/mura-recovery/Image"
        && frame.image.repart.partitions."15-mura-recovery".contents ? "/EFI/mura-recovery/initrd"
        && lib.hasInfix "[slot.rescue.0]" frame.environment.etc."rauc/system.conf".text
        && lib.hasInfix "device=/dev/disk/by-partlabel/mura_recovery" frame.environment.etc."rauc/system.conf".text;
  };

  failures = lib.filterAttrs (_: v: v != true) results;
in
if failures == { }
then nixpkgs.legacyPackages.${system}.runCommand "mura-persist-tests-pass" { } "echo ok > $out"
else throw "mura persist tests failed: ${builtins.toJSON (builtins.attrNames failures)}"
