# pkgs/mura-session — the session wrapper greetd execs (specs/session-bootstrap.md §4, D4 rev 3).
# Rust, `libc` only. Implements uwsm's verified mechanism over STATIC user units
# (modules/os/session.nix) — no interpreter, no login-time unit generation, no daemon-reload.
# The systemd and D-Bus tool paths are baked at build time so the wrapper never depends on
# PATH in the greetd session.
{ lib, rustPlatform, systemd, dbus }:
rustPlatform.buildRustPackage {
  pname = "mura-session";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  env = {
    MURA_SYSTEMCTL = "${systemd}/bin/systemctl";
    MURA_DBUS_UPDATE_ENV = "${dbus}/bin/dbus-update-activation-environment";
  };
  meta = {
    description = "Mura session wrapper (greetd → user manager → compositor unit), replaces uwsm";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-session";
    platforms = lib.platforms.linux;
  };
}
