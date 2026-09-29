# ADR 0006: XR compositor strategy — revive the zxr lineage as `zxr-shell-v2`, Wayland-native, on Monado (amended 2026-09-29: the container pair)

**Status:** accepted (draft); **base library ratified 2026-09-23** (Rust + smithay, see §The
compositor base; evidence in [39-compositor-base-landscape](../../research/39-compositor-base-landscape.md));
**amended 2026-09-29 (Amendment 4, below): the 3D-client contract is `XR_EXT_spatial_container`
+ `_self_rendering` implemented in Monado; `zxr-shell-v2` is retired to a reserved hook.**
**Date:** 2026-09-22 (amended 2026-09-23; amended 2026-09-26 — §Program shape, below; amended
2026-09-29 — Amendment 4)
**Context sources:** [08-wxrc](../../research/08-wxrc.md) (Motorcar→wxrc→wxrd lineage + code),
[09-wxrc-ecosystem-gap-2026](../../research/09-wxrc-ecosystem-gap-2026.md) (2026 patch archaeology),
[10-xr-wayland-protocol-comparison](../../research/10-xr-wayland-protocol-comparison.md) (five-model
comparison), [05-xr-userspace](../../research/05-xr-userspace.md) (Monado runtime; StardustXR's
missing in-tree compositor). Relates to [adr/0005](0005-flake-layout-and-outputs.md) (nixpkgs-xr as
input) and the open 2D-app question in [05-xr-userspace §11](../../research/05-xr-userspace.md).

## Context

Mura needs an XR compositor/shell: the component that puts multiple applications into one
shared 3D space on a headset, keeping unmodified 2D Wayland apps first-class (the Motorcar
philosophy, [08](../../research/08-wxrc.md) Part 1 §1.1). The user co-authored the original
`zxr_shell_unstable_v1` protocol for wxrc (2019, with Drew DeVault and Simon Ser) and wants to
continue that work; it is intended to be the backbone of the Mura compositor.

The research established five candidate architectures ([10](../../research/10-xr-wayland-protocol-comparison.md)
§2), spanning "no new protocol" to "leave Wayland entirely":

- **motorcar** (2014, QtWayland): the ancestor — view-dependent depth-composited 3D windows,
  cuboid/portal clipping, 6DoF input. Dead since 2015.
- **zxr / wxrc** (2019, wlroots+OpenXR): motorcar re-expressed for OpenXR — N views, typed
  pixel/depth buffers, `wl_buffer` transport — but with input, clipping, timing, and size
  negotiation dropped. wxrc froze 2021; it is a ~3,460-line prototype on wlroots 0.8, GLES2+EGL,
  Monado-only, with the zxr depth path defined but **not actually wired** ([08](../../research/08-wxrc.md) §2.1).
- **wxrd** (2023, Collabora): the documented descendant — but it **dropped zxr entirely** and became
  "2D windows in VR via xrdesktop/gxr," wlroots 0.15, prototype-quality
  ([08](../../research/08-wxrc.md) §2.3).
- **zwin** (2022): the opposite philosophy — a serialized GLES 3.2 command stream executed by the
  compositor (server-side rendering). Powerful but a GPU-process-weight compositor; dormant since
  2023/2024 ([10](../../research/10-xr-wayland-protocol-comparison.md) §2.3).
- **StardustXR** (active 2026): leaves Wayland for a scene-graph IPC; 2D delegated to a client
  compositor (Flatland). The strongest *running* 3D platform, but non-Wayland
  ([10](../../research/10-xr-wayland-protocol-comparison.md) §2.4).
- **WayVR** (most active, 2026): zero new protocol — an embedded smithay compositor textured into
  flat `XR_EXTX_overlay` quads. Proves the 2D tier needs no protocol, but is an overlay accessory,
  not a session ([10](../../research/10-xr-wayland-protocol-comparison.md) §2.5).

Two research findings are decisive:

1. **The 2019 patch burden is mostly gone** ([09](../../research/09-wxrc-ecosystem-gap-2026.md)):
   DRM leasing (`wp_drm_lease_v1`), Monado's EGL binding + Wayland direct mode, wlroots' GLES2
   texture access, Xwayland lease bridging, and Sway integration all LANDED; the Mesa/Vulkan
   Wayland-lease WSI patches were SUPERSEDED by the ratified `VK_EXT_acquire_drm_display`. No
   seven-fork upstream effort remains.
