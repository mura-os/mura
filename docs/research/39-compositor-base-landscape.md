# 39 — Compositor base landscape: smithay ratification evidence

**Question.** ADR 0006 left the compositor base library open ("Rust + smithay" as a documented
leaning, not ratified) pending four checks: smithay's coverage beyond the 2D tier (Vulkan renderer
integration, DRM leasing, the OpenXR/`ash` boundary, explicit sync via `wp_linux_drm_syncobj_v1`),
and whether any wlroots-only capability forces a larger C surface than "FFI where it counts".
This doc answers those checks at file level from pinned clones and records the dispositions of
every other candidate. It is the evidence record for the ADR 0006 amendment that ratifies
**Rust + smithay**.

**Method.** Code study of the clones pinned in `references/MANIFEST.json` (2026-09-23): smithay
`79bbed5` (0.7.0, master), niri `5f4469b`, openxrs `eba4c6a` (openxr 0.22.0), wayvr (wayvr-org
monorepo, wayvr 26.8.0), wlroots `297e01d` (0.21.0-dev, freedesktop upstream — the archived
`swaywm/wlroots` GitHub mirror is a known trap), weston `3c6ce8d` (16.0.90), louvre `e6f7086`
(3.0.0), mir `10d0907` (2.31.0), waynest `bce31ec`, stardustxr-server `cf20614`. Paths below are
relative to each repo root.

---

## 1. smithay coverage audit

Recall zxr's shape (ADR 0006, composition doc): a single OpenXR client of Monado with its **own
Vulkan renderer (ash)** targeting OpenXR swapchains — never smithay's DRM/KMS output path. What
zxr needs from a base library is the Wayland protocol frontend, buffer import plumbing, explicit
sync, seat/input, and Xwayland.

### 1.1 The crux: the Wayland frontend is renderer-independent

A grep across `src/wayland/` finds **zero references to the `Renderer` trait**. The only
`backend::renderer` imports in the wayland tree are free helpers with no renderer instance
(`buffer_type`, the `Fence` trait, `buffer_dimensions`). `wayland::compositor` — surface tree,
double-buffered cached state, `with_states`, pre-commit hooks, transaction blockers — is pure
protocol machinery.

The bridge layer, `src/backend/renderer/utils/wayland.rs`, splits cleanly:

- **Renderer-free and reusable by zxr:** `on_commit_buffer_handler::<D>` (line 369; generic over
  the state type only) maintains `RendererSurfaceState` per surface — buffer dimensions/scale/
  transform, damage accumulation (`CommitCounter`/`damage_since`), viewport-aware `SurfaceView`,
  opaque regions — and its `Buffer` wrapper owns the two things easiest to get wrong solo:
  `wl_buffer.release()` **and** syncobj release-point signalling on drop (lines 60–118, points
  taken from `DrmSyncobjCachedState` at 168–178). The per-context texture cache is also open to
  foreign types (`ContextId::<T>::new()` is public; the custom texture only needs the small
  `Texture` trait).
- **Renderer-bound and bypassed:** `import_surface`/`import_surface_tree` (`R: Renderer +
  ImportAll`) and `draw_render_elements`.

`desktop::Space` is mostly renderer-free (mapping, stacking, `element_under`, output mapping,
`refresh` — no `R:` bounds in `src/desktop/space/mod.rs:90–426`); only the
`render_elements_for_*`/`render_output` family requires a smithay Renderer.
`send_frames_surface_tree` (frame-callback dispatch) is renderer-free. The module docs state the
drawing helpers merely require `on_commit_buffer_handler` for buffer management
(`src/desktop/mod.rs:46–50`).

**What bypassing the Renderer trait costs:** the `RenderElement` ecosystem,
`OutputDamageTracker`, `render_output`, and the multi-GPU machinery. Concretely zxr reimplements
dmabuf→`VkImage` import with modifiers, shm upload, per-XR-layer damage aggregation from the
per-surface `DamageBag`s, and subsurface-tree draw ordering (the traversal primitives
`with_surface_tree_downward` etc. live in `wayland::compositor` and are free to use). For a
compositor projecting surfaces into 3D space, all of that was ours to own anyway (composition
§2–§5).

