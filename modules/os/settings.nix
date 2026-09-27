# modules/os/settings.nix — the settings schema artifact and the settings daemon
# (specs/settings-schema.md rev 3; specs/settings-daemon.md rev 1; research/58; D7).
#
# GSettings' shape on a NixOS machine: the schema is compiled at build time from the evaluated
# option tree (lib/settings) into /etc/mura/settings-schema.json — the only default channel
# (constraint 9) — exactly as programs.dconf compiles its system db and locks into /etc/dconf;
# per-user values are sparse files under XDG written by one session daemon on the session bus
# (dconf-service's single writer), D-Bus-activated, resident once started (it is the notifier).
# A generation switch reaches running sessions through NixOS's own hook for it,
# system.userActivationScripts (the nixos-activation user service switch-to-configuration
# restarts on every switch). The device stratum (settings-schema.md §2.1, ruled shape C) is
# reserved: the artifact carries it, the system mode is enabled only when a target declares a
# device key, and until that rung lands an assertion refuses a device key rather than leaving it
# unserved.
{ lib, config, pkgs, options, ... }:
let
  inherit (lib) mkOption types;
  cfg = config.mura.settings;
  settingsLib = import ../../lib/settings { inherit lib; };
  artifact = settingsLib.generate {
    inherit options config;
    templates = cfg.templates;
    schemaVersions = cfg.schemaVersions;
    locks = cfg.locks;
  };
  artifactFile = pkgs.writeText "settings-schema.json" (builtins.toJSON artifact);
  deviceKeys = lib.filter (k: k.stratum == "device") artifact.keys;
  settingsd = pkgs.mura.settingsd;

  keyType = types.submodule {
    options = {
      type = mkOption { type = types.enum [ "bool" "int" "double" "string" "enum" ]; description = "The key's type (settings-schema.md §1)."; };
      default = mkOption { type = types.anything; description = "The generated default (the only default channel)."; };
      values = mkOption { type = types.listOf types.str; default = [ ]; description = "Enum values (type = enum)."; };
      range = mkOption {
        type = types.nullOr (types.submodule { options = { min = mkOption { type = types.number; }; max = mkOption { type = types.number; }; }; });
        default = null;
        description = "Numeric range (int/double).";
      };
      description = mkOption { type = types.str; default = ""; };
      class = mkOption { type = types.enum [ "preference" "state" ]; default = "preference"; };
      mutability = mkOption { type = types.enum [ "mutable" "immutable" ]; default = "mutable"; description = "Template keys exist to be written: mutable (Nix default, wearer override) unless a template says otherwise."; };
      apply = mkOption { type = types.str; default = "live"; description = "A label: live | restart:<unit> | relogin | reboot (§6)."; };
    };
  };
