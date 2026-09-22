# 08 — Motorcar → wxrc → wxrd: the 3D-windowing compositor lineage

**Date:** 2026-09-22. This document has two parts. **Part 1 (philosophy and design lineage)** is
written from the primary sources — Forrest Reiling's 2014 Cal Poly Master's thesis
(`references/motorcar-thesis/thesis.pdf`) and the two protocol XMLs — to capture the *intent* the
project carries forward. **Part 2 (code-level analysis)** is the deep read of the Motorcar, wxrc,
and wxrd codebases and their protocols. spatial-os's compositor decision (ADR 0006) is grounded in
Part 1, not just the surviving wire formats.

Primary sources:
- Thesis: `references/motorcar-thesis/thesis.pdf` — "Toward General Purpose 3D User Interfaces:
  Extending Windowing Systems to Three Dimensions", Forrest Reiling, June 2014.
- `references/motorcar/src/protocol/motorcar.xml` — the original 3D-windowing Wayland protocol.
- `references/wxrc/protocol/zxr-shell-unstable-v1.xml` — the 2019 OpenXR-era reworking (Status
  Research & Development GmbH / Drew DeVault + Simon Ser).
- `references/wxrc/` (canonical, git.sr.ht/~sircmpwn/wxrc), `references/wxrc-mirror/` (GitHub copy),
  `references/wxrd/` (Collabora's xrdesktop descendant), `references/motorcar/` (`stable` branch).

---

## Part 1 — Philosophy and design lineage

### 1.1 The thesis: a windowing system, not a VR app

Motorcar's core claim is that 3D interfaces face the same two problems 2D interfaces already solved
with windowing systems, and should be solved the same way (thesis Abstract, §2.1):

1. **Device abstraction** — every 3D app otherwise integrates every input/display device itself.
   A windowing system abstracts input hardware behind *input primitives* and abstracts the display
   behind a compositing step.
2. **Multiple-application support** — there is otherwise no way for several apps to share the same
   3D interface hardware. A windowing system multiplexes the hardware among clients.

The thesis frames this as *extending* the existing windowing system rather than replacing it: build
3D windowing on top of 2D windowing infrastructure so that (a) the modified surface of the stack is
minimal and (b) **unmodified 2D applications keep working in the same 3D space** (thesis §6.3, §7.3.3).
This is the philosophy the user wants carried into spatial-os: the XR shell is a *windowing system*,
2D apps are first-class citizens embedded on planes, and hardware is abstracted below a protocol —
not a bespoke VR application that happens to show windows.

### 1.2 Why Wayland specifically

The thesis rejects X for one structural reason (thesis §5.3.2): in X the compositor is a separate
client, so the X server keeps its own strictly-2D window layout and routes input by that layout —
a 3D-transformed window can't receive correctly-transformed input. In Wayland the compositor *is*
the display server, so it can give a window an arbitrary 3D embedding and still deliver input in the
window's local coordinate space, and route input by an arbitrary spatial data structure associated
with the surface rather than the surface rectangle (thesis §5.3.2). That property is exactly what a
3D windowing system needs. It is also the argument for spatial-os keeping the shell Wayland-native
rather than adopting a non-Wayland scene-graph IPC.

### 1.3 The three interpretations of a 3D window

The thesis's most distinctive design contribution (thesis §6.1.2) is that "window" has three valid
3D interpretations, and a compositor can support all of them because they differ only in clipping:

- **Cuboid** — a box-shaped region of the interface space the client fills with 3D content, carved
  out of the whole (the direct 3D analog of a rectangular 2D window).
- **Portal** — a 2D opening onto an unbounded client space, like a physical window between two rooms;
  content is visible only through the opening.
- **Unbounded** — the full-screen case of either: extend the cuboid to infinity or stretch the portal
  over the whole buffer, and the client can draw anywhere in the interface space.

Plus the trivially-embedded **2D interface context**: a rectangular plane in the 3D space carrying an
ordinary 2D surface, with 3D input events projected onto the plane and delivered as 2D events —
so unmodified 2D apps participate (thesis §6.1.2.2). `motorcar.xml` encodes cuboid/portal as a
`clipping_mode` enum on `motorcar_surface`; unbounded is the degenerate large-size case.

### 1.4 The central mechanism: 3D windows with 2D buffers + depth compositing

The key engineering idea (thesis §6.2) is that clients still hand the compositor **2D buffers**, just
as in ordinary Wayland — no 3D geometry crosses the protocol. Consistency of the shared 3D scene is
achieved by coordinating the *projection*, not by moving geometry:

- **Synchronized view + projection matrices, per viewpoint** (thesis §6.2.1). The compositor owns one
  viewpoint per eye and sends each client the view matrix (every frame, as the head moves) and the
  projection matrix (rarely — never for an HMD). All parties project with identical matrices, so
  stereopsis, motion parallax, and relative-size cues stay consistent across every client and the
  compositor.
- **Stereo via a double-wide buffer** (thesis §6.2.2): the client draws both eyes side-by-side in one
  buffer at viewports the compositor specifies, avoiding multi-surface synchronization.
- **Depth-buffer compositing** (thesis §6.2.3): clients send their *depth buffer* alongside color,
  and the compositor composites all clients (and its own geometry) with the ordinary GPU depth test.
  This lets clients render however they like (even ray tracing) as long as they fill depth correctly,
  and keeps the protocol tiny — no scene-graph API over the wire.
- **Clipping by stencil** (thesis §6.2.4, §7.3.3.1): the compositor stencils each client to its window
  bounds (cuboid faces or portal opening), so an uncooperative client can't draw outside its window.

### 1.5 The load-bearing workaround: the depth viewport (and the Mesa dependency)

The single most important implementation fact for a 2026 revival (thesis §7.1.1.3.1): **Wayland EGL
gives the compositor zero-copy access to a client's *color* buffer but not its *depth* buffer.**
Motorcar works around this by having the client render into an FBO, then pack the depth buffer into a
*second color viewport* (a 32-bit float depth value encoded across RGBA8), doubling buffer size; the
compositor unpacks it back into a real depth buffer. The thesis calls giving the compositor direct
depth access "the single most pressing area of future work," and says the clean fix "would likely
require modification of the implementation of Wayland EGL within Mesa." **This is almost certainly the
origin of wxrc's README requirement to patch Mesa** — Part 2/doc 09 must confirm whether wxrc kept the
depth-viewport hack or attempted the Mesa fix, and whether modern dmabuf + explicit-sync makes a
clean depth path possible in 2026.

### 1.6 Frame timing is an explicit, client-visible policy choice

The thesis (§7.3.2) identifies two timing modes with a real tradeoff: draw-with-last-frame's-data
(no client can drop the compositor's frame rate, but motion-to-photon latency gains a frame and
clients can desync from the current head pose) versus wait-for-clients (minimal latency, but one slow
client drops frames). It proposes the mode could be toggled per client by application profile. For an
XR compositor this is central; the zxr protocol's own TODOs (below) flag "better timing information"
as unfinished, so spatial-os's protocol revision must treat frame pacing as first-class.

### 1.7 Modularity: compositor library vs. device compositor

Motorcar is deliberately split (thesis §7.3.1) into a reusable compositor *library* (Wayland backend,
scene graph, compositing logic) and thin *device compositors* that instantiate device-specific classes
(HMD, 6DoF tracker) and a window manager. Device SDKs stay out of the core. This maps directly onto
spatial-os's device→adaptation split: the compositor is common; the per-headset display/tracking
integration is a device concern. The original device compositor targeted an Oculus Rift DK1 + Razer
Hydra via a scene graph separating *virtual* nodes (any parent) from *physical* nodes (only physical
parents) — the tracked-hardware topology.

### 1.8 The protocol lineage in one table

| Concern | `motorcar.xml` (2014, QtWayland) | `zxr-shell-unstable-v1.xml` (2019, wlroots/OpenXR) |
|---|---|---|
| Shell object | `motorcar_shell.get_motorcar_surface(surface, clipping_mode, enable_depth_compositing)` | `zxr_shell_v1.get_xr_surface(surface)` + `create_composite_buffer` |
| 3D surface | `motorcar_surface` with `clipping_mode` enum (cuboid/portal), `transform_matrix` event, `set/request_size_3d` (metres) | `zxr_surface_v1` (role on a `wl_surface`; 2D buffer attach is a protocol error) |
| Per-eye viewpoint | `motorcar_viewpoint` global per eye: `view_matrix`, `projection_matrix`, `view_port` (separate color + depth viewports) | `zxr_view_v1` global per view; `zxr_surface_view_v1.mvp_matrix` event (row-major 4x4) |
| Stereo/depth transport | double-wide buffer; depth packed into a second color viewport (EGL limitation) | `zxr_composite_buffer_v1`: one 2D `wl_buffer` per view, `buffer_type` = pixel_buffer \| depth_buffer |
| 3D input | `motorcar_six_dof_pointer`: enter/leave/motion/button with 3-vec position + 3x3 rotation | none defined (TODO) |
| Coordinate space | window-local metres; model/view/projection split | composite-buffer local (-1,-1)..(1,1); MVP folded into one matrix |
| Status | thesis prototype, "current protocol limitations" §7.1.2 | explicitly experimental; TODOs for timing, geometry buffers (glTF), 3D movies |

The essential evolution: motorcar keeps model/view/projection *separate* (clean for a windowing
system — the compositor owns view+projection, the client owns its model transform) and specifies a
6DoF input device; zxr **folds everything into one MVP matrix per surface-view**, generalizes "eye" to
an arbitrary number of `zxr_view_v1` globals, and moves depth from a packed-color-viewport hack to a
typed `buffer_type` on a composite buffer — but drops input entirely and leaves timing unfinished. A
2026 spatial-os protocol should keep motorcar's view/projection/model separation and 6DoF input, keep
zxr's N-view generalization and typed depth buffers, and finish timing + explicit-sync + dmabuf.

### 1.9 What Part 2 and the sibling docs must establish

- **Part 2 (below, B1):** the actual wxrc/wxrd code — wlroots version + API surface, the OpenXR
  session/swapchain path, how the depth viewport is really implemented, how 2D surfaces become
  textured quads, and what wxrd changed (gxr/xrdesktop instead of raw OpenXR?).
- **[09-wxrc-ecosystem-gap-2026.md](09-wxrc-ecosystem-gap-2026.md) (B2):** which of the Mesa / Monado /
  OpenXR / Sway / Vulkan / Xwayland / wlroots patches are landed / superseded / still needed in 2026.
- **[10-xr-wayland-protocol-comparison.md](10-xr-wayland-protocol-comparison.md) (B3):** motorcar vs.
  zxr vs. zwin vs. StardustXR vs. WayVR on one matrix, and what a 2026 zxr revision should change.

---

## Part 2 — Code-level analysis

<!-- Written by the B1 research pass; appended below this line without altering Part 1. -->

Read directly from the pinned clones (the B1 subagent hit a resource limit; this was written in the
main session from the same sources). wxrc is small — ~3,460 lines of C across 10 files
(`references/wxrc/src/`). All file:line citations are against the pinned commits in
`references/MANIFEST.json` (wxrc `db59692`, 2021-07-08; wxrd `892fa19`, 2023-05-24;
xrdesktop/gxr `dbf3dba`/`8de5cca`, 2026-01-12; motorcar `e1cb943`, 2015).

### 2.1 wxrc architecture

**Build system and pinned dependency versions** (`references/wxrc/meson.build`). This is the raw
material for doc 09's archaeology, quoted verbatim:

- `wlroots_version = ['>=0.8.1', '<0.9.0']` — a **2019-era wlroots**, tried first as a subproject
  then as a system dep. This single line is the biggest revival cost signal (see §2.5).
- Project-wide defines: `-DWLR_USE_UNSTABLE`, **`-DXR_USE_GRAPHICS_API_OPENGL_ES`**,
  **`-DXR_USE_PLATFORM_EGL`** — i.e. the OpenXR graphics API is **GLES2**, bound through **EGL**.
- Deps: `cglm`, `egl`, `gbm`, `glesv2`, `openxr`, `xkbcommon`, `wayland-client`, `wayland-server`,
  `wayland-protocols`, `wlroots`. No Vulkan. Note it links **both** `wayland-client` and
  `wayland-server`: wxrc is simultaneously a Wayland *server* (to its 2D/3D app clients) and a
  Wayland *client* (it nests inside a host compositor via wlroots' wayland backend — see §2.1 output
  handling).

**Source files** (`references/wxrc/src/`, sizes): `backend.c` (644) the custom OpenXR wlr_backend;
`input.c` (583) seat/pointer + XR-pose-driven ray cursor; `main.c` (543) the compositor entry and XR
frame loop; `render.c` (458) the GLES2 renderer; `xr-shell-protocol.c` (444) the zxr server;
`xrutil.c` (364) OpenXR helpers + projection math; `xdg-shell.c` (155) 2D window management;
`view.c` (143) the view abstraction; `mathutil.c` (68); `xr-shell.c` (56) zxr glue.

**The OpenXR binding path** (`backend.c`) — directly answers doc 09's central question:
- Instance created with exactly two extensions (`backend.c:82-85`):
  `XR_KHR_OPENGL_ES_ENABLE_EXTENSION_NAME` and **`XR_MNDX_EGL_ENABLE_EXTENSION_NAME`**. The `MNDX`
  multi-vendor prefix is present in this 2021 code (the `MND`→`MNDX` rename referenced in the log is
  already applied here).
- Session bound via `XrGraphicsBindingEGLMNDX` (`backend.c:281-287`): `getProcAddress =
  eglGetProcAddress`, and the EGL `display`/`config`/`context` are pulled straight from wlroots'
  GLES2 renderer (`wlr_gles2_renderer_get_egl(renderer)`, `backend.c:633`). **This is the load-bearing
  coupling to Monado**: `XR_MNDX_egl_enable` is a Monado-specific extension, so wxrc only runs on
  Monado, and only because it shares wlroots' EGL context with the OpenXR runtime.
- Graphics requirements checked via `xrGetOpenGLESGraphicsRequirementsKHR` (`backend.c:249-278`);
  GLES2 required. Startup asserts `GL_OES_depth_texture` (`backend.c:528-531`) — needed for the real
  depth attachment (below).
- Standard OpenXR session lifecycle: `XR_FORM_FACTOR_HEAD_MOUNTED_DISPLAY`, requires
  `XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO` (`backend.c:179`), `XR_REFERENCE_SPACE_TYPE_LOCAL`
  (`backend.c:356-372`), one swapchain per view with GL framebuffers + a `GL_DEPTH_COMPONENT` depth
  texture per view (`backend.c:375-475`).

**The XR frame loop** (`main.c:370-532`): textbook OpenXR — `xrWaitFrame` → `xrPollEvent` →
`wl_event_loop_dispatch` → render → `xrEndFrame`. `wxrc_xr_push_frame` (`main.c:79-142`) does
`xrLocateViews` → `xrBeginFrame` → per-view `xrAcquireSwapchainImage`/`xrWaitSwapchainImage` →
`wxrc_gl_render_xr_view` → `xrReleaseSwapchainImage` → `xrEndFrame` with one
`XrCompositionLayerProjection` over N `XrCompositionLayerProjectionView`s
(`XR_ENVIRONMENT_BLEND_MODE_OPAQUE`). Two frame-timing shortcuts confirm the thesis's
"draw-with-current-data" mode (Part 1 §1.6): `/* TODO: time from predictedDisplayTime */`
(`main.c:511, 521`), and frame-done is sent to clients *after* the compositor has already rendered
(`main.c:524-531`) — clients race to be ready for the next frame. `wxrc_xr_view_get_matrix`
(`xrutil.c:353-364`) **zeroes the head Y position** (`/* TODO: don't zero out Y-axis */`) — a
seated-height hack.

**The renderer** (`render.c`, GLES2 `#version 100` shaders). Three programs: a floor **grid**, an
RGB **texture** quad, and an external-OES texture quad (`render.c:16-92`). `wxrc_gl_render_view`
(`render.c:397-428`) clears, builds `vp = projection * view`, draws the grid, then — with
**`glDepthMask(GL_FALSE)`** so windows integrate into the scene without writing depth — iterates
mapped views back-to-front and draws each. **2D views** become textured quads: per `wl_surface`,
`render_surface_iterator` (`render.c:303-318`) computes a model matrix and draws the surface's
`wlr_texture` via `render_texture`, which reads GLES2 texture attribs directly
(`wlr_gles2_texture_get_attribs`, `render.c:239-240`) and handles `GL_TEXTURE_2D` vs
`GL_TEXTURE_EXTERNAL_OES` and Y-inversion. This is the thesis's "unmodified 2D apps on planes"
mechanism, concretely (Part 1 §1.3).

**The zxr protocol server** (`xr-shell-protocol.c`). Implements all four interfaces from
`zxr-shell-unstable-v1.xml`:
- `zxr_shell_v1`: `create_composite_buffer` + `get_xr_surface` (`:390-393`).
- `zxr_view_v1`: one `wl_global` per XR view (`:271-289`); wxrc advertises exactly the OpenXR stereo
  views as zxr views.
- `zxr_surface_v1.get_surface_view` allocates a per-(surface,view) `zxr_surface_view_v1` (`:316-343`).
- The compositor pushes MVP per frame: `xr_view_update_mvp_matricies` (`main.c:332-352`) computes,
  per XR view, `mvp = projection * inverse(view) * model` and calls
  `wxrc_zxr_surface_v1_send_mvp_matrix_for_view` → `zxr_surface_view_v1_send_mvp_matrix`
  (`xr-shell-protocol.c:298-314`). So **the compositor owns view+projection and the client's model
  transform, folds them, and hands the client one MVP per view** — matching Part 1 §1.8.
- `zxr_composite_buffer_v1` is the interesting part: it registers a **custom `wlr_buffer_impl`**
  (`composite_wlr_buffer_impl`, `:123-130`, registered `:441`) wrapping N per-view `wl_buffer`s keyed
  by `(view, buffer_type)` where `buffer_type ∈ {PIXEL_BUFFER, DEPTH_BUFFER}` (`:132-183`).

**Depth reality — the single most important nuance for the ADR.** Despite the protocol defining a
`DEPTH_BUFFER` type, wxrc's renderer **never consumes it**: `render_xr_shell_view`
(`render.c:331-356`) only ever fetches `ZXR_COMPOSITE_BUFFER_V1_BUFFER_TYPE_PIXEL_BUFFER` and draws
it as a **single fullscreen quad** at z-plane 0 (`glm_translate {-1,-1,0}`, `glm_scale {2,2,1}`),
with an explicit `/* TODO: Don't show on one view if we can't show on all views */`. So:
  1. wxrc's own scene depth uses a **real GL depth buffer** (`GL_DEPTH_ATTACHMENT` +
     `GL_OES_depth_texture`, `backend.c:469-474`, `render.c:441-442`) — it did **not** inherit
     Motorcar's depth-packed-into-color-viewport hack.
  2. The zxr client **depth-compositing path is specified but not wired** — xr-shell clients are
     effectively drawn as flat textured quads, not depth-composited 3D content. **3D content is
     proof-of-concept**, exactly as the README warns. Motorcar's actual depth-composited-3D-windows
     contribution (Part 1 §1.4) was therefore *designed into* zxr but never finished in wxrc.
