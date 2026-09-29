# specs/composition: how a window reaches the display — quads, containers, Monado's order, the controller seam

**Status:** rev 0 (2026-09-29). Normative for `pkgs/zxr` (the quad path, §2 — written from the
code as built and checked against it) and for the Mura Monado series (§3–§5 — the contract
Monado must meet; nothing built yet, the C-track in
[implementation-path.md §3](../docs/architecture/implementation-path.md)).
**Design sources:** [ADR 0006 amendment 4](../docs/architecture/adr/0006-compositor-strategy.md)
(the ruling), [research/79](../docs/research/79-openxr-extensions-and-zxr.md) (the container pair
against the lineage), [research/65](../docs/research/65-embedded-frame-path-efficiency.md) and
[research/67](../docs/research/67-overlay-efficiency-beside-native-apps.md) (the measured quad
and overlay paths), [zxr-core.md](zxr-core.md) §4/§5a/§7 (bands, arenas, the frame).
**Grounding:** "XDG" does not occur; every space is an OpenXR `XrSpace`; "layer" is OpenXR's
composition layer unless qualified ("layer-shell", "zxr band"); the OpenXR text is the pinned
`references/openxr-docs` (1.1.63), Monado the pinned `references/monado` (`b9883f2`).
**Budget impact** (overview invariant 9): this spec adds no pass of its own — it removes one.
With 3D content in Monado's containers, zxr runs no projection pass for it (a full-resolution
read of every client per eye per frame on a tiler, avoided); the quad path is unchanged from the
measured M1 shape (research/65 §2.4: −47 % zxr CPU, 0 GPU for static UI under head motion);
Monado's per-client squasher cost is measured (research/67 §2: +0.33 ms/frame at 16 quads on the
host GPU). §7 names the gates.

## 1. Ownership

1. **Monado composes and presents.** The specification: "Composition layers allow an
   application to offload the composition of the final image to a runtime-supplied compositor"
   (`rendering.adoc:1210`); layers "must: be drawn in the same order as they are specified in
   `XrFrameEndInfo`, with the 0th layer drawn first … with a 'painter's algorithm'"
   (`:1143-1147`); "if no layers are provided then the display must: be cleared" (`:1046`);
   `xrEndFrame` "may: return immediately" (`:1008`). The runtime is the only thing that touches
   the display.
2. **Every application — zxr included — submits layers into that compositor.** zxr submits quads
   (§2). A 3D application submits its layers into a *spatial container* (§3). Nothing composes
   on the application side; zxr's projection pass exists only for quad overflow (§2.6).
3. **The frame loop is core's.** `xrWaitFrame` → `xrBeginFrame` → render → `xrEndFrame`, one
   loop per session. Containers change what a session submits and how views are located, not
   the loop (`ext_spatial_container_self_rendering.adoc:70-75`).
4. **Policy is zxr's.** Where every window is, how large, which is interactable, which are
   visible: decided by zxr's places and window-management policy
   ([window-workspace-management.md](../docs/architecture/window-workspace-management.md)) for 2D
   windows *and* 3D containers alike, and pushed to Monado over the controller seam (§5).

## 2. Quads — how a 2D window reaches the display (as built, rev 0)

A mapped `xdg_toplevel` (or layer surface, popup, cursor) is one **`XrCompositionLayerQuad`**
(`rendering.adoc:1389-1430`): "useful for user interface elements or 2D content rendered into
the virtual world … a two-dimensional object positioned and oriented in 3D space"; the
position is the quad's centre, the orientation its front-face normal, the size in metres in the
space's x-y plane; only the front face is drawn (`:1414-1430`). zxr's construction
(`pkgs/zxr/src/xr.rs:195-207`, `:700-755`):

| field | value | source |
|---|---|---|
| `space` | the session's `LOCAL` reference space (`xr.rs:407`); the pose below is the member's world pose in that space, derived per tick from its place's frame (zxr-core §5) | places-model.md; `XrSpace` semantics |
| `pose` | `Member.local` composed with the frame's pose; billboarding, follow and grabs are policy edits to this pose (wm §4a) | zxr-core §5a |
| `size` | width × height in metres from the plane's angular scale at its spawn distance (wm §3) | research/64 verdict |
| `subImage` | one **panel swapchain per window**, grow-only (research/62 §8), `imageRect` = the top-left `image_extent` the window occupies; `imageArrayIndex` 0; `faceCount` 1 | `xr.rs:201-203,732` |
| `eyeVisibility` | `BOTH` | `xr.rs:731` |
| `layerFlags` | `BLEND_TEXTURE_SOURCE_ALPHA` — the panel image is premultiplied (the panel pass composites the surface tree premultiplied), so `UNPREMULTIPLIED_ALPHA_BIT` is **not** set (`rendering.adoc:1176-1195`) | `xr.rs:729` |
| `next` | `XrCompositionLayerColorScaleBiasKHR` when the runtime has `XR_KHR_composition_layer_color_scale_bias` and the member's touch-class hover emphasis > 0 (spatial-input §4): scale `s` on RGB, alpha 1, bias 0 | `xr.rs:717-739` |

