{ lib, writeShellApplication, nix }:
writeShellApplication {
  name = "frame-build";
  runtimeInputs = [ nix ];
  text = ''
    target="''${1:-.#packages.aarch64-linux.frame-image}"
    if [ "$#" -gt 0 ]; then shift; fi

    # Repo-local native-aarch64 build path (ADR 0004; research/33 §10).  This deliberately
    # needs no /etc/nix or root SSH configuration: OpenSSH uses the invoking user's key and
    # known_hosts.  nixbuild.net schedules internally; 16 client jobs is the proven ceiling
    # (100 caused SSH drops during the first Frame proof).
    export NIX_SSHOPTS="''${NIX_SSHOPTS:--o BatchMode=yes}"
    exec nix build "$target" \
      --builders "ssh://eu.nixbuild.net aarch64-linux - 16 1 benchmark,big-parallel,kvm,nixos-test" \
      --builders-use-substitutes \
      --max-jobs 0 \
      "$@"
  '';
  meta = {
    description = "Build Mura aarch64 artifacts on nixbuild.net without host configuration";
    license = lib.licenses.gpl3Plus;
    mainProgram = "frame-build";
    platforms = lib.platforms.linux;
  };
}
