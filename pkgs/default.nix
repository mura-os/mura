# spatial-os package overlay.
#
# XR components come from nixpkgs-xr (pulled as a flake input, per ADR 0005) rather
# than being repackaged here. This overlay is for spatial-os-specific packages:
# per-device Monado driver builds (monado-rev + patch series), kernels, and tooling.
# Empty in the scaffold beyond a marker attribute.
final: prev: {
  spatial = (prev.spatial or { }) // {
    # Marker so `pkgs.spatial ? scaffold` is a cheap "overlay applied" check.
    scaffold = true;
  };
}
