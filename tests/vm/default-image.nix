# The default-image fixture: profiles/default.nix — user `mura`, no password, autologin.
# Subtests accumulate per D-track rung (implementation-path §3c); D0's exit criteria first.
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-default-image";
  profileModules = [ ../../profiles/default.nix ];

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("D0: greetd autologins the passwordless mura straight into the stand-in session"):
        machine.wait_for_unit("greetd.service")
        machine.wait_until_succeeds("pgrep -u mura -x sway", timeout=120)
        machine.succeed("loginctl list-sessions --no-legend | grep -w mura")
        machine.screenshot("default-image-session")

    with subtest("D0: no getty autologin, no permanent device-group membership"):
        machine.fail("pgrep -f 'agetty.*--autologin'")
        groups = machine.succeed("id -nG mura").split()
        assert "video" not in groups and "input" not in groups, f"mura has device groups: {groups}"
        assert "wheel" in groups, f"mura is not in wheel: {groups}"

    with subtest("D0: the declared account has an empty password, not a locked one"):
        shadow = machine.succeed("getent shadow mura").strip()
        assert shadow.split(":")[1] == "", f"unexpected shadow field: {shadow}"

    with subtest("D1: /persist is a stage-1 mount and the class skeleton exists"):
        machine.succeed("findmnt -no SOURCE /persist | grep -q vdb")
        for d, mode in [("factory", "750"), ("identity", "700"), ("enrollment", "700"), ("pairing", "700"), ("state", "750"), ("state/provisioning", "750")]:
            got = machine.succeed(f"stat -c %a /persist/mura/{d}").strip()
            assert got == mode, f"/persist/mura/{d} is {got}, want {mode}"
        # a bind mount's SOURCE is the backing device plus the subtree: /dev/vdb[/mura]
        machine.succeed("findmnt -no SOURCE /var/lib/mura | grep -q '\\[/mura\\]'")
        machine.fail("findmnt /var/lib/bluetooth")  # bluetooth = false on the VM: no pairing bind
        # nothing failed on the way up (ordering cycles show up here as deleted jobs)
        failed = machine.succeed("systemctl --failed --no-legend --plain").strip()
        assert failed == "", f"failed units: {failed}"
        machine.fail("journalctl -b --no-pager | grep -q 'ordering cycle'")

    with subtest("D1: /etc is a mutable overlay whose upper layer lives on /persist"):
        machine.succeed("findmnt -no FSTYPE /etc | grep -qx overlay")
        # mounted in the initrd as /sysroot/.rw-etc/upper; the kernel keeps reporting that path
        machine.succeed("findmnt -no OPTIONS /etc | grep -q 'upperdir=[^,]*\\.rw-etc/upper'")
        machine.succeed("findmnt -no SOURCE /.rw-etc | grep -q '\\[/etc-rw\\]'")
        # the account database: real files inside the overlay (userborn, hybrid mode)
        machine.succeed("test -f /etc/shadow && ! test -L /etc/shadow")
        assert machine.succeed("stat -c %a /etc/shadow").strip() == "0"

    with subtest("D1: machine-id is committed into the persisted layer and the F1 pattern unit ran"):
        mid1 = machine.succeed("cat /etc/machine-id").strip()
        assert len(mid1) == 32, f"machine-id not committed: {mid1!r}"
        assert machine.succeed("cat /persist/etc-rw/upper/machine-id").strip() == mid1
        machine.wait_for_unit("mura-f1-seed-state.service")
        machine.succeed("test -s /persist/mura/state/provisioning/seed-state")
        machine.succeed("test -s /var/lib/mura/identity/ssh/ssh_host_ed25519_key")

    with subtest("D1: passwd persists across a reboot; F1 does not re-run; machine-id stable"):
        machine.succeed("echo 'mura:s3cret' | chpasswd")
        hashed = machine.succeed("getent shadow mura").split(":")[1]
        assert hashed.startswith("$"), f"password not set: {hashed!r}"
        # the change landed in the persisted upper layer, not slot-local
        machine.succeed(f"grep -q '^mura:{hashed}' /persist/etc-rw/upper/shadow")
        first_marker = machine.succeed("cat /persist/mura/state/provisioning/seed-state").strip()
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        assert machine.succeed("getent shadow mura").split(":")[1] == hashed, "password lost across reboot"
        assert machine.succeed("cat /etc/machine-id").strip() == mid1, "machine-id changed across reboot"
        assert machine.succeed("cat /persist/mura/state/provisioning/seed-state").strip() == first_marker, "F1 re-ran"
        machine.succeed("systemctl show -p Result mura-f1-seed-state.service | grep -q 'Result=success'")
        # the unit's condition must have failed on this boot (skipped, not re-executed)
        machine.succeed("systemctl show -p ConditionResult mura-f1-seed-state.service | grep -q 'ConditionResult=no'")
        machine.wait_until_succeeds("pgrep -u mura -x sway", timeout=120)
  '';
}
