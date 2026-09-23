# D2 spike: attempt to build wxrc (2021, wlroots 0.8) against modern wlroots 0.19.3,
# to mechanically enumerate the wlroots API breaks a port must cross (ADR 0006).
#
# This is a SPIKE, not part of the product build. It is impure (reads the git-ignored
# references/wxrc clone beside this file) and deliberately never fails the build: it
# tees meson/ninja output to $out so the error enumeration is captured even when the
# compile fails (which is the expected, informative outcome per docs/research/09 §4).
#
# Run:  nix build --impure -f spikes/wxrc-modern-probe.nix -o spikes/result-d2
# Then: cat spikes/result-d2/ninja.log
{ system ? "x86_64-linux" }:
let
  flake = builtins.getFlake (toString ../.);
  pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
  wxrcSrc = ../references/wxrc;
in
pkgs.stdenv.mkDerivation {
  name = "wxrc-modern-probe";
  # Copy the wxrc worktree (impurely) so we can patch meson.build in place.
  src = builtins.path { path = wxrcSrc; name = "wxrc-src"; };

  nativeBuildInputs = with pkgs; [ meson ninja pkg-config wayland-scanner ];
  buildInputs = with pkgs; [
    wlroots_0_19
    wayland
    wayland-protocols
    cglm
    openxr-loader
    libGL
    libgbm
    libdrm
    libxkbcommon
    pixman
  ];

  # We WANT to see the errors, not fail the derivation.
  dontUseMesonConfigure = true;
  configurePhase = "true";

  buildPhase = ''
    mkdir -p "$out"
    echo "wlroots (probe): $(pkg-config --modversion wlroots-0.19 2>&1 || echo '?')" | tee "$out/versions.txt"

    # Break #0 (the version constraint itself): wxrc's meson.build pins
    # wlroots '>=0.8.1','<0.9.0'. Relax it just enough to reach the compiler,
    # and point at the 0.19 pkg-config name.
    sed -i \
      -e "s/wlroots_version = \[.*\]/wlroots_version = ['>=0.19.0','<0.20.0']/" \
      -e "s/dependency('wlroots'/dependency('wlroots-0.19'/g" \
      meson.build
    echo "--- patched wlroots dependency stanza ---" | tee -a "$out/versions.txt"
    grep -n wlroots meson.build | tee -a "$out/versions.txt" || true

    echo "===== meson setup =====" | tee "$out/meson.log"
    meson setup build >>"$out/meson.log" 2>&1 || echo "[meson setup exit $?]" >>"$out/meson.log"

    echo "===== ninja =====" | tee "$out/ninja.log"
    ninja -C build >>"$out/ninja.log" 2>&1 || echo "[ninja exit $?]" >>"$out/ninja.log"

    # Enumerate + classify the breaks.
    echo "===== error summary =====" | tee "$out/summary.txt"
    {
      echo "error/warning lines in ninja.log:"
      grep -cE 'error:|warning:' "$out/ninja.log" || true
      echo
      echo "top undeclared/unknown symbols (implicit-function-declaration + unknown-type):"
      grep -hoE "(implicit declaration of function|unknown type name|has no member named|too many arguments to function|error: '[A-Za-z_]+' undeclared) '?[A-Za-z0-9_]*'?" "$out/ninja.log" \
        | sort | uniq -c | sort -rn | head -60
      echo
      echo "fatal missing headers:"
      grep -hoE "fatal error: [^:]+: No such file" "$out/ninja.log" | sort | uniq -c | sort -rn
    } >>"$out/summary.txt" 2>&1 || true

    cp meson.build "$out/patched-meson.build"
  '';

  installPhase = "true";
  dontFixup = true;
}