The file is littered with `// TODO: assert`, `/* TODO: unref children */`, `// TODO: surface destroy
listener` — prototype maturity.

**The nested-output preview** (`main.c:230-283`): when run inside a host compositor (wlroots wayland
backend), wxrc also renders a flat monocular preview of view[0] to a normal `wl_output` and mirrors
the host pointer via `zwp_pointer_constraints_v1`. Debug affordance, not the XR path.

### 2.2 The "patch large swaths of the ecosystem" list, as evidenced in code

The README's patch list, cross-referenced against in-repo evidence. Confidence: **evidenced** = a
concrete code/build artifact requires it; **inferred** = consistent with the code but the patch
itself isn't in the tree (the shallow `depth=1` clone has only HEAD, so commit archaeology is limited
— doc 09 covers the upstream side).

| Component | What the patch was for (evidence) | Confidence |
|---|---|---|
| **Monado** | `XR_MNDX_EGL_ENABLE_EXTENSION_NAME` + `XrGraphicsBindingEGLMNDX` (`backend.c:84,281`) are Monado-specific; GL(ES)-on-OpenXR under Linux needed Monado to support the EGL-enable path and share wlroots' EGL context. The runtime is assumed to be Monado. | evidenced |
| **OpenXR** | Same extension must exist in the OpenXR headers/loader (`XR_MNDX_egl_enable`); in 2019 this was a fresh vendor extension not yet in a released `openxr` package. | evidenced (via the extension name) |
| **wlroots** | wxrc reaches into wlroots internals that were unstable/unexposed: `wlr_gles2_renderer_get_egl`, `wlr_gles2_texture_get_attribs` (`render.c`), `wlr_buffer_register_implementation` + a custom `wlr_buffer_impl` with `.is_instance/.initialize/.get_resource_size` (`xr-shell-protocol.c:123-130,441`), the `wlr_backend_autocreate(display, create_renderer)` **renderer-callback** signature (`main.c:406`), `wlr_backend_get_renderer` (`main.c:416`). Several of these needed patched/unstable wlroots in 2019 and **no longer exist** in modern wlroots (§2.5). | evidenced |
| **Mesa** | The thesis (Part 1 §1.5) says compositor depth-buffer access "would likely require modification of the implementation of Wayland EGL within Mesa." wxrc sidesteps client-depth entirely (§2.1), so the Mesa patch was more likely about the **EGL context / dmabuf / GLES depth-texture interop** with Monado than about client depth. The tree carries no Mesa patch; doc 09 resolves this against upstream. | inferred |
| **Vulkan** | No Vulkan in `meson.build`; the Vulkan patch (if real) is inferred to relate to the Monado runtime's own Vulkan backend, not wxrc itself. | inferred |
| **Xwayland** | wxrc has no `xwayland.c` (unlike wxrd). Any Xwayland patch was for running X apps as quads and is not exercised by wxrc's own source. | inferred |
| **Sway** | wxrc's structure (server.h/view/output/xdg-shell layout, `wlr_*` idioms) is the **tinywl/Sway reference-compositor pattern**; "patch Sway" most likely meant reusing/adapting Sway-era wlroots plumbing, not patching Sway the app. | inferred |

