# dev-session — rung 1 of the development ladder (composition doc §7.1 / ADR 0006
# dev mode): the Mura session as a plain window on your desktop, with Monado
# running the simulated HMD. No VM, no image; iteration cost = process relaunch.
#
#   nix run .#dev-session               # nested session window + simulated Monado
#   nix run .#dev-session -- --client   # + xrgears rendering against the runtime
#   nix run .#dev-session -- --rotate   # canned head motion in the simulated HMD
#   nix run .#dev-session -- --zxr      # zxr (R0) as the session against Monado; foot inside
#
# The nested compositor is sway until zxr's M1 lands; `--zxr` runs zxr in that slot (R0
# bring-up, specs/zxr-core.md §12), with Monado's mirror window as the only view of it.
{ lib
, writeShellApplication
, writeText
, monado
, sway
, foot
, xrgears
, xterm
, coreutils
, gnugrep
, procps
, mura
}:
let
  swayConfig = writeText "dev-session-sway.cfg" ''
    # Mura dev-session (nested). Alt is the modifier: the host WM usually
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
  runtimeInputs = [ monado sway foot xrgears xterm coreutils gnugrep procps mura.zxr ];
  text = ''
    usage() {
      cat <<USAGE
    dev-session: Mura rung-1 dev loop (nested session + simulated-HMD Monado)

      --client       also launch xrgears inside the session (OpenXR smoke);
                     implies --mirror so you can see the XR view
      --zxr          run zxr (R0) as the session instead of sway: foot spawned inside,
                     Monado's mirror window shows the composited view; implies --mirror.
                     Extra zxr flags go after "--" (e.g. -- --frames 600 --journal /tmp/j)
      --x11          with --zxr: also start xwayland-satellite on :7 and spawn xterm (gate 4)
      --mirror       show Monado's XR output window (black until a client renders)
      --no-mirror    force the windowless null compositor even with --client
      --rotate       simulated HMD follows a canned rotation (SIMULATED_ROTATE)
      --controllers  add simulated left/right controllers
      --no-monado    session only, no XR runtime
      --verbose      debug logging from Monado
      --help         this text
    USAGE
    }

    client=0 rotate=0 controllers=0 monado_on=1 verbose=0 mirror=auto zxr=0 x11=0
    zxr_args=()
    while [ $# -gt 0 ]; do a=$1; shift; case "$a" in
      --client) client=1 ;;
      --zxr) zxr=1 ;;
      --x11) x11=1 ;;
      --mirror) mirror=1 ;;
      --no-mirror) mirror=0 ;;
      --rotate) rotate=1 ;;
      --controllers) controllers=1 ;;
      --no-monado) monado_on=0 ;;
      --verbose) verbose=1 ;;
      --help) usage; exit 0 ;;
      --) zxr_args=("$@"); break ;;
      *) echo "dev-session: unknown flag $a" >&2; usage; exit 1 ;;
    esac; done
    [ "$zxr" = 1 ] && [ "$mirror" = auto ] && mirror=1
    [ "$mirror" = auto ] && mirror=$client

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
      if [ -S "$sock" ] && ! pgrep -x monado-service >/dev/null; then
        echo "dev-session: removing stale Monado socket (no live service) at $sock" >&2
        rm -f "$sock"
      fi
      if [ -S "$sock" ]; then
        echo "dev-session: a live Monado is already running at $sock — reusing it." >&2
      else
        export SIMULATED_ENABLE=true
        # We manage lifetime; monado's stdin-watching mainloop must not (it
        # epoll-fails on a non-terminal stdin — ipc_server_process.c).
        export XRT_NO_STDIN=true
        if [ "$mirror" = 0 ]; then
          # Windowless null compositor: no black "XR output" window when nothing
          # renders (target_instance.c: XRT_COMPOSITOR_NULL).
          export XRT_COMPOSITOR_NULL=true
          echo "[monado] mirror window off (null compositor); use --mirror to see XR output"
        else
          echo "[monado] mirror window on: the extra window shows the composited XR view"
        fi
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

    if [ "$zxr" = 1 ]; then
      # zxr is an OpenXR client: it needs no host window; Monado's mirror is the view.
      # The control socket is announced in its log (zxr-<pid>.sock under XDG_RUNTIME_DIR).
      xw=()
      [ "$x11" = 1 ] && xw=(--xwayland :7 --spawn "xterm -fa Monospace -fs 14")
      echo "[session] starting zxr (R0) with foot inside; SIGUSR1 dumps the frame journal"
      COMPOSITOR_CMD=(zxr --spawn foot "''${xw[@]}" "''${zxr_args[@]}")
      "''${COMPOSITOR_CMD[@]}"
      exit $?
    fi

    echo "[session] starting nested compositor (sway; Alt+Return = terminal, Alt+Shift+E = quit)"
    COMPOSITOR_CMD=(sway --config ${swayConfig})
    unset SWAYSOCK
    "''${COMPOSITOR_CMD[@]}"
  '';
}
