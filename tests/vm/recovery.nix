# The recovery environment (modules/os/recovery.nix; implementation-path §4 "Mura recovery
# environment"; research/57) on the default-image fixture, booted straight into it with the
# kernel parameter the family's recovery entry carries (`rd.systemd.unit=mura-recovery.target`).
# The NixOS test driver's initrd backdoor (`testing.initrdBackdoor`) lets the script run inside
# stage 1, where the environment lives. Proves: sshd on the gadget address with the device's own
# host key when /persist is readable (else a generated one, fingerprint shown), and the three
# frontends of one menu (specs/recovery-menu.md §7): the panel driven by keys — QEMU's PS/2
# keyboard stands in for the HMD's buttons through the keyboard fallback codes (§4.5), register
# on release, a held key ignored, Confirm defaulting to Cancel — the shell over ssh from the
# gadget's host end (dummy_hcd), and the web page on the gadget address refusing an unconfirmed
# reset. The factory reset itself — systemd-repart --factory-reset over the FactoryReset=yes
# syspersist partition — wipes and re-creates it, once through the web form and once through the
# panel's confirm screen. The counter → recovery-entry reboot needs an ESP: deckard's manual proof.
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-recovery";
  profileModules = [ ../../profiles/default.nix ];
  extraModules = [
    ({ pkgs, ... }: {
      # TEST-ONLY: an ordinary (non-wheel) account with a declared key — its key must NOT open
      # the recovery environment's root shell.
      users.users.guest = {
        isNormalUser = true;
        openssh.authorizedKeys.keys = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA guest-not-an-admin" ];
      };
      testing.initrdBackdoor = true;
      boot.kernelParams = [ "rd.systemd.unit=mura-recovery.target" ];
      boot.initrd.kernelModules = [ "cdc_ncm" ]; # the HOST end's class driver, in stage 1 for the test
      boot.initrd.systemd = {
        # TEST-ONLY: an ssh client and the fixture key inside stage 1, and an address on the
        # gadget's host end (usb1 — the same kernel is both sides through dummy_hcd), so the
        # script can log in to the recovery sshd the way a laptop on the cable would.
        extraBin = {
          ssh = "${pkgs.openssh}/bin/ssh";
          ip = "${pkgs.iproute2}/bin/ip";
          curl = "${pkgs.curl}/bin/curl";
        };
        # TEST-ONLY: the reset must not reboot the VM under the script (the action honours this)
        services.mura-recovery-panel.environment.MURA_RECOVERY_NO_REBOOT = "1";
        services.mura-setup-recovery.environment.MURA_RECOVERY_NO_REBOOT = "1";
        contents."/etc/mura-test/fixture-ssh-key".source = ./fixture-ssh-key;
        # A static host-end address: the gadget's DHCP server is stage 2's proof (tests/vm/oob.nix);
        # one networkd instance serving and requesting on two ends of the same emulated cable is
        # not what a laptop does.
        network.networks."20-test-host-end" = {
          matchConfig.Name = "usb1";
          address = [ "172.16.42.5/24" ];
        };
      };
    })
  ];

  testScript = ''
    from datetime import timedelta
    import time

    machine.start()
    # stage 1 only: the recovery target, never a root mount
    machine.wait_for_unit("mura-recovery.target")

    def panel():
        return machine.succeed("cat /run/mura-recovery/panel.txt")

    def key(name):
        # one key at a time: QEMU holds a key ~100 ms, and a second make code while the first is
        # still down is a kernel auto-repeat, which the panel ignores by design (§4.1)
        machine.send_key(name)
        time.sleep(0.3)

    def partuuids():
        return machine.succeed("ls /dev/disk/by-partuuid 2>/dev/null || true").split()

    def assert_reset(before):
        # the re-created partition has a new UUID; wait for udev to publish it, then look inside.
        # (Mounting while the reset runs would itself block it: the kernel refuses to drop a
        # partition that is in use.)
        def recreated(_):
            now = partuuids()
            return now and now != before
        retry(recreated, timeout=timedelta(seconds=60))
        machine.succeed("udevadm settle")
        machine.wait_until_succeeds("test -e /dev/disk/by-partlabel/syspersist", timeout=30)
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt")
        machine.fail("test -e /mnt/mura")                      # empty, re-created
        machine.succeed("umount /mnt")

    def panel_shows(text, timeout=20):
        # (no grep in stage 1; poll the panel's text from here)
        def shown(_):
            return text in machine.succeed("cat /run/mura-recovery/panel.txt 2>/dev/null || true")
        retry(shown, timeout=timedelta(seconds=timeout))
        return panel()

    with subtest("recovery: the environment is up — sshd on the gadget address, a host key, the panel menu on plymouth"):
        machine.succeed("systemctl is-active mura-recovery-identity.service mura-recovery-sshd.service mura-recovery-panel.service mura-setup-recovery.service")
        machine.fail("systemctl is-active initrd-root-fs.target")
        machine.fail("test -e /sysroot/etc/os-release")
        fp = machine.succeed("cat /run/mura-recovery/fingerprint").strip()
        assert fp.startswith("SHA256:"), fp
        # a fresh disk has no persist yet: the key was generated for this session
        assert machine.succeed("cat /run/mura-recovery/keysource").strip() == "generated"
        # plymouth is up (virtio_gpu in the initrd; the serial console is ignored) and the menu is drawn
        assert machine.succeed("cat /run/mura-recovery/panel.status").strip() == "plymouth"
        first = panel_shows("> Try again")
        assert "Factory reset" in first and "Power off" in first and len(first.encode()) <= 200, first
        status = machine.succeed("mura-recovery action status")
        assert "ssh root@172.16.42.1" in status and fp in status, status
        # the contract's roles landed in the config: keyboard fallbacks appended
        cfg = machine.succeed("cat /etc/mura/recovery.json")
        assert '"next":[114,108]' in cfg and '"select":[115,28]' in cfg and '"back":[114,1]' in cfg, cfg
        def gadget_configured(_):
            return "172.16.42.1" in machine.succeed("networkctl status usb0 2>/dev/null || true")
        retry(gadget_configured, timeout=timedelta(seconds=60))
        print(machine.succeed("networkctl list"))

    with subtest("recovery: panel — a key registers on release, moves wrap, Confirm defaults to Cancel, a held key is ignored"):
        key("down")
        assert "> Factory reset" in panel_shows("> Factory reset")
        key("down"); key("down"); key("down")
        assert "> Try again" in panel_shows("> Try again")          # 4 items, wrapped
        key("up")
        assert "> Show details" in panel_shows("> Show details")
        key("ret")
        assert "ssh root@172.16.42.1" in panel_shows("ssh root@")   # Details: the ways in
        key("ret")
        panel_shows("> Show details")                                # any key returns
        key("up"); key("up")
        key("ret")                                      # Factory reset -> Confirm
        confirm = panel_shows("> Cancel")
        assert "Erase everything" in confirm and "CANNOT BE UNDONE" in confirm, confirm
        key("esc")                                      # Back on Confirm returns to Main
        def on_main(_):
            text = panel()
            return "> Factory reset" in text and "Erase everything" not in text
        retry(on_main, timeout=timedelta(seconds=20))
        key("ret")                                      # Confirm again
        panel_shows("> Cancel")
        key("ret")                                      # Cancel is the default
        retry(on_main, timeout=timedelta(seconds=20))
        before = panel()
        # a held key (QEMU holds it 2 s; the kernel auto-repeats) is one long release: ignored
        machine.send_monitor_command("sendkey down 2000")
        time.sleep(3)
        assert panel() == before, panel()
        key("esc")                                      # Back on Main: nothing
        assert panel() == before, panel()

    with subtest("recovery: shell — the same menu over ssh from the cable's host end, with the administrator's key"):
        machine.succeed("cp /etc/mura-test/fixture-ssh-key /tmp/key && chmod 600 /tmp/key")
        def host_configured(_):
            return "172.16.42.5" in machine.succeed("networkctl status usb1 2>/dev/null || true")
        retry(host_configured, timeout=timedelta(seconds=60))
        ssh = "ssh -i /tmp/key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 root@172.16.42.1"
        out = machine.succeed(ssh + " mura-recovery shell < /dev/null 2>&1")
        assert "Try again" in out and "Factory reset" in out and "Power off" in out, out
        assert "Host key " + fp in out, out   # the banner carries the fingerprint the panel shows
        # only administrators' keys open recovery: the fixture key belongs to mura (wheel); guest's does not
        keys = machine.succeed("cat /etc/ssh/authorized_keys.d/root")
        assert "guest-not-an-admin" not in keys, keys
        assert "ssh-ed25519" in keys, keys

    with subtest("recovery: the device's own host key is used once /persist is readable"):
        # create syspersist as a normal first boot would (systemd-repart, the FactoryReset=yes
        # definition from vm-persist.nix), and give it an identity key
        machine.succeed("systemd-repart --dry-run=no --empty=allow --definitions=/etc/repart.d /dev/vdb")
        machine.wait_until_succeeds("test -e /dev/disk/by-partlabel/syspersist", timeout=30)
        machine.succeed("mkdir -p /mnt && mount /dev/disk/by-partlabel/syspersist /mnt")
        machine.succeed("mkdir -p /mnt/mura/identity/ssh && ssh-keygen -q -t ed25519 -N \"\" -f /mnt/mura/identity/ssh/ssh_host_ed25519_key && echo hello > /mnt/mura/marker")
        own = machine.succeed("ssh-keygen -lf /mnt/mura/identity/ssh/ssh_host_ed25519_key.pub | cut -d\" \" -f2").strip()
        machine.succeed("umount /mnt")
        machine.succeed("systemctl restart mura-recovery-identity.service mura-recovery-panel.service")
        assert machine.succeed("cat /run/mura-recovery/fingerprint").strip() == own
        assert machine.succeed("cat /run/mura-recovery/keysource").strip() == "own"
        panel_shows("> Try again")

    with subtest("recovery: shell — a wrong confirmation erases nothing"):
        out = machine.succeed("printf \"2\\nno\\n\" | " + ssh + " mura-recovery shell 2>&1")
        assert "Not erased" in out, out
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt && test -f /mnt/mura/marker && umount /mnt")
        # and the action alone refuses without the frontend's confirmation
        machine.fail("mura-recovery action factory-reset")

    with subtest("recovery: web — the page on the gadget address; an unconfirmed reset is refused, a confirmed one wipes"):
        page = machine.succeed("curl -sf http://172.16.42.1/")
        assert "Mura recovery" in page and own in page and "confirm" in page, page
        code = machine.succeed("curl -s -o /dev/null -w %{http_code} -X POST http://172.16.42.1/factory-reset").strip()
        assert code == "400", code
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt && test -f /mnt/mura/marker && umount /mnt")
        # a mount left behind by someone inspecting the partition is unmounted first (Android
        # recovery's EraseVolume shape); the reset then proceeds
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt")
        before = partuuids()
        out = machine.succeed("curl -sf -d confirm=erase http://172.16.42.1/factory-reset")
        assert "erasing" in out, out      # the response goes out first; the action runs after it
        assert_reset(before)

    with subtest("recovery: panel — Confirm's second item wipes and re-creates exactly the FactoryReset=yes partition"):
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt && mkdir -p /mnt/mura && echo hello > /mnt/mura/marker && umount /mnt")
        panel_shows("> Try again")
        key("down")
        panel_shows("> Factory reset")
        key("ret")
        panel_shows("> Cancel")
        key("down")
        panel_shows("> Erase everything")
        before = partuuids()
        key("ret")
        # back on Main after the reset (the selection stays where it was); the reboot is skipped
        # for the test
        def back_on_main(_):
            text = panel()
            return "> Factory reset" in text and "Erase everything" not in text
        retry(back_on_main, timeout=timedelta(seconds=60))
        assert_reset(before)
        # the disk still has exactly one partition (nothing else was touched)
        assert machine.succeed("ls /dev/vdb* | wc -l").strip() == "2", machine.succeed("ls -la /dev/vdb*")
  '';
}
