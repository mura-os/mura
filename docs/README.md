# spatial-os documentation

A Nix-built, NixOS-based, Wayland-based Linux XR distribution targeting many standalone VR headsets,
producing reproducible flashable images from pinned vendor firmware ("donor") inputs.

## Research (`research/`)

Study of the reference multi-device OS projects, done as parallel deep-dives. Each doc follows the
same template (purpose · build architecture · device abstraction · donor handling · kernel · images ·
updates · reproducibility · adopt · reject · open questions) so they compare like-for-like.

- [00-synthesis.md](research/00-synthesis.md) — cross-cutting analysis; the bridge to architecture
- [01-mobile-nixos.md](research/01-mobile-nixos.md) — Mobile NixOS + Tow-Boot
- [02-postmarketos.md](research/02-postmarketos.md) — postmarketOS (pmbootstrap/pmaports) + meta-qcom
- [03-android-compat.md](research/03-android-compat.md) — Halium, libhybris, UBports, droid-hal, Waydroid
- [04-nix-imaging.md](research/04-nix-imaging.md) — robotnix, nixpkgs images, Jovian, apple-silicon, mkosi
- [05-xr-userspace.md](research/05-xr-userspace.md) — Monado, WiVRn, StardustXR, nixpkgs-xr, Envision
- [06-donor-pipeline.md](research/06-donor-pipeline.md) — donor ingestion (brick, SteamOS, robotnix)
- [07-device-landscape.md](research/07-device-landscape.md) — the target headsets
- [08-wxrc.md](research/08-wxrc.md) — the Motorcar→wxrc→wxrd 3D-windowing compositor lineage (philosophy + code)
- [09-wxrc-ecosystem-gap-2026.md](research/09-wxrc-ecosystem-gap-2026.md) — which of wxrc's 2019 ecosystem patches are landed/superseded/still-needed in 2026
- [10-xr-wayland-protocol-comparison.md](research/10-xr-wayland-protocol-comparison.md) — motorcar vs zxr vs zwin vs StardustXR vs WayVR
- [11-display-managers-greeters.md](research/11-display-managers-greeters.md) — greetd/GDM/SDDM/seatd, the greeter, and the seat/DRM handoff
- [12-lock-screens-and-appliance-login.md](research/12-lock-screens-and-appliance-login.md) — session-lock protocol, PAM, appliance autologin, doff/don policy
- [13-passthrough.md](research/13-passthrough.md) — camera passthrough view-correction (Rectus, viewcorrection, NeuralPassthrough, production pattern)
- [14-mobile-stereo-depth.md](research/14-mobile-stereo-depth.md) — mobile stereo depth backends (TC-Stereo, XR-Stereo, LightStereo, teacher-student)
- [15-hand-segmentation-matting.md](research/15-hand-segmentation-matting.md) — egocentric hand cutout (four artifacts, Mercury prior, matting, policy)
- [16-perception-claims-audit.md](research/16-perception-claims-audit.md) — verification of Qualcomm depth paths + 2026 preprint claims
- [17-sharing-capture-stack.md](research/17-sharing-capture-stack.md) — portals, gnome-remote-desktop, wayvnc, obs-vkcapture, libei, PipeWire ground truth
- [18-xr-streaming.md](research/18-xr-streaming.md) — WiVRn protocol deep dive, ALVR, Sunshine/Moonlight, wolf, comp_multi
- [19-wayland-proxying.md](research/19-wayland-proxying.md) — waypipe, wprs, Sommelier, crosvm cross-domain, wayland-proxy-virtwl, Spectrum OS
- [20-slam-stacks-for-xr.md](research/20-slam-stacks-for-xr.md) — SLAM/VIO stacks at code level: the VIT seam, Basalt's marg-data egress, orbslam3-monado, Atlas internals, licenses
- [21-anchors-persistence-openxr.md](research/21-anchors-persistence-openxr.md) — `XR_EXT_spatial_entity` family, the map/local frame split, anchor data model, privacy, Monado gap
- [22-dense-geometry-no-lidar.md](research/22-dense-geometry-no-lidar.md) — planes/TSDF meshing/semantics without LiDAR; per-device depth sources; tri-state confidence
- [23-relocalization-multisession.md](research/23-relocalization-multisession.md) — recognizing mapped rooms: reloc funnels, multi-session merging, map stores, dynamic-object gating
- [28-eye-tracking-stack.md](research/28-eye-tracking-stack.md) — pupil detection lineage, pye3d rotation-center IPD, Monado ET surface, placement + motor policy
- [29-eye-hardware-ipd-per-target.md](research/29-eye-hardware-ipd-per-target.md) — per-target eye cameras, IPD mechanisms, access classes, iris auth, gaze privacy

