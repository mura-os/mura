# modules/os/oob.nix — out-of-band access (implementation-path §2 (iii) F3, D3).
# first-run-onboarding.md §5 as code: the headset is a Linux host reachable from a device you
# already hold — over a USB Ethernet gadget from the initramfs (postmarketOS's pattern), and,
# until the wearer finishes setup, over a headset-hosted WPA2 hotspot with a captive portal that
# lands on the setup web app. sshd itself is modules/os/default.nix's (upstream defaults).
#
#   USB gadget   : configfs NCM function from the initrd; usb0 = 172.16.42.1/24 with a
#                  systemd-networkd DHCP server; NetworkManager never manages the link (the
#                  hotspot condition counts NM's active connections).
#   hotspot      : NM AP profile `mura-setup`, per-boot 8-digit PSK (shown in-headset from G1;
#                  journal/serial on the dev profile), dnsmasq-shared.d wildcard + option 114;
#                  up while `setup-complete` is absent AND no other NM connection is active;
#                  per-boot idle timeout with no station associated.
#   mura-setup   : the setup program's SYSTEM instance (first-run §5.1) — own identity, scoped
#                  polkit rules, HTTP on the gadget + hotspot addresses only (never the LAN),
#                  ConditionPathExists=!setup-complete. D3 ships the launcher + a stub page;
#                  the web UI is its own rung.
#   marker       : /var/lib/mura/state/setup/setup-complete (sticky dir, persist skeleton).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura;
  marker = "/var/lib/mura/state/setup/setup-complete";
  gadgetAddr = "172.16.42.1";
  hotspotAddr = "10.42.0.1";
  psks = "/run/mura/hotspot.env";

  # The setup web app, system instance — D3 STUB (pkgs/mura-setup, Rust): the captive-portal
  # launcher and the probe redirects, bound to the two trusted addresses with IP_FREEBIND (the
  # hotspot address exists only while the AP is up), POST /finish writes the marker, and the
  # process exits when the marker appears. Replaced by the real mura-setup at its own rung; the
  # identity, unit shape and addresses are the contract.
  setupStub = pkgs.mura.setup;

  # Hotspot lifecycle (first-run §5): a small supervisor rather than NM autoconnect, because
  # the rule has two inputs NM cannot express — the marker and "no other active connection".
  hotspotCtl = pkgs.writeShellApplication {
    name = "mura-hotspot";
    runtimeInputs = [ config.networking.networkmanager.package pkgs.iw pkgs.coreutils pkgs.gnugrep pkgs.gawk ];
    text = ''
      idle_limit=$(( ${toString cfg.oob.hotspot.idleTimeoutMinutes} * 60 ))
      idle=0
      timed_out=0
      ap_dev=""
      while :; do
        if [ -e "${marker}" ]; then
          nmcli -w 5 connection down mura-setup >/dev/null 2>&1 || true
          echo "setup complete; hotspot retired for good (administrator setting from here on)"
          exit 0
        fi
        # active connections other than the AP itself; NM lists `lo` as loopback since 1.42
        others=$(nmcli -t -f NAME,TYPE connection show --active | grep -v '^mura-setup:' | grep -v ':loopback$' || true)
        ap_state=$(nmcli -t -f NAME,DEVICE connection show --active | awk -F: '$1=="mura-setup"{print $2}')
        if [ -n "$others" ] || [ "$timed_out" = 1 ]; then
          if [ -n "$ap_state" ]; then
            echo "another connection is active (or idle-timed out); taking the hotspot down"
            nmcli -w 10 connection down mura-setup >/dev/null 2>&1 || true
          fi
        else
          if [ -z "$ap_state" ]; then
            echo "no active connection and setup unfinished; bringing the hotspot up"
            nmcli -w 20 connection up mura-setup >/dev/null 2>&1 || echo "hotspot up failed (no radio?)"
          else
            ap_dev="$ap_state"
            if iw dev "$ap_dev" station dump 2>/dev/null | grep -q '^Station'; then
              idle=0
            else
              idle=$(( idle + 5 ))
              if [ "$idle" -ge "$idle_limit" ]; then
                echo "no station for ${toString cfg.oob.hotspot.idleTimeoutMinutes} min; hotspot down for this boot"
                timed_out=1
              fi
            fi
          fi
        fi
        sleep 5
      done
    '';
  };
