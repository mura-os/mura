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

---

## Part 3 — Protocol inventory for the v2 draft (added 2026-09-23, specification workstream)

Drafting brief for `protocols/zxr-shell-v2.xml`. This section turns Parts 1–2, the three ancestor
protocol files, and the repo's committed requirements
(`docs/architecture/zxr-shell-v2-composition.md` §2/§5/§7.2/§7.4/§8,
`docs/research/10-xr-wayland-protocol-comparison.md` §4.5,
`docs/architecture/adr/0006-compositor-strategy.md` §The protocol) into a keep/rework/drop/add map.
"Wired" verdicts are per Part 2 §2.1 against `references/wxrc/src/xr-shell-protocol.c`.

### 3.1 v1 inventory — every interface and message

Source: `references/wxrc/protocol/zxr-shell-unstable-v1.xml`. Note the protocol element itself is
named `xr_shell_unstable_v1` (no `z`); only interfaces carry the `zxr_` prefix, and the protocol
description is literally "TODO: Describe overall protocol here".

| v1 construct | Purpose | Wired in wxrc? | v2 verdict |
|---|---|---|---|
| `zxr_view_v1` (global) | one perspective (eye/view); N globals generalize motorcar's per-eye viewpoint | yes — one `wl_global` per OpenXR view (`xr-shell-protocol.c:271-289`) | **KEEP** — the N-view model is v1's best idea (ADR 0006 §The protocol "keep"); gains resolution/fov/projection events and a real removal lifecycle |
| `zxr_view_v1.destroy` | client releases the view | handler is an empty `// TODO` (`xr-shell-protocol.c:239-242`) | **KEEP** (trivial destructor) |
| `zxr_shell_v1` (global) | entry point: buffer factory + role assignment | yes (`xr-shell-protocol.c:390-393`) | **KEEP** — becomes the negotiation root (transport tiers, depth encoding — composition §5) |
| `zxr_shell_v1.get_xr_surface(surface)` | assign the XR role to a `wl_surface` | yes (`:356-388`) — but with `// TODO: surface destroy listener` | **KEEP** — role factory survives verbatim in concept |
| `zxr_shell_v1.create_composite_buffer` | make an empty per-view buffer collection | yes (`:220-237`) | **REWORK** — becomes creation of a negotiated buffer *pool/slot* (composition §7.2 `acquire_reusable_slot`), not an untyped collection |
| `zxr_surface_v1` | the 3D surface role; 2D buffer attach is a protocol error | yes (`:344-388`); the `invalid_buffer` error is defined but no enforcement exists in the shell module | **KEEP** (rework details) — gains bounds/clipping state, configure/ack lifecycle, a world-transform event, real errors |
| `zxr_surface_v1.get_surface_view(view)` | per-(surface,view) object | yes (`:316-343`) | **REWORK** — the surface×view addressing survives (per-view render targets need it), but the object's one job changes (below) |
| `zxr_surface_view_v1` | carrier of per-view state | yes; resource created with a **NULL implementation** — the interface has no requests at all (`:339`), not even a destructor | **REWORK** — survives only as per-view target addressing; loses its single event |
| `zxr_surface_view_v1.mvp_matrix` (event) | server pushes one folded model·view·projection per surface×view | yes — sent per frame (`main.c:332-352` → `xr-shell-protocol.c:298-314`) | **DROP** — the fold is v1's documented regression (doc 10 §4.1); replaced by motorcar's split delivered in the atomic frame snapshot (§3.4 item 6) |
| `zxr_composite_buffer_v1` | N per-view 2D buffers under one handle; coord space (-1,-1)..(1,1) | attach/lookup wired via a custom `wlr_buffer_impl` (`:123-183,441`) | **REWORK** — the *typed colour+depth per view* concept is the T1 core (composition §2) and survives; the container semantics change wholesale |
| `…buffer_type` enum (`pixel_buffer`, `depth_buffer`) | typed buffers — v1's second-best idea | types accepted (`:139-145`) but **depth is never consumed**: the renderer fetches only `PIXEL_BUFFER` and draws a flat quad (Part 2 §2.1; `render.c:331-356`) | **KEEP** — extended with depth-encoding metadata (§3.4 item 2) |
| `…attach_buffer(view, wl_buffer?, type)` | attach/update/clear one buffer per (view,type) | yes (`:132-183`), incl. null-detach | **REWORK** — per-buffer trickle attach violates composition §7.2's atomicity ("never four buffers … hoping their commits line up"); becomes population of a slot submitted atomically |
| `…get_wl_buffer` | wrap the collection as a `wl_buffer` for `wl_surface.attach` | yes (`:190-205`) — the wrapper is why wxrc needed the now-removed `wlr_buffer_register_implementation` API (Part 2 §2.5) | **DROP** — atomic `submit_spatial_frame(frame_id, slot)` (composition §7.2) replaces the attach-a-wrapper lifecycle; the indirection was also the biggest source of v1's implementation coupling |
| `error` enums (surface + composite buffer, both `invalid_buffer`) | 2D-buffer-on-XR-surface; bad attach | attach-type check only (`:139-145`) | **REWORK** — into a real error taxonomy (§3.4 item 11) |

