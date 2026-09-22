# Per-subsystem adaptation backends (ADR 0003).
#
# Each subsystem independently resolves to native | android-backed | device-specific.
# The scaffold implements the 'native' backends (mainline driver stack, the default
# posture given the device landscape) and leaves android-backed/device-specific as
# explicit, warned stubs so an incomplete port fails loud rather than silently.
{ lib, config, pkgs, ... }:
let
  a = config.spatial.adaptation;

  # Subsystems that currently only have a native implementation in the scaffold.
  stubbed = lib.filterAttrs (_: v: v.backend or "native" != "native")
    {
      inherit (a) display gpu camera sensors audio wifiBt tracking;
    };
in
{
  imports = [ ./android-compat ];

  config = {
    # Native backends: rely on the common graphics/udev stack already enabled in
    # modules/os. SoC modules (soc/<name>) layer firmware search paths and the
    # DSP/sensor userspace on top. Nothing device-specific belongs here.

    warnings = lib.mapAttrsToList
      (name: v: "spatial.adaptation.${name}.backend = ${v.backend}: not yet implemented in the scaffold (only 'native' is wired). Implement it in modules/adaptation before relying on this subsystem.")
      (lib.filterAttrs (_: v: v.backend != "device-specific") stubbed);
  };
}
