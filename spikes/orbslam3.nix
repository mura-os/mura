# D1/D2 spike: build ORB-SLAM3 (upstream, references/orbslam3) with Nix.
#
# SPIKE: impure (reads the git-ignored reference clone via builtins.path), mirrors
# upstream build.sh: Thirdparty DBoW2 + g2o + Sophus first, then the main library and
# the EuRoC examples. Installs the stereo-inertial EuRoC runner, the ORB vocabulary,
# and the EuRoC settings so the persistence spike (Atlas save/load via
# System.SaveAtlasToFile / LoadAtlasFromFile YAML keys, src/Settings.cc:475) can run.
#
# Run:  nix build --impure -f spikes/orbslam3.nix -o spikes/result-orbslam3
{ system ? "x86_64-linux" }:
let
  flake = builtins.getFlake (toString ../.);
  pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
  pangolin = import ./pangolin.nix { inherit pkgs; };
  src = builtins.path {
    path = /run/media/j/tinystore/experiments/spatial-os/references/orbslam3;
    name = "orbslam3-src";
  };
in
pkgs.stdenv.mkDerivation {
  pname = "orb-slam3";
  version = "1.0-spike";
  inherit src;

  nativeBuildInputs = with pkgs; [ cmake pkg-config ];
  buildInputs = with pkgs; [
    opencv
    eigen
    pangolin
    boost
    openssl # libcrypto for map file checksums (System.cc)
    glew
    libGL
  ];

  # ORB-SLAM3 v1.0 is C++14-era; newer GCC needs a couple of accommodations.
  env.NIX_CFLAGS_COMPILE = "-Wno-error -Wno-deprecated-declarations";

  dontUseCmakeConfigure = true;

  buildPhase = ''
    runHook preBuild
    chmod -R u+w .

    build_sub() {
      (cd "$1" && cmake -B build -DCMAKE_BUILD_TYPE=Release ''${2:-} && cmake --build build -j$NIX_BUILD_CORES)
    }

    echo ">>> Thirdparty/DBoW2"
    build_sub Thirdparty/DBoW2
    echo ">>> Thirdparty/g2o"
    build_sub Thirdparty/g2o
    echo ">>> Thirdparty/Sophus"
    build_sub Thirdparty/Sophus "-DBUILD_TESTS=OFF -DBUILD_SOPHUS_TESTS=OFF -DBUILD_SOPHUS_EXAMPLES=OFF"

    echo ">>> Vocabulary"
    (cd Vocabulary && tar -xf ORBvoc.txt.tar.gz)

    echo ">>> ORB_SLAM3"
    cmake -B build -DCMAKE_BUILD_TYPE=Release
    cmake --build build -j$NIX_BUILD_CORES
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin $out/lib $out/share/orbslam3
    cp lib/libORB_SLAM3.so $out/lib/ 2>/dev/null || cp build/libORB_SLAM3.so $out/lib/ || true
    cp Thirdparty/DBoW2/lib/*.so Thirdparty/g2o/lib/*.so $out/lib/
    # The euroc example binaries land next to their sources per the CMakeLists.
    for b in Examples/Stereo-Inertial/stereo_inertial_euroc Examples/Stereo/stereo_euroc Examples/Monocular/mono_euroc; do
      [ -f "$b" ] && cp "$b" $out/bin/
    done
    cp Vocabulary/ORBvoc.txt $out/share/orbslam3/
    cp Examples/Stereo-Inertial/EuRoC.yaml $out/share/orbslam3/EuRoC-Stereo-Inertial.yaml
    cp Examples/Stereo/EuRoC.yaml $out/share/orbslam3/EuRoC-Stereo.yaml 2>/dev/null || true
    cp Examples/Stereo-Inertial/EuRoC_TimeStamps/*.txt $out/share/orbslam3/ 2>/dev/null || true
    # Binaries link libs by build path; fix rpaths.
    for f in $out/bin/*; do
      patchelf --add-rpath $out/lib "$f" || true
    done
    runHook postInstall
  '';
}
