# pkgs/mura-setup — the setup program's system instance (first-run-onboarding.md §5.1).
# D3 STUB: the captive-portal launcher, the phone-OS probe redirects, and POST /finish; bound to
# the gadget and hotspot addresses only (IP_FREEBIND), never the LAN. The setup web app itself
# is its own rung and replaces the page, not the unit shape. Rust (no interpreter on the device).
# `mura-setup --recovery` serves the recovery page in stage 1 and execs mura-recovery's actions
# (specs/recovery-menu.md §7); the path is baked so stage 1 needs no PATH.
{ lib, rustPlatform, mura }:
rustPlatform.buildRustPackage {
  pname = "mura-setup";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  env.MURA_RECOVERY_BIN = "${mura.recovery}/bin/mura-recovery";
  meta = {
    description = "Mura setup program (system instance) — D3 stub: launcher + captive-portal probes on the trusted addresses";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-setup";
    platforms = lib.platforms.linux;
  };
}
