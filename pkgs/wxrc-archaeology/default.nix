# D1 spike: era-pinned "archaeology build" of wxrc (2021, wlroots 0.8, GLES2+EGL, Monado).
#
# GOAL (per the plan): build wxrc as-is against an era-correct nixpkgs and run it in the
# virtual-headset VM with Monado's simulated driver, OR document the precise failure point.
#
# RESULT: the archaeology build is NOT constructible from stock nixpkgs, and this
# derivation records why. wxrc's meson.build requires wlroots '>=0.8.1','<0.9.0' AND a
# Monado OpenXR runtime exposing XR_MNDX_egl_enable. Those two never coexisted in any
# nixpkgs channel:
#
#   nixpkgs channel | wlroots | openxr-loader | monado   | satisfies wxrc?
#   ----------------+---------+---------------+----------+----------------------------
#   nixos-19.09     | 0.7.0   | 1.0.2         | ABSENT   | no (wlroots too OLD; no Monado)
#   nixos-20.09     | 0.11.0  | 1.0.11        | ABSENT   | no (wlroots too new; no Monado)
#   nixos-21.11     | 0.14.1  | 1.0.20        | 21.0.0   | no (wlroots far too new)
#   nixos-23.11     | 0.16.2  | 1.0.31        | 2023-08  | no (wlroots far too new)
#   (current)       | 0.20.2  | 1.1.62        | 25.1.0   | no (see spikes/wxrc-modern-probe.nix / D2)
#
# The wlroots 0.8.x window (mid/late 2019) predates Monado's arrival in nixpkgs (~21.05).
# So wxrc-as-2021 depended on a PATCHED, out-of-tree wlroots 0.8 plus a then-bleeding-edge
# OpenXR/Monado stack simultaneously (docs/research/08 §2.2, docs/research/09) — which is
# exactly the "patch large swaths of the ecosystem" the README warns about, and confirms
# from the packaging side that reviving wxrc is a rewrite, not a port (ADR 0006).
#
# A genuine archaeology build would therefore require vendoring a 2019 patched wlroots 0.8
# source tree + a pinned Monado source build with the (then-unmerged) XR_MNDX_egl_enable
# work — deliberately out of scope for this spike; the finding above is the deliverable.
#
# This derivation emits that version matrix + conclusion as a small runnable report so the
# result is reproducible rather than only prose.
#
# Run:  nix build --impure -f pkgs/wxrc-archaeology/default.nix -o result-d1
{ system ? "x86_64-linux" }:
let
  flake = builtins.getFlake (toString ../../.);
  pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
in
pkgs.runCommand "wxrc-archaeology-report" { } ''
  mkdir -p "$out"
  cat > "$out/FINDING.txt" <<'EOF'
  wxrc archaeology build (D1): NOT CONSTRUCTIBLE FROM STOCK NIXPKGS.

  wxrc/meson.build requires wlroots >=0.8.1,<0.9.0 and a Monado OpenXR runtime with
  XR_MNDX_egl_enable. No nixpkgs channel ever shipped both at once:

    channel      wlroots  openxr-loader  monado
    nixos-19.09  0.7.0    1.0.2          ABSENT
    nixos-20.09  0.11.0   1.0.11         ABSENT
    nixos-21.11  0.14.1   1.0.20         21.0.0
    nixos-23.11  0.16.2   1.0.31         2023-08
    current      0.20.2   1.1.62         25.1.0

  The wlroots 0.8.x window (2019) predates Monado in nixpkgs (~21.05); by the time Monado
  lands, wlroots is >=0.14. wxrc depended on a PATCHED out-of-tree wlroots 0.8 + a
  bleeding-edge OpenXR/Monado stack simultaneously. This confirms (from packaging) the
  docs/research/08 + 09 conclusion and ADR 0006: revive as a rewrite (zxr-shell-v2 on
  modern wlroots/smithay + Vulkan), do not port the wxrc codebase.

  A real archaeology build would need a vendored 2019 patched wlroots 0.8 tree + a pinned
  Monado source build with the then-unmerged XR_MNDX_egl_enable work: out of scope here.
  See spikes/wxrc-modern-probe.nix (D2) for the modern-wlroots break enumeration.
  EOF
  cat "$out/FINDING.txt"
''
