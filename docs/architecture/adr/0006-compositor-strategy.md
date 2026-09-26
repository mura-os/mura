# ADR 0006: XR compositor strategy — revive the zxr lineage as `zxr-shell-v2`, Wayland-native, on Monado

**Status:** accepted (draft); **base library ratified 2026-09-23** (Rust + smithay, see §The
compositor base; evidence in [39-compositor-base-landscape](../../research/39-compositor-base-landscape.md))
**Date:** 2026-09-22 (amended 2026-09-23; amended 2026-09-26 — §Program shape, below)
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

### The protocol: `zxr-shell-v2`

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

### Sequencing: ship the 2D tier first

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
