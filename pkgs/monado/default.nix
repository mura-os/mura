# Monado from the mura-os/monado fork (ADR 0006 amendment 4 D13).
#
# nixpkgs-xr already packages Monado (build system, features, patches for the nixpkgs
# module); Mura keeps that package and swaps only its source for the fork — the mechanism
# nixpkgs-xr itself uses to track upstream (`pkgs/overrides/monado.nix` there:
# `inherit (final.xrSources.monado) pname version src`). The fork's branch `mura` is upstream
# `main` plus Mura's upstream-shaped series: the spatial-container pair, the controller seam,
# the depth policy, the dmabuf-import swapchain, per-device drivers — the C-track of
# docs/architecture/implementation-path.md §3, one feature branch per upstreamable piece.
#
# `monadoSrc` is the flake input (`inputs.monado`, `flake = false`), so the rev lives in
# flake.lock and `nix flake update monado` is the bump. Per-device driver experiments that are
# not yet on the fork still ride `mura.xr.monado.patches` (lib/contract) on top of this.
#
# Series on `mura` beyond upstream `main` (one branch each, newest last):
#   - `wayland-resize` (2026-09-29, ccae7f3c1): the Wayland window target honours the
#     compositor's configured size, acking a resize with its re-created images (upstream #152;
#     research/78 §9 F26) — what lets tests/vm/scene.nix's picture fill the output.
#
# The version string carries the fork's short rev so `monado-service --version` and the closure
# name say which Monado this is.
monadoSrc: final: prev: {
  monado = prev.monado.overrideAttrs (prevAttrs: {
    src = monadoSrc;
    version = "${prevAttrs.version or "0"}-mura+${monadoSrc.shortRev or "dirty"}";
    passthru = (prevAttrs.passthru or { }) // {
      # For probes and docs: where this Monado came from.
      muraFork = {
        url = "https://github.com/mura-os/monado";
        branch = "mura";
        rev = monadoSrc.rev or null;
      };
    };
  });
}