2.1 **Order.** Quads are submitted band-ascending, nearest-last within a band (zxr-core §5a):
submission order *is* composition order (`rendering.adoc:1143-1147`). The cursor is the last
quad (research/70 §9: one 64×64 swapchain, one layer). When the overflow projection layer exists
(§2.6) it is layer 0.

2.2 **The panel pass — one draw per commit, not per frame.** A window's panel swapchain image is
re-rendered only when its surface tree commits: the client's `wl_buffer` (dmabuf imported with
modifiers, or shm uploaded) is sampled into the runtime-allocated swapchain image
(`render.rs`; zxr-core §6.2). Between commits the runtime re-samples the same image at the
display pose every frame — head motion costs zxr nothing (research/65 §2.4). This is the one
copy on the 2D path; **its removal is the dmabuf-import swapchain** (§2.5).

2.3 **Cap and overflow.** The runtime reports `graphicsProperties.maxLayerCount` (`xr.rs:383`;
Monado: `XRT_MAX_LAYERS` = 128 on Linux, research/65). Members past the cap, taken band 5 first
then 4, 3, 2, nearest-first (zxr-core §5a), are drawn by zxr's own projection pass into a
stereo `XrCompositionLayerProjection` submitted as layer 0 — **the only case in which zxr
composes anything** (§2.6).

2.4 **The quiet shape.** With a native application primary (native-openxr-apps §4) zxr submits
`xrEndFrame` with `layerCount == 0`: Monado discards the frame and retires the client's
delivered layers (`comp_multi_compositor.c:609-623`, MR !2769); the game stays on the
one-projection-layer fast path (`comp_renderer.c:1131`). A placeholder layer is forbidden
(research/67 §5).

2.5 **Zero-copy 2D (Monado work item, D8).** Monado's compositor creates swapchains from native
images internally (`ipc_arg_swapchain_from_native`, `ipc_protocol.h:406`) but exposes no OpenXR
way to import a dmabuf as a swapchain image. A Monado-private extension that does so makes the
panel pass disappear for single-buffer surfaces: the client's buffer *is* the quad's image.
**Exists only when** the measured per-commit panel cost on target hardware exceeds the budget
line (budgets.md §3); gate: research/61's client bench with the import path vs the panel pass.

2.6 **The projection pass is overflow-only.** It composes overflow planes (2.3) and nothing
else — never 3D content (ADR 0006 amd. 4), never environment or cutout (Monado-side layers,
ADR 0008). Predicate and shape in zxr-core §7. `--debug-panels projection` forces every plane
into it for the R0 comparison path.

2.7 **Where the quad's space comes from once containers exist.** While zxr is an
`XR_EXTX_overlay` session (§6) the space is `LOCAL`. Once zxr's windows live in containers
(§6), each quad's `space` is its container's space (`xrCreateSpatialContainerSpaceEXT`,
`ext_spatial_container.adoc:278-290`: origin at the bounds centre, +X right, +Y up, +Z front)
and its pose is container-local.

## 3. Containers — how a 3D application reaches the display (the contract Monado must meet)

