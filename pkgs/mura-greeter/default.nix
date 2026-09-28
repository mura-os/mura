# pkgs/mura-greeter — the greeter and lock program (specs/session-auth.md rev 6 §5; shell-plane.md
# §3.1; research/78). Rust; Slint on Mura's sctk platform (platform/: wl_shm + the software
# renderer, layer-shell and session-lock roles, text-input-v3) with Slint's AccessKit tree
# translation as a crate beside it (accesskit/: upstream's winit module on accesskit_unix —
# shell-plane §4). No Slint fork: the two crates depend on i-slint-core's internals at the exact
# pinned version. The scene carries no images, so the closure carries no decoder (build.rs).
# greetd is spoken through the greetd_ipc crate; logind through zbus (blocking); PAM never
# (mura-authd is spawned per conversation, its path baked here).
# Budget: one process, resident only in lock mode; measured at gate 9 (spec §12).
{ lib
, rustPlatform
, pkg-config
, fontconfig
, freetype
, libxkbcommon
, wayland
, mura
}:
rustPlatform.buildRustPackage {
  pname = "mura-greeter";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ fontconfig freetype libxkbcommon wayland ];
  # the lock path's helper, by path (session-auth §2: spawned per conversation)
  MURA_AUTHD = "${mura.authd}/bin/mura-authd";
  # only the program ships: mura-fake-authd is the nested gate's stand-in (research/78 §7),
  # built by cargo in development and never installed on an image
  cargoBuildFlags = [ "--bin" "mura-greeter" ];
  # the workspace's unit tests (sessions file parsing, the lock ladder's neighbours)
  cargoTestFlags = [ "--bin" "mura-greeter" ];
  meta = {
    description = "Mura's greeter and lock program: greetd's kiosk child in greeter mode, an ext-session-lock client under a user unit in lock mode";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-greeter";
    platforms = lib.platforms.linux;
  };
}
