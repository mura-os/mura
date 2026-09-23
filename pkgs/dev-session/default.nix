# dev-session — rung 1 of the development ladder (composition doc §7.1 / ADR 0006
# dev mode): the spatial-os session as a plain window on your desktop, with Monado
# running the simulated HMD. No VM, no image; iteration cost = process relaunch.
#
#   nix run .#dev-session               # nested session window + simulated Monado
#   nix run .#dev-session -- --client   # + xrgears rendering against the runtime
#   nix run .#dev-session -- --rotate   # canned head motion in the simulated HMD
#
# The nested compositor is sway until zxr's M1 lands; swap COMPOSITOR_CMD then.
{ lib
, writeShellApplication
, writeText
, monado
, sway
, foot
, xrgears
, coreutils
, gnugrep
}:
let
  swayConfig = writeText "dev-session-sway.cfg" ''
    # spatial-os dev-session (nested). Alt is the modifier: the host WM usually
    # owns Super, and Alt chords pass into the nested window reliably.
    set $mod Mod1
    xwayland disable
    output * bg #1a1c2c solid_color
    bindsym $mod+Return exec ${foot}/bin/foot
    bindsym $mod+Shift+q kill
    bindsym $mod+Shift+e exit
    default_border pixel 2
    exec ${foot}/bin/foot
  '';
in
writeShellApplication {
  name = "dev-session";
  runtimeInputs = [ monado sway foot xrgears coreutils gnugrep ];
  text = ''
    usage() {
      cat <<USAGE
    dev-session: spatial-os rung-1 dev loop (nested session + simulated-HMD Monado)

      --client       also launch xrgears inside the session (OpenXR smoke)
      --rotate       simulated HMD follows a canned rotation (SIMULATED_ROTATE)
      --controllers  add simulated left/right controllers
      --no-monado    session only, no XR runtime
      --verbose      debug logging from Monado
      --help         this text
    USAGE
    }

    client=0 rotate=0 controllers=0 monado_on=1 verbose=0
    for a in "$@"; do case "$a" in
      --client) client=1 ;;
      --rotate) rotate=1 ;;
      --controllers) controllers=1 ;;
      --no-monado) monado_on=0 ;;
      --verbose) verbose=1 ;;
      --help) usage; exit 0 ;;
      *) echo "dev-session: unknown flag $a" >&2; usage; exit 1 ;;
    esac; done

    # Preflight: we nest inside an existing graphical session.
    if [ -z "''${WAYLAND_DISPLAY:-}" ] && [ -z "''${DISPLAY:-}" ]; then
      echo "dev-session: no WAYLAND_DISPLAY or DISPLAY — run this from inside a desktop session." >&2
      exit 1
    fi
    if [ ! -e /dev/dri ]; then
      echo "dev-session: /dev/dri missing — no GPU render node available." >&2
      exit 1
    fi

    pids=()
    cleanup() {
      for p in "''${pids[@]:-}"; do kill "$p" 2>/dev/null || true; done
      wait 2>/dev/null || true
    }
    trap cleanup EXIT INT TERM

    export XR_RUNTIME_JSON=${monado}/share/openxr/1/openxr_monado.json

    if [ "$monado_on" = 1 ]; then
      sock="''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/monado_comp_ipc"
      if [ -S "$sock" ]; then
        echo "dev-session: a Monado socket already exists at $sock — reusing that instance." >&2
      else
        export SIMULATED_ENABLE=true
        # We manage lifetime; monado's stdin-watching mainloop must not (it
        # epoll-fails on a non-terminal stdin — ipc_server_process.c).
        export XRT_NO_STDIN=true
        [ "$rotate" = 1 ] && export SIMULATED_ROTATE=true
        if [ "$controllers" = 1 ]; then
          export SIMULATED_LEFT=simple SIMULATED_RIGHT=simple
        fi
        if [ "$verbose" = 1 ]; then export XRT_LOG=debug SIMULATED_LOG=debug; fi
        echo "[monado] starting monado-service (simulated HMD)"
        monado-service > >(sed 's/^/[monado] /') 2>&1 &
        pids+=($!)
        # Give the service a moment to create its socket before clients race it.
        for _ in $(seq 1 50); do [ -S "$sock" ] && break; sleep 0.1; done
        [ -S "$sock" ] || { echo "[monado] service did not come up" >&2; exit 1; }
      fi
    fi

    if [ "$client" = 1 ]; then
      ( sleep 2; echo "[xrgears] starting"; exec xrgears ) > >(sed 's/^/[xrgears] /') 2>&1 &
      pids+=($!)
    fi

    echo "[session] starting nested compositor (sway; Alt+Return = terminal, Alt+Shift+E = quit)"
    COMPOSITOR_CMD=(sway --config ${swayConfig})
    unset SWAYSOCK
    "''${COMPOSITOR_CMD[@]}"
  '';
}
