# pkgs/mura-perception-intake — the perception→compositor intake (specs/perception-intake.md):
# the protocol library the real producers (passthrough, hand cutout) and zxr's intake link, and
# its §8 conformance harness — `intake-fake-producer` + `intake-test-consumer`, test-only, the
# mura-authd-harness shape — driven by tests/vm/perception-intake.nix. libc only: fixed-layout
# wire, raw DRM syncobj + udmabuf ioctls (no libdrm, no serde). Unit tests run in the build;
# the kernel-facing checks need /dev/dri and /dev/udmabuf and run in the VM.
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "mura-perception-intake";
  version = "0.1.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;
  doCheck = true;
  meta = {
    description = "Mura perception-layer intake: protocol library + §8 conformance harness";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
  };
}
