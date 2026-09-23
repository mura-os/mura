# muraSystem: evaluate a device into a NixOS system.
#
# The single integration path (ADR 0005): mirrors robotnix's lib.robotnixSystem. A
# device directory is a NixOS module; this wraps it with nixpkgs' eval-config plus the
# Mura module list.
{ nixpkgs }:
{ device            # path to devices/<codename> (or a module)
, system ? "x86_64-linux"
, extraModules ? [ ]
, specialArgs ? { }
}:
nixpkgs.lib.nixosSystem {
  inherit system specialArgs;
  modules =
    (import ../modules)      # the Mura module list
    ++ [ device ]
    ++ extraModules;
}
