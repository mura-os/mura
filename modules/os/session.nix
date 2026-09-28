# modules/os/session.nix — the login chain from the contract (implementation-path §2 (ii)).
#
# Consumes mura.xr.session.{autoLogin,greeter,readinessTimeoutSeconds} and mura.xr.shell and
# owns services.greetd, the session wrapper and the session's user units. Two profiles,
# exactly one selected by the contract's exclusivity assertion:
#
#   appliance  : greetd initial_session autologins the declared user into the session
#                (ADR 0007 §Two profiles; the default image, profiles/default.nix)
#   multi-user : greetd default_session runs the greeter DIRECTLY as the `greeter` user —
#                no dispatcher, no runtime-state session selection (ADR 0017 rev 2)
#
# The session wrapper (B6a, specs/session-bootstrap.md, D4 rev 3) is **mura-session**
# (pkgs/mura-session, Rust, libc only). It implements the mechanism uwsm demonstrated and D4
# verified — "the display manager execs a program that starts the compositor as a user unit,
# publishes its environment after readiness, and stays alive until the session is torn down"
# (AGENTS rule 1) — over the STATIC units below instead of uwsm's login-time unit generation:
#   - seat acquisition inside a user unit: `mura-session start` writes XDG_SESSION_ID/XDG_SEAT/
#     XDG_VTNR to $XDG_RUNTIME_DIR/mura/session.env, the EnvironmentFile of
#     mura-compositor.service, so libseat's logind backend finds the session — finding F1;
#   - readiness: mura-compositor.service is Type=notify with TimeoutStartSec from the contract;
#     the compositor runs `mura-session finalize` (sway stand-in) or sd_notify natively (zxr);
#   - lifetime: `mura-session start` waits on mura-session.target and stops it on
#     SIGTERM/SIGHUP, returning only when the session is down — spec §4.6/4.7.
# Why not uwsm itself: it is 6.4k lines of Python and three interpreter starts on every login
# (~1.8 s measured in the VM, D4); the ruling is that no interpreter sits on the session-start
# path (AGENTS.md; specs/session-bootstrap.md §9). Unit semantics are copied 1:1 from uwsm's
# templates (uwsm 0.26.7 lib/systemd/user/*) minus the per-compositor templating we do not need.
#
# Stand-ins (implementation-path §1, the stand-in rule): until the zxr compositor exists,
# sway is the session body and cage+gtkgreet the greeter. Both are development fixtures
# and never ship; each swap is an exit criterion (M1 for the session, G2 for the greeter).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura.xr.session;
  shell = config.mura.xr.shell;

  # STAND-IN — replaced at M1 by the zxr session binary. Until then every mura.xr.shell
  # value lands in sway, as ExecStart of mura-compositor.service.
  compositorBinary = "${pkgs.sway}/bin/sway";

  # The session command greetd execs (both profiles). The compositor is not an argument: it
  # is the static unit's ExecStart. greetd's config and gtkgreet's session list read
  # `mura-session`.
  sessionCommand = "${lib.getExe pkgs.mura.session} start";

  # STAND-IN — replaced at G2 by zxr --greeter (registry: zxr --greeter mode row;
  # implementation-path §3 G2: gtkgreet and cage leave the closure at the swap).
  greeterCommand = "${pkgs.cage}/bin/cage -s -- ${pkgs.gtkgreet}/bin/gtkgreet";

  # G1's greeter command (specs/zxr-core.md §9 rev 3.13; session-auth rev 6 §5): greetd runs
  # zxr in greeter mode as the `greeter` user; zxr spawns the program and the OSK over
  # socketpairs (the kiosk's primary and its keyboard) and exits with the program (cage's
  # rule). Defined here, wired at G2 (the `greeterCommand` flip is G2's exit criterion).
  # The picker's UID window rides the program's environment (`accounts.rs`), never login.defs.
  zxrGreeterCommand = "${lib.getExe pkgs.mura.zxr} --greeter --trusted 'MURA_UID_MIN=${toString cfg.multiUser.uidRange.min} MURA_UID_MAX=${toString cfg.multiUser.uidRange.max} exec ${lib.getExe pkgs.mura.greeter}' --osk ${lib.getExe pkgs.squeekboard}";

  profileName = if cfg.autoLogin != null then "appliance" else "multi-user";

  # Every session unit ends with the session (uwsm's shutdown-target shape): a unit that
  # stops or fails pulls in mura-session-shutdown.target, which conflicts with the whole
  # graphical session, irreversibly.
  endsSession = {
    OnSuccess = "mura-session-shutdown.target";
    OnSuccessJobMode = "replace-irreversibly";
    OnFailure = "mura-session-shutdown.target";
    OnFailureJobMode = "replace-irreversibly";
    CollectMode = "inactive-or-failed";
  };
