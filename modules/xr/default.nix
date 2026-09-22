# XR runtime and session wiring.
#
# Monado as the system OpenXR runtime, out-of-process, socket-activated, with
# /etc/xdg/openxr/1/active_runtime.json declared here — never symlink-flipped at
# runtime (docs/architecture/device-contract.md §xr, docs/research/05-xr-userspace.md
# §9 item 1). The actual services.monado module lives upstream in nixpkgs; this
# module drives it from the spatial.* contract and layers per-device config.
{ lib, config, options, pkgs, ... }:
let
  cfg = config.spatial.xr;
  # Check option *existence* (not config value) to avoid infinite recursion.
  monadoAvailable = options.services ? monado;
in
{
  config = lib.mkMerge [
    (lib.mkIf (cfg.runtime == "monado" && monadoAvailable) {
      services.monado = {
        enable = true;
        defaultRuntime = true; # materializes /etc/xdg/openxr/1/active_runtime.json
      };
      systemd.user.services.monado.environment = cfg.environment;
    })

    (lib.mkIf (cfg.runtime == "monado" && !monadoAvailable) {
      warnings = [
        "spatial.xr.runtime = monado but services.monado is unavailable in this nixpkgs; XR runtime wiring is stubbed. Wire the runtime package in pkgs/ or bump nixpkgs."
      ];
    })

    (lib.mkIf (cfg.runtime == "wivrn") {
      warnings = [ "spatial.xr.runtime = wivrn: WiVRn server wiring is not yet implemented in the scaffold." ];
    })
  ];
}
