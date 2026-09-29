# OpenXR extensions and the zxr lineage — the registry at 1.1.63 read against Mura

**Date:** 2026-09-29. **Status:** research; §7a carries proposed deltas per draft document,
§9 the owner questions. Nothing in the protocols, specs or ADRs changes on this doc's authority.
**Sources studied** (pinned in `references/MANIFEST.json`): `openxr-docs` @ `5a82d45` =
"OpenXR Specification 1.1.63 (2026-09-01)" — `specification/registry/xr.xml` and
`specification/sources/chapters/extensions/*/*.adoc`; `monado` @ `b9883f2` (2026-09-19);
`godot` @ `941ea18` (master, newly pinned); `displayxr-runtime` @ `81b3458` and
`displayxr-extensions` @ `d94d851` (newly pinned); `openxrs`; `pkgs/zxr/src` (what zxr enables
today); `protocols/zxr-shell-v2.xml`, `zxr-window-management-v1.xml`, `zxr-workspace-v1.xml`.
**[external]** where named: the supplied catalogue
(`openxr_spatial_api_catalogue_2026-09-29.md`, an LLM-produced summary — every claim used here
was re-read at the pin), the Khronos SIGGRAPH 2026 BOF slides, GitHub/GitLab API listings
(Monado MRs, the CTS tree), Godot's project-settings docs.
**Grounding:** "XDG" does not occur in this doc; spatial terms are used in OpenXR's `XrSpace`
sense; Wayland protocol terms with their upstream meanings (ADR 0012 §4). **Budget impact:**
none of its own — §4's shapes each carry one.

## 0. Discipline, counts, baselines

- **The registry at the pin.** `xr.xml` carries 999 `<extension>` tags: **253** with
  `supported="openxr"`, 746 `supported="disabled"` (reserved numbers, no contract). By author
  tag among the 253: EXT 56, FB 54, META 35, KHR 32, ANDROID 28, BD 27, MSFT 17, ML 14, HTC 9,
  VARJO 8, OCULUS 3, SONY 2, QCOM 2, MNDX 2, MND 2, one each of YVR, VALVE, ULTRALEAP, OPPO,
  LOGITECH, HUAWEI, HTCX, EXTX, EPIC, ALMALENCE. The docs README's sweep rule ("whole
  directories, never named lists") is honoured in §10: **every one of the 253 has a row.**
- **Monado's baseline.** Its supported set is generated from
  `src/xrt/state_trackers/oxr/extension_support/oxr_extension_support.py`: 83 names, of which 73
  are registry-supported; the other ten are Monado-private (`XR_MNDX_system_buttons`,
  `XR_MND_query_egl_device`, seven controller profiles) or registry-disabled
  (`XR_MNDX_xdev_space`). Monado implements **none** of the `XR_EXT_spatial_*` family and
  **neither** container extension (the header is present under `src/external/openxr_includes`
  and nothing else references it).
- **zxr's baseline** (`pkgs/zxr/src/xr.rs:256-286`): requires `XR_KHR_vulkan_enable2`; enables
  when available `XR_KHR_locate_spaces`, `XR_EXT_user_presence`,
  `XR_KHR_composition_layer_color_scale_bias`, `XR_EXT_hand_interaction`,
  `XR_EXT_eye_gaze_interaction`, `XR_EXT_hand_tracking`, `XR_MNDX_system_buttons` (by name,
  `xr.rs:278-281`), and `XR_EXTX_overlay` under `--overlay` (`xr.rs:283-286`). It synthesises
  `XR_FB_hand_tracking_aim`'s system-gesture flags itself (`input/bridge.rs:2-4`).