in
{
  options.mura.settings = {
    templates = mkOption {
      type = types.attrsOf (types.submodule {
        options = {
          schemaVersion = mkOption { type = types.ints.positive; default = 1; };
          keys = mkOption { type = types.attrsOf keyType; };
        };
      });
      default = { };
      description = ''
        Relocatable schemas (settings-schema.md §1.1): a template declared once and instantiated
        at runtime ids `<template>:<instance>` by the component that owns the referent.
      '';
    };
    schemaVersions = mkOption {
      type = types.attrsOf types.ints.positive;
      default = { };
      description = "Per-schema version (settings-schema.md §5); a bump needs a migration step in pkgs/mura-settingsd.";
    };
    locks = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "xr.passthrough.latencyMode" ];
      description = "Keys the profile locks (settings-schema.md §7): they resolve to the generated value and refuse writes.";
    };
  };

  config = {
    # The one template the contract names (settings-schema.md §8): entry-policy grants are
    # `places.entry:<place_id>` instances. Its owner (the places model) is M1's; the template
    # exists so the instance machinery is real and tested before it.
    mura.settings.templates."places.entry" = {
      keys = {
        enabled = { type = "bool"; default = true; description = "Whether this place may be entered."; };
        launch = { type = "string"; default = ""; description = "What to launch on entry (empty: nothing)."; };
        summon = { type = "string"; default = ""; description = "What to summon (bring to the wearer) on entry (empty: nothing) — settings-schema.md §1.1 names it beside `launch`."; };
      };
    };

    # The shell placement table (shell-plane.md §2.6; owner ruling 2026-09-27, research/77 §3.3a):
    # `shell.place:<namespace>` instances, one per layer-shell namespace (`osk`, `notifications`,
    # `waybar`, …), written by the wearer's grab on a shell plane or by hand. A row wins over the
    # client's zxr-layer-anchoring request; without a row the request applies; without either the
    # head fallback (`shell.head.*`). Hyprland's layer rules by namespace are the precedent. The
    # template's defaults are the head fallback; zxr's seed rows for the carried components'
    # namespaces (research/77 §3.3a) apply to an instance with no stored value. Class `state`:
    # remembered placement, not intent (settings-schema.md §2).
    mura.settings.templates."shell.place" = {
      keys = {
        frame = { type = "enum"; values = [ "head" "body" "hand_left" "hand_right" "world" "docked" ]; default = "head"; class = "state"; description = "The anchoring frame (zxr-layer-anchoring-v1's enum). An unavailable frame falls back per the protocol (hand → body, docked → head)."; };
        azimuth_deg = { type = "double"; default = 0.0; range = { min = -180.0; max = 180.0; }; class = "state"; description = "Centre azimuth in the frame, degrees (positive = right)."; };
        elevation_deg = { type = "double"; default = 0.0; range = { min = -90.0; max = 90.0; }; class = "state"; description = "Centre elevation in the frame, degrees (positive = up)."; };
        distance_m = { type = "double"; default = 0.5; range = { min = 0.2; max = 5.0; }; class = "state"; description = "Presentation distance from the frame origin, metres."; };
        pitch_deg = { type = "double"; default = 0.0; range = { min = -90.0; max = 90.0; }; class = "state"; description = "Pitch of the plane about its horizontal axis, degrees (negative = tilted toward a wearer looking down at it — WayVR's keyboard −10)."; };
        width_deg = { type = "double"; default = 0.0; range = { min = 0.0; max = 180.0; }; class = "state"; description = "Horizontal angular size, degrees; 0 = the compositor's choice (the arranged pixel size at the frame's pixels-per-degree)."; };
      };
    };

    assertions = [
      {
        assertion = deviceKeys == [ ];
        message = "mura.settings: ${toString (map (k: k.id) deviceKeys)} declare stratum = device, but the system mode (settings-schema.md §2.1) is not built yet; a device key would be unserved.";
      }
    ];

    system.build.muraSettingsSchema = artifactFile;
    environment.etc."mura/settings-schema.json".source = artifactFile;

    environment.systemPackages = [ settingsd ]; # mura-settings, the CLI
    services.dbus.packages = [ settingsd ]; # org.mura.Settings1 activation on the user bus

    systemd.user.services.mura-settingsd = {
      description = "Mura settings daemon (org.mura.Settings1)";
      unitConfig.ConditionUser = "!@system";
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "org.mura.Settings1";
        ExecStart = "${settingsd}/bin/mura-settingsd";
        Restart = "on-failure";
        RestartSec = "1s";
      };
    };

    # The generation hook (settings-daemon.md §3): switch-to-configuration restarts
    # nixos-activation.service in every logged-in user's manager; a RUNNING daemon re-reads the
    # artifact and signals what moved. One not running has no subscribers to tell and reads the
    # new artifact when the bus activates it — so the hook never starts it (that would defeat
    # activation on first use: nixos-activation also runs at every login).
    system.userActivationScripts.muraSettings.text = ''
      if ${config.systemd.package}/bin/systemctl --user --quiet is-active mura-settingsd.service; then
        ${settingsd}/bin/mura-settings generation-changed >/dev/null 2>&1 || true
      fi
    '';
  };
}
