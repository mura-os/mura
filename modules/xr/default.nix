# XR runtime and session wiring.
#
# Monado as the system OpenXR runtime, out-of-process, socket-activated, with
# /etc/xdg/openxr/1/active_runtime.json declared here — never symlink-flipped at
# runtime (docs/architecture/device-contract.md §xr, docs/research/05-xr-userspace.md
# §9 item 1). The services.monado module is upstream nixpkgs'; this module drives it
# from the mura.* contract and layers per-device config. (The pre-D0 "unavailable in
# this nixpkgs" stub is gone: the pinned nixpkgs provides the module.)
#
# The Monado the module runs is `pkgs.monado` after Mura's overlay (pkgs/monado/default.nix):
# nixpkgs-xr's package with its `src` swapped for the pinned `mura-os/monado` fork — upstream
# `main` plus Mura's upstream-shaped series, where the C-track lands (ADR 0006 amendment 4 D13:
# the spatial-container pair, the controller seam, the depth policy, the dmabuf-import
# swapchain; specs/composition.md). `services.monado.package` is the override point if a
# profile needs a different build; nothing here is per-device (that is `mura.xr.monado.*`).
{ lib, config, pkgs, ... }:
let
  cfg = config.mura.xr;
in
{
  config = lib.mkMerge [
    (lib.mkIf (cfg.runtime == "monado") {
      services.monado = {
        enable = true;
        defaultRuntime = true; # materializes /etc/xdg/openxr/1/active_runtime.json
      };
      systemd.user.services.monado.environment = cfg.environment;
    })

    (lib.mkIf (cfg.runtime == "wivrn") {
      warnings = [ "mura.xr.runtime = wivrn: WiVRn server wiring is not yet implemented in the scaffold." ];
    })
  ];
}
