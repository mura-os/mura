# uefi-rauc image family — first implemented for the Steam Frame (deckard).
#
# Mirrors the donor's slot/update architecture (docs/research/33-steam-frame-donor.md):
#   - GPT with partlabels esp / rootfs_a / rootfs_b / syspersist / home
#     (the donor additionally has per-slot `efi_a/efi_b` for its U-Boot payload;
#      our VM path boots UEFI/systemd-boot from the shared ESP, so those are
#      device-side artifacts added at hardware bring-up, not here)
#   - A/B raw rootfs slots, RAUC `bootloader=custom` with a steamos-bootconf-shaped
#     backend script (ours flips the systemd-boot `default` entry on the ESP)
#   - the `rauc.slot=A|B` kernel-cmdline contract
#   - an in-store TEST-key-signed RAUC bundle (overview.md invariant 5: real signing
#     happens outside the store; test keys are cacheable)
#
# Frame-scoped by design (design-backlog standing rule): no speculative generality.
{ lib, config, pkgs, modulesPath, ... }:
let
  cfg = config.spatial;
  compatible = "spatial-os-${cfg.device.codename}";

  toplevel = config.system.build.toplevel;
  kernelParamsCommon = lib.concatStringsSep " " ([
    "init=${toplevel}/init"
    "console=ttyAMA0"
  ] ++ config.boot.kernelParams);

  bootEntry = slot: pkgs.writeText "entry-${slot}.conf" ''
    title spatial-os (slot ${lib.toUpper slot})
    linux /EFI/spatial/Image
    initrd /EFI/spatial/initrd
    options root=PARTLABEL=rootfs_${slot} rauc.slot=${lib.toUpper slot} ${kernelParamsCommon}
  '';

  # RAUC custom bootloader backend (interface: rauc calls with
  # get-primary | set-primary <bootname> | get-state <bootname> | set-state <bootname> good|bad).
  # Primary selection = systemd-boot `default` line in /esp/loader/loader.conf;
  # slot state lives in /esp/loader/spatial-slot-state. steamos-bootconf shape, minimal.
  bootconf = pkgs.writeShellApplication {
    name = "spatial-bootconf";
    text = ''
      LOADER=/esp/loader/loader.conf
      STATE=/esp/loader/spatial-slot-state
      cmd="''${1:-}"; slot="''${2:-}"; val="''${3:-}"
      to_entry() { case "$1" in A) echo a.conf ;; B) echo b.conf ;; *) echo "unknown slot $1" >&2; exit 1 ;; esac; }
      case "$cmd" in
        get-primary)
          d=$(sed -n 's/^default[[:space:]]*//p' "$LOADER")
          case "$d" in a.conf) echo A ;; b.conf) echo B ;; *) echo "unknown default $d" >&2; exit 1 ;; esac ;;
        set-primary)
          e=$(to_entry "$slot")
          tmp=$(mktemp); sed "s/^default[[:space:]].*/default $e/" "$LOADER" > "$tmp"; cat "$tmp" > "$LOADER"; rm -f "$tmp" ;;
        get-state)
          touch "$STATE"
          s=$(sed -n "s/^$slot=//p" "$STATE"); echo "''${s:-good}" ;;
        set-state)
          touch "$STATE"
          tmp=$(mktemp); { grep -v "^$slot=" "$STATE" || true; echo "$slot=$val"; } > "$tmp"; cat "$tmp" > "$STATE"; rm -f "$tmp" ;;
        get-current)
          sed -n 's/.*rauc\.slot=\([AB]\).*/\1/p' /proc/cmdline ;;
        *) echo "usage: spatial-bootconf get-primary|set-primary S|get-state S|set-state S good|bad|get-current" >&2; exit 1 ;;
      esac
    '';
  };

  # In-store TEST key/cert (never a release key; invariant 5). Non-deterministic
  # keygen is acceptable for the scaffold; the release path signs out-of-store.
  testCert = pkgs.runCommand "rauc-test-cert" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir -p $out
    openssl req -x509 -newkey rsa:2048 -nodes -keyout $out/key.pem -out $out/cert.pem \
      -days 3650 -subj "/O=spatial-os/CN=spatial-os TEST signing (never for release)"
  '';