### 1.2 `backend::vulkan`: utilities, not a renderer — as expected

The module says it outright: "This module does not provide abstractions for logical devices,
rendering or memory allocation" (`src/backend/vulkan/mod.rs:6–7`). It provides `Instance` and
`PhysicalDevice` (~1,088 lines of thin ash wrapping), including the piece zxr genuinely needs:
DRM-node correlation via `VK_EXT_physical_device_drm` — `PhysicalDevice::primary_node()` /
`render_node()` (`mod.rs:481,499`) — which is how the Vulkan device maps to the DRM fd that
dmabuf feedback and drm_syncobj require. No `impl Renderer for` Vulkan exists in-tree (only
GLES/Glow/Pixman/Multi/test-Dummy). Adjacent precedent: `backend/allocator/vulkan/` is an
ash-based `VulkanAllocator` whose images export as dmabuf — smithay's own ash (0.38, matching
openxrs') already coexists with compositor-owned Vulkan usage.

### 1.3 Explicit sync: designed for exactly zxr's release path

`src/wayland/drm_syncobj/` (feature `backend_drm`):

- **Acquire model (documented default):** CPU-side — attach a `DrmSyncPointBlocker` so the
  commit isn't current until the acquire fence signals (`mod.rs:1–14`,
  `sync_point.rs:244–256`). GPU-side semaphore waits are possible off the documented path:
  `DrmSyncPoint::export_sync_file` (`sync_point.rs:164`) yields a sync-file fd importable as
  `VK_SEMAPHORE_HANDLE_TYPE_SYNC_FD`.
- **Release model:** signalled when all references to the utils `Buffer` drop (`mod.rs:13–14`) —
  i.e. reusing `on_commit_buffer_handler` gets correct release-point signalling for free. And
  `DrmSyncPoint::import_sync_file` (`sync_point.rs:200`) exists, per its own doc comment, for
  "compositors that drive Vulkan explicit sync via
  `VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_SYNC_FD_BIT`" — zxr's exact release path.
- **Requirements:** `DrmSyncobjState::new` takes a `DrmDeviceFd` (the render node from §1.2);
  gate the global on `supports_syncobj_eventfd`; the pre-commit hook enforces dmabuf-only for
  synced surfaces (`mod.rs:269–276`).

### 1.4 dmabuf feedback: format tables are ours by design

`create_global_with_default_feedback` creates the global at **version 6** (`dmabuf/mod.rs:716`);
`DmabufFeedbackBuilder::new(main_device, formats)` takes the format/modifier `Vec<Format>` as
input — the docs suggest a smithay renderer as the "typical" source, but zxr computes them from
its Vulkan device (`vkGetPhysicalDeviceFormatProperties2` + `VK_EXT_image_drm_format_modifier`),
which is precisely composition §7.3's dmabuf-format-filtering constraint. Import handshake is
handler-owned (`DmabufHandler::dmabuf_imported` + `ImportNotifier`) — "import the dmabuf into
*your* renderer" (`mod.rs:56`). Per-surface feedback and preference tranches exist
(`SurfaceDmabufFeedbackState`, `TrancheFlags::Scanout`).

### 1.5 DRM lease: present, and aimed at our use case

`src/wayland/drm_lease/` implements the **lessor** side of `wp_drm_lease_device_v1`; the module
docs name the use case: "in particular useful for VR applications, that would like to take over
a directly attached VR display" (`mod.rs:6–7`) — i.e. Monado as the lease client on the
desktop/dev profile, matching doc 10 §1 (appliance bypasses leasing via `VK_KHR_display`).

### 1.6 Xwayland: rootless, with the WM half provided