**v1's own TODO comments — the protocol knew its gaps.** All in
`references/wxrc/protocol/zxr-shell-unstable-v1.xml`:

1. "TODO: Describe overall protocol here" — no normative overview at all.
2. `zxr_view_v1`'s description ends mid-sentence: "If a view global is removed and the client" —
   view removal was never specified; worse, `attach_buffer`'s text references a
   **`zxr_view_v1.finished` event that does not exist** in the file. View lifecycle is a hole, not
   just a TODO.
3. `<!-- TODO: Resolution event (and subpixel?) -->` on `zxr_view_v1` — clients can't size buffers.
4. `<!-- TODO: Explain what a model view projection matrix is? -->` on `mvp_matrix`.
5. `<!-- TODO: From which view's perspective? -->` on the composite-buffer coordinate space — the
   (-1,-1)..(1,1) space is undefined for stereo.
6. The trailing block: "Add better timing information? Prepare frames in advance? Map OpenXR more
   closely onto this protocol", "3D geometry buffers, e.g. glTF", "2D buffers with left/right
   views, for e.g. 3D movies".

Items 2, 3, 5, 6(timing) are obligations on v2; 6(glTF) is an explicit non-goal (§3.6).

### 3.2 Motorcar concepts to resurrect (that v1 dropped)

From `references/motorcar/src/protocol/motorcar.xml`, mapped to composition-doc needs:

- **The model/view/projection split** — `motorcar_viewpoint.view_matrix` (per frame) +
  `projection_matrix` (on change), with the window's model transform a separate
  `motorcar_surface.transform_matrix` event. This is composition §2 constraint 1 verbatim
  (`clip = P_e · V_e · T_i`, compositor-owned P/V, per-app T): the split is what makes every
  client's depth *comparable*, so it is a T1 correctness requirement, not a style preference.
- **Cuboid/portal clipping** — the `clipping_mode` enum on `get_motorcar_surface`. Composition §2
  constraint 3 makes clipping cooperative *and* enforced (early visibility resolve is lossy, so
  clipping must precede submission); doc 10 §4.5 Q4 makes it normative for depth trust. v1 has no
  clipping at all.
- **3D size negotiation** — `request_size_3d`/`set_size_3d` (compositor requests, client chooses a
  fitting size). Resurrected through zwin's configure/ack serial idiom (§3.3); feeds composition
  §7.2's "window bounds" in the frame snapshot and §8's resolution/bandwidth concern.
- **6DoF pointer input** — `motorcar_six_dof_pointer` (enter/leave/motion/button with vec3 position
  + 3×3 orientation, surface-local). v1 defines *no* input; a shell cannot be built on it as
  written (doc 10 §4.1). ADR 0006 restores this alongside the ray device.
- **The depth-compositing opt-in** (`enable_depth_compositing` arg) — resurrect as *negotiation*
  rather than a boolean: in v2, depth is mandatory for the 3D tier but its format/encoding is
  negotiated (composition §5).
- **Do not resurrect:** `view_port` (the packed depth-viewport hack is protocol-visible EGL
  archaeology — Part 1 §1.5; dmabuf+syncobj retires it, ADR 0006), and the implicit double-wide
  stereo buffer layout (superseded by per-view typed buffers; an optional multiview/array layout
  returns only as a negotiated capability, §3.4 item 12).

### 3.3 zwin lessons (`references/zwin/protocol/`)

**Adopt:**

