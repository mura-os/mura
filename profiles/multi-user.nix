# profiles/multi-user.nix — the shared-device (greeter) profile.
#
# Declares the login profile only. It declares NO account: the importer must declare at
# least one human account (`users.users.<name>.isNormalUser = true`, typically with
# `hashedPasswordFile`), and the device contract refuses to evaluate otherwise
# (`mura.xr.session.allowNoDeclaredAccount` is the escape hatch; ADR 0017 rev 2 decision 4).
# There is no runtime "create the first account" screen — the image is the installation.
{ ... }:
{
  # Standard Linux multi-user (docs/architecture/multi-user.md): the account database is
  # mutable through standard tools; D1 persists it across A/B slots via userborn.
  users.mutableUsers = true;

  mura.xr.session = {
    greeter = "zxr-greeter";
    multiUser.enable = true;
  };
}
