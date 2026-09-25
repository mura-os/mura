# The multi-user fixture: profiles/multi-user.nix + the declared fixture account, the
# stand-in greeter (cage + gtkgreet until G2). Subtests accumulate per D-track rung.
#
# Note: nixpkgs wraps GTK programs, so gtkgreet's process name is `.gtkgreet-wrapped`;
# match on the command line (`pgrep -f`), never `pgrep -x gtkgreet`.
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-multi-user";
  profileModules = [ ../../profiles/multi-user.nix ./fixture-user.nix ];
  extraModules = [
    # TEST-ONLY session units for the readiness-bound subtest (D4): a compositor that never
    # signals readiness, behind its own target so the real mura-session.target is untouched.
    # The wrapper is pointed at it with its test-only `--target` flag. Same shape as the real
    # units (modules/os/session.nix): Type=notify, TimeoutStartSec from the contract, the target
    # bound to the service so a timed-out service ends the target and `start --wait` returns.
    ({ config, pkgs, ... }: {
      systemd.user.services.mura-compositor-stub = {
        description = "TEST-ONLY never-ready compositor stand-in";
        bindsTo = [ "mura-session-stub.target" ];
        before = [ "mura-session-stub.target" ];
        serviceConfig = {
          Type = "notify";
          NotifyAccess = "all";
          ExecStart = "${pkgs.coreutils}/bin/sleep 600";
          TimeoutStartSec = "${toString config.mura.xr.session.readinessTimeoutSeconds}s";
          Slice = "session.slice";
        };
      };
      systemd.user.targets.mura-session-stub = {
        description = "TEST-ONLY session target around the never-ready stub";
        requires = [ "mura-compositor-stub.service" ];
        bindsTo = [ "mura-compositor-stub.service" ];
        unitConfig.StopWhenUnneeded = true;
      };
    })
  ];

  testScript = ''
    GREETER = "pgrep -u greeter -f bin/gtkgreet"

    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("D0: greetd runs the stand-in greeter directly as the greeter user"):
        machine.wait_for_unit("greetd.service")
        machine.wait_until_succeeds(GREETER, timeout=120)
        machine.fail("pgrep -x sway")
        machine.screenshot("multi-user-greeter")

    with subtest("D0: an undeclared username is refused uniformly and the greeter stays up"):
        machine.send_chars("nobody-here\n")
        machine.sleep(2)
        machine.send_chars("wrong\n")
        machine.sleep(4)
        machine.succeed(GREETER)
        machine.fail("pgrep -x sway")
        machine.screenshot("multi-user-refused")

    with subtest("D0: greetd respawns the greeter when it exits"):
        old = machine.succeed(GREETER).strip()
        machine.succeed("pkill -u greeter -f bin/gtkgreet")
        machine.wait_until_succeeds(f"{GREETER} | grep -vqx '{old}'", timeout=60)
        machine.sleep(2)  # a fresh prompt, independent of gtkgreet's post-error state

    with subtest("D0: the declared account logs in through the greeter into the stand-in session"):
        machine.send_chars("mura\n")
        machine.sleep(2)
        machine.send_chars("mura\n")
        machine.wait_until_succeeds("pgrep -u mura -x sway", timeout=120)
        machine.wait_until_fails(GREETER)
        machine.succeed("loginctl list-sessions --no-legend | grep -w mura")
        machine.screenshot("multi-user-session")

    with subtest("D2: the greeter profile ships the one Mura polkit rule (greeter may add system Wi-Fi profiles)"):
        # -R: the rules file is a symlink into the store; -r would not follow it
        machine.succeed("grep -Rq 'NetworkManager.settings.modify.system' /etc/polkit-1/rules.d/")
        machine.succeed("grep -Rq 'subject.user == \"greeter\"' /etc/polkit-1/rules.d/")
        # NetworkManager itself arrives at D3; the rule's effect is exercised there

    with subtest("D2: sshd is on with upstream defaults on the greeter profile too"):
        machine.wait_for_unit("sshd.service")
        assert "passwordauthentication yes" in machine.succeed("sshd -T").lower()
        # the declared fixture account (a real password) logs in over SSH like on any Linux host
        machine.succeed("timeout 30 sshpass -p mura ssh -o StrictHostKeyChecking=no -o PubkeyAuthentication=no -o ConnectTimeout=5 mura@127.0.0.1 true")

    userctl = "systemctl --user -M mura@ "

    with subtest("D4: the session came up through the wrapper (mura-session) with the static environment of the greeter profile"):
        machine.wait_until_succeeds(userctl + "is-active mura-compositor.service graphical-session.target mura-session.target", timeout=60)
        env = machine.succeed(userctl + "show-environment")
        assert "MURA_PROFILE=multi-user" in env and "WAYLAND_DISPLAY=" in env, env
        # the session vars reach the compositor unit through the wrapper's session.env (F1)
        machine.succeed("grep -q '^XDG_SESSION_ID=' /run/user/1000/mura/session.env")
        machine.fail("pgrep -u mura -f 'uwsm|python'")

    with subtest("D-sweep: the wheel time-zone rule holds on the greeter profile too, from inside the user manager (research/56 §9)"):
        machine.succeed("systemd-run --user -M mura@ --wait --pipe --quiet timedatectl set-timezone Europe/Berlin")
        assert machine.succeed("timedatectl show -p Timezone --value").strip() == "Europe/Berlin"
        machine.succeed("timedatectl set-timezone UTC")

    with subtest("D4: logout tears the session down through the wrapper and returns to the greeter, without racing device release"):
        sid = machine.succeed("loginctl list-sessions --no-legend | awk '$3==\"mura\"{print $1}'").strip()
        machine.succeed("journalctl --rotate && journalctl --vacuum-time=1s >/dev/null 2>&1 || true")
        # a logout is: the session target stops -> the wrapper returns -> greetd restarts the greeter
        machine.succeed(userctl + "stop mura-session.target")
        machine.wait_until_fails("test -e /run/user/1000/mura/session.env", timeout=60)  # the wrapper cleaned up
        machine.wait_until_fails("pgrep -u mura -x sway", timeout=60)
        machine.wait_until_succeeds(GREETER, timeout=120)
        machine.wait_until_fails(f"loginctl show-session {sid} >/dev/null 2>&1", timeout=60)
        # the greeter's compositor took the DRM device cleanly
        machine.fail("journalctl -b --no-pager | grep -qi 'EBUSY\\|Failed to become DRM master\\|drm master'")
        machine.screenshot("multi-user-after-logout")

    with subtest("D4: a compositor that never signals readiness is torn down at the bound and the wrapper returns"):
        # a login session for mura with no compositor (su starts a logind session; the
        # user manager is idle after the logout above), running the wrapper on a stub
        # A real logind session for mura without the greeter: SSH (pam_systemd gives it a
        # session, XDG_RUNTIME_DIR and a user manager; `su -` does none of that here). An SSH
        # session has no seat/VT; the bound under test is the unit machinery's, not seat
        # semantics, so the stub is handed a seat and VT explicitly. The stub units are
        # TEST-ONLY (below, extraModules): a Type=notify sleep that never notifies, behind a
        # target the wrapper is pointed at with its test-only --target flag.
        import time
        stub = "XDG_SEAT=seat0 XDG_VTNR=1 mura-session start --target mura-session-stub.target"
        t0 = time.monotonic()
        rc, out = machine.execute(f"timeout 120 sshpass -p mura ssh -o StrictHostKeyChecking=no -o PubkeyAuthentication=no mura@127.0.0.1 '{stub}' 2>&1; echo rc=$?")
        took = time.monotonic() - t0
        assert "rc=124" not in out, f"the wrapper hung past the bound:\n{out}"
        # torn down BECAUSE of the readiness bound (30 s, the contract default): the unit result
        # and the journal carry the failure — the wrapper's exit status does not (it waits on
        # the session *target*, and targets do not fail; spec §4 step 7)
        jr = machine.execute("journalctl -b --no-pager _UID=1000 | grep -i 'mura-session\\|stub\\|shutdown\\|error\\|fatal' | tail -40")[1]
        assert 20 < took < 90, f"wrapper returned after {took:.0f}s, not at the readiness bound:\n{out}\n--- journal ---\n{jr}"
        # (the unit is CollectMode=inactive-or-failed and already collected; the journal is the record)
        machine.succeed("journalctl -b --no-pager _UID=1000 | grep -q 'mura-compositor-stub.service: start operation timed out'")
        machine.succeed("journalctl -b --no-pager _UID=1000 | grep -q \"mura-compositor-stub.service: Failed with result 'timeout'\"")
        machine.fail("pgrep -u mura -x sleep")
        machine.fail(userctl + "is-active mura-compositor-stub.service mura-session-stub.target")

    with subtest("D5: the lock authenticates the declared fixture account through mura-lock"):
        h = "su - mura -c 'mura-authd-harness --authd /run/current-system/sw/bin/mura-authd --user mura --scenario basic --password {pw}{extra}'"
        machine.succeed(h.format(pw="mura", extra=""))
        machine.succeed(h.format(pw="wrong", extra=" --expect-fail"))

    with subtest("D6: on the greeter profile the blessing tier is a stable greeter, never a login"):
        machine.succeed("systemctl show -p Result --value mura-preflight.service | grep -qx success")
        machine.wait_for_unit("mura-readiness.service", timeout=360)
        machine.wait_for_unit("boot-complete.target", timeout=60)
        assert machine.succeed("cat /var/lib/mura/state/health/crashloop").strip() == "0"

    with subtest("D1: a runtime-created account survives a reboot (userborn hybrid mode on the persisted /etc overlay)"):
        machine.succeed("findmnt -no FSTYPE /etc | grep -qx overlay")
        machine.succeed("useradd -m -G wheel guestadmin && echo 'guestadmin:pw' | chpasswd")
        machine.succeed("grep -q '^guestadmin:' /persist/etc-rw/upper/passwd")
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.succeed("getent passwd guestadmin")          # runtime row preserved (hybrid mode)
        machine.succeed("getent passwd mura")                # declared row re-materialised
        machine.wait_until_succeeds(GREETER, timeout=120)
  '';
}
