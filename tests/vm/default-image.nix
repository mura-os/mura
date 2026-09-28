# The default-image fixture: profiles/default.nix — user `mura`, no password, autologin.
# Subtests accumulate per D-track rung (implementation-path §3c); D0's exit criteria first.
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-default-image";
  profileModules = [ ../../profiles/default.nix ];

  testScript = ''
    import re

    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("D0: greetd autologins the passwordless mura straight into the zxr session"):
        machine.wait_for_unit("greetd.service")
        machine.wait_until_succeeds("pgrep -u mura -x zxr", timeout=120)
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

    # `systemctl --user` for another user, from root: -M user@ enters their manager.
    userctl = "systemctl --user -M mura@ "

    with subtest("D4: the compositor runs as a static user unit under mura-session and acquired the seat from there"):
        machine.wait_until_succeeds(userctl + "is-active mura-compositor.service", timeout=60)
        machine.wait_until_succeeds(userctl + "is-active graphical-session.target", timeout=60)
        machine.succeed(userctl + "is-active mura-session.target")
        machine.succeed(userctl + "is-active monado.socket")      # the session's own Monado, socket-activated
        # no ordering cycle in the user manager either (target Wants= imply After=)
        machine.fail("journalctl -b --no-pager _UID=1000 | grep -q 'ordering cycle'")
        # zxr is inside the unit, not a child of greetd
        cg = machine.succeed("cat /proc/$(pgrep -u mura -x zxr | head -1)/cgroup").strip()
        assert "mura-compositor.service" in cg, f"zxr cgroup: {cg}"
        # the F1 regression: seat acquisition through libseat's XDG_SESSION_ID fallback — the
        # session vars reach the compositor UNIT via the wrapper's session.env (0600), never the
        # user manager itself (which outlives sessions)
        machine.succeed("grep -q '^XDG_SESSION_ID=' /run/user/1000/mura/session.env")
        assert machine.succeed("stat -c %a /run/user/1000/mura/session.env").strip() == "600"
        machine.succeed("tr '\\0' '\\n' < /proc/$(pgrep -u mura -x zxr | head -1)/environ | grep '^XDG_SESSION_ID=' >/dev/null")
        machine.fail(userctl + "show-environment | grep -q '^XDG_SESSION_ID='")
        machine.fail("journalctl -b --no-pager _UID=1000 | grep -qi 'libseat.*\\(fail\\|error\\|could not\\)'")

    with subtest("D4: the wrapper is the greetd session's leader and stays alive"):
        # greetd's session worker is the logind leader and forks the session command: the
        # wrapper is its child, in the same session scope. The wrapper binds the graphical session
        # to its own PID (mura-session-bindpid@<pid>): that PID must be in a session
        # scope, descend from that session's leader, and be alive. (The session is derived
        # from the wrapper's cgroup — other mura sessions may exist, e.g. the readiness gate's.)
        bind = machine.succeed(userctl + "list-units --no-legend --plain 'mura-session-bindpid@*' | awk '{print $1}'").strip()
        wpid = bind.split("@", 1)[1].split(".", 1)[0]
        cg = machine.succeed(f"cat /proc/{wpid}/cgroup")
        m = re.search(r"session-(\d+)\.scope", cg)
        assert m, f"wrapper {wpid} is not in a logind session scope: {cg}"
        sid = m.group(1)
        leader = machine.succeed(f"loginctl show-session {sid} -p Leader --value").strip()
        cmd = machine.succeed(f"tr '\\0' ' ' < /proc/{wpid}/cmdline").strip()
        assert "mura-session start" in cmd, f"bound pid is not the wrapper: {cmd}"
        p = wpid
        while p not in ("0", "1", leader):
            p = machine.succeed(f"awk '{{print $4}}' /proc/{p}/stat").strip()
        assert p == leader, f"wrapper {wpid} does not descend from the session leader {leader}"

    with subtest("D4: environment classes — static from environment.d, compositor-created after readiness"):
        env = machine.succeed(userctl + "show-environment")
        for var in ("MURA_PROFILE=appliance", "XDG_SESSION_TYPE=wayland", "XDG_CURRENT_DESKTOP=mura", "WAYLAND_DISPLAY=", "SWAYSOCK="):
            assert var in env, f"{var} missing from the user manager environment:\n{env}"
        # WAYLAND_DISPLAY names a socket that is actually bound (no pre-readiness leak)
        wd = [l for l in env.splitlines() if l.startswith("WAYLAND_DISPLAY=")][0].split("=", 1)[1]
        machine.succeed(f"test -S /run/user/1000/{wd}")
        # nothing from greetd's own environment leaked into the user manager
        assert "GREETD_SOCK" not in env, env
        # the readiness bound is the contract value
        assert machine.succeed(userctl + "show -p TimeoutStartUSec --value mura-compositor.service").strip() == "30s"

    with subtest("D4: no interpreter on the session-start path (the ruling behind the uwsm port)"):
        # nothing of uwsm's remains, and no Python process belongs to the login: the wrapper is
        # one static binary, the units are static files (no login-time generation, no daemon-reload)
        machine.fail("pgrep -u mura -f uwsm")
        machine.fail("pgrep -u mura -f python")
        machine.fail("test -e /run/user/1000/uwsm")
        machine.fail(userctl + "list-units --all --no-legend --plain 'wayland-wm@*' | grep -q wayland")
        machine.fail("journalctl -b --no-pager _UID=1000 | grep -q 'daemon-reload'")
        # the wrapper's own cost, journal-timed: greetd session start -> compositor unit active
        t_login = machine.succeed("journalctl -b --no-pager -o short-monotonic -u greetd.service | grep -m1 'session opened for user mura' | sed 's/^\\[ *\\([0-9.]*\\)\\].*/\\1/'").strip()
        t_ready = machine.succeed(userctl + "show -p ActiveEnterTimestampMonotonic --value mura-compositor.service").strip()
        took = int(t_ready) / 1e6 - float(t_login)
        print(f"login -> compositor ready: {took:.2f}s (greetd session opened at {t_login}s, compositor active at {int(t_ready)/1e6:.3f}s monotonic)")
        assert 0 < took < 15, f"login took {took:.2f}s"

    with subtest("D4: a compositor crash restarts it inside the same session"):
        sid_before = machine.succeed("loginctl list-sessions --no-legend | awk '$3==\"mura\"{print $1}'").strip()
        pid_before = machine.succeed("pgrep -u mura -x zxr | head -1").strip()
        machine.succeed("pkill -9 -u mura -x zxr")
        machine.wait_until_succeeds(f"pgrep -u mura -x zxr | grep -qv '^{pid_before}$'", timeout=60)
        machine.wait_until_succeeds(userctl + "is-active mura-compositor.service", timeout=60)
        machine.wait_until_succeeds(userctl + "is-active graphical-session.target", timeout=60)
        sid_after = machine.succeed("loginctl list-sessions --no-legend | awk '$3==\"mura\"{print $1}'").strip()
        assert sid_before == sid_after, f"login session changed across the crash: {sid_before} -> {sid_after}"

    with subtest("D1: /persist is a stage-1 mount and the class skeleton exists"):
        machine.succeed("findmnt -no SOURCE /persist | grep -q vdb")
        for d, mode in [("factory", "750"), ("identity", "700"), ("enrollment", "755"), ("pairing", "700"), ("state", "755"), ("state/provisioning", "750"), ("state/faillock", "755"), ("state/credential-hint", "1777")]:
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

    # `timeout`: sshpass waits forever for a prompt that never comes; make a hang a failure
    ssh_pw = "timeout 30 sshpass -p {pw} ssh -o StrictHostKeyChecking=no -o PubkeyAuthentication=no -o ConnectTimeout=5 mura@127.0.0.1 true"
    ssh_key = "timeout 30 ssh -o StrictHostKeyChecking=no -o BatchMode=yes -o ConnectTimeout=5 -i /etc/mura-test/fixture-ssh-key mura@127.0.0.1 true"

    with subtest("D2: sshd is on with upstream defaults; a passwordless account gets SSH only by key"):
        machine.wait_for_unit("sshd.service")
        # `sshd -T` prints the effective config; keyword case varies between versions
        eff = machine.succeed("sshd -T").lower()
        assert "passwordauthentication yes" in eff, eff       # every interface, no Match scoping
        assert "kbdinteractiveauthentication yes" in eff, eff
        assert "permitemptypasswords no" in eff, eff          # OpenSSH's default; never flipped (policy.nix)
        machine.fail("sshd -T | grep -qi '^match'")
        machine.fail(ssh_pw.format(pw=""))                    # the empty password is not a credential
        machine.succeed(ssh_key)                              # the self-builder path: a declared key, first boot

    harness = "su - mura -c 'mura-authd-harness --authd /run/current-system/sw/bin/mura-authd --user mura {args}'"

    with subtest("D5: the lock refuses a passwordless account (PAM_DISALLOW_NULL_AUTHTOK): no credential, no unlock"):
        # an empty --password is passed as a double-quoted empty string (the Nix indented string
        # would swallow two adjacent single quotes)
        machine.succeed(harness.format(args='--scenario basic --password "" --expect-fail'))
        machine.succeed("grep -q 'auth.*pam_faillock.so preauth' /etc/pam.d/mura-lock")
        machine.fail("grep -E '^auth.*pam_unix.so.*nullok' /etc/pam.d/mura-lock")  # the password stack may carry nullok; auth must not

    with subtest("D2: a passwordless mura cannot administer; passwd is the gate"):
        machine.fail("su - mura -c 'sudo -n true'")
        machine.fail("su - mura -c \"printf '\\n' | sudo -S true\"")
        # own password, no old-password prompt, through passwd itself (the welcome item's path)
        machine.succeed("su - mura -c \"printf 's3cret\\ns3cret\\n' | passwd\"")
        assert machine.succeed("getent shadow mura").split(":")[1].startswith("$"), "passwd did not set a hash"
        machine.succeed("su - mura -c \"echo s3cret | sudo -S true\"")
        machine.succeed(ssh_pw.format(pw="s3cret"))           # and SSH by password now works

    # zxr's control socket in mura's session (`zxr ctl`; spec §11)
    ZXR_CTL = "su -s /bin/sh mura -c '${pkgs.mura.zxr}/bin/zxr ctl $(ls -t /run/user/1000/zxr-*.sock | head -1) {}'"
    def user(cmd):
        # in mura's session (its manager environment: session bus, XDG dirs); absolute paths
        return machine.succeed(f"systemd-run --quiet --user -M mura@ --pipe --wait --collect {cmd} 2>&1")

    with subtest("G3: the lock/unlock cycle on the real stack — loginctl lock-session -> mura-greeter-lock locks zxr; the password typed at the VT unlocks"):
        sid = machine.succeed("loginctl list-sessions --no-legend | awk '$3==\"mura\"{print $1}'").strip()
        machine.succeed(userctl + "is-active mura-greeter-lock.service")
        listing = machine.succeed(ZXR_CTL.format("list"))
        assert "mode=Normal" in listing and 'lock="unlocked"' in listing, listing
        assert "ns=osk layer=Top frame=body" in listing, listing  # the OSK is zxr's child in the session too
        machine.succeed(f"loginctl lock-session {sid}")
        # ext-session-lock: the lock unit's surface is composed, the mode gate closes (I1), logind's hint is set (I2)
        machine.wait_until_succeeds(ZXR_CTL.format("list") + " | grep -q 'mode=Locked'", timeout=30)
        machine.wait_until_succeeds(f"loginctl show-session {sid} -p LockedHint --value | grep -qx yes", timeout=30)
        listing = machine.succeed(ZXR_CTL.format("list"))
        assert 'lock="locked"' in listing, listing
        # a wrong password: the conversation fails, the lock stays (mura-authd through PAM mura-lock)
        machine.send_chars("wrong\n")
        machine.sleep(4)
        assert "mode=Locked" in machine.succeed(ZXR_CTL.format("list"))
        machine.succeed(f"loginctl show-session {sid} -p LockedHint --value | grep -qx yes")
        # the right one: unlock_and_destroy -> Normal, the hint cleared, the same session
        machine.send_chars("s3cret\n")
        machine.wait_until_succeeds(ZXR_CTL.format("list") + " | grep -q 'mode=Normal'", timeout=30)
        machine.wait_until_succeeds(f"loginctl show-session {sid} -p LockedHint --value | grep -qx no", timeout=30)
        machine.succeed(userctl + "is-active mura-greeter-lock.service mura-compositor.service")
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")

    with subtest("G3: the idle rung — session.lock.on_idle with a short delay locks the session; the trigger is zxr's (loginctl lock-session)"):
        S = "${pkgs.mura.settingsd}/bin/mura-settings"
        user(f"{S} set session.lock.on_idle true")
        user(f"{S} set session.idle.delay_s 5")
        machine.send_chars("x")                                   # one event, so the idle clock exists
        machine.wait_until_succeeds(ZXR_CTL.format("list") + " | grep -q 'mode=Locked'", timeout=60)
        machine.succeed(f"loginctl show-session {sid} -p LockedHint --value | grep -qx yes")
        machine.send_chars("s3cret\n")
        machine.wait_until_succeeds(ZXR_CTL.format("list") + " | grep -q 'mode=Normal'", timeout=30)
        user(f"{S} reset session.lock.on_idle")
        user(f"{S} reset session.idle.delay_s")

    with subtest("D2: faillock really locks after the configured failures, counters on /persist"):
        machine.succeed("test -d /persist/mura/state/faillock")
        for _ in range(5):
            machine.fail(ssh_pw.format(pw="wrong"))
        # tally on /persist (the compiled default is /run/faillock — nixpkgs' PAM does not read
        # /etc/security/faillock.conf without conf=), and the contract's deny count applies
        machine.succeed("test -e /persist/mura/state/faillock/mura")
        machine.fail("test -e /run/faillock/mura")
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura | grep -q 127.0.0.1")
        machine.fail(ssh_pw.format(pw="s3cret"))              # locked: the right password is refused
        machine.succeed("journalctl -b --no-pager | grep -q 'pam_faillock(sshd:auth): Consecutive login failures'")
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")
        machine.succeed(ssh_pw.format(pw="s3cret"))

    with subtest("D5: mura-authd conformance (session-auth §6 items 1, 2, 3, 7) against the zxr session"):
        for scenario in ("basic", "stale-nonce", "cancel", "kill", "revoked"):
            print(machine.succeed(harness.format(args=f"--scenario {scenario} --password s3cret")))
        print(machine.succeed(harness.format(args="--scenario slow --service mura-lock-slow --password s3cret")))
        print(machine.succeed(harness.format(args="--scenario batched --service mura-lock-batched --password batched-ok")))
        machine.succeed(harness.format(args="--scenario basic --password wrong --expect-fail"))
        # legacy argv nonce still works for one release (with a warning); a foreign PAM service is refused before pam_start
        print(machine.succeed(harness.format(args="--scenario argv-nonce --password s3cret")))
        print(machine.succeed(harness.format(args="--scenario bad-service --password s3cret")))

    with subtest("D5: mura-authd rejects malformed records as failure(internal) (session-auth §2.1, §6 item 9)"):
        # each malformed batch aborts the conversation; PAM counts them as failures, so start clean
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")
        for scenario in ("oversize", "truncated", "empty", "badjson", "unknown-type", "nul-response", "dup-index"):
            print(machine.succeed(harness.format(args=f"--scenario {scenario} --password s3cret")))
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")

    with subtest("D5: the helper hides its nonce and secrets (not dumpable, nonce off argv, conversation fd CLOEXEC)"):
        # the slow stack sleeps 5 s before its first prompt, leaving a live helper to inspect
        machine.execute(harness.format(args="--scenario slow --service mura-lock-slow --password s3cret") + " >/dev/null 2>&1 &")
        pid = machine.wait_until_succeeds("pgrep -x mura-authd").split()[0]
        cmdline = machine.succeed(f"tr '\\0' ' ' < /proc/{pid}/cmdline")
        print(cmdline)
        assert "--nonce" not in cmdline, cmdline
        flags = machine.succeed(f"grep flags /proc/{pid}/fdinfo/3").split()[1]
        assert int(flags, 8) & 0o2000000, f"conversation fd lacks O_CLOEXEC: {flags}"
        # PR_SET_DUMPABLE=0: another process of the same uid may not read the helper's memory-derived
        # files (environ carries the nonce), while root still can
        status, out = machine.execute(f"su - mura -c 'cat /proc/{pid}/environ' 2>&1")
        print(status, out)
        assert status != 0 and "Permission denied" in out, out
        assert "MURA_AUTHD_NONCE=" in machine.succeed(f"tr '\\0' ' ' < /proc/{pid}/environ")
        machine.wait_until_fails("pgrep -x mura-authd")

    with subtest("D5: the lock counts towards the same faillock ladder and is refused while locked"):
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")
        for _ in range(5):
            machine.succeed(harness.format(args="--scenario basic --password wrong --expect-fail"))
        out = machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura")
        assert out.count(" V") >= 5, out                       # five valid failures in the user-owned tally
        machine.succeed(harness.format(args="--scenario basic --password s3cret --expect-fail"))  # locked: the right password is refused
        machine.succeed("faillock --dir /var/lib/mura/state/faillock --user mura --reset")
        machine.succeed(harness.format(args="--scenario basic --password s3cret"))

    with subtest("D2: logind leaves the power key to the compositor; no greeter polkit rule on the default image"):
        out = machine.succeed("busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager HandlePowerKey")
        assert '"ignore"' in out, out
        # -R: the rules file is a symlink into the store; -r would not follow it (vacuous pass)
        machine.succeed("grep -Rq 'polkit.addRule' /etc/polkit-1/rules.d/")
        # the greeter rule is the greeter profile's; mura-setup's scoped rule (D3) is on both
        machine.fail("grep -Rq 'subject.user == \"greeter\"' /etc/polkit-1/rules.d/")
        machine.succeed("grep -Rq 'subject.user == \"mura-setup\"' /etc/polkit-1/rules.d/")

    with subtest("D-sweep: a wheel member in the active local session sets the time zone without a password (research/56 §9)"):
        # The probe runs INSIDE the user manager (systemd-run --user), where the compositor and
        # everything it launches live — not in the logind session scope. polkit must still see
        # an active local session for it (Ubuntu's same rule works under GNOME's systemd-managed
        # session); this is the attribution question research/56 §9 says to verify, not assume.
        assert machine.succeed("timedatectl show -p Timezone --value").strip() == "UTC"
        machine.succeed("systemd-run --user -M mura@ --wait --pipe --quiet timedatectl set-timezone Europe/Berlin")
        assert machine.succeed("timedatectl show -p Timezone --value").strip() == "Europe/Berlin"
        machine.succeed("systemd-run --user -M mura@ --wait --pipe --quiet hostnamectl set-hostname headset-test")
        assert machine.succeed("hostnamectl --static").strip() == "headset-test"
        # not from a non-local session: `su -` opens a logind session with no seat, and polkit
        # falls back to systemd's auth_admin_keep, which nothing can answer here
        machine.fail("su - mura -c 'timedatectl set-timezone Europe/Paris'")
        assert machine.succeed("timedatectl show -p Timezone --value").strip() == "Europe/Berlin"
        # and never set-ntp or set-time: systemd's defaults stay for those
        machine.fail("systemd-run --user -M mura@ --wait --pipe --quiet timedatectl set-ntp false")
        machine.succeed("timedatectl set-timezone UTC && hostnamectl set-hostname machine")

    with subtest("D2: the credential-hint directory has the /tmp shape and a user can write their own file"):
        machine.succeed("su - mura -c 'echo numeric > /var/lib/mura/state/credential-hint/mura'")
        assert machine.succeed("stat -c %U /var/lib/mura/state/credential-hint/mura").strip() == "mura"
        # -f: without it rm prompts on the write-protected file and waits on stdin forever
        machine.fail("su - nobody -s /bin/sh -c 'rm -f /var/lib/mura/state/credential-hint/mura'")
        machine.succeed("test -e /var/lib/mura/state/credential-hint/mura")

    with subtest("D6: the preflight ran before greetd, wrote its report, and the boot was blessed"):
        machine.succeed("systemctl show -p Result --value mura-preflight.service | grep -qx success")
        pre = machine.succeed("systemctl show -p ExecMainExitTimestampMonotonic --value mura-preflight.service").strip()
        gr = machine.succeed("systemctl show -p ExecMainStartTimestampMonotonic --value greetd.service").strip()
        assert int(pre) <= int(gr), f"preflight finished at {pre} but greetd started at {gr}"
        import json
        rep = json.loads(machine.succeed("cat /run/mura/preflight.json"))
        assert rep["result"] in (0, 1), rep    # a soft failure is allowed (no select key in the VM)
        passed = {c["check"] for c in rep["checks"] if c["pass"]}
        for name in ("P1 persist", "P2 factory calibration", "P3 display path", "P4 vulkan", "P5 tracking nodes", "P6 monado probe"):
            assert name in passed, f"{name} did not pass: {rep}"
        machine.wait_for_unit("mura-readiness.service", timeout=360)
        machine.wait_for_unit("boot-complete.target", timeout=60)
        assert machine.succeed("cat /var/lib/mura/state/health/crashloop").strip() == "0"
        machine.fail("systemctl is-active mura-recovery.target")

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
        machine.wait_until_succeeds("pgrep -u mura -x zxr", timeout=120)
  '';
}
