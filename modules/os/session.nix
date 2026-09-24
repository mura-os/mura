# modules/os/session.nix — the login chain from the contract (implementation-path §2 (ii)).
#
# Consumes mura.xr.session.{autoLogin,greeter,readinessTimeoutSeconds} and mura.xr.shell and
# owns services.greetd plus the session wrapper. Two profiles, exactly one selected by the
# contract's exclusivity assertion:
#
#   appliance  : greetd initial_session autologins the declared user into the session
#                (ADR 0007 §Two profiles; the default image, profiles/default.nix)
#   multi-user : greetd default_session runs the greeter DIRECTLY as the `greeter` user —
#                no dispatcher, no runtime-state session selection (ADR 0017 rev 2)
#
# The session wrapper (B6a, specs/session-bootstrap.md, D4) is **uwsm** — the standard
# "display manager execs a program that starts the compositor as a user unit, publishes its
# environment after readiness, and stays alive until the session is torn down" mechanism
# (AGENTS rule 1; evaluated against the spec in D4, ruled). What uwsm gives us, by file:
#   - seat acquisition inside a user unit: `uwsm start` saves XDG_SESSION_ID/XDG_SEAT/XDG_VTNR
#     into env_session.conf, the EnvironmentFile of wayland-wm@.service, so libseat's logind
#     backend finds the session (uwsm/main.py Varnames.session_specific) — review finding F1;
#   - readiness: wayland-wm@.service is Type=notify with TimeoutStartSec; the compositor runs
#     `uwsm finalize` (sway stand-in) or sd_notify natively (zxr) — spec §4.4/§4.5;
#   - lifetime: `uwsm start` waits on the session target and stops it on SIGTERM/SIGHUP,
#     returning only when the session is down (uwsm-libexec/signal-handler.sh) — spec §4.6/4.7.
# Mura adds two drop-ins (below) and a thin `mura-session.target` so the corpus name is true.
#
# Stand-ins (implementation-path §1, the stand-in rule): until the zxr compositor exists,
# sway is the session body and cage+gtkgreet the greeter. Both are development fixtures
# and never ship; each swap is an exit criterion (M1 for the session, G2 for the greeter).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura.xr.session;
  shell = config.mura.xr.shell;

  uwsm = lib.getExe pkgs.uwsm;

  # STAND-IN — replaced at M1 by the zxr session binary. Until then every mura.xr.shell
  # value lands in sway. uwsm derives the unit instance name from the binary: sway →
  # wayland-wm@sway.service / wayland-session@sway.target.
  compositorBinary = "${pkgs.sway}/bin/sway";

  # The session command greetd execs (both profiles): uwsm in front of the compositor.
  # -F hardcodes the command line into the unit drop-in; -N/-D give the session its name
  # and XDG_CURRENT_DESKTOP (the static environment class, spec §3). Wrapped in a named
  # script so greetd's config and gtkgreet's session list read `mura-session`.
  muraSession = pkgs.writeShellScriptBin "mura-session" ''
    exec ${uwsm} start -F -N Mura -D mura -- ${compositorBinary}
  '';
  sessionCommand = "${muraSession}/bin/mura-session";

  # STAND-IN — replaced at G2 by zxr --greeter (registry: zxr --greeter mode row;
  # implementation-path §3 G2: gtkgreet and cage leave the closure at the swap).
  greeterCommand = "${pkgs.cage}/bin/cage -s -- ${pkgs.gtkgreet}/bin/gtkgreet";

  profileName = if cfg.autoLogin != null then "appliance" else "multi-user";