- **What was verified against the supplied catalogue.** Correct at the pin: both container
  extensions are ratified (`xr.xml:24456`, `:24536`; numbers 811 and 814; 814 `depends=811`);
  the base extension adds eight commands including
  `xrEnumerateSupportedSpatialContainerGraphicsPresentationsEXT` (`xr.xml:24456-24535`); bounds
  modes are `BOUNDED`/`IMMERSIVE`; `XR_EXT_spatial_image_tracking` is new and ratified (#783).
  Not established by it and not by this doc: any open runtime implementing the container pair
  (§4c).

## 1. Corrections to the corpus (forced by the pin)

1. **[zxr-shell-v2-composition.md §6](../architecture/zxr-shell-v2-composition.md)** called
   `XR_EXT_spatial_container_self_rendering` "FABRICATED / UNVERIFIED — no such extension in
   the registry". At the pin it exists, is **ratified** (#814, `xr.xml:24536`, `depends=
   XR_EXT_spatial_container`), and its spec is 878 lines
   (`ext/ext_spatial_container_self_rendering.adoc`, last modified 2026-08-06). The bullet's
   *conclusion* survives and is now citable: containers give lifecycle and placement, and the
   runtime "may: precomposite each bounded spatial container's composition layers into a single
   stereo quad layer" (`self_rendering.adoc:484-486`); "Future extensions may: allow for
   clipping based the 3D volume bounds if depth information is submitted"
   (`self_rendering.adoc:690-691`) — i.e. **no cross-app per-pixel depth today**. The
   "DisplayXR `XR_DXR_spatial_workspace`" aside in the same bullet was right about its status
   (provisional, unregistered — `displayxr-runtime/docs/specs/extensions/XR_DXR_spatial_workspace.md:8`)
   but wrong about its shape: it is not a self-rendering-windows extension for apps, it is a
   *workspace-controller* extension for the shell (§4a). Corrected in this pass.
2. **`XR_EXTX_overlay` is still provisional** (`xr.xml`: `provisional="true"`; spec revision 1
   dated 2018-11-05, last modified 2021-01-13, `extx/extx_overlay.adoc:9,316`). research/59
   §7 says so (`59:476`); [native-openxr-apps.md §2](../architecture/native-openxr-apps.md)
   rules "an `XR_EXTX_overlay` session, always" without stating the dependency. The container
   session model is the ratified alternative to the same problem (§3 row 11, §7a). Corrected
   in this pass: the dependency is now stated where the ruling is.
3. **research/09 §2's "reserved in the OpenXR registry"** refers to `XR_MND_egl_enable`
   (`09:68`), not to anything zxr-shaped — ADR 0006's "the original was reserved in the
   OpenXR/registry process" (`0006:93`) should be read that way. No text change; recorded.

## 2. zxr as an OpenXR client today — what Monado offers that zxr does not take

| extension | Monado | zxr | gap / position |
|---|---|---|---|
| `XR_KHR_composition_layer_depth` | yes (`comp_render_cs.c:250-261` binds it) | no | research/65 §7: bound but never read by the CS path (`layer.comp`), so it buys nothing today; rule stands — submit depth only when the runtime reads it |
| `XR_KHR_visibility_mask` | yes | no | the hidden-area mesh for the projection pass; a GPU-budget item once the projection pass carries 3D content (M2) |
| `XR_EXT_view_configuration_depth_range` | no | no | recommended near/far; zxr's transient depth (research/65 §7) picks its own |
| `XR_EXT_local_floor` | yes (promoted to 1.1) | no | the places model's floor frame should ground on `LOCAL_FLOOR` rather than derive it |
| `XR_FB_display_refresh_rate` | yes | no | the refresh-rate setting zxr-core §14 records as a user setting — this is its runtime face |
| `XR_EXT_performance_settings` | yes | no | CPU/GPU level + notifications; budgets.md's thermal seam; the only cross-vendor thermal channel Monado has |
| `XR_EXT_thermal_query` | no | no | Monado lacks; budgets.md reads thermal from the device contract instead |
| `XR_EXT_frame_synthesis` | no | no | cross-vendor space warp (research/65's row); nothing to consume until Monado grows it |
| `XR_EXT_view_configuration_views_change` | no | no | `XrEventDataViewConfigurationViewsChangedEXT` → re-enumerate → regrow; zxr's grow-only swapchains (research/62 §8) already fit; handle the event when Monado sends it |
| `XR_KHR_generic_controller` | yes (`bindings.json:320-324`) | no | the rich fallback profile; zxr's action set binds the simulated devices' profiles today (research/70 stand-ins) |
| `XR_EXT_active_action_set_priority` | yes | no | needed only when a second action set exists |
| `XR_KHR_vulkan_swapchain_format_list` | yes | no | mutable-format swapchains; a candidate for the panel swapchains' sRGB/UNORM views |
| `XR_KHR_convert_timespec_time` | yes | no | `XrTime` ↔ `CLOCK_MONOTONIC`; `wp_presentation` feedback and the frame journal convert by hand today |
| `XR_EXT_debug_utils` | yes | no | object names + messenger; dev profile |
| `XR_FB_composition_layer_alpha_blend` | yes | no | explicit blend factors; zxr's premultiplied quads rely on the default — verify |
| `XR_MND_headless` | yes | no | no-graphics session; not for `--greeter` (it draws) |

None of these is a determination by itself; the ones that change a mechanism are in §7a.

## 3. The container pair against `zxr-shell-v2` — element by element

The spec's own framing: spatial containers "are analogous to the concept of 'windows' in
desktop 2D windowing systems" (`ext_spatial_container.adoc:45-46`); "like windows extended to
3D" (`:58`); the OpenXR 1.x app model "does not extend to multiple simultaneous experiences"
(`:54`); visible containers "are able to belong to different apps and runtimes manage their
size and placement on behalf of the user" (`:61-62`). That is zxr-shell-v2's problem statement
(`protocols/zxr-shell-v2.xml:9-17`) with the **runtime** in the compositor's chair. Lineage
first (motorcar's thesis, research/08 §1.5, research/59 §3), then the spec.

| # | zxr-shell-v2 (rev 2) | container pair (rev 1) | verdict |
|---|---|---|---|
| 1 | **Who composites.** The compositor composites every client's colour+depth into one depth-tested scene and submits one projection layer (`zxr-shell-v2.xml:9-17`; zxr-core §1). | The runtime composites containers "in any order they choose" and "may: precomposite each bounded spatial container's composition layers into a single stereo quad layer" (`self_rendering.adoc:476-486`); layers *within* a container are painter's order (`:559-562`); pixels outside the 2D projection of the bounds are discarded, pixels inside the 2D bounds but outside the 3D volume are kept (`:673-677`); volume clipping by depth is "future extensions" (`:690-691`). | **zxr superset** on cross-app occlusion (the lineage's reason to exist: research/08 §1.5). **Container superset** on independence — a container never needs another container's depth. Orthogonal in mechanism: containers are a *runtime* contract, zxr-shell-v2 a *compositor* one; both can hold (§4). |
| 2 | **Views.** N `zxr_view` globals with slowly-changing `resolution`/`fov` (`xml:223-247`); the authoritative per-frame view+projection arrives in the frame snapshot (`view_state`, `xml:720-733`); the client owns its model transform (`transform`, `:734-745`). | `xrLocateSpatialContainerViewsEXT` returns per-container views ordered container-major (`self_rendering.adoc:233-250`); the runtime "should: adapt each XrView::fov … as small as possible while still containing the entire bounds" and "may: return non-symmetrical FOVs" (`:278-283`); it "may: decay the view configuration" but "must: still return the original number of views … populating the extra views with duplicates" (`:265-275`); it "should: take the container's motion into account" (`:307-308`). | **Equivalent core** (compositor/runtime-supplied view+projection per frame, client-owned model). **Container superset** on *bounds-fitted, per-container, possibly-mono frusta* — a real render-cost saver for a small far window. zxr could return per-surface fitted matrices in `view_state` today without a protocol change (the projection is per frame already); the mono decay needs a rule (§7a). |
| 3 | **Bounds.** `configure(serial, half-extents)` → `ack_configure` (`xml:421-503`), borrowed from `xdg_surface`/zwin; the acknowledged bounds are the clipping bounds; `set_clipping_mode` cuboid/portal (`:412-419`). | `suggestedBounds` at create, "a runtime may: ignore" (`ext_spatial_container.adoc:181-187`); bounds change only by the runtime, reported by `XrEventDataSpatialContainerBoundsChangedEXT` (coalesced; a resize may be one event or many; origin moves with an edge drag, `:738-791`); "an app cannot: request changes to bounds other than changing between bounded and immersive" (`:876-879`); a request may be denied with a denied event (`:701-705`). | **Equivalent** in authority (compositor/runtime decides; client acknowledges). zxr's serial-acknowledge is the *stronger* contract (the client says which bounds it rendered against; containers infer it from `xrGetSpatialContainerBoundsEXT` timing). **Container superset**: an explicit *denied* event and a bounds-**mode** (§ row 5). |
| 4 | **Submission and retention.** Per-view colour+depth in a `zxr_frame_slot` submitted against a named frame id; late = discarded + placeholder; "the compositor never re-presents previously submitted images for a newer frame" (`xml:448-479`); `clear` unmaps (`:480-487`). | Per container per frame the app *submits*, *clears* (`layerCount 0`, retain false) or *retains* (`retainPreviousSubmission`, the runtime reuses the layers and images "as latched" and later swapchain writes must not show, `self_rendering.adoc:551-590`); `shouldSubmitLayers == false` → the app must not read fov/extent and should retain (`:402-411`). | **Container superset**: explicit per-frame retain gives independent update rates with defined semantics. The lineage's reason for *never re-presenting* is exactness — a 3D client rendered with this frame's matrices; a retained image under a moved head is wrong without reprojection, which per-pixel depth makes possible (research/65's depth-reprojection row). Candidate delta, discretionary (§7a-1). |
| 5 | **Modes.** No mode; a 3D surface is a volume in the scene; exclusive scenes are the WM seam's `exclusive_requested`/`grant_exclusive` (`zxr-window-management-v1.xml:405,549`) and native-openxr-apps.md's fullscreen-game model. | `BOUNDED` / `IMMERSIVE`; immersive = "the same as a full-screen and immersive app created without spatial container usage", infinite bounds, other containers may be hidden (`ext_spatial_container.adoc:643-662`); the note: every 1.x runtime "can: implement the OpenXR 1.x path … by implicitly creating a spatial container … immersive" (`:667-670`); requests are async, "many seconds", may pop a consent UI (`:690-694`). | **Equivalent** in intent to Mura's one-experience rule (native-openxr-apps §8): an immersive container *is* the fullscreen game. Mura already has the verbs; the spec confirms the shape. |
| 6 | **Visibility / focus.** `xdg_toplevel`/WM-seam states (`hide`/`show`, `focus(window, serial)` through the activation rule, `zxr-window-management-v1.xml:486-548`); interaction serials from every commit. | `xrRequestSpatialContainerVisibleEXT` async; "runtimes must: not show a spatial container prior to the app requesting" once, then may change it freely (taskbar) (`ext_spatial_container.adoc:482-489`); may deny with a denied event (`:476-480`); "should: not delay rendering … over 0.5 seconds" after visible (`:557-562`). **Interactable** "is fully controlled by runtimes", "similar to 'focus' … with the difference that multiple spatial containers may: be interactable at once" (`:430-434`, `:589-591`). | **Equivalent** to the Wayland/WM model (apps never place or focus themselves — research/64's verdict, the spec's `:589-590`). **Container superset**: the denied event; the first-show rule. zxr's seam already has the refusal path (urgency-only). |
| 7 | **Space.** The surface's model space; places are `XrSpace`-grounded frames (places-model.md). | A container space per container: origin at bounds centre, +X right, +Y up, +Z front (`:278-290`); "a non-world locked origin" the runtime may re-pose at any time and on mode change (`:295-304`); unlocatable while not visible (`:309-313`, Issue `:1279-1288`); cross-process poses go through `XR_EXT_spatial_anchor` (Issue `:1268-1277`). | **Equivalent** (centre origin; zxr's `configure` half-extents are from the surface origin — the same convention). One check: zxr's origin is the surface origin, containers' the *bounds* centre — identical while bounds are symmetric about the origin, which `configure` guarantees. |
| 8 | **Timing.** `zxr_frame_timing.frame` (display time, period, cutoff) + `retired`; `wp_presentation`-style feedback (`xml:702-795`). | Core `xrWaitFrame`/`xrBeginFrame`/`xrEndFrame` unchanged (`self_rendering.adoc:70-75`); per-container `shouldSubmitLayers` and `recommendedImageExtent` may change every frame; "should: not recreate a swapchain solely because recommendedImageExtent changed" (`:434-437`). | **Equivalent** loop. **Container superset**: the per-frame *should-submit* and *recommended extent* hints. zxr's `resolution` event is slowly-changing by design; a per-frame extent would live in `view_state` (§7a-2). |
| 9 | **Input.** `zxr_ray` (wl_pointer-shaped) and `zxr_pointer_6dof` per seat (`xml:796-990`); the compositor hit-tests its own scene (research/68). | Nothing per container: `xrSyncActions` "would apply to all spatial containers in the session"; per-container filtering "would require an additional extension"; the WG expects an *event-channel* and *present-request* extension instead (Issues `:1237-1266`). | **zxr superset** today. The spec's direction (event-based input, decoupled from the frame loop) is the Wayland model. |
| 10 | **Capabilities.** `depth_format`/`transport`/`view` batches on bind (`xml:52-97`). | `XrSystemSpatialContainerPropertiesEXT` (`maxSpatialContainerCount`, `supportsBounded`, `supportsImmersive`); "highly constrained devices may: support only one spatial container being displayed at a time" (`:936-958`); graphics-presentation enumeration, stable for the instance (`:966-990`). | **Equivalent.** The "one displayed container" clause is the spec's own embedded-budget hook — the same posture as budgets.md. |
| 11 | **Session model.** zxr is one always-present `XR_EXTX_overlay` session; a native app is Monado's main session (native-openxr-apps §1–§2). | A container session "is effectively 'headless'" and stays `IDLE` for its life (`:1006-1012`, `:114-122`); never `VISIBLE`/`FOCUSED`; `xrBeginSession`/`xrEndSession`/`xrRequestExitSession` error; `xrLocateViews` errors; `xrWaitFrame.shouldRender` always false; plain-layer `xrEndFrame` errors (`:1020-1093`); `XR_MND_headless` must not be combined (`:1096-1099`). | **Orthogonal, and the alternative.** Under containers the "overlay vs main" question dissolves: the shell and the game are both container clients, the runtime orders them. `EXTX_overlay` is provisional (§1.2); containers are ratified. Which one Mura's *shell* speaks is the §9 Q1 fork. |

**Reading of the table.** zxr-shell-v2 is a strict superset on the thing the lineage was built
for — per-pixel cross-client occlusion with geometry never crossing the wire — and a subset on
three runtime-side conveniences that cost nothing to adopt in a compositor: bounds-fitted
per-surface frusta with mono decay, an explicit retain/clear per frame, and per-frame
should-submit/recommended-extent hints. Everything else is the same design arrived at from the
other side.

## 4. Who could implement containers on Mura — the fork, from comparables

Three shapes, each a comparable's actual position. **Decider: owner (§9 Q1).**

- **(A) The runtime implements the pair and delegates policy to a system shell.** The
  Android XR / Horizon OS shape [external — closed]; the *open* instance of the architecture is
  DisplayXR (§4a). Monado does not implement it (§4b). Mura's zxr becomes the controller
  (policy + its own Wayland clients as content) and Monado the multi-app compositor. **Budget
  impact:** the game's and every 3D app's layers stay in Monado's compositor (no extra hop);
  zxr's own scene stays one projection layer; the WM seam becomes IPC to Monado (one RPC per
  policy change, not per frame); the cost line is Monado's squasher, already measured
  (research/67 §2: any second client's layer leaves the fast path; +0.33 ms/frame at 16 quads).
  Gate: the same bench with N container clients.
- **(B) An implicit OpenXR API layer translates the pair into `zxr-shell-v2`/`xdg-shell`.**
  The OpenComposite/xrizer shape (translation in the app's process; both pinned). The layer
  implements the container entry points, allocates the app's swapchains as dmabuf-exportable
  images, and is a Wayland client of zxr. **Budget impact:** one extra process-boundary hop
  per frame for container apps (app → zxr → Monado instead of app → Monado), zero-copy if the
  swapchain images are dmabufs; the cross-app depth interleave is preserved. Gate: research/61's
  R0 client bench with the layer as the client. Needs OpenXR-SDK-Source pinned (loader/layer
  mechanics) — not done, by the plan's rule.
- **(C) `zxr-shell-v2` only; the container pair unimplemented on Mura.** The lineage today.
  Engines that speak containers (Godot master, §4c) get the 1.x path — one immersive session —
  which is exactly native-openxr-apps.md's model. **Budget impact:** none. Cost: no bounded
  multi-app from container-native engines until (A) or (B) exists.

These are not exclusive: (A) and (B) both keep zxr-shell-v2 for Wayland-native 3D clients; the
question is where *OpenXR-native* bounded apps land.

### 4a. DisplayXR's workspace controller, read from source

`displayxr-runtime` is a Monado fork (`NOTICE:4-7`; `README.md:16`: "Built on Monado …
strips away headset-centric infrastructure"; the `src/xrt/{state_trackers/oxr,ipc,compositor}`
layout is Monado's). It targets tracked 3D displays, not headsets; the multi-app path ships on
a D3D11 service compositor (`docs/roadmap/workspace-runtime-contract.md:2-9`), Linux is a
Vulkan/Wayland preview; the reference shell `displayxr-shell-pvt` is closed. What is open is
the **seam**, and it is the only open one of its kind:

- **The contract.** `XR_DXR_spatial_workspace` (spec v23, provisional, DXR tag registered
  July 2026 — `docs/specs/extensions/XR_DXR_spatial_workspace.md:3-11`): "the runtime owns
  mechanism (atlas composition, IPC, hit-test geometry, swapchain plumbing) and the workspace
  controller owns policy (which apps exist, where their windows go, how they animate, what
  chrome looks like, what the cursor does)" (`:17`). A privileged session calls
  `xrActivateSpatialWorkspaceDXR` — "at most one per system" (`:27`) — which requires an
  **IPC-mode** session (`oxr_workspace.c:287-290`) and is authorised server-side by
  orchestrator-PID match / verified controller class (`ipc_server_handler.c:4055-4080`). The
  surface: enumerate clients + per-client info (`spec:81-82`), window pose/size in metres +
  visibility (`:38-40`), focus (`:54-55`), input-event drain (POINTER/KEY/SCROLL/MOTION/
  FRAME_TICK/FOCUS_CHANGED/WINDOW_POSE_CHANGED, `:59-67`), pointer capture (`:67-68`),
  per-client frame-rate cap (`ipc_server_handler.c:4732`), request exit / fullscreen
  (`spec:76-77`), controller-drawn chrome as ordinary swapchains with hit regions (`:84-95`),
  an event-driven wakeup handle (`:104-106`), a per-client style struct (`:108-117`).
  `oxr_workspace.c` is 1 619 lines; the server handlers `ipc_server_handler.c:4055-5350`.
- **What they learned, in their words.** Hit-testing *moved to the controller* in v22: "the
  workspace controller now owns hit-testing end to end (it already ran the same eye→cursor
  raycast for click policy) and the runtime no longer raycasts" (`spec:45`); the runtime keeps
  only the cursor sprite's depth (`:49-50`). Every keyboard shortcut moved to the controller by
  v23 (`:60`, `ADR-014:25-39`). ADR-018 had argued the opposite (raycast as plumbing at the
  data) and named the drag/resize/rotation state machines as "the real layering violation"
  (`ADR-018:43-62`); the code followed ADR-018's second half and then overtook its first. This
  is research/68's "the seat is the compositor's" arrived at by a runtime team the hard way.
- **What broke.** ADR-035 (`:18-48`) is the audit of a Monado fork after it grew a multi-app
  shell: "identity and authorization do not exist … 111 of 131 handlers are unauthenticated;
  the gates that exist fail *open* without an orchestrator"; "the compositor has two modes
  selected by one process-global bool"; "shared display state has ~10 writers and no owner";
  "capacity is 8 connections"; "the IPC core is stock Monado: blocking pipes with no timeouts,
  one global mutex held across pipe I/O"; "input arbitration is per-provider, not per-consumer
  … there is no input focus". Their decisions: server-derived peer identity and verified client
  *classes* (`ADR-035:81-95`), **leases held by the service** with a built-in default policy
  when no controller is present (`:97-113`), one always-on compositor pipeline (`:115`).
  Option 2 — "controller-owned arbitration — the shell decides … the service obeys" — was
  rejected as *primary* because it "fails the moment there is no shell" (`:55-59`).
- **Transfer to Mura.** Everything above the display processor transfers: the privileged
  session as the WM seam; per-client pose/visibility/focus/frame-rate cap; the input drain
  (Mura's answer is the Wayland seat + `xrSyncActions`, but the *shape* — runtime delivers,
  controller decides — is the same); chrome as swapchains (Mura's chrome is Wayland surfaces).
  What does not: Kooima projection, weave, HWND capture/puppeting (`ADR-013`), the 2D-window
  capture path. The lessons transfer in full: identity from `SO_PEERCRED` at accept (Monado's
  IPC has the socket; research/68 already wants it for the trusted socketpair), leases with a
  default policy when zxr is absent (Monado's existing primary/overlay rule *is* that default),
  no second compositor mode.

### 4b. Implementing `XR_EXT_spatial_container(_self_rendering)` on Monado — the gap, file by file

What Monado already has:

- **Multi-client compositing with per-client state.** `comp_multi_system.c:270-300` collects
  each visible, active client's latched layers into one array, sorts by `z_order`
  (`:216-221`), finds the base session (`:305-315`); per-client `visible`/`focused`/`z_order`/
  `is_base_session` are set through `xrt_multi_compositor_control` (`xrt_compositor.h:2380-2420`:
  `set_state`, `set_z_order`, `set_main_app_visibility`) and reach the client as
  `XRT_SESSION_EVENT_STATE_CHANGE` / `_OVERLAY_CHANGE` (`xrt_session.h:44-47`;
  `comp_multi_system.c:642-710`). The layer squasher that draws N layers into the distortion
  pass (`comp_renderer.c:1131-1144`, fast path when exactly one projection layer) is the
  natural implementation of "precomposite each bounded container" — per frame, across clients.
- **A control channel for a shell.** `libmonado` (`monado.h`, 611 lines): client list, name,
  state, `set_client_primary`/`set_client_focused`/`toggle_client_io_active`/
  `set_client_io_blocks` (`monado.c:348-403`), all over `ipc_call_system_*`; zxr polls it at
  1 Hz today (research/67). `ipc_app_state` (`ipc_protocol.h:387-401`) carries
  `primary_application`, `session_visible/focused/overlay`, `io_blocks`, `z_order`, `pid`.
  `IPC_MAX_CLIENTS` is 32 (`ipc_protocol.h:41`) — four times DisplayXR's wire cap.
- **The session state machine** in one place (`oxr_session.c:365` `oxr_session_change_state`;
  `oxr_session_begin:414`, `_end:519`, `_request_exit:595`, `_locate_views:827`,
  `_frame_wait:1077`, `_frame_begin:1166`; `oxr_session_frame_end.c:1764` with per-type layer
  verification `:437-1005`) and the overlay create path (`oxr_session.c:1577-1582`).

What is missing, and where it would go (a proposal for a spike, not code):

| piece | spec requirement | Monado location | size yardstick |
|---|---|---|---|
| `XrSpatialContainerEXT` handle, state, events | create/destroy/space/state/bounds; six events; `XR_ERROR_SPATIAL_CONTAINER_*` | new `oxr_spatial_container.c` + `oxr_objects.h` struct; events via `oxr_event.c` (455 lines today) | DisplayXR's `oxr_workspace.c` 1 619 lines for 29 entry points → ~600–900 for 11 |
| session-state rewrite | `IDLE` for life; begin/end/exit/`xrLocateViews`/plain `xrEndFrame` → errors; `shouldRender=false` (`ext_spatial_container.adoc:1020-1093`) | `oxr_session.c` guards keyed on a `containers_enabled` flag from `XrSessionCreateInfoSpatialContainersEXT` (`:1000-1002`); `oxr_session_frame_end.c:1764` branch | small; the risk is the `xrt_session_event` mapping (`STATE_CHANGE` must not surface as VISIBLE/FOCUSED) |
| per-container `xrt_compositor` slot | each container = one entry in `comp_multi`'s client array with its own latched layers, `visible`, `interactable`, `z_order`, bounds, pose | `comp_multi_compositor.c` (per-client) gains a container list, or a container *is* a `multi_compositor` child; `ipc_client_state` per container | the largest piece; DisplayXR keeps one slot per client (`ipc_server_handler.c:4364-4487` pose/visibility by slot) |
| `xrLocateSpatialContainerViewsEXT` | bounds-fitted asymmetric FOV per container per view, decay, `shouldSubmitLayers`, `recommendedImageExtent` (`self_rendering.adoc:233-311`, `:386-437`) | new: fit a frustum to an oriented box from the head pose (a few dozen lines of math) + a policy input (extent scale) from the controller | new math; no Monado precedent |
| `xrEndFrame` grouping | `XrSpatialContainerLayerFrameEndInfoEXT` → per-container layer arrays; retain/clear (`:450-590`) | `oxr_session_frame_end.c` loop keyed by container; `comp_multi`'s retire/latch already distinguishes "delivered" from "retained" (`comp_multi_system.c:274-300`, research/67's !2769) | moderate |
| graphics-presentation enumeration | `xrEnumerateSupportedSpatialContainerGraphicsPresentationsEXT` stable per instance | `oxr_api_system.c` | trivial |
| **the policy seam** | who sets visible / interactable / bounds / pose / z-order / recommended extent | two comparable shapes: **(i)** DisplayXR's — a privileged *OpenXR session* with an extension (`xrActivateSpatialWorkspaceDXR`), authorised by peer identity; **(ii)** `libmonado`'s — a C API over the same IPC, extended from today's `set_client_primary` to per-container verbs | (i) ~30 entry points in DisplayXR; (ii) ~10 new `ipc_call_system_*` |

**Upstream posture.** The spec's contributor list names Jakob Bornecrantz (NVIDIA, Monado's
author) and Rylie Pavlik (Collabora, Monado's maintainer) (`ext_spatial_container.adoc:24,36`;
`self_rendering.adoc:22,31`); no MR, issue or branch exists on `gitlab.freedesktop.org/monado`
for it as of today [external: GitLab API]. A Mura implementation would be first, and shaped for
upstream only if the policy seam is Monado-native (`libmonado`, shape ii) or an extension Monado
would accept; DisplayXR's DXR seam is theirs.

### 4c. Reference-implementation status

- **Runtime side: none open.** Implementers named by the spec's authorship (Google, Meta,
  ByteDance, Qualcomm, Varjo, Microsoft) ship closed runtimes; the SIGGRAPH 2026 BOF says
  "ratified with multiple runtime implementers and engine support" [external]. Monado, WiVRn
  (`6f9e146`) and DisplayXR do not implement the pair.
- **Conformance: none yet.** OpenXR-CTS `main` and `devel` test the spatial-entity family
  (`test_XR_EXT_spatial_{anchor,marker_tracking,persistence,persistence_operations,plane_tracking}.cpp`)
  and not the containers [external: GitHub tree]; the spec promises "a separate test
  extension runtimes implement for CTS only, allowing CTS to control all the system policy"
  (`ext_spatial_container.adoc:1225-1229`) — not in the registry at the pin.
- **Client side: Godot master**, `modules/openxr/extensions/spatial_container/` — 902 lines
  across three units: `openxr_spatial_container_extension.cpp` (events → signals,
  `:191-210`; bounds-mode requests gated on `supportsImmersive`, `:346-362`; play space vs
  container space by mode, `:380`; project settings `xr/openxr/extensions/spatial_container/
  {enabled,bounds,bounds_mode}`, `doc/classes/ProjectSettings.xml:3675-3681`),
  `openxr_spatial_container_self_rendering_extension.cpp` (`xrBeginSpatialContainerRenderingEXT`
  `:186`; per-frame `xrLocateSpatialContainerViewsEXT` → `shouldSubmitLayers` `:314-319`;
  `retainPreviousSubmission` cleared per frame `:236`), `openxr_spatial_container_state.cpp`.
  This is what a Mura runtime implementation would be tested against first — before the CTS.
- **Architecture side: DisplayXR** (§4a) — the seam, the ADRs, the audit.

## 5. The spatial-entity family since research/21

research/21 covers `XR_EXT_spatial_entity` / `_plane_tracking` / `_marker_tracking` /
`_anchor` / `_persistence` / `_persistence_operations` and the Monado gap (§7 there). New at
the pin: **`XR_EXT_spatial_image_tracking`** (#783, ratified; reference-image databases reused
across contexts) and **`XR_EXT_stationary_reference_space`** (#743, published not ratified):
a reference space whose origin is best-effort continuous across tracking loss, relaunch and
restart, with `xrGetStationaryReferenceSpaceGenerationIdEXT` telling the app when continuity
broke. That is the OpenXR face of spatial-mapping.md §3's *local* frame: the map frame stays
Mura's (anchors, persistence), the stationary space is what an app sees. The `ANDROID_*`
adjuncts on the EXT spine are mechanism evidence, not targets; one of them,
`XR_ANDROID_spatial_anchor_space` (anchors as `XrSpace` handles), is the bridge the places
model's frame graph wants and the EXT family lacks. The `BD_*` and `FB_*`/`META_*` spatial
families are separate frameworks (§10 marks them superseded for Mura's purposes). Monado's gap
is unchanged: it has the deprecated `XR_EXT_plane_detection` and nothing on the EXT spine.

## 6. Interaction, haptics, models, sundry

- **`XR_EXT_render_model` + `XR_EXT_interaction_render_model`** (ratified; Monado lacks):
  runtime-supplied glTF controller/hand models with animated state, bound to top-level user
  paths. zxr draws no controller today; research/63/68 leave "who renders the controller" open.
  Under either seam the answer is the runtime provides the asset and the shell draws it —
  these two extensions are that contract. Monado work.
- **`XR_KHR_generic_controller`** (Monado has): the rich fallback profile; a binding row for
  zxr's action set.
- **`XR_EXT_hand_interaction`**: Monado has the profile; zxr enables it and bridges from joints
  because Monado has no device for it (`input/bridge.rs:1-4`). `XR_FB_hand_tracking_aim`
  remains the system-gesture flags zxr synthesises; research/68's ask of Monado stands.
- **`XR_EXT_haptic_parametric`** (published): device-agnostic parametric haptics, streaming;
  app-facing; Monado lacks.
- **`XR_EXT_interaction_profile_battery_state_display`** (ratified): display-only battery for
  system UI; the spec's own warning is not to drive behaviour from it. The consumer is
  `mura-panel`'s tray, via `libmonado`'s existing `mnd_root_get_device_battery_status` today
  and this extension when Monado grows it.
- **`XR_EXT_view_configuration_views_change`** (published): the event zxr must handle (§2).
- **`XR_EXT_frame_synthesis`** (published; deprecates `FB_space_warp`): research/65's row;
  Monado lacks.
- **`XR_EXTX_overlay`**: provisional since 2018 (§1.2). Its `sessionLayersPlacement` and
  `XrEventDataMainSessionVisibilityChangedEXTX` (`extx_overlay.adoc:57-75, :93-100`) are what
  native-openxr-apps.md builds on; the container session model replaces the same function
  with ratified text (§3 row 11).

## 7. Agentic tooling and `XR_EXT_conformance_automation`

- `XR_METAX1_agentic_external_tool` is not in the registry at the pin (experimental vendor
  preview; [external]). The Khronos BOF's `agentic_ai` API layer is "experimental", with a
  standard schema and a reference implementation as *goals* [external]. DisplayXR ships
  `XR_DXR_mcp_tools` and `displayxr-mcp` (a JSON-RPC 2.0 framework; the user installs an
  opt-in flag) — a shipped, open instance of the same idea.
- `XR_EXT_conformance_automation` (ratified) injects synthetic input for the CTS; zxr has a
  test-only injector on the compositor side (`control.rs`, research/70). Different layers,
  same function. Not a production agent API.
- **Ethos note** (overview invariant 10): an agent driving the headset is a client the wearer
  installs and authorises — DisplayXR's opt-in flag is the right posture, and Mura's is polkit
  per action. Nothing here is a Mura dependency.

## 7a. Proposed deltas per draft document

Every spec and design doc is a draft under development. Deltas are **forced** (a stale fact,
or a semantic every consumer of the cited spec must honour) or **discretionary** (a judgment
call, rule 4). Forced deltas are applied in this pass; discretionary ones wait for §9.

### `protocols/zxr-shell-v2.xml` (rev 3 candidates — all discretionary)

1. **Retain per frame.** Today a surface that misses a frame gets a placeholder and "the
   compositor never re-presents previously submitted images" (`xml:472-479`). Proposal: a
   per-surface `retain` request (latched, like `set_clipping_mode`) meaning "compose my last
   submitted slot with its recorded transform until I submit again"; the compositor reprojects
   using the slot's depth (the lineage has depth per pixel — the container runtime does not and
   still retains). Semantics from `self_rendering.adoc:551-556`: later writes to a retained
   slot must not show, so a retained slot stays *held* (not released) until replaced. Reason to
   keep the current rule: motorcar's exactness argument (research/08 §1.5). Reason to change:
   independent update rates are the spec's stated purpose and research/65's biggest lever for
   static 3D content. **Decider: owner (§9 Q2).**
2. **Per-frame render hints in `view_state`.** Add `should_submit` (bool) and
   `recommended_extent` (w, h) per (view, surface) — or a per-surface `render_hint` event in
   the frame snapshot — carrying the container's `shouldSubmitLayers` / `recommendedImageExtent`
   (`self_rendering.adoc:386-437`), with the same rule: do not reallocate on a changed extent;
   render into a sub-rect. zxr's `zxr_view.resolution` stays the allocation size. Reason: the
   compositor knows a surface's projected size; the client cannot. **Discretionary** (it is an
   optimisation with no correctness effect).
3. **Bounds-fitted per-surface frusta and mono decay.** `view_state` is already per view per
   frame; proposal: the compositor may send a *fitted* projection (asymmetric, minimal around
   the acknowledged bounds, `self_rendering.adoc:278-283`) and may send the same view matrix
   for both views (decay) with a `decayed` flag, keeping the view count constant
   (`:265-275`). Reason: render cost for small/far volumes. Reason against: the composite is
   per pixel with depth — a mono client image under stereo composition is only right at the
   plane. **Decider: owner (§9 Q2).**
4. **A denied event beside `configure`.** Containers deny visibility and mode requests with
   events (`ext_spatial_container.adoc:476-480`, `:701-705`); zxr-shell-v2 has no client
   request to deny (bounds are the compositor's) — nothing to add here; the WM seam has it
   (`limits` + refusal). **No delta.** Recorded so the comparison is complete.

### `specs/zxr-core.md`

- **§1 / §7 (forced):** state the OpenXR extension set zxr enables and why, as a table
  (§0 baseline), and that `XR_EXTX_overlay` is provisional at the pin with the ratified
  alternative named (§3 row 11). Applied.
- **§7 (forced):** handle `XrEventDataViewConfigurationViewsChangedEXT` when the runtime
  offers `XR_EXT_view_configuration_views_change` — re-enumerate views, regrow swapchains
  (grow-only, research/62 §8). Monado does not send it today; the handler is cheap and the
  semantics are the spec's. Applied as a rule; built when Monado sends it.
- **§14 (forced):** record the container fork as an open item with its decider (§9 Q1). Applied.
- **§7 (discretionary):** `XR_KHR_visibility_mask` for the projection pass at M2;
  `XR_EXT_local_floor` as the places model's floor; `XR_FB_display_refresh_rate` behind the
  refresh-rate setting. Each is an enable-when-available with a measured gate; listed, not
  ruled.

### `docs/architecture/native-openxr-apps.md` §2 (forced)

State that `XR_EXTX_overlay` is provisional (revision 1, 2018; `xr.xml` `provisional="true"`)
and that the ratified session model for the same problem is `XR_EXT_spatial_container`, under
which the shell and the game are both container clients ordered by the runtime; the ruling
("an overlay session, always") stands until §9 Q1 is decided. Applied.

### `docs/architecture/spatial-mapping.md` §8 and `places-model.md` (discretionary)

- `XR_EXT_stationary_reference_space` as the OpenXR face of the *local* frame: the generation
  id is the "relocalisation broke" signal apps get; the map frame stays Mura's. Proposed text
  for §8; not applied (it touches the two-frame contract's wording).
- `XR_ANDROID_spatial_anchor_space` as the `XrSpace`-bridge shape the frame graph wants from
  the EXT family; proposed as a "reserved hook" line in places-model. Not applied.

### `docs/architecture/window-workspace-management.md` / `zxr-window-management-v1.xml` (discretionary)

The container spec's request-and-deny pattern (async, may take seconds, may show consent UI,
denial as an event, no-op requests queue nothing, `ext_spatial_container.adoc:360-380`) is a
comparable for the seam's `limits` and refusal semantics; the "first show is the app's, then
the system's" rule (`:482-489`) is a comparable for the manager's `show`/`hide` after the
first map. Proposed as two sentences in §12; not applied.

### ADR 0006 (draft amendment text, not applied)

> **Amendment 4 — the container pair.** `XR_EXT_spatial_container` and
> `XR_EXT_spatial_container_self_rendering` (ratified 2026-08, published in 1.1.63) standardise
> runtime-managed 3D windows for OpenXR-native apps. They do not replace `zxr-shell-v2`: the
> lineage's per-pixel cross-client occlusion is not in their contract (the runtime may
> precomposite each container to a stereo quad; depth-based volume clipping is "future
> extensions"). Mura's position: [one of §4 (A)/(B)/(C), owner's ruling], with the policy seam
> [DisplayXR-shaped privileged session / `libmonado`]. The three rev-3 candidates in research/79
> §7a are adopted / declined as [ruling].

## 8. Determinations (confident; the precedent named)

1. **zxr-shell-v2 stays Mura's protocol for Wayland-native 3D clients.** The container pair
   does not carry the lineage's cross-client depth interleave (`self_rendering.adoc:484-486`,
   `:690-691`); motorcar's reason (research/08 §1.5) is untouched. No comparable does both;
   DisplayXR's own compositor depth-tests *window quads* (`spec:125`), not client pixels.
2. **The container pair is the ratified session model for multi-app OpenXR, and Mura's
   fullscreen-game model is its immersive mode.** `ext_spatial_container.adoc:643-670`.
   native-openxr-apps.md's *experience* is unchanged; its *mechanism* (`EXTX_overlay`) is now a
   provisional dependency with a ratified alternative — stated, not decided (§9 Q1).
3. **Three container semantics are worth having in zxr regardless of §9 Q1** — per-frame
   should-submit/recommended-extent hints, an explicit retain, bounds-fitted frusta — because
   they reduce client render cost and cost the compositor nothing it does not already know.
   Their *protocol shape* is discretionary (§7a-1..3).
4. **If Mura implements the pair, it implements it in Monado, not in zxr**, because the
   spec's mechanism (per-client slots, z-order, squashing, visibility/focus events) is Monado's
   multi-client compositor already (§4b), and the one open comparable that built a shell seam
   on Monado (DisplayXR) put the mechanism in the runtime and the policy in the controller
   (`XR_DXR_spatial_workspace.md:17`). What zxr would be is the controller.
5. **Peer identity and leases before any seam.** DisplayXR's audit (`ADR-035:18-48`) is the
   record of skipping them. Monado's IPC has the socket; `SO_PEERCRED` at accept and a
   lease table with Monado's existing primary/overlay rule as the default policy are the
   prerequisites of either seam shape.
6. **Monado work items, in the order the corpus already wants them:** `EXT_view_configuration_
   views_change` (event), `EXT_render_model` + `interaction_render_model` (controller drawing),
   `FB_hand_tracking_aim` on the simulated hands (research/68), the EXT spatial spine
   (research/21 §7), then the container pair if ruled. None is on Mura's critical path before M2.

## 9. Owner questions

**Q1 — Where do OpenXR-native bounded apps land?** *What is decided:* whether Mura implements
`XR_EXT_spatial_container(_self_rendering)`, and where. *Why it is a decision:* Godot master
and Unity 6.6 ship container clients; a Mura without a runtime-side implementation runs them
as immersive 1.x sessions only; implementing it changes what Monado is on Mura. *Options (the
comparables' positions):* **(A)** Monado implements, zxr is the controller (Android XR /
Horizon OS shape; DisplayXR the open instance) — Monado work ≈ one `oxr_workspace.c` plus the
per-container compositor slot, and a seam; **(B)** an API layer translates containers into
zxr-shell-v2 clients (OpenComposite/xrizer shape) — keeps one compositor and the depth
interleave for container apps too, adds a hop; **(C)** not implemented (the lineage today).
*Consequences:* (A) two composition tiers on the device (Monado composites containers, zxr
composites Wayland) and zxr's WM policy crosses IPC; (B) Mura owns a second OpenXR-facing
codebase and container apps do not get runtime-fitted frusta unless zxr sends them; (C) no
bounded multi-app from container-native engines.

**Q2 — Which of the three container semantics enter zxr-shell-v2 rev 3, and how?** *What is
decided:* §7a-1 (retain), §7a-2 (per-frame hints), §7a-3 (fitted frusta + decay). *Why:* each
trades the lineage's exactness rule for client render cost. *Options:* adopt as proposed;
adopt hints only (no semantic change); decline (motorcar's rule). *Consequences:* retain needs
depth reprojection in the compositor (research/65's row, unmeasured); hints are free; decay
changes what a stereo composite of a mono image means at depth.

**Q3 — The policy seam shape, if Q1 = (A).** *Options:* DisplayXR's privileged OpenXR
session with an extension (self-contained, upstream-unfriendly as DXR; Mura would author an
`MNDX`/`EXT` one), or `libmonado` extended (Monado-native C API, no OpenXR surface, what zxr
uses today). *Consequences:* the first makes zxr's WM a second OpenXR session with its own
frame loop; the second keeps it a library call from the state loop.

## 10. Appendix — every supported extension at the pin (253)

Columns: registry number · name · author tag · status at the pin (ratified / published /
provisional; promotions and deprecations from `xr.xml`) · Monado implements (`oxr_extension_
support.py`) · zxr enables today (`xr.rs:256-286`) · relation to Mura, one of
`zxr-consumes` (the compositor as OpenXR client does or should enable it) ·
`Monado-must-provide` (Mura wants it; Monado lacks it; runtime work) · `shell-consumer` (a
shell component's) · `perception-side` (Monado-side perception services' surface) ·
`app-facing-only` (native apps'; zxr indifferent) · `vendor-hw-n/a` (hardware or platform
Mura does not target) · `superseded` (promoted to core, deprecated, or a vendor lineage the
EXT family replaces). Counts: app-facing-only 61, superseded 57, vendor-hw-n/a 44,
perception-side 44, zxr-consumes 27, Monado-must-provide 16, shell-consumer 4. The 746
disabled numbers are reservations without a contract and are not listed.

| # | extension | tag | status | Monado | zxr | relation — note |
|---|---|---|---|---|---|---|
| 4 | `XR_KHR_android_thread_settings` | KHR | ratified | yes | no | app-facing-only — Android thread hints; no Android runtime on Mura |
| 5 | `XR_KHR_android_surface_swapchain` | KHR | ratified | no | no | app-facing-only — Android Surface swapchains; N/A |
| 7 | `XR_KHR_composition_layer_cube` | KHR | ratified | yes | no | app-facing-only — skybox layer; zxr submits none (environment is its own pass) |
| 9 | `XR_KHR_android_create_instance` | KHR | ratified | yes | no | app-facing-only — Android instance creation; N/A |
| 11 | `XR_KHR_composition_layer_depth` | KHR | ratified | yes | no | zxr-consumes — depth beside the projection layer; enables runtime reprojection, never cross-layer ordering (research/65 §7 rule: submit only when the runtime reads it) |
| 15 | `XR_KHR_vulkan_swapchain_format_list` | KHR | ratified | yes | no | zxr-consumes — mutable-format swapchains (sRGB/UNORM views); candidate for the panel swapchains |
| 16 | `XR_EXT_performance_settings` | EXT | ratified | yes | no | zxr-consumes — CPU/GPU level hints + perf notifications; the budgets.md thermal seam (Monado has) |
| 17 | `XR_EXT_thermal_query` | EXT | published | no | no | Monado-must-provide — thermal headroom query; Monado lacks — budgets.md wants it from the device contract instead |
| 18 | `XR_KHR_composition_layer_cylinder` | KHR | ratified | yes | no | app-facing-only — curved quad; zxr composes its own planes |
| 19 | `XR_KHR_composition_layer_equirect` | KHR | ratified | yes | no | superseded — equirect2 replaces |
| 20 | `XR_EXT_debug_utils` | EXT | ratified | yes | no | zxr-consumes — object names + messenger; dev profile only |
| 24 | `XR_KHR_opengl_enable` | KHR | ratified | yes | no | app-facing-only — GL apps (Monado has); zxr is Vulkan |
| 25 | `XR_KHR_opengl_es_enable` | KHR | ratified | yes | no | app-facing-only — GLES apps (Monado has) |
| 26 | `XR_KHR_vulkan_enable` | KHR | ratified | yes | no | superseded — vulkan_enable2 replaces (ADR 0006) |
| 28 | `XR_KHR_D3D11_enable` | KHR | ratified | no | no | vendor-hw-n/a — Windows |
| 29 | `XR_KHR_D3D12_enable` | KHR | ratified | no | no | vendor-hw-n/a — Windows |
| 30 | `XR_KHR_metal_enable` | KHR | ratified | no | no | vendor-hw-n/a — Apple |
| 31 | `XR_EXT_eye_gaze_interaction` | EXT | ratified | yes | yes | zxr-consumes — enabled today (`xr.rs:272`); the gaze tier |
| 32 | `XR_KHR_visibility_mask` | KHR | ratified | yes | no | zxr-consumes — hidden-area mesh for the projection pass; not enabled today — a GPU-budget item for M2 |
| 34 | `XR_EXTX_overlay` | EXTX | provisional | yes | yes | zxr-consumes — enabled today (`xr.rs:286`); **provisional** at the pin — native-openxr-apps.md rests on it (§6) |
| 35 | `XR_KHR_composition_layer_color_scale_bias` | KHR | ratified | yes | yes | zxr-consumes — enabled today (`xr.rs:267`); hover emphasis on quads |
| 36 | `XR_KHR_win32_convert_performance_counter_time` | KHR | ratified | yes | no | vendor-hw-n/a — Windows |
| 37 | `XR_KHR_convert_timespec_time` | KHR | ratified | yes | no | zxr-consumes — XrTime ↔ CLOCK_MONOTONIC; needed for `wp_presentation` feedback and the frame journal |
| 38 | `XR_VARJO_quad_views` | VARJO | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 (`STEREO_WITH_FOVEATED_INSET`); Monado has |
| 39 | `XR_MSFT_unbounded_reference_space` | MSFT | published | yes | no | app-facing-only — large-scale space; Monado has; places-model uses LOCAL/STAGE + stationary (§5) |
| 40 | `XR_MSFT_spatial_anchor` | MSFT | published | no | no | superseded — vendor lineage; EXT_spatial_anchor is the target (research/21) |
| 41 | `XR_FB_composition_layer_image_layout` | FB | published | yes | no | app-facing-only — flip-Y hint; Monado has |
| 42 | `XR_FB_composition_layer_alpha_blend` | FB | published | yes | no | zxr-consumes — per-layer blend factors; premultiplied-alpha quads — Monado has, zxr should verify its quad alpha path against it |
| 43 | `XR_MND_headless` | MND | published | yes | no | app-facing-only — no-graphics sessions (Monado has); candidate for a future non-compositing `--greeter` probe only |
| 45 | `XR_OCULUS_android_session_state_enable` | OCULUS | published | no | no | vendor-hw-n/a — Oculus Android |
| 47 | `XR_EXT_view_configuration_depth_range` | EXT | ratified | no | no | zxr-consumes — recommended near/far per view; the projection pass's depth range |
| 48 | `XR_EXT_conformance_automation` | EXT | ratified | no | no | app-facing-only — CTS input injection; zxr's test-only injector is the compositor-side analogue (§7) |
| 49 | `XR_MNDX_egl_enable` | MNDX | provisional | yes | no | superseded — wxrc's GLES path; ADR 0006 rejected it for vulkan_enable2 |
| 50 | `XR_MSFT_spatial_graph_bridge` | MSFT | published | no | no | vendor-hw-n/a — WMR |
| 51 | `XR_MSFT_hand_interaction` | MSFT | published, promoted→XR_EXT_hand_interaction | yes | no | superseded — promoted to EXT_hand_interaction (Monado has both) |
| 52 | `XR_EXT_hand_tracking` | EXT | ratified | yes | yes | zxr-consumes — enabled today (`xr.rs:273`); the joint bridge (`input/bridge.rs`) |
| 53 | `XR_MSFT_hand_tracking_mesh` | MSFT | published | no | no | vendor-hw-n/a — WMR |
| 54 | `XR_MSFT_secondary_view_configuration` | MSFT | published | no | no | app-facing-only — MRC observer views; no Mura consumer |
| 55 | `XR_MSFT_first_person_observer` | MSFT | published | no | no | vendor-hw-n/a — HoloLens MRC |
| 56 | `XR_MSFT_controller_model` | MSFT | published | no | no | superseded — EXT_render_model is the cross-vendor successor |
| 57 | `XR_MSFT_perception_anchor_interop` | MSFT | published | no | no | vendor-hw-n/a — WinRT |
| 58 | `XR_EXT_win32_appcontainer_compatible` | EXT | published | no | no | vendor-hw-n/a — Windows |
| 60 | `XR_EPIC_view_configuration_fov` | EPIC | published | no | no | app-facing-only — recommended/max FOV per view; zxr reads FOV from `xrLocateViews` |
| 64 | `XR_MSFT_holographic_window_attachment` | MSFT | published | no | no | vendor-hw-n/a — UWP |
| 67 | `XR_MSFT_composition_layer_reprojection` | MSFT | published | no | no | app-facing-only — per-layer reprojection hints; Monado lacks; research/65 ATW row |
| 70 | `XR_HUAWEI_controller_interaction` | HUAWEI | published | no | no | vendor-hw-n/a — interaction profile |
| 71 | `XR_FB_android_surface_swapchain_create` | FB | published | no | no | vendor-hw-n/a — Android |
| 72 | `XR_FB_swapchain_update_state` | FB | published | no | no | app-facing-only — Meta swapchain state; Monado lacks |
| 73 | `XR_FB_composition_layer_secure_content` | FB | published | no | no | app-facing-only — DRM content flag; no Mura policy (capture design may want an analogue) |
| 77 | `XR_FB_body_tracking` | FB | published | yes | no | perception-side — Monado has the API; avatar-persona driver (ADR 0010) |
| 79 | `XR_EXT_dpad_binding` | EXT | ratified | yes | no | app-facing-only — thumbstick-as-dpad bindings; Monado has; zxr's action set could use it for the OSK/menu |
| 80 | `XR_VALVE_analog_threshold` | VALVE | published | no | no | app-facing-only — Monado lacks |
| 81 | `XR_EXT_hand_joints_motion_range` | EXT | ratified | no | no | perception-side — unobstructed vs conforming joints; Monado lacks |
| 89 | `XR_KHR_loader_init` | KHR | ratified | yes | no | app-facing-only — Android loader |
| 90 | `XR_KHR_loader_init_android` | KHR | ratified | yes | no | app-facing-only — Android loader |
| 91 | `XR_KHR_vulkan_enable2` | KHR | ratified | yes | yes | zxr-consumes — enabled today (`xr.rs:260`); the runtime-created device (ADR 0006) |
| 92 | `XR_KHR_composition_layer_equirect2` | KHR | ratified | yes | no | app-facing-only — 360 media layer; a media player's, not zxr's |
| 95 | `XR_EXT_samsung_odyssey_controller` | EXT | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 96 | `XR_EXT_hp_mixed_reality_controller` | EXT | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 97 | `XR_MND_swapchain_usage_input_attachment_bit` | MND | published, deprecated by XR_KHR_swapchain_usage_input_attachment_bit | yes | no | superseded — deprecated by KHR_swapchain_usage_input_attachment_bit |
| 98 | `XR_MSFT_scene_understanding` | MSFT | published | no | no | vendor-hw-n/a — WMR scene API; EXT_spatial_* is the target |
| 99 | `XR_MSFT_scene_understanding_serialization` | MSFT | published | no | no | vendor-hw-n/a — WMR |
| 102 | `XR_FB_display_refresh_rate` | FB | published | yes | no | zxr-consumes — refresh-rate enumerate/request (Monado has); the `display.refresh_rate` setting zxr-core §14 records |
| 103 | `XR_HTC_vive_cosmos_controller_interaction` | HTC | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 104 | `XR_HTCX_vive_tracker_interaction` | HTCX | provisional | yes | no | app-facing-only — provisional tracker profile (Monado has) |
| 105 | `XR_HTC_facial_tracking` | HTC | published | yes | no | perception-side — Monado has; avatar driver input (research/25) |
| 106 | `XR_HTC_vive_focus3_controller_interaction` | HTC | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 107 | `XR_HTC_hand_interaction` | HTC | published | no | no | vendor-hw-n/a — Vive-only profile |
| 108 | `XR_HTC_vive_wrist_tracker_interaction` | HTC | published | no | no | vendor-hw-n/a — Vive |
| 109 | `XR_FB_color_space` | FB | published | no | no | app-facing-only — colour space negotiation; Monado lacks; zxr-core §10 defers colour management to M4 |
| 111 | `XR_FB_hand_tracking_mesh` | FB | published | no | no | perception-side — hand mesh; Monado lacks; the cutout layer is perception-intake's, not this |
| 112 | `XR_FB_hand_tracking_aim` | FB | published | no | no | Monado-must-provide — system-gesture flags zxr already bridges itself (`input/bridge.rs`); research/68 asks Monado for it |
| 113 | `XR_FB_hand_tracking_capsules` | FB | published | no | no | perception-side — collision capsules; Monado lacks |
| 114 | `XR_FB_spatial_entity` | FB | published | no | no | superseded — Meta lineage; EXT_spatial_entity is the target (research/21 §3.2) |
| 115 | `XR_FB_foveation` | FB | published | no | no | app-facing-only — fixed foveation; research/65 foveation row |
| 116 | `XR_FB_foveation_configuration` | FB | published | no | no | app-facing-only — as above |
| 117 | `XR_FB_keyboard_tracking` | FB | published | no | no | perception-side — tracked keyboard; BD has a dynamic-object twin |
| 118 | `XR_FB_triangle_mesh` | FB | published | no | no | app-facing-only — mesh handles for FB scene |
| 119 | `XR_FB_passthrough` | FB | published | yes | no | perception-side — Monado has the API; perception-passthrough-hands.md + ADR 0008 put passthrough Monado-side |
| 120 | `XR_FB_render_model` | FB | published | no | no | superseded — EXT_render_model |
| 121 | `XR_KHR_binding_modification` | KHR | ratified | yes | no | app-facing-only — binding tweaks; Monado has |
| 122 | `XR_VARJO_foveated_rendering` | VARJO | published | no | no | vendor-hw-n/a — Varjo |
| 123 | `XR_VARJO_composition_layer_depth_test` | VARJO | published | no | no | app-facing-only — vendor depth-tested layers (composition.md §6) |
| 124 | `XR_VARJO_environment_depth_estimation` | VARJO | published | no | no | vendor-hw-n/a — Varjo |
| 125 | `XR_VARJO_marker_tracking` | VARJO | published | no | no | superseded — EXT_spatial_marker_tracking |
| 126 | `XR_VARJO_view_offset` | VARJO | published | no | no | vendor-hw-n/a — Varjo |
| 130 | `XR_VARJO_xr4_controller_interaction` | VARJO | published | no | no | vendor-hw-n/a — profile |
| 135 | `XR_ML_ml2_controller_interaction` | ML | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 136 | `XR_ML_frame_end_info` | ML | published | no | no | vendor-hw-n/a — Magic Leap |
| 137 | `XR_ML_global_dimmer` | ML | published | no | no | vendor-hw-n/a — Magic Leap optics |
| 138 | `XR_ML_compat` | ML | published | no | no | vendor-hw-n/a — Magic Leap |
| 139 | `XR_ML_marker_understanding` | ML | published | no | no | superseded — EXT_spatial_marker_tracking |
| 140 | `XR_ML_localization_map` | ML | published | no | no | app-facing-only — map switching; research/23 multi-map comparable |
| 141 | `XR_ML_spatial_anchors` | ML | published | no | no | superseded — EXT_spatial_anchor |
| 142 | `XR_ML_spatial_anchors_storage` | ML | published | no | no | superseded — EXT_spatial_persistence |
| 143 | `XR_MSFT_spatial_anchor_persistence` | MSFT | published | no | no | superseded — EXT_spatial_persistence |
| 148 | `XR_MSFT_scene_marker` | MSFT | published | no | no | superseded — EXT_spatial_marker_tracking |
| 149 | `XR_KHR_extended_struct_name_lengths` | KHR | ratified | yes | no | app-facing-only — name-length bump; Monado has |
| 150 | `XR_ULTRALEAP_hand_tracking_forearm` | ULTRALEAP | published | no | no | perception-side — forearm joint; Ultraleap-only |
| 157 | `XR_FB_spatial_entity_query` | FB | published | no | no | superseded — Meta lineage |
| 159 | `XR_FB_spatial_entity_storage` | FB | published | no | no | superseded — Meta lineage |
| 160 | `XR_OCULUS_audio_device_guid` | OCULUS | published | no | no | vendor-hw-n/a — Windows audio |
| 161 | `XR_FB_foveation_vulkan` | FB | published | no | no | app-facing-only — foveation images |
| 162 | `XR_FB_swapchain_update_state_android_surface` | FB | published | no | no | vendor-hw-n/a — Android |
| 163 | `XR_FB_swapchain_update_state_opengl_es` | FB | published | no | no | vendor-hw-n/a — GLES |
| 164 | `XR_FB_swapchain_update_state_vulkan` | FB | published | no | no | app-facing-only — Monado lacks |
| 166 | `XR_KHR_swapchain_usage_input_attachment_bit` | KHR | ratified | yes | no | app-facing-only — Monado has; zxr's panel pass may want it if it ever reads its own swapchain |
| 168 | `XR_FB_touch_controller_pro` | FB | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 170 | `XR_FB_spatial_entity_sharing` | FB | published | no | no | superseded — Meta lineage |
| 172 | `XR_FB_space_warp` | FB | published, deprecated by XR_EXT_frame_synthesis | no | no | superseded — deprecated by EXT_frame_synthesis |
| 174 | `XR_FB_haptic_amplitude_envelope` | FB | published | no | no | app-facing-only — haptics; EXT_haptic_parametric is the cross-vendor successor |
| 176 | `XR_FB_scene` | FB | published | no | no | superseded — Meta scene lineage; research/22 |
| 177 | `XR_EXT_palm_pose` | EXT | published, promoted→XR_VERSION_1_1 | yes | no | app-facing-only — promoted to 1.1; Monado has; grab pose candidate (research/76) |
| 197 | `XR_ALMALENCE_digital_lens_control` | ALMALENCE | published | no | no | vendor-hw-n/a — API layer |
| 199 | `XR_FB_scene_capture` | FB | published | no | no | superseded — Meta room capture; spatial-mapping.md |
| 200 | `XR_FB_spatial_entity_container` | FB | published | no | no | superseded — Meta lineage — not the EXT container |
| 201 | `XR_META_foveation_eye_tracked` | META | published | no | no | app-facing-only — ETFR; research/65 foveation row |
| 202 | `XR_FB_face_tracking` | FB | published | no | no | superseded — face_tracking2 |
| 203 | `XR_FB_eye_tracking_social` | FB | published | no | no | perception-side — social gaze for avatars (research/25) |
| 204 | `XR_FB_passthrough_keyboard_hands` | FB | published | no | no | perception-side — Meta's hand cutout over a tracked keyboard — mechanism evidence for the cutout layer |
| 205 | `XR_FB_composition_layer_settings` | FB | published | yes | no | app-facing-only — sharpen/supersample flags; Monado has |
| 207 | `XR_FB_touch_controller_proximity` | FB | published | yes | no | app-facing-only — Monado has |
| 210 | `XR_FB_haptic_pcm` | FB | published | yes | no | app-facing-only — Monado has |
| 212 | `XR_EXT_frame_synthesis` | EXT | published | no | no | Monado-must-provide — cross-vendor space warp; research/65 frame-synthesis row; Monado lacks |
| 213 | `XR_FB_composition_layer_depth_test` | FB | published | yes | no | app-facing-only — Monado has; irrelevant to zxr (single projection layer, composition.md §6) |
| 217 | `XR_META_local_dimming` | META | published | no | no | vendor-hw-n/a — Quest Pro |
| 218 | `XR_META_passthrough_preferences` | META | published | no | no | perception-side — passthrough default preference |
| 220 | `XR_META_virtual_keyboard` | META | published | no | no | shell-consumer — runtime-owned OSK — the design Mura rejects (the OSK is a Wayland client, shell-plane.md) |
| 227 | `XR_OCULUS_external_camera` | OCULUS | published | no | no | vendor-hw-n/a — MRC |
| 228 | `XR_META_vulkan_swapchain_create_info` | META | published | no | no | app-facing-only — extra Vulkan usage bits |
| 233 | `XR_META_performance_metrics` | META | published | no | no | zxr-consumes — runtime perf counters; research/65's instrumentation wants an equivalent from Monado |
| 239 | `XR_FB_spatial_entity_storage_batch` | FB | published | no | no | superseded — Meta lineage |
| 241 | `XR_META_detached_controllers` | META | published | no | no | app-facing-only — controllers as trackers |
| 242 | `XR_FB_spatial_entity_user` | FB | published | no | no | superseded — Meta lineage |
| 246 | `XR_META_headset_id` | META | published | no | no | vendor-hw-n/a — Meta |
| 248 | `XR_META_spatial_entity_discovery` | META | published | no | no | superseded — Meta lineage |
| 253 | `XR_META_hand_tracking_microgestures` | META | published | no | no | app-facing-only — thumb swipes; research/63 gesture evidence |
| 255 | `XR_META_recommended_layer_resolution` | META | published | no | no | zxr-consumes — per-layer recommended resolution — the container `recommendedImageExtent` idea for plain layers; Monado lacks |
| 260 | `XR_META_spatial_entity_persistence` | META | published | no | no | superseded — Meta lineage |
| 267 | `XR_META_passthrough_color_lut` | META | published | no | no | perception-side — passthrough colour LUT |
| 270 | `XR_META_spatial_entity_mesh` | META | published | no | no | superseded — Meta lineage |
| 272 | `XR_META_automatic_layer_filter` | META | published | no | no | app-facing-only — layer filtering hint |
| 275 | `XR_META_body_tracking_full_body` | META | published | yes | no | perception-side — Monado has API |
| 280 | `XR_META_touch_controller_plus` | META | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 283 | `XR_META_passthrough_layer_resumed_event` | META | published | no | no | perception-side — passthrough resume event |
| 284 | `XR_META_body_tracking_calibration` | META | published | yes | no | perception-side — Monado has API |
| 285 | `XR_META_body_tracking_fidelity` | META | published | yes | no | perception-side — Monado has API |
| 288 | `XR_FB_face_tracking2` | FB | published | yes | no | perception-side — Monado has; the verified Linux expression path (research/25) |
| 291 | `XR_META_spatial_entity_sharing` | META | published | no | no | superseded — Meta lineage |
| 292 | `XR_META_environment_depth` | META | published | no | no | perception-side — environment depth textures; research/22 depth sources |
| 300 | `XR_EXT_uuid` | EXT | published, promoted→XR_VERSION_1_1 | no | no | app-facing-only — promoted to 1.1; the spatial family's id type |
| 301 | `XR_EXT_render_model` | EXT | ratified | no | no | Monado-must-provide — runtime-supplied glTF controller/hand models; Monado lacks; who draws controllers in zxr is open (§6) |
| 302 | `XR_EXT_interaction_render_model` | EXT | ratified | no | no | Monado-must-provide — binds render models to user paths; Monado lacks |
| 303 | `XR_EXT_hand_interaction` | EXT | ratified | yes | yes | zxr-consumes — enabled today (`xr.rs:271`); Monado has the profile but no device — the bridge fills it |
| 307 | `XR_QCOM_tracking_optimization_settings` | QCOM | published | no | no | vendor-hw-n/a — Snapdragon |
| 311 | `XR_QCOM_hand_tracking_gesture` | QCOM | published, deprecated by XR_EXT_hand_interaction | no | no | superseded — deprecated by EXT_hand_interaction |
| 318 | `XR_HTC_passthrough` | HTC | published | no | no | vendor-hw-n/a — Vive |
| 319 | `XR_HTC_foveation` | HTC | published | no | no | vendor-hw-n/a — Vive |
| 320 | `XR_HTC_anchor` | HTC | published | no | no | superseded — EXT_spatial_anchor |
| 321 | `XR_HTC_body_tracking` | HTC | published | no | no | perception-side — Vive |
| 374 | `XR_EXT_active_action_set_priority` | EXT | published | yes | no | zxr-consumes — per-sync action-set priority; Monado has; zxr's one action set does not need it until a second set exists |
| 376 | `XR_MNDX_force_feedback_curl` | MNDX | provisional | yes | no | app-facing-only — provisional glove feedback (Monado) |
| 385 | `XR_BD_controller_interaction` | BD | published, promoted→XR_VERSION_1_1 | yes | no | superseded — promoted to 1.1 |
| 386 | `XR_BD_body_tracking` | BD | published | yes | no | perception-side — Monado has API |
| 387 | `XR_BD_facial_simulation` | BD | published | no | no | perception-side — PICO |
| 390 | `XR_BD_spatial_sensing` | BD | published | no | no | superseded — PICO's own framework, not EXT |
| 391 | `XR_BD_spatial_anchor` | BD | published | no | no | superseded — PICO lineage |
| 392 | `XR_BD_spatial_anchor_sharing` | BD | published | no | no | superseded — PICO lineage |
| 393 | `XR_BD_spatial_scene` | BD | published | no | no | superseded — PICO lineage |
| 394 | `XR_BD_spatial_mesh` | BD | published | no | no | superseded — PICO lineage |
| 395 | `XR_BD_future_progress` | BD | published | no | no | app-facing-only — future progress reporting |
| 396 | `XR_BD_body_tracking_auxiliary_metrics` | BD | published | no | no | perception-side — PICO |
| 397 | `XR_BD_spatial_plane` | BD | published | no | no | superseded — PICO lineage |
| 398 | `XR_BD_spatial_light_estimation` | BD | published | no | no | perception-side — light estimation; BD framework |
| 404 | `XR_BD_ultra_controller_interaction` | BD | published | no | no | vendor-hw-n/a — profile |
| 410 | `XR_BD_spatial_audio_rendering` | BD | published | no | no | app-facing-only — runtime spatial audio; Mura's is PipeWire's |
| 427 | `XR_EXT_local_floor` | EXT | published, promoted→XR_VERSION_1_1 | yes | no | zxr-consumes — promoted to 1.1; Monado has; places-model's floor frame should ground here |
| 429 | `XR_EXT_hand_tracking_data_source` | EXT | ratified | yes | no | perception-side — Monado has; hands-from-controllers vs sensed |
| 430 | `XR_EXT_plane_detection` | EXT | published, deprecated by XR_EXT_spatial_plane_tracking | yes | no | superseded — deprecated by EXT_spatial_plane_tracking; Monado has the old one (research/21 §7.3) |
| 454 | `XR_OPPO_controller_interaction` | OPPO | published | yes | no | vendor-hw-n/a — profile (Monado has) |
| 456 | `XR_ANDROID_trackables` | ANDROID | published, deprecated by XR_EXT_spatial_entity, XR_EXT_spatial_plane_tracking, XR_ANDROID_spatial_component_subsumed_by, XR_EXT_spatial_anchor, XR_ANDROID_spatial_entity_bound_anchor, XR_ANDROID_spatial_anchor_space | no | no | superseded — deprecated by the EXT family |
| 457 | `XR_ANDROID_eye_tracking` | ANDROID | published | no | no | perception-side — Android XR gaze |
| 458 | `XR_ANDROID_device_anchor_persistence` | ANDROID | published, deprecated by XR_EXT_spatial_persistence, XR_EXT_spatial_persistence_operations | no | no | superseded — deprecated by EXT_spatial_persistence(_operations) |
| 459 | `XR_ANDROID_face_tracking` | ANDROID | published | yes | no | perception-side — Monado has API |
| 461 | `XR_ANDROID_passthrough_camera_state` | ANDROID | published | no | no | perception-side — camera state |
| 462 | `XR_ANDROID_recommended_resolution` | ANDROID | published | no | no | zxr-consumes — dynamic recommended resolution per view; Monado lacks; the views_change twin |
| 463 | `XR_ANDROID_composition_layer_passthrough_mesh` | ANDROID | published | no | no | perception-side — passthrough through a mesh — cutout-layer mechanism evidence |
| 464 | `XR_ANDROID_raycast` | ANDROID | published, deprecated by XR_ANDROID_spatial_discovery_raycast | no | no | superseded — deprecated by ANDROID_spatial_discovery_raycast |
| 466 | `XR_ANDROID_performance_metrics` | ANDROID | published | no | no | zxr-consumes — perf counters (see META twin) |
| 467 | `XR_ANDROID_trackables_object` | ANDROID | published, deprecated by XR_ANDROID_spatial_object_tracking | no | no | superseded — deprecated by ANDROID_spatial_object_tracking |
| 468 | `XR_ANDROID_unbounded_reference_space` | ANDROID | published | no | no | app-facing-only — as MSFT twin |
| 470 | `XR_EXT_future` | EXT | ratified | yes | no | app-facing-only — async pattern for the spatial family; Monado has |
| 471 | `XR_EXT_user_presence` | EXT | published | yes | yes | zxr-consumes — enabled today (`xr.rs:265`); doff/don (ADR 0007) |
| 472 | `XR_KHR_locate_spaces` | KHR | ratified, promoted→XR_VERSION_1_1 | yes | yes | zxr-consumes — enabled today (`xr.rs:262`); promoted to 1.1; batch space location |
| 473 | `XR_ML_user_calibration` | ML | published | no | no | vendor-hw-n/a — Magic Leap |
| 474 | `XR_ML_system_notifications` | ML | published | no | no | shell-consumer — suppress runtime notifications — Mura's notifications are mako's, not the runtime's |
| 475 | `XR_ML_world_mesh_detection` | ML | published | no | no | perception-side — Magic Leap meshing |
| 483 | `XR_ML_facial_expression` | ML | published | no | no | perception-side — Magic Leap |
| 484 | `XR_ML_view_configuration_depth_range_change` | ML | published | no | no | zxr-consumes — depth-range change event; twin of EXT_view_configuration_depth_range |
| 498 | `XR_YVR_controller_interaction` | YVR | published | no | no | vendor-hw-n/a — profile |
| 529 | `XR_META_boundary_visibility` | META | published | no | no | shell-consumer — suppress the runtime boundary; Mura's boundary is the compositor's authority layer |
| 533 | `XR_META_simultaneous_hands_and_controllers` | META | published | no | no | app-facing-only — multimodal input; research/63 handoff evidence |
| 542 | `XR_META_face_tracking_visemes` | META | published | no | no | perception-side — avatar audio-visemes |
| 553 | `XR_META_spatial_entity_semantic_label` | META | published | no | no | superseded — Meta lineage |
| 554 | `XR_META_spatial_entity_room_mesh` | META | published | no | no | superseded — Meta lineage |
| 555 | `XR_EXT_composition_layer_inverted_alpha` | EXT | published | yes | no | app-facing-only — Monado has; inverted-alpha layers |
| 572 | `XR_META_colocation_discovery` | META | published | no | no | app-facing-only — colocated sessions; spatial-sharing.md workspace-join comparable |
| 573 | `XR_META_spatial_entity_group_sharing` | META | published | no | no | superseded — Meta lineage |
| 593 | `XR_META_environment_raycast` | META | published | no | no | perception-side — environment raycast; research/22 |
| 610 | `XR_META_tile_properties_hint` | META | published | no | no | vendor-hw-n/a — Meta tiler |
| 694 | `XR_META_hand_tracking_unextrapolated_poses` | META | published | no | no | perception-side — pose extrapolation control |
| 695 | `XR_META_hand_tracking_frequency_hint` | META | published | no | no | perception-side — hand-tracking rate hint — a budgets.md knob if Monado grows one |
| 696 | `XR_META_hand_tracking_wide_motion_mode2` | META | published | no | no | perception-side — wide-motion hands |
| 701 | `XR_ANDROID_light_estimation` | ANDROID | published | no | no | perception-side — light estimation |
| 702 | `XR_ANDROID_anchor_sharing_export` | ANDROID | published | no | no | app-facing-only — anchor export; research/21 sharing |
| 705 | `XR_ANDROID_mouse_interaction` | ANDROID | published | no | no | zxr-consumes — a mouse as an OpenXR interaction profile — Mura's mice arrive via libinput on the seat (research/68), so a comparable, not a consumer |
| 708 | `XR_ANDROID_trackables_marker` | ANDROID | published, deprecated by XR_EXT_spatial_marker_tracking | no | no | superseded — deprecated by EXT_spatial_marker_tracking |
| 709 | `XR_ANDROID_trackables_qr_code` | ANDROID | published, deprecated by XR_EXT_spatial_marker_tracking | no | no | superseded — deprecated by EXT_spatial_marker_tracking |
| 710 | `XR_ANDROID_trackables_image` | ANDROID | published | no | no | superseded — EXT_spatial_image_tracking is the cross-vendor form |
| 711 | `XR_KHR_maintenance1` | KHR | ratified, promoted→XR_VERSION_1_1 | yes | no | app-facing-only — promoted to 1.1; Monado has |
| 712 | `XR_KHR_generic_controller` | KHR | ratified | yes | no | zxr-consumes — rich fallback profile; Monado has; zxr's action set should bind it (research/70 stand-ins) |
| 719 | `XR_ANDROID_scene_meshing` | ANDROID | published | no | no | perception-side — Android meshing; research/22 comparable |
| 741 | `XR_EXT_spatial_entity` | EXT | ratified | no | no | Monado-must-provide — the shared spatial spine; Monado lacks (research/21 §7) |
| 742 | `XR_EXT_spatial_plane_tracking` | EXT | ratified | no | no | Monado-must-provide — planes via the entity spine; Monado lacks |
| 743 | `XR_EXT_stationary_reference_space` | EXT | published | no | no | Monado-must-provide — best-effort persistent world origin + generation id; the map/local split's OpenXR face (§5) |
| 744 | `XR_EXT_spatial_marker_tracking` | EXT | ratified | no | no | Monado-must-provide — QR/ArUco/AprilTag; Monado lacks |
| 746 | `XR_LOGITECH_mx_ink_stylus_interaction` | LOGITECH | published | yes | no | app-facing-only — stylus profile (Monado has) |
| 747 | `XR_BD_dynamic_object_tracking` | BD | published | no | no | perception-side — PICO dynamic objects |
| 748 | `XR_BD_dynamic_object_keyboard` | BD | published | no | no | perception-side — tracked keyboard (PICO) |
| 749 | `XR_BD_dynamic_object_mouse` | BD | published | no | no | perception-side — tracked mouse (PICO) |
| 756 | `XR_BD_camera_image` | BD | published | no | no | perception-side — vendor camera API; Mura's cameras are V4L2 → perception (research/47) |
| 762 | `XR_ANDROID_spatial_discovery_bounds` | ANDROID | published | no | no | app-facing-only — discovery bounds config |
| 763 | `XR_EXT_spatial_anchor` | EXT | ratified | no | no | Monado-must-provide — anchors as entities; Monado lacks |
| 764 | `XR_EXT_spatial_persistence` | EXT | ratified | no | no | Monado-must-provide — persistence scopes/contexts; Monado lacks; spatial-mapping.md §6 store |
| 776 | `XR_EXT_haptic_parametric` | EXT | published | no | no | app-facing-only — parametric haptics; Monado lacks |
| 777 | `XR_SONY_swapchain_color_space` | SONY | published | no | no | vendor-hw-n/a — PSVR2 |
| 778 | `XR_SONY_hdr_metadata` | SONY | published | no | no | vendor-hw-n/a — PSVR2 |
| 782 | `XR_EXT_spatial_persistence_operations` | EXT | ratified | no | no | Monado-must-provide — persist/unpersist; Monado lacks |
| 783 | `XR_EXT_spatial_image_tracking` | EXT | ratified | no | no | Monado-must-provide — reference-image tracking (new at 1.1.63); Monado lacks |
| 786 | `XR_ANDROID_spatial_object_tracking` | ANDROID | published | no | no | perception-side — object categories on the EXT spine |
| 787 | `XR_ANDROID_spatial_discovery_raycast` | ANDROID | published | no | no | perception-side — raycast discovery on the EXT spine |
| 788 | `XR_ANDROID_google_cloud_auth` | ANDROID | published | no | no | vendor-hw-n/a — Google Cloud |
| 790 | `XR_ANDROID_geospatial` | ANDROID | published | no | no | vendor-hw-n/a — VPS |
| 791 | `XR_ANDROID_spatial_entity_bound_anchor` | ANDROID | published | no | no | app-facing-only — anchor-to-component; a places-model attachment comparable |
| 792 | `XR_ANDROID_spatial_component_subsumed_by` | ANDROID | published | no | no | perception-side — merged-plane bookkeeping |
| 796 | `XR_ANDROID_spatial_anchor_space` | ANDROID | published | no | no | Monado-must-provide — anchors as `XrSpace` handles — the bridge the places model wants (§5) |
| 798 | `XR_ANDROID_geospatial_anchor` | ANDROID | published | no | no | vendor-hw-n/a — VPS |
| 811 | `XR_EXT_spatial_container` | EXT | ratified | no | no | Monado-must-provide — 3D windows managed by the runtime; **the zxr-shell-v2 question** (§3–§4) |
| 814 | `XR_EXT_spatial_container_self_rendering` | EXT | ratified | no | no | Monado-must-provide — per-container views + layer submission (§3–§4) |
| 837 | `XR_EXT_interaction_profile_battery_state_display` | EXT | ratified | no | no | shell-consumer — display-only battery for the panel/tray; Monado lacks; libmonado has `get_device_battery_status` today |
| 839 | `XR_EXT_loader_init_properties` | EXT | published | no | no | app-facing-only — loader init on non-Android; Monado lacks |
| 840 | `XR_EXT_view_configuration_views_change` | EXT | published | no | no | zxr-consumes — recommended-view change event; zxr must re-enumerate + regrow (§2); Monado lacks |
| 875 | `XR_KHR_extended_result_name_lengths` | KHR | ratified | no | no | app-facing-only — name-length bump |
