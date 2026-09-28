# The out-of-band path (D3, modules/os/oob.nix) on the default-image fixture: the USB gadget
# through dummy_hcd (device and host side in the same kernel), the provisioning hotspot through
# two mac80211_hwsim radios (wlan0 = the headset, wlan1 = the "phone"), the mura-setup stub on
# the two trusted addresses only, and the setup-complete marker ending both.
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-oob";
  profileModules = [ ../../profiles/default.nix ];
  extraModules = [
    ({ lib, pkgs, ... }: {
      # Test-only: the phone side (wlan1) and the HOST side of the gadget (usb1 — dummy_hcd
      # puts both ends in this kernel) are driven by hand so they never count as "another
      # active NM connection" of the headset itself.
      networking.networkmanager.unmanaged = [ "interface-name:wlan1" "interface-name:eth*" "interface-name:usb1" ];
      environment.systemPackages = [ pkgs.wpa_supplicant pkgs.iw pkgs.curl pkgs.dnsutils pkgs.dhcpcd ];
      # a short idle timeout so the per-boot timeout is observable
      mura.oob.hotspot.idleTimeoutMinutes = lib.mkForce 1;
    })
  ];

  testScript = ''
    import re

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("pgrep -u mura -x zxr", timeout=120)

    with subtest("D3: the USB gadget exists from the initrd; usb0 carries 172.16.42.1 with a DHCP server"):
        machine.succeed("journalctl -b --no-pager | grep -q 'gadget bound to dummy_udc'")
        machine.succeed("test -d /sys/kernel/config/usb_gadget/mura/functions/ncm.usb0")
        machine.wait_until_succeeds("ip -4 addr show usb0 | grep -q 172.16.42.1/24", timeout=60)
        machine.succeed("networkctl status usb0 | grep -q 172.16.42.1")  # the DHCP server is proven by the host lease below
        # NM never touches the link (load-bearing for the hotspot condition)
        assert "unmanaged" in machine.succeed("nmcli -t -f DEVICE,STATE device status | grep '^usb0:'")

    with subtest("D3: the gadget's HOST side (dummy_hcd) gets a lease and reaches sshd and the setup page"):
        # cdc_ncm bound the host end of the same gadget; find the interface that is not usb0
        host = machine.wait_until_succeeds("ls /sys/bus/usb/drivers/cdc_ncm/*/net/ | head -1", timeout=60).strip()
        assert host and host != "usb0", f"host-side interface: {host!r}"
        dbg = machine.execute(f"ip link set {host} up; networkctl status usb0; networkctl list; timeout 60 dhcpcd -4 -d --noipv4ll --waitip=4 --nohook resolv.conf {host} 2>&1 | tail -20")[1]
        hip = machine.succeed(f"ip -4 -o addr show {host} | awk '{{print $4}}'").strip()
        assert hip.startswith("172.16.42.") and not hip.startswith("172.16.42.1/"), f"{hip}\n{dbg}"
        machine.succeed("timeout 30 ssh -o StrictHostKeyChecking=no -o BatchMode=yes -i /etc/mura-test/fixture-ssh-key mura@172.16.42.1 true")
        page = machine.succeed("curl -sS --max-time 10 http://172.16.42.1/")
        assert "mura.local" in page, page
        # captive-portal probes redirect to the launcher
        assert "302" in machine.succeed("curl -s -o /dev/null -w '%{http_code}' --max-time 10 http://172.16.42.1/generate_204")

    with subtest("D3: mura-setup runs under its own identity, conditioned on the marker, never on the LAN"):
        machine.succeed("systemctl is-active mura-setup.service")
        assert machine.succeed("systemctl show -p User --value mura-setup.service").strip() == "mura-setup"
        # `systemctl show` renders conditions as "Conditions=..."; `systemctl cat` shows the source
        machine.succeed("systemctl cat mura-setup.service | grep -q 'ConditionPathExists=!/var/lib/mura/state/setup/setup-complete'")
        machine.succeed("grep -Rq 'subject.user == \"mura-setup\"' /etc/polkit-1/rules.d/")
        lan = machine.succeed("ip -4 -o addr show eth0 | awk '{print $4}' | cut -d/ -f1").strip()
        machine.fail(f"curl -sS --max-time 5 http://{lan}/")
        machine.succeed("stat -c %a /var/lib/mura/state/setup | grep -qx 1777")

    with subtest("D3: the hotspot is up with a per-boot PSK while nothing else is connected"):
        try:
            machine.wait_until_succeeds("nmcli -t -f NAME connection show --active | grep -qx mura-setup", timeout=120)
        except Exception:
                raise
        env = machine.succeed("cat /run/mura/hotspot.env")
        m = re.search(r"MURA_HOTSPOT_PSK=(\d{8})", env)
        n = re.search(r"MURA_HOTSPOT_SSID=(Mura-[0-9a-f]{4})", env)
        assert m and n, f"hotspot.env malformed:\n{env}"
        psk, ssid = m.group(1), n.group(1)
        machine.succeed("journalctl -b --no-pager -u mura-hotspot-psk | grep -q 'SSID='")
        machine.succeed("ip -4 addr show | grep -q 10.42.0.1/24")
        machine.succeed("test -e /etc/NetworkManager/dnsmasq-shared.d/mura-portal.conf")

    with subtest("D3: the phone joins only with the PSK, is captured, and reaches the launcher"):
        # wrong PSK: association fails within the timeout
        machine.succeed(f"printf 'network={{\\n ssid=\"{ssid}\"\\n psk=\"00000000\"\\n}}\\n' > /tmp/wrong.conf")
        machine.succeed("wpa_supplicant -B -i wlan1 -c /tmp/wrong.conf -P /tmp/wpa.pid")
        machine.sleep(8)
        machine.fail("iw dev wlan1 link | grep -q Connected")
        machine.succeed("kill $(cat /tmp/wpa.pid); sleep 1")
        # right PSK
        machine.succeed(f"printf 'network={{\\n ssid=\"{ssid}\"\\n psk=\"{psk}\"\\n}}\\n' > /tmp/right.conf")
        machine.succeed("wpa_supplicant -B -i wlan1 -c /tmp/right.conf -P /tmp/wpa.pid")
        machine.wait_until_succeeds("iw dev wlan1 link | grep -q Connected", timeout=60)
        machine.succeed("timeout 60 dhcpcd -4 -q --waitip=4 --nohook resolv.conf wlan1")
        machine.succeed("ip -4 -o addr show wlan1 | grep -q 10.42.0.")
        # dnsmasq wildcard: every name is the headset
        assert "10.42.0.1" in machine.succeed("dig +short +time=5 @10.42.0.1 mura.local A")
        assert "10.42.0.1" in machine.succeed("dig +short +time=5 @10.42.0.1 connectivitycheck.gstatic.com A")
        # (no --interface: with both ends in one kernel, SO_BINDTODEVICE would drop the reply
        # that arrives over lo; the phone's request is a plain TCP connect to the address)
        assert "302" in machine.succeed("curl -s -o /dev/null -w '%{http_code}' --max-time 10 http://10.42.0.1/hotspot-detect.html")
        assert "mura.local" in machine.succeed("curl -sS --max-time 10 http://10.42.0.1/")
        # a station is associated: the idle timer does not run
        machine.succeed("iw dev wlan0 station dump | grep -q Station")

    with subtest("D3: the hotspot yields to any other active connection and returns when it goes"):
        machine.succeed("nmcli connection add type dummy ifname dummy0 con-name test-uplink ipv4.method manual ipv4.addresses 192.0.2.1/24 connection.autoconnect no")
        machine.succeed("nmcli connection up test-uplink")
        machine.wait_until_fails("nmcli -t -f NAME connection show --active | grep -qx mura-setup", timeout=60)
        machine.succeed("nmcli connection delete test-uplink")
        machine.wait_until_succeeds("nmcli -t -f NAME connection show --active | grep -qx mura-setup", timeout=90)

    with subtest("D3: the idle timeout takes the radio down for this boot only"):
        machine.succeed("kill $(cat /tmp/wpa.pid); sleep 1; ip addr flush dev wlan1")
        machine.wait_until_fails("nmcli -t -f NAME connection show --active | grep -qx mura-setup", timeout=180)
        machine.succeed("journalctl -b --no-pager -u mura-hotspot | grep -q 'hotspot down for this boot'")
        machine.succeed("systemctl is-active mura-hotspot.service")   # the supervisor stays; the web app too
        machine.succeed("systemctl is-active mura-setup.service")

    with subtest("D3: finishing setup on the web app writes the marker and retires hotspot + web app"):
        machine.succeed("curl -sS --max-time 10 -X POST http://172.16.42.1/finish")
        machine.succeed("test -e /var/lib/mura/state/setup/setup-complete")
        assert machine.succeed("stat -c %U /var/lib/mura/state/setup/setup-complete").strip() == "mura-setup"
        machine.wait_until_fails("systemctl is-active mura-hotspot.service", timeout=60)
        machine.wait_until_fails("systemctl is-active mura-setup.service", timeout=60)
        # the condition refuses a restart while the marker exists
        machine.fail("systemctl start mura-setup.service && systemctl is-active mura-setup.service")
        machine.fail("curl -sS --max-time 5 http://172.16.42.1/")

    with subtest("D3: the marker survives a reboot; removing it (factory reset) brings both back"):
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.fail("systemctl is-active mura-setup.service")
        machine.fail("systemctl is-active mura-hotspot.service")
        machine.succeed("rm /var/lib/mura/state/setup/setup-complete")
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.wait_for_unit("mura-setup.service")
        machine.wait_until_succeeds("nmcli -t -f NAME connection show --active | grep -qx mura-setup", timeout=120)
  '';
}
