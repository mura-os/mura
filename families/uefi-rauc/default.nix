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
  cfg = config.mura;
  compatible = "mura-${cfg.device.codename}";

  toplevel = config.system.build.toplevel;
  kernelParamsCommon = lib.concatStringsSep " " ([
    "init=${toplevel}/init"
    "console=ttyAMA0"
  ] ++ config.boot.kernelParams);

  bootEntry = slot: pkgs.writeText "entry-${slot}.conf" ''
    title Mura (slot ${lib.toUpper slot})
    linux /EFI/mura/Image
    initrd /EFI/mura/initrd
    options root=PARTLABEL=rootfs_${slot} rauc.slot=${lib.toUpper slot} ${kernelParamsCommon}
  '';
  # The recovery environment (modules/os/recovery.nix; research/57 §3): the SAME kernel and
  # initrd booted to mura-recovery.target — systemd's boot-menu-entry shape. Never counted,
  # never the default; reached with `systemctl reboot --boot-loader-entry=recovery` (the
  # LoaderEntryOneShot EFI variable), which is what the crash-loop counter does at its threshold.
  recoveryEntry = pkgs.writeText "entry-recovery.conf" ''
    title Mura recovery
    linux /EFI/mura/Image
    initrd /EFI/mura/initrd
    options rd.systemd.unit=mura-recovery.target ${kernelParamsCommon}
  '';

  # RAUC custom bootloader backend (interface: rauc calls with
  # get-primary | set-primary <bootname> | get-state <bootname> | set-state <bootname> good|bad).
  # Primary selection = systemd-boot `default` line in /esp/loader/loader.conf (entry ID `a`/`b`
  # — the ID is the file name minus `.conf` and minus any boot-counting suffix, so it stays
  # stable across `a+3.conf` → `a+2-1.conf` → `a.conf`); slot state lives in
  # /esp/loader/mura-slot-state. steamos-bootconf shape, minimal.
  #
  # Boot counting (D6, implementation-path §3a; references/systemd/docs/AUTOMATIC_BOOT_ASSESSMENT.md):
  # `set-primary S` arms the target slot's entry with `+N` tries (`a.conf` → `a+3.conf`);
  # systemd-boot renames it per attempt (`a+2-1.conf`, …) and falls back to the other entry when
  # the counter hits zero; `systemd-bless-boot good` (upstream, after boot-complete.target ←
  # mura-readiness) strips the counters; `mura-mark-good.service` then tells RAUC — the third,
  # separate transition. A slot that was never armed boots uncounted (the factory image).
  bootTries = toString cfg.deployment.bootTries;
  bootconf = pkgs.writeShellApplication {
    name = "mura-bootconf";
    text = ''
      LOADER=/esp/loader/loader.conf
      STATE=/esp/loader/mura-slot-state
      ENTRIES=/esp/loader/entries
      cmd="''${1:-}"; slot="''${2:-}"; val="''${3:-}"
      to_id() { case "$1" in A) echo a ;; B) echo b ;; *) echo "unknown slot $1" >&2; exit 1 ;; esac; }
      case "$cmd" in
        get-primary)
          d=$(sed -n 's/^default[[:space:]]*//p' "$LOADER" | sed 's/\.conf$//')
          case "$d" in a) echo A ;; b) echo B ;; *) echo "unknown default $d" >&2; exit 1 ;; esac ;;
        set-primary)
          id=$(to_id "$slot")
          # arm the entry with +N tries if it carries no counter yet (a.conf or none → a+N.conf)
          if [ -e "$ENTRIES/$id.conf" ]; then mv "$ENTRIES/$id.conf" "$ENTRIES/$id+${bootTries}.conf"; fi
          tmp=$(mktemp); sed "s/^default[[:space:]].*/default $id/" "$LOADER" > "$tmp"; cat "$tmp" > "$LOADER"; rm -f "$tmp" ;;
        get-state)
          touch "$STATE"
          s=$(sed -n "s/^$slot=//p" "$STATE"); echo "''${s:-good}" ;;
        set-state)
          touch "$STATE"
          tmp=$(mktemp); { grep -v "^$slot=" "$STATE" || true; echo "$slot=$val"; } > "$tmp"; cat "$tmp" > "$STATE"; rm -f "$tmp" ;;
        get-current)
          sed -n 's/.*rauc\.slot=\([AB]\).*/\1/p' /proc/cmdline ;;
        get-tries)
          # observability: the counted entry for a slot, if any (a+2-1.conf → "2 tries left, 1 done")
          id=$(to_id "$slot")
          f=$(ls "$ENTRIES"/"$id"+*.conf 2>/dev/null | head -n1)
          [ -n "$f" ] && basename "$f" || echo "$id.conf (not counted)" ;;
        *) echo "usage: mura-bootconf get-primary|set-primary S|get-state S|set-state S good|bad|get-current|get-tries S" >&2; exit 1 ;;
      esac
    '';
  };

  # In-store TEST key/cert (never a release key; invariant 5). Non-deterministic
  # keygen is acceptable for the scaffold; the release path signs out-of-store.
  testCert = pkgs.runCommand "rauc-test-cert" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir -p $out
    openssl req -x509 -newkey rsa:2048 -nodes -keyout $out/key.pem -out $out/cert.pem \
      -days 3650 -subj "/O=mura/CN=mura TEST signing (never for release)"
  '';
