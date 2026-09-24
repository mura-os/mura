# tests/vm/lib.nix — the D-track's VM test harness (implementation-path §3c).
#
# Builds a NixOS VM test (pkgs.testers.runNixOSTest) whose single machine is the
# virtual-headset device composed with a profile — the same composition flake.nix uses
# for the interactive fixtures — plus test-only overrides that make the stand-ins run
# headless. Run on demand: `nix build .#vm-test-<name>` (deliberately NOT part of
# `nix flake check`; each test boots a VM and takes minutes).
#
# Test-only overrides (never in a fixture or a shipped image):
#   - `-vga none -device virtio-gpu-pci`: cage/wlroots need a real DRM device in the test
#     VM (upstream nixos/tests/cage.nix does the same).
#   - WLR_RENDERER=pixman: no GL inside the test VM (upstream nixos/tests/sway.nix).
{ pkgs }:
{ name
, profileModules # e.g. [ ../../profiles/default.nix ]
, extraModules ? [ ]
, testScript
}:
pkgs.testers.runNixOSTest {
  inherit name testScript;
  meta.maintainers = [ ];

  nodes.machine = { lib, ... }: {
    imports = (import ../../modules) ++ [
      ../../devices/virtual-headset
    ] ++ profileModules ++ extraModules;

    # Test-only (see header).
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    environment.sessionVariables.WLR_RENDERER = "pixman";
    fonts.packages = [ pkgs.dejavu_fonts ];

    # The device sets these for a real disk image; the test framework owns the VM's disk
    # and the node name (both sides use mkDefault, so the test must decide).
    boot.loader.systemd-boot.enable = lib.mkForce false;
    boot.loader.efi.canTouchEfiVariables = lib.mkForce false;
    networking.hostName = lib.mkForce "machine";
  };
}
