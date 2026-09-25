# modules/os/recovery.nix — failure feedback on the panels and the Mura recovery environment
# (implementation-path §4 "Mura recovery environment"; research/56 §3, research/57).
#
# Two failure classes, two mechanisms (research/57 §1): bad CODE is the bootloader's (A/B slot
# fallback, families/uefi-rauc); bad STATE is the OS's. This module is the state half:
#
#   1. Feedback on the FIRST hard preflight failure (pmOS's shape): plymouth stays on the panels
#      and shows what failed and how to reach the device (ssh over the gadget, the hotspot, the
#      docs). Nobody waits three boots to say something; Android's own reason for keeping its
#      escalation short is that time with an inoperable device is what sends people to support.
#   2. The recovery environment: the SAME initrd booted to `mura-recovery.target`
#      (rd.systemd.unit=…; systemd's boot-menu-entry shape, Mobile NixOS's recovery-is-stage-1) —
#      sshd on the gadget address with the device's own host key when /persist mounts, the panel
#      screen listing the ways in, and OFFERED actions: factory reset (systemd-repart
#      --factory-reset over the FactoryReset=yes partitions — systemd's mechanism, executed from
#      early boot's "well-defined clean state"), slot switch where the family has one, reboot.
#      Never an automatic wipe (rule 3; Lineage, Rescue Party and Quest all confirm first).
#   3. The step at the crash-loop threshold (health.nix): reboot into that environment,
#      automatically (ruled 2026-09-25) — where the family provides an entry
#      (mura.recovery.rebootCommand); otherwise the old mura-recovery.target in stage 2.
#
# Per family: uefi-rauc adds the `recovery.conf` BLS entry and sets rebootCommand; the
# Android-derived families package this initrd as their recovery boot image and use
# `reboot recovery` (with their bring-up). The device provides its DRM driver in the initrd
# (plymouth needs it: devices/virtual-headset adds virtio_gpu).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura;
  gadgetAddr = "172.16.42.1";
  hotspotEnv = "/run/mura/hotspot.env";
  docsUrl = "https://mura.dev/recovery"; # placeholder domain — the docs URL is the project's to fix
  plymouth = "${config.boot.plymouth.package}/bin/plymouth";

  theme = pkgs.callPackage ../../pkgs/mura-plymouth-theme {
    panelWidth = cfg.hardware.panel.width;
    panelHeight = cfg.hardware.panel.height;
    displays = cfg.hardware.displays;
  };

  # The declared self-builder keys of every account: the recovery environment has only root,
  # and the administrator's keys are the ones that open it (first-run §5.3).
  authorizedKeys = lib.unique (lib.concatMap (u: u.openssh.authorizedKeys.keys) (lib.attrValues config.users.users));

  # The persist partition, when this configuration has one (the bare virtual-headset toplevel
  # gets it only from vm-persist.nix / the family); recovery works without it, minus the key.
  persistFs = config.fileSystems."/persist" or { device = ""; fsType = "ext4"; };

  # plymouth's client protocol caps one message at 255 bytes (ply-boot-client.c asserts on it);
  # the two-step theme stacks messages, so a text file goes up as chunks of whole lines.
  plymouthSay = pkgs.writeShellScript "mura-plymouth-say" ''
        file="$1"; chunk=""; ok=1
        flush() {
          [ -n "$chunk" ] || return 0
          ${plymouth} display-message --text="$chunk" || ok=0
          chunk=""
        }
        while IFS= read -r line || [ -n "$line" ]; do
          if [ $(( ''${#chunk} + ''${#line} + 1 )) -gt 200 ]; then flush; fi
          chunk="''${chunk:+$chunk
    }$line"
        done < "$file"
        flush
        [ "$ok" = 1 ]
  '';

  # The lines every surface shows: what to do now. Plain POSIX shell; runs once per failure.
  waysIn = pkgs.writeShellScript "mura-ways-in" ''
    echo "Reach this headset:"
    echo "  USB cable:  ssh mura@${gadgetAddr}"
    if [ -r ${hotspotEnv} ]; then
      # shellcheck disable=SC1090
      . ${hotspotEnv}
      echo "  Wi-Fi:      network $MURA_HOTSPOT_SSID   password $MURA_HOTSPOT_PSK"
    fi
    echo "  Help:       ${docsUrl}"
  '';

  # The recovery shell's menu (POSIX shell; thin glue over systemd-repart/bootconf/reboot).
  recoveryMenu = pkgs.writeShellScript "mura-recovery" ''
    set -u
    persist_dev="${persistFs.device}"
    [ -n "$persist_dev" ] || persist_dev=/dev/disk/by-partlabel/syspersist
    show() {
      echo "Mura recovery"
      echo "  This headset did not boot into a session."
      [ -s /run/mura/preflight.summary ] && { echo "  Last preflight:"; sed 's/^/    /' /run/mura/preflight.summary; }
      echo
      echo "  factory-reset   wipe the persistent state and reboot — asks first"
      ${lib.optionalString (cfg.recovery.switchSlotCommand != null) ''echo "  switch-slot     boot the other system slot next time"''}
      echo "  reboot          try again"
      echo "  poweroff"
      echo
    }
    case "''${1:-}" in
      factory-reset)
        echo "This erases everything this headset has stored: accounts, settings, Wi-Fi, pairings."
        echo "The system itself is kept. Type: yes, erase"
        read -r answer
        [ "$answer" = "yes, erase" ] || { echo "Not erased."; exit 1; }
        ${plymouth} display-message --text="Resetting this headset. Do not power off." 2>/dev/null || true
        # systemd's factory reset, executed from early boot: every FactoryReset=yes partition in
        # /etc/repart.d is deleted and re-created empty (repart.d(5)); nothing else is touched.
        # the whole disk that carries the persist partition (sysfs: a partition's parent)
        part=$(readlink -f "$persist_dev"); name=$(basename "$part")
        if [ -e "/sys/class/block/$name/partition" ]; then
          disk=/dev/$(basename "$(readlink -f "/sys/class/block/$name/..")")
        else
          disk=$part
        fi
        systemd-repart --dry-run=no --factory-reset=yes --definitions=/etc/repart.d "$disk" || exit 1
        sync
        # (tests/vm/recovery.nix inspects the result in place; a person always reboots)
        [ -n "''${MURA_RECOVERY_NO_REBOOT:-}" ] || systemctl reboot ;;
      ${lib.optionalString (cfg.recovery.switchSlotCommand != null) ''
        switch-slot) ${cfg.recovery.switchSlotCommand} && systemctl reboot ;;
      ''}
      reboot) systemctl reboot ;;
      poweroff) systemctl poweroff ;;
      *) show ;;
    esac
  '';
