# pkgs/mura-preflight — the XR preflight probe (implementation-path §3a-bis; modules/os/health.nix).
# Rust (the D-track language ruling: no interpreter on the boot path). Reads one JSON config the
# module writes from the contract; writes /run/mura/preflight.json; exits 0 / 1 (soft) / 2 (hard).
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mura-preflight";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  meta = {
    description = "Mura XR preflight: persist, calibration, display, Vulkan, tracking, Monado, input floor";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-preflight";
    platforms = lib.platforms.linux;
  };
}
