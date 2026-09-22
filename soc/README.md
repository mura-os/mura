# soc/ — shared per-SoC integration

One module per SoC family, providing what a device shouldn't have to restate: the shared kernel base,
firmware search paths, the DSP/sensor userspace stack, and default kconfig contract fragments. This
mirrors postmarketOS `soc-qcom-<family>` and meta-qcom `qcom-<soc>.inc`
(docs/research/02-postmarketos.md §3.5, §3.7).

Planned:

- `msm8998/` — Snapdragon 835 (Oculus Quest 1). Mature linux-msm mainline.
- `sm8250/` — Snapdragon XR2 Gen 1 / "kona" (Lynx R1). Mature linux-msm mainline; existing pmaports port.
- `sm8550/` — Snapdragon XR2+ Gen 2 class (Samsung Galaxy XR, Play For Dream MR). Newer GKI/mainline.
- `sm8650/` — Snapdragon 8 Gen 3 (Valve Steam Frame). Modern upstream.
- `virtual/` — VM smoke-target stand-in (implemented).

Per Mobile NixOS guidance, implement real devices before extracting shared families/SoC commonality
rather than designing a deep hierarchy up front (docs/research/01-mobile-nixos.md §9 item 1).
