# pkgs/mura-osk — the on-screen keyboard (shell-plane.md §3.2; research/75 §3.2). Rust; Slint on
# Mura's sctk platform (pkgs/mura-greeter/platform, feature `input-method`: zwp_input_method_v2 +
# zwp_virtual_keyboard_v1 as a client, the keymap from xkbcommon over a memfd) with the AccessKit
# bridge, so every key is an AT-SPI button. squeekboard's shape: layer `top`, bottom|left|right,
# namespace `osk`, exclusive zone = height, `commit_string` typing, a virtual-keyboard Backspace,
# the digit pad by content purpose, `sm.puri.OSK0` on the session bus when there is one. zxr's
# child in every mode (`--osk`). The source is the two crates by path: this one and the platform
# beside the greeter; the closure carries no image decoder (build.rs).
# Budget: one process, one thread (+ zbus's when a bus exists); hidden = unmapped.
{ lib
, rustPlatform
, pkg-config
, fontconfig
, freetype
, libxkbcommon
, wayland
}:
rustPlatform.buildRustPackage {
  pname = "mura-osk";
  version = "0.1.0";
  # this crate plus the greeter tree it depends on by path (the platform and accesskit crates
  # inherit their dependency versions from the greeter's workspace manifest)
  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      let rel = lib.removePrefix (toString ../. + "/") (toString path); in
      lib.cleanSourceFilter path type && (rel == "mura-osk" || lib.hasPrefix "mura-osk/" rel || rel == "mura-greeter" || lib.hasPrefix "mura-greeter/" rel);
  };
  sourceRoot = "source/mura-osk";
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ fontconfig freetype libxkbcommon wayland ];
  meta = {
    description = "Mura's on-screen keyboard: a layer-shell input-method client on the Slint sctk platform, zxr's child in every mode";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-osk";
    platforms = lib.platforms.linux;
  };
}
