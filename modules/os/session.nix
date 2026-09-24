# modules/os/session.nix — the login chain from the contract (implementation-path §2 (ii)).
#
# Consumes mura.xr.session.{autoLogin,greeter} and mura.xr.shell and owns services.greetd.
# Two profiles, exactly one selected by the contract's exclusivity assertion:
#
#   appliance  : greetd initial_session autologins the declared user into the session
#                (ADR 0007 §Two profiles; the default image, profiles/default.nix)
#   multi-user : greetd default_session runs the greeter DIRECTLY as the `greeter` user —
#                no dispatcher, no runtime-state session selection (ADR 0017 rev 2)
#
# Stand-ins (implementation-path §1, the stand-in rule): until the zxr compositor exists,
# sway is the session body and cage+gtkgreet the greeter. Both are development fixtures
# and never ship; each swap is an exit criterion (M1 for the session, G2 for the greeter).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura.xr.session;
  shell = config.mura.xr.shell;

  # STAND-IN — replaced at M1 by the zxr session (mura-session.target via the B6a wrapper,
  # specs/session-bootstrap.md). Until then every mura.xr.shell value lands in sway.
  sessionCommand = "${pkgs.sway}/bin/sway";

  # STAND-IN — replaced at G2 by zxr --greeter (registry: zxr --greeter mode row;
  # implementation-path §3 G2: gtkgreet and cage leave the closure at the swap).
  greeterCommand = "${pkgs.cage}/bin/cage -s -- ${pkgs.gtkgreet}/bin/gtkgreet";
in
{
  config = lib.mkIf (shell != "none") (lib.mkMerge [
    {
      services.greetd.enable = true;

      # Stand-in session body. sway's NixOS module wires the Wayland session basics
      # (portals, polkit agent env) the real session will provide itself.
      programs.sway.enable = lib.mkDefault true;

      # gtkgreet's session list — the stand-in greeter offers the stand-in session.
      # (zxr --greeter enumerates sessions from mura.xr.shell instead; session-auth §5.)
      environment.etc."greetd/environments".text = "sway\n";
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