2. **The real work is a compositor rewrite, not a port** ([08](../../research/08-wxrc.md) §2.5,
   [09](../../research/09-wxrc-ecosystem-gap-2026.md) §4): every wlroots API wxrc uses predates the
   0.11+ scene-graph/renderer/allocator/buffer rework and is gone in 0.19.

## Decision

**Continue the zxr lineage as `zxr-shell-v2`: a Wayland-native, client-renders /
compositor-composites XR shell protocol, implemented in a new compositor that is itself an OpenXR
client of Monado.** The compositor serves ordinary `xdg-shell` for 2D apps and `zxr-shell-v2` for
3D apps, compositing both into one depth-tested space.

This is **option 4** from the plan (a new protocol informed by zxr + zwin + motorcar), implemented
in a fresh compositor — explicitly **not** a port of the wxrc codebase (option 1), **not** a revival
of wxrd (option 2), and **not** adopting StardustXR's substrate (option 3). StardustXR and WayVR are
instead **packaged as optional alternative sessions** ([adr/0005](0005-flake-layout-and-outputs.md),
[05 §9.7](../../research/05-xr-userspace.md)).

### The protocol: `zxr-shell-v2` (superseded by Amendment 4, 2026-09-29 — the XML stays as a retired reserved hook)

Per [10 §4.4](../../research/10-xr-wayland-protocol-comparison.md), refill the zxr skeleton with the
flesh motorcar had and the mechanisms 2026 provides:

- **Keep from zxr:** N `zxr_view` globals (add the missing resolution/fov events); typed per-view
  pixel/depth composite buffers; the XR-surface role with a 2D-buffer-attach protocol error.
