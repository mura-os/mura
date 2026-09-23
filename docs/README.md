# spatial-os documentation

Development workflow (the three dev loops, one command each): [README.md §Development](../README.md).

A Nix-built, NixOS-based, Wayland-based Linux XR distribution targeting many standalone VR headsets,
producing reproducible flashable images from pinned vendor firmware ("donor") inputs.

**Grounding rule:** every design grounds its vocabulary in the applicable freedesktop/XDG
specification before inventing terms — and states *which* "XDG" it means (the CDG spec family,
the `xdg_*` Wayland protocol namespace, or xdg-desktop-portal; the three-way trap is
[desktop-environment.md §2](architecture/desktop-environment.md)). Protocol surveys sweep whole
directories, never named lists (doc 30's scope rule).

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
- [24-avatar-representation-enrollment.md](research/24-avatar-representation-enrollment.md) — avatar representation code audit (RGBAvatar/GaussianAvatars/MATCH-GEM/FlexAvatar): controls, memory math, eyes/teeth, enrollment
- [25-avatar-driving-sensing.md](research/25-avatar-driving-sensing.md) — the verified Linux expression/gaze path (WiVRn→Monado FB2), blendshape schema map, per-device sensing matrix, degraded modes
- [26-codec-avatar-route.md](research/26-codec-avatar-route.md) — Ava-256 archaeology (main + PRs 1/7/19), the new-person-into-latent-space problem, v2 hooks + experiment sequence
- [27-avatar-claims-audit.md](research/27-avatar-claims-audit.md) — avatar paper/repo claim verification, XR2-class splat-rendering feasibility, licensing map
- [28-eye-tracking-stack.md](research/28-eye-tracking-stack.md) — pupil detection lineage, pye3d rotation-center IPD, Monado ET surface, placement + motor policy
- [29-eye-hardware-ipd-per-target.md](research/29-eye-hardware-ipd-per-target.md) — per-target eye cameras, IPD mechanisms, access classes, iris auth, gaze privacy
- [30-wayland-de-anatomy-protocol-seams.md](research/30-wayland-de-anatomy-protocol-seams.md) — privileged Wayland protocol inventory (ext-workspace, foreign-toplevel, layer-shell, capture…), KWin/COSMIC mechanism-vs-policy factoring, per-spin-out verdicts
- [31-kwin-vr.md](research/31-kwin-vr.md) — code study of the KWin VR fork (MR !8671): plugin architecture, the WM-core patch seams, the Qt/XWayland patch-carry surface, the Monado galaxyxr Galaxy XR bring-up, maintainer objections mapped
- [32-toplevel-export-prior-art.md](research/32-toplevel-export-prior-art.md) — prior art for per-toplevel zero-copy delegation: buffer lifetime/sync (waypipe, syncobj), consumer pacing, popup/input contracts, per-DE producer feasibility, governance path, R1–R22 requirements
- [33-steam-frame-donor.md](research/33-steam-frame-donor.md) — Steam Frame donor: reconstruction audit, system inventory, pre-hardware inference (first real donor through the pipeline)
- [34-workspace-models.md](research/34-workspace-models.md) — workspace/places models: KWin two-axis, GNOME dynamic, COSMIC pinned, niri topology, ext-workspace mechanics, visionOS/Horizon/Android XR precedents, session persistence
- [35-settings-config-models.md](research/35-settings-config-models.md) — settings models: GSettings/dconf, KConfig, cosmic-config, image-based-OS precedent, the NixOS schema-from-options interplay (constraint 9)
- [36-vr-shell-interaction-patterns.md](research/36-vr-shell-interaction-patterns.md) — comparative VR-shell patterns (placement, launcher, notifications, consent, boundary, keyboard, recenter) across six OSS shells + three commercial platforms, scored against composition constraints 6–9
- [37-accessibility-atspi.md](research/37-accessibility-atspi.md) — AT-SPI2 anatomy, Newton/AccessKit 2026 status, the XR mapping (compositor a11y duties, dwell/motor overlap, reduced-motion caps, the spatial-semantics gap)
- [38-desktop-linux-security-landscape.md](research/38-desktop-linux-security-landscape.md) — how desktop Linux security composes (PAM/polkit/portals/sandboxing/MAC/NixOS) + the consolidated index of every decided spatial-os security control

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
- [avatar-persona.md](architecture/avatar-persona.md) — Persona avatars: enrollment/asset/driver/runtime decomposition, control-interface + asset-format specs, kill-gates
- [desktop-environment.md](architecture/desktop-environment.md) — the DE plane model (system/authority/perception/shell/service), mechanism/policy/presentation rule, XR redefinitions, component dependency graph
- [foreign-session-integration.md](architecture/foreign-session-integration.md) — foreign 2D sessions as per-toplevel floating windows: the client-integration taxonomy, the toplevel export/delegation seam (maintainer provenance, buffers/pacing/popups/input), `protocols/zext-toplevel-export-v1.xml`
- [budgets.md](architecture/budgets.md) — the global frame/compute/power/thermal partition across planes + the budget-impact standing rule (overview invariant 9)
- [places-model.md](architecture/places-model.md) — the places model: typed reference-frame graph (XrSpace-grounded), attachment constraints, per-place layout, decomposed currency with the C1–C7 reconciliation rules, entry policies, protocol/restore/docked/mode-5 bindings
- [spatial-a11y.md](architecture/spatial-a11y.md) — spatial accessibility design note: AT-SPI2 baseline, zxr's compositor duties, the `zext-a11y` spatial-semantics reservation, allow-list posture
- [component-registry.md](architecture/component-registry.md) — the master component inventory: 6 planes, evidence-based status (specified/partial/missing), the gap list
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
  - [0010](architecture/adr/0010-avatar-control-space-and-driver.md) Persona avatar control space + driver (semantic v1, versioned latent hook, Monado-side driver)
  - [0011](architecture/adr/0011-eye-tracking-ipd.md) eye tracking and IPD (Monado-side eye-frame service, rotation-center IPD, event-gated motors)
  - [0012](architecture/adr/0012-de-modularity-spinout-seams.md) DE modularity and spin-out seams (standard-protocol clients vs in-process plugins vs authority-only; the zxr-private extension surface)
  - [0013](architecture/adr/0013-kwin-vr-disposition.md) KWin VR disposition (not the backbone; design donor for the 2D tier; `kwin-vr` reserved as optional session; galaxyxr fork into Galaxy XR evidence)
  - [0014](architecture/adr/0014-toplevel-delegation-protocol.md) toplevel delegation protocol (specify upstream-shaped `zext-toplevel-export-v1` now; consumer-first, smithay → KWin MR → wayland-protocols; GNOME post-standardization)
  - [0015](architecture/adr/0015-docked-desktop-mode.md) docked desktop mode (mirror tier + same-session flat presentation on external displays; quiescence ladder to 2D-compositor power; fact-gated on `spatial.hardware.externalDisplay`)
  - [0016](architecture/adr/0016-places-model.md) places model (typed frame graph; exclusive+overlay membership; decomposed currency — no active bit; groups = frames; transient+pin lifecycle; no second axis)
- [REVIEW.md](architecture/REVIEW.md) — cross-model red-team review of the base architecture
- [design-backlog.md](architecture/design-backlog.md) — disposition of the base review (fixed now vs.
  deferred to the Lynx spike / pre-release design)
- [REVIEW-perception.md](architecture/REVIEW-perception.md) — red-team review of the perception design
- [perception-design-backlog.md](architecture/perception-design-backlog.md) — disposition of the
  perception review (fixed now vs. the P-1 BSP kill-gate vs. pre-release)
- [REVIEW-avatar.md](architecture/REVIEW-avatar.md) — red-team review of the Persona avatar design
- [avatar-design-backlog.md](architecture/avatar-design-backlog.md) — disposition of the avatar
  review (fixed now vs. the A-1 adapter spike vs. pre-implementation/pre-release)
- [REVIEW-mapping.md](architecture/REVIEW-mapping.md) — red-team review of the spatial-mapping design
- [mapping-design-backlog.md](architecture/mapping-design-backlog.md) — disposition of the mapping
  review (fixed now vs. the M0 foundations gate vs. design-before-milestone)

## Normative artifacts (`../protocols/`, `../specs/`)

Wire and format contracts live outside `docs/`: [protocols/](../protocols/README.md) (Wayland
XMLs — zxr-shell-v2, zxr-workspace, zxr-layer-anchoring, zext-toplevel-export — house style in
[CONVENTIONS.md](../protocols/CONVENTIONS.md), CI-validated by wayland-scanner) and
[specs/](../specs/README.md) (perception intake, session/auth, settings schema, SpatialCast
portal). Design docs here say *why*; those say *exactly what*.

## Reference clones (`../references/`)

Git-ignored study clones, reproducible from `references/clone.sh` + the pinned
`references/MANIFEST.json`.