The pair as published: `XR_EXT_spatial_container` (#811) and
`XR_EXT_spatial_container_self_rendering` (#814, `depends` on 811). Every rule below is the
specification's; Monado's implementation is conformant when it meets them and Godot's
`spatial_container` module (`references/godot/modules/openxr/extensions/spatial_container/`)
runs unmodified against it (§7).

3.1 **Session.** A session opts in by chaining `XrSessionCreateInfoSpatialContainersEXT`
(`ext_spatial_container.adoc:1000-1002`). It is then "effectively 'headless'" (`:1010`): it
stays `XR_SESSION_STATE_IDLE` while running and goes only to `LOSS_PENDING` (`:1104-1106`);
`VISIBLE`/`FOCUSED`/`READY`/`SYNCHRONIZED`/`STOPPING`/`EXITING` are never entered
(`:1020-1030`); `xrBeginSession`/`xrEndSession`/`xrRequestExitSession` return
`XR_ERROR_SPATIAL_CONTAINERS_ENABLED_EXT` (`:1036-1043`); `xrLocateViews` errors (`:1070-1072`);
`xrWaitFrame` always sets `shouldRender = false` (`:1075-1076`); `xrEndFrame` with core
`layerCount > 0` is a validation failure (`:1079-1082`); the session is "focused" when at
least one container is interactable (`:1033-1035`); `XR_MND_headless` must not be combined
(`:1097-1099`). A graphics binding is required only by self rendering (`:1090-1093`).

3.2 **Handles and state.** `xrCreateSpatialContainerEXT` with `graphicsPresentation` and
`suggestedBounds` (metres; `{0,0,0}` = no preference; the runtime "may: ignore" them,
`:181-187`); a new container is not visible, not interactable, `BOUNDED` (`:144-146`);
`XR_ERROR_LIMIT_REACHED` past `maxSpatialContainerCount` (`:155-157`). State =
`{visible, interactable, boundsMode}` (`:430-436`), each change an event
(`XrEventDataSpatialContainerVisibleChangedEXT` / `InteractableChangedEXT` /
`BoundsChangedEXT`, `:441-447`); no event for a no-op request; every runtime-initiated change is
an event (`:360-380`). `XrEventDataSpatialContainerClosedEXT` when the container can no longer
be shown (user close, `LOSS_PENDING`); after it every call but destroy returns
`XR_ERROR_SPATIAL_CONTAINER_CLOSED_EXT` (`:197-231`).

3.3 **Space.** `xrCreateSpatialContainerSpaceEXT`: origin at the bounds centre, +X/+Y/+Z =
right/top/front faces; the runtime "may: set the pose … to any arbitrary pose when that
container becomes visible" and may change it at any time (`:278-304`); non-world-locked
(`:295`); unlocatable while not visible or after destroy (`:306-313`).

3.4 **Visibility, interactability, bounds.** Visibility is requested
(`xrRequestSpatialContainerVisibleEXT`), asynchronous, deniable with
`XrEventDataSpatialContainerVisibleRequestDeniedEXT`; the runtime must not show a container
before the app's first request and may change visibility freely after it (a taskbar)
(`:470-489`); it should not delay first render past 0.5 s (`:557-562`). Interactability is
"fully controlled by runtimes" and implies visible (`:589-591`); if only one container may be
interactable, the losing container's event precedes the gaining one's (`:618-624`). Bounds are
the runtime's: the app may only request `BOUNDED` ↔ `IMMERSIVE` (`:876-879`), asynchronously
and deniably (`:690-705`); `IMMERSIVE` = infinite bounds and "the same as a full-screen and
immersive app created without spatial container usage" (`:643-662`) — **Mura's fullscreen-game
model** (native-openxr-apps.md). Bounds-change events coalesce; a resize may be one event or
many; the origin moves with an edge drag (`:763-791`).

3.5 **Capabilities.** `XrSystemSpatialContainerPropertiesEXT`: `maxSpatialContainerCount`,
`supportsBounded`, `supportsImmersive` — at least one true (`:936-958`); self rendering requires
`supportsImmersive` (`self_rendering.adoc:50-53`).
`xrEnumerateSupportedSpatialContainerGraphicsPresentationsEXT` is stable for the instance
(`ext_spatial_container.adoc:966-990`). Mura's Monado reports both bounded and immersive;
`maxSpatialContainerCount` is "limited by memory allocation" (`:944-945`), the displayed count a
runtime limit the spec leaves to "highly constrained devices" (`:951-956`) — a budgets.md line.

3.6 **Rendering lifetime.** `xrBeginSpatialContainerRenderingEXT` /
`xrEndSpatialContainerRenderingEXT` add/remove a container from the session's *active render
set* and "replace `xrBeginSession` and `xrEndSession`" (`self_rendering.adoc:64-66`); illegal
between `xrBeginFrame` and `xrEndFrame` (`:126-129,184-187`); `primaryViewConfigurationType` at
begin, which the runtime may decay (`:148-157`).

3.7 **Views.** `xrLocateSpatialContainerViewsEXT` replaces `xrLocateViews` for the set
(`:233-250`): views container-major; the runtime "may: decay the view configuration" but "must:
still return the original number of views … populating the extra views with duplicates"
(`:265-275`); it "should: adapt each `XrView::fov` … as small as possible while still containing
the entire bounds" and "may: return non-symmetrical FOVs" (`:278-283`); it should account for
container motion (`:307-308`); per container `XrSpatialContainerViewStateEXT` carries
`viewStateFlags`, the (possibly decayed) `viewConfigurationType`, `shouldSubmitLayers` and
`recommendedImageExtent` (`:379-392`); when `shouldSubmitLayers` is false the app must not read
the FOV or the extent and should retain (`:402-411`); the extent is a per-frame `imageRect`
hint, never a reason to recreate a swapchain (`:434-437`).

3.8 **Submission.** `XrSpatialContainerLayerFrameEndInfoEXT` chained to `XrFrameEndInfo`, one
`XrSpatialContainerLayerEXT` per container in the active render set — missing, duplicate or
non-rendering containers are errors and the whole frame renders nothing (`:450-511`). Per
container the app **submits** (layers), **clears** (`layerCount 0`, retain false — transparent
black) or **retains** (`retainPreviousSubmission`, the runtime reuses the layers and images "as
latched" and later swapchain writes must not show) (`:551-590`). Within a container, painter's
order (`:559-562`); a projection layer must chain
`XrSpatialContainerCompositionLayerViewConfigurationEXT` naming the configuration it was
rendered with (`:603-637`). Pixels outside the projected 2D bounds are discarded, pixels inside
them but outside the 3D volume are kept (`:673-677`); `XrSpatialContainerLayerVolumeClippingEXT`
is the app's hint about what it clipped (`:646-690`).

3.9 **What the pair leaves to the runtime — and therefore to Mura (§4, §5).** Composition order
across containers (`:476-486`); depth between containers (depth-based clipping is "future
extensions", `:690-691`); every policy input (visible, interactable, bounds, pose, decay,
recommended extent).

## 4. Monado's composition order and depth policy

4.1 **The mechanism that exists.** Monado's system compositor collects every visible, active
client's latched layers into one array per frame, sorts clients by `z_order`
(`comp_multi_system.c:216-221`, `:270-305`), finds the base session (`:305-315`), and the
renderer squashes all layers into the distortion pass unless exactly one projection layer takes
the fast path (`comp_renderer.c:1131-1144`). Per-client `visible`/`focused`/`z_order`/
`is_base_session` arrive through `xrt_multi_compositor_control` (`xrt_compositor.h:2380-2420`).
A container is one such slot — the C-track's per-container `comp_multi` entry.

4.2 **Order (normative).** Containers and zxr's quads are ordered by the controller's z-order
(§5); within a session, submission order (`rendering.adoc:1143-1147`); within a container,
painter's order (`self_rendering.adoc:559-562`). An immersive container is the base session
and is drawn first (`ext_spatial_container.adoc:647-650`; native-openxr-apps §1). Monado's
existing rule — the main session below, overlay sessions above by `sessionLayersPlacement`
(`extx_overlay.adoc:57-75`) — is the default order when no controller is present (§5.3).

4.3 **Depth (the policy — one rule, one open choice).** For a container's projection layer that
carries `XrCompositionLayerDepthInfoKHR` (`XR_KHR_composition_layer_depth`, which Monado
accepts and binds in its compute path, `comp_render_cs.c:250-261`, and never reads), Mura's
Monado **resolves visibility per pixel against every other depth-carrying layer in the same
frame** — motorcar's rule (research/08 §1.5), implemented in the runtime the spec designates.
Layers without depth compose in order (4.2) at their container's bounds. **Open (decider: the
owner):** whether a container *must* submit depth to be interleaved (opt-in, quad order
otherwise — the shipping platforms' behaviour) or whether Mura's runtime treats a depth-less
container as opaque at its front face for interleaving purposes. Not promised in either case:
cross-container per-pixel depth for applications that submit none.

4.4 **Retain, decay, recommended extent** are the runtime's (3.7–3.8) and get their policy inputs
from §5: the controller supplies a recommended-extent scale per container (angular size →
pixels, the same derivation as the quad's `size`, §2) and may request mono decay for containers
under an angular threshold (a `wm.*` key, research/73's mutability posture).

## 5. The controller seam

5.1 **What zxr pushes, per container:** visible (the controller's answer to a visibility request
and its own taskbar/tidy decisions), interactable (focus-on-commit, one or many), bounds
(metres) and pose in the base space (the place's frame → `LOCAL`), z-order (band order, zxr-core
§4), recommended-extent scale and decay hint (4.4), close. **What zxr receives:** container
created (with `suggestedBounds`, `graphicsPresentation`, the owning client's identity and app
info), visibility request, bounds-mode request, destroyed; per-frame nothing.

5.2 **Two candidate shapes (decider: the owner; ADR 0006 amd. 4 D5).** (i) `libmonado`
(`references/monado/src/xrt/targets/libmonado/monado.h`, 611 lines; `mnd_root_set_client_
primary/_focused/_io_blocks` today, `monado.c:348-403`) grown by per-container calls over the
same `ipc_call_system_*` channel — a C API zxr already links, polled at 1 Hz today
(research/67), event delivery by a new blocking call or fd; (ii) the `comp_multi` **listener
interface** upstream has sketched (`wallbraker/monado-collabora:jakob/comp/multi-interface`
"[WIP] c/multi: Add listener interface", 2023; MR !1354 "Bubble compositor events through the
multi" [external]) exposed to one privileged IPC client. Both are Monado-native; neither is an
OpenXR extension of the app-facing kind. Rejected: a DisplayXR-style `xr*Workspace*` extension
spoken by a second OpenXR session (non-standard; makes the WM a frame-loop client).

5.3 **Prerequisites (conformance items, ADR 0006 amd. 4 D6).** (a) Peer identity is
server-derived at accept — `SO_PEERCRED` on the IPC socket; `ipc_app_state.pid`
(`ipc_protocol.h:399`) becomes informational. (b) The controller role is a **lease**: one
holder; granted to a client whose peer identity is the session compositor's unit (the same
trust root as zxr's socketpair, zxr-core §9); revoked on disconnect. (c) **Default policy
without a controller**: Monado's existing primary/overlay rule (4.2) — containers of the main
session visible and interactable, overlays above; the runtime is usable with no shell. (d)
Every seam verb is authorised against the lease; unauthorised calls fail closed.

## 6. zxr's own windows as containers (D7 — the transition rule)

While Monado does not advertise `XR_EXT_spatial_container`, zxr is an `XR_EXTX_overlay` session
submitting quads (§2). **When it does**, zxr becomes a container-session client: its quads are
submitted through `XrSpatialContainerLayerFrameEndInfoEXT` into containers zxr itself owns, so a
Wayland window and an OpenXR app are the same kind of object to Monado, ordered by one rule
(4.2). **Open (decider: the owner):** one container per Wayland toplevel (each window an
independent Monado slot — natural for the taskbar/visibility model, N containers per frame) or
one container for the whole shell (one slot, zxr's band order inside it — fewer slots, no
per-window runtime visibility). The seam (§5) is identical either way; the difference is what a
"container" means for a 2D window.

## 7. Conformance

1. **Quad path unchanged.** zxr-core §12 gates 1–11 pass with no regression after any change
   to this spec's §2 (the R0/M1 measurements are the baseline).
2. **Fast path preserved.** With one container app primary and zxr quiet, Monado's
   `one_projection_layer_fast_path` is taken (research/67's bench, `comp_renderer.c:1131`).
3. **Godot unmodified.** Godot master's `spatial_container` module with
   `xr/openxr/extensions/spatial_container/enabled = true` (`doc/classes/ProjectSettings.xml:
   3675-3681`) creates, shows, renders and closes a bounded container on Mura's Monado. **The
   client is `pkgs/spatial-container-sample`** (`nix run .#spatial-container-sample`; inside the
   dev loop `nix run .#dev-session -- --godot`): a Godot 4.8 project derived from the
   `godot_openxr_vendors` spatial-container sample [external] (m4gr3d's, MIT) that emits one
   `SCS <event> …` line per container event — `ext present|absent`, `caps`, `visible`,
   `interactable`, `bounds`, `request_bounds_mode`/`…_denied`, `closed` — and submits
   `XR_KHR_composition_layer_depth` (its README §3 maps each line to the item here that reads
   it). Today, against Monado without the pair, it logs `SCS ext absent` and runs as an
   immersive session — the C0 baseline every later run is diffed against.
4. **zxr places it.** The same container's pose, bounds, visibility and interactability follow
   zxr's WM policy through the seam: spawn below the eye line, tidy, focus-on-commit, close from
   the window menu (window-workspace-management §3–§5 applied to a container proxy).
5. **Two containers interleave.** Two depth-submitting container apps whose volumes intersect
   show per-pixel occlusion (4.3), measured against a single-process ground truth (the M3
   method, implementation-path §3).
6. **Session-state conformance.** Every rule in 3.1 (IDLE-only; errors from begin/end/exit/
   `xrLocateViews`/core-layer `xrEndFrame`; `shouldRender = false`) holds against a probe client.
7. **Seam prerequisites.** 5.3 (a)–(d) demonstrated: a client with the wrong peer identity is
   refused the controller lease; the runtime composes containers correctly with no controller
   connected.
8. **Budget.** Monado's per-container squasher cost measured on target hardware with N = 1, 4,
   16 containers, beside research/67's host numbers; recorded in budgets.md §3.
