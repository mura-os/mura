# tests/vm/lib.nix — the D-track's VM test harness (implementation-path §3c).
#
# Builds a NixOS VM test (pkgs.testers.runNixOSTest) whose single machine is the
# virtual-headset device composed with a profile — the same composition flake.nix uses
# for the interactive fixtures — plus test-only overrides that make the stand-ins run
# headless. Run on demand: `nix build .#vm-test-<name>` (deliberately NOT part of
# `nix flake check`; each test boots a VM and takes minutes).
#
# Test-only overrides (never in a fixture or a shipped image):
#   - `-vga none -device virtio-gpu-pci`: a real DRM device in the test VM (upstream
#     nixos/tests/cage.nix does the same) — the VT console, logind's seat; the XR path itself
#     is blind here (Monado's null compositor, devices/virtual-headset; research/78 §9 F14).
{ pkgs }:
{ name
, profileModules # e.g. [ ../../profiles/default.nix ]
, extraModules ? [ ]
, testScript
}:
pkgs.testers.runNixOSTest {
  inherit name testScript;
  meta.maintainers = [ ];

  nodes.machine = { lib, config, ... }: {
    imports = (import ../../modules) ++ [
      ../../devices/virtual-headset
      ../../devices/virtual-headset/vm-persist.nix # /persist on /dev/vdb, as in the interactive VM
    ] ++ profileModules ++ extraModules;

    # Test-only (see header).
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    fonts.packages = [ pkgs.dejavu_fonts ];
    environment.systemPackages = [ pkgs.sshpass ];

    # sshd runs with upstream defaults (modules/os/default.nix); nothing is overridden here.
    # Test-only: the faillock subtest hammers sshd from one address; OpenSSH's per-source
    # penalties would otherwise drop the post-reset login for reasons unrelated to PAM.
    services.openssh.settings.PerSourcePenalties = "no";

    # The self-builder path (first-run-onboarding.md §5.3): a declared key gives SSH from
    # first boot to a passwordless account. FIXTURE keypair, checked in on purpose — see
    # fixture-ssh-key.README. Standard NixOS option; no Mura option exists for this.
    users.users.mura.openssh.authorizedKeys.keys = [ (builtins.readFile ./fixture-ssh-key.pub) ];
    environment.etc."mura-test/fixture-ssh-key" = {
      source = ./fixture-ssh-key;
      mode = "0600"; # ssh refuses world-readable identity files
    };

    # TEST-ONLY PAM services for the mura-authd conformance harness (session-auth §6 items 2
    # and 7): a stack that sleeps before pam_unix, and one whose module issues a batched
    # conversation. The real `mura-lock` service is modules/os/policy.nix's.
    security.pam.services.mura-lock-slow.text = ''
      auth required ${pkgs.mura.pamTestModule}/lib/security/pam_mura_test.so sleep=5
      auth required ${config.security.pam.package}/lib/security/pam_unix.so
      account required ${config.security.pam.package}/lib/security/pam_unix.so
    '';
    security.pam.services.mura-lock-batched.text = ''
      auth required ${pkgs.mura.pamTestModule}/lib/security/pam_mura_test.so batched
      account required ${config.security.pam.package}/lib/security/pam_permit.so
    '';

    # qemu-vm.nix assumes a VM has no radio and mkVMOverride-disables wpa_supplicant; the
    # virtual headset has a mac80211_hwsim radio and NetworkManager needs the supplicant for
    # the provisioning hotspot (modules/os/oob.nix), so put it back.
    networking.wireless.enable = lib.mkOverride 5 true; # beats qemu-vm.nix's mkVMOverride (10)

    # The device sets these for a real disk image; the test framework owns the VM's disk
    # and the node name (both sides use mkDefault, so the test must decide).
    boot.loader.systemd-boot.enable = lib.mkForce false;
    boot.loader.efi.canTouchEfiVariables = lib.mkForce false;
    networking.hostName = lib.mkForce "machine";
  };
}
