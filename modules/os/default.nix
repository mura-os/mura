# Common distribution policy — device-independent.
#
# Owns session, networking policy, users, logging, and the base userspace shape.
# Device modules select and configure; they do not fork these services
# (docs/architecture/overview.md, invariant 6).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura;
in
{
  # One file per concern (docs/architecture/repo-structure.md §modules/os ownership table).
  imports = [ ./session.nix ./persist.nix ./policy.nix ./oob.nix ./health.nix ];

  config = {
    # Identify the distribution.
    system.nixos.distroId = lib.mkDefault "mura";
    system.nixos.distroName = lib.mkDefault "Mura";

    # A Wayland-based system with a systemd user session — the contract the XR
    # runtime and shell assume (docs/research/05-xr-userspace.md §6).
    services.dbus.enable = true;
    security.polkit.enable = true;
    hardware.graphics.enable = lib.mkDefault true;

    # The headset is an ordinary Linux host reachable from a device you already hold
    # (first-run-onboarding.md §5, the postmarketOS pattern): sshd on every profile, with
    # OpenSSH's own defaults — password auth on every interface, PermitEmptyPasswords no. A
    # passwordless account therefore gets SSH after `passwd` (in the headset or via the setup
    # web app over cable/hotspot) or with a declared key (profiles/dev.nix). Internet exposure
    # is the network's job, as for any Linux PC. mkDefault: a profile or device may turn it off.
    services.openssh.enable = lib.mkDefault true;

    # Surface the evaluated device contract to on-device tools as JSON, mirroring
    # postmarketOS installing deviceinfo into the image (single descriptor consumed
    # at build- and run-time; docs/research/02-postmarketos.md §9 item 9).
    environment.etc."mura-device.json".text = builtins.toJSON {
      inherit (cfg.device) codename vendor name arch supportTier;
      soc = cfg.hardware.soc;
      bootScheme = cfg.deployment.bootScheme;
      xrRuntime = cfg.xr.runtime;
    };

    # Minimal sane defaults for a bring-up image; devices/images override.
    networking.hostName = lib.mkDefault "mura-${cfg.device.codename}";
    # The zone is the wearer's, set at runtime through timedated (the setup instance derives it,
    # the in-headset confirm and settings change it — first-run §4.2, multi-user §3.1's wheel
    # rule). NixOS's null means "UTC until set imperatively with timedatectl"; a fixed value
    # would make /etc/localtime a store symlink that timedated cannot change.
    time.timeZone = lib.mkDefault null;
    i18n.defaultLocale = lib.mkDefault "C.UTF-8";
  };
}