in
{
  config = lib.mkMerge [
    ## USB Ethernet gadget (§5.4) ---------------------------------------------------------
    (lib.mkIf cfg.hardware.input.usbGadget {
      boot.initrd.kernelModules = [ "libcomposite" "usb_f_ncm" ];
      boot.initrd.systemd.services.mura-usb-gadget = {
        description = "Mura USB Ethernet gadget (configfs, NCM)";
        wantedBy = [ "initrd.target" ];
        after = [ "systemd-modules-load.service" "sys-kernel-config.mount" ];
        requires = [ "sys-kernel-config.mount" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        # pmOS init_functions.sh is the configfs/NCM mechanism precedent, not the identity
        # precedent (pmOS actually defaults to Google's 18d1:d001; research/55).
        # idVendor/idProduct: Linux Foundation's legacy multifunction-gadget example identity.
        # Kept as Mura's development placeholder while the shipping identity strategy is open;
        # it does not match this NCM-only composition and is not a Mura allocation.
        # Serial: fixed per device model in stage 1; a per-unit value (hash of machine-id)
        # would need /persist's etc-rw in the initrd — recorded as a later item.
        script = ''
          g=/sys/kernel/config/usb_gadget/mura
          [ -d /sys/kernel/config/usb_gadget ] || { echo "no usb_gadget in configfs; skipping"; exit 0; }
          udc=$(${pkgs.coreutils}/bin/ls /sys/class/udc 2>/dev/null | ${pkgs.coreutils}/bin/head -n1)
          [ -n "$udc" ] || { echo "no UDC; skipping gadget"; exit 0; }
          ${pkgs.coreutils}/bin/mkdir -p "$g/strings/0x409" "$g/configs/c.1/strings/0x409" "$g/functions/ncm.usb0"
          echo 0x1d6b > "$g/idVendor"
          echo 0x0104 > "$g/idProduct"
          echo Mura > "$g/strings/0x409/manufacturer"
          echo ${lib.escapeShellArg cfg.device.codename} > "$g/strings/0x409/product"
          echo mura-${cfg.device.codename} > "$g/strings/0x409/serialnumber"
          echo "USB Ethernet (NCM)" > "$g/configs/c.1/strings/0x409/configuration"
          echo 250 > "$g/configs/c.1/MaxPower"
          [ -e "$g/configs/c.1/ncm.usb0" ] || ${pkgs.coreutils}/bin/ln -s "$g/functions/ncm.usb0" "$g/configs/c.1/"
          echo "$udc" > "$g/UDC"
          echo "gadget bound to $udc"
        '';
      };

      # Stage 2: the link is systemd-networkd's, never NetworkManager's — an NM-managed usb0
      # would count as "another active connection" and kill the hotspot (first-run §5).
      systemd.network.enable = true;
      systemd.network.networks."10-mura-usb0" = {
        matchConfig.Name = "usb0";
        address = [ "${gadgetAddr}/24" ];
        networkConfig = {
          DHCPServer = true;
          LinkLocalAddressing = "no";
          IPv6AcceptRA = false;
        };
        dhcpServerConfig = {
          PoolOffset = 2;
          PoolSize = 19;
          EmitDNS = false;
          EmitRouter = false;
        };
        linkConfig.RequiredForOnline = "no";
      };
      networking.networkmanager.unmanaged = [ "interface-name:usb0" ];
      # The DHCP server and the setup page on the cable (sshd's 22 is open everywhere already).
      networking.firewall.interfaces.usb0 = {
        allowedUDPPorts = [ 67 ];
        allowedTCPPorts = [ 80 ];
      };
    })

    ## Hotspot + captive portal (§5, §5.2, §5.4) ------------------------------------------
    {
      networking.networkmanager.enable = true;
      # Shared mode's DHCP/DNS accept rules for the AP interface exist only in NM's iptables
      # backend (the nftables backend writes NAT/forward rules alone and assumes firewalld);
      # with the NixOS firewall on, the phone would associate and never get a lease (D3).
      networking.networkmanager.settings.main."firewall-backend" = lib.mkDefault "iptables";

      # Per-boot PSK and SSID for the NM profile below (envsubst in ensure-profiles).
      systemd.services.mura-hotspot-psk = {
        description = "Mura provisioning hotspot: per-boot PSK";
        wantedBy = [ "multi-user.target" ];
        before = [ "NetworkManager-ensure-profiles.service" ];
        requiredBy = [ "NetworkManager-ensure-profiles.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          ${pkgs.coreutils}/bin/mkdir -p /run/mura
          psk=$(printf '%08d' "$(( $(${pkgs.coreutils}/bin/od -An -N4 -tu4 /dev/urandom) % 100000000 ))")
          ssid="Mura-$(${pkgs.coreutils}/bin/tail -c 5 /etc/machine-id | ${pkgs.coreutils}/bin/head -c 4)"
          ${pkgs.coreutils}/bin/install -m 0600 /dev/null ${psks}
          printf 'MURA_HOTSPOT_PSK=%s\nMURA_HOTSPOT_SSID=%s\n' "$psk" "$ssid" > ${psks}
          # The in-headset display is a compositor scene (G1+); until then the dev profile's
          # journal/serial console is where a developer reads it (first-run §5.4).
          echo "provisioning hotspot SSID=$ssid PSK=$psk"
        '';
      };

      networking.networkmanager.ensureProfiles = {
        environmentFiles = [ psks ];
        profiles.mura-setup = {
          connection = {
            id = "mura-setup";
            type = "wifi";
            autoconnect = false; # mura-hotspot.service decides (marker + no other connection)
          };
          wifi = {
            mode = "ap";
            ssid = "$MURA_HOTSPOT_SSID";
            band = "bg";
          };
          wifi-security = {
            key-mgmt = "wpa-psk";
            psk = "$MURA_HOTSPOT_PSK";
          };
          ipv4 = {
            method = "shared";
            address1 = "${hotspotAddr}/24";
          };
          ipv6.method = "disabled";
        };
      };

      # NM's shared mode opens dnsmasq's DHCP/DNS ports on the AP interface itself; the setup
      # page on the hotspot address is ours to open. Destination-based because the AP's
      # interface name is the device's, not ours (iptables backend, the NixOS default).
      networking.firewall.extraCommands = lib.mkIf (!config.networking.nftables.enable) ''
        iptables -A nixos-fw -d ${hotspotAddr} -p tcp --dport 80 -j nixos-fw-accept
      '';
      networking.firewall.extraInputRules = lib.mkIf config.networking.nftables.enable ''
        ip daddr ${hotspotAddr} tcp dport 80 accept
      '';

      # Every name resolves to the headset on the hotspot (mura.local without phone mDNS) and
      # RFC 8910 option 114 carries the launcher URL — balena wifi-connect / comitup mechanics.
      environment.etc."NetworkManager/dnsmasq-shared.d/mura-portal.conf".text = ''
        address=/#/${hotspotAddr}
        dhcp-option=114,http://${hotspotAddr}/
      '';

      systemd.services.mura-hotspot = {
        description = "Mura provisioning hotspot (up while setup is unfinished and nothing else is connected)";
        wantedBy = [ "multi-user.target" ];
        after = [ "NetworkManager.service" "NetworkManager-ensure-profiles.service" "mura-persist-setup.service" ];
        requires = [ "NetworkManager-ensure-profiles.service" ];
        partOf = [ "mura-setup.service" ];
        unitConfig.ConditionPathExists = "!${marker}";
        serviceConfig = {
          ExecStart = lib.getExe hotspotCtl;
          ExecStopPost = "-${config.networking.networkmanager.package}/bin/nmcli connection down mura-setup";
          Restart = "on-failure";
        };
      };
    }

    ## mura-setup: identity, polkit, the HTTP surface (§5.1, §5.3) ------------------------
    {
      users.users.mura-setup = {
        isSystemUser = true;
        group = "mura-setup";
        description = "Mura setup program (system instance)";
      };
      users.groups.mura-setup = { };

      # Exactly the setup actions, for exactly this identity — the gnome-initial-setup shape
      # (references/gnome-initial-setup/data/20-gnome-initial-setup.rules.in) narrowed from
      # prefixes to actions (multi-user.md §3.1). The privileged work is the standard daemons'.
      security.polkit.extraConfig = ''
        /* Mura: the setup program's system instance may perform the setup actions. */
        polkit.addRule(function(action, subject) {
          if (subject.user == "mura-setup" && (
              action.id == "org.freedesktop.NetworkManager.settings.modify.system" ||
              action.id == "org.freedesktop.NetworkManager.network-control" ||
              action.id == "org.freedesktop.NetworkManager.wifi.scan" ||
              action.id == "org.freedesktop.timedate1.set-timezone" ||
              action.id == "org.freedesktop.hostname1.set-static-hostname" ||
              action.id == "org.freedesktop.hostname1.set-hostname" ||
              action.id == "org.freedesktop.accounts.user-administration")) {
            return polkit.Result.YES;
          }
        });
      '';

      systemd.services.mura-setup = {
        description = "Mura setup program, system instance (web app on the gadget and hotspot addresses)";
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" "mura-persist-setup.service" ];
        unitConfig.ConditionPathExists = "!${marker}";
        serviceConfig = {
          ExecStart = lib.getExe setupStub; # D3 STUB — replaced by the real mura-setup at its rung
          User = "mura-setup";
          Group = "mura-setup";
          AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
          CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];
          NoNewPrivileges = true;
          Restart = "on-failure";
          RestartSec = "2s";
        };
      };

      # `mura.local` on the cable and the LAN (on the hotspot dnsmasq's wildcard answers).
      services.avahi = {
        enable = lib.mkDefault true;
        hostName = lib.mkDefault "mura"; # the launcher says mura.local, whatever the hostname
        nssmdns4 = lib.mkDefault true;
        publish = {
          enable = lib.mkDefault true;
          addresses = lib.mkDefault true;
        };
      };
    }
  ];
}
