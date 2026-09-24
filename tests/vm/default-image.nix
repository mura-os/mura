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
  '';
}