in
{
  options.mura.recovery = {
    rebootCommand = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        How stage 2 reboots into the recovery environment; set by the image family
        (uefi-rauc: `systemctl reboot --boot-loader-entry=recovery`). When null the crash-loop
        threshold falls back to `mura-recovery.target` in stage 2.
      '';
    };
    switchSlotCommand = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "How the recovery environment selects the other A/B slot; set by the family (needs the ESP mounted in the recovery initrd).";
    };
  };

  config = lib.mkIf (cfg.xr.shell != "none") {
    ## 1. plymouth, and the failure feedback ------------------------------------------------
    boot.plymouth = {
      enable = true;
      theme = "mura";
      themePackages = [ theme ];
    };
    # pmOS's reason (postmarketos-bootsplash/20-plymouth.conf): "If plymouth detects *any*
    # serial console, it disables the splash" — every device here has one.
    boot.kernelParams = [ "plymouth.ignore-serial-consoles" ];

    # The splash stays when there is no greeter: plymouth-quit is ordered after the preflight
    # and skipped on a hard failure (the marker the probe writes). On a good boot greetd takes
    # the display as usual (plymouth-quit-wait ordering, NixOS's greetd module).
    systemd.services.plymouth-quit = {
      after = [ "mura-preflight.service" ];
      unitConfig.ConditionPathExists = "!/run/mura/preflight.failed";
    };
    systemd.services.plymouth-quit-wait = {
      after = [ "mura-preflight.service" ];
      unitConfig.ConditionPathExists = "!/run/mura/preflight.failed";
    };
    systemd.services.mura-preflight.onFailure = [ "mura-preflight-feedback.service" ];
    systemd.services.mura-preflight-feedback = {
      description = "Mura: put the preflight failure and the ways in on the panels";
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      path = [ pkgs.coreutils pkgs.gnused ];
      script = ''
        {
          echo "This headset could not start its session."
          echo
          sed 's/^/  /' /run/mura/preflight.summary 2>/dev/null
          echo
          ${waysIn}
        } > /run/mura/feedback.txt
        ${plymouthSay} /run/mura/feedback.txt || echo "plymouth is not running; the panel shows nothing"
      '';
    };

    ## 2. the recovery environment (stage 1) --------------------------------------------------
    boot.initrd.systemd = {
      # networkd only in recovery (the gadget interface + a DHCP server for the cable's host);
      # a normal boot leaves it to stage 2.
      network.enable = true;
      network.networks."10-mura-recovery-usb0" = {
        matchConfig.Name = "usb0";
        address = [ "${gadgetAddr}/24" ];
        networkConfig.DHCPServer = true;
        # the gadget has no carrier until a host enumerates it; hold the address regardless
        networkConfig.ConfigureWithoutCarrier = true;
        dhcpServerConfig = { PoolOffset = 2; PoolSize = 19; EmitDNS = false; EmitRouter = false; };
      };
      services.systemd-networkd.wantedBy = lib.mkForce [ "mura-recovery.target" ];
      sockets.systemd-networkd.wantedBy = lib.mkForce [ "mura-recovery.target" ];

      users.sshd = { uid = 1; group = "sshd"; };
      groups.sshd = { gid = 1; };
      contents = {
        "/etc/ssh/sshd_recovery_config".text = ''
          UsePAM no
          Port 22
          PasswordAuthentication no
          KbdInteractiveAuthentication no
          AuthorizedKeysFile /etc/ssh/authorized_keys.d/%u
          HostKey /run/mura-recovery/ssh_host_ed25519_key
          Banner /run/mura-recovery/banner
        '';
        "/etc/ssh/authorized_keys.d/root".text = lib.concatStringsSep "\n" authorizedKeys + "\n";
        "/etc/repart.d".source = lib.mkDefault (pkgs.runCommand "empty-repart.d" { } "mkdir $out");
      };
      storePaths = [
        "${pkgs.openssh}/bin/sshd"
        "${pkgs.openssh}/bin/ssh-keygen"
        "${pkgs.openssh}/libexec/sshd-auth"
        "${pkgs.openssh}/libexec/sshd-session"
        recoveryMenu
        waysIn
        plymouthSay
        "${pkgs.gnused}/bin/sed"
      ];
      extraBin = {
        sed = "${pkgs.gnused}/bin/sed";
        ssh-keygen = "${pkgs.openssh}/bin/ssh-keygen";
        mura-recovery = "${recoveryMenu}";
      };

      # `rd.systemd.unit=mura-recovery.target` makes this the initrd's default target in place of
      # initrd.target, so like initrd.target it must pull basic.target itself (journald, udev,
      # plymouth-start, the test driver's initrd backdoor all hang off sysinit/basic).
      targets.mura-recovery = {
        description = "Mura recovery environment (stage 1: sshd on the gadget, panel screen, offered reset)";
        requires = [ "basic.target" "mura-recovery-identity.service" "mura-recovery-sshd.service" "mura-recovery-screen.service" ];
        after = [ "basic.target" "mura-recovery-identity.service" "mura-recovery-sshd.service" "mura-recovery-screen.service" ];
        unitConfig.AllowIsolate = true;
      };

      # The host key: the device's own, when /persist mounts read-only; else generated for this
      # session, its fingerprint shown on the panels. Also assembles the banner.
      services.mura-recovery-identity = {
        description = "Mura recovery: host key and banner";
        after = [ "systemd-udev-settle.service" ];
        wants = [ "systemd-udev-settle.service" ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = ''
          mkdir -p /run/mura-recovery /run/mura
          key=/run/mura-recovery/ssh_host_ed25519_key
          dev="${persistFs.device}"; [ -n "$dev" ] || dev=/dev/disk/by-partlabel/syspersist
          if [ -e "$dev" ] && mkdir -p /run/mura-persist \
             && mount -o ro -t ${persistFs.fsType} "$dev" /run/mura-persist 2>/dev/null; then
            if [ -r /run/mura-persist/mura/identity/ssh/ssh_host_ed25519_key ]; then
              cp /run/mura-persist/mura/identity/ssh/ssh_host_ed25519_key "$key"
              cp /run/mura-persist/mura/identity/ssh/ssh_host_ed25519_key.pub "$key.pub" 2>/dev/null || true
              echo "host key: the device's own"; echo own > /run/mura-recovery/keysource
            fi
            umount /run/mura-persist
          fi
          if [ ! -s "$key" ]; then
            ssh-keygen -q -t ed25519 -N "" -f "$key"
            echo "host key: generated for this recovery session (persist unreadable)"; echo generated > /run/mura-recovery/keysource
          fi
          chmod 0600 "$key"
          fp=$(ssh-keygen -lf "$key.pub" | cut -d' ' -f2)
          {
            echo "Mura recovery on this headset. Host key $fp"
            echo "Run: mura-recovery"
          } > /run/mura-recovery/banner
          echo "$fp" > /run/mura-recovery/fingerprint
        '';
      };

      services.mura-recovery-sshd = {
        description = "Mura recovery: sshd on the gadget address";
        after = [ "mura-recovery-identity.service" "systemd-networkd.service" "mura-usb-gadget.service" ];
        requires = [ "mura-recovery-identity.service" ];
        wants = [ "systemd-networkd.service" "mura-usb-gadget.service" ];
        before = [ "shutdown.target" ];
        conflicts = [ "shutdown.target" ];
        serviceConfig = {
          ExecStart = "${pkgs.openssh}/bin/sshd -D -e -f /etc/ssh/sshd_recovery_config";
          Type = "simple";
          KillMode = "process";
          Restart = "on-failure";
          RestartSec = "2s";
        };
      };

      services.mura-recovery-screen = {
        description = "Mura recovery: the panel screen";
        after = [ "mura-recovery-identity.service" "plymouth-start.service" ];
        requires = [ "mura-recovery-identity.service" ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = ''
          {
            echo "This headset is in recovery."
            echo
            echo "  USB cable:  ssh root@${gadgetAddr}   (host key $(cat /run/mura-recovery/fingerprint))"
            echo "  then run:   mura-recovery   — factory reset, ${lib.optionalString (cfg.recovery.switchSlotCommand != null) "switch slot, "}reboot"
            echo "  Help:       ${docsUrl}"
          } > /run/mura-recovery/screen.txt
          if ${plymouthSay} /run/mura-recovery/screen.txt; then
            echo shown > /run/mura-recovery/screen.status
          else
            echo "plymouth is not running"; echo no-plymouth > /run/mura-recovery/screen.status
          fi
        '';
      };
    };
  };
}
