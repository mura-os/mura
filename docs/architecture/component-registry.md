# Component registry

**Status:** draft, desktop-architecture workstream. **Date:** 2026-09-22.
**Companions:** [desktop-environment.md](desktop-environment.md) (the plane model and the
component DEPENDENCY GRAPH — this registry deliberately does not sequence anything, and no build
order has been chosen anywhere),
[adr/0012-de-modularity-spinout-seams.md](adr/0012-de-modularity-spinout-seams.md) (the spin-out
decisions — §8 below only nominates candidates).

## 1. What this is

The master inventory of every component spatial-os must create (or adopt) to be a complete
desktop environment on a headset: what exists on paper, what is half-designed, what is missing
entirely, and where each piece lives. It is the canonical "what we need to create, and what lives
where" index; the dependency structure between these components lives in
[desktop-environment.md §6](desktop-environment.md) (build order is a later, separate decision).

**The five runtime planes** (plus one build plane, §7):

- **System plane** — seat/session brokering (logind/seatd), the greetd display manager, the zxr
  `--greeter` mode's system half, PAM, boot splash, session lifecycle. ([adr/0007](adr/0007-session-greeter-lock.md))
- **Authority plane** — the zxr compositor ([adr/0006](adr/0006-compositor-strategy.md)): the
  Wayland protocol server + `zxr-shell-v2`, input/focus/activation/stacking authority, the window
  and spatial-workspace model, lock ENFORCEMENT (ADR 0007 invariants I1–I3), capture
  authorization, and sort-last composition into one OpenXR projection layer. Gating question:
  *does this need knowledge or control over arbitrary clients?*
- **Perception plane** — XR-specific, no desktop analog: the Monado runtime and the Monado-side
  services of [adr/0008](adr/0008-perception-services-placement.md)–[0011](adr/0011-eye-tracking-ipd.md)
  (VIO/SLAM + mapping/anchors, passthrough + hand cutout, avatar driver, eyes/IPD). Gating
  question: *does this need camera frames or pose at exposure time?*
- **Shell plane** — presentation: launcher, task switcher, pager/overview, panels, OSDs,
  notification UI, lock-scene UI, decoration chrome.
- **Service plane** — settings daemon/config model, xdg-desktop-portal backend, polkit agent,
  secrets/keyring, notification spec service, input methods/virtual keyboard, power/idle policy,
  clipboard, accessibility, audio policy.

**The mechanism / policy / presentation rule.** Every feature splits into up to three parts and
each part is placed separately. Example — alt-tab: shortcut interception (authority mechanism) +
switcher model (policy) + switcher UI (shell presentation) + activation (authority mechanism).
The tables tag each row accordingly; a row tagged `mech+policy` is a candidate for later
splitting, which is exactly what ADR 0012 adjudicates.

