# Common distribution policy — device-independent.
#
# Owns session, networking policy, users, logging, and the base userspace shape.
# Device modules select and configure; they do not fork these services
# (docs/architecture/overview.md, invariant 6).
{ lib, config, pkgs, ... }:
let
  cfg = config.spatial;
in
{
  config = {
    # Identify the distribution.
    system.nixos.distroId = lib.mkDefault "spatial-os";
    system.nixos.distroName = lib.mkDefault "spatial-os";

    # A Wayland-based system with a systemd user session — the contract the XR
    # runtime and shell assume (docs/research/05-xr-userspace.md §6).
    services.dbus.enable = true;
    security.polkit.enable = true;
    hardware.graphics.enable = lib.mkDefault true;

    # Surface the evaluated device contract to on-device tools as JSON, mirroring
    # postmarketOS installing deviceinfo into the image (single descriptor consumed
    # at build- and run-time; docs/research/02-postmarketos.md §9 item 9).
    environment.etc."spatial-device.json".text = builtins.toJSON {
      inherit (cfg.device) codename vendor name arch supportTier;
      soc = cfg.hardware.soc;
      bootScheme = cfg.deployment.bootScheme;
      xrRuntime = cfg.xr.runtime;
    };

    # Minimal sane defaults for a bring-up image; devices/images override.
    networking.hostName = lib.mkDefault "spatial-${cfg.device.codename}";
    time.timeZone = lib.mkDefault "UTC";
    i18n.defaultLocale = lib.mkDefault "C.UTF-8";
  };
}