The firmly evidenced dependencies are **Monado + OpenXR (the `XR_MNDX_egl_enable` GLES/EGL path)** and
**a patched/old wlroots**. The rest (Mesa/Vulkan/Xwayland/Sway) are not demonstrable from wxrc's own
tree and are handed to doc 09's upstream analysis.

### 2.3 wxrd: the descendant dropped the 3D-windowing protocol

wxrd (`references/wxrd/`, Collabora, `README.md`: "prototype-quality standalone client for xrdesktop
based on wlroots and the wxrc codebase") is the documented descendant, but it made a **different
architectural choice** that matters for the ADR:

- **It abandoned zxr / 3D windowing entirely.** wxrd has no `protocol/` directory and no `zxr_*`
  code; its `meson.build:110-112` generates only `xdg-shell-protocol.h`. Its `src/` is
  `backend.c input.c main.c output.h server.h view.c wxrd-renderer.c xdg-shell.c xwayland.c` — i.e.
  ordinary 2D windows (xdg-shell + **Xwayland**, `xwayland.c`) textured into a VR scene. It is the
  **WayVR-style "2D windows in VR" model, not the Motorcar/zxr depth-composited-3D model.**
- **It renders through xrdesktop/gxr, not raw OpenXR.** `meson.build:59-90`: depends on `xrdesktop
  >=0.16` (and, per README, `gulkan`, `gxr`, `g3k`), with its own `wxrd-renderer.c`; README TODO
  even says "disable wlroots' gles renderer completely (it runs but is unused)". gxr is xrdesktop's
  OpenXR/OpenVR abstraction, so wxrd talks to the headset through Collabora's VR-overlay framework
  rather than driving an OpenXR projection layer itself.
- **wlroots 0.15** (`meson.build:68`, submodule) — newer than wxrc's 0.8 but still 2022-era.
- Input model (README): VR-controller ray cursor; a wlroots-created window captures the physical
  keyboard and forwards it to the focused VR window; alt+enter/alt+q/alt+right window management.
  Known-broken: Chromium-in-Xwayland, popup placement, subsurfaces ignored.

**Dependency health verdict:** mixed. wxrd itself last moved **2023-05-24** and is "prototype
quality," on wlroots 0.15. But its dependencies **xrdesktop and gxr both have 2026-01-12 commits** —
the Collabora VR framework is *alive* in 2026 even though wxrd is not. So option 2 in ADR 0006
("revive wxrd") means adopting a 2023 prototype on stale wlroots that routes through a still-maintained
but heavyweight xrdesktop/gulkan/gxr/g3k stack — and it would **not** give spatial-os the 3D-windowing
protocol, because wxrd already dropped it.

### 2.4 Motorcar (the ancestor): architecture in brief

`references/motorcar/` (QtWayland, last commit 2015). Confirms the Part 1 philosophy in code shape:
a reusable **compositor library** (`libmotorcar-compositor`) with a **scene graph**
(`SceneGraphNode`, the `virtual` vs `physical` node split, and `DepthCompositedSurfaceNode` doing the
actual depth-composite/clip), plus thin **device compositors** (the Rift+Hydra one) that instantiate
device classes and a window manager — built on the QtWayland Compositor API, with QtWayland isolated
behind wrapper classes (thesis §7.3.1). What wxrc **carried forward**: the protocol concepts
(shell/surface/viewpoint → zxr shell/surface/view), client-renders-2D-buffers, compositor-owned
view/projection, and the 2D-apps-on-quads mechanism. What wxrc **discarded**: Qt/QtWayland (→ wlroots),
the actual depth-compositing implementation (specified in zxr, not wired — §2.1), and the fully
realized cuboid/portal clipping (zxr has no clipping-mode arg at all — Part 1 §1.8).

### 2.5 Implications for spatial-os

**It is a rewrite, not a port.** Every wlroots touchpoint wxrc uses is from the pre-scene-graph,
pre-renderer-rework era and is gone or radically changed by wlroots 0.19:
- `wlr_backend_autocreate(display, create_renderer)` with a renderer callback (`main.c:406`) — the
  signature changed; renderer/allocator creation is now separate and explicit.
- `wlr_backend_get_renderer` (`main.c:416`) — **removed**; renderer is created by the app now.
- The custom `wlr_buffer_impl` / `wlr_buffer_register_implementation` API (`xr-shell-protocol.c`) —
  the buffer/allocator model was completely reworked (`wlr_buffer` is now import/dmabuf-centric).
- Direct GLES2 poking via `wlr_gles2_texture_get_attribs` and manual FBO/shader code (`render.c`) —
  still possible, but modern wlroots strongly pushes `wlr_scene` + `wlr_render_pass`, and a from-scratch
  GLES2 path is swimming upstream.
- The nested `wl_output` plumbing and manual `wl_output` global (`main.c:470`) predate `wlr_scene`
  output management.

**What is reusable** is the *design*, not the C: the OpenXR frame loop shape (`backend.c` + `main.c`
frame push), the projection-from-FOV math (`xrutil.c:313-337`), the zxr protocol XML itself, and the
compositor-owns-MVP model. The ~3,460 lines are small enough that a clean reimplementation against
wlroots 0.19 + a modern renderer is comparable in effort to porting, and yields a far better base.

**The 2026 OpenXR binding decision.** wxrc's GLES2 + `XR_MNDX_egl_enable` path is the fragile part:
it ties the compositor to Monado and to a GL interop that Vulkan never needed. A 2026 rewrite should
seriously consider a **Vulkan** renderer with `XR_KHR_vulkan_enable2`, which (a) removes the
`XR_MNDX_egl_enable` dependency, (b) matches the rest of the spatial-os XR stack (Monado's main
compositor is Vulkan; StardustXR/`dmatex` is Vulkan+dmabuf+syncobj per doc 10), and (c) gives clean
dmabuf + explicit-sync buffer sharing — which is also how a *real* zxr depth path would finally be
wired (doc 09 resolves whether GL-on-OpenXR is still viable at all in 2026).

**Concrete cost signals for ADR 0006:**
- wxrc: ~3.5k LOC, wlroots 0.8, GLES2+EGL+Monado-only, zxr depth path unfinished, prototype TODOs
  throughout → **design document, not a codebase to port**.
- wxrd: 2023 prototype, wlroots 0.15, routes through xrdesktop/gxr, **already dropped zxr** → not a
  path to the 3D-windowing protocol the user wants to continue.
- The reusable, durable assets are the **zxr protocol** (to be revised per doc 10's `zxr-shell-v2`
  proposal) and the **thesis design** (Part 1) — plus WayVR/wxrd as proof that the 2D-windows-in-VR
  tier needs no new protocol and can ship first while the 3D-windowing protocol is rebuilt.