**Status vocabulary** (applied strictly): **specified** = an ADR or architecture-doc section
contains a real design; **partial** = mentioned, required, or sketched but underspecified (a
research doc's open-questions list is at best partial); **missing** = no coverage found. Every
row cites its evidence; missing rows say "no doc found".

**Placement vocabulary:** `in-compositor` (the zxr process), `in-Monado` (the runtime process or
a dlopen'd tracker), `separate client` (an ordinary Wayland/zxr client), `separate daemon`
(system or session service), `build-time` (a Nix derivation/module, never on the device's
critical path).

## 2. System plane

| Component | M/P/P | Placement | Protocol seam | Status | Evidence |
|---|---|---|---|---|---|
| Seat/session brokering (logind or seatd) | mech | separate daemon | D-Bus (logind) / seatd socket | **partial** | [adr/0007](adr/0007-session-greeter-lock.md) §Context names logind/seatd as the DRM-master/hidraw broker; "logind vs seatd on the appliance image" is an explicit open question (§Open questions) |
| greetd display manager | mech | separate daemon | greetd JSON IPC (`$GREETD_SOCK`) | **specified** | ADR 0007 §Decision (both profiles use `services.greetd`); [research/11](../research/11-display-managers-greeters.md) §2/§5 |
| zxr `--greeter` mode | mech+pres | in-compositor (restricted mode, `greeter` user) | `$GREETD_SOCK`; Monado (IMU-only); no client Wayland socket | **specified** | ADR 0007 §Two profiles; contract `spatial.xr.session.greeter` ([lib/contract](../../lib/contract/default.nix)) |
| Appliance autologin profile | policy | NixOS module | greetd `initial_session` | **specified** | ADR 0007 §Two profiles; contract `spatial.xr.session.autoLogin` + profile-exclusivity assertion (lib/contract) |
| `spatial-authd` PAM helper | mech | separate daemon (per-conversation helper) | private socketpair; PAM | **specified** | ADR 0007 §PAM out of process; [specs/session-auth.md](../../specs/session-auth.md) rev 2 (nonced batched conversation, transition table, L1–L3 instrumentation); NixOS `security.pam.services.spatial-lock` |
| PIN credential (`pam_spatial_pin`) + enrollment | mech | inside PAM stack | PAM | **specified** | [ADR 0017](adr/0017-first-run-provisioning.md) ratifies doc-12 option (b): argon2 hash in the `enrollment/` state class, enrolled via `spatial-provisiond` at OOBE; wired into `security.pam.services.spatial-lock` only (greetd login stays account-password); owner-password-is-PIN recorded as the appliance bridge |
| First-boot provisioning (F1 units: keys, store seeding, growth; marker-gated) | mech | oneshot system units | filesystem (state classes) | **specified** | [first-run-onboarding.md §3](first-run-onboarding.md): persist marker authoritative over `ConditionFirstBoot`, idempotent units + atomic markers, machine-id class rules; skeleton service implemented in [families/uefi-rauc](../../families/uefi-rauc/default.nix) |
| Session dispatcher (greetd `default_session` wrapper: provisioned-flag → `--oobe` \| `--greeter`; appliance launch-wait-exec continuation) | mech | wrapper binary run as greetd's session user | greetd exec; non-secret `/run/spatial/provisioned` flag (root-published mirror of the 0700 marker) | **specified** | [first-run-onboarding.md §4.1](first-run-onboarding.md) (marker-access + continuation semantics recorded), ADR 0017 (greetd cannot select sessions from runtime state; contract `spatial.xr.session.provisioning.mode`) |
| `spatial-provisiond` (privileged enrollment authority: PIN hash, device keys, transactional marker) | mech | separate root daemon (per-conversation, spatial-authd shape) | private SOCK_SEQPACKET socket | **specified** | [first-run-onboarding.md §4.2](first-run-onboarding.md), ADR 0017 (UI/authority split; OOBE UI can never mint credentials or the marker) |
| zxr `--oobe` mode (onboarding wizard UI) | mech+pres | in-compositor (restricted mode, unprivileged) | provisiond socket; NetworkManager D-Bus; settings stores | **specified** | [first-run-onboarding.md §4](first-run-onboarding.md) (wizard ladder, restrictions = greeter mode's); visual/UX design of the wizard scenes still open (shell-plane presentation rule) |
| Session bootstrap wrapper (B6a: pam_systemd session, three-class environment, readiness-ordered targets, teardown-before-return) | mech | wrapper process (the greetd session) | systemd user manager; sd-notify; `graphical-session(-pre).target` | **specified** | [implementation-path.md §2 B6a](implementation-path.md) (manager-correct lifetimes: no cross-manager BindsTo; wrapper owns coupling and keeps the greetd session alive); normative `specs/session-bootstrap.md` gated on G2 experience |
| Session lifecycle (`spatial-session.target`) | mech | systemd user target | systemd | **specified** | ADR 0007 §Two profiles (owns Monado + compositor + shell services; crash/restart is systemd's job; boot-locked restart per invariant I3) |
| XR-init preflight probe + recovery ladder (gate before greeter/session; crash-loop threshold; flat/SSH/diagnostic fallback) | mech | separate probe process (under the session target) | exit status / small report | **specified** | [implementation-path.md §2 B1b](implementation-path.md) (gates: runtime-created Vulkan device, GPU match, factory-calibration validity, DRM/IMU nodes, Monado first frame; never a permanently dark headset); pattern from KWin VR's `kwinvr-xrtest` ([31 §2.7](../research/31-kwin-vr.md), ADR 0013 §2); composition §7.3 |
| Boot splash (per-eye pre-distorted) | pres | separate early-boot component | none-yet (KMS, from system-state calibration) | **partial** | ADR 0007 §Cross-cutting: no Plymouth; dark panels default, pre-distorted logo a stretch goal; no design beyond that ([research/12](../research/12-lock-screens-and-appliance-login.md) §7) |
| Per-unit calibration state (`/var/lib/spatial/`) | mech | system state (not a process) | filesystem contract | **specified** | ADR 0007 §Cross-cutting; [device-contract.md](device-contract.md) `spatial.xr.calibration.paths`, `protectedPartitions` |
| Update agent (slot switch, mark-successful, readiness gating, boot-attempt fallback) | mech+policy | separate daemon / oneshot units | RAUC D-Bus / systemd-boot `+tries` BLS counting + `boot-complete.target`/`systemd-bless-boot` / `qbootctl`-class slot ioctls | **specified** | [images-and-updates.md](images-and-updates.md) §Updates + §Health-gated success; [implementation-path.md §3a](implementation-path.md) (blessing tiers profile-specific, stability interval, three separate transitions: readiness → boot blessing → RAUC state; wired explicitly in-family since `boot.loader.systemd-boot.enable = false`); `spatial.qualification.readinessCheck` **is** in [lib/contract](../../lib/contract/default.nix) with the tier assertion |
| Docked-mode policy + quiescence ladder (dock detect, docked presence branch, soft/deep idle target switching, don-resume) | policy | systemd targets + in-compositor state | systemd target subset of `spatial-session.target`; socket-activated Monado; `XR_EXT_user_presence` + DRM hotplug | **missing** | ladder and policy decided by [adr/0015](adr/0015-docked-desktop-mode.md) (amending ADR 0007's doff ladder); `spatial.xr.session.docked.*` doc-only; no implementation design |

## 3. Authority plane (the zxr compositor)

Everything here is one process (ADR 0006: a single OpenXR client of Monado that composites
everything and submits one stereo projection layer). Rows are the subsystems of that process.

| Component | M/P/P | Placement | Protocol seam | Status | Evidence |
|---|---|---|---|---|---|
| Wayland protocol server (core + proxied-client baseline globals) | mech | in-compositor | standard: `wl_compositor` 6, `wl_shm`, `wl_seat`, `wl_output` 4, `xdg_wm_base` 7, `wl_data_device_manager` 3, `zwp_linux_dmabuf_v1` v4+ w/ feedback, `wp_viewporter`, `wp_fractional_scale_v1`, `zxdg_decoration_manager_v1`, `wp_linux_drm_syncobj_v1`, `wp_presentation` | **specified** | ADR 0006 §Decision (base ratified Rust+smithay 2026-09-23, evidence [research/39](../research/39-compositor-base-landscape.md)); [spatial-sharing.md](spatial-sharing.md) §3 must/should list; [research/19](../research/19-wayland-proxying.md) §8 (versions + citations) |
| `zxr-shell-v2` protocol (3D tier: views, colour+depth buffers, matrix split, clipping, size negotiation, 6DoF/ray input, frame timing) | mech | in-compositor | zxr-private (`zxr-shell-v2.xml`, upstreaming intent) | **specified** | ADR 0006 §The protocol; [zxr-shell-v2-composition.md](zxr-shell-v2-composition.md) §7.2/§8; normative XML drafted at rev 2 ([protocols/zxr-shell-v2.xml](../../protocols/zxr-shell-v2.xml)) |
| Vulkan renderer + OpenXR client loop | mech | in-compositor | `XR_KHR_vulkan_enable2` via openxrs (ash 0.38); one projection layer to Monado | **specified** | ADR 0006 §The renderer + §The compositor base (ratified; device created by the runtime, WayVR-proven shape — [research/39](../research/39-compositor-base-landscape.md) §1.9/§2); composition §7.4 |
| Sort-last depth composition engine (T1; T2/T3/T4 tiers reserved) | mech | in-compositor | internal (negotiated dmabuf/opaque-fd transport, composition §5) | **specified** | composition §2–§5 |
| Frame scheduling (snapshot distribution, composition cutoff, deadline/placeholder rule) | mech+policy | in-compositor | zxr frame events | **specified** | composition §7.4; pacing policy across heterogeneous clients carried open (§8) |
| 2D quad tier (xdg-shell surface trees, subsurfaces, popups, shm+dmabuf, plane-depth generation) | mech | in-compositor | `xdg-shell` + core protocols | **specified** | composition §7.3 (first-class from M1) |
| Rootless Xwayland | mech | separate client (Xwayland) + in-compositor WM glue | X11/xwayland | **partial** | composition §7.3 names it a requirement and M4 tests it; no integration design (no doc on the XWayland WM half) |
| Window model (toplevel lifecycle, initial 3D placement, move/resize/stacking) | policy | in-compositor | none-yet | **partial** | composition §7.3 holds texture/size/world transform + the five normative WM-core constraints mined from KWin VR (no output binding; designed 2D↔3D transitions; popup placement volumes — [31 §3](../research/31-kwin-vr.md), ADR 0013 §2); M1 tests move/rotate/resize; "how 2D toplevels map to initial 3D placement" now has a cross-ecosystem convergence to start from ([research/36 §2](../research/36-vr-shell-interaction-patterns.md): head-relative spawn at a declared distance facing the user; recenter = rigid whole-layout re-seat preserving relative positions, anchored content exempt); no window-management policy doc |
| Spatial-workspace / space model — the places model (typed frame graph, membership, currency, lifecycle) | policy | in-compositor | `ext-workspace-v1` + zxr workspace extension (fields in [places-model.md §6](places-model.md)) | **specified** | [places-model.md](places-model.md) + [ADR 0016](adr/0016-places-model.md) (three-layer split; exclusive+overlay membership; decomposed currency with C1–C7 rules; transient+pin lifecycle with entry policy; groups=frames); evidence [research/34](../research/34-workspace-models.md); open items in places-model §9 |
| Input authority (seat, keyboard focus, ray→plane hit-testing, 6DoF pointer routing) | mech | in-compositor | `wl_seat`; zxr ray/6DoF pointer (ADR 0006 protocol) | **specified** | ADR 0006 §The protocol (ray device, hit-test compositor-derived); composition §7.3 (ray → window-local `wl_pointer`; hover/focus resolution pluggable, pointer space unbounded — KWin VR constraints 1–2, ADR 0013 §2). Gaze/ray **stabilization is a requirement, not a nicety**: the KWin VR field evidence is pure distance-ordered picking with zero smoothing/deadzone/magnetism in code, and users asked for all three ([31 §2.11](../research/31-kwin-vr.md)) |
| Focus/activation authority (`xdg-activation`, focus-stealing policy) | mech+policy | in-compositor | `xdg-activation-v1` | **missing** | no doc found. composition §7.3 says "focus/activation" is required for real 2D support, with no design; research/19 §8 notes xdg-activation merely passes through waypipe |
| Lock state machine (invariants I1–I3; boot-locked; `SetLockedHint` ordering) | mech | in-compositor | internal; logind D-Bus; `ext-session-lock-v1` served on dev profile only | **specified** | ADR 0007 §The lock model; composition §8 "lock as a composition-policy state"; contract `spatial.xr.session.lock.*` |
| Idle/presence policy (doff/don grace, idle-to-lock; serve `ext-idle-notify-v1`, honor `zwp_idle_inhibit_v1`) | mech+policy | in-compositor (own OpenXR loop, `XR_EXT_user_presence`) | standard protocols + OpenXR | **specified** | ADR 0007 §Cross-cutting; research/12 §6.2; contract `lock.triggers`/`doffGraceSeconds` |
| Capture surface (`ext-image-copy-capture-v1` + `ext-image-capture-source-v1` + `ext-foreign-toplevel-list-v1`, output/toplevel/cursor sessions) | mech | in-compositor | standard staging protocols | **specified** | spatial-sharing §2; [research/17](../research/17-sharing-capture-stack.md) §3/§8 |
| Capture/sensitive-global authorization (per-origin policy engine) | mech+policy | in-compositor | `wp_security_context_manager_v1` | **specified** | spatial-sharing §3/§6 invariant 3; research/19 §8 (highest-leverage item; gates screencopy/data-control against proxied clients); end-to-end verification open (19 §11.1) |
| Input injection server (remote/emulated input) | mech | in-compositor | libei/EIS; `mapping_id` join to streams | **specified** | spatial-sharing §2; research/17 §5 (per-share absolute device, badged, pausable) |
| Spectate tap (pre-distortion crop-blit → publication) | mech | in-compositor | PipeWire (`Video/Source`) | **specified** | spatial-sharing §2.1 (symmetric-FOV crop; `comp_mirror_to_debug_gui` as reference); policy-filtered spectate rides observer views (known fork) |
| Observer-view authorization/budget objects | mech+policy | in-compositor | zxr-private (reserved hook) | **specified** | spatial-sharing §8.1 (origin, budget, revocation, decline capability) |
| Per-app capture groups (mode-3 egress endpoints) | mech | in-compositor | zxr-private (reserved hook) | **specified** | spatial-sharing §8.2 (wolf pattern) |
| Mode-3 RGBD bridge (egress/ingress, pacer, encoder abstraction, depth codec, validity masks, per-observer budgets) | mech | in-compositor (bridge component) | private typed two-channel network protocol (WiVRn-shaped) | **specified** | spatial-sharing §4 (vendored WiVRn modules + the four genuinely new pieces); codec bake-off open ([research/18](../research/18-xr-streaming.md) §9) |
| Toplevel-delegation consumer (foreign 2D sessions as per-toplevel floating windows) | mech | in-compositor | `zspatial-toplevel-export-v1` rev 3 (XR-agnostic, upstream-intent — [protocols/](../../protocols/README.md)) | **specified** | [foreign-session-integration.md](foreign-session-integration.md) + rev-3 XML against R1–R24 ([research/32 §8](../research/32-toplevel-export-prior-art.md)); producer side now specified too: conformance spec [specs/toplevel-export-producer.md](../../specs/toplevel-export-producer.md) + code-verified briefs [producers/kwin.md](producers/kwin.md) / [producers/mutter.md](producers/mutter.md) on [research/40](../research/40-toplevel-export-producers.md); implementation staged behind composition M1 (ADR 0014 M-A); producers remain ADR 0014 milestones, not registry components |
| Workspace-join replication (mode-5 placement-graph sync, rights, control leases) | mech+policy | in-compositor + sharing service | small reliable control protocol | **partial** | spatial-sharing §5 defines the state and rights split; no protocol spec, no service placement; "sharing service" named only in §6 invariant 4 |
| Perception-layer intake (environment + hand-top layers; latest-complete, never awaited) | mech | in-compositor | dedicated SEQPACKET IPC + dmabuf + syncobj timelines ([specs/perception-intake.md](../../specs/perception-intake.md)) | **specified** | ADR 0008 §Decision (placement) + [specs/perception-intake.md](../../specs/perception-intake.md) rev 2 (dual-rate generation record, registration/image tables, GPU-safe reclamation, snapshot selection, producer-death); backlog #5/#8 dispositions in its §9 |
| Boundary breach response (forced passthrough, no client cooperation) + boundary overlay rendering | mech | in-compositor | internal; IMU-rate probe queries from geometry service | **specified** | spatial-mapping §7 (compositor-owned overlay + composition-policy breach response; threshold semantics part of the contract) |
| Desktop windowed output mode (mouse-camera dev mode) | mech | in-compositor | ordinary window (`spatial.xr.compositor.backend = window`) | **specified** | composition §7.1; contract option (lib/contract) |
| Decoration enforcement (force server-side) | mech | in-compositor | `zxdg_decoration_manager_v1` | **specified** | spatial-sharing §3 / research/19 §8 (force `server_side`); the *chrome renderer* itself is a shell-plane row (§5) |
| Scene graph (surface→world transforms, decoration nodes, damage tracking) | mech | in-compositor | internal | **partial** | implicit in composition §7.3 (per-window texture/size/world transform) and the damage note in research/17 §8; never named or designed as a subsystem ([desktop-environment.md §3](desktop-environment.md) authority table) |
| Decoration *policy* (per-window gets-chrome decision, SSD/CSD negotiation stance, per-state border behaviour) | policy | in-compositor | `zxdg_decoration_manager_v1` (negotiation only) | **partial** | forced-SSD for proxied clients is decided (research/19 §8); the per-window/per-state policy itself has no design; three-concern split in [desktop-environment.md §2](desktop-environment.md) trap 4 |
| Colour pipeline (colour-management/-representation service, panel calibration application, sRGB/linear composition policy, passthrough↔rendered matching) | mech | in-compositor | `color-management-v1` + `color-representation-v1` (staging, in pinned wayland-protocols) | **missing** | no doc found; composition §2 fixes depth encoding but not colour; per-device panel calibration is a build-plane fact with no runtime owner; seam evidence in doc 30 addendum |
| Effects / animation module (open/close/move/switch transitions under comfort caps) | policy+pres | in-compositor (plugin seam) | in-process plugin API (KWin-effects precedent) | **missing** | no doc found; XR comfort makes sudden large-surface motion a safety concern, not eye-candy; ADR 0012 places it in-process |
| Session-restore mechanism (server side: session identity, toplevel state restore) | mech | in-compositor | `xdg-session-management-v1` (staging, in pinned wayland-protocols) | **missing** | no doc found; the relaunch half is the service-plane restore manager (§6); split per [desktop-environment.md §2](desktop-environment.md) trap 5 |
| Docked flat-composition output path (external DRM connector scanout; per-output presentation policy incl. fullscreen/direct scanout; mirror-tier cells) | mech+policy | in-compositor | DRM/KMS on `spatial.hardware.externalDisplay`; capture-taxonomy cells ([spatial-sharing §2.2](spatial-sharing.md)) | **missing** | decided by [adr/0015](adr/0015-docked-desktop-mode.md) (B3 + mirror tier; B2 rejected); no component design; per-device facts verified in research/07 §External video-out |
| Virtual-screen quads (compat presentation surface: a planar window-group quad in space; the inverse of the docked output — same one-model-two-presentations question) | policy+pres | in-compositor | internal (presentation policy over the window model; composition §7.3 constraint 5) | **missing** | no zxr design; mechanism precedent fully mined from KWin VR (real backend virtual outputs, pixels-per-cm sizing, render-pass skip — [31 §2.12](../research/31-kwin-vr.md)); quad↔space transitions per [31 §2.9](../research/31-kwin-vr.md) |

## 4. Perception plane

| Component | M/P/P | Placement | Protocol seam | Status | Evidence |
|---|---|---|---|---|---|
| Monado runtime (OpenXR, socket-activated, `active_runtime.json` declared) | mech | separate daemon (per-user) | OpenXR + Monado IPC | **specified** | [overview.md](overview.md) §Common layer; contract `spatial.xr.runtime`/`compositor.backend`/`environment`; wired in [modules/xr](../../modules/xr/default.nix) |
| Frameserver / `xrt_frame` fan-out (one clock, one calibration) | mech | in-Monado | Monado-internal | **specified** | ADR 0008 §Decision (the central invariant); fan-out resource policy (queue bounds, retention) open — REVIEW-mapping M-16 |
| Pose-at-exposure query API | mech | in-Monado | Monado-internal (needs definition) | **partial** | named a **blocking dependency**, not a detail ([perception-passthrough-hands.md](perception-passthrough-hands.md) §invariants 2; ADR 0008 §Consequences); concrete API + uncertainty contract gated (perception backlog #6, an enable gate) |
| Basalt VIO via VIT | mech | in-Monado (dlopen'd tracker) | VIT (thin pose seam, unextended) | **specified** | [adr/0009](adr/0009-spatial-mapping-architecture.md) Decision 1; spatial-mapping §2 (nixpkgs `basalt-monado`, cached) |
| Keyframe egress (versioned packet: epoch, masks, descriptors-or-images; non-blocking relay) | mech | in-Monado → IPC | versioned private packet; candidate VIT minor extension | **partial** | ADR 0009 Decision 2 + spatial-mapping §4; packet contents/backpressure are the **M0 gate** (spatial-mapping §12.1/§12.4) |
| Mapping + anchor service (factor recovery, robust PGO, `T_local_map`, anchor table, epochs) | mech+policy | separate daemon | subscription/topic IPC (frames, IMU, poses, MargData); internal shell↔anchor xrt API at M1 | **specified** | ADR 0009 Decision 3; spatial-mapping §3–§4 (two-frame contract, correction policy, reset epochs); mapping-core choice (NfrMapper vs RTAB-Map engine) left to M0 |
| Two-tier relocalization + boot-reloc flow | mech | inside mapping service | internal | **specified** | spatial-mapping §5 (classical always-on + learned at boot; UNLOCALIZED→LOCALIZED states); room-transition state machine gated (mapping backlog M-13) |
| Encrypted map/anchor store | mech | inside mapping service | SQLite-class store; TPM2/systemd-credentials key wrapping | **specified** | spatial-mapping §6 (normative durability rules from [research/21](../research/21-anchors-persistence-openxr.md) §5.2–5.4; privacy posture) |
| Geometry service (gravity-RANSAC planes → TSDF clipmap → 8-label semantics) | mech | separate daemon (duty-cycled) | `xrt_plane_detector` shapes; mesh as dmabuf | **specified** | spatial-mapping §7; [research/22](../research/22-dense-geometry-no-lidar.md) §4 |
| Boundary system (floor + play volume + tri-state keep-out; IMU-rate distance probes) | mech+policy | probes in geometry service; response in-compositor | probe query IPC | **specified** | spatial-mapping §7/§9; contract `spatial.xr.mapping.boundary`. Boundary *setup/drawing UX* is a shell gap (§6) |
| `XR_EXT_spatial_entity`/`_anchor`/`_plane_tracking`/`_persistence` implementation | mech | in-Monado | ratified OpenXR extensions (upstream target) | **specified** | spatial-mapping §8 (state mapping, upstream order); ADR 0009 Decision 4 |
| Passthrough service (view correction: two-pass warp, anti-wobble, hole fill) | mech | Monado-side (frame sink or Monado-adjacent process — one topology to be fixed) | dmabuf + explicit-sync layers to compositor | **specified** | perception-passthrough-hands.md §Passthrough pipeline; ADR 0008 §Decision; execution-topology conflation flagged for recast (perception backlog #7) |
| Depth backend interface (`DepthFrame`: encoding, confidence, timestamps, calibration version) | mech | inside passthrough service | `DepthFrame` contract | **specified** | perception doc §Depth backend; [research/14](../research/14-mobile-stereo-depth.md) §5 |
| Vendor depth backends (`vk-qcom`, Adreno DFS, Hexagon HTP learned stereo) | mech | inside passthrough service | same `DepthFrame` interface | **partial** | perception doc §Depth backend table: all gated on unverified BSP access ([research/16](../research/16-perception-claims-audit.md) Part 1); classical Vulkan baseline is the only unconditional one |
| Hand-cutout service (matte αF+α+depth; tiers 0–2; hand-region removal from environment) | mech | Monado-side, one coupled service with passthrough; GPL nets process-isolated | dmabuf + explicit-sync; atomic matte contract | **specified** | perception doc §Hand cutout + §invariants 1; ADR 0008 |
| Hand-cutout composition policy (`visible`/`hidden`/`automatic`, per-client) | policy | zxr protocol attribute + shell default | zxr-private | **specified** | perception doc §Composition policy; contract `handCutout.upperLimbVisibility`; per-client policy after a single resolve open (perception backlog #2) |
| Mercury hand tracking (joints; cutout prior; SLAM masks) | mech | in-Monado (existing) | Monado-internal; `XR_EXT_hand_tracking` | **specified** | existing Monado component consumed as-is; perception doc §Hand cutout (ROI/trimap/depth priors; cutout must run without it) |
| Eye-tracking service (PuRe-class detector → pye3d rotation-center model) | mech | in-Monado (session-scoped eye-frame group) | Monado-internal; gaze via `XR_EXT_eye_gaze_interaction`; IPD into `eye_relation` | **specified** | [adr/0011](adr/0011-eye-tracking-ipd.md) §2 (pipeline, privacy boundary, session scoping) |
| IPD motor actuation (propose→confirm→move→re-settle; frozen models during travel) | mech+policy | in-Monado per-device driver | Monado-internal device controls | **specified** | ADR 0011 §3; contract `spatial.hardware.ipd.source` + eyes-backend assertion (lib/contract) |
| Iris authentication verifier | mech | inside ET service privacy boundary, beside PAM | none-yet | **partial** | reserved by ADR 0007 §PAM ("biometrics come later, parallel to PAM, never replacing") and ADR 0011 §4; no design |
| Avatar driver service (UE-88 normalization, calibration, degraded-mode ladder, `ControlFrame` emission) | mech | Monado-side (ownership decided; in-process vs sibling fixed at implementation against one specified boundary) | `ControlFrame` (versioned control space); consumes Monado face/gaze devices | **specified** | [adr/0010](adr/0010-avatar-control-space-and-driver.md) Decisions 1–4; [avatar-persona.md](avatar-persona.md) §driver; semantic-v1 channel registry still to publish (avatar backlog #2) |
| Derived face-device re-publication (+ the missing `XRT_INPUT_FB_FACE_TRACKING2_AUDIO` device) | mech | in-Monado (`xrt_device`) | OpenXR `XR_FB_face_tracking2` | **partial** | ADR 0010 §Consequences: state tracker routes it, "no device registers it today — verified"; role-policy rules specified, device unbuilt |
| Persona asset format + validator | mech | data format (loaded by runtime; runtime is a shell-plane client) | versioned container | **partial** | avatar-persona §asset format specifies the layout; the binary spec, operator set, resource limits and conformance corpus are pre-implementation work (avatar backlog #8) |

## 5. Shell plane

| Component | M/P/P | Placement | Protocol seam | Status | Evidence |
|---|---|---|---|---|---|
| Greeter auth scene (login panel + session list) | pres | in-compositor (`--greeter` mode) | greetd IPC; sessions from `spatial.xr.shell` | **specified** | ADR 0007 §Two profiles ("composes one built-in auth scene"; sessions from the module system, not `.desktop` files) |
| Lock-scene UI (generic PAM prompts, controller-ray PIN pad, keyboard fallback) | pres | in-compositor | PAM prompt types via spatial-authd | **specified** | ADR 0007 §PAM out of process; research/12 §6 |
| Virtual keyboard (general-purpose, ray-reachable) | mech+pres | none decided | none-yet (`zwp_virtual_keyboard`/`input-method` candidates, unadopted) | **partial** | strong convergence found: [research/36 §8](../research/36-vr-shell-interaction-patterns.md) — floating focus-bound system-owned panel, distance-switched direct-touch/ray dual mode (WiVRn's 0.18/0.22 m hysteresis independently mirrors Quest), dictation fallback; ADR 0007's lock PIN pad is the in-repo precedent; no general on-screen-keyboard design; research/19 §6 shows waypipe carries `virtual-keyboard`/`input-method-v2` protocols but nothing binds them here |
| App launcher | policy+pres | none decided | none-yet | **missing** | pattern evidence: [research/36 §3](../research/36-vr-shell-interaction-patterns.md) — flat pinned grid on one reserved gesture is universal (no shipped spatial-metaphor launcher); every OSS shell independently wrote an XDG desktop-entry scanner (WayVR `desktop_finder.rs` = best code precedent); design still missing |
| Task switcher (switcher model + UI; shortcut interception + activation are authority-plane) | policy+pres | none decided | none-yet | **missing** | no doc found (no global-shortcuts mechanism exists either — §6) |
| Pager / spatial overview | policy+pres | separate client (ADR 0012 §1) | `ext-workspace-v1` + zxr workspace extension + compositor-rendered previews | **missing** | unblocked: the model it consumes is now specified ([places-model.md §6](places-model.md) enumerates exactly what it reads, incl. partial-transition offsets for animating walks); UI design still missing |
| Panels / status surfaces (clock, battery, tracking state) | pres | none decided | none-yet | **missing** | no doc found |
| OSD framework (volume/brightness/mode toasts) | pres | none decided | none-yet | **missing** | no doc found |
| Notification presentation (spatial placement, gaze-aware, do-not-disturb) | pres+policy | none decided | none-yet | **missing** | pattern evidence: [research/36 §4](../research/36-vr-shell-interaction-patterns.md) (head-locked small transients are universal and exempt from motion caps; interruption policy diverges by immersion state); previously only a *leak risk* mention (spatial-sharing §2.1/§6); design still missing |
| Decoration chrome renderer (server-side title/move/close affordances in 3D) | pres | in-compositor (current assumption) | server-side decorations (forced) | **partial** | server-side is mandated (research/19 §8, spatial-sharing §3); zero design for what the chrome *is* in 3D |
| In-space consent picker / share chooser | pres | undecided — backend in-process vs Mutter-style private API is an open fork | portal backend chooser hook | **partial** | spatial-sharing §2 (native backend brings "the in-space consent picker"); research/17 §11.4 |
| Active-share badges + emulated-input badge | pres | in-compositor | internal | **partial** | required by spatial-sharing §6 invariant 2 and research/17 §5 (badged, pausable); no visual/UX design |
| Anchor/reloc state affordances (`LOCALIZED_TENTATIVE` fade-in, "may have moved") | pres | in-compositor/shell | anchor states from mapping service | **partial** | states and rules specified (spatial-mapping §5); the affordance rendering is named, not designed |
| Boundary setup UX (draw/confirm play area) | pres | none decided | none-yet | **missing** | spatial-mapping §7 specifies the boundary *system* but not the UX; [research/36 §7](../research/36-vr-shell-interaction-patterns.md) found **no cross-ecosystem convergence** (Quest draw-on-floor vs visionOS invisible auto-zone vs Android XR presets+floor-confirm vs nothing in OSS) — divergence tracks posture + passthrough quality, so the design should branch per profile |
| IPD wizard (fixation-target measurement UX) | pres | shell/session | ET service | **partial** | ADR 0011 §4 names "a fixation-target 'IPD wizard' at enrollment"; placement now fixed — F2 wizard step 5 ([first-run-onboarding.md §4.3](first-run-onboarding.md)) with slider/hardware-readback fallback where ET is absent; the measurement UX itself still has no design; kappa-calibration UX open (research/28 §6) |
| Avatar runtime renderer | pres | separate client (ordinary zxr client, opaque-cutout profile) | zxr-shell-v2; asset container | **specified** | avatar-persona §runtime; ADR 0010 (no privileged access); gated R-0/Z-1/R-1 |
| Window placement/manipulation UI (grab, rotate, resize handles) | pres | in-compositor | zxr/xdg-shell interactions | **partial** | M1 acceptance test requires it (composition §7.5); interaction-design evidence adopted from KWin VR's daily-driven headgaze/headscroll/follow-mode/grab-all/recenter vocabulary ([31 §2.5–2.6](../research/31-kwin-vr.md), ADR 0013 §2); the follow-mode *algorithm* (all-windows-outside-start-FOV engagement, 0.5 s dwell, rigid group rotation about the head to stop-FOV, exponential slerp with no velocity cap) is now mined as the policy precedent — and its missing comfort cap is exactly what ADR 0012's effects-module caps must supply ([31 §2.10](../research/31-kwin-vr.md)); no zxr-specific design yet |
| SNI watcher + host (items as typed panel badges) | pres | watcher: separate supervised daemon; host: panel applet/component | StatusNotifierItem D-Bus (de-facto spec, draft 0.1) | **missing** | no doc found; hosting decided by ADR 0012 (vs dropping tray compatibility); COSMIC `cosmic-applet-status-area` / Plasma systemtray are the precedent (doc 30 §A3); no design |
| Spatial capture tool (scope picker: window/window-set/plane-region/view/volume; still-vs-stream; region-on-plane sweep; gallery/clipboard destinations; appliance capture-chord preset) | policy+pres | separate client (doubles as the in-space consent picker / share chooser row above) | portal Screenshot/ScreenCast + capture seams; taxonomy normative in [spatial-sharing.md §2.2](spatial-sharing.md) | **partial** | taxonomy, validity matrix, privacy defaults (passthrough excluded from captures unless explicitly consented), and compat verdicts (Spectacle: KWin-private, not a target) specified in spatial-sharing §2.2; the tool's UI/UX itself has no design |

## 6. Service plane

| Component | M/P/P | Placement | Protocol seam | Status | Evidence |
|---|---|---|---|---|---|
| Settings daemon + user-facing configuration model | mech+policy | separate daemon (session half + privileged apply agent) | `org.spatialos.Settings1` (session bus) + schema artifact + sparse versioned stores ([specs/settings-schema.md](../../specs/settings-schema.md)) | **partial** | the *contract* is specified: [specs/settings-schema.md](../../specs/settings-schema.md) rev 2 (schema artifact from NixOS options, preference/state XDG split, relocatable instance schemas, quarantine, typed migrations, apply transactions), on the [research/35](../research/35-settings-config-models.md) evidence base; the daemon's process design itself is still missing (the spec's §10) |
| xdg-desktop-portal backend — capture tier | mech | separate daemon (day-one: `xdg-desktop-portal-wlr` unmodified; then native `xdg-desktop-portal-spatial`) | D-Bus `org.freedesktop.impl.portal.*`; PipeWire | **specified** | spatial-sharing §2; research/17 §1/§8 (xdpw needs only our protocols + `UseIn` name; native backend = 3 D-Bus methods + chooser + PW producer). Yes — the portal backend is already implied by docs 17–19; SpatialCast source types now normative in [specs/spatialcast-portal.md](../../specs/spatialcast-portal.md) rev 2 (`XR_VIEW`/`APP_VOLUME` via a frontend patch; workspace join moved to the sharing service's session API) |
| xdg-desktop-portal backend — non-capture interfaces (FileChooser, OpenURI, Settings/appearance, Account, Notification portal…) | mech | separate daemon | D-Bus | **missing** | no doc found; docs 17–19 cover only ScreenCast/RemoteDesktop/Clipboard portals |
| Notification spec service (`org.freedesktop.Notifications`) | mech | separate daemon | D-Bus notification spec | **missing** | no doc found (presentation half also missing, §5) |
| Polkit authentication agent (spatial presentation of privilege prompts) | mech+pres | separate daemon/client | polkit D-Bus agent API | **missing** | no doc found. `security.polkit.enable = true` is set in [modules/os](../../modules/os/default.nix) with **no agent**, so any privileged action would silently fail in-session; the lock's PAM plumbing (ADR 0007) is adjacent but distinct |
| Secrets service / keyring | mech | separate daemon | `org.freedesktop.secrets` D-Bus | **missing** | landscape + ownership question now evidenced in [research/38 §7](../research/38-desktop-linux-security-landscape.md) (gnome-keyring vs kwallet vs oo7; unlock coupling to PAM/lock); no design (build-host donor secrets are separate — [design-backlog.md](design-backlog.md) #12) |
| Input-method framework (text-input/IM routing; the virtual keyboard's mechanism half) | mech | in-compositor seam + separate IM client | `text-input-v3` / `input-method-v2` / `virtual-keyboard-v1` | **missing** | no doc found; ADR 0007's lock PIN pad + keyboard fallback is the partial precedent (cited in §5); research/19 §6 confirms the protocols proxy fine but nothing adopts them |
| Clipboard + DnD semantics in 3D | mech+policy | in-compositor (data-device) + policy | `wl_data_device_manager` 3; `ext-data-control` (policy-gated) | **partial** | copy-paste is an M1 requirement (composition §7.3/§7.5); research/19 proxies clipboard/DnD fully and mandates data-device v3; workspace-join authorizes clipboard separately (spatial-sharing §5). What "drag between two planes in 3D space" means (cursor? ray? spatial gesture?) — no design; data-control gating specified (spatial-sharing §3) |
| xdg-activation service behaviour (launch-token→focus handoff) | mech | in-compositor | `xdg-activation-v1` | **missing** | no doc found (authority-plane row §3; listed again here because launcher/notification "focus this app" flows depend on it) |
| Global shortcuts service (system chords, per-app grants) | mech+policy | in-compositor + portal (`org.freedesktop.portal.GlobalShortcuts`) | compositor-internal + D-Bus | **missing** | no doc found |
| Default apps / MIME associations | policy | separate config/service | `mimeapps.list`, `org.freedesktop.portal.OpenURI` | **missing** | no doc found |
| Accessibility (AT-SPI or successor, magnification, high-contrast, motor alternatives in 3D) | mech+policy+pres | a11y bus + service-plane AT clients; compositor duties in-zxr | AT-SPI2 D-Bus now; `zspatial-a11y` reserved ([spatial-a11y.md](spatial-a11y.md) §2, ADR 0012 §4.7) | **missing** | design *note* exists: [spatial-a11y.md](spatial-a11y.md) fixes the shape (baseline, three compositor duties, the spatial-semantics surface incl. place membership/currency, allow-list posture for the docs-37/38 bus-escape gap); evidence [research/37](../research/37-accessibility-atspi.md); assistive components themselves still undesigned |
| Audio policy/routing (device roles, per-app routing, spatial audio, HRTF) | mech+policy | separate daemon (PipeWire session manager — WirePlumber-class) | PipeWire | **missing** | no doc found beyond hardware facts: `spatial.adaptation.audio.backend = native (PipeWire)` (device-contract §adaptation) and "Common services: audio" in overview.md's diagram — a label, not a design |
| Power/thermal policy (suspend sequencing, thermal governors, performance profiles) | mech+policy | separate daemon | logind/upower D-Bus | **missing** | no doc found. The *idle/doff* half is specified in-compositor (ADR 0007); system suspend is referenced only as a lock trigger (`lock.triggers = [ "suspend" ]`); nothing owns thermal/perf policy despite sustained-thermal being a qualification test (device-contract §tiers). Second customer: docked idle-depth selection (soft vs deep, on-external-power bias — [adr/0015](adr/0015-docked-desktop-mode.md)) |
| Display/runtime configuration (refresh rate switching, render scale, FOV overrides) | mech+policy | none decided | none-yet | **missing** | no doc found. The contract declares hardware *facts* (`spatial.hardware.panel.{width,height,refresh}`); no runtime component lets a user or policy change render scale/refresh; foveation latency budget open in ADR 0011 |
| Recentering / reference-space reset UX ownership | mech+policy | none decided | `XrEventDataReferenceSpaceChangePending` (reserved) | **missing** | no doc found. spatial-mapping §3 reserves the event for "genuine LOCAL/STAGE redefinition" but no component owns the recenter gesture/command |
| Session restore manager (relaunch apps after login; bind restored windows to places) | mech+policy | separate daemon | `xdg-session-management-v1` (compositor side, §3) + .desktop database + `place_id`s ([places-model.md §7](places-model.md): unresolved-anchor state = currency rule C6; delegated members are R23/R24 slots) | **missing** | the place-binding half is now specified (places-model §7); the manager itself undesigned; the protocol deliberately excludes relaunching — a manager must own it (doc 30 addendum); XR-amplified: anchored places persist placement, nothing relaunches into them |
| Sharing/session service (share lifecycle, mode-5 authority, consent state) | mech+policy | separate daemon (implied) | D-Bus/portal + compositor share objects | **partial** | "the sharing service" is load-bearing in spatial-sharing §6 invariant 4 (observer views added only through it) but has no named process, placement, or API |
| Networking policy, logging/diagnostics, user management | policy | NixOS modules / standard daemons | systemd/D-Bus | **specified** | overview.md §Common layer assigns ownership to the common distribution layer, and the user-management half is now explicit: fixed declared owner account, `users.mutableUsers = false`, all mutation is state-class writes ([ADR 0017](adr/0017-first-run-provisioning.md) §1); networking = standard NetworkManager with the OOBE Wi-Fi step as its only spatial touchpoint (first-run-onboarding §4.3); logging/diagnostics = standard journald + the B1b diagnostic target |
| Update *policy* surface (channel selection, auto-update consent UI) | policy+pres | none decided | none-yet | **missing** | no doc found; images-and-updates.md covers transaction mechanics only |

## 7. Build plane

Build-time components — Nix derivations and modules; never on the device runtime path
(overview.md invariant 1). Placement is `build-time` throughout; "Impl" notes what is scaffolded
versus stubbed in the tree today.

| Component | Protocol seam | Status | Evidence / Impl |
|---|---|---|---|
| Donor pipeline (acquire→identify→parse→extract→qualify) | donor manifest schema; per-stage derivations | **specified** | [donor-pipeline.md](donor-pipeline.md); impl: [lib/donor](../../lib/donor/default.nix) remains a typed stub, but the **first real donor is through the stages manually** — Steam Frame payload byte-verified, inventoried, manifested ([33](../research/33-steam-frame-donor.md), [devices/valve-steam-frame/donor.nix](../../devices/valve-steam-frame/donor.nix)) |
| Donor contracts (reviewed, hash-bound; null-propagation gating) | `contracts/*.json` | **specified** | donor-pipeline.md §qualify; none authored yet |
| Device contract option library | `spatial.*` typed options + assertions | **specified** | [device-contract.md](device-contract.md); impl: [lib/contract](../../lib/contract/default.nix) implements the core **and** (2026-09-23 catch-up, §10.2) `spatial.kernel.{source,structuredExtraConfig,configFile,dtbs}`, `spatial.xr.monado.*`, `spatial.xr.tracking.slam.package`, `spatial.xr.calibration.paths`, `spatial.qualification.readinessCheck` + tier assertion, with eval tests; remaining doc-only: `spatial.deployment.partitions` (owned by the Steam Frame workstream shaping `deployment.*`); soc naming aligned (§10.1) |
| Kernel build + two-phase kconfig contract check | `buildLinux`; eval-time + realization-time checks | **specified** | device-contract §kernel (IFD forbidden, lazy per-device checks); not implemented |
| Image assembly (`uefi-rauc` repart, `android-bootimg` packer, dev-vm) | `image.modules` deferred modules | **specified** | [images-and-updates.md](images-and-updates.md); impl: `devVm` + **`uefi-rauc` implemented Frame-scoped** ([families/uefi-rauc](../../families/uefi-rauc/default.nix): repart GPT, donor-mirroring labels, systemd-boot A/B entries) and **VM-boot-proven** ([33 §9](../research/33-steam-frame-donor.md)); android-bootimg/installer-usb remain stubs |
| Update bundle generation (RAUC+casync; Android slot images) | RAUC bundle format; slot images | **specified** | images-and-updates §Updates; impl: test-signed RAUC bundle built + **A→B round-trip proven in the VM** incl. custom bootloader backend and mark-good ([33 §9](../research/33-steam-frame-donor.md)); casync/desync-seeded install + boot-time mark-good service are follow-ups ([33 §10](../research/33-steam-frame-donor.md)) |
| Reference-free flashing bundle + installer | install manifest + bare-tool flash script | **partial** | images-and-updates §flashing bundle specifies the bundle; the unlock-to-flash install state machine is gated on the Lynx spike (design-backlog #3) |
| Adaptation backend modules (native / android-backed / device-specific) | per-subsystem NixOS modules | **partial** | ADR 0003 + device-contract §adaptation; impl: [modules/adaptation](../../modules/adaptation/default.nix) wires `native` only, warns on the rest; concrete backend interface gated (design-backlog #11) |
| android-compat building blocks (libhybris, headers, late-LXC) | per-subsystem blob closures | **partial** | [modules/adaptation/android-compat](../../modules/adaptation/android-compat/default.nix) is an intentionally empty placeholder; design in [research/03](../research/03-android-compat.md) §9-§10 |
| Flake outputs + `spatialSystem` + checks | flake API ([repo-structure.md](repo-structure.md)) | **specified** | impl: [flake.nix](../../flake.nix), [lib/eval-device.nix](../../lib/eval-device.nix), `devices/virtual-headset` builds and passes `nix flake check` |
| Patch management + lockfile-driven input pinning | `patches/<upstream>/`; nvfetcher/Renovate automation | **partial** | repo-structure §patch management + §pinning specify the rules; no automation tooling exists yet |
| Qualification evidence + tier CI | signed qualification records | **partial** | device-contract §tiers encodes tiers; evidence classes/freshness gated (design-backlog #9) |
| Reproducibility protocol (two-builder byte comparison) | attestations | **partial** | overview invariant 7 + images-and-updates §Reproducibility name it; protocol design gated (design-backlog #15) |
| Authenticated donor-acquisition CLI | out-of-store CLI + redacted lock records | **partial** | gated design (design-backlog #12) |
| Avatar enrollment tool exporter contract | normative exporter schema + validator + conformance set | **partial** | enrollment is explicitly *not* an OS component (avatar-persona §decomposition); the OS-owned exporter contract is pre-implementation work (avatar backlog #17) |

### 7.1 Contract-namespace → component map

Every `spatial.*` namespace and the component whose configuration surface it is:

| Namespace | Component (plane) |
|---|---|
| `spatial.device.*` | device identity/tiering — contract library + CI (build) |
| `spatial.hardware.{soc,displays,panel}` | declared hardware facts — adaptation + image assembly (build) |
| `spatial.hardware.ipd.*` | eye-tracking/IPD service + greeter default (perception; system) — ADR 0011 |
| `spatial.donor` | donor pipeline (build) |
| `spatial.kernel.*` | kernel build + kconfig contract gate (build) |
| `spatial.adaptation.{display,gpu,camera,sensors,audio,wifiBt,tracking}` | per-subsystem hardware backends (build → runtime services) |
| `spatial.adaptation.eyes` | eye-camera backend for the ET service (perception) — ADR 0011 |
| `spatial.xr.runtime`, `spatial.xr.compositor.backend`, `spatial.xr.environment` | Monado runtime wiring (perception) — modules/xr |
| `spatial.xr.monado.*`, `spatial.xr.tracking.slam.package` | per-device XR driver + VIT tracker packaging (build → perception) |
| `spatial.xr.shell` | compositor/session selection: zxr / stardust / wayvr (authority) — ADR 0006 |
| `spatial.xr.session.*` | profiles, greeter, lock policy (system + authority) — ADR 0007 |
| `spatial.xr.passthrough.*` (incl. `handCutout.*`) | passthrough + hand-cutout services (perception) — ADR 0008 |
| `spatial.xr.mapping.*` (`enable`, `depthAssist`, `persistence`, `boundary`) | mapping/anchor, geometry, boundary services (perception) — ADR 0009 |
| `spatial.xr.sensing.*` | declared sensing facts feeding the avatar driver's ladder (perception) — ADR 0010 |
| `spatial.xr.avatar.enable` | avatar driver + runtime (perception + shell) — ADR 0010 |
| `spatial.xr.calibration.paths` | per-unit calibration state (system) |
| `spatial.deployment.*` | image families, flashing, protected partitions (build) |
| `spatial.qualification.*` | acceptance/readiness checks (build; readiness consumed by the update agent at runtime) |

Note the asymmetry this table exposes: **every perception-plane service has a contract
namespace; no shell-plane or service-plane component has one.** There is no
`spatial.desktop.*`/`spatial.shell.*` options surface for launcher, notifications, portals,
audio policy, or settings — consistent with those rows being missing, and a concrete signal of
where the contract must grow.

## 8. The gap list (all rows with status = missing)

Collected from the tables above; each names its nearest existing precedent so the eventual design
starts from evidence rather than zero.

1. **RESOLVED as designed (2026-09-23).** The spatial-workspace/space model is specified:
   [places-model.md](places-model.md) + [ADR 0016](adr/0016-places-model.md) (typed frame graph,
   decomposed currency answering "active when switching is a walk", lifecycle, no second axis).
   Row moved to **specified** in §3; remaining open items (place volumes, multi-user ownership,
   vehicle frames, extension XML) tracked in places-model §9. Implementation unbuilt.
2. **Focus/activation authority + xdg-activation** (authority/service, §3/§6) — composition §7.3
   requires "focus/activation" with no design; launcher and notification flows are blocked on it.
3. **App launcher** (shell) — no doc found.
4. **Task switcher** (shell) — no doc found; also blocked on global shortcuts (mechanism).
5. **Pager/overview** (shell) — blocked on gap 1.
6. **Panels / status surfaces** (shell) — no doc found.
7. **OSD framework** (shell) — no doc found.
8. **Notifications, both halves** (service + shell) — no spec service, no spatial presentation;
   today notifications exist only as a hypothetical leak in spectate streams (spatial-sharing §6).
9. **Settings daemon + configuration model** (service) — *contract closed
   ([specs/settings-schema.md](../../specs/settings-schema.md) rev 2)*: schema artifact, storage
   strata, bus interface, migrations. Remaining gap: the daemon's own process design (spec §10).
10. **Polkit agent** (service) — polkit is *enabled* (modules/os) with no agent to present
    prompts; privilege escalation in-session is currently a dead end.
11. **Secrets/keyring** (service) — no doc found.
12. **Input-method framework** (service) — the mechanism half of text entry; ADR 0007's lock PIN
    pad + virtual-keyboard fallback is the partial precedent to generalize.
13. **Global shortcuts** (service/authority) — no doc found.
14. **Default apps / MIME** (service) — no doc found.
15. **Accessibility** (service/shell) — nothing exists in any document.
16. **Audio policy/routing + spatial audio** (service) — only the hardware backend fact
    (PipeWire) exists; no session-manager policy, no HRTF/spatialization design.
17. **Power/thermal policy** (service) — idle/doff is compositor-owned (specified); system
    suspend sequencing, thermal, and performance profiles have no owner.
18. **Display/runtime configuration** (service) — hardware facts are declared
    (device-contract); runtime render-scale/refresh/FOV configuration has no component.
19. **Recentering/reference-space UX ownership** (service/authority) — the OpenXR event is
    reserved (spatial-mapping §3); no owner for the user-facing action.
20. **Boundary setup UX** (shell) — the boundary *system* is specified (spatial-mapping §7); how
    a user draws/edits a play area is not.
21. **Non-capture portal interfaces** (service) — FileChooser/OpenURI/Settings etc.; docs 17–19
    design only the capture/remote-desktop tier.
22. **Update policy/consent surface** (service) — transaction mechanics are specified
    (images-and-updates); channel selection and user consent UX are not.
23. **Notification-adjacent partials worth flagging with the gaps:** the in-space consent
    picker, share badges, decoration chrome, clipboard/DnD-in-3D, and the sharing service
    (§5/§6, status partial) are one design pass away from missing — they are *named as
    obligations* by the sharing/security invariants but have no owning design.

Added by the terminology-trap review (see [desktop-environment.md §2](desktop-environment.md) and
doc 30's addenda for the evidence):

24. **Colour pipeline / HDR** (authority, §3) — `color-management-v1`/`color-representation-v1`
    are staging protocols with shipped KWin/Mutter precedent; XR-amplified (panel calibration,
    sRGB/linear composition, passthrough colour matching); no owner.
25. **Effects/animation module** (authority, §3) — every mature compositor names this subsystem;
    in XR sudden large-surface motion is a comfort/safety concern; no owner.
26. **Application session restoration, both halves** (authority §3 + service §6) — the
    `xdg-session-management-v1` mechanism and the relaunch-owning restore manager; the deepest
    XR amplification (places persist placement; nothing relaunches apps into them).
27. **SNI host** (shell, §5) — StatusNotifierItem compatibility, hosted in the panel per
    ADR 0012's decision; presentation design missing.
28. **Docked desktop mode, both rows** (authority §3 + system §2) — the docked
    flat-composition output path and the quiescence-ladder policy; architecture decided by
    ADR 0015 (B3 + mirror tier, fact-gated on `spatial.hardware.externalDisplay`), component
    design missing.

(The same review upgraded two implicit subsystems to explicit **partial** rows in §3: the scene
graph and decoration policy.)

## 9. Spin-out candidates (input to ADR 0012 — no decisions here)

Components whose policy/presentation could plausibly be modular (separately replaceable,
separately shipped), with what the registry evidence says about their current coupling:

- **Workspace/space-model presentation** (pager/overview, §5): the model itself is missing
  (gap 1), so the seam can still be drawn cleanly; the sharing placement graph (spatial-sharing
  §5) already treats placement state as replicable data, which argues the model can live behind
  a state-sync-shaped interface rather than as compositor internals.
- **Task-switcher UI** (§5): missing today; the mechanism/policy/presentation split in §1 was
  chosen with exactly this row in mind — interception and activation must stay authority-plane,
  the switcher UI need not.
- **Window-management policy** (§3, partial): currently assumed in-compositor; the open
  "2D-toplevel → initial 3D placement" question (composition §8) is precisely a policy seam that
  could be externalized before it calcifies.
- **Decoration renderer** (§5, partial): server-side decorations are already *forced* at the
  protocol level (research/19 §8), which concentrates chrome in one place — either a compositor
  subsystem or a delegated renderer; nothing yet couples it irreversibly.
- **Notifications** (§6/§5, missing): green-field; the spec service is a plain D-Bus daemon by
  nature and the spatial presentation could be a privileged client — but "privileged client"
  machinery (beyond the sharing/security-context policy engine) does not exist yet either.
- **Portal backend** (§6, specified): already designed as a separate daemon following the xdpw
  shape (research/17 §8) — the most spin-out-ready component in the registry; the one coupling
  question is the in-space chooser (in-process vs Mutter-style private API, research/17 §11.4).
- **Counter-signal to record:** the lock scene, greeter scene, and boundary overlay are
  deliberately *not* candidates — ADR 0007 and spatial-mapping §7 bind them to the compositor
  for enforcement-invariant reasons (I1–I3; breach response without client cooperation), and the
  registry should not re-open that.

## 10. Known cross-document placement divergences

Places where existing documents disagree (or have drifted) about where a component lives or what
it is called — recorded here so the registry is honest about its sources; resolving them belongs
to the owning docs, not this index.

1. **RESOLVED (2026-09-23).** `spatial.soc` vs `spatial.hardware.soc`: doc aligned to the
   implemented contract — [device-contract.md](device-contract.md) now says
   `spatial.hardware.soc`.
2. **MOSTLY RESOLVED (2026-09-23).** The doc-only contract options are implemented in
   [lib/contract](../../lib/contract/default.nix) with eval tests:
   `spatial.kernel.{source,structuredExtraConfig,configFile,dtbs}`, `spatial.xr.monado.*`,
   `spatial.xr.tracking.slam.package`, `spatial.xr.calibration.paths`,
   `spatial.qualification.readinessCheck` (+ tier assertion). Deliberately left to its owner:
   `spatial.deployment.partitions` — the Steam Frame workstream is actively shaping
   `spatial.deployment.*`; theirs to add.
3. **RESOLVED (2026-09-23).** [ADR 0008](adr/0008-perception-services-placement.md) recast to the
   ADR 0010 corrected framing: ownership + boundary decided (Monado owns cameras/clock/
   calibration; compositor consumes finished per-eye dmabuf layers); per-service *execution*
   placement fixed at implementation against exactly one specified boundary (in-process sink ABI
   + crash containment, or adjacent process on a versioned frame+pose relay). Perception backlog
   #5/#7/#8 phrasing satisfied.
4. **RESOLVED (2026-09-23).** Consent-picker placement decided in
   [spatial-sharing.md](spatial-sharing.md) §2: chooser owned by the portal backend, presented as
   a separate privileged layer-shell client; Mutter-style private compositor-API chooser
   rejected (presentation never enters the authority plane). Row stays `partial` (UX design open).
5. **RESOLVED (2026-09-23).** The sharing service is `spatial-sharingd` —
   [spatial-sharing.md](spatial-sharing.md) §6 invariant 4: separate service-plane session daemon
   (D-Bus, socket-activated) owning share lifecycle/consent; authorizes observer views via a
   privileged compositor API; the mode-3 data path stays in-compositor. Row stays `partial`
   (API design open).
6. **WiVRn's role** was a genuine contradiction (runtime enum value vs not-a-runtime) and is
   already resolved: `wivrn` models the optional streaming-server role only
   ([design-backlog.md](design-backlog.md) #16, device-contract §xr).

## 11. Registry totals

Counts by status (rows in §2–§7 tables):

| Plane | specified | partial | missing | total |
|---|---|---|---|---|
| System | 14 | 2 | 1 | 17 |
| Authority | 22 | 5 | 6 | 33 |
| Perception | 17 | 6 | 0 | 23 |
| Shell | 3 | 8 | 8 | 19 |
| Service | 2 | 3 | 15 | 20 |
| Build | 7 | 8 | 0 | 15 |
| **Total** | **65** | **32** | **30** | **127** |

The shape is stark and expected: the authority and perception planes are deeply specified (the
ADR work to date), the build plane is specified-but-stubbed by deliberate policy (the Lynx-spike
standing rule, [design-backlog.md](design-backlog.md)), and the desktop-environment surface —
shell presentation and the service plane — is where nearly everything is missing. The dependency
structure among all of it is mapped in [desktop-environment.md §6](desktop-environment.md).
