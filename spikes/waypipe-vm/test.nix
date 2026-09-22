# Spike: "share-the-app" tier via Wayland protocol proxying (waypipe), across a real
# machine boundary, fully reproducible as a two-node NixOS VM test.
#
#   appHost:  runs the app (foot, shm/pixman rendering) under `waypipe server`
#   xrHost:   runs headless sway (pixman renderer) + `waypipe client`
#   bridge:   waypipe's unix sockets joined over the test network with socat
#             (waypipe only needs a reliable byte stream; this stands in for
#              ssh / USB-gadget CDC-NCM in the real deployment)
#
# Asserts the remote app's toplevel appears in sway's tree as a REAL wl_surface
# (mode 4 of the sharing taxonomy: no pixels captured, protocol forwarded), and
# takes a grim screenshot as evidence.
#
# Run:  nix build --impure -f spikes/waypipe-vm/test.nix -o spikes/result-waypipe
# This is a spike artifact, not part of `nix flake check` (it boots two VMs).
{ system ? "x86_64-linux" }:
let
  flake = builtins.getFlake (toString ../../.);
  pkgs = flake.inputs.nixpkgs.legacyPackages.${system};

  # Environment for talking to the headless sway instance on xrHost.
  swayEnv = "XDG_RUNTIME_DIR=/tmp/xdg WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$(ls /tmp/xdg/sway-ipc.* 2>/dev/null | head -1)";
in
pkgs.testers.runNixOSTest {
  name = "waypipe-share-the-app";

  nodes = {
    appHost = { pkgs, ... }: {
      environment.systemPackages = with pkgs; [ waypipe socat foot ];
      # foot renders via wl_shm/pixman -> exercises waypipe's shm diff+compress path;
      # no GPU needed in the guest.
    };

    xrHost = { pkgs, ... }: {
      environment.systemPackages = with pkgs; [ waypipe socat sway grim jq ];
      # sway runs with WLR_BACKENDS=headless + WLR_RENDERER=pixman: no DRM, no GPU,
      # no logind session needed.
      fonts.packages = with pkgs; [ dejavu_fonts ];
    };
  };

  testScript = ''
    start_all()
    appHost.wait_for_unit("multi-user.target")
    xrHost.wait_for_unit("multi-user.target")

    # --- headless sway on xrHost -------------------------------------------
    xrHost.succeed("mkdir -p -m 700 /tmp/xdg")
    xrHost.execute(
        "XDG_RUNTIME_DIR=/tmp/xdg WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 "
        "WLR_RENDERER=pixman sway --config /etc/sway/config >/tmp/sway.log 2>&1 &"
    )
    xrHost.wait_until_succeeds(
        "test -e /tmp/xdg/wayland-1 || test -e /tmp/xdg/wayland-0", timeout=60
    )
    # Detect the display name sway actually took, and wait for its IPC socket
    # (swaymsg needs SWAYSOCK; it cannot derive it from WAYLAND_DISPLAY).
    display = xrHost.succeed(
        "basename $(ls /tmp/xdg/wayland-? | head -1)"
    ).strip()
    xrHost.wait_until_succeeds("ls /tmp/xdg/sway-ipc.*.sock", timeout=60)
    env = (
        f"XDG_RUNTIME_DIR=/tmp/xdg WAYLAND_DISPLAY={display} "
        "SWAYSOCK=$(ls /tmp/xdg/sway-ipc.*.sock | head -1)"
    )
    xrHost.succeed(f"{env} swaymsg -t get_version >&2")

    # --- waypipe client on xrHost (compositor side) ------------------------
    xrHost.execute(
        f"XDG_RUNTIME_DIR=/tmp/xdg WAYLAND_DISPLAY={display} "
        "waypipe --socket /tmp/wpc.sock client >/tmp/waypipe-client.log 2>&1 &"
    )
    xrHost.wait_until_succeeds("test -S /tmp/wpc.sock")
    # Bridge: TCP 9500 -> waypipe client's unix socket.
    xrHost.execute(
        "socat TCP-LISTEN:9500,fork,reuseaddr UNIX-CONNECT:/tmp/wpc.sock "
        ">/tmp/socat.log 2>&1 &"
    )

    # --- waypipe server + app on appHost (app side) -------------------------
    appHost.execute(
        "socat UNIX-LISTEN:/tmp/wps.sock,fork TCP:xrHost:9500 "
        ">/tmp/socat.log 2>&1 &"
    )
    appHost.wait_until_succeeds("test -S /tmp/wps.sock")
    appHost.execute(
        "XDG_RUNTIME_DIR=/tmp waypipe --socket /tmp/wps.sock server -- "
        "foot >/tmp/waypipe-server.log 2>&1 &"
    )

    # --- assert: the remote app is a real toplevel in sway ------------------
    xrHost.wait_until_succeeds(
        f"{env} swaymsg -t get_tree "
        "| jq -e '.. | objects | select(.app_id? == \"foot\")' >&2",
        timeout=120,
    )
    xrHost.succeed(f"{env} grim /tmp/screenshot.png")
    xrHost.copy_from_machine("/tmp/screenshot.png")
    xrHost.copy_from_machine("/tmp/waypipe-client.log")
    appHost.copy_from_machine("/tmp/waypipe-server.log")

    print("share-the-app via waypipe: foot (appHost) is a native wl_surface in sway (xrHost)")
  '';
}