in
{
  ###### Image: systemd-repart GPT disk ######
  # repart.nix is NOT in the default module list (only in the docs' extraModules);
  # it must be imported explicitly, and is additionally gated on `enable`.
  imports = [ "${modulesPath}/image/repart.nix" ];
  image.repart = {
    enable = true;
    name = "spatial-${cfg.device.codename}";
    split = true; # emit per-partition files too; the rootfs one feeds the RAUC bundle
    # zstd both artifacts: raw disk sparseness does not survive NAR transfer from
    # the remote builder; compressed, the mostly-empty 33G image moves as ~a few GB.
    compression = {
      enable = true;
      algorithm = "zstd";
    };
    partitions = {
      "10-esp" = {
        repartConfig = {
          Type = "esp";
          Label = "esp";
          Format = "vfat";
          SizeMinBytes = "512M";
        };
        contents = {
          "/EFI/BOOT/BOOTAA64.EFI".source =
            "${pkgs.systemd}/lib/systemd/boot/efi/systemd-bootaa64.efi";
          "/EFI/spatial/Image".source =
            "${config.system.build.kernel}/${config.system.boot.loader.kernelFile}";
          "/EFI/spatial/initrd".source =
            "${config.system.build.initialRamdisk}/${config.system.boot.loader.initrdFile}";
          "/loader/loader.conf".source = pkgs.writeText "loader.conf" ''
            default a.conf
            timeout 3
            editor yes
          '';
          "/loader/entries/a.conf".source = bootEntry "a";
          "/loader/entries/b.conf".source = bootEntry "b";
        };
      };
      "20-rootfs-a" = {
        repartConfig = {
          Type = "root";
          Label = "rootfs_a";
          Format = "btrfs"; # matches the donor payload filesystem
          SizeMinBytes = "16G";
          SizeMaxBytes = "16G";
          SplitName = "rootfs_a"; # emitted as <image>.rootfs_a.raw; feeds the bundle
        };
        storePaths = [ toplevel ];
      };
      "21-rootfs-b" = {
        # Empty slot: populated by the RAUC update round-trip (type=raw install).
        repartConfig = {
          Type = "root";
          Label = "rootfs_b";
          Format = "btrfs";
          SizeMinBytes = "16G";
          SizeMaxBytes = "16G";
          SplitName = "-";
        };
      };
      "30-syspersist" = {
        repartConfig = {
          Type = "linux-generic";
          Label = "syspersist";
          Format = "ext4";
          SizeMinBytes = "64M";
        };
      };
      "40-home" = {
        repartConfig = {
          Type = "home";
          Label = "home";
          Format = "ext4";
          SizeMinBytes = "512M";
        };
      };
    };
  };

  ###### Mounts (donor-mirroring: doc 33 §2) ######
  fileSystems."/" = {
    # systemd's fstab-generator lets the kernel cmdline `root=` (per boot entry)
    # take precedence in the initrd; this is the slot-A default.
    device = "/dev/disk/by-partlabel/rootfs_a";
    fsType = "btrfs";
  };
  fileSystems."/esp" = {
    device = "/dev/disk/by-partlabel/esp";
    fsType = "vfat";
    options = [ "umask=0077" "nofail" "x-systemd.automount" ];
  };
  fileSystems."/home" = {
    device = "/dev/disk/by-partlabel/home";
    fsType = "ext4";
    options = [ "nofail" ];
  };
  # Per-unit persistent state. Read-write: per-unit state durability is a contract
  # requirement (device-contract `spatial.xr.calibration.paths`, overview invariant 4)
  # — the earlier `ro` mount was donor-mirroring that couldn't survive first contact
  # with the lock/PIN/calibration design (PIN hashes, user calibration, and the
  # provisioning marker all live here; docs/architecture/first-run-onboarding.md).
  #
  # /persist/spatial subtree classes (first-run-onboarding.md §state classes —
  # factory reset treats each differently, never the tree as one blob):
  #   factory/    factory calibration — survives factory reset
  #   identity/   device keys — survive reset; regenerated only by re-provisioning
  #   enrollment/ PIN hash, user credentials — wiped on reset
  #   state/      update/migration state — reset per policy
  #   machine-id  own class: survives A/B slot updates, rotated on factory reset
  fileSystems."/persist" = {
    device = "/dev/disk/by-partlabel/syspersist";
    fsType = "ext4";
    options = [ "nofail" ];
  };
  # /var/lib/spatial is the contract-visible path; it binds into /persist so it
  # survives A/B slot switches. The bind mount *pulls in* the setup service
  # (x-systemd.requires — ordering alone is not a dependency); the service creates
  # the directory skeleton, which is what makes this work after a factory reset
  # (an image-seeded directory would not).
  fileSystems."/var/lib/spatial" = {
    device = "/persist/spatial";
    fsType = "none";
    options = [
      "bind"
      "nofail"
      "x-systemd.requires=spatial-persist-setup.service"
      "x-systemd.after=spatial-persist-setup.service"
    ];
  };
  systemd.services.spatial-persist-setup = {
    description = "Create the /persist/spatial state skeleton";
    unitConfig.RequiresMountsFor = "/persist";
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      install -d -m 0750 /persist/spatial
      install -d -m 0750 /persist/spatial/factory
      install -d -m 0700 /persist/spatial/identity
      install -d -m 0700 /persist/spatial/enrollment
      install -d -m 0750 /persist/spatial/state
      # userdb class (multi-user profile; multi-user.md §1.1): world-traversable —
      # /etc/passwd symlinks here and getpwuid is universal, so it cannot live under
      # the 0750 spatial/ tree. File perms (passwd/group 0644, shadow 0000) are
      # userborn's; the initrd-early mount + RequiresMountsFor wiring lands with the
      # multi-user profile module.
      install -d -m 0755 /persist/userdb
    '';
  };

  boot.initrd.systemd.enable = true;
  boot.initrd.supportedFilesystems = [ "btrfs" ];
  # QEMU aarch64 virt machine devices for the VM proof.
  boot.initrd.availableKernelModules = [ "virtio_pci" "virtio_blk" "virtio_scsi" "virtio_net" ];
  # No bootloader installer runs inside the image build; entries are baked above.
  boot.loader.grub.enable = false;
  boot.loader.systemd-boot.enable = false;

  ###### RAUC ######
  environment.systemPackages = [ pkgs.rauc bootconf ];
  services.dbus.packages = [ pkgs.rauc ];

  environment.etc."rauc/system.conf".text = ''
    [system]
    compatible=${compatible}
    bootloader=custom
    statusfile=/tmp/rauc.status

    [handlers]
    bootloader-custom-backend=${bootconf}/bin/spatial-bootconf

    [keyring]
    path=/etc/rauc/keyring.pem

    [slot.rootfs.0]
    bootname=A
    device=/dev/disk/by-partlabel/rootfs_a
    type=raw

    [slot.rootfs.1]
    bootname=B
    device=/dev/disk/by-partlabel/rootfs_b
    type=raw
  '';
  environment.etc."rauc/keyring.pem".source = "${testCert}/cert.pem";

  systemd.services.rauc = {
    description = "RAUC update service";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "dbus";
      BusName = "de.pengutronix.rauc";
      ExecStart = "${pkgs.rauc}/bin/rauc service";
    };
  };

  ###### The update bundle (test-signed) ######
  system.build.raucBundle =
    let
      bundleManifest = pkgs.writeText "manifest.raucm" ''
        [update]
        compatible=${compatible}
        version=${config.system.nixos.version}

        [bundle]
        format=plain

        [image.rootfs]
        filename=rootfs.img
      '';
    in
    pkgs.runCommand "spatial-${cfg.device.codename}-bundle"
      { nativeBuildInputs = [ pkgs.rauc pkgs.zstd pkgs.squashfsTools ]; } ''
      mkdir -p bundle $out
      split="${config.system.build.image}/${config.image.baseName}.rootfs_a.raw"
      if [ -e "$split.zst" ]; then
        zstd -d --sparse "$split.zst" -o bundle/rootfs.img
      else
        cp "$split" bundle/rootfs.img
      fi
      install -m 0644 ${bundleManifest} bundle/manifest.raucm
      rauc bundle --cert=${testCert}/cert.pem --key=${testCert}/key.pem \
        bundle $out/spatial-${cfg.device.codename}.raucb
    '';
}
