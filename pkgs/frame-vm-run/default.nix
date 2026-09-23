# QEMU runner for the Steam Frame uefi-rauc disk image (aarch64 full-system
# emulation on the x86_64 dev host; no binfmt needed).
#
# Usage:
#   nix run .#frame-vm-run -- <path-to-image.raw> [extra qemu args]
# The image is copied to a writable sparse working copy first (never boots the
# store artifact in place). Serial console on stdio; ssh forwarded to :2222.
{ lib, writeShellApplication, qemu, zstd, coreutils }:
writeShellApplication {
  name = "frame-vm-run";
  runtimeInputs = [ qemu zstd coreutils ];
  text = ''
    img="''${1:?usage: frame-vm-run <image.raw[.zst]> [qemu args...]}"; shift || true
    work="''${FRAME_VM_DIR:-$PWD/.frame-vm}"
    mkdir -p "$work"
    case "$img" in
      *.zst) zstd -d --sparse -f "$img" -o "$work/disk.raw" ;;
      *)     cp --sparse=always -f "$img" "$work/disk.raw" ;;
    esac
    chmod +w "$work/disk.raw"

    fw=${qemu}/share/qemu/edk2-aarch64-code.fd
    varstore="$work/efivars.fd"
    [ -f "$varstore" ] || { truncate -s 64M "$varstore"; }

    exec qemu-system-aarch64 \
      -machine virt -cpu cortex-a72 -smp 4 -m 4096 \
      -drive if=pflash,format=raw,readonly=on,file="$fw" \
      -drive if=pflash,format=raw,file="$varstore" \
      -drive if=none,id=disk,format=raw,file="$work/disk.raw" \
      -device virtio-blk-pci,drive=disk \
      -device virtio-net-pci,netdev=net0 \
      -netdev user,id=net0,hostfwd=tcp::2222-:22 \
      -device virtio-gpu-pci -display none \
      -serial mon:stdio \
      "$@"
  '';
}
