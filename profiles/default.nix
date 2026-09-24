# profiles/default.nix — the default image.
#
# The image is the installation (docs/architecture/first-run-onboarding.md §1, ADR 0017
# rev 2): everything a desktop installer would have collected is declared here. A device
# or image imports this explicitly (docs/architecture/repo-structure.md §profiles/); a
# self-builder who declares their own users imports nothing from profiles/.
#
# Posture (first-run §5.3, all static): `mura` has no password and is a full *user*;
# administration (sudo, polkit auth_admin) waits until the wearer sets a password with
# `passwd`, which asks no old password for a passwordless account. Nothing here relaxes
# sudo or polkit.
{ ... }:
{
  # A password set with `passwd` must persist across reboots — the shadow database is
  # mutable state, not configuration. (D1 moves it onto /persist/userdb via userborn so it
  # also survives A/B slot switches, the same wiring the multi-user profile uses.)
  users.mutableUsers = true;

  users.users.mura = {
    isNormalUser = true;
    description = "Mura";
    extraGroups = [ "wheel" ];
    # Empty hash = login without a password, until the wearer chooses one. Never `password`
    # or `hashedPassword` here: those would re-impose a value on every activation.
    initialHashedPassword = "";
  };

  # Appliance profile: greetd's initial_session autologins `mura` straight into the session
  # (ADR 0007 §Two profiles). The contract asserts the autologin user is declared above.
  mura.xr.session.autoLogin = "mura";

  # Locale and time zone defaults are modules/os's (ownership table); the welcome surface
  # offers both as items. Nothing else is declared here on purpose.
}
