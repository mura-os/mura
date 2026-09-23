# virtual-headset — the x86_64 VM smoke target.
#
# Proves the whole module stack (contract + os + xr + adaptation) evaluates and builds
# end-to-end without hardware: a NixOS VM running the common userspace under Wayland
# with Monado's simulated driver. This is the "prove the module stack" half of the
# Phase 1 implementation order (docs/research/00-synthesis.md §7 item 9), not a real port.
{ lib, pkgs, config, ... }:
{
  imports = [ ../../soc/virtual ];

  spatial.device = {
    codename = "virtual-headset";
    vendor = "spatial-os";
    name = "Virtual Headset (VM smoke target)";
    arch = "x86_64";
    supportTier = "booting";
    maintainers = [ ];
  };

  spatial.hardware = {
    displays = 1;
    panel = { width = 1920; height = 1080; refresh = 60; };
  };

  # No donor: this is a from-source VM, so donor stays null and no flashable image
  # outputs are produced (null-propagation gating).
  spatial.donor = null;

  # Native everything; simulated tracking (Monado's SIMULATED driver).
  spatial.adaptation = {
    display.backend = "native";
    gpu.backend = "native";
    camera.backend = "native";
    sensors.backend = "native";
    audio.backend = "native";
    wifiBt.backend = "native";
    tracking.backend = "device-specific"; # simulated, provided by the runtime itself
  };

  spatial.xr = {
    runtime = "monado";
    compositor.backend = "window"; # windowed compositor inside the VM, not vk-display
    environment = {
      # Monado's simulated-HMD setup for a VM without real hardware: the simulated
      # system builder is excluded from auto-discovery unless explicitly enabled
      # (monado target_builder_simulated.c).
      SIMULATED_ENABLE = "true";
    };
  };

  spatial.kernel.contract = [ "systemd" "container" ];

  spatial.deployment = {
    bootScheme = "vm";
    flashMethod = "none";
    imageVariants = [ "dev-vm" ];
  };

  # Standard NixOS bits that make the VM boot and present a Wayland session.
  # (Kept at the top level: mixing these with an explicit `config` block is rejected
  # by the module system when top-level `spatial.*` options are also set.)
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = false;
  fileSystems."/" = lib.mkDefault { device = "/dev/disk/by-label/nixos"; fsType = "ext4"; };

  services.getty.autologinUser = lib.mkDefault "spatial";
  users.users.spatial = {
    isNormalUser = true;
    password = "spatial";
    extraGroups = [ "wheel" "video" "input" ];
  };

  # A minimal Wayland compositor so the "common userspace under Wayland" contract
  # is actually exercised in the VM.
  programs.sway.enable = lib.mkDefault true;

  # Rung-2 dev-loop tuning (VM builds only; docs: README §Development). The VM
  # shares the host /nix/store, so iteration never builds an image: edit modules,
  # `nix run .#virtual-headset-vm`, and the QEMU window boots straight into sway.
  virtualisation.vmVariant = {
    virtualisation = {
      memorySize = 8192;
      cores = 4;
      # virgl: real GL inside the guest (wlroots/Monado want more than llvmpipe).
      qemu.options = [
        "-device virtio-gpu-gl-pci"
        "-display gtk,gl=on,show-cursor=on"
      ];
      forwardPorts = [
        { from = "host"; host.port = 2221; guest.port = 22; }
      ];
    };

    services.openssh = {
      enable = true;
      settings.PasswordAuthentication = true;
    };

    # Boot to a visible session with zero manual steps: the autologin getty on
    # tty1 execs sway. (Guarded so serial/ssh shells stay plain shells.)
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ] && [ -z "''${WAYLAND_DISPLAY:-}" ]; then
        exec sway
      fi
    '';
  };

  system.stateVersion = lib.mkDefault "25.05";
}
