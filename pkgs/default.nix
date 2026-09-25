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
    # The lock-path PAM helper + its conformance harness (specs/session-auth.md; D5).
    authd = final.callPackage ./mura-authd { };
    # TEST-ONLY PAM module for the harness (session-auth §6 items 2 and 7); never shipped.
    pamTestModule = final.callPackage ./mura-authd/test { };
    # The XR preflight probe (implementation-path §3a-bis; modules/os/health.nix; D6).
    preflight = final.callPackage ./mura-preflight { };
    # The setup program's system instance — D3 stub (first-run §5.1; modules/os/oob.nix).
    setup = final.callPackage ./mura-setup { };
  };
}
