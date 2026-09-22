# Image-variant builders.
#
# Per docs/architecture/images-and-updates.md these are deferred modules injected into
# nixpkgs `image.modules`, each defining system.build.image with the standard
# image.baseName/filePath. The scaffold provides the dev-vm variant (a NixOS VM, used
# by devices/virtual-headset) and declares the intended real variants as stubs.
#
# Real variants to implement:
#   - android-bootimg : Nix-native wrapper over current mkbootimg/unpack_bootimg/avbtool/lpmake
#                        (NOT the 2020 mkbootimg fork). Header v0-v4 + vendor_boot + init_boot + AVB.
#   - uefi-rauc        : systemd-repart GPT/UEFI disk + RAUC+casync A/B bundle (Steam Frame).
#   - installer-usb    : reference-free flashing bundle (imgs + manifest + bare-tool flash script).
{ lib }:
{
  # A device's system.build.vm is the dev-vm "image". The flake exposes it directly;
  # once nixpkgs image.modules wiring lands, this becomes an image.modules entry.
  devVm = nixosConfig: nixosConfig.config.system.build.vm;
}
