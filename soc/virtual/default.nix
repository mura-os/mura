# "virtual" SoC: the x86_64 VM smoke target's stand-in for a real Qualcomm SoC.
# Real SoC modules (soc/msm8998, soc/sm8250, soc/sm8550, soc/sm8650) will provide
# the shared kernel base, firmware search paths, and DSP/sensor userspace stack.
{ lib, ... }:
{
  mura.hardware.soc = lib.mkDefault "virtual";
}
