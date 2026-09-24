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
