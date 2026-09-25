# pkgs/mura-settingsd — the settings daemon + CLI (specs/settings-daemon.md). Rust; libc, serde,
# zbus 5 on the async-io executor (research/58 §11: measured +0.9 MB / +1 MB RSS / 3 threads
# over libc-only; never tokio's multi-thread runtime). Ships the D-Bus activation file for the
# user bus (`Type=dbus` unit in modules/os/settings.nix) and, test-only, `mura-settingsd-liar`
# (settings-schema.md §9 item 8) in the same output, as mura-authd ships its harness.
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mura-settingsd";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  doCheck = true; # resolver, store, migration engine, artifact lookup (15 unit tests)
  postInstall = ''
    mkdir -p $out/share/dbus-1/services
    cat > $out/share/dbus-1/services/org.mura.Settings1.service <<EOF
    [D-BUS Service]
    Name=org.mura.Settings1
    Exec=$out/bin/mura-settingsd
    SystemdService=mura-settingsd.service
    EOF
  '';
  meta = {
    description = "Mura settings daemon (org.mura.Settings1) over the generated schema artifact, and the mura-settings CLI";
    license = lib.licenses.gpl3Plus;
    mainProgram = "mura-settings";
    platforms = lib.platforms.linux;
  };
}
