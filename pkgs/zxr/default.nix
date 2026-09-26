# pkgs/zxr — Mura's compositor, R0 bring-up (specs/zxr-core.md). Rust; smithay (Wayland frontend
# only, no GL/pixman renderers, no winit, no in-process Xwayland; libinput + libseat + libei
# intake for the input module, spatial-input §1a), openxrs on the runtime-created
# Vulkan device (XR_KHR_vulkan_enable2), ash for the renderer. Shaders compile at build time
# with glslc; the OpenXR loader and the Xwayland satellite are baked in by path so the binary
# needs no environment beyond a runtime's XR_RUNTIME_JSON. Budget fence (spec §12): binary
# ≤ 40 MB, RSS ≤ 60 MB nested, ≤ 4 threads — measured by tests/closure.nix and the R0 gates.
{ lib
, rustPlatform
, pkg-config
, shaderc
, libxkbcommon
, libinput
, seatd
, udev
, openxr-loader
, vulkan-loader
, xwayland-satellite
}:
rustPlatform.buildRustPackage {
  pname = "zxr";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock = {
    lockFile = ./Cargo.lock;
    outputHashes = {
      "smithay-0.7.0" = "sha256-DUSciVTN5Ds2AYZVaGmMu7DBINRxu3CIdUCT4QCplTY=";
    };
  };
  nativeBuildInputs = [ pkg-config shaderc ];
  buildInputs = [ libxkbcommon libinput seatd udev ];
  GLSLC = "${shaderc.bin}/bin/glslc";
  MURA_OPENXR_LOADER = "${openxr-loader}/lib/libopenxr_loader.so.1";
  MURA_XWAYLAND_SATELLITE = "${xwayland-satellite}/bin/xwayland-satellite";
  # ash dlopens libvulkan.so.1; give the binary an rpath rather than a wrapper (the RSS and
  # thread numbers in the gates are of the bare process).
  postFixup = ''
    patchelf --add-rpath ${vulkan-loader}/lib $out/bin/zxr
  '';
  doCheck = false; # no unit tests yet; the R0 gates are the tests (docs/research/61)
  meta = {
    description = "Mura's XR compositor: one OpenXR client of Monado, one Wayland compositor";
    license = lib.licenses.gpl3Plus;
    mainProgram = "zxr";
    platforms = lib.platforms.linux;
  };
}