- **Restore from motorcar:** the **view/projection/model matrix split** (compositor owns
  view+projection per view; client owns per-surface model transform) replacing zxr's folded
  per-surface-view MVP; **cuboid/portal clipping modes**; **3D size negotiation**
  (configure/ack_configure with a serial, borrowing zwin-shell's idiom); **6DoF pointer input**
  (surface-local position + orientation).
- **Borrow from zwin:** a **`wl_pointer`-shaped ray input device** (enter/leave/motion/button/axis
  + frame) as a second seat capability — rays are what controllers and hand-pinch produce; keep
  hit-test geometry compositor-derived, not client-uploaded.
- **Borrow from StardustXR (mechanism only):** **explicit sync via `wp_linux_drm_syncobj_v1`** on
  both pixel and depth buffers, with **linux-dmabuf** as the primary transport — this retires
  Motorcar's depth-viewport-packing hack ([08 §1.5](../../research/08-wxrc.md)); the client exports
  its real depth attachment as a dmabuf, no Mesa fork ([09 §5](../../research/09-wxrc-ecosystem-gap-2026.md)).
- **Add:** first-class **frame timing** mapping `xrWaitFrame`/`XrFrameState.predictedDisplayTime`
  onto a per-surface XR frame event carrying the predicted-display-time view matrices, plus
  `wp_presentation`-style feedback; and **free 2D** — an `xdg_toplevel` appears as a quad with no XR
  interface at all (compositor policy places it, thesis §6.1.2.2).

The protocol lives in-tree initially (`zxr-shell-v2.xml`), with an explicit intent to propose it to
`wayland-protocols` staging once it stabilizes — the user's authorship and the fact that the
original was reserved in the OpenXR/registry process ([09 §2](../../research/09-wxrc-ecosystem-gap-2026.md))
make upstreaming a realistic goal, not a fork.

### The renderer: Vulkan, not GLES2+EGL

wxrc's GLES2 + `XR_MNDX_egl_enable` path ties the compositor to Monado and to a GL interop Vulkan
never needed ([08 §2.5](../../research/08-wxrc.md), [09 §2](../../research/09-wxrc-ecosystem-gap-2026.md)).
Target **Vulkan with `XR_KHR_vulkan_enable2`**: Khronos-ratified (not the provisional MNDX
extension), matches Monado's native Vulkan path and the rest of the Mura XR stack
(StardustXR's `dmatex` is Vulkan+dmabuf+syncobj), and provides the external-memory/modifier/
sync primitives the dmabuf depth path needs. GLES2+MNDX remains available only as a throwaway
bring-up shortcut, never the production target.

### The compositor base: **ratified — Rust + smithay** (amendment, 2026-09-23)

The original decision deferred the base to the D2 wxrc-port sizing spike plus a smithay
evaluation, with four open checks. Both halves now exist: **D2 was run** ([09 Appendix
D1/D2](../../research/09-wxrc-ecosystem-gap-2026.md)) — wxrc does not even preprocess against
wlroots 0.19.3, confirming total-rewrite and removing any port-sizing argument for C — and the
smithay evaluation was done as a code study of pinned clones,
[39-compositor-base-landscape](../../research/39-compositor-base-landscape.md), where all four
checks pass:

1. **Vulkan renderer integration:** smithay's Wayland frontend contains zero references to its
   `Renderer` trait (grep-verified); the one bridge worth keeping (`on_commit_buffer_handler`,
   which owns wl_buffer release *and* syncobj release-point signalling) is renderer-free. zxr's
   ash renderer plugs in without fighting the library (doc 39 §1.1–§1.2).
2. **DRM leasing:** implemented lessor-side (`wayland::drm_lease`), with VR named as the use
   case in the module docs; needed only on the desktop/dev profile (doc 39 §1.5).
3. **The OpenXR/`ash` boundary:** openxrs wraps `XR_KHR_vulkan_enable2` end-to-end
   (`create_vulkan_instance`/`vulkan_graphics_device`/`create_vulkan_device`) on the same ash
   major smithay uses; WayVR proves the runtime-created-device shape on a real headset
   (doc 39 §1.9, §2).
4. **Explicit sync:** `wayland::drm_syncobj` is implemented with sync-file import/export APIs
   whose doc comments explicitly anticipate a Vulkan-driven compositor (doc 39 §1.3).

**Decision: Rust + smithay**, pinned to a git rev with `default-features = false` (the niri /
cosmic-comp convention), features `wayland_frontend`, `backend_drm`, `backend_vulkan`,
`desktop`, `xwayland`. C FFI where it counts — Monado/OpenXR via openxrs (`openxr`-sys),
`libwayland` where needed — via the usual `-sys` bindings. Known frictions, accepted and
recorded (doc 39 §1.11): a render node must be opened even though zxr never touches KMS on the
headset path; smithay's winit backend is GLES-coupled, so the windowed dev mode drives winit +
the ash swapchain directly; the documented acquire model is CPU-side blockers (GPU-side
semaphore waits available via `export_sync_file`).

**Fallback (recorded, with trigger):** wlroots 0.21-dev. Its headline advantage dissolved on
inspection — the built-in Vulkan renderer cannot adopt an externally (OpenXR-) created device
without patching private ABI, so a fallback recovers protocol/seat/Xwayland plumbing but not the
render path (doc 39 §3). The fallback triggers only on a **structural** smithay failure during
bring-up (a protocol-frontend defect unfixable without forking) — never on effort overrun.

**No further base spikes remain**: D1/D2 are closed with recorded results (doc 09 appendix), and
wxrc stays a design reference. The **R0 bring-up spike** (doc 39 §5) is re-scoped from decision gate to
**risk-retirement**: it is the first code milestone and must retire the integration risks
(projection-layer presentation, zero-copy dmabuf + explicit sync end-to-end, window churn,
Xwayland) against measured gates, but it cannot change the base choice. The protocol and the
client-rendered / Monado-client / Vulkan decisions above were always base-independent, and the
ratification does not alter them.

### Sequencing: ship the 2D tier first (the "then the 3D-native tier" half superseded by Amendment 4 — the 3D tier is Monado's container pair, implementation-path.md §3 C-track)

Per [10 §4.4](../../research/10-xr-wayland-protocol-comparison.md) and WayVR's evidence, the
2D-panels-in-XR tier needs no new protocol and is independently useful. Ship it first (xdg-shell
quads composited by Monado, on the chosen base), then layer the `zxr-shell-v2` 3D-native tier onto
the same compositor. This de-risks the project: a usable 2D XR workspace exists before the novel 3D
protocol is finished, and it directly closes the open 2D-app question in
[05 §11](../../research/05-xr-userspace.md).

## Consequences

- A new module surface is added: `mura.xr.shell` selecting the compositor/session
  (`zxr` = the Mura compositor | `stardust` | `wayvr` | `none`), and a compositor backend
  option. [overview.md](../overview.md) and `modules/xr/` are updated to reflect that the shell/2D
  path is now a defined layer, not an open question.
- The reusable assets from the lineage are the **zxr protocol design** and the **thesis philosophy**
  ([08](../../research/08-wxrc.md) Part 1) — not the wxrc or wxrd code, which are design references.
- The engineering program is the four items from [09](../../research/09-wxrc-ecosystem-gap-2026.md)'s
  bottom line: (1) the compositor on smithay (ratified above), (2) the Vulkan renderer, (3) the
  `zxr-shell-v2` depth/timing/input protocol, (4) hardware qualification across AMD/Intel/NVIDIA and
  real HMDs — sequenced by the boot-forward ladder in
  [implementation-path.md](../implementation-path.md) (the R0 bring-up spike opens it; the
  display-path feasibility test that [overview.md](../overview.md) names as the first XR
  experiment remains the first *hardware* gate).
- `wp_drm_lease_v1` is consumed by Monado on the desktop/dev profile and bypassed by `VK_KHR_display`
  on the appliance ([10 §1](../../research/10-xr-wayland-protocol-comparison.md)); the new protocol
  needs nothing from it.
- Open protocol questions are carried in [10 §4.5](../../research/10-xr-wayland-protocol-comparison.md)
  (frame pacing across clients, 2D-toplevel→3D mapping, whether geometry ever crosses the wire, depth
  trust/clipping, and the depth-dmabuf driver matrix).
- The concrete composition model (renderer-agnostic opaque colour+depth "sort-last" baseline, the
  transparency/reprojection/light-transport tiers, the renderer-agnostic GL/Vulkan/CPU transport,
  first-class 2D windows, and the MVP milestones) is worked out in
  [zxr-shell-v2-composition.md](../zxr-shell-v2-composition.md), which also records the verification
  of the 2026 OpenXR spatial-container / depth-test-layer claims that bear on this decision.
- **The protocol itself is drafted**: [`protocols/zxr-shell-v2.xml`](../../../protocols/zxr-shell-v2.xml)
  (specification workstream; drafting brief in [08 Part 3](../../research/08-wxrc.md) — v1's
  folded `mvp_matrix` and `get_wl_buffer` wrapper dropped, motorcar's matrix split and clipping
  resurrected, atomic frame snapshots per the composition doc's contract), validated by the
  `checks.protocols` scanner gate.
- Sharing (spectate / 2D window / per-observer 3D / protocol-proxied remote+VM apps / workspace
  join) is designed in [spatial-sharing.md](../spatial-sharing.md), grounded in research docs
  [17](../../research/17-sharing-capture-stack.md)–[19](../../research/19-wayland-proxying.md); it
  adds the proxied-client globals baseline and security-context requirement to this compositor's
  scope and treats remote observers as authorized `zxr_view`s.

## Alternatives considered

- **Port wxrc as-is (option 1):** rejected — it is a rewrite regardless
  ([08 §2.5](../../research/08-wxrc.md)); the code is a design reference, not a base.
- **Revive wxrd (option 2):** rejected — it dropped the zxr 3D-windowing protocol, routes through a
  heavy xrdesktop/gxr stack, and is a stale 2023 prototype ([08 §2.3](../../research/08-wxrc.md)).
- **zwin server-side rendering:** rejected — GPU-process-weight compositor, capped client rendering,
  dormant ecosystem ([10 §4.2](../../research/10-xr-wayland-protocol-comparison.md)).
- **StardustXR substrate:** rejected as the *backbone* — non-Wayland contradicts the Part 1
  philosophy and delegates 2D to a second compositor; kept as a packaged optional session.
- **WayVR only:** rejected as the *backbone* — an overlay with no shared 3D space or 3D apps; but
  adopted as the model for the first (2D) tier and packaged as-is.
- **Adapt an existing DE (KWin VR fork):** not evaluated when this ADR was written; evaluated
  post-decision in [ADR 0013](0013-kwin-vr-disposition.md) on a code-level study
  ([31-kwin-vr](../../research/31-kwin-vr.md)) — rejected as the backbone (the Qt Quick 3D XR
  substrate forecloses the client-depth 3D tier; five-upstream patch carry; desktop-first session
  model), but adopted as a **design donor** for this ADR's 2D tier (the five WM-core seams, the
  XR-preflight and dmabuf-format-filter patterns) and reserved as an optional session
  (`mura.xr.shell = kwin-vr`). Its topology — one process, one projection layer, ray→plane
  input, zero-copy dmabuf — independently validates this ADR's shape at daily-driver quality.


## Amendment 2026-09-26 — the program shape, from comparables with the lineage first

[research/59](../../research/59-xr-compositor-architecture-from-comparables.md) re-derived every
mechanism the compositor program must fix under AGENTS rules 7/8, reading **motorcar and wxrc**
— the lineage, held by the owner as the design centre — first and every other comparable as
evidence for or against. Results, recorded here so the program spec
([specs/zxr-core.md](../../../specs/zxr-core.md)) is built on decisions rather than drafts:

- **The model is confirmed, not re-opened.** Clients render; the compositor composites depth
  into one scene and submits one stereo projection layer. OpenXR composes layers by painter's
  algorithm "whether or not the new layers are virtually closer to the viewer" and Monado never
  depth-tests across layers, so the thesis's argument is stronger under OpenXR than it was.
- **Loop ownership — ruled (b), 2026-09-26.** The state loop (smithay's `calloop`) owns the
  thread; a dedicated thread blocks in `xrWaitFrame` and posts the `XrFrameState` into the loop.
  Why not the lineage's single loop (wxrc, wayvr): the spec intends the *runtime* to own the
  throttle through `xrWaitFrame` and expects pipelined applications to call it off their main
  thread ("intended to provide scalable performance when used on multiple host threads"; "a
  pipelined system may call xrWaitFrame on a separate thread"); the single-loop comparables give
  no reason for their choice, while every comparable with a latency reason — Qt Quick 3D XR's
  `WaitForFrame` worker (KWin-VR's engine), gamescope's `vrflip` thread, KWin's and mutter's
  display threads — moved the blocking wait off the state loop; and smithay's own explicit-sync
  design refuses to block the loop thread (waits become eventfd sources). Two threads, calloop
  used as designed, Wayland input handled between frames.
- **Determinations that close research/39's open R0 outputs:** no compositor-side windowed
  backend — Monado's simulated HMD in a desktop window is the development backend and
  `pkgs/dev-session` already runs it (winit vs direct swapchain: neither); **xwayland-satellite**
  for X11 (niri's reason — no global 2D coordinate system for X11 — holds a fortiori for planes
  in a frame graph, and wayvr chose the same), smithay `X11Wm` the recorded fallback; tracing
  spans plus a frame journal as instrumentation; frame callbacks after submit and never a wait
  on clients (motorcar's stated policy, every 2D compositor's practice, Monado's expectation).
- **Base re-affirmed with the lineage's own reasons:** motorcar chose QtWayland because it
  "handles almost all of the behavior needed to correctly interact with 2D clients" and isolates
  the 3D work — smithay's renderer-free frontend is that today; wxrc's hand-tracked wlroots 0.8
  pin no longer preprocesses against 0.19. Rust + smithay pinned to a git rev with
  `default-features = false` (niri's convention); wlroots fallback only on a structural
  frontend defect. Budgets measured (niri 35.5 MB / 30 MB / 1 thread; cosmic-comp 115 MB / 28;
  gamescope 144 MB / 17) set the fence in the spec.

## Amendment 2 (2026-09-26) — the composition path: quads always, the projection layer only with depth content

**Ruled by the owner, 2026-09-26**, on [research/65 §2](../../research/65-embedded-frame-path-efficiency.md)
(the fork brought under AGENTS rule 8 with the comparables' three positions).

- **Every 2D window reaches the display as a runtime composition layer** (`XrCompositionLayerQuad`,
  cylinder later), one per window, its content rendered by zxr into a runtime-owned panel
  swapchain **only when the window's surface tree commits**. The runtime re-samples the panels
  every display frame at the display pose.
- **zxr submits its projection layer only while something needs depth**: a mapped 3D process
  (zxr-shell-v2 volume), the environment (passthrough) layer, the foreground (hand cutout)
  layer — or panel overflow past the runtime's layer cap. **A windows-only session has no
  projection layer**: zxr does no GPU work and makes five runtime round trips per tick instead of
  eleven.
- **Why.** On Monado the two paths cost the same number of full-resolution passes (one
  projection layer takes the distortion fast path; N quads take the layer squasher then
  distortion — `comp_compositor.c:272-303`, `comp_render.h:64-68`), so the projection path buys
  no bandwidth; what it costs is that zxr must re-render every frame the head moves. Measured on
  the dev host with a static client under head motion: quads −47 % zxr CPU, −48 % wake-ups,
  0 GPU, Monado's cost flat (research/65 §2.3). The OpenXR spec's own recommendation for UI is
  the quad layer — "a better match between the resolutions of the XrSwapchain image and
  footprint of that image in the final composition … improves legibility … allows optimal
  sampling during any composition distortion corrections" (`rendering.adoc:1223-1230`) — and the
  one XR comparable that ships desktop-like panels (wayvr/wlx-overlay-s) does exactly this
  (`backend/openxr/overlay.rs:133-199`), re-submitting a stale swapchain image when nothing
  changed (`mod.rs:406-424`).
- **What the model keeps.** The thesis's model — clients render, the compositor composites depth
  — is untouched for what has depth: 3D volumes, the environment and the cutout are composited by
  zxr in its projection layer exactly as the first amendment states. What changes is that flat
  panels, which have no depth of their own, are handed to the runtime's compositor instead of
  being rasterised twice.
- **Accepted consequences, stated so they are not rediscovered:** (1) a copy per commit — OpenXR
  swapchain images are runtime-allocated (`comp_swapchain.c:693-704`), so a Wayland client's
  buffer can never *be* a panel image; import stays zero-copy, the panel pass is the one designed
  copy and the journal counts it; (2) painter's order only, between windows and between windows
  and the projection layer — quads always composite over depth content (`rendering.adoc:1143-1147`);
  a window a volume should occlude cannot be, which is spec §14's M2 item (candidate rule: a
  window whose quad intersects a volume is drawn in the projection layer that frame); (3) **the
  hand cutout is a runtime layer submitted after every quad — hands composite above all
  windows** (ruled 2026-09-26; painter's order is the mechanism, and Monado alpha-blends every
  layer by its source alpha, premultiplied or not — `render_gfx.c:409, 767-776`,
  `comp_render_gfx.c:236, 848-867`). Its *shape* is open, recorded in
  [perception-passthrough-hands.md](../perception-passthrough-hands.md) §1a: a view-aligned
  cutout projection layer at reduced resolution, per-hand billboard quads at the hand's depth,
  or depth-correct ordering by drawing intersecting windows in zxr's projection layer — decider:
  the owner, at the passthrough rung, on measured edge quality and bandwidth; (4) the runtime's layer cap
  (`XrSystemGraphicsProperties::maxLayerCount`; Monado 128 on Linux, 32 on Android,
  `xrt_limits.h:80-89`) bounds the panel count — the nearest panels get layers, the rest fall
  into the projection layer for that frame.
- Spec: [specs/zxr-core.md](../../../specs/zxr-core.md) rev 3 §4, §6.2, §7, §12, §14.

## Amendment 3 (2026-09-26) — native OpenXR applications beside zxr (DRAFT; the five forks ruled by the owner the same day)

From [research/66](../../research/66-native-openxr-apps-and-the-system-input.md) and
[native-openxr-apps.md](../native-openxr-apps.md), under the owner's framing that zxr is a
desktop environment's compositor and a native OpenXR application is what a fullscreen game is to
GNOME/KDE:

- **zxr's OpenXR session is an `XR_EXTX_overlay` session** so that a native application may be
  Monado's main session beside it — the shape of WayVR, kwin-vr, xrdesktop and Valve's Steam
  Frame shell (Steam's UI as the SteamVR dashboard overlay). "One OpenXR client of Monado" in
  this ADR and spec §1 is qualified accordingly: one *always-present* client. Ruled (Q-A):
  always the overlay session, never a role switch, with the owner's condition that yielding be
  "efficient and minimally taxing" — quiet mode's bound.
- **Quiet mode**: while a native application is primary zxr submits no layers (the unredirect
  analogue, with mutter's stated reason: "reduces the overhead for apps like games") and costs
  only the frame-loop IPC and the Wayland loop; it resumes for layer 5 always, for the layer-6
  hand cutout by default with a wearer toggle in the OSD (visionOS's default, ruled Q-D), for
  what the wearer summons, and for planes kept per window. **The game is VISIBLE, not FOCUSED,
  while the shell is up** (the spec's and every platform's rule; Monado `io_blocks` until a
  focus switch exists upstream).
- **The reserved system input** — one control per tier no application receives — is the
  compositor's non-maskable chord; it summons the shell; long press recenters; double press
  shows/hides or toggles passthrough; quit is a shell menu item plus a force chord (ruled Q-B).
  Carried by the HMD-body button where one exists, the controller system button with identical
  semantics (ruled Q-E), and on every tier a posture-gated held palm gesture — the owner's
  requirement being that no reserved gesture interrupt the experience (ruled Q-C).
- **Launch/close**: spawn as a systemd scope with the runtime environment; set primary over
  `libmonado`; `request_exit` then kill the scope. zxr's death does not take the game with it.
- **Upstream (Monado)**: reservation of `/input/system/click` for a system client; a real
  `set_focused_client`. Recorded on ADR 0013's upstream list shape.

## Amendment 4 (2026-09-29) — the container pair replaces `zxr-shell-v2` as the 3D contract; Monado composites; zxr is the workspace controller

**Context.** When this ADR was written the corpus believed no ratified multi-app contract existed
in OpenXR ([zxr-shell-v2-composition.md §6](../zxr-shell-v2-composition.md) called the
self-rendering half "fabricated"). At the pinned registry (OpenXR 1.1.63, 2026-09-01) both
`XR_EXT_spatial_container` (#811) and `XR_EXT_spatial_container_self_rendering` (#814) are
**ratified** (`references/openxr-docs/specification/registry/xr.xml:24456,24536`); Godot master
implements the client side (`references/godot/modules/openxr/extensions/spatial_container/`);
Google's Android XR runtime — Monado-derived — is the one known runtime implementation (closed;
Godot PRs #123124 and #123736 [external]); no open runtime implements it (Monado's main
branches, its 200 most-active public forks and its open MRs surveyed, [research/79 §4c](../../research/79-openxr-extensions-and-zxr.md)).
Under AGENTS rule 7 that is a rethink candidate, not a patch. The rethink is
[research/79 §3–§4](../../research/79-openxr-extensions-and-zxr.md); the owner ruled the
following on 2026-09-29.

### Decisions

1. **The 3D-client contract is the container pair, implemented in Monado.** OpenXR-native
   applications (Godot, Unity, StereoKit, LÖVR, anything that speaks the standard) are hosted as
   spatial containers by the runtime. Implemented as Mura's patch series over upstream Monado,
   written for upstreaming (the spec's contributors include Monado's author and maintainer,
   `ext_spatial_container.adoc:24,36`). Not a Mura-private extension. Mura would be the first
   open implementation.
2. **Monado composes and presents everything.** The specification fixes this ownership —
   "composition layers allow an application to offload the composition of the final image to a
   runtime-supplied compositor" (`rendering.adoc:1210`); a compositor outside the runtime is a
   second composition pass by construction. **zxr does not composite 3D content.** Its 2D windows
   are already quad layers to Monado (Amendment 2). zxr's roles: the Wayland server (`xdg-shell`,
   layer-shell, IME, clipboard, session lock, Xwayland satellite), the places/window-management
   policy for every window — 2D or 3D — and Monado's **workspace controller**. The normative
   statement of how a quad and a container reach the display is [specs/composition.md](../../../specs/composition.md).
3. **`zxr-shell-v2` is retired to a reserved hook.** Wayland-native 3D clients are a non-goal:
   no engine speaks a Wayland 3D protocol and every prior attempt (motorcar, wxrc, zwin,
   StardustXR — research/10) died on adoption; containers have Godot and Unity today. The XML
   stays in `protocols/`, CI-validated, status "retired — reserved, not served"; the hook exists
   for the one gap containers leave (a Wayland-native app wanting a 3D surface *and* the
   desktop's clipboard/IME). §"The protocol: `zxr-shell-v2`" and §"Sequencing … then the
   3D-native tier" of this ADR are superseded; everything else stands (Rust + smithay, Vulkan,
   quads always, the program shape, Amendment 3's overlay session as a transitional mechanism).
4. **Motorcar's per-pixel cross-client occlusion is a Monado runtime policy.** The spec fixes who
   composes, not how: the runtime "may: composite containers in any order they choose" and
   *may* precomposite each to a quad (`ext_spatial_container_self_rendering.adoc:476-486`);
   applications may attach `XR_KHR_composition_layer_depth` to container projection layers;
   Monado's compute path already binds that depth and never reads it (`comp_render_cs.c:250-261`,
   research/65 §7). Reading it and depth-testing container layers in the squasher is Mura's
   runtime policy. **Open (decider: the owner):** depth required from container apps to be
   interleaved, or opt-in with quad order otherwise.
5. **The controller seam is Monado-native** — `libmonado` extended with per-container verbs
   (`monado.c:348-403` today: primary/focused/io) and/or the `comp_multi` listener interface
   upstream has already sketched (MR !1354 "Bubble compositor events through the multi",
   `wallbraker/monado-collabora:jakob/comp/multi-interface` [external]). Never an OpenXR
   extension of the DisplayXR `XR_DXR_spatial_workspace` kind. **Open (decider: the owner):**
   which of the two shapes.
6. **Prerequisites before any seam:** server-derived peer identity at IPC accept
   (`SO_PEERCRED`; Monado's `ipc_app_state.pid` is client-asserted today, `ipc_protocol.h:399`)
   and a lease table whose default policy, with no controller present, is Monado's existing
   primary/overlay rule. DisplayXR's ADR-035 audit of a Monado fork that grew a multi-client
   shell without these [external] is the record of the failure mode.
7. **zxr's session is `XR_EXTX_overlay` transitionally** (provisional at the pin,
   `extx_overlay.adoc:316`) until Monado has the pair; then zxr is a container-session client.
   **Open (decider: the owner):** one container per Wayland toplevel, or one for the whole shell.
8. **Zero-copy 2D:** a Monado-private dmabuf-import swapchain extension removes zxr's per-commit
   blit (Monado has `create_swapchain_from_native` internally, `ipc_protocol.h:406`). A Monado
   work item behind a measurement gate ([specs/composition.md §2](../../../specs/composition.md)).
9. **Evidence posture.** Android XR is the closest comparable — mechanism evidence only (rule 2),
   internals not cited. Godot's `spatial_container` module and the `godot_openxr_vendors`
   spatial-container sample are the conformance substitute until Khronos publishes the container
   test extension (OpenXR-CTS has none at the pin). **DisplayXR is unpinned**: a non-standard,
   Windows-compositor Monado fork; only its ADR-035 audit is cited, [external].
10. **System rendering** (Android XR's `KHRX1_system_renderer` / SceneCore, PICO's spatial engine)
    is a non-goal: it puts the engine inside the runtime.
11. **Retain, per-frame should-submit/recommended-extent hints and bounds-fitted frusta with mono
    decay** are Monado implementation items under the spec, not `zxr-shell-v2` deltas
    (research/79 §7a-1..3 withdrawn).
12. **Perception layers** (passthrough, hand cutout — ADR 0008, Monado-side) lose the "zxr
    projection pass" fallback; they are Monado layers. The cutout-shape item stays open at the
    passthrough rung (spec §14), re-grounded.
13. **Monado is carried as a fork under the `mura-os` GitHub org** (`mura-os/monado`): `main` a
    mirror of upstream; branch `mura` = upstream `main` + Mura's series, rebased on every upstream
    bump; one feature branch per upstreamable series (`containers`, `controller-seam`,
    `dmabuf-swapchain`, per-device drivers), each the source of a GitLab MR. The flake pins the
    fork by rev (a `flake = false` input overriding `xrSources.monado`'s `src` — nixpkgs-xr's
    own mechanism, `references/nixpkgs-xr/pkgs/overrides/monado.nix:6-8`). Comparables: WiVRn
    (pinned rev + in-tree `patches/monado/*.patch`, `references/wivrn/patches/monado/`) — the
    shape this tree named until now; kwin-vr and `monado-galaxyxr` (fork repo + feature branch,
    both pinned as study clones); DisplayXR (hard fork that deleted upstream's drivers —
    rejected). The ruling takes the fork-repo shape with WiVRn's discipline: every commit
    upstream-shaped, the fork is where Mura's work waits for review, not where it diverges.
    `references/monado` stays the upstream study pin.

### Consequences

- One composition pass fewer on the device for 3D content: the app's pixels are read once by
  Monado's squasher and once by distortion, never by zxr. Research/65's ruling ("quads always")
  taken to its end.
- Two authorities become one: placement, focus and arrangement for every window are zxr's; every
  pixel is Monado's. Container input is Monado's action system, routed to the container zxr
  marks interactable; Wayland input stays zxr's seat (research/68's ruling unchanged).
- The Monado work is on the critical path for any bounded 3D app on Mura — the **C-track** in
  [implementation-path.md §3](../implementation-path.md) replaces M2–M4's protocol milestones.
- `pkgs/zxr` changes nothing today: no `zxr-shell-v2` server was ever built; `Shape::Volume`
  becomes a container proxy the scene tracks for hit-test and arrangement.

**Budget impact** (overview invariant 9): frame path — removes the projection pass zxr would have
run for 3D content (a full-resolution read of every client per eye per frame on a tiler), adds
nothing to the quad path; Monado's squasher cost per extra client is measured (research/67 §2:
+0.33 ms/frame at 16 quads, equal for 1–4); memory — no zxr-side colour+depth slots per 3D
client; IPC — the seam is per policy change, not per frame; gates in
[specs/composition.md §7](../../../specs/composition.md).
