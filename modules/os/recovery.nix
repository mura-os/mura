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
#   2. The recovery environment: a dedicated family recovery image booted to
#      `mura-recovery.target` (uefi-rauc: a self-contained UKI; Mobile NixOS's
#      recovery-is-stage-1 shape) —
#      sshd on the gadget address with the device's own host key when /persist mounts, and the
#      menu — pkgs/mura-recovery, specs/recovery-menu.md: the actions once (factory reset via
#      systemd-repart --factory-reset over the FactoryReset=yes partitions — systemd's mechanism,
#      executed from early boot's "well-defined clean state"; slot switch where the family has
#      one; reboot; power off), three ways in: the HMD's buttons on the panels (`panel`: evdev +
#      plymouth, the roles from the contract via /etc/mura/recovery.json), ssh/console
#      (`shell`), and the web page on the cable/hotspot (`mura-setup --recovery`). Every
#      destructive action is OFFERED behind a confirm, never automatic (rule 3; Lineage, Rescue
#      Party and Quest all confirm first).
#   3. The step at the crash-loop threshold (health.nix): reboot into that environment,
#      automatically (ruled 2026-09-25) — where the family provides an entry
#      (mura.recovery.rebootCommand); otherwise the old mura-recovery.target in stage 2.
#
# Every family packages this stage-1 environment as a dedicated Mura recovery image (uefi-rauc:
# one UKI binding kernel+initrd+cmdline; no recovery root filesystem), separate from normal Mura
# boot artifacts. Android-derived families must additionally preserve stock/vendor recovery as the
# independent path that can reinstall Mura when Mura itself is broken; their bring-up needs an
# added Mura partition and selector, and `reboot recovery` is not that selector. The device
# provides its DRM driver in the initrd (plymouth needs it: devices/virtual-headset adds
# virtio_gpu).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura;
  gadgetAddr = "172.16.42.1";
  hotspotAddr = "10.42.0.1";
  hotspotEnv = "/run/mura/hotspot.env";
  docsUrl = "https://mura.dev/recovery"; # placeholder domain — the docs URL is the project's to fix
  plymouth = "${config.boot.plymouth.package}/bin/plymouth";

  theme = pkgs.callPackage ../../pkgs/mura-plymouth-theme {
    panelWidth = cfg.hardware.panel.width;
    panelHeight = cfg.hardware.panel.height;
    displays = cfg.hardware.displays;
  };

  # The keys that open the recovery environment (root, the only account in stage 1): the declared
  # keys of ADMINISTRATORS only — wheel members and root itself (ruled 2026-09-25). A non-wheel
  # account's key must not become a root shell that can wipe the device.
  isAdmin = name: u: name == "root" || lib.elem "wheel" u.extraGroups || lib.elem name (config.users.groups.wheel.members or [ ]);
  authorizedKeys = lib.unique (lib.concatLists (lib.mapAttrsToList
    (name: u: lib.optionals (isAdmin name u) u.openssh.authorizedKeys.keys)
    config.users.users));

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

  # Recovery's local-only Wi-Fi path: hostapd owns AP mode; systemd-networkd below owns the
  # address and DHCP server. This is the standard Linux split and avoids pulling NetworkManager
  # into stage 1. NixOS hostapd's default is the same 2.4 GHz + ACS (`channel=0`) shape; drivers
  # without ACS support are a per-target recovery qualification failure, not a guessed channel.
  recoveryHotspot = pkgs.writeShellScript "mura-recovery-hotspot" ''
    set -eu
    radio=
    for candidate in /sys/class/net/*; do
      [ -d "$candidate/wireless" ] || continue
      radio="''${candidate##*/}"
      break
    done
    if [ -z "$radio" ]; then
      echo "mura-recovery-hotspot: no wireless interface; USB recovery remains available"
      ${config.boot.initrd.systemd.package}/bin/systemd-notify --ready
      exit 0
    fi

    ${pkgs.coreutils}/bin/mkdir -p /run/mura /run/mura-recovery /run/hostapd
    if [ -r ${hotspotEnv} ]; then
      # A service restart in the same recovery boot must not invalidate credentials already
      # shown on the panel.
      # shellcheck disable=SC1090
      . ${hotspotEnv}
      psk=$MURA_HOTSPOT_PSK
      ssid=$MURA_HOTSPOT_SSID
    else
      psk=$(printf '%08d' "$(( $(${pkgs.coreutils}/bin/od -An -N4 -tu4 /dev/urandom) % 100000000 ))")
      suffix="''${psk#????}"
      ssid="Mura-Recovery-$suffix"
    fi

    cat > /run/systemd/network/05-mura-recovery-hotspot.network <<EOF
    [Match]
    Name=$radio

    [Network]
    Address=${hotspotAddr}/24
    DHCPServer=yes
    ConfigureWithoutCarrier=yes
    LinkLocalAddressing=no
    IPv6AcceptRA=no

    [DHCPServer]
    PoolOffset=2
    PoolSize=19
    EmitDNS=no
    EmitRouter=no
    EOF
    ${config.boot.initrd.systemd.package}/bin/networkctl reload
    ${config.boot.initrd.systemd.package}/bin/networkctl reconfigure "$radio"

    cat > /run/mura-recovery/hostapd.conf <<EOF
    interface=$radio
    driver=nl80211
    ctrl_interface=/run/hostapd
    ssid=$ssid
    hw_mode=g
    channel=0
    wmm_enabled=1
    auth_algs=1
    wpa=2
    wpa_key_mgmt=WPA-PSK
    rsn_pairwise=CCMP
    wpa_passphrase=$psk
    EOF
    echo "mura-recovery-hotspot: SSID=$ssid interface=$radio (2.4 GHz ACS)"
    ${pkgs.hostapd}/bin/hostapd -B -P /run/mura-recovery/hostapd.pid \
      /run/mura-recovery/hostapd.conf
    pid=$(cat /run/mura-recovery/hostapd.pid)

    # The normal OOB hotspot gives activation 20 s (oob.nix); use that established bound.
    # Do not publish credentials to the panel until hostapd says the AP is actually enabled.
    i=0
    while [ "$i" -lt 20 ]; do
      if ${pkgs.hostapd}/bin/hostapd_cli -i "$radio" status 2>/dev/null \
          | ${pkgs.gnugrep}/bin/grep -qx 'state=ENABLED'; then
        if [ ! -r ${hotspotEnv} ]; then
          ${pkgs.coreutils}/bin/install -m 0600 /dev/null ${hotspotEnv}.tmp
          printf 'MURA_HOTSPOT_PSK=%s\nMURA_HOTSPOT_SSID=%s\n' "$psk" "$ssid" \
            > ${hotspotEnv}.tmp
          ${pkgs.coreutils}/bin/mv ${hotspotEnv}.tmp ${hotspotEnv}
        fi
        ${config.boot.initrd.systemd.package}/bin/systemd-notify --ready --pid="$pid"
        exit 0
      fi
      i=$((i + 1))
      sleep 1
    done
    kill "$pid" 2>/dev/null || true
    echo "mura-recovery-hotspot: AP did not become ready within 20 s" >&2
    exit 1
  '';

  # /etc/mura/recovery.json (specs/recovery-menu.md §6): the contract's button roles as evdev
  # codes with Android recovery's keyboard fallbacks appended (KEY_UP/DOWN/ENTER/ESC — also what
  # the VM test drives through QEMU's keyboard), the family's slot switch, the persist device.
  evdevCode = name: {
    KEY_POWER = 116;
    KEY_VOLUMEUP = 115;
    KEY_VOLUMEDOWN = 114;
    KEY_SELECT = 353;
    KEY_ENTER = 28;
    KEY_UP = 103;
    KEY_DOWN = 108;
    KEY_ESC = 1;
  }.${name} or (throw "recovery.nix: no evdev code for ${name}; extend the table");
  buttons = cfg.hardware.input.hmdButtons;
  roleCode = role: lib.optional (role != null && buttons ? ${role}) (evdevCode buttons.${role});
  recoveryConfig = pkgs.writeText "recovery.json" (builtins.toJSON {
    keys = {
      next = roleCode "volumeDown" ++ [ 108 ];
      prev = roleCode "volumeUp" ++ [ 103 ];
      select = roleCode cfg.hardware.input.selectRole ++ [ 28 ];
      back = roleCode cfg.hardware.input.backRole ++ [ 1 ];
    };
    switchSlotCommand = cfg.recovery.switchSlotCommand;
    persistDevice = if persistFs.device != "" then persistFs.device else "/dev/disk/by-partlabel/syspersist";
    gadgetAddr = gadgetAddr;
    docsUrl = docsUrl;
    longPressMs = 750;
  });
  recovery = lib.getExe pkgs.mura.recovery;
