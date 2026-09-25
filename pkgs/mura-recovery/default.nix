# pkgs/mura-recovery — the recovery environment's one program (specs/recovery-menu.md).
# Rust, libc + serde. `action <name>` is the only place the actions live; `panel` drives the menu
# from the HMD's buttons over raw evdev and draws it through plymouth; `shell` is the same menu
# over ssh or the console. The tool paths are baked at build time (stage 1 has no PATH to trust).
{ lib, rustPlatform, systemd, plymouth }:
rustPlatform.buildRustPackage {
  pname = "mura-recovery";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  env = {
    MURA_SYSTEMCTL = "${systemd}/bin/systemctl";
    MURA_REPART = "${systemd}/bin/systemd-repart";
    MURA_UDEVADM = "${systemd}/bin/udevadm";
    MURA_PLYMOUTH = "${plymouth}/bin/plymouth";
  };
  doCheck = true; # the menu state machine's unit tests (specs/recovery-menu.md §3-§4)
  meta = {
    description = "Mura recovery: actions + menu; panel (evdev + plymouth), shell and web frontends";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-recovery";
    platforms = lib.platforms.linux;
  };
}
