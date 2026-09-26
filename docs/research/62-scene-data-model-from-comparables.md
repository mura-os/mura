# 62 — The scene data model: how compositors shape the in-memory world, and what zxr's `scene` should be

**Research date:** 2026-09-26. **Question:** [specs/zxr-core.md §3](../../specs/zxr-core.md)
gives the `scene` module the layer model (§4), places and frames (§5), window/plane state,
stacking, the depth sort and buffer references — but not the *shape* of the structure that holds
them. R0 has `Vec<Plane>`. Before M1 code adds frames, layers, popups-as-children and, at M2, 3D
client volumes to it, this document reads how every pinned compositor shaped the same thing and
why, so the shape is derived rather than invented. The places model
([places-model.md §2](../architecture/places-model.md)) already fixes *what* the graph contains
(frames rooted in the runtime's spaces → places → windows; reparent verbs); this is about *how the
code holds it*: one structure or two, tree or list, what is a node, how policy attaches, how it
is tested, how it flattens into a frame.
**Method (AGENTS.md rule 7):** lineage first (motorcar → wxrc → wxrd), then the XR comparables
(StardustXR, zen/zwin, Simula, xrdesktop/g3k, kwin-vr, wayvr) and the 2D ones whose problem is
the same in a different dimension (niri, cosmic-comp, KWin, mutter, gamescope), and finally the
runtime's own view (Monado). For each: the problem, what it chose, **why** (its comments), its
assumptions, whether they transfer, the trade-off. Citations are `references/<clone>/path:line`;
absences in the pins are stated as such. **Budget impact:** a research document; the shape it
selects is charged where §6 says (an arena of small nodes and a per-frame flatten — no retained
render tree, no per-node damage state).

## 1. Lineage

### 1.1 motorcar — one scene graph for everything, a WindowManager object for policy

**Problem.** A 3D windowing compositor where 2D surfaces, 3D clients, displays and 6-DoF input
devices share one space. **Shape.** A single tree. `SceneGraphNode` holds `m_transform`,
`m_inverseTransform`, `m_parentNode`, `m_childNodes` (`motorcar/src/compositor/scenegraph/scenegraphnode.h:119-121`);
every subtree operation is `mapOntoSubTree` — "This function forms the core of the scenegraph …
calls it on the current node, and then recursively maps it onto all of its children"
(`scenegraphnode.h:137-139`, impl `scenegraphnode.cpp:124-129`). Two typed families:
`PhysicalNode` (things with a physical pose — `Scene`, `Display`, `Skeleton`, `Bone`,
`SixDOFPointingDevice`) and `VirtualNode` (things that may animate — `Drawable`, `ViewPoint`,
surface nodes; `virtualnode.h:46-48`). **Surfaces are nodes**: `WaylandSurfaceNode : Drawable`
with `m_surface`, `m_mapped`, `m_surfaceTransform` (`waylandsurfacenode.h:90-109`);
`MotorcarSurfaceNode` adds the 3D client's dimensions, depth-composite shaders and cuboid clip
(`motorcarsurfacenode.h:42-98`, "extracts the depth and color information from the client
surface, clips them against the surface boundaries, and composites with the scene",
`:51-52`). **Displays and viewpoints are nodes** (`display.h:48-94`, `viewpoint.h:51-121`).
**Input devices are nodes**: `SixDOFPointingDevice : PhysicalNode` casts its ray every frame via
`scene->intersectWithSurfaces` and emits `MouseEvent`/`SixDofEvent`
(`sixdofpointingdevice.cpp:82-188`); the cursor is a surface node re-parented under whatever
surface the ray hits (`:98-107`). **Popups and transients are children of the surface they
belong to**, at `popupZOffset = 0.05` in front (`windowmanager.cpp:166-197`). **Policy** is one
object: `WindowManager` — "Handles input events and window positioning … creating surfaceNodes
for new surfaces, and for positioning them in the scene, as well as delivering events"
(`windowmanager.h:48-50`); toplevels are parented to the scene at translate(0,0,1)·rotate((n−1)·−30°)·translate(0,0,−1.5)
(`windowmanager.cpp:147-164`) — the fan zxr's R0 copies. **Frame discipline**:
`handleFrameBegin` "is virtual function called once per frame on all nodes … the spacial
configuration of the scene should not be modified outside of this function"
(`scenegraphnode.h:56-59`); `Scene::drawFrame` maps `handleFrameDraw` per display
(`scene.cpp:88-96`). **Why** (README): "designed to provide basic 3D windowing infrastructure …
with the simplest mechanism possible" (`README.md:6-7`); the compositor binary only "set[s] up
the scene and insert[s] devices into the scenegraph" (`:115`). **Assumptions:** one process,
GL, Qt-Wayland, one author. **Transfer:** the *content* of the graph transfers wholesale — it is
the places model's frame graph with input and viewpoints added — and the popup-as-child rule
transfers as is. The *implementation* (virtual-dispatch node classes, pointer parents, hard-coded
8 px/cm at `waylandsurfacenode.cpp:237-241`) does not. **Trade-off:** a single graph makes
"reparent = the operation" trivial, and makes every node pay for every capability.

### 1.2 wxrc — a flat list; the prototype's shape

`wxrc_server.views` is a `wl_list` of `wxrc_view { position, rotation, mapped, link }`
(`wxrc/include/view.h:30-38`; `view.c:12`); render iterates it in reverse (`render.c:415-421`),
hit-testing iterates forward and takes the *first* plane hit, not the nearest (`input.c:158-193`)
with the TODOs "logical Z ordering" (`:138`) and depth strategies between 2D and 3D (`:181-186`).
Placement: `(0,0,−2)` rotated into the first view (`xdg-shell.c:75-95`). **Why flat:** no comment
says; the shape is what a wlroots prototype gets for free. **Verdict against the lineage's own
ancestor:** wxrc's TODOs are exactly the things motorcar's tree already had (ordering, nearest
hit, parenting). The list is the *prototype* shape, not a position.

### 1.3 wxrd — the list kept, the graph outsourced to g3k

`wxrd_server.views` is still a `wl_list` (`wxrd/src/server.h:23-86`); each `wxrd_view` carries an
`XrdWindow *window`, a parent view and `offset_to_parent` (`view.h:79-108`); on map it creates an
`XrdWindow`, parents it under the parent's window or places it through g3k, and hands it to
`xrd_shell_add_window` (`view.c:174-236`). It does not use `wlr_scene` (no occurrence in the
tree; README: wlroots' GL renderer "runs but is unused", `README.md:99`). So the descendant kept
wxrc's list for *Wayland bookkeeping* and let a 3D scene library (g3k) own poses, parenting and
hover — a two-structure shape by accident of dependency.

## 2. XR comparables

### 2.1 StardustXR server — `Spatial` graph, SDF fields, the shell is a client

**Shape.** `Spatial { entity, parent: Option<Arc<Spatial>>, transform: Mat4, children: Registry<Spatial>, bounding_box_calc, moved_callback }`
(`stardustxr-server/src/nodes/spatial.rs:179-186`); global = parent × local (`:348-355`);
`space_to_space_matrix` (`:283-287`). **Reparenting is designed, not incidental** — its comment is
the most precise statement in the corpus of what a reparent *is*: "Reparenting changes this
subtree's global pose even though no local transform was touched" and "This node's registry
holds its own moved callbacks plus those aggregated up from its entire subtree. Reparenting moves
that whole set off the old ancestor chain and onto the new one" (`spatial.rs:422-453`). Hit
testing is not ray-vs-plane but **fields** (SDFs: `Field { spatial, shape, … }`,
`fields.rs:510-516`, sample/ray-march `:667-698`) and typed queries — Beam / Zone / Points
(`query/spatial_query.rs:45-49`), hits typed "so a Beam hit can never be handed to a Zone
handler". **Input devices are Spatials** (`objects/input/mod.rs:118-132`); HMD and stage are
Spatials (`objects/hmd.rs:36-48`, `objects/stage.rs:36-48`). **Policy is not in the server:**
clients mutate the graph over `set_parent` / `set_local_transform` / `set_relative_transform`
(`spatial.rs:531-566`); the server owns the registry and the entities. **Testable without a
runtime:** `Spatial::test_new`, `Field::test_new`, unit tests and benches on zone/beam math
(`spatial.rs:238-248, 656-792`; `spatial_query.rs:730+`). **Pin caveat:** this commit
(`cf20614`) has no `items/panel` and no `src/wayland/` although `WARP.md:32-40` still describes
them — the 2D-surface node is not readable here. **Transfer:** the reparent semantics and the
"server owns graph, a client owns placement" seam are exactly ADR 0012's WM-policy seam in its
eventual protocol form; SDF fields are more than planes need at M1 but are the natural hit model
once 3D clients (M2) have volumes. **Trade-off:** a graph that any client may mutate needs the
capture/priority machinery Stardust has (`order_handlers_and_captures`, `mod.rs:58-67`); zxr's
in-process policy avoids that until the bounded protocol exists.

### 2.2 zen (zwin) — 2D views on boards, boards in a 3D ray-cast tree; the closest living relative of motorcar's *virtual objects*

**Its own structure statement** (`zen/doc/structure.adoc`): five prefixes = five layers —
`zn_` "Core component", `zna_` "Appearance of objects in immersive display system", `zns_`
"Interactive shell in immersive display system", `znr_` "Rendering backend for immersive display
system", `zwnr_` "thin library of zwin protocol". That is a model / appearance / shell / renderer
split made explicit in the file layout, and the reason is zen's deployment: the model lives on
the PC, the appearance is streamed as GLES objects to a remote headset renderer (`znr-remote/`,
the `zwin-gles-v32` wire format of research/10 §2.3). **Shape.** Three layers of structure. `zn_scene { screen_layout, board_list, view_list, cursor, ray, focused_view }`
(`zen/include/zen/scene.h:13-31`); `zn_board` is a plane with `geometry.{transform,size}`
("translation and rotation only") and a `view_list` "sorted from back to front"
(`board.h:15-37`); `zn_view` is a 2D window *on a board* with `x, y, z_index` (`view.h:28-73`),
stacked by `VIEW_Z_OFFSET_GAP` (`board.h:11-13`). Above that, `zns_node { parent, children, type: ROOT|BOUNDED|BOUNDED_NAMEPLATE|EXPANSIVE|BOARD, transform, vtable }`
is the **hit tree**: `zns_node_ray_cast` multiplies transforms and keeps the nearest child
(`zns/src/node.c:6-26`; contract `node.h:17-21`), and events bubble to the parent when a child
declines (`node.c:29-86`). 3D apps are `zwnr_virtual_object` with `BOUNDED` (half-size + region)
or `EXPANSIVE` roles (`zwnroot/include/zwnr/virtual-object.h:15-36`, `bounded.h:24-47`) — the
direct descendants of motorcar's `CUBOID` and `PORTAL` clipping modes (§1.1): a bounded app owns
a half-size box it may not draw outside; an expansive one owns a region and no box. The shell's
node for a bounded app carries `seat_capsule_azimuthal/polar` (`zns/include/zns/bounded.h:18-24`):
3D apps are allotted positions on a capsule around the seat — the same angular allocation
kwin-vr's `SpaceAllocator3D` (§2.5) and research/36's selected slot search arrive at. The
`zns_node_interface` vtable is the hit contract in full: `ray_cast` "stores the minimum
collision distance found so far … overrides `distance` with the collision distance" and every
`ray_*` event handler returns "false to pass the event to the parent node"
(`zns/include/zns/node.h:16-56`). A separate `zna_*` layer mirrors scene objects into remote
GLES per frame. **Why:** the design prose is the structure document above; beyond it the code
speaks — 2D windows never get a free 3D pose, they live on boards, and boards are the unit of 3D
placement, while 3D apps are free nodes on the capsule. **Where it is motorcar's heir and where
it departs:** the virtual-object model (bounded/expansive, server-side clipping, one ray tree for
2D boards and 3D apps alike) is motorcar's thesis carried forward; the treatment of 2D windows
is not — motorcar (and the places model) let a surface be a free node; zen pins every 2D window
to a board. **Transfer:** the board is the places model's *place* with a flat layout policy; the
z-gap stacking of views within a board is the popup/child offset rule again; the ray tree with
nearest-wins + bubbling is the cleanest statement of 3D hit traversal in the corpus; the
bounded/expansive roles are what zxr-shell-v2's 3D clients (M2) will be as nodes. The
model/appearance/renderer split is zen's answer to *remote* rendering, which zxr does not do
(research/59 §2: one process, one projection layer) — it is the same reasoning as KWin/mutter's
render tree (§3.3) and does not transfer for the same reason. **Trade-off:** the strict
2D-on-board rule forbids a free-floating window; the places model allows a place to hold one
window, so no loss.

### 2.3 Simula — Godot's scene tree, workspaces as parent Spatials

Windows are `GodotSimulaViewSprite : GodotRigidBody` with a `MeshInstance` and a BoxShape
(`simula/addons/godot-haskell-plugin/src/Plugin/Types.hs:532-574`), parented on map to the
**current workspace `Spatial`** (`SimulaViewSprite.hs:1182-1186`) then `setInFrontOfUser`
(`:1214`); workspaces are ten `GodotSpatial`s and switching changes which Spatial new windows
inherit (`Types.hs:471-528`, `SimulaServer.hs:599-606`). Hit testing is Godot physics
(`_input_event` on the RigidBody → wlroots notify, `SimulaViewSprite.hs:414-451`). **Why:** README
— "VR window manager … that runs on top of Godot"; a reimplementation fork of motorcar
(`README.org:2, 20`). **Transfer:** "workspace = a parent node; switching = reparent target" is
the places model's *place* again, from a third independent implementation. Nothing else
transfers (engine-owned tree, Haskell).

### 2.4 xrdesktop / g3k — plane objects with flags; identity by lookup

`XrdWindow : G3kPlane` with `native`, `children`/`parent`, `pinned`, `rect`
(`xrdesktop/src/xrd-window.c:56-73`); "The 'base window' has a width of 1 unit … Scale the
window by scaling the scene object" (`:37-42`). `XrdShell` holds `G3kContext`, a
`window_mapping` hash and the windows node (`xrd-shell.c:36-84`). Policy attaches as **flags on
the object**: draggable → `DRAGGABLE | MANAGED`; "Desktop windows should set [draggable] to TRUE
… FALSE for child windows / windows in a container attached to the FOV, a controller, etc."
(`xrd-shell.c:205-236`). The identity lesson: "an #XrdWindow can be replaced by the overlay-scene
switch. Therefore the #XrdWindow should always be looked up instead of cached" (`:216-218`).
Hover state lives in g3k per controller (`:406-408`, `1484-1493`). **Pin caveat:** g3k itself is
not cloned; the scene/overlay dual implementation is unreadable here. **Transfer:** flags as the
policy vocabulary (draggable/managed/hoverable) and *ids over pointers* both transfer.

### 2.5 kwin-vr — reparenting **is** the detach; angular allocation; picking by ray

The Qt Quick 3D scene: `XrScene` holds an `XrView`, an `allWindows` node, a `SpaceAllocator3D`,
output mirrors and application windows (`kwin-vr/src/plugins/vr/qml/XrScene.qml:258-434`). A
`KwinPseudoOutputMirror` is a node per KWin output with a pickable `VrScreenFrame` and a
`ZStacker` ordering children by KWin's `stackingOrder` (`KwinPseudoOutputMirror.qml:11-58`). A
`KwinApplicationWindow` — "one non transient window and all its transient windows (menus,
popups, other normal windows) arranged as a stack of 3D rectangles" (`KwinApplicationWindow.qml:12-15`)
— has two states: `screen` (parent = the output mirror; position from 2D `frameGeometry`) and
`vr` (parent = `allWindowsGrabHandle`, own grab handle) (`XrScene.qml:398-432`). **The detach
of research/31 §2.9 is a reparent between those two parents.** Placement of free objects:
`SpaceAllocator3D` — "Objects are projected onto a sphere around the viewpoint. The allocator
checks for overlap in angular space (azimuth/elevation) to prevent occlusion"
(`spaceallocator3d.h:24-29`). Picking: `xrView.rayPickAll` → first object accepting `onPick`
(`VrPicking.qml:34-65`). **Assumption:** KWin's `Window` model remains the WM authority; the 3D
tree is a *presentation* of it. **Transfer:** the two-parent state machine is the cleanest
existing code for "member of a screen place" vs "member of a free place"; the angular allocator
is a candidate for `policy` (research/36 already selected the angular-slot search).

### 2.6 wayvr — no graph; poses live on the overlay

`WvrServerState { processes: DenseSlotMap, wm.windows: DenseSlotMap<WindowHandle, Window>, window_to_overlay … }`
(`wlx-overlay-s/wayvr/src/backend/wayvr/mod.rs:137-151`, `window.rs:116-120`); `Window` has size
bounds, a toplevel and a process handle — **no pose** (`window.rs:12-23`); the pose is on the
host `OverlayWindowState.transform` (`windowing/window.rs:15-21, 70-95`). Hit testing walks the
Wayland surface tree per overlay (`hit_test.rs:186-220`). **Why:** each window is one OpenXR
*overlay layer*; the runtime composites them; there is nothing to order in a scene. **Transfer:**
none for structure (zxr submits one projection layer and does its own compositing — research/59
§2); the *slotmap keyed by handle* is the Rust idiom for identity that xrdesktop's lesson asks
for.

## 3. The 2D compositors — the same problem in one fewer dimension

### 3.1 niri — a layout model generic over the window, tested without Wayland

`Layout<W: LayoutElement>` → `MonitorSet` → `Monitor` → `Workspace` → `ScrollingSpace`/`FloatingSpace`
→ `Column` → `Tile { window: W }` (`niri/src/layout/mod.rs:342-394`, `monitor.rs:50-67`,
`workspace.rs:47-52`, `scrolling.rs:36-38, 166-170`, `tile.rs:40-42`, `floating.rs:36-38`). The
`LayoutElement` trait is what the layout needs of a window and nothing else — `size` ("the size
the user would consider … Corresponds to the Wayland window geometry size"), `request_size`,
`min_size`/`max_size`, `set_activated`, `set_bounds`, `is_child_of`, `rules`, `refresh`,
`on_commit`, and a `render` that draws "in such a way that its visual geometry ends up at the
given location" (`mod.rs:131-339`). The real window `Mapped` implements it with
`type Id = Window` (`window/mapped.rs:622-626`); **the tests implement it with `TestWindow`**
(`layout/tests.rs:26-47, 152-285`) and drive the layout with a proptest `Op` enum of every
user-visible operation (`:408-757`), calling `Layout::verify_invariants` after each
(`mod.rs:2420-2618`: valid indices, workspaces on their preferred connected output, unique ids,
at most one gesture …) — "Running every op from an empty state doesn't get us to all the
interesting states" (`:1897-1899`), so the sweep also starts from a populated layout.
**Qualification:** the module is generic over the window but not free of smithay — it imports
`Output`, `WlSurface`, `Logical` geometry and the GLES render-element types (`mod.rs:46-51`).
Layer-shell is **outside** the layout (`Niri.mapped_layer_surfaces`, `niri.rs:253-254`;
`MappedLayer` does not implement `LayoutElement`); the frame interleaves layout and layers in
`Niri` (`niri.rs:4452+`). Handlers call into the layout (`handlers/compositor.rs:213-221`
`layout.add_window(...)`). **Why (module doc):** the two output principles — "Disconnecting and
reconnecting the same output must not change the layout"; "Connecting an output must not change
the layout for any workspaces that were never on that output" (`mod.rs:16-20`) — are
*invariants*, and the generic/tested shape exists to hold them. **Transfer:** the discipline
transfers exactly: zxr's graph has invariants of the same kind (places-model C1–C7 reconciliation
rules; "a window is parented to exactly one place"), and a member trait + test member + property
sweep is how a compositor keeps them. **Trade-off:** the trait has ~40 methods because rendering
goes through it; zxr can keep rendering out of the trait (§6).

### 3.2 cosmic-comp — one `Shell`, a tiling tree, no offline tests

`Shell { workspaces, pending_windows, pending_layers, seats, overview_mode, resize_state, zoom_state, … }`
(`cosmic-comp/src/shell/mod.rs:278-311`) → `Workspaces { sets: IndexMap<Output, WorkspaceSet> }`
(`:832-843`) → `Workspace { tiling_layer: TilingLayout, floating_layer: FloatingLayout, focus_stack, … }`
(`shell/workspace.rs:104-121`); tiling is an `id_tree::Tree<Data>` with `Group | Mapped | Placeholder`
nodes (`shell/layout/tiling/mod.rs:42, 133-168`); elements are `CosmicMapped` wrapping
`CosmicWindow | CosmicStack` (`shell/element/mod.rs:81-114`). Held as
`Common.shell: Arc<RwLock<Shell>>` (`state.rs:224-243`). The only unit test in the tree is a KMS
scale calculation (`backend/kms/device.rs:1384-1396`). **Transfer:** the tiling `id_tree` (ids,
not pointers) is the arena idiom; the god-struct is the shape to avoid — policy state (resize,
zoom, overview) and the data model in one lock.

### 3.3 KWin and mutter — a WM model *and* a render tree, WM owns

KWin: `Workspace` owns `m_windows` and `stacking_order` ("Topmost last", `kwin/src/workspace.h:212-214, 265-268, 664-668`);
each `Window` owns a `std::unique_ptr<WindowItem>` (`window.h:2006`) created into the scene's
container (`window.cpp:198-201`); `Item` is the render node with parent/children, transform,
`boundingRect`, `scheduleRepaint` (`scene/item.h:53-168`); `WorkspaceScene` paints a
`QList<WindowItem*> stacking_order` (`workspacescene.h:112-113`). mutter: `MetaWindow` is "the
windowing API", `MetaWindowActor` "a #ClutterActor that adds a notion of a window to the Clutter
scene graph" containing a `MetaSurfaceActor` for content (`mutter/src/compositor/meta-window-actor.c:3-18`);
stacking is `MetaStack` ("windows are first sorted by layer, then by stack_position within each
layer", `core/stack.h:23-40`); the WM side tracks compositor sync explicitly
(`visible_to_compositor`, `known_to_compositor`, `core/window-private.h:445-455`). **Why two
structures:** the render tree carries per-item *repaint/damage* state and paints only what
changed; the WM model carries stacking/focus semantics that are not spatial. **Assumption that
does not transfer:** damage-driven partial repaint. An XR projection layer is re-rendered in full
every frame because the head moved (research/59 §2); the retained render tree exists to avoid
work zxr cannot avoid. **Transfer:** the *authority* split does transfer — the model decides,
rendering derives — but as a per-frame flatten (a draw list), not a second tree.

### 3.4 gamescope — flat list plus named focus roles

`steamcompmgr_win_t` in a per-Xwayland linked list (`gamescope/src/steamcompmgr_shared.hpp:68-69, 103-282`);
`focus_t { focusWindow, inputFocusWindow, overlayWindow, externalOverlayWindow, notificationWindow, overrideWindow, … }`
(`xwayland_ctx.hpp:27-48`); `paint_all` paints fixed roles in fixed order into `FrameInfo_t`
layers (`steamcompmgr.cpp:3104-3381`). **Why:** one game, one overlay, one notification — the
roles *are* the scene. **Transfer:** the role list is the spec §4 layer model in miniature
(environment / windows / top / overlay / cutout) and confirms that layers are an ordered set of
roots, not a property to sort by.

### 3.5 What no comparable models: the perception layers

The owner's observation, checked against the pins: **no open-source XR compositor or shell in
the corpus models passthrough view-correction or a hand cutout in its scene.** A grep for hand
occlusion / cutout / mask / segmentation, `XR_FB_passthrough`, passthrough layers and
environment depth over the source of StardustXR, kwin-vr, wlx-overlay-s/wayvr, zen, Simula,
xrdesktop, wxrd and Envision finds nothing. The nearest things: kwin-vr has a *blend mode* —
`passthroughEnabled: KWinVRConfig.blend` with a transparent clear colour (research/31 §2.2,
`:86`), i.e. it asks the runtime for `ALPHA_BLEND` and draws nothing where there is no window;
Monado's layer enum has `XRT_LAYER_PASSTHROUGH` (`monado/src/xrt/include/xrt/xrt_compositor.h:86`)
plumbed through the client and IPC compositors (`oxr_session_frame_end.c`, `comp_vk_client.c`,
`ipc_server_handler.c`) but with no implementation in the main compositor's render path
(`compositor/main/`, `compositor/util/` — no occurrence), and its "hand masks" are joint
bounding boxes, not mattes (research/15 §2, `tracking/hand/mercury/hg_sync.hpp:246`); WiVRn
forwards the FB passthrough extension to the Quest client, where the device does the work. The
one project that composites camera imagery *by depth* is not a compositor: openxr-steamvr-passthrough,
a SteamVR API layer with "3D stereo reconstruction to estimate projection depth" and
"compositing the passthrough based on scene depth, for applications that supply depth buffers"
(`openxr-steamvr-passthrough/readme.md:34, 37, 50`) — research/13's precedent for
view-correction, and coarsely a hand cutout (a real hand nearer than virtual content wins the
depth test), but with no matte and no hand model.

**Consequence for the scene model:** the spec §4 environment layer (view-corrected passthrough,
farthest contributor) and foreground layer (the matte + hand depth, nearest) are Mura designs
without a comparable's structure to copy ([perception-passthrough-hands.md](../architecture/perception-passthrough-hands.md)
§1 rows 18–19; research/13, /15). What §6 selects still holds for them — they are two of the
ordered roots (verdict 3), never nodes a client can parent to (perception-passthrough-hands
§1: "Neither is ever a *client*") — but their *contents* are not scene nodes at all: an
environment root holds the per-eye corrected camera image and its depth proxy, a foreground root
holds a per-eye matte + depth, both produced by the perception service and consumed by
`render` at fixed positions in the pass. The graph carries them as roots so the layer order is
one structure; it does not traverse into them. This is the one place the design rests on Mura's
own perception research rather than a compositor precedent, and it should be recorded as such.

## 4. The runtime's view — Monado

What zxr's graph must flatten into every frame: `xrt_layer_data { type, timestamp, flags, flip_y, union { projection | quad | cylinder | … }, view_count }`
(`monado/src/xrt/include/xrt/xrt_compositor.h:434-520`), accumulated per frame into
`comp_layer[]` (`compositor/util/comp_layer_accum.h:35-48`) and, across clients, into
`multi_layer_slot`s (`compositor/multi/comp_multi_private.h:53-101`); `comp_render` takes layer
arrays plus per-view pose/fov and never depth-tests between layers (`util/comp_render.h:33-46, 42-43`).
There is no graph on the runtime side; whatever shape zxr keeps is invisible past `xrEndFrame`.

## 5. Matrix

| | structure | what is a node | pose | popups/transients | policy attaches as | identity | offline-testable |
|---|---|---|---|---|---|---|---|
| motorcar | one tree | surfaces, 3D clients, displays, viewpoints, bones, pointers | parent-relative mat4 | children of the surface, +0.05 z | `WindowManager` object | pointers | no |
| wxrc | flat list | 2D views, XR-shell views | pos+rot on view | not handled (toplevels only) | in map handlers | pointers | no |
| wxrd | list + g3k planes | views ↔ `XrdWindow` | on g3k object | parent window + offset | `XrdShell` flags | pointers | no |
| StardustXR | one graph (`Spatial`) | everything incl. HMD, stage, input | parent-relative mat4; reparent semantics documented | (panel items not in pin) | **clients** over set_parent/set_transform | `Arc` + registry | **yes** (test_new, unit tests) |
| zen | scene lists + `zns_node` hit tree | boards, bounded/expansive apps, nameplates | boards: xform+size; views: x,y,z_index on board | z-gap on the board | shell grabs on boards | pointers/lists | no |
| Simula | Godot tree | windows (RigidBody), workspaces (Spatial), controllers | engine transforms | child sprites | Haskell plugin + Godot physics | Haskell maps | no |
| xrdesktop | g3k objects + hash | planes with parent/children | g3k pose; unit width | children follow parent | flags (draggable/managed/hoverable) | **lookup, never cache** | no (needs g3k) |
| kwin-vr | Qt Quick 3D tree over KWin's `Window` | output mirrors, app windows, transients, XrView | node transforms; `screen`/`vr` parent states | stacked rectangles under the app window | QML states + `SpaceAllocator3D` | KWin `Window` | no |
| wayvr | slotmaps, no graph | processes, windows, overlays | on the **overlay** | surface tree per overlay | overlay manager | **slotmap handles** | partial |
| niri | `Layout<W>` tree, layers outside | monitors, workspaces, columns, tiles | 2D, derived | via `render_popups` on the element | `Layout` methods from handlers | `W::Id` | **yes** (proptest + invariants) |
| cosmic | `Shell` + `id_tree` | workspaces, groups, mapped | 2D | in the element | inside `Shell` | tree ids | no |
| KWin / mutter | WM model + render tree | `Window`↔`WindowItem`; `MetaWindow`↔`MetaWindowActor` | 2D | items/actors | `Workspace` / `MetaStack` | pointers | no |
| gamescope | list + focus roles | windows | 2D | roles | `paint_all` order | pointers | no |
| Monado | per-frame layer arrays | layers | pose per layer | — | — | — | data structs only |

## 6. Verdicts (lineage → confirmed / refined / contradicted)

1. **One spatial graph, and it is the model — confirmed, and the 2D split is explained away.**
   Every XR implementation that grew past a prototype keeps *one* tree in which parenting is the
   operation (motorcar, StardustXR, zen's hit tree, Simula, kwin-vr). The 2D desktops' second
   structure (KWin `Item`, mutter `MetaWindowActor`) exists for damage-driven partial repaint,
   which an XR projection layer does not do — the whole layer is re-rendered every frame and
   Monado flattens it to `xrt_layer_data` regardless (§4). zxr therefore keeps **one graph as the
   authority and derives a per-frame draw list from it** (what R0's `on_tick` already does), with
   no retained render tree and no per-node damage state. wxrc's flat list is the prototype shape
   its own TODOs argue against (§1.2).
2. **What is a node — refined from the lineage.** Frames (places-model §2: runtime spaces,
   anchors, head, hands, docked output), places, windows-as-planes, their popups/transients as
   children with a z-gap (motorcar 0.05 m, zen `VIEW_Z_OFFSET_GAP`, kwin-vr's stacked
   rectangles, R0's 0.5 mm — converging), and at M2 3D client volumes (motorcar's
   `MotorcarSurfaceNode` dimensions; zen's `BOUNDED` half-size + region). **Viewpoints and input
   devices as nodes** is what motorcar, StardustXR and kwin-vr do and the 2D desktops do not; the
   places model already grounds frames in `XrSpace`s (head, hands *are* frames), so the head and
   hand poses enter the graph as frames, not as a separate structure — the lineage's choice, held
   for the lineage's reason (one space for everything the ray can hit or be cast from). This is
   a determination, not a preference: three converging XR implementations, no XR counter-example.
3. **Layers are ordered roots, not a sort key — confirmed** (spec §4; gamescope's role list;
   niri keeping layer-shell outside the layout; Monado's painter's order across layers). Each
   spec §4 layer is a root of the graph; within a layer the depth test orders.
4. **Policy attaches through a bounded mutation API on the graph — refined.** motorcar's
   `WindowManager` object, StardustXR's client-side `set_parent`/`set_transform`, xrdesktop's
   flags, kwin-vr's two-parent states and niri's `Layout` methods are five spellings of the same
   thing: policy never reaches into node internals; it calls *add / reparent / set-transform /
   set-flags / focus* on the graph. ADR 0012's amendment (in-process policy first, bounded
   `zxr_window_management` protocol after M1) is satisfied by making that API the module
   boundary now — the protocol later exposes the same verbs (StardustXR is the proof it can be a
   wire protocol). kwin-vr's `screen`/`vr` parent states are the places-model reparent verbs in
   code; `SpaceAllocator3D`'s angular allocation is a `policy` candidate research/36 already
   selected.
5. **Identity by handle, never by pointer — confirmed by two independent lessons.**
   xrdesktop: "should always be looked up instead of cached" because objects are replaced under
   you; wayvr and cosmic use slotmap / `id_tree` keys. In Rust this is an arena with generational
   keys; the `Buffer` clones and textures already key by protocol object id.
6. **Testable without a runtime — imported from niri, confirmed by StardustXR.** The graph is
   generic over its member (`trait Member` with what the graph needs: geometry size, size
   request, activation, parent relation — *not* rendering), the real member is the smithay
   `Window`, the test member is a struct, and a property sweep of the reparent verbs checks the
   places-model invariants (C1–C7; one place per window; layer roots stay roots) after every
   operation — niri's `Op` + `verify_invariants` shape. StardustXR's `test_new` constructors are
   the same idea for the spatial math.
7. **Frame-phase discipline — confirmed from the lineage.** motorcar's "the spatial configuration
   of the scene should not be modified outside of `handleFrameBegin`" becomes: graph mutations
   from protocol handlers and policy are applied before `xrLocateViews` in a tick and never
   between the draw-list flatten and `xrEndFrame`. R0 already satisfies this by construction
   (calloop dispatch and the tick are sequential on one thread); the rule needs stating so M1's
   animations (kwin-vr's follow, research/36's caps) apply in one phase.
8. **Hit testing — confirmed with a refinement from zen and StardustXR.** Nearest hit over the
   graph (motorcar `intersectWithSurfaces`, zen `zns_node_ray_cast` keeping the closer child,
   kwin-vr `rayPickAll`), then 2D local hit within the plane (zen's reverse `view_list`; smithay's
   surface tree, as R0 does), with events bubbling to the parent node when the child declines
   (zen `node.c:29-86`). wxrc's first-hit-wins is the prototype defect. StardustXR's SDF fields
   are the hit model for M2 volumes; planes stay planes.

## 7. What this selects for `scene` — recorded in specs/zxr-core.md §5a (draft 2026-09-26; normative from spec rev 3.3 the same day)

*Amendment (rev 3.3).* This section was written when every plane was drawn into zxr's projection
layer, so it describes the flatten's output as "a flat, layer-bucketed draw list". Under the
composition ruling (ADR 0006 amendment 2: 2D planes are runtime quad layers rendered only on
commit; the projection layer exists only with depth content) the per-tick output is a
**band-ordered layer list** — one quad entry per mapped 2D member, draw items only for depth
content and overflow — and dirtiness is set by the commit handler rather than found per tick.
The arenas, the fixed depth, poses-not-matrices, the one batched locate and the mutation API are
unchanged; spec §5a carries the reconciled text and the frame-path pass's endorsement
(research/65's recommendation converged on this shape independently).

The verdicts of §6 admit a general node graph (motorcar's, StardustXR's). Applying AGENTS.md
rule 6 to them — what is the cheapest structure on a battery SoC that is closest to the runtime
and still meets every requirement — narrows the shape further, because two facts remove the need
for generality:

1. **The hierarchy has fixed depth.** The places model fixes layer → frame → place → window
   (→ transient children); places do not nest; a window has one place (ADR 0016); 3D clients
   render their own interiors and zxr never models them. Nothing in the design ever exceeds
   three stored levels.
2. **Frames are not a tree among themselves.** Every frame the model names is an `XrSpace` the
   runtime locates *directly against the session's base space*, and Monado's IPC client batches
   `xrLocateSpaces` into one exchange (`monado/src/xrt/ipc/client/ipc_client_space_overseer.c:161-195`)
   where `xrLocateSpace` is one round trip each (`:135-157`). The frame layer is the list of
   spaces located per tick; the output is a flat, layer-bucketed draw list. Nothing between them
   needs recursion.

So the selected shape is **three typed arenas with generational handles** — `frames`, `places`,
`members` — plus a reusable per-tick draw scratch bucketed by layer; poses stored as `XrPosef`
(28 B; the runtime's form), matrices built per draw item per view; transient children read from
smithay's surface tree at flatten time rather than stored; reparent verbs as single index writes;
"one place per window" a type-level fact. The full shape, its budget and the ownership of pinning
(runtime/mapping service → where the anchor is; zxr → what is attached; session state → which
named place sits on which anchor UUID) are written in [specs/zxr-core.md §5a](../../specs/zxr-core.md).

What the general graphs needed generality *for* does not apply: StardustXR's clients build
arbitrary scenes inside the server's graph (zxr's clients do not parent into zxr's world);
Godot's and Bevy's trees are the engine's; motorcar put displays and input devices in the tree so
one traversal served everything (zxr's head and hands arrive as located frames, which the frame
array *is*). What the 2D desktops' second structure was for (damage-driven partial repaint) does
not apply either (§3.3, §6 verdict 1). The efficiency gain over a general arena tree at this N
(≈ 100 nodes) is not cycles — both are microseconds — but the absence of tree-maintenance code,
recursion and DFS-order invariants, and one IPC exchange for all frames instead of one per frame.

**Resolved by the shape** (were discretionary in the first draft): head/hands are frames in the
frame array, located by the runtime (motorcar / StardustXR / kwin-vr's position, for their reason
— one space for everything a ray is cast from or can hit); rendering stays out of the member
trait (the flatten is the renderer's input; niri's ~40-method trait is what putting it inside
costs); the arena is a dependency choice, budget-neutral.

**Stand-ins that remain flagged inside the draft** (AGENTS.md rule 4):

- **Popup z-gap value.** motorcar 0.05 m; zen a constant gap per z_index; kwin-vr unspecified;
  R0 0.5 mm. No comparable derives the number; the M1 depth budget (near plane 0.05 m at R0)
  fixes it from what the eye resolves at panel distance.
- **Overlay-class members** (places-model §4.3) are encoded as members of a place parented to
  VIEW rather than as an exemption from "one place per window". If the owner wants the exemption
  to be structural, the single `place` field is the one spot the shape bends.

## 8. Implemented and measured (host, 2026-09-26)

The arenas replaced R0's flat plane list in `pkgs/zxr/src/scene.rs` (spec §5a normative, rev 3.3)
on the dev workstation: Monado simulated HMD rotating, 60 Hz, RADV, isolated `XDG_RUNTIME_DIR`;
"before" is master `0a3582a` (the flat list with per-tick tree walk, texture update, buffer hold
and commit-signature hash for every surface), "after" is the same tree with the arenas. Every
number is **measured (host)** from `/proc/<pid>/stat` over 10 s and the frame journal; the
milliseconds do not transfer to the device, the structure (counts) does.

| case | before | after | what changed |
|---|---|---|---|
| 16 static `foot`, idle — the case the tick's O(surfaces) work shows in | zxr **32–34 ms/s** CPU; `buffers_released` 152 271 in 1064 frames (≈ 9 `Buffer` clones per client per tick — foot's CSD subsurfaces); 33 panel passes | zxr **19 ms/s** (−42 %); `buffers_released` **333** (one per surface per *pass*); 37 passes; `members_composed` 16/tick, `members_dirty` 37 total; 0 dirty fallbacks | non-dirty members are not walked and hold nothing (§5a) |
| 3 static `foot` | 15 ms/s | 12 ms/s | same, smaller N |
| 1 static `foot` | 12 ms/s | 10 ms/s | — |
| `vkcube` MAILBOX on RADV + foot (gate 2 on the arenas) | 298 ms/s at 13.4 k commits/s; 4 imports, 0 copies, 148 755 syncobj acquires, retention max 2 mean 2.0, 0 missed | 310 ms/s at 13.5 k commits/s; 4 imports, 0 copies, 147 899 syncobj acquires, retention max 2 mean 2.0, 0 missed; one panel pass per tick (662 in 662 ticks) | unchanged — the per-commit protocol cost is the floor (research/65 §1); the dirty flag collapses 225 commits/tick into one pass |
| gate 3 churn: `resize 900 600`, `kill -9` vkcube mid-commit with dmabufs in flight, gtk3-demo menu open then `kill -9` with the popup mapped | pass | pass — `stale_texture_draws` 0, `fences_outstanding` 0, compositor alive, 0 missed | panel targets of removed members die two ticks later (`retired_panels`), never under an in-flight pass |
| gtk3-demo menu open/close ×5 | 2 swapchain recreations + 2 full passes per open+close (exact-bounds rule; research/67) | **1 grow** on the first open, **0 destroys** across the five cycles, one pass per open and per close into the top-left sub-rect; the lazy shrink fires once, 1 s after the last close (`panel_swapchains_shrunk` 1) | grow-only with lazy shrink (§5a) |
| quiet mode (`quiet on`, 2 foot) | — | 301 quiet ticks: `members_composed` 0, no passes, zero layers, 10 ms/s | the short-circuit precedes the flatten |
| quiet mode with a *committing* RADV `vkcube` beside `xrgears` (the research/67 §6 harness, re-run on the arenas after that pass found its vkcube rows were llvmpipe/shm) | research/67 §6 on the pre-arena build: FIFO 20 → 6 ms/s; MAILBOX 290 → 488 ms/s | **the same shape**: FIFO 20 → **6** ms/s (the client idles at the fallback cadence); MAILBOX 286 → **516** ms/s at 14.3 k → 29 k commits/s — with no pass there is no held buffer, release-at-replacement hands the callback-ignoring client a free image per commit | the arenas change nothing here: the buffer-hold policy for a committing member that is *not presented* is the open item of research/67 §9 (rule 7 loop owed: mutter, KWin, weston, gamescope, smithay's defaults), and §5a's "non-dirty members hold nothing" is about members that did not commit |
| validation layer, best-practices Arm/IMG/AMD, ~700 frames with the menu cycling | 0 errors; the research/65 §0.2 warning set | **0 errors**; the same warning classes and nothing new (sub-rect passes, grow-only images) | — |
| `cargo test` | 10 | 13 — arena aliasing, pose ∘ matrix equivalence, frame→place→member composition and reparent, fan, nearest-mapped hit, band-priority budget and band-asc/nearest-last order, focus across remove, and a 4 000-step seeded sweep of the verbs against the arena invariants | — |

What the 19 ms/s that remains is: ≈ 35 state-loop wake-ups per tick with 16 clients (the
runtime's round trips plus the clients' frame-callback traffic), not scene work — the flatten's
16 pose compositions, frustum tests and one sort are below the journal's resolution. The
`xrLocateSpacesKHR` path is wired and tested for the raw call but **never taken** at this rung
(`locate_spaces_ticks` 0): the only frames are LOCAL and the view midpoint.

Housekeeping found on the way: the vulkan-tools `vkcube` used by the frame-path pass's vkcube rows (research/65 §2.3, research/67)
(`0gic378…-1.4.328.0`) is linked against an older glibc than the system Mesa and can only load
`llvmpipe`, so those runs exercised the **shm** path (Mesa's software WSI) rather than dmabuf; the
system's `vulkan-tools-1.4.357.0` sees RADV and is what the gate-2 row above used.
