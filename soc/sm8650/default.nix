# Qualcomm SM8650 (Snapdragon 8 Gen 3) — Steam Frame (deckard) SoC.
#
# Facts: docs/research/07-device-landscape.md §Steam Frame,
# docs/research/33-steam-frame-donor.md (production kernel 6.18 LTS, UFS storage,
# mainline SM8650 support is broad). The device-kernel path (Valve's
# linux-618-deckard) is a follow-up gated on its source publication (doc 33 §4);
# until then devices on this SoC build the nixpkgs kernel, which is sufficient
# for VM proofs and early bring-up.
{ lib, ... }:
{
  mura.hardware.soc = lib.mkDefault "sm8650";
}
