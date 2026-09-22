# Pangolin 0.9.3 — build dependency for the ORB-SLAM3 spike (not in nixpkgs).
# Minimal headless-friendly build: no examples/tools/tests, X11 windowing available
# for interactive use but the spike runs viewer-less.
{ pkgs }:
pkgs.stdenv.mkDerivation {
  pname = "pangolin";
  version = "0.9.3";
  src = pkgs.fetchurl {
    url = "https://github.com/stevenlovegrove/Pangolin/archive/refs/tags/v0.9.3.tar.gz";
    hash = "sha256-zMH8w9gSK/OwTTfQXxSZDOuZ7RafE3gBwQEKh8WR6Zs=";
  };
  nativeBuildInputs = with pkgs; [ cmake ninja pkg-config ];
  buildInputs = with pkgs; [
    eigen
    libGL
    libGLU
    glew
    xorg.libX11
    xorg.libXext
    libepoxy
  ];
  cmakeFlags = [
    "-DBUILD_EXAMPLES=OFF"
    "-DBUILD_TOOLS=OFF"
    "-DBUILD_TESTS=OFF"
    "-DBUILD_PANGOLIN_PYTHON=OFF"
    "-DBUILD_PANGOLIN_FFMPEG=OFF"
    "-DBUILD_PANGOLIN_LIBREALSENSE2=OFF"
    "-DBUILD_PANGOLIN_OPENNI2=OFF"
  ];
}