in
{
  options.mura.recovery = {
    rebootCommand = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        How stage 2 reboots into the recovery environment; set by the image family
        (uefi-rauc: `systemctl reboot --boot-loader-entry=recovery.conf`). When null the crash-loop
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
    # The panel frontend reads the HMD's buttons as raw evdev (/dev/input/event*): the event
    # interface must exist in stage 1 (a module on the NixOS kernel). The button drivers
    # themselves are the device's declaration (gpio-keys/pmic on the targets; PS/2 in the VM).
    boot.initrd.kernelModules = [ "evdev" ];
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
        "/etc/mura/recovery.json".source = recoveryConfig;
      };
      storePaths = [
        "${pkgs.openssh}/bin/sshd"
        "${pkgs.openssh}/bin/ssh-keygen"
        "${pkgs.openssh}/libexec/sshd-auth"
        "${pkgs.openssh}/libexec/sshd-session"
        recovery
        "${lib.getExe pkgs.mura.setup}"
        "${pkgs.hostapd}/bin/hostapd"
        "${pkgs.hostapd}/bin/hostapd_cli"
        recoveryHotspot
        waysIn
        plymouthSay
        "${pkgs.gnused}/bin/sed"
      ];
      extraBin = {
        sed = "${pkgs.gnused}/bin/sed";
        ssh-keygen = "${pkgs.openssh}/bin/ssh-keygen";
        mura-recovery = recovery;
      };

      # NixOS's repart module intentionally orders its service after sysroot.mount when no
      # explicit whole-disk device is configured: that is how it discovers the root disk
      # (nixpkgs repart.nix:202-210). Mura Recovery deliberately never mounts sysroot. Skip the
      # automatic grow/add pass in this mode; a confirmed reset invokes systemd-repart directly
      # with the whole disk resolved from the persist partition. This is systemd's standard
      # per-boot-mode condition shape (ConditionKernelCommandLine), not a failed-unit exception.
      services.systemd-repart.unitConfig.ConditionKernelCommandLine =
        "!rd.systemd.unit=mura-recovery.target";

      # `rd.systemd.unit=mura-recovery.target` makes this the initrd's default target in place of
      # initrd.target, so like initrd.target it must pull basic.target itself (journald, udev,
      # plymouth-start, the test driver's initrd backdoor all hang off sysinit/basic).
      targets.mura-recovery = {
        description = "Mura recovery environment (stage 1: panel, USB + hotspot ssh/web, offered reset)";
        requires = [ "basic.target" "mura-recovery-identity.service" "mura-recovery-sshd.service" "mura-recovery-panel.service" "mura-setup-recovery.service" ];
        wants = [ "mura-recovery-hotspot.service" ];
        after = [ "basic.target" "mura-recovery-identity.service" "mura-recovery-hotspot.service" "mura-recovery-sshd.service" "mura-recovery-panel.service" "mura-setup-recovery.service" ];
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

      services.mura-recovery-hotspot = {
        description = "Mura recovery: per-boot-PSK Wi-Fi hotspot (hostapd)";
        after = [ "systemd-udev-settle.service" ];
        wants = [ "systemd-udev-settle.service" ];
        serviceConfig = {
          ExecStart = recoveryHotspot;
          Type = "notify";
          NotifyAccess = "all";
          Restart = "on-failure";
          RestartSec = "2s";
        };
      };

      services.mura-recovery-sshd = {
        description = "Mura recovery: sshd on the USB gadget and recovery hotspot";
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

      # Plymouth is an optional surface, never a recovery availability dependency. pmOS waits
      # 10 s for its framebuffer and then continues (research/56 §5); apply the same bound to
      # plymouth-start. Units ordered after it proceed whether it starts or times out, avoiding
      # both a permanent dark-device hang and a one-shot ping race in the panel frontend.
      services.plymouth-start.serviceConfig.TimeoutStartSec = lib.mkDefault "10s";

      # The panel frontend (specs/recovery-menu.md §4–§5): the HMD's buttons over raw evdev
      # (register on release, long press ignored — Android recovery's semantics), the menu drawn
      # through plymouth; the ways-in lines drawn once and left standing. Runs for the life of
      # stage 1; a restart re-scans the input devices.
      services.mura-recovery-panel = {
        description = "Mura recovery: the panel menu (HMD buttons, plymouth)";
        after = [ "mura-recovery-identity.service" "mura-recovery-hotspot.service" "plymouth-start.service" "systemd-udev-settle.service" ];
        requires = [ "mura-recovery-identity.service" ];
        wants = [ "plymouth-start.service" "systemd-udev-settle.service" ];
        serviceConfig = {
          ExecStart = "${recovery} panel";
          Restart = "on-failure";
          RestartSec = "2s";
        };
      };

      # The web frontend (§7): the setup program's recovery instance on the gadget and hotspot
      # addresses — the keyless way in for a phone or a laptop on the cable. Root: no other
      # identity exists in stage 1.
      services.mura-setup-recovery = {
        description = "Mura recovery: the web page on the cable/hotspot addresses";
        after = [ "systemd-networkd.service" "mura-usb-gadget.service" "mura-recovery-identity.service" ];
        wants = [ "systemd-networkd.service" "mura-usb-gadget.service" ];
        requires = [ "mura-recovery-identity.service" ];
        serviceConfig = {
          ExecStart = "${lib.getExe pkgs.mura.setup} --recovery";
          Restart = "on-failure";
          RestartSec = "2s";
        };
      };
    };
  };
}
