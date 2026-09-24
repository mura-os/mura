# The multi-user fixture: profiles/multi-user.nix + the declared fixture account, the
# stand-in greeter (cage + gtkgreet until G2). Subtests accumulate per D-track rung.
#
# Note: nixpkgs wraps GTK programs, so gtkgreet's process name is `.gtkgreet-wrapped`;
# match on the command line (`pgrep -f`), never `pgrep -x gtkgreet`.
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-multi-user";
  profileModules = [ ../../profiles/multi-user.nix ./fixture-user.nix ];

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

    with subtest("D0: the declared account logs in through the greeter into the stand-in session"):
        # gtkgreet re-asks the username after a failure; type the fixture account.
        machine.send_chars("mura\n")
        machine.sleep(2)
        machine.send_chars("mura\n")
        machine.wait_until_succeeds("pgrep -u mura -x sway", timeout=120)
        machine.wait_until_fails(GREETER)
        machine.succeed("loginctl list-sessions --no-legend | grep -w mura")
        machine.screenshot("multi-user-session")
  '';
}