## Architecture (`architecture/`)

- [overview.md](architecture/overview.md) — layers, boundaries, invariants
- [device-contract.md](architecture/device-contract.md) — the typed `spatial.*` device contract
- [donor-pipeline.md](architecture/donor-pipeline.md) — acquire→identify→parse→extract→qualify
- [images-and-updates.md](architecture/images-and-updates.md) — image families + two-backend updates
- [repo-structure.md](architecture/repo-structure.md) — monorepo layout + patch management
- [zxr-shell-v2-composition.md](architecture/zxr-shell-v2-composition.md) — the XR compositor's renderer-agnostic colour+depth composition model and MVP
- [perception-passthrough-hands.md](architecture/perception-passthrough-hands.md) — passthrough view-correction + hand cutout as compositor layers
- [spatial-sharing.md](architecture/spatial-sharing.md) — the five sharing modes (spectate / 2D window / per-observer 3D / share-the-app proxying / workspace join)
- [spatial-mapping.md](architecture/spatial-mapping.md) — anchors, persistence, relocalization, planes/mesh/boundary (Tier 4 world understanding)
- [adr/](architecture/adr/) — decision records:
  - [0001](architecture/adr/0001-monorepo-vs-subprojects.md) monorepo vs subprojects
  - [0002](architecture/adr/0002-nixos-vs-nix-built-userspace.md) NixOS vs Nix-built userspace
  - [0003](architecture/adr/0003-android-compat-scope.md) Android-compat scope
  - [0004](architecture/adr/0004-cross-vs-native-builds.md) cross vs native builds
  - [0005](architecture/adr/0005-flake-layout-and-outputs.md) flake layout and outputs
  - [0006](architecture/adr/0006-compositor-strategy.md) XR compositor strategy (revive zxr as zxr-shell-v2)
  - [0007](architecture/adr/0007-session-greeter-lock.md) session/greeter/lock model (appliance autologin + greetd; lock as compositor state)
  - [0008](architecture/adr/0008-perception-services-placement.md) perception services placement (passthrough + hand cutout, Monado-side)
  - [0009](architecture/adr/0009-spatial-mapping-architecture.md) spatial mapping architecture (layered Basalt VIO + separate mapping/anchor service, not single-SLAM)
  - [0011](architecture/adr/0011-eye-tracking-ipd.md) eye tracking and IPD (Monado-side eye-frame service, rotation-center IPD, event-gated motors)
- [REVIEW.md](architecture/REVIEW.md) — cross-model red-team review of the base architecture
- [design-backlog.md](architecture/design-backlog.md) — disposition of the base review (fixed now vs.
  deferred to the Lynx spike / pre-release design)
- [REVIEW-perception.md](architecture/REVIEW-perception.md) — red-team review of the perception design
- [perception-design-backlog.md](architecture/perception-design-backlog.md) — disposition of the
  perception review (fixed now vs. the P-1 BSP kill-gate vs. pre-release)
- [REVIEW-mapping.md](architecture/REVIEW-mapping.md) — red-team review of the spatial-mapping design
- [mapping-design-backlog.md](architecture/mapping-design-backlog.md) — disposition of the mapping
  review (fixed now vs. the M0 foundations gate vs. design-before-milestone)

## Reference clones (`../references/`)

Git-ignored study clones, reproducible from `references/clone.sh` + the pinned
`references/MANIFEST.json`.