`XWayland::spawn` hardcodes `-rootless` (`src/xwayland/xserver.rs:150`); `xwm/` provides
`X11Wm`, the X11-window-manager helper that associates X11 windows to `WlSurface`s via the
xwayland-shell handshake. zxr implements the `XwmHandler` policy trait (map/configure/resize/
selection) and treats `X11Surface` as a window-model element — `desktop::Window` already
abstracts over xdg toplevels and X11 surfaces. This resolves the registry's "no doc on the
XWayland WM half" gap: the mechanism half is smithay's.

### 1.7 Event loop: blocking `xrWaitFrame` coexists on its own thread

calloop is single-threaded poll dispatch, but cross-thread feeding via `calloop::channel` is the
established pattern (niri uses it for PipeWire and a11y worker threads). The openxr crate is
split for exactly this: `create_session` returns separate `Session`/`FrameWaiter`/`FrameStream`
"to ensure multithreaded pipelined renderers can safely wait for the cue to begin a new frame
while a prior frame is still being rendered" (`openxr/src/instance.rs:381–388`). Natural zxr
shape: calloop thread owns Wayland dispatch + state; the XR thread blocks in
`FrameWaiter::wait()` and wakes the loop per frame.

### 1.8 niri as the pattern reference

niri pins smithay to git rev `79bbed5` — the exact rev in our MANIFEST — with
`default-features = false` (`Cargo.toml:28–32`). Three patterns to copy:

1. **Two-level state with a backend enum, not a trait:** `State { backend: Backend, niri: Niri }`
   (`src/niri.rs:719–721`); `Backend` is `enum { Tty, Winit, Headless }` with delegating methods
   (`src/backend/mod.rs:25–80`). Maps directly onto zxr's `{ Xr, WinitDev, Headless }`
   (composition §7.1's windowed dev mode).
2. **Per-output `FrameClock` + `RedrawState` machine** (`src/frame_clock.rs:7–60`,
   `src/niri.rs:513–525`): `Idle/Queued/WaitingForVBlank/WaitingForEstimatedVBlank` driven by
   real or estimated vblanks. For zxr, "vblank" becomes the predicted display time from
   `xrWaitFrame`; the same machine prevents double-queued redraws and keeps frame callbacks
   honest.
3. **Dmabuf-readiness pre-commit hooks generalizing the blocker model to implicit sync**
   (`src/handlers/compositor.rs:528–534`), plus the `unmapped_windows` map/unmap lifecycle
   discipline (`src/niri.rs:247–248`).

### 1.9 openxrs: the ash/OpenXR boundary exists and matches

`openxr` 0.22.0 / `openxr-sys` 0.14.0, depending on **ash 0.38 — the same major as smithay's**
(no version conflict). `XR_KHR_vulkan_enable2` is fully wrapped:
`Instance::create_vulkan_instance` (`instance.rs:226`), `vulkan_graphics_device`
(`instance.rs:296–323`), `create_vulkan_device` (`instance.rs:344` —
`xrCreateVulkanDeviceKHR`), `graphics_requirements::<Vulkan>()`. The `Vulkan` graphics marker
uses raw handles, so interop with an ash device is `ash::Handle::as_raw()` into
`SessionCreateInfo`. Swapchain loop: `enumerate_images() → Vec<VkImage>`, `acquire_image`,
`wait_image`, `release_image`; pacing via `FrameWaiter::wait`/`FrameStream::begin`/`end` with a
documented canonical loop (`frame_stream.rs:13–60`).

### 1.10 Maturity and pinning

In-tree 0.7.0, edition 2024, MSRV 1.87. Release history: 0.3.0 (2021) → 3.5-year gap → 0.4.0
(2025-01) → 0.7.0 (2025-06), roughly bi-monthly now — but both flagship consumers ride pinned
master revs (niri: `79bbed5`; cosmic-comp: crates.io 0.7.0 `[patch.crates-io]`-overridden to a
git rev). Recommendation for zxr: same — pin a rev, `default-features = false`, features
`wayland_frontend`, `backend_drm`, `backend_vulkan`, `desktop`, `xwayland` (+ input as needed);
plan for upgrades (the in-tree CHANGELOG already carries breaking `## Unreleased` entries).

### 1.11 Verdict on the four ADR 0006 checks

**No structural blockers.** (1) Vulkan renderer integration: the frontend never requires the
Renderer trait; the one bridge worth keeping (`on_commit_buffer_handler`) is renderer-free.
(2) DRM leasing: implemented, lessor-side, VR-motivated. (3) The OpenXR/ash boundary: openxrs
wraps vulkan_enable2 end-to-end on the same ash major. (4) Explicit sync: implemented, with
sync-file import/export APIs explicitly anticipating a Vulkan-driven compositor. No
wlroots-only capability forces a C surface beyond "FFI where it counts".

Frictions to plan for (none blocking): `backend_drm` + an opened render node are mandatory for
drm_syncobj/dmabuf-feedback even though zxr never touches KMS on the headset path; **smithay's
winit backend is unusable for the Vulkan dev mode** (hard-coupled to EGL/GLES:
`init<R>() where R: From<GlesRenderer> + Bind<EGLSurface>`, `src/backend/winit/mod.rs:66–68`) —
the windowed dev backend uses winit directly with the ash swapchain and translates winit input
into `input::Seat` (niri's `src/backend/winit.rs` shows the input-translation shape); the
documented acquire model is CPU-side blockers (GPU-side semaphore waits are available but
off-path).

---

## 2. WayVR anatomy: the existence proof

WayVR (doc 10 §2.5) is the running proof that smithay's Wayland frontend composes with a custom
Vulkan stack whose device is created **by the OpenXR runtime**. Findings from the wayvr-org
monorepo (wayvr 26.8.0):

- **smithay 0.7.0 from crates.io**, `default-features = false`, features `backend_vulkan`,
  `desktop`, `wayland_frontend`, `xwayland` — of which `backend_vulkan` and `xwayland` are dead
  weight (never referenced; Xwayland is handled by spawning **xwayland-satellite** instead,
  `backend/wayvr/client.rs:101–108`). No GLES feature is even compiled in.
- **Vulkan through OpenXR** (`graphics/mod.rs:180–387`): `create_vulkan_instance` →
  `vulkano::Instance::from_handle`; `vulkan_graphics_device` (GPU chosen by the runtime —
  device identity with Monado guaranteed); raw `DeviceCreateInfo` → `create_vulkan_device` →
  `Device::from_handle`. Extensions: external_memory{,_fd}, external_memory_dma_buf,
  image_drm_format_modifier, physical_device_drm. **No timeline semaphores.**
- **Renderer trait bypassed entirely** — no `impl Renderer for` in the workspace. smithay's role
  ends at the protocol state machines + delegate macros + `PopupManager` + `Seat` + `Output`;
  imported textures are stashed in the surface `data_map` and composited by WayVR's own vulkano
  pipelines into per-overlay XR swapchains.
- **dmabuf v4 with default feedback, falling back to v3** (`backend/wayvr/mod.rs:241–260`);
  `main_device` derived from `VK_EXT_physical_device_drm` render major/minor via
  `libc::makedev`; format table computed from Vulkan modifier queries
  (`graphics/dmabuf.rs:361–404`). The import path (`dmabuf_texture_ex`,
  `graphics/dmabuf.rs:42–139`) does explicit-modifier `VkImage` creation with per-plane
  subresource layouts, memory-fd import — but **plane-0-only** and `Optimal`-tiling guessing for
  implicit modifiers: limitations not to inherit.
- **Synchronization is too naive for a session compositor:** implicit sync only (no syncobj
  global), previous-buffer-released-on-next-commit, and a full CPU
  `then_signal_fence_and_flush().wait(None)` before `xrEndFrame`
  (`backend/openxr/mod.rs:383–428`).
- **Overlay-shaped, not session-shaped:** `XR_EXTX_overlay` session (hand-built raw struct
  chain, `helpers.rs:182–212`), one XR swapchain **per overlay window**, quad/cylinder layers
  composited by Monado, **no projection layer anywhere**, single-threaded xrWaitFrame-clocked
  loop dispatching Wayland once per XR frame.

**Transfers to zxr:** the OpenXR→Vulkan bring-up (the hardest-won code), the dmabuf import/format
table module shape, the compositor delegate scaffold, frame-callback deferral for hidden
surfaces, xwayland-satellite as a bring-up-cheap Xwayland answer. **Does not transfer:** the
overlay session model, per-window swapchains, the sync model, the fixed virtual output.

---

## 3. The wlroots fallback record

From upstream master (`297e01d`, 0.21.0-dev):

- **No external-device adoption.** The only public Vulkan-renderer constructor is
  `wlr_vk_renderer_create_with_drm_fd` (`include/wlr/render/vulkan.h:21`), which performs the
  entire bring-up internally (own `VkInstance` at `render/vulkan/vulkan.c:84`, phdev matched to
  the DRM fd, own `VkDevice` at `vulkan.c:477`). The public accessors go the other direction —
  extract instance/device for others to adopt. `vulkan_renderer_create_for_device` exists but
  only in the **private, uninstalled** header (`include/render/vulkan.h:407`), taking a
  wlroots-internal type: using it means patching wlroots. The constructor still logs the renderer
  as "only experimental and not expected to be ready for daily use" (`renderer.c:2812–2813`).
  So the supported shapes are "OpenXR adopts wlroots' device" or a patch — the built-in Vulkan
  renderer, the headline advantage, does not connect to `xrCreateVulkanDeviceKHR` cleanly.
- **Extension list** (hard-required at device creation, `vulkan.c:512–531`): external_memory_fd,
  image_format_list, external_memory_dma_buf, queue_family_foreign, image_drm_format_modifier,
  timeline_semaphore, synchronization2, sampler_ycbcr_conversion, and friends — a useful
  checklist for zxr's own renderer requirements.
- **Explicit-sync contract:** advertising `wlr_linux_drm_syncobj_v1` requires *both*
  `wlr_renderer.features.timeline` and `wlr_backend.features.timeline`
  (`include/wlr/types/wlr_linux_drm_syncobj_v1.h:40–46`); with a custom OpenXR presentation path
  zxr would have to supply the backend half of those semantics itself — the same work as under
  smithay.
- **What a port would reuse renderer-free:** `wlr_seat`, `wlr_xdg_shell`,
  `wlr_compositor_create(..., NULL renderer)` (documented), `wlr_linux_dmabuf_v1_create` with a
  manually built feedback table, `wlr_shm_create` with explicit formats, `wlr_xwayland` (zero
  renderer references), the headless backend. Entangled and dropped: `wlr_scene`, the
  `wlr_output` commit/swapchain machinery, screencopy/image-copy-capture, color management,
  cursor.

**Fallback conclusion:** wlroots recovers protocol machinery, seat, and Xwayland — but not the
render path, which is the same custom work smithay requires. The fallback is therefore real but
buys nothing on the hard axis; it would additionally cost the Rust/C boundary for every protocol
handler.

---

## 4. Dispositions

- **weston/libweston** (16.0.90): a real library ("API/ABI-compatible within a single stable
  release", `README.md:90–99`) and it now ships a Vulkan renderer
  (`libweston/renderer-vulkan/vulkan-renderer.c`, behind a meson option) — but that renderer
  likewise creates its own instance/device internally (`vulkan-renderer.c:4259,4343`) behind the
  internal `weston_renderer` pointer; no seam for an OpenXR-created device or OpenXR-swapchain
  presentation. The reusable surface is the shell/output layer zxr replaces. Rejected; kept as
  Vulkan-renderer reading material.
- **Louvre** (3.0.0): C++ factory/override model over OpenGL/Skia; README lists "Vulkan (WIP)"
  but **no Vulkan implementation exists in the tree** (only incidental protocol-XML strings);
  Xwayland is **rootful only** (`README.md:43`). Rejected.
- **Mir** (2.31.0): operates at the miral shell-abstraction layer; the renderer seam is GL-typed
  end to end (`CustomRenderer::Builder` takes a `gl::OutputSurface` +
  `GLRenderingProvider`, `include/miral/miral/custom_renderer.h:40–42`; no Vulkan rendering path
  in-tree). Wrong abstraction level and wrong graphics typing. Rejected.
- **Qt Wayland Compositor** (not cloned; disposition from documentation): QML/Qt Quick
  compositor framework, GPLv3-or-commercial licensing, sensible only for a Qt-centric shell.
  zxr owns an ash renderer and has no Qt dependency. Rejected.
- **waynest** (0.2.0-rc1): tokio wire-protocol + codegen'd dispatch traits, self-described as
  "not intended for direct use"; no buffer import, no seat/xdg helpers — a substrate two layers
  below smithay. Rejected as base; noted as prior art for from-scratch wire handling.
- **stardustxr-server** (`cf20614`): **does not depend on waynest** (absent from Cargo.lock) and
  no longer embeds any Wayland compositor — it is now a Bevy/OpenXR application whose 2D content
  arrives as dmabuf textures over the Stardust protocol from an out-of-process Wayland
  compositor ("Flatland", `src/session.rs:97–101`). Validates decoupled
  compositor-as-client architectures (our delegation seam, ADR 0014) but supplies nothing for an
  in-process base. The earlier claim that Stardust's Wayland layer uses waynest is corrected by
  the tree: it has moved out of the server entirely.

---

## 5. Decision input and the R0 bring-up gates

**Ratify Rust + smithay** (the ADR 0006 amendment records this): the frontend/renderer split is
clean and grep-verified (§1.1), all four open checks pass (§1.11), WayVR proves the
OpenXR-created-device shape end-to-end on a real headset (§2), and the fallback's headline
advantage dissolves on inspection (§3). The wxrc-port sizing spike (old D2) is obsolete: ADR
0006 already concluded this is a rewrite, and the base question no longer needs sizing evidence.

The **R0 bring-up spike** is thereby re-scoped from decision gate to **risk-retirement**: it can
no longer change the base choice, but it must retire the integration risks before M1 work
builds on them. Its gates (each measured, not eyeballed; the spike lives in the `dev-session`
slot where sway runs today):

1. **Real presentation:** a native Wayland client appears as a movable textured plane inside a
   real OpenXR session (Monado simulated HMD) — the session created on a Vulkan device from
   `xrCreateVulkanDeviceKHR` via openxrs, projection layer, not overlay quads.
2. **Real GPU integration:** dmabuf client buffers (weston-simple-dmabuf-egl / a Vulkan client)
   reach the ash renderer with **no CPU readback**, format/modifier table computed from the
   Vulkan device, explicit sync exercised end-to-end (`wp_linux_drm_syncobj_v1` acquire wait +
   release-point signal after composition completes), bounded buffering verified under a
   client submitting faster than composition.
3. **Real window behavior under churn:** resize, popups (positioner-constrained), focus
   handoff, client kill mid-frame, surface destruction with in-flight GPU work — no unresolved
   waits, no stale textures (composition §7.4's stopping-a-client rule, previewing M4's
   criterion).
4. **Xwayland early:** one X11 app participates (via smithay's `X11Wm` or, at minimum,
   xwayland-satellite as the recorded bring-up shortcut) — the decision of which to keep is an
   R0 output, not an input.

Instrumentation required at the gates: compositor GPU time per frame, missed `xrWaitFrame`
deadlines, per-buffer retention time, and copy count on the client-buffer path (must be zero
CPU-side). Exit is a written result against each gate; failures inform design, not the base
choice — the recorded fallback (wlroots, §3) triggers only if smithay exhibits a *structural*
inability (a protocol-frontend defect unfixable without forking), not an effort overrun.
