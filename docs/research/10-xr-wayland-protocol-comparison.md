# 10 — XR/Wayland protocol comparison: motorcar, zxr, zwin, StardustXR, WayVR

**Date:** 2026-09-22. Research pass B3 for Mura. This document compares the five known
architectures for putting multiple applications into one shared XR space on Linux, to ground
**ADR 0006 (compositor strategy)**. It builds directly on
[08-wxrc.md](08-wxrc.md) Part 1 (the Motorcar→wxrc design lineage and its terminology:
*windowing system not VR app*, *cuboid/portal/unbounded windows*, *depth-viewport hack*) and
[05-xr-userspace.md](05-xr-userspace.md) (Monado as the runtime; StardustXR's missing in-tree
Wayland compositor, §2.3/§11).

Primary sources (local clones; paths relative to repo root):

- `references/motorcar/src/protocol/motorcar.xml` (2014, last commit 2015-12-17)
- `references/wxrc/protocol/zxr-shell-unstable-v1.xml` (2019, wxrc last commit 2021-07-08)
- `references/zwin/protocol/{zwin,zwin-shell,zwin-gles-v32}.xml` (2022, last commit 2023-01-16)
  and reference compositor `references/zen/` (last commit 2024-02-11)
- `references/stardustxr-server/` (Rust/Bevy, last commit 2026-09-14 — active)
- `references/wayvr/` (Rust/smithay, last commit 2026-09-20 — active)

Web sources are cited inline as links. Commit dates are from `git log -1` on the clones.

---

## 1. The five models, and where `wp_drm_lease_v1` fits

The design space is a spectrum of *how much 3D semantics crosses the client↔compositor wire*:

```
 nothing ──────────── thin 3D metadata ─────────── rich 3D objects ────── full scene graph
 WayVR          motorcar (2014)   zxr (2019)         zwin (2022)         StardustXR (2020–)
 (no protocol;  (per-surface 3D   (motorcar redone   (virtual objects,   (leaves Wayland
 flat quads in   transform, depth  for OpenXR/        geometry regions,   entirely: node/
 an overlay      compositing,     wlroots: N views,   serialized GLES     aspect/method IPC
 session)        6DoF input)      typed buffers)      command stream)     over Unix sockets)
```

