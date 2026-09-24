# Mura documentation

Development workflow (the three dev loops, one command each): [README.md §Development](../README.md).

A Nix-built, NixOS-based, Wayland-based Linux XR distribution targeting many standalone VR headsets,
producing reproducible flashable images from pinned vendor firmware ("donor") inputs.

**The ethos rule** ([overview.md invariant 10](architecture/overview.md), binding agent rules in
[AGENTS.md](../AGENTS.md)): this is a Linux PC in headset form — Free Software, no walled gardens,
the wearer is the administrator. Standard Linux mechanisms are the default answer to solved
problems; consumer XR platforms are anti-patterns unless cited strictly for mechanism.

**Grounding rule:** every design grounds its vocabulary in the applicable freedesktop/XDG
specification before inventing terms — and states *which* "XDG" it means (the CDG spec family,
the `xdg_*` Wayland protocol namespace, or xdg-desktop-portal; the three-way trap is
[desktop-environment.md §2](architecture/desktop-environment.md)). Protocol surveys sweep whole
directories, never named lists (doc 30's scope rule).

**No-deferral rule:** design docs and ADRs *specify* — a design, a condition-shaped rule ("X
exists only when Y does"), a non-goal with reserved hooks, or an open question that names its
decider (a gate, a measurement, or an owner). Scheduling language ("deferred", "later", build
order) lives only in
[implementation-path.md §5](architecture/implementation-path.md), the deferral register; the four
review backlogs are its gate-detail satellites.

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
- [11-display-managers-greeters.md](research/11-display-managers-greeters.md) — greetd/GDM/SDDM/seatd, the greeter, and the seat/DRM handoff; §11 addendum: the pre-authentication greeter *furniture* inventory (power menu via login1 `allow_active`, session chooser, a11y, network, clock/banner, zero-accounts and empty-password behaviour, lock-vs-greeter) across GDM/SDDM/LightDM/gtkgreet/regreet/tuigreet with the standard-set parity list
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
- [38-desktop-linux-security-landscape.md](research/38-desktop-linux-security-landscape.md) — how desktop Linux security composes (PAM/polkit/portals/sandboxing/MAC/NixOS) + the consolidated index of every decided Mura security control
- [39-compositor-base-landscape.md](research/39-compositor-base-landscape.md) — the compositor-base ratification evidence: smithay coverage audit (frontend/renderer split, syncobj, dmabuf feedback, lease, Xwayland), WayVR's OpenXR-Vulkan anatomy, the wlroots fallback record, weston/Louvre/Mir/waynest dispositions, the R0 bring-up gates
- [41-multi-user-login-landscape.md](research/41-multi-user-login-landscape.md) — multi-account mechanics: AccountsService/GDM/SDDM/greetd enumeration + picker patterns, LightDM's guest lifecycle contract, Quest/Vision Pro/AOSP/Steam Deck precedents, NixOS account durability on A/B images (the mutableUsers slot-switch trap; userborn recommendation)
- [42-input-bootstrap.md](research/42-input-bootstrap.md) — input bootstrap: what a headset accepts before anything is configured (XR first-boot flows and their head-crosshair + button fallbacks; VR text-entry mechanisms and measured rates; Linux pre-login OSK/Bluetooth/HID/passwordless mechanics; Monado's 3DoF floor, HMD buttons via libinput/logind, proximity; hands-free pointing baselines; out-of-band provisioning — USB gadget + sshd, NM hotspot + captive portal, the Cockpit/wifi-connect/comitup/RaspAP/LuCI comparison; hypothesis verdicts, adopt/reject candidates, open questions with deciders)
- [43-microphone-native-linux-capture-audit.md](research/43-microphone-native-linux-capture-audit.md) — first hardware-enablement audit: per-target microphone hardware and stock/native capture paths, physical-vs-logical channels, and static/runtime qualification
- [44-hardware-enablement-audit-methodology.md](research/44-hardware-enablement-audit-methodology.md) — canonical L0–L7 method, evidence ladder, base/accessory/community profile model, shared-artifact ownership, and review/output contract for docs 43/45–53
- [45-imu-3dof-monado-native-linux-audit.md](research/45-imu-3dof-monado-native-linux-audit.md) — per-target headset IMU transport, firmware/calibration, timestamps and Monado 3DoF/F4 readiness
- [46-display-panel-drm-native-linux-audit.md](research/46-display-panel-drm-native-linux-audit.md) — per-target eye panels, DSI/DRM/KMS, calibration, direct mode/leases and on-glass qualification
- [47-world-camera-native-linux-ingestion-audit.md](research/47-world-camera-native-linux-ingestion-audit.md) — outward/attachable cameras through CSI/ISP/V4L2 or donor bridge to perception, including the separate Arcturus color profile
- [48-wifi-bluetooth-native-linux-audit.md](research/48-wifi-bluetooth-native-linux-audit.md) — radio chips, board data/firmware/regulatory closure, NetworkManager/BlueZ and AP+STA qualification
- [49-power-thermal-charging-native-linux-audit.md](research/49-power-thermal-charging-native-linux-audit.md) — PMIC, battery/gauge, charging/USB-PD, fan/thermal ABI and sustained-load safety
- [50-speaker-output-audio-native-linux-audit.md](research/50-speaker-output-audio-native-linux-audit.md) — speaker/amp/DSP playback through ALSA/PAL, UCM and PipeWire sink, separate from mic capture and HRTF policy
- [51-android-boot-donor-extraction-audit.md](research/51-android-boot-donor-extraction-audit.md) — target/build-specific boot chain, partition/protected-state evidence, safe extraction, unlock/root separation and recovery gates
- [52-eye-camera-ipd-actuator-native-linux-audit.md](research/52-eye-camera-ipd-actuator-native-linux-audit.md) — inward cameras/illuminators, IPD readback/motor actuation, raw versus derived gaze and motor-safety gates
- [53-proximity-presence-native-linux-audit.md](research/53-proximity-presence-native-linux-audit.md) — wear-sensor hardware through IIO/SSC to Monado/OpenXR presence, including profile occlusion semantics
- [54-first-run-authority.md](research/54-first-run-authority.md) — how shipping Linux first-run flows let the first user set system state (gnome-initial-setup's two modes and `new_user_only` pages; SteamOS `holo-polkit-helpers` granting the passwordless `deck` user `set-timezone`/`set-hostname`; elementary's `lightdm`-identity rule; Lomiri; Calamares; the tours), Mura's seven steps with authority per instance, candidates for the in-headset time-zone confirm judged on UX (recommendation: derive + a Mura rule for exactly two actions; decider: owner)

## Architecture (`architecture/`)

- [overview.md](architecture/overview.md) — layers, boundaries, invariants
- [device-contract.md](architecture/device-contract.md) — the typed `mura.*` device contract
- [donor-pipeline.md](architecture/donor-pipeline.md) — acquire→identify→parse→extract→qualify
- [images-and-updates.md](architecture/images-and-updates.md) — image families + two-backend updates
- [repo-structure.md](architecture/repo-structure.md) — monorepo layout + patch management; `profiles/` (default / multi-user / dev — declared configurations, opt-in by import) and the `modules/os/` one-file-per-concern ownership table
- [zxr-shell-v2-composition.md](architecture/zxr-shell-v2-composition.md) — the XR compositor's renderer-agnostic colour+depth composition model and MVP
- [perception-passthrough-hands.md](architecture/perception-passthrough-hands.md) — passthrough view-correction + hand cutout as compositor layers
- [spatial-sharing.md](architecture/spatial-sharing.md) — the five sharing modes (spectate / 2D window / per-observer 3D / share-the-app proxying / workspace join)
- [spatial-mapping.md](architecture/spatial-mapping.md) — anchors, persistence, relocalization, planes/mesh/boundary (Tier 4 world understanding)
- [avatar-persona.md](architecture/avatar-persona.md) — Persona avatars: enrollment/asset/driver/runtime decomposition, control-interface + asset-format specs, kill-gates
- [desktop-environment.md](architecture/desktop-environment.md) — the DE plane model (system/authority/perception/shell/service), mechanism/policy/presentation rule, XR redefinitions, component dependency graph
- [foreign-session-integration.md](architecture/foreign-session-integration.md) — foreign 2D sessions as per-toplevel floating windows: the client-integration taxonomy, the toplevel export/delegation seam (maintainer provenance, buffers/pacing/popups/input), `protocols/zspatial-toplevel-export-v1.xml`
- [budgets.md](architecture/budgets.md) — the global frame/compute/power/thermal partition across planes + the budget-impact standing rule (overview invariant 9)
- [places-model.md](architecture/places-model.md) — the places model: typed reference-frame graph (XrSpace-grounded), attachment constraints, per-place layout, decomposed currency with the C1–C7 reconciliation rules, entry policies, protocol/restore/docked/mode-5 bindings
- [spatial-a11y.md](architecture/spatial-a11y.md) — spatial accessibility design note: AT-SPI2 baseline, zxr's compositor duties, the `zspatial-a11y` spatial-semantics reservation, allow-list posture
- [component-registry.md](architecture/component-registry.md) — the master component inventory: 6 planes, evidence-based status (specified/partial/missing), the gap list
- [implementation-path.md](architecture/implementation-path.md) — the plan of record, rev 4, on **two axes**: the compositor rungs (R0 → G1 → G2-as-a-swap → G3; M1–M4) and the **D-track** of compositor-free NixOS distribution groundwork (D0 session-from-contract with stand-ins → D1 persist/F1 → D2 policy → D3 out-of-band → D4 session wrapper → D5 authd → D6 preflight/mark-good → D7 settings daemon), each VM-verified on two fixtures; the boot chain grouped by dependency class; the B1b preflight probe contract (§3a-bis); the stand-in rule; the deferral register incl. the pre-groundwork specifications
- [first-run-onboarding.md](architecture/first-run-onboarding.md) — the F-track, rev 2: the image is the installation (default image = declared `mura`, no password, autologin; greeter images build-assert a declared account), persistent-state classes, F1 silent provisioning (per-task markers), F2 first-session welcome surface (per-item gated, never a wall; contents open on research/42), F3 out-of-band access (rev 2.5: sshd on every profile with upstream defaults; PSK hotspot + captive portal + the bespoke `mura-setup` web app on gadget/hotspot only, until the explicit `setup-complete`; static passwordless posture — admin requires a password, SSH after `passwd` or with a declared key), factory-vs-user calibration, factory reset as device-transfer (never credential recovery)
- [multi-user.md](architecture/multi-user.md) — standard Linux multi-user (no cap, wheel+polkit admin, **one credential** — a numeric password renders a digit pad, encryption as the user's choice): userborn-persisted account database across A/B, the XR greeter at parity with the standard furniture set and operable at the input floor (GDM's Wi-Fi polkit rule shipped), the complete PAM-service + polkit-rule table (§3.1; rev 3.5: sshd upstream, the `mura-setup` scoped rule set, no rule for ordinary sessions), per-user calibration, optional LightDM-style ephemeral guest, places-by-account
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
  - [0014](architecture/adr/0014-toplevel-delegation-protocol.md) toplevel delegation protocol (specify upstream-shaped `zspatial-toplevel-export-v1` now; consumer-first, smithay → KWin MR → wayland-protocols; GNOME post-standardization)
  - [0015](architecture/adr/0015-docked-desktop-mode.md) docked desktop mode (mirror tier + same-session flat presentation on external displays; quiescence ladder to 2D-compositor power; fact-gated on `mura.hardware.externalDisplay`)
  - [0016](architecture/adr/0016-places-model.md) places model (typed frame graph; exclusive+overlay membership; decomposed currency — no active bit; groups = frames; transient+pin lifecycle; no second axis)
  - [0017](architecture/adr/0017-first-run-provisioning.md) first-run provisioning (rev 2: the image is the installation — default image declares `mura`/no password/autologin; no pre-login onboarding, no dispatcher, F2 = first-session welcome surface; greeter images build-assert a declared account with an `allowNoDeclaredAccount` escape hatch; rev 2.1: one credential with a digit-pad rendering hint, welcome surface = see → walk → speak, the input floor as conformance (`mura.hardware.input.*`), Cockpit + captive-portal launcher, `pairing/` class wiped on reset; rev 2.2: static passwordless posture — no `nullok` on sudo/polkit, `passwd` is the admin gate, in-headset-PSK hotspot; rev 2.4: the regular-Linux-PC correction — sshd upstream defaults on every profile (key-only scoping withdrawn), the password is the wearer's choice + a builder's declared key, `mura-setup` one program in two instances (Cockpit dropped), `setup-complete` as the explicit end of provisioning, welcome-surface authority verified per card; per-task F1 markers over `ConditionFirstBoot`; secrets never in the store; factory reset = device transfer, never credential recovery)
  - [0018](architecture/adr/0018-multi-user-accounts.md) multi-user (rev 3.1, Linux-native: standard accounts, no cap, wheel+polkit, one credential — a numeric password gets a digit-pad rendering, encryption offered never adjudicated; userborn-persisted userdb across A/B; NSS picker + the standard greeter furniture with GDM's Wi-Fi rule; optional LightDM-style guest; rev-2 appliance imports rescinded)
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
XMLs — zxr-shell-v2, zxr-workspace, zxr-layer-anchoring, zspatial-toplevel-export — house style in
[CONVENTIONS.md](../protocols/CONVENTIONS.md), CI-validated by wayland-scanner) and
[specs/](../specs/README.md) (perception intake, session/auth, session bootstrap, settings schema, SpatialCast
portal). Design docs here say *why*; those say *exactly what*.

## Reference clones (`../references/`)

Git-ignored study clones, reproducible from `references/clone.sh` + the pinned
`references/MANIFEST.json`.