in
{
  config = lib.mkIf (shell != "none") (lib.mkMerge [
    {
      services.greetd.enable = true;

      ## The wrapper: uwsm ---------------------------------------------------------------
      programs.uwsm.enable = true;
      environment.systemPackages = [ muraSession ];

      # Drop-ins on uwsm's units (NixOS merges these as overrides.conf on the packaged units;
      # the uwsm module already marks them restartIfChanged=false / enableDefaultPath=false).
      systemd.user.services."wayland-wm@" = {
        # ADR 0007 crash semantics: a compositor crash restarts it INSIDE the session (into
        # the locked state once authd exists, D5). uwsm ships Restart=no + OnFailure=
        # wayland-session-shutdown.target (compositor death = session end). Since systemd
        # v254 a failing service passes through `failed` before an auto-restart and
        # OnFailure= fires each time (found at D4, references/systemd/src/core/unit.c
        # unit_notify + service.c SERVICE_FAILED_BEFORE_AUTO_RESTART) — RestartMode=direct
        # is the standard answer: restarts skip the failed state, OnFailure= only fires when
        # the start-rate limit is hit, and the session then shuts down as uwsm intends.
        # [engineering judgment, D4; recorded in specs/session-bootstrap.md §5]
        unitConfig = {
          StartLimitIntervalSec = "60s";
          StartLimitBurst = 3;
        };
        serviceConfig = {
          Restart = "on-failure";
          RestartMode = "direct";
          RestartSec = "1s";
          # The readiness bound, from the contract (spec §4 step 4).
          TimeoutStartSec = "${toString cfg.readinessTimeoutSeconds}s";
          # The compositor creates these (spec §3); on a direct restart the user manager still
          # holds the dead instance's values, and a compositor that inherits WAYLAND_DISPLAY
          # will try to run nested inside itself (found at D4 with sway). Never inherit them.
          UnsetEnvironment = "WAYLAND_DISPLAY DISPLAY";
        };
      };

      # `mura-session.target`: the XR session body named throughout the corpus (B6, ADR 0007;
      # specs/session-bootstrap.md §5). Under uwsm it is a thin target pulled in by the
      # compositor's session target, ordered before graphical-session.target (which the
      # compositor reaches on readiness) and stopped with it; it owns the session's Monado
      # socket. Ordering note: target units implicitly order After= their Wants=, so this
      # target must NOT be After=graphical-session.target — that is an ordering cycle with
      # wayland-session@.target (found at D4).
      systemd.user.targets.mura-session = {
        description = "Mura XR session (Monado + compositor + shell services)";
        wants = lib.optional config.services.monado.enable "monado.socket";
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session-pre.target" ];
        before = [ "graphical-session.target" ];
      };
      systemd.user.targets."wayland-session@" = {
        wants = [ "mura-session.target" ];
      };

      # Static environment class (spec §3): read by the user manager from environment.d.
      # XDG_CURRENT_DESKTOP comes from `uwsm start -D`; XR_RUNTIME_JSON is not needed
      # (services.monado installs the active_runtime.json the loader finds on its own).
      environment.etc."environment.d/60-mura.conf".text = ''
        MURA_PROFILE=${profileName}
        XDG_SESSION_TYPE=wayland
      '';

      ## Stand-in session body --------------------------------------------------------------
      # sway's NixOS module wires the Wayland session basics (portals, polkit agent env) the
      # real session will provide itself.
      programs.sway.enable = lib.mkDefault true;
      # STAND-IN — replaced at M1 by zxr. The NixOS sway config starts sway-session.target →
      # graphical-session.target itself, *before* uwsm's readiness ordering; under uwsm the
      # compositor must instead publish its variables and signal readiness through
      # `uwsm finalize` (spec §4.5; uwsm README "sway"). Override the NixOS drop-in.
      environment.etc."sway/config.d/nixos.conf".source = lib.mkForce (pkgs.writeText "nixos.conf" ''
        # STAND-IN (D4) — sway under uwsm: export the compositor-created variables to the
        # user manager and D-Bus, then notify wayland-wm@sway.service READY=1.
        exec ${uwsm} finalize SWAYSOCK I3SOCK
      '');

      # gtkgreet's session list — the stand-in greeter offers the stand-in session, through
      # the wrapper. (zxr --greeter enumerates sessions from mura.xr.shell instead;
      # session-auth §5.)
      environment.etc."greetd/environments".text = "mura-session\n";
    }

    # Appliance / default image: autologin. greetd's module sets `restart = false` when
    # initial_session exists, so a crashed session does not autologin in a loop; the
    # session's own units handle restarts (specs/session-bootstrap.md §6).
    (lib.mkIf (cfg.autoLogin != null) {
      services.greetd.settings = {
        initial_session = {
          command = sessionCommand;
          user = cfg.autoLogin;
        };
        # Logout re-runs the autologin (the Steam Deck shape; power off is the way out).
        default_session = {
          command = sessionCommand;
          user = cfg.autoLogin;
        };
      };
    })

    # Multi-user: the greeter, directly. The `greeter` user is created by the greetd
    # module; the contract's declared-account assertion guarantees someone can log in.
    (lib.mkIf (cfg.greeter != "none") {
      services.greetd.settings.default_session = {
        command = greeterCommand; # STAND-IN — replaced at G2 by zxr --greeter
        user = "greeter";
      };
    })
  ]);
}