All five need the same thing *underneath*: exclusive access to the HMD display and pose data.
That layer is already solved and shipped, and is **orthogonal to this comparison**:
[`wp_drm_lease_v1`](https://wayland.emersion.fr/protocol/drm-lease-v1.html) (staging,
wayland-protocols ≥ 1.22) lets a desktop compositor lease the headset's
[non-desktop DRM connector](https://drewdevault.com/blog/DRM-leasing-and-VR-for-Wayland/) to the
OpenXR runtime; Monado consumes it in `comp_window_direct_wayland.c` (doc 05 §5). It hands a
*display* to a *runtime* — it says nothing about how *apps* share a 3D space. On the Mura
appliance, where the panel is the only display, Monado's `VK_KHR_display` backend can own the DRM
device with no host compositor at all (doc 05 §5, §11 Q1); `wp_drm_lease_v1` matters mainly for
the desktop/dev profile where Mura components run under an existing compositor. Either way,
every model below sits *above* this layer as an OpenXR client (or, in motorcar's 2014 case, its
pre-OpenXR equivalent).

## 2. Model characterizations

### 2.1 motorcar (2014) — view-dependent, depth-composited 3D windows

`references/motorcar/src/protocol/motorcar.xml`, four interfaces:

- `motorcar_shell.get_motorcar_surface(surface, clipping_mode, enable_depth_compositing)` puts a
  3D role on an ordinary `wl_surface`.
- `motorcar_surface`: a 3D box-shaped window with its own local space; `clipping_mode` enum =
  **cuboid | portal** (doc 08 §1.3's three window interpretations; unbounded is the degenerate
  case); `transform_matrix` event (column-major 4×4, metres — the window's *model* matrix);
  `request_size_3d`/`set_size_3d` negotiate 3D extent like 2D size negotiation.
- `motorcar_viewpoint`: one global per eye. `view_matrix` (per frame), `projection_matrix` (on
  bind, rarely resent), and `view_port` — *two* viewports per viewpoint, color and depth, because
  the depth buffer is packed into a second color region of the same buffer ("since EGL does not
  support a color mode that includes depth", per the XML). This is the **depth-viewport hack**
  of doc 08 §1.5, protocol-visible.
- `motorcar_six_dof_pointer`: enter/leave/motion/button carrying a 3-vector position + column-major
  3×3 rotation (metres) — a complete 6DoF analog of `wl_pointer`, delivered in surface-local terms.

Client renders both eyes plus packed depth into one double-wide `wl_buffer`; the compositor
unpacks depth and composites all clients with the GPU depth test, stencil-clipped to window
bounds. Model/view/projection stay **separate**: compositor owns view+projection, client owns its
model transform. Status: thesis prototype on QtWayland, dead since 2015.

### 2.2 zxr_shell_unstable_v1 (2019) — motorcar minus input, generalized to N views

`references/wxrc/protocol/zxr-shell-unstable-v1.xml`, by Drew DeVault / Simon Ser for wxrc:

- `zxr_view_v1`: a global per view — not "per eye"; any number, so one protocol covers mono
  debug windows, stereo HMDs, or CAVE-style N-view rigs.
- `zxr_shell_v1.get_xr_surface(surface)` assigns the XR role; attaching a plain 2D buffer to an
  XR surface is a **protocol error** — 2D and 3D surfaces are disjoint by construction.
- `zxr_surface_view_v1.mvp_matrix`: one folded model-view-projection matrix (row-major 4×4) per
  surface × view, updated by the server.
- `zxr_composite_buffer_v1`: built by `attach_buffer(view, wl_buffer, buffer_type)` where
  `buffer_type` = **pixel_buffer | depth_buffer**, then materialized via `get_wl_buffer` and
  attached with the ordinary `wl_surface.attach`. Depth becomes a *typed first-class buffer*
  riding standard `wl_buffer` transport (shm or, in principle, linux-dmabuf) instead of a packed
  color viewport.

What's missing is written in the file itself: no input interfaces at all, and a trailing TODO
block — "Add better timing information? Prepare frames in advance? Map OpenXR more closely onto
this protocol", "3D geometry buffers, e.g. glTF", "2D buffers with left/right views". The
surface-local coordinate note "(-1,-1) to (1,1) … <!-- TODO: From which view's perspective? -->"
is unresolved. Status: explicitly experimental; wxrc froze in 2021.

### 2.3 zwin (2022) — server-side virtual objects, geometry regions, and a GLES wire format

Three protocol files under `references/zwin/protocol/`, implemented by the
[Zen reference compositor](https://www.zwin.dev/what_is_it/what_is_zwin) (`references/zen/`):

- **`zwin.xml` (core)**: `zwn_compositor.create_virtual_object` / `create_region`;
  `zwn_virtual_object` with `commit` and `frame(wl_callback)` — deliberately `wl_surface`-shaped,
  including reuse of `wl_callback` for frame pacing; its **own shm stack**
  (`zwn_shm`/`zwn_shm_pool`/`zwn_buffer`, sizes passed as `off_t` arrays for 64-bit) instead of
  `wl_shm`, because these buffers hold vertex/texture/shader *data*, not pixels;
  `zwn_region.add_cuboid(half_size, center, quaternion)` / `add_sphere(center, radius)` —
  **server-side 3D hit-test geometry**, the 3D analog of `wl_region` (a `FIXME: hierarchical
  node` comment shows they knew it was underpowered); `zwn_seat.get_ray` + `zwn_ray` with
  enter/leave/motion (origin + direction vec3s), button, and the full `wl_pointer` axis suite
  (axis/axis_source/axis_stop/axis_discrete/frame) — **ray input modeled exactly on wl_pointer**,
  hit-tested by the compositor against client-declared regions.
- **`zwin-shell.xml`**: `zwn_shell.get_bounded(virtual_object, half_size)` and `get_expansive` —
  motorcar's cuboid vs unbounded window, rebuilt with xdg-shell idioms: `configure(half_size,
  serial)` + `ack_configure`, `set_title`, interactive `move(seat, serial)`. This is the most
  Wayland-idiomatic 3D shell surface spec of the five.
- **`zwin-gles-v32.xml`**: the radical part. Clients do not render. They upload GL buffers,
  compile shaders from `zwn_buffer` contents, build programs, textures (`image_2d`), samplers,
  vertex arrays, and a `zwn_gl_base_technique` with uniforms and `draw_arrays`/`draw_elements`
  calls — a **serialized OpenGL ES 3.2 command/resource stream executed by the compositor**.
  The server owns the entire render loop and camera; clients never see a view matrix.

That last file is the philosophical break: motorcar/zxr coordinate *projections* of
client-rendered pixels; zwin ships the *scene* (geometry + shaders + draw calls) to a
server-side renderer. It buys Zen real powers — per-view rendering without client round-trips,
and `references/zen/znr-remote` even streams the render to a Quest over the network ("Zen
Mirror") — at the cost of a huge compositor (a GL state machine per client) and a capped
rendering model (clients can only do what the technique abstraction expresses; no client-side
ray tracing, no custom pipelines — compare doc 08 §1.4's "clients render however they like").
Status: [first release Jan 2023](https://www.zwin.dev/roadmap); the planned Jan 2024 second
release never shipped; an [open Dec 2024 issue asks for a revised 2025/2026 roadmap](https://github.com/zwin-project/zwin.dev/issues/52);
zwin repo idle since 2023-01, zen since 2024-02. Effectively dormant.

### 2.4 StardustXR — leave Wayland: an object scene-graph IPC

Not a Wayland protocol at all. The [technical overview](https://stardustxr.org/docs/dive-deeper/deep-overview)
describes it plainly: Unix-domain-socket IPC with a
[FlatBuffers message envelope and FlexBuffers payloads](https://docs.rs/stardust-xr-wire/latest/stardust_xr_wire/),
addressing an **object-oriented scene graph** by `(node_id, aspect, method)` with one-way
*signals* and request/response *methods*, fd-passing included; protocol definitions are KDL files
in [StardustXR/core](https://github.com/StardustXR/core/blob/main/WARP.md), with generated Rust
bindings. The current server (`references/stardustxr-server/`, v0.52) has migrated the transport
to its `gluon-ipc`/`strong-ipc` crates (`Cargo.toml` lines 151–169) but the model is unchanged:
clients create persistent server-side nodes —

- **Spatials** (`src/nodes/mura.rs`): transform nodes with parent-child relationships, mapped
  1:1 onto Bevy ECS entities server-side;
- **Fields** (`src/nodes/fields.rs`): signed-distance-field shapes for input/intersection queries
  (`RayMarchResult`, `FieldSample`);
- **Drawables** (`src/nodes/drawable/`): server-loaded **glTF models by resource path**
  (`model.rs` loads `.glb`/`.gltf`), lines, text, sky — *assets* cross the wire (or just paths),
  not draw calls;
- **Dmatex** (`src/nodes/drawable/dmatex.rs`): dmabuf texture import with **DRM syncobj timeline
  explicit sync** (`timeline_syncobj` crate) — the most modern buffer path of any project here.

Input is mediated by SUIS (Spatial Universal Interaction System): input *methods* and input
*handlers* are both nodes, and the server routes between them — richer than any pointer/ray
model, and fully compositor-arbitrated. The server itself is an OpenXR client of Monado/WiVRn
(doc 05 §2.3). The 2D story is the known gap: **no in-tree Wayland compositor in the current
revision**; 2D apps get `FLAT_WAYLAND_DISPLAY` forwarded to a *client* app (Flatland) that is
expected to be the panel compositor (doc 05 §2.3, §11 Q2 — the packaging/decision hole Mura
flagged). Status: actively developed through September 2026.

### 2.5 WayVR / wlx-overlay lineage — the zero-new-protocol baseline

`references/wayvr/` ([formerly WlxOverlay-S](https://github.com/wayvr-org/wayvr)). Defines **no
protocol**. Two mechanisms:

- Existing desktop screens are captured via PipeWire/wlr protocols (`wlx-capture` crate) and shown
  as floating panels.
- The "WayVR" subsystem proper (`wayvr/src/backend/wayvr/comp.rs`) is an **embedded smithay
  Wayland compositor**: full `xdg_toplevel`/popup handling, KDE decoration, dmabuf import with
  feedback (`DmabufState`, `get_dmabuf`), its own `WAYLAND_DISPLAY` + Xwayland; launched apps
  render into it and each toplevel is textured onto a panel.

Output goes to OpenXR as **flat quads**: the OpenXR backend requires
[`XR_EXTX_overlay`](https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html#XR_EXTX_overlay)
(`backend/openxr/helpers.rs` line 40, `create_overlay_session`) and submits
`xr::CompositionLayerQuad` layers per panel (`backend/openxr/overlay.rs` line 174). Input is the
overlay's own laser pointer, converted to synthetic 2D pointer/keyboard events into the embedded
compositor (`hit_test.rs`, `input.rs`). Apps are 100% unmodified; there is no 3D-app story at all
— by design. It runs *alongside* another OpenXR app (a game) rather than being the session.
Status: the most active project in this space (last commit 2026-09-20; packaged in nixpkgs and
nixpkgs-xr, doc 05 §2.4).

## 3. The matrix

| Dimension | motorcar (2014) | zxr_shell_unstable_v1 (2019) | zwin (2022) | StardustXR (2020–) | WayVR / wlx (2021–) |
|---|---|---|---|---|---|
| **Rendering model** | Client renders 2D (both eyes + depth); compositor depth-composites | Client renders 2D per view; compositor depth-composites | **Server renders 3D**: client ships GL resources + draw calls (`zwin-gles-v32.xml`) | Server renders scene-graph nodes (glTF models, text, lines) declared by clients | Textured overlay: embedded compositor's 2D output onto flat OpenXR quads |
| **Geometry over the wire** | None — buffers only + 3D size/transform metadata | None — buffers only + MVP metadata (glTF a TODO) | Yes: vertex buffers, shaders, draw calls; plus hit-test regions (cuboid/sphere) | Yes: model assets/paths, SDF field shapes, spatial hierarchy | None |
| **Buffer transport** | `wl_buffer` (double-wide, depth packed in color viewport) | `wl_buffer` per view per type via `zxr_composite_buffer_v1` (dmabuf-capable in principle) | Custom `zwn_shm` (not `wl_shm`); data buffers, not pixels | FlatBuffers/FlexBuffers messages + fd passing; dmabuf textures w/ **syncobj timeline explicit sync** (`dmatex.rs`) | Standard `wl_shm`/linux-dmabuf into smithay, then OpenXR swapchains |
| **Stereo & depth** | Double-wide buffer; depth-viewport hack (doc 08 §1.5); per-eye `motorcar_viewpoint` | N `zxr_view_v1` globals; typed **pixel/depth** buffers per view | N/A — server renders per view natively; no client depth | N/A — server renders per view natively | None: flat quads, no depth compositing between panels |
| **Input model** | **6DoF pointer** (position + 3×3 rotation, enter/leave/motion/button) | **None** (dropped; TODO) | **Ray** (`zwn_ray`: origin+direction, full `wl_pointer` event suite incl. axis) hit-tested against server-side regions | SUIS: input-method↔input-handler nodes, server-routed; fields for intersection | Overlay laser → synthetic 2D pointer/keyboard into embedded compositor |
| **2D-app integration** | 2D surfaces on quads in same space, input projected to 2D (thesis §6.1.2.2; pre-xdg-shell) | Implied same-compositor coexistence (wxrc renders 2D toplevels as quads); nothing in the XML | Zen supports Wayland 2D apps + planned Xwayland ([FAQ](https://www.zwin.dev/what_is_it/faq)); 2D and 3D protocols side by side | **Delegated to a client** (Flatland) via `FLAT_WAYLAND_DISPLAY`; no in-tree compositor (doc 05 §11 Q2) | **The whole product**: full xdg-shell embedded compositor + Xwayland |
| **Frame timing / pacing** | `view_matrix` "ideally at the beginning of each frame"; policy discussion only in thesis §7.3.2 | Explicit **TODO** ("better timing… map OpenXR more closely") | `zwn_virtual_object.frame(wl_callback)` — 2D-style frame callbacks; server-side loop absorbs the rest | Server-side: clients declare, server renders every XR frame; latency-insensitive clients | Overlay session paced by OpenXR runtime; embedded clients paced by frame callbacks |
| **Multi-client & security** | Depth test across clients; stencil clip to window bounds stops overdraw outside window (thesis §6.2.4); depth values themselves trusted | Same model; clipping/stencil unspecified in XML | Strong: server owns rendering, regions bound input; but GL command stream = large attack/complexity surface in compositor | Strong mediation (server arbitrates all nodes/input); own security model outside Wayland's | Panels isolated by construction (separate texture per toplevel); weakest *spatial* integration |
| **Wayland-native?** | Yes (wire protocol + `wl_surface` role) | Yes (role + `wl_buffer` reuse) | Yes (wire protocol, `wl_callback`, xdg-shell idioms) — but parallel object world incl. own shm | **No** — separate socket, serialization, object model | Yes and no: *implements* Wayland (smithay server), *speaks* no XR protocol |
| **Status / 2026 activity** | Dead (2015) | Dead (wxrc 2021); protocol spec co-authored by user, revivable | Dormant (zwin 2023-01, zen 2024-02; [roadmap stale](https://github.com/zwin-project/zwin.dev/issues/52)) | **Active** (2026-09-14); packaged in nixpkgs-xr | **Active** (2026-09-20); in nixpkgs |
| **Fit: Monado standalone-headset appliance session** | Right model, wrong decade: pre-OpenXR, QtWayland, packed depth | Best *protocol* skeleton: wlroots/OpenXR-era, N-view, typed depth — but unfinished (input, timing) | Compositor too heavy for appliance (GL-over-wire interpreter); 3D-app-first, 2D secondary | Runs today on Monado; but non-Wayland IPC + 2D story delegated to client + Bevy/fork supply chain (doc 05 §8) | Runs today; but flat quads only — no shared depth-composited space, no 3D apps; an overlay, not a shell |

## 4. Analysis

### 4.1 motorcar → zxr: what the 2019 rework improved and what it regressed

**Improved:**

1. **N-view generalization.** motorcar hardcodes the viewpoint-per-eye idea; zxr makes views an
   arbitrary set of `zxr_view_v1` globals, which cleanly covers mono preview windows, stereo HMDs,
   and future multi-view targets, and lets the compositor add/remove views at runtime.
2. **Typed depth buffers.** motorcar's packed depth-in-color viewport (doc 08 §1.5) becomes
   `buffer_type = pixel_buffer | depth_buffer` on `zxr_composite_buffer_v1` — the *protocol* no
   longer encodes the EGL workaround, so a modern implementation can attach a real depth image
   (e.g. a dmabuf of `Z32_FLOAT`/`D32`) without a wire change.
3. **`wl_buffer` transport reuse.** Buffers per view/type are plain `wl_buffer`s, so shm and
   linux-dmabuf both work in principle, and `get_wl_buffer` re-enters the standard
   `wl_surface.attach`/commit lifecycle — more idiomatic than motorcar's implicit double-wide
   layout convention.
4. **Role hygiene.** Attaching a 2D buffer to an XR surface is a protocol error; the 2D/3D split
   is enforceable.

**Regressed:**

1. **Input is gone.** motorcar's `motorcar_six_dof_pointer` was a complete, surface-local 6DoF
   pointer spec; zxr defines nothing. A shell can't be built on zxr as written.
2. **The folded MVP.** `zxr_surface_view_v1.mvp_matrix` collapses motorcar's model/view/projection
   split into one matrix per surface-view. That loses the windowing-system structure doc 08 §1.8
   identifies: with the split, the compositor owns view+projection (sent rarely or per-frame
   per *view*, shared by all surfaces) and the client owns its model transform; window moves don't
   perturb the client's projection math; clients can correctly transform normals, do lighting in
   world space, and cull. With the fold, every surface×view gets a fresh opaque matrix every time
   *anything* moves — more traffic, less meaning. (It also leaves no protocol slot for the
   compositor to *request* a size, dropping motorcar's `request_size_3d`/`set_size_3d`
   negotiation.)
3. **Timing left as a TODO** exactly where XR needs it most (doc 08 §1.6): no analog of
   `wl_surface.frame`, no predicted-display-time, no advance-frame pipelining — the protocol
   can't yet express either of the thesis §7.3.2 pacing modes.
4. **Clipping is gone too:** no cuboid/portal equivalent, so nothing stops a client's fragments
   from escaping its window except unspecified compositor behavior.

Net: zxr is the right *skeleton* (N views, typed buffers, `wl_buffer` reuse) that deleted the
*flesh* motorcar had (input, matrix structure, clipping, size negotiation).

### 4.2 zxr vs zwin: the philosophical fork

Both are Wayland wire protocols; they answer "what crosses the wire" oppositely.

- **zxr/motorcar (client-rendered 2D buffers + depth compositing):** the compositor coordinates
  *projections* and merges *pixels*. Geometry never crosses the wire. Clients can render with any
  API, any technique — the thesis explicitly includes ray tracing (doc 08 §1.4) — as long as they
  fill color+depth for each view. The compositor stays small: unpack, depth-test, clip.
- **zwin (server-side virtual objects + regions + ray input):** the compositor *is* the renderer.
  Clients describe scenes: GL buffers/shaders/techniques (`zwin-gles-v32.xml`), hit-test regions
  (`zwn_region.add_cuboid/add_sphere`), and receive high-level ray events rather than view
  matrices.

**Which is more Wayland-idiomatic?** In surface *idiom*, zwin — `zwn_bounded`'s
configure/ack_configure/serial dance, `set_title`, `move(seat, serial)`, `frame(wl_callback)`,
and `zwn_ray` mirroring `wl_pointer` event-for-event are straight out of xdg-shell/wl_seat. But in
*architecture*, zxr — Wayland's core contract is "clients render, the compositor composites
buffers" (every mainstream protocol from `wl_shm` to `linux-dmabuf` to `wp_presentation` assumes
it), and zwin abandons exactly that contract, to the point of needing its own shm. zwin is
Wayland-*flavored* remote rendering; zxr is Wayland *extended by one role*.

**Which is more powerful?** For compositor-side capability, zwin: per-view rendering with no
client round-trip (head-pose latency absorbed server-side — the same property that let Zen stream
to a Quest via `znr-remote`), true input mediation against known geometry, resize/LOD policy in
one place. For *client* capability, zxr: zwin's clients are capped at what the
`gl_base_technique` abstraction expresses — no compute, no custom pipelines, no non-GL renderers
— while zxr clients own their whole frame.

**Which is simpler to implement?** zxr, by a wide margin. A zxr compositor is a wlroots/smithay
compositor plus: N view globals, a composite-buffer tracker, one extra sampling pass with depth
test and stencil clip. A zwin compositor embeds a remote GLES 3.2 interpreter — resource
lifetime, shader compilation of untrusted sources, GL state isolation *per client* — which is
close to writing a GPU process. Zen's stall despite a working first release is consistent with
that maintenance weight.

**Which better fits "unmodified 2D apps + native 3D apps in one space"?** zxr's model. The 2D
path and the 3D path are the *same* path — a `wl_surface` with a buffer, composited; a 2D
toplevel is just an XR surface whose quad the compositor textures and whose depth is a plane,
exactly the thesis §6.1.2.2 embedding. Input likewise degrades gracefully: a 6DoF/ray event
projected onto the plane becomes an ordinary `wl_pointer` event, invisible to the 2D app. Under
zwin, 2D and 3D apps live in two different protocol worlds (`wl_shm`+xdg-shell vs
`zwn_shm`+zwin-shell) that only meet inside the compositor's renderer; Zen made it work, but
nothing is shared, and every 2D feature (popups, subsurfaces, Xwayland — all on Zen's roadmap
rather than free) must be re-plumbed into the 3D world by hand. One zwin idea is worth stealing
regardless: **ray input shaped exactly like `wl_pointer`** (enter/leave/motion/button/axis +
frame grouping) is the most portable input design of the five.

### 4.3 Where StardustXR and WayVR sit

They bracket the design space from §1.

**StardustXR is the maximal position** — it concludes that Wayland is the wrong substrate for a
spatial shell and builds a new display-server protocol: persistent scene-graph nodes, SDF fields,
server-routed input (SUIS), fd-passing, dmabuf-with-explicit-sync textures. What it buys:
latency-insensitive clients (the server re-renders your nodes at full XR framerate whether or not
you're awake — no per-client pacing problem at all), genuinely spatial semantics (zones, fields,
spatial parenting across clients), and it *exists and runs on Monado today*. What it costs, for
Mura specifically: (a) it re-derives the thesis §5.3.2 argument in reverse — doc 08 §1.2's
case for Wayland-native (input routed by the display server that owns the surfaces) is abandoned,
and with it interop with every existing Wayland tool, portal, and protocol; (b) the 2D story is
*delegated to a client* (Flatland via `FLAT_WAYLAND_DISPLAY`) with no in-tree compositor — doc 05
§11 Q2's open packaging hole — so "unmodified 2D apps first-class" depends on a second,
separately-maintained compositor project; (c) the supply chain is branch-pinned Bevy/wgpu forks
(doc 05 §8, §10). It is the strongest *running* 3D-app platform here, and the right thing to
*package* as an optional session (doc 05 §9.7) — but adopting its protocol means Mura's
shell is no longer a Wayland compositor, which contradicts the Part 1 philosophy the project has
already committed to.

**WayVR is the minimal position** — and its success is the strongest *evidence* in this document.
The most active, most packaged, most used project in the Linux XR shell space (nixpkgs, AUR,
Flatpak-adjacent, WiVRn/Envision integration) defines **zero protocol**: an embedded smithay
compositor, dmabuf import, quads via `XR_EXTX_overlay`. Lesson one: *the 2D-panels-in-XR problem
requires no new protocol at all* — an embedded compositor + OpenXR quad layers is sufficient, and
any Mura compositor gets that tier almost for free (smithay/wlroots plumbing that WayVR has
already demonstrated end-to-end, including Xwayland). Lesson two: the ceiling is hard — no
depth-composited shared space, no 3D clients, no cross-app occlusion, input limited to
laser-onto-plane. WayVR is an overlay accessory to someone else's session; Mura needs to
*be* the session. The gap between WayVR and zxr is precisely the set of things a protocol is for:
per-view matrices, client depth, 3D input focus.

### 4.4 Recommendation for Mura: continue the zxr lineage, refilled from motorcar

Given the constraints — Monado is the runtime (doc 05 §9.1), unmodified 2D Wayland apps are
first-class by philosophy (doc 08 §1.1), and the user co-authored the original zxr spec and
intends to continue it — the recommendation is:

**Back the Mura compositor with a revised zxr: a Wayland-native, client-renders/
compositor-composites protocol, implemented in a compositor that is itself an OpenXR client of
Monado.** The compositor exposes ordinary xdg-shell for 2D apps (WayVR-tier functionality,
smithay/wlroots-standard) and `zxr-shell-v2` for 3D apps; both composite into one
depth-tested space. Reject the zwin server-side-rendering fork (compositor weight, capped
clients, dead ecosystem) and the StardustXR substrate swap (non-Wayland, 2D story delegated),
while stealing specific pieces of each.

Concretely, a **2026 zxr revision (call it `zxr-shell-v2`)** should adopt:

- **From zxr as-is:** N `zxr_view` globals (with the missing `resolution`/fov events actually
  specified); typed per-view pixel/depth composite buffers; the XR-surface role with the
  2D-buffer protocol error.
- **From motorcar:** the **view/projection/model split** — per-view `view_matrix` (per frame) +
  `projection_matrix` (on change) events on the view global, per-surface `transform_matrix`
  (model) event, replacing the folded per-surface-view MVP; **cuboid/portal clipping modes** and
  3D **size negotiation** (`request_size_3d`/`set_size_3d` → configure/ack_configure, borrowing
  zwin-shell's serial idiom); **6DoF pointer input** (position + orientation enter/leave/motion/
  button, surface-local).
- **From zwin:** the **`wl_pointer`-shaped ray device** (`zwn_ray`'s event suite including
  axis/axis_source/frame) as a second seat capability beside the 6DoF pointer — rays are what
  controllers and hand-pinch actually produce, and the compositor hit-tests them; keep hit-test
  geometry *server-side and compositor-derived* (surface bounds/cuboids), not client-uploaded
  regions, unless a real need appears.
- **From StardustXR (mechanism, not model):** **explicit sync via
  [`wp_linux_drm_syncobj_v1`](https://wayland.app/protocols/linux-drm-syncobj-v1)** timeline
  points on both pixel and depth buffers (Stardust's `dmatex.rs` + `timeline_syncobj` proves the
  kernel path works in this exact stack, doc 05 §5) — this, plus **linux-dmabuf as the primary
  buffer transport**, is what finally retires the depth-viewport hack: the client exports its
  real depth attachment as a dmabuf, no Mesa patch (doc 08 §1.5 → to be confirmed against doc 09's
  patch-gap findings).
- **From modern wayland-protocols:** first-class **frame timing** — a per-surface XR frame event
  carrying predicted display time and view matrices for that prediction (mapping
  `xrWaitFrame`/`XrFrameState.predictedDisplayTime` semantics onto the wire, resolving zxr's
  "map OpenXR more closely" TODO), plus `wp_presentation`-style feedback so clients can measure;
  and normal **`xdg_toplevel` integration** — a 2D toplevel needs *no* XR interface to appear as
  a quad (compositor policy places it; thesis §6.1.2.2), with an optional request to query/set
  its 3D placement for XR-aware 2D apps.

`wp_drm_lease_v1` stays where it is: consumed by Monado on the desktop profile, bypassed by
`VK_KHR_display` on the appliance (doc 05 §11 Q1). It needs nothing from the new protocol.

Sequencing note for ADR 0006: the WayVR-tier (xdg-shell quads on Monado, no new protocol) is
shippable first and is independently useful; `zxr-shell-v2` layers the 3D-native tier on top of
the same compositor. StardustXR remains a packaged alternative session (doc 05 §9.7), not the
backbone.

### 4.5 Open questions for the revision

1. **Frame pacing across clients.** Thesis §7.3.2's two modes (draw-with-last-frame vs
   wait-for-clients) still need a protocol expression: is the mode per-client (application
   profile), and how does a slow zxr client degrade — reprojected stale color+depth (depth makes
   naive reprojection *harder*: which pose do you trust)? Does the compositor expose one XR frame
   clock or per-surface clocks?
2. **How 2D toplevels map into 3D.** Who assigns the initial transform of an xdg_toplevel quad —
   compositor policy only, or a negotiation? What do xdg popups, subsurfaces, and drag-and-drop
   mean spatially (Zen's roadmap shows each is real work)? Is there a curved-panel mode, and is
   that a surface property or pure compositor policy?
3. **Does geometry ever cross the wire?** zxr's glTF-buffer TODO vs the position taken here
   (buffers only). Candidate middle ground: a StardustXR-style *asset reference* (path/fd to a
   glTF) as a separate, optional protocol for latency-tolerant decorative content — or nothing.
   Related: do zwin-style client hit-test regions ever become necessary (e.g. for non-planar 2D
   surfaces), or does compositor-derived geometry suffice?
4. **Depth trust and clipping.** Depth compositing lets a client claim any depth; motorcar bounded
   the damage by stencil-clipping to window bounds (thesis §6.2.4). The revision must specify
   clipping normatively (cuboid/portal), and decide whether depth values outside the window volume
   are clamped or a protocol error.
5. **The depth-dmabuf path.** Confirm (doc 09) that current Mesa exports depth-renderable formats
   as dmabuf with syncobj timelines on the target GPUs — if not on some driver, the packed-depth
   fallback needs a protocol-visible negotiation (a `formats`-style event per view).

---

*Written 2026-09-22 for research pass B3; feeds ADR 0006 (compositor strategy) together with
[08-wxrc.md](08-wxrc.md) Part 2 and [09-wxrc-ecosystem-gap-2026.md](09-wxrc-ecosystem-gap-2026.md).*
