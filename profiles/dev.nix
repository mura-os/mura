# profiles/dev.nix — developer conveniences. NEVER in a shipped image.
#
# Everything here deliberately loosens the shipped posture of first-run-onboarding.md
# §5.3 for the rung-2 VM and a developer's own headset: SSH with password auth on every
# interface, a serial console. Imported explicitly by devices/virtual-headset and by a
# developer who wants it; nothing in modules/ or profiles/default.nix pulls it in.
{ lib, ... }:
{
  services.openssh = {
    enable = true;
    # DEV ONLY — the shipped posture (modules/os/policy.nix) is key-only except the
    # USB-gadget subnet; the VM's forwarded SSH arrives from user-mode networking, so the
    # dev profile forces password auth on every interface. Both knobs: with UsePAM,
    # keyboard-interactive is password auth too. Priority 60 (below mkForce): the VM tests
    # mkForce the shipped posture back on to verify it.
    settings = {
      PasswordAuthentication = lib.mkOverride 60 true;
      KbdInteractiveAuthentication = lib.mkOverride 60 true;
    };
  };

  # The serial console (`console=` kernel parameter, serial-getty unit) is the device's:
  # the UART name differs per SoC (ttyS0 on the x86 VM, ttyAMA0 on the Frame).
}
