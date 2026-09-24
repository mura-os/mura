# profiles/dev.nix — developer conveniences. NEVER in a shipped image.
#
# Imported explicitly by devices/virtual-headset and by a developer who wants it; nothing in
# modules/ or profiles/default.nix pulls it in. The shipped posture is not loosened here:
# sshd is already on with upstream defaults on every profile (modules/os/default.nix).
#
# SSH from first boot on an image you built yourself — the self-builder path
# (first-run-onboarding.md §5.3): the default `mura` account has no password, and OpenSSH
# refuses empty passwords, so declare your key with the standard NixOS option in your own
# device file or a flake overlay:
#
#   users.users.mura.openssh.authorizedKeys.keys = [ "ssh-ed25519 AAAA... you@laptop" ];
#
# No Mura-specific option exists for this on purpose. The VM tests do exactly this with a
# checked-in fixture key (tests/vm/fixture-ssh-key.pub).
{ ... }:
{
  # The serial console (`console=` kernel parameter, serial-getty unit) is the device's:
  # the UART name differs per SoC (ttyS0 on the x86 VM, ttyAMA0 on the Frame).
}