in
{
  config = lib.mkIf (shell != "none") (lib.mkMerge [
    {
      services.greetd.enable = true;

      ## The wrapper ------------------------------------------------------------------------
      environment.systemPackages = [ pkgs.mura.session pkgs.mura.authd pkgs.mura.greeter ];
      # uwsm's module chose dbus-broker for the user bus; the reason (activation-environment
      # handling for units the session starts) holds without uwsm, so the choice stays.
      services.dbus.implementation = lib.mkDefault "broker";

      ## The session units (static; specs/session-bootstrap.md §5) ---------------------------
      # `mura-compositor.service`: the compositor as a user unit. Never wantedBy anything —
      # only mura-session.target (Requires=) starts it, and only the wrapper starts that.
      systemd.user.services.mura-compositor = {
        description = "Mura compositor (XR session body)";
        bindsTo = [ "mura-session.target" ];
        before = [ "mura-session.target" "graphical-session.target" "mura-session-shutdown.target" ];
        after = [ "graphical-session-pre.target" ];
        wants = [ "graphical-session-pre.target" ];
        conflicts = [ "mura-session-shutdown.target" ];
        # No unit-private PATH: the compositor (and everything it execs) inherits the user
        # manager's session PATH from environment.d/50-systemd-path.conf — /run/wrappers,
        # the per-user profile, the system profile — like any desktop session unit. NixOS's
        # default `path` for services would otherwise pin PATH to coreutils+systemd and sway's
        # `exec` lines (which run through `sh -c`) fail with ENOENT (found at D4 rev 3).
        path = lib.mkForce [ ];
        unitConfig = endsSession // {
          PropagatesStopTo = "mura-session.target graphical-session.target";
          # ADR 0007 crash semantics: a compositor crash restarts it INSIDE the session (into
          # the locked state once authd exists, D5). uwsm ships Restart=no + OnFailure=
          # (compositor death = session end). Since systemd v254 a failing service passes
          # through `failed` before an auto-restart and OnFailure= fires each time (found at
          # D4, references/systemd/src/core/unit.c unit_notify + service.c
          # SERVICE_FAILED_BEFORE_AUTO_RESTART) — RestartMode=direct is the standard answer:
          # restarts skip the failed state, OnFailure= only fires when the start-rate limit is
          # hit, and the session then shuts down. [engineering judgment, D4; spec §5]
          StartLimitIntervalSec = "60s";
          StartLimitBurst = 3;
        };
        serviceConfig = {
          Type = "notify";
          NotifyAccess = "all";
          # STAND-IN — replaced at M1 by zxr, which notifies READY=1 natively.
          ExecStart = compositorBinary;
          # The session-specific class (spec §3), written by `mura-session start` step 2.
          EnvironmentFile = "-%t/mura/session.env";
          Restart = "on-failure";
          RestartMode = "direct";
          RestartSec = "1s";
          # The readiness bound, from the contract (spec §4 step 4).
          TimeoutStartSec = "${toString cfg.readinessTimeoutSeconds}s";
          TimeoutStopSec = "10s";
          # The compositor creates these (spec §3); on a direct restart the user manager still
          # holds the dead instance's values, and a compositor that inherits WAYLAND_DISPLAY
          # will try to run nested inside itself (found at D4 with sway). Never inherit them.
          UnsetEnvironment = "WAYLAND_DISPLAY DISPLAY";
          Slice = "session.slice";
          SyslogIdentifier = "mura-compositor";
        };
      };

      # `mura-session.target`: the XR session body named throughout the corpus (B6, ADR 0007;
      # spec §5). The wrapper's `start --wait` target: active while the session runs, gone
      # when it is torn down. BindsTo=graphical-session.target starts the standard target
      # (which shell services and portals hang off) and stops with it. Ordering note: target
      # units implicitly order After= their Requires=/Wants= units, so this target must NOT
      # be After=graphical-session.target — that is an ordering cycle (found at D4).
      systemd.user.targets.mura-session = {
        description = "Mura XR session (Monado + compositor + shell services)";
        requires = [ "mura-compositor.service" ];
        wants = lib.optional config.services.monado.enable "monado.socket";
        bindsTo = [ "graphical-session.target" ];
        before = [ "graphical-session.target" "mura-session-shutdown.target" ];
        after = [ "graphical-session-pre.target" ];
        conflicts = [ "mura-session-shutdown.target" ];
        unitConfig = {
          PropagatesStopTo = "graphical-session.target";
          StopWhenUnneeded = true;
        };
      };

      # `mura-session-bindpid@PID.service`: the session ends if the wrapper dies (greetd
      # killed it, the login was torn down) — util-linux waitpid on the wrapper's pid.
      systemd.user.services."mura-session-bindpid@" = {
        description = "Bind the Mura session to wrapper PID %i";
        before = [ "mura-session-shutdown.target" ];
        conflicts = [ "mura-session-shutdown.target" ];
        unitConfig = endsSession;
        serviceConfig = {
          Type = "exec";
          ExecStart = "${pkgs.util-linux}/bin/waitpid -e %i";
          Restart = "no";
          Slice = "background.slice";
          SyslogIdentifier = "mura-session-bindpid";
        };
      };

      # `mura-session-shutdown.target`: the one-way exit. Conflicts with the whole graphical
      # session; StopWhenUnneeded so it vanishes once everything it conflicted with is down.
      systemd.user.targets.mura-session-shutdown = {
        description = "Shut down the Mura session";
        conflicts = [ "graphical-session-pre.target" "graphical-session.target" "xdg-desktop-autostart.target" ];
        after = [ "graphical-session-pre.target" "graphical-session.target" "xdg-desktop-autostart.target" ];
        unitConfig = {
          DefaultDependencies = false;
          StopWhenUnneeded = true;
        };
      };

      # `mura-greeter-lock.service`: the lock program, resident in the session (ADR 0007
      # amendment 2; session-auth rev 6 §2, §5; research/78 §9 Q1 ruled). It waits for logind's
      # `Session.Lock`, locks through ext-session-lock-v1 on the compositor's public socket and
      # owns its mura-authd conversation; the compositor keeps the lock when it dies and this
      # unit restarts it, which re-locks (cosmic-session's shape for cosmic-greeter's locker:
      # `PartOf=graphical-session.target`, `Restart=on-failure`). The compositor's triggers
      # (doff grace, idle ladder) run `loginctl lock-session`, which reaches this program.
      systemd.user.services.mura-greeter-lock = {
        description = "Mura lock screen (ext-session-lock client)";
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        wantedBy = [ "graphical-session.target" ];
        path = lib.mkForce [ ];
        serviceConfig = {
          Type = "exec";
          ExecStart = "${lib.getExe pkgs.mura.greeter} --lock";
          Restart = "on-failure";
          RestartSec = "1s";
          Slice = "session.slice";
          SyslogIdentifier = "mura-greeter-lock";
        };
      };

      # The greeter's own state directory (multi-user.md §2; research/78 §9 F7): `last-user`
      # preselection is written by the greeter itself after `start_session`, so the directory is
      # the greeter user's — regreet's `/var/lib/regreet` and tuigreet's `/var/cache/tuigreet`,
      # both created `greeter greeter 0755` by their NixOS modules (`programs/regreet.nix`,
      # `services/display-managers/greetd.nix`). tmpfiles rather than the persist skeleton: it
      # runs after the account database exists. Per-user metadata (`<user>/`) is not here — an
      # open item (AccountsService's root-daemon shape; never a 1777 directory, since pre-auth
      # display names and avatars must not be forgeable by any local user).
      systemd.tmpfiles.rules = [ "d /var/lib/mura/state/accounts 0755 greeter greeter - -" ];

      # The greeter picker's enumeration window is login.defs' (multi-user.md §2; the greeter reads
      # UID_MIN/UID_MAX from /etc/login.defs — SDDM's and tuigreet's source). login.defs is NixOS's
      # own (`security.loginDefs`: UID_MAX 29999, because the nixbld range starts at 30000 —
      # misc/ids.nix); writing the contract's `multiUser.uidRange` there broke the multi-user VM's
      # login (found 2026-09-28, research/78 §9 F9), so it is not written. The option reaches the
      # greeter with G2's command line (`MURA_UID_MIN/MAX` in its environment), not through
      # login.defs.

      # Static environment class (spec §3): read by the user manager from environment.d.
      # XDG_CURRENT_DESKTOP is set by `mura-session start` (step 2); XR_RUNTIME_JSON is not
      # needed (services.monado installs the active_runtime.json the loader finds on its own).
      environment.etc."environment.d/60-mura.conf".text = ''
        MURA_PROFILE=${profileName}
        XDG_SESSION_TYPE=wayland
      '';

      ## Stand-in session body --------------------------------------------------------------
      # sway's NixOS module wires the Wayland session basics (portals, polkit agent env) the
      # real session will provide itself.
      programs.sway.enable = lib.mkDefault true;
      # STAND-IN — replaced at M1 by zxr. The NixOS sway config starts sway-session.target →
      # graphical-session.target itself, *before* our readiness ordering; under mura-session
      # the compositor must instead publish its variables and signal readiness through
      # `mura-session finalize` (spec §4.5). Override the NixOS drop-in.
      environment.etc."sway/config.d/nixos.conf".source = lib.mkForce (pkgs.writeText "nixos.conf" ''
        # STAND-IN (D4) — sway under mura-session: export the compositor-created variables to
        # the user manager and D-Bus, then notify mura-compositor.service READY=1.
        exec ${lib.getExe pkgs.mura.session} finalize SWAYSOCK I3SOCK
      '');

      # gtkgreet's session list — the stand-in greeter offers the stand-in session, through
      # the wrapper. (zxr --greeter enumerates sessions from mura.xr.shell instead;
      # session-auth §5.)
      environment.etc."greetd/environments".text = "mura-session start\n";
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
