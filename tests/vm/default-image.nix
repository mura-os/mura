# The default-image fixture: profiles/default.nix — user `mura`, no password, autologin.
# Subtests accumulate per D-track rung (implementation-path §3c); D0's exit criteria first.
# (Nix indented string: never write two consecutive single quotes inside the script.)
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
        for d, mode in [("factory", "750"), ("identity", "700"), ("enrollment", "755"), ("pairing", "700"), ("state", "755"), ("state/provisioning", "750"), ("state/faillock", "750"), ("state/credential-hint", "1777")]:
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

    with subtest("D2: a passwordless mura cannot administer; passwd is the gate"):
        machine.fail("su - mura -c 'sudo -n true'")
        machine.fail("su - mura -c \"printf '\\n' | sudo -S true\"")
        # own password, no old-password prompt, through passwd itself (the welcome item's path)
        machine.succeed("su - mura -c \"printf 's3cret\\ns3cret\\n' | passwd\"")
        assert machine.succeed("getent shadow mura").split(":")[1].startswith("$"), "passwd did not set a hash"
        machine.succeed("su - mura -c \"echo s3cret | sudo -S true\"")

    with subtest("D2: sshd is key-only everywhere except the USB-gadget subnet"):
        probe = "sshd -T -C user=mura,host=h,addr={addr},laddr=172.16.42.1,lport=22"
        # `sshd -T -C` evaluates Match blocks for a hypothetical connection; keyword case varies
        off = machine.succeed(probe.format(addr="10.0.2.2")).lower()
        on = machine.succeed(probe.format(addr="172.16.42.7")).lower()
        for key in ("passwordauthentication", "kbdinteractiveauthentication"):
            assert f"{key} no" in off, f"{key} not refused off-subnet:\n{off}"
            assert f"{key} yes" in on, f"{key} not allowed on the gadget subnet:\n{on}"
        # never PermitEmptyPasswords: its `none` probe poisons sshd's PAM handle (policy.nix)
        assert "permitemptypasswords no" in on, on
        assert "permitemptypasswords no" in off, off

    with subtest("D2: faillock really locks after the configured failures, counters on /persist"):
        machine.succeed("ip addr add 172.16.42.1/24 dev lo")
        # `timeout`: sshpass waits forever for a prompt that never comes; make a hang a failure
        ssh = "timeout 30 sshpass -p {pw} ssh -o StrictHostKeyChecking=no -o PubkeyAuthentication=no -o ConnectTimeout=5 mura@172.16.42.1 true"
        machine.succeed(ssh.format(pw="s3cret"))           # sanity: password auth works on-subnet
        machine.succeed("test -d /persist/mura/state/faillock")
        for _ in range(5):
            machine.fail(ssh.format(pw="wrong"))
        # tally on /persist (the compiled default is /run/faillock — nixpkgs' PAM does not read
        # /etc/security/faillock.conf without conf=), and the contract's deny count applies
        machine.succeed("test -e /persist/mura/state/faillock/mura")
        machine.fail("test -e /run/faillock/mura")
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura | grep -q 172.16.42.1")
        machine.fail(ssh.format(pw="s3cret"))              # locked: the right password is refused
        machine.succeed("journalctl -b --no-pager | grep -q 'pam_faillock(sshd:auth): Consecutive login failures'")
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")
        machine.succeed(ssh.format(pw="s3cret"))

    with subtest("D2: logind leaves the power key to the compositor; no polkit rule on the default image"):
        out = machine.succeed("busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager HandlePowerKey")
        assert '"ignore"' in out, out
        # -R: the rules file is a symlink into the store; -r would not follow it (vacuous pass)
        machine.succeed("grep -Rq 'polkit.addRule' /etc/polkit-1/rules.d/")
        machine.fail("grep -Rq 'NetworkManager.settings.modify.system' /etc/polkit-1/rules.d/")

    with subtest("D2: the credential-hint directory has the /tmp shape and a user can write their own file"):
        machine.succeed("su - mura -c 'echo numeric > /var/lib/mura/state/credential-hint/mura'")
        assert machine.succeed("stat -c %U /var/lib/mura/state/credential-hint/mura").strip() == "mura"
        # -f: without it rm prompts on the write-protected file and waits on stdin forever
        machine.fail("su - nobody -s /bin/sh -c 'rm -f /var/lib/mura/state/credential-hint/mura'")
        machine.succeed("test -e /var/lib/mura/state/credential-hint/mura")

    with subtest("D1: passwd persists across a reboot; F1 does not re-run; machine-id stable"):
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