in
{
  ###### Image: systemd-repart GPT disk ######
  # repart.nix is NOT in the default module list (only in the docs' extraModules);
  # it must be imported explicitly, and is additionally gated on `enable`.
  imports = [ "${modulesPath}/image/repart.nix" ];
  image.repart = {
    enable = true;
    name = "mura-${cfg.device.codename}";
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
          "/EFI/mura/Image".source =
            "${config.system.build.kernel}/${config.system.boot.loader.kernelFile}";
          "/EFI/mura/initrd".source =
            "${config.system.build.initialRamdisk}/${config.system.boot.loader.initrdFile}";
          "/loader/loader.conf".source = pkgs.writeText "loader.conf" ''
            default a
            timeout 3
            editor yes
          '';
          "/loader/entries/a.conf".source = bootEntry "a";
          "/loader/entries/b.conf".source = bootEntry "b";
          "/loader/entries/recovery.conf".source = recoveryEntry;
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
          FactoryReset = true; # the recovery menu's factory reset deletes and re-creates it (repart.d(5))
        };
      };
      "40-home" = {
        repartConfig = {
          Type = "home";
          Label = "home";
          Format = "ext4";
          SizeMinBytes = "512M";
          FactoryReset = true;
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
  # Per-unit persistent state: the `syspersist` partition (the donor's own partlabel set,
  # docs/research/33) mounted at /persist. Read-write: per-unit state durability is a
  # contract requirement (device-contract `mura.xr.calibration.paths`, overview
  # invariant 4). Everything *inside* /persist — the state classes, machine-id, the
  # userdb, Bluetooth pairing, the F1 pattern — is modules/os/persist.nix's.
  #
  # Stage 1 and no `nofail` (multi-user.md §1.1 rule 2): userborn reads /persist/userdb
  # before sysinit.target and /etc/machine-id is bound from here in the initrd; a device
  # without its account database must not boot to a greeter — the B1b recovery ladder is
  # the answer to a missing persist, not a silent boot.
  fileSystems."/persist" = {
    device = "/dev/disk/by-partlabel/syspersist";
    fsType = "ext4";
    neededForBoot = true;
  };

  boot.initrd.systemd.enable = true;
  boot.initrd.supportedFilesystems = [ "btrfs" ];

  # The runtime repart definitions the recovery environment's factory reset operates on: the
  # two state partitions, marked FactoryReset=yes (systemd-repart --factory-reset deletes and
  # re-creates exactly these; the slots and the ESP are untouched). Present in the initrd via
  # boot.initrd.systemd.repart; a normal boot's repart run is a no-op on a populated disk.
  systemd.repart.partitions = {
    "30-syspersist" = { Type = "linux-generic"; Label = "syspersist"; Format = "ext4"; FactoryReset = true; };
    "40-home" = { Type = "home"; Label = "home"; Format = "ext4"; FactoryReset = true; };
  };
  boot.initrd.systemd.repart.enable = true;
  mura.recovery.rebootCommand = "systemctl reboot --boot-loader-entry=recovery";
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
    statusfile=/var/lib/mura/state/health/rauc.status

    [handlers]
    bootloader-custom-backend=${bootconf}/bin/mura-bootconf

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
    after = [ "mura-persist-setup.service" ]; # the status file lives on /persist
    serviceConfig = {
      Type = "dbus";
      BusName = "de.pengutronix.rauc";
      ExecStart = "${pkgs.rauc}/bin/rauc service";
    };
  };

  # Health-gated success (images-and-updates.md; implementation-path §3a): the THIRD transition.
  # boot-complete.target is reached only after mura-readiness (modules/os/health.nix);
  # systemd-bless-boot then strips the +N counters from the ESP entry (second transition); only
  # then is RAUC told the slot is good. Three observable steps, each on its own.
  systemd.services.mura-mark-good = {
    description = "Mura: mark the booted RAUC slot good after the boot was blessed";
    wantedBy = [ "boot-complete.target" ];
    after = [ "boot-complete.target" "systemd-bless-boot.service" "rauc.service" ];
    requires = [ "boot-complete.target" "rauc.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${pkgs.rauc}/bin/rauc status mark-good";
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
    pkgs.runCommand "mura-${cfg.device.codename}-bundle"
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
        bundle $out/mura-${cfg.device.codename}.raucb
    '';
}
