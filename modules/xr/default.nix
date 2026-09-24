# XR runtime and session wiring.
#
# Monado as the system OpenXR runtime, out-of-process, socket-activated, with
# /etc/xdg/openxr/1/active_runtime.json declared here — never symlink-flipped at
# runtime (docs/architecture/device-contract.md §xr, docs/research/05-xr-userspace.md
# §9 item 1). The services.monado module is upstream nixpkgs'; this module drives it
# from the mura.* contract and layers per-device config. (The pre-D0 "unavailable in
# this nixpkgs" stub is gone: the pinned nixpkgs provides the module.)
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
