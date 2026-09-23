# Mura package overlay.
#
# XR components come from nixpkgs-xr (pulled as a flake input, per ADR 0005) rather
# than being repackaged here. This overlay is for Mura-specific packages:
# per-device Monado driver builds (monado-rev + patch series), kernels, and tooling.
# Empty in the scaffold beyond a marker attribute.
final: prev: {
  mura = (prev.mura or { }) // {
    # Marker so `pkgs.mura ? scaffold` is a cheap "overlay applied" check.
    scaffold = true;
  };
}