- **The `wl_pointer`-shaped ray** (`zwin.xml` `zwn_ray`): enter/leave/motion (origin+direction
  vec3), button, and the *complete* axis suite (axis/axis_source/axis_stop/axis_discrete) plus a
  `frame` grouping event — the most portable input design in the lineage (doc 10 §4.2), and the
  second seat capability beside the 6DoF pointer (ADR 0006).
- **Seat capability advertisement** (`zwn_seat.capabilities` bitfield) — matches how v2 must
  advertise ray vs 6DoF vs future hand input.
- **xdg-shell idioms on the 3D shell surface** (`zwin-shell.xml` `zwn_bounded`):
  `configure(half_size, serial)` + `ack_configure`, `set_title`, `move(seat, serial)` — the
  serial-acknowledged state dance v2's bounds negotiation should borrow, and the shape (bounded vs
  expansive) that maps onto cuboid vs unbounded windows.
- **Craft lesson:** zwin's descriptions are mostly bare documentation URLs (`zwn_compositor`'s
  summary is a GitHub link). v2's XML must be normative and self-contained.

**Avoid:**

- **The protocol-per-graphics-API coupling** — `zwin-gles-v32.xml` hardcodes *OpenGL ES 3.2 itself*
  into the wire: `zwn_gl_shader.type` uses raw GL enum values (0x8830/0x8831), `zwn_gl_texture.image_2d`
  takes GL format/type ints, `zwn_gl_base_technique` serializes draw calls. Any GL version bump or
  non-GL client is a new protocol. Our negotiated-transport design (composition §5) is the direct
  counter: the wire fixes *meaning* (colour, depth encoding, sync points) and negotiates
  format/modifier/handle-type per client — GL, Vulkan, and CPU clients ride the same interfaces.
  This is the single most instructive mistake in the corpus (§3.7c).
- **The parallel object world** — zwin re-implements shm (`zwn_shm`/`zwn_shm_pool`/`zwn_buffer`)
  because its buffers carry vertex/shader data, not pixels. v2 stays on `wl_buffer`/linux-dmabuf
  precisely so it inherits the ecosystem (doc 10 §4.2 "Wayland extended by one role").
- **Client-uploaded hit-test regions** — `zwn_region.add_cuboid/add_sphere` (with its own
  `FIXME: hierarchical node` admitting it was underpowered). Hit-test geometry stays
  compositor-derived from surface bounds (ADR 0006), unless a real need appears (doc 10 §4.5 Q3).

### 3.4 The v2 requirements matrix

Each contract item from composition §7.2/§7.4 and each open question from composition §8 /
doc 10 §4.5, against the ancestor construct that addresses it:

