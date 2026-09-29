# The Mura spatial-container sample — the conformance client of specs/composition.md §7.3
# (the C-track's gates, implementation-path.md §3): a Godot 4.8 project that opts into
# XR_EXT_spatial_container(_self_rendering), logs every container event as a `SCS <event>`
# line, cycles bounded<->immersive, and runs unchanged on a runtime without the extension
# (the C0 baseline against Monado today). See README.md beside this file.
#
# Godot master (the container pair landed in #123124, 2026-09-08) is not packaged here:
# nixpkgs ships 4.7-stable, and a reproducible `pkgs/godot` is deferred until C1 exists
# (the plan's A4; decider the owner). Until then the editor binary is built once from the
# sibling clone with `nix develop .#godot` (README §1) and passed in as `GODOT`.
{ lib, writeShellApplication, coreutils, gnused }:
writeShellApplication {
  name = "spatial-container-sample";
  runtimeInputs = [ coreutils gnused ];
  text = ''
    project=${./project}
    godot="''${GODOT:-/run/media/j/tinystore/experiments/godot/bin/godot.linuxbsd.editor.x86_64}"
    if [ ! -x "$godot" ]; then
      echo "spatial-container-sample: no Godot at $godot — set GODOT=/path/to/godot.linuxbsd.editor.x86_64 (README §1)" >&2
      exit 66
    fi
    if [ -z "''${XR_RUNTIME_JSON:-}" ] && [ ! -e /etc/xdg/openxr/1/active_runtime.json ] && [ ! -e "''${XDG_CONFIG_HOME:-$HOME/.config}/openxr/1/active_runtime.json" ]; then
      echo "spatial-container-sample: no OpenXR runtime declared (XR_RUNTIME_JSON unset) — run inside \`nix run .#dev-session -- --godot\` or export it" >&2
    fi
    # A Godot project only runs after the editor has imported it: `.godot/imported/` holds
    # the converted .glb/.png and `.godot/global_script_class_cache.cfg` registers the
    # `class_name`s scripts extend (without it main.gd fails with "Could not find base class
    # StartXR" and the window stays black). A game run never imports, so do one headless
    # `--import` pass into a writable copy keyed on the project's store path (the store is
    # read-only); it is skipped on later runs of the same store path.
    work="''${XDG_CACHE_HOME:-$HOME/.cache}/mura/spatial-container-sample/$(basename "$project")"
    if [ ! -d "$work/.godot/imported" ]; then
      rm -rf "$work"
      mkdir -p "$work"
      cp -r --no-preserve=mode "$project"/. "$work"/
      echo "spatial-container-sample: importing project into $work (once per build)" >&2
      "$godot" --headless --path "$work" --import 2>&1 | sed 's/^/[import] /' >&2
      if [ ! -d "$work/.godot/imported" ]; then
        echo "spatial-container-sample: import produced no .godot/imported — see the [import] lines above" >&2
        rm -rf "$work/.godot"
        exit 65
      fi
    fi
    # `--xr-mode on` fails loudly when no runtime answers instead of falling back to a
    # flat window (Godot's default is "default" = try, then continue without XR).
    # Editor builds flush stdout on every print (`application/run/flush_stdout_on_print.debug`);
    # release templates do not, and a harness reads the SCS lines from a pipe — line-buffer
    # regardless so they never arrive late or die unflushed on SIGTERM.
    exec stdbuf -oL -eL "$godot" --path "$work" --xr-mode on "$@"
  '';
  meta = {
    description = "Mura's Godot spatial-container conformance client (specs/composition.md §7.3)";
    platforms = lib.platforms.linux;
    mainProgram = "spatial-container-sample";
  };
}
