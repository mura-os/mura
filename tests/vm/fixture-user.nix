# VM FIXTURE ONLY — a declared human account for the multi-user (greeter) shape, so the
# greeter has someone to log in (the contract refuses a greeter image without one).
# Log in as `mura` / `mura`. This `mura` is a *fixture* user with a password, not
# profiles/default.nix's passwordless one (that profile is not imported alongside).
# A shipped image declares `hashedPasswordFile` — never a hash in the store (ADR 0017 d.5).
# Shared by flake.nix (interactive VM) and tests/vm (automated).
{ ... }:
{
  users.users.mura = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    # mkpasswd -m sha-512 -S murafixture00001 mura
    hashedPassword = "$6$murafixture00001$/OEUueXZ0lBL.stcb70lwUr3UvdBSYEP1cSWJIS2jVQODmeW1J/6dyoFfV5tMpOQ.c1TTZVyRyzfeLt7l6pJf/";
  };
}
