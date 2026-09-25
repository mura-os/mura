# The recovery environment (modules/os/recovery.nix; implementation-path §4 "Mura recovery
# environment"; research/57) on the default-image fixture, booted straight into it with the
# kernel parameter the family's recovery entry carries (`rd.systemd.unit=mura-recovery.target`).
# The NixOS test driver's initrd backdoor (`testing.initrdBackdoor`) lets the script run inside
# stage 1, where the environment lives. Proves: sshd on the gadget address with the device's own
# host key when /persist is readable (else a generated one, fingerprint shown), the panel screen,
# the menu over ssh from the gadget's host end (dummy_hcd), and the factory reset — systemd-repart
# --factory-reset over the FactoryReset=yes syspersist partition — wiping and re-creating it.
# The counter → recovery-entry reboot needs an ESP and is the deckard image's manual proof.
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-recovery";
  profileModules = [ ../../profiles/default.nix ];
  extraModules = [
    ({ pkgs, ... }: {
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
        };
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

    machine.start()
    # stage 1 only: the recovery target, never a root mount
    machine.wait_for_unit("mura-recovery.target")

    with subtest("recovery: the environment is up — sshd on the gadget address, a host key, the panel screen"):
        machine.succeed("systemctl is-active mura-recovery-identity.service mura-recovery-sshd.service mura-recovery-screen.service")
        machine.fail("systemctl is-active initrd-root-fs.target")
        machine.fail("test -e /sysroot/etc/os-release")
        fp = machine.succeed("cat /run/mura-recovery/fingerprint").strip()
        assert fp.startswith("SHA256:"), fp
        # a fresh disk has no persist yet: the key was generated for this session
        assert machine.succeed("cat /run/mura-recovery/keysource").strip() == "generated"
        screen = machine.succeed("cat /run/mura-recovery/screen.txt")
        assert "ssh root@172.16.42.1" in screen and fp in screen and "mura-recovery" in screen, screen
        # plymouth drew it (the VM has virtio_gpu in the initrd; the serial console is ignored)
        assert machine.succeed("cat /run/mura-recovery/screen.status").strip() == "shown"
        def gadget_configured(_):
            return "172.16.42.1" in machine.succeed("networkctl status usb0 2>/dev/null || true")
        retry(gadget_configured, timeout=timedelta(seconds=60))
        print(machine.succeed("networkctl list"))

    with subtest("recovery: the menu over ssh from the cable's host end, with the administrator's key"):
        machine.succeed("cp /etc/mura-test/fixture-ssh-key /tmp/key && chmod 600 /tmp/key")
        def host_configured(_):
            return "172.16.42.5" in machine.succeed("networkctl status usb1 2>/dev/null || true")
        retry(host_configured, timeout=timedelta(seconds=60))
        out = machine.succeed("ssh -i /tmp/key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 root@172.16.42.1 mura-recovery 2>&1")
        assert "Mura recovery" in out and "factory-reset" in out and "reboot" in out, out
        assert "Host key " + fp in out, out   # the banner carries the fingerprint the panel shows

    with subtest("recovery: the device's own host key is used once /persist is readable"):
        # create syspersist as a normal first boot would (systemd-repart, the FactoryReset=yes
        # definition from vm-persist.nix), and give it an identity key
        machine.succeed("systemd-repart --dry-run=no --empty=allow --definitions=/etc/repart.d /dev/vdb")
        machine.wait_until_succeeds("test -e /dev/disk/by-partlabel/syspersist", timeout=30)
        machine.succeed("mkdir -p /mnt && mount /dev/disk/by-partlabel/syspersist /mnt")
        machine.succeed("mkdir -p /mnt/mura/identity/ssh && ssh-keygen -q -t ed25519 -N \"\" -f /mnt/mura/identity/ssh/ssh_host_ed25519_key && echo hello > /mnt/mura/marker")
        own = machine.succeed("ssh-keygen -lf /mnt/mura/identity/ssh/ssh_host_ed25519_key.pub | cut -d\" \" -f2").strip()
        machine.succeed("umount /mnt")
        machine.succeed("systemctl restart mura-recovery-identity.service mura-recovery-screen.service")
        assert machine.succeed("cat /run/mura-recovery/fingerprint").strip() == own
        assert machine.succeed("cat /run/mura-recovery/keysource").strip() == "own"

    with subtest("recovery: factory reset asks, then wipes and re-creates exactly the FactoryReset=yes partition"):
        # a wrong answer erases nothing
        machine.fail("printf \"no\\n\" | mura-recovery factory-reset")
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt && test -f /mnt/mura/marker && umount /mnt")
        # the right answer: systemd-repart --factory-reset (the reboot is skipped for the test)
        machine.succeed("printf \"yes, erase\\n\" | MURA_RECOVERY_NO_REBOOT=1 mura-recovery factory-reset")
        machine.wait_until_succeeds("test -e /dev/disk/by-partlabel/syspersist", timeout=30)
        machine.succeed("mount /dev/disk/by-partlabel/syspersist /mnt")
        machine.fail("test -e /mnt/mura")                      # empty, re-created
        machine.succeed("umount /mnt")
        # the disk still has exactly one partition (nothing else was touched)
        assert machine.succeed("ls /dev/vdb* | wc -l").strip() == "2", machine.succeed("ls -la /dev/vdb*")
  '';
}