| # | v2 requirement (source) | v1 | motorcar | zwin | v2 disposition |
|---|---|---|---|---|---|
| 1 | Typed colour+depth pair per view (composition §7.2) | `zxr_composite_buffer_v1.buffer_type` — right types, dead depth path, non-atomic attach | depth packed into a colour viewport (hack) | n/a (server renders) | REWORK v1: typed per-view images in an atomically-submitted slot |
| 2 | Depth *encoding* negotiation — reversed-Z, near/far, normalization (composition §2 c.2, §8) | none | none | none | **NEW** — copy `XrCompositionLayerDepthInfoKHR`'s metadata shape (minDepth/maxDepth/nearZ/farZ) |
| 3 | Explicit sync, syncobj timeline points (composition §5) | none — v1 predates syncobj | none | none | **NEW** — `wp_linux_drm_syncobj_v1` points per slot; wire vocabulary already exists in-repo: `protocols/zspatial-toplevel-export-v1.xml` `attach`/`release_buffer` (timeline fd + point_hi/point_lo) |
| 4 | Transport tiers: dmabuf / opaque-fd / shm-CPU, allocation ownership (composition §5) | implicit `wl_buffer` only | implicit EGL double-wide | own shm stack (anti-pattern) | **NEW, narrowed at rev 2** — shm + dmabuf only; opaque-fd and allocation-ownership negotiation deferred to a later protocol revision (the XML says so normatively; composition §5's full tier set remains the design goal) |
| 5 | Frame timing: snapshot, predicted display time, commit cutoff (composition §7.4) | none — the trailing TODO | thesis §7.3.2 discussion only, nothing on the wire | `zwn_virtual_object.frame(wl_callback)` — 2D pacing, no prediction | **NEW** — share vocabulary with `zspatial_export_pacing_v1`: `frame(display_time, period_ns, cutoff_ns)` + `presented`/`discarded` feedback |
| 6 | View/projection delivery (composition §2 c.1, §7.2) | folded `mvp_matrix` per surface×view — DROP | `view_matrix`/`projection_matrix` split per viewpoint — RESURRECT | clients never see a camera | split matrices delivered **inside the atomic frame snapshot**, paired with frame id (see §3.7d) |
| 7 | Size/bounds negotiation (composition §7.2 "bounds") | none — dropped | `request_size_3d`/`set_size_3d` | `configure/ack_configure` serial idiom | RESURRECT motorcar semantics in zwin's idiom |
| 8 | Clipping modes, enforced (composition §2 c.3; doc 10 §4.5 Q4) | none | `clipping_mode` cuboid/portal | bounded/expansive shell types | RESURRECT — enum + normative out-of-bounds behavior (clamp vs error must be decided in the draft) |
| 9 | Ray + 6DoF input (ADR 0006; v1 dropped motorcar's!) | none | `motorcar_six_dof_pointer` | `zwn_ray` + seat capabilities | **NEW interfaces** synthesized from both, as two seat capabilities |
| 10 | Surface roles + lifecycle (composition §7.2) | role factory + 2D-buffer error KEEP; but no destructors on surface/surface-view, dangling `finished` reference, no destroy listener in wxrc | one-motorcar-surface-per-wl_surface rule | `role`/`invalid_state` errors, `unconfigured` | REWORK — full lifecycle: destructors everywhere, view add/remove events, mapped/unmapped states |
| 11 | Error conditions | two enums, both `invalid_buffer` | **zero** error enums | per-interface typed errors | **NEW taxonomy** — per interface; typed denial precedent in `zspatial_exported_tree_v1.denied` |
| 12 | Multiview/layout option (v1 TODO "left/right views"; composition §8 bandwidth) | TODO only | double-wide layout convention | n/a | per-view images primary; **dropped from rev 2 entirely** (a capability with no wire contract is worse than absence — red-team F17); a layered/array layout returns only with a full attachment contract in a later revision |

Doc 10 §4.5's remaining questions land as follows: Q1 (pacing across clients) is item 5 plus the
composition §7.4 scheduling rule; Q2 (2D toplevel → 3D placement) is **out of v2** — an
`xdg_toplevel` needs no XR interface (ADR 0006 "free 2D"), placement is compositor policy and the
places model; Q3 (geometry on the wire) is a non-goal (§3.6); Q4 is item 8; Q5 (depth-dmabuf driver
matrix) is item 4's negotiate-and-fall-back, per composition §5.

### 3.5 Naming continuity

The draft should read as v1 grown up. Proposal:

- **Survive with a version bump:** `zxr_shell_v1` → `zxr_shell_v2`; `zxr_view_v1` → `zxr_view_v2`;
  `zxr_surface_v1` → `zxr_surface_v2`. These three carry the lineage's identity (the user
  co-authored them; ADR 0006 names the family) and their concepts survive intact.
- **Fix the protocol element name:** v1's `<protocol name="xr_shell_unstable_v1">` lacks the `z`
  its own interfaces carry; v2 uses `zxr_shell_v2` consistently (the `z`/version conventions in
  v1's own description text apply — drop them only at stabilization).
- **Renames justified by changed semantics:** `zxr_composite_buffer_v1` → a pool/slot pair (e.g.
  `zxr_buffer_pool_v2` + a slot/submission object): "composite buffer" described a `wl_buffer`
  wrapper that no longer exists once submission is an atomic frame request (§3.1). Keeping the old
  name would promise the old lifecycle. `zxr_surface_view_v1` → keep the name `zxr_surface_view_v2`
  only if the surface×view object survives as target addressing; if per-view targets are expressed
  purely inside the slot, drop the interface rather than keep a hollow name.
- **New interfaces take v1's naming shape, not motorcar's or zwin's:** `zxr_frame_v2` (timing
  snapshot), `zxr_pointer_6dof_v2` and `zxr_ray_v2` (seat capabilities), `zxr_seat_v2` if the seat
  extension point is ours. Event/arg vocabulary for pacing and sync copies the sibling
  `protocols/zspatial-toplevel-export-v1.xml` (`display_time_hi/lo`, `period_ns`, `cutoff_ns`,
  `presented`/`discarded`, timeline-fd + `point_hi/lo`) so the two spatial-os protocols read as one
  family where semantics overlap.
- **Vocabulary continuity from motorcar** where concepts return: `clipping_mode` with `cuboid` and
  `portal` entries keeps the thesis terminology (Part 1 §1.3) that the whole doc set already uses.

### 3.6 Explicit non-goals for v2 (deferred, with rationale)

- **Transparency tiers T2+** — ordered per-pixel samples / deferred stochastic profiles are
  additive negotiated capabilities by design (composition §1, §3); the T1 opaque contract must ship
  and stabilize first. The draft reserves capability negotiation but defines no T2 interfaces.
- **Geometry on the wire** — v1's glTF TODO is *rejected*, not deferred-by-default: the lineage's
  value is exactly that geometry never crosses the wire (composition §2; doc 10 §4.5 Q3). An asset-
  reference sidecar, if ever, is a separate protocol.
- **Per-client reprojection** — composition §3's T3 honesty rule: the compositor never silently
  reuses a stale eye-space depth image; slow clients get current-frame-or-placeholder (§7.4). A
  declared-reprojection-validity capability is future work.
- **Places/workspace semantics** — v2 surfaces get *world transforms* (the model-transform event);
  parenting to typed frames, place membership, and reparent verbs are
  `docs/architecture/places-model.md` §2's layer, exposed via ext-workspace plus a **separate zxr
  workspace extension whose XML explicitly follows zxr-shell-v2's drafting** (places-model §9).
  The boundary: v2 says *where a surface is*; places says *what it belongs to*.
- **A11y semantics** — deferred to the zspatial-a11y workstream; v2 must merely not preclude it.
- Also out (composition §7.5): unmodified-app interception, multi-GPU, curved panels (a
  presentation policy, not surface state).

### 3.7 Summary for the drafting brief

- **(a) v1 verdict counts:** interfaces — 3 KEEP (`zxr_shell`, `zxr_view`, `zxr_surface`),
  2 REWORK (`zxr_surface_view`, `zxr_composite_buffer`), 0 DROP. Messages — 2 KEEP (`view.destroy`,
  `get_xr_surface`), 3 REWORK (`create_composite_buffer`, `get_surface_view`, `attach_buffer`),
  2 DROP (`mvp_matrix`, `get_wl_buffer`). Nothing in v1 is conceptually dead — but only two of its
  seven messages survive unchanged, and its two DROPs are exactly its two 2019 shortcuts (the
  folded MVP and the buffer-wrapper indirection).
- **(b) The single biggest v1 gap composition requires v2 to fill:** the **atomic frame contract**
  — composition §7.2's non-negotiable "both eyes, both colour and depth, and their view metadata
  are one atomic submission" bound to §7.4's predicted-display-time/cutoff loop, with explicit
  sync. v1 has *none* of this: per-buffer trickle attach, no timing (its own trailing TODO), no
  sync primitive (it predates syncobj), and "latest matrix" racing "latest colour" by construction.
  Input is the biggest *feature* gap, but the frame contract is the gap that makes T1 correctness
  impossible without it.
- **(c) The zwin mistake most worth documenting as avoided:** `zwin-gles-v32.xml`'s
  graphics-API-and-version-in-the-wire coupling (raw GL enums as protocol args, a GLES-3.2
  interpreter as the compositor). v2's transport tiers (composition §5) exist precisely so the
  protocol fixes meaning and negotiates transport — a GL client, a Vulkan client, and a CPU client
  on one wire contract (composition §5's acceptance test).
- **(d) The conflict the draft must resolve:** **matrix delivery channel.** Doc 10 §4.4 (and ADR
  0006 following it) restores motorcar's split as *events on the view global* — `view_matrix` per
  frame, `projection_matrix` on change, free-running. Composition §7.2 forbids exactly that
  pairing pattern: per-view P·V must arrive *inside the atomic frame snapshot*, tied to a frame id
  — "never 'latest matrix' paired with 'latest colour'". The draft has to pick the snapshot model
  (motorcar's *split* survives; motorcar's *delivery channel* does not), leaving view-global events
  only for slow-changing capability state (resolution, fov). The related soft tension — doc 10
  §4.5 Q1 floats "reprojected stale colour+depth" for slow clients, which composition §3/§7.4
  categorically forbids — should be resolved in the draft's timing section in composition's favor.
