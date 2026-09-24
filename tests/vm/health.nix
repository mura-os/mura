# The crash-loop ladder (D6, modules/os/health.nix, implementation-path §3a-bis) on the
# default-image fixture with a FORCED hard preflight failure: a declared factory-calibration
# file that does not exist. No greeter or session may start; the durable counter climbs one per
# boot; at the threshold the boot enters mura-recovery.target with sshd still reachable; a boot
# that passes again is blessed and resets the counter.
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-health";
  profileModules = [ ../../profiles/default.nix ];
  extraModules = [
    ({ lib, ... }: {
      # The forced failure: P2 is a hard check. Removed at runtime for the recovery subtest by
      # creating the file (the path is on /persist so it survives the reboots).
      mura.xr.calibration.paths.factory = "/persist/mura/factory/calibration.json";
      mura.health.crashLoopThreshold = lib.mkForce 2;
    })
  ];

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("D6: a hard preflight failure keeps the greeter and session down and counts once"):
        machine.wait_until_succeeds("systemctl show -p Result --value mura-preflight.service | grep -qx exit-code", timeout=120)
        machine.succeed("grep -q 'FAIL P2 factory calibration' <(journalctl -b --no-pager -u mura-preflight)")
        machine.sleep(5)
        machine.fail("systemctl is-active greetd.service")
        machine.fail("pgrep -x sway")
        assert machine.succeed("cat /var/lib/mura/state/health/crashloop").strip() == "1"
        machine.fail("systemctl is-active mura-recovery.target")   # below the threshold
        machine.fail("systemctl is-active boot-complete.target")   # never blessed
        machine.succeed("test -e /run/mura/preflight.json")

    with subtest("D6: at the threshold the boot enters mura-recovery.target; sshd stays reachable"):
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.wait_until_succeeds("test \"$(cat /var/lib/mura/state/health/crashloop)\" = 2", timeout=120)
        machine.wait_for_unit("mura-recovery.target", timeout=60)
        machine.fail("systemctl is-active greetd.service")
        machine.wait_for_unit("sshd.service")
        machine.succeed("timeout 30 ssh -o StrictHostKeyChecking=no -o BatchMode=yes -i /etc/mura-test/fixture-ssh-key mura@127.0.0.1 true")

    with subtest("D6: a passing boot is blessed and resets the counter"):
        machine.succeed("echo '{}' > /persist/mura/factory/calibration.json")
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("multi-user.target")
        machine.wait_for_unit("greetd.service")
        machine.wait_until_succeeds("pgrep -u mura -x sway", timeout=120)
        machine.wait_for_unit("mura-readiness.service", timeout=360)
        machine.wait_for_unit("boot-complete.target", timeout=60)
        assert machine.succeed("cat /var/lib/mura/state/health/crashloop").strip() == "0"
        machine.fail("systemctl is-active mura-recovery.target")
  '';
}
