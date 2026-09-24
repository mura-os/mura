# Valve Steam Frame ("deckard") — the first real device target.
#
# Facts: docs/research/07-device-landscape.md §Steam Frame and the donor audit
# docs/research/33-steam-frame-donor.md. This device file targets the VM-boot
# proof (uefi-rauc image family, QEMU/aarch64): the image structure mirrors the
# donor's slot layout; hardware bring-up (panel/XR/U-Boot payload) is device-side
# future work gated on hardware.
{ lib, pkgs, config, ... }:
{
  imports = [ ../../soc/sm8650 ../../families/uefi-rauc ];

  mura.device = {
    codename = "deckard";
    vendor = "valve";
    name = "Valve Steam Frame";
    arch = "aarch64";
    supportTier = "booting";
    maintainers = [ ];
  };

  mura.hardware = {
    displays = 2;
    # 2160x2160 per eye LCD, 72-120 Hz (144 experimental) — doc 07.
    panel = { width = 2160; height = 2160; refresh = 90; };
    # Input floor facts, donor-verified from the archived sm8650-mp.dts (doc 42 §4.3):
    # PMIC pwrkey KEY_POWER, resin KEY_VOLUMEDOWN, gpio-keys "Volume Up" KEY_VOLUMEUP and
    # "Select" KEY_SELECT (0x161) — Valve's Aux button, documented as the head-cursor click
    # for controller-less login; vcnl4040 IIO proximity sensor.
    input = {
      hmdButtons = {
        power = "KEY_POWER";
        volumeUp = "KEY_VOLUMEUP";
        volumeDown = "KEY_VOLUMEDOWN";
        select = "KEY_SELECT";
      };
      selectRole = "select";
      backRole = "volumeDown";
      controllers = "imu-3dof"; # Frame controllers: buttons + IMU before cameras are up
      bluetooth = true;
      proximitySource = "iio";
    };
  };

  # First real donor: byte-verified reconstruction of Valve's VR-channel update
  # payload (doc 33). localOnly; never fetched by nix build.
  mura.donor = import ./donor.nix;

  mura.adaptation = {
    display.backend = "native";
    gpu.backend = "native"; # Adreno 750 / freedreno-turnip upstream
    camera.backend = "native";
    sensors.backend = "native";
    audio.backend = "native";
    wifiBt.backend = "native";
    tracking.backend = "device-specific"; # inside-out CV; no open driver yet
  };

  mura.xr = {
    runtime = "monado";
    compositor.backend = "window"; # VM proof: windowed; vk-display on hardware
    environment = { };
  };

  mura.kernel.contract = [ "systemd" "container" ];

  mura.deployment = {
    bootScheme = "uefi-rauc";
    abSlots = true;
    flashMethod = "rauc";
    imageVariants = [ "uefi-rauc" ];
    protectedPartitions = [ "syspersist" ]; # donor: PARTLABEL syspersist, mounted ro
  };

  ###### VM-proof userspace (parity with devices/virtual-headset) ######
  services.getty.autologinUser = lib.mkDefault "mura";
  users.users.mura = {
    isNormalUser = true;
    password = "mura";
    extraGroups = [ "wheel" "video" "input" ];
  };
  security.sudo.wheelNeedsPassword = false;

  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = true;
  };

  # Serial console for qemu -nographic and headless smoke runs.
  boot.kernelParams = [ ];
  systemd.services."serial-getty@ttyAMA0".enable = true;

  # Wayland userspace exercised in the VM, as on virtual-headset.
  programs.sway.enable = lib.mkDefault true;

  system.stateVersion = lib.mkDefault "25.05";
}
