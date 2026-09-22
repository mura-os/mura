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
      # Monado's in-headless/simulated setup for a VM without real HMD hardware.
      XRT_COMPOSITOR_FORCE_XCB = "0";
      P_OVERRIDE_ACTIVE_CONFIG = "1";
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

  system.stateVersion = lib.mkDefault "25.05";
}
