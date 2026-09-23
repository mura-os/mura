# 31 — KWin VR: code study of the lightofmysoul fork

**Date:** 2026-09-23. Code-level study of Stanislav Aleksandrov's ("lightofmysoul") KWin VR work:
the draft KDE merge request [kwin!8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671)
("Draft: VR Mode"), read from the pinned local clones. Follows the doc-08 template: what it is →
code-level analysis → verdicts. This is **input** to the compositor-strategy follow-up (ADR 0013
decides); no decision is made here.

**Pinned sources** (all under `references/`):
- `kwin-vr/` — the fork, branch `vr`, HEAD `ccdd46e` "Add VR plugin" (2026-05-11 series update;
  commit authored 2026-03-26). All fork hashes below are from this pin; they churn on every rebase.
- `kwin/` — upstream KWin master 2026-09-22 (comparison baseline only; the fork's base is May-2026
  master, so the fork's own history is used to identify the patches).
- `kwin-vr-patches/` — the required out-of-tree patches (Qt 6.10.1/.2/.3, XWayland, plasma-desktop,
  plasma-workspace, kwin-6.5.4 backport).
- `monado-galaxyxr/` — the author's Monado fork, branch `galaxyxr`, HEAD `6ea9442` "galaxyxr: add
  eye tracking driven foveation support".
- `xrinfo/` — a 1,175-line OpenXR/EGL/Vulkan probe tool (`xrinfo.c`, `egl_info.c`, `vulkan_info.c`);
  diagnostic sidecar, not analyzed further.
- MR discussion quotes were pulled from the GitLab discussion feed of !8671 on 2026-09-23.

---

## 1. What it is

One person turned KWin/Plasma into a usable 3D VR desktop as an **in-process plugin** plus a short
series of core patches. MR !8671 was opened **2026-01-18** and is still **Draft** eight months later
(21 commits, 203 changed files at the current revision). The author uses it daily; testers ran it
on Rokid Max, RayNeo Air 4 Pro, HP Reverb G2, Quest 2/3 (WiVRn), Viture Luma Ultra, and an Intel
UHD 600 Celeron laptop. Distribution is a Kubuntu 26.04 [Ubuntu PPA](https://launchpad.net/~lightofmysoul/+archive/ubuntu/kwin-vr)
carrying patched KWin (`kwin-common 4:6.6.4-…+vr…`), patched Monado, and patched
`libqt6quick3dxr6 6.10.2+vr`.

**Maintainer position.** KDE has not rejected it, but both leads are hesitant, on the record:

- Vlad Zahorodnii (2026-02-03): *"The first thing is the sheer size of the merge request, it's very
  very big (it's even bigger than the amount of code for Xorg session support). We have concerns
  about the impact that it will have on maintenance burden… Output handling is a quite challenging
  thing, and we arrived at the current design after a painstaking process of dealing with crash
  reports. So we'd rather leave core things as is. Another thing with `if (isVr) { ... } else
  { ... }` is that it will lead to code bitrot… Not saying that 'we will not merge it', but I'm also
  not saying that 'we will definitely merge it'."*
- Vlad Zahorodnii (2026-03-25): *"KWin is a (2D) stacking window manager. I'm not entirely convinced
  about making it 3D, the window management bits are written with 2D in mind. For 3D, we'd rather
  integrate with something that would take care most of things for us, e.g. we could provide info
  about windows, thumbnails, perhaps a more convenient way to deal with input, and let them compose
  overlays, etc."* Also: *"we are already operating on the edge of our limits. 16K is still a lot of
  code"*, and on the pieces KDE disagrees with technically: *"for example like leasing regular
  outputs."*
- David Edmundson (2026-01-19): *"you're doing separate input, with separate rendering with separate
  window placement logic. You're not using much else kwin provides and things like placement logic
  are only getting in the way. So the key question I have before anything else is what is kwin
  providing that means there's a benefit to having it in the kwin process. We can forward windows
  buffers pretty cheaply and a path to do application level input forwarding is definitely something
  we want to pursue."*

The author's status summary (2026-04-20): the two hard core changes (move/resize, window→output)
would need rewrites nobody has designed; merging the rest *"seems … impossible right now due to the
devs position. So, we are pretty much stuck here… it seems that way it just naturally grows into a
fork on its own :/"*.

**The Galaxy XR demo.** In July 2026 the author posted
"Native Kubuntu and Steam on Galaxy XR (KWin VR + Monado)" on r/Galaxy_XR: **Kubuntu 26.04 running
natively (ARM) on the Samsung Galaxy XR** with KWin VR, his Monado `galaxyxr` fork (§5), passthrough,
and the **native Steam client**; 3DoF-only at demo time, eye tracking "on the way". His own caveat:
*"due to locked bootloader this is only possible on the very first firmware version or if you manage
to root Android."* That matches [07-device-landscape.md](07-device-landscape.md) §Galaxy XR: launch
firmware was unlockable, the 2025-12-09 update removed the unlock, and the 2026-04-08 update added a
rollback barrier. The demo path is therefore closed on current retail units.

## 2. Plugin architecture

The plugin is `references/kwin-vr/src/plugins/vr/` — one squashed commit `ccdd46e`, 151 files,
16,783 insertions (~12,000 lines of C++/QML in the plugin proper). It is a standard KWin
`PluginFactory` (`main.cpp`) loading a `KwinVr` controller (`kwinvr.cpp`) that registers a
`org.kde.kwinvr` D-Bus service, a global shortcut (Ctrl+Meta+J), and on activation spins up a
`QQmlApplicationEngine` **inside the KWin process** that loads a Qt Quick 3D XR scene.

### 2.1 Scene: one Qt Quick 3D scene, one OpenXR session

`qml/Main.qml` instantiates `qml/XrScene.qml`, an `XrView` — **Qt Quick 3D XR owns the OpenXR
session, swapchains, and frame loop**; KWin never calls OpenXR directly (only the loader-init
helper, `kwinvr.cpp:95-178`, which selects a runtime JSON via `XR_EXT_loader_init_properties`).
Notable `XrView` settings (`XrScene.qml:19-132`): `referenceSpace: ReferenceSpaceLocal`,
**`depthSubmissionEnabled: false`** (no depth layer even for reprojection),
`passthroughEnabled: KWinVRConfig.blend` with a transparent background mode, sky-blue clear color
otherwise. The whole desktop is submitted as **one projection layer** (optionally as an
`XR_EXTX_overlay` layer so it can float above another OpenXR app — Qt patch, §4).

The scene tree (`XrScene.qml:258-437`):

- `Repeater3D` over `OutputModel` → `KwinPseudoOutputMirror` planes — **physical screens** mirrored
  into 3D, placed by `SpaceAllocator3D` (free-position search, `spaceallocator3d.cpp`).
- `Repeater3D` over `KwinWindowModel` filtered by `PrimaryWindowModelFilter`
  (`kwinwaylandsurfacemodel.cpp`, `windowmodelfilter.cpp`) → `KwinApplicationWindow` per top-level
  window. Each has a QML state machine: state `"screen"` parents it to its output's pseudo-mirror at
  the window's 2D offset; state `"vr"` (when `window.vr` is set) reparents it into free 3D space
  with its own grab handle (`XrScene.qml:398-433`). **Windows literally leave their outputs.**
- `KwinApplicationWindow` = `KwinTransientWindow` recursion (`qml/KwinTransientWindow.qml`): the
  main window plus a z-stack of transient menus and a z-stack of transient normal windows, each
  offset by `ZStacker` (`zstacker.cpp`) so popups/dialogs float in front of their parents — this is
  what the popup-bounds and transientness core patches feed.

### 2.2 How a window becomes 3D: three render modes

`KwinTransientWindow.qml:66-77` switches on `KWinVRConfig.windowMode`:

1. **DecoratedSurface** (default, the real path): `qml/KwinDecoratedSurfacedWindow3D.qml` builds the
   window from parts — `KwinDecorations3D` generates decoration quads from KDecoration geometry
   (`decorationgeometry.cpp` triangulates the frame *around* the content hole;
   `kwinwindowdecoration.cpp` textures it), `kwinshadowitem.cpp`/`shadowgeometry.cpp` add the shadow
   as geometry, and `KwinSurfacedWindow3D` walks the Wayland **subsurface tree** recursively
   (`KwinWaylandSubSurface3DRecursive.qml`), one textured `Model` per `SurfaceInterface`. Every
   piece is an individually pickable 3D object.
2. **Thumbnail**: `qml/KwinWindowThumbnail3D.qml` — KWin's scripting `WindowThumbnail` renders the
   whole window (decorations, shadows, effects) offscreen with KWin's own renderer, and the result
   is one textured quad. Fallback fidelity path; "bad for performance" per its own comment.
3. **ThumbnailXrItem**: same texture drawn via a Quick 2D item (`KwinWindowThumbnailXrItem.qml`).

### 2.3 The buffer path: direct dmabuf import into Qt scene-graph textures

`kwinwaylandsurface.cpp` subscribes to `SurfaceInterface::committed`, holds the current
`GraphicsBuffer` in a `BufferRef`, and in `updatePaintNode()` (render thread) imports it via
`kwingraphicshelpers.cpp`: dmabuf → `eglCreateImageKHR` (all planes/modifiers,
`kwingraphicshelpers.cpp:149-220`) → `glEGLImageTargetTexture2DOES` → `QRhiTexture::createFrom`
→ `QSGTexture` — **zero-copy client buffer to Qt Quick 3D material**. Multi-plane YUV (NV12, P010)
gets a two-texture path with a `yuv.frag` shader fed by the surface's `colorDescription`
yuvMatrix (`KwinWaylandSurface3D.qml:113-128`); shm buffers fall back to `QImage` upload with a
`RenderBufferHolder` keeping the buffer alive across the async render phase
(`kwinwaylandsurface.cpp:112-119`). UV rectangles come from `bufferSourceBox` (viewporter-correct),
and picking is clipped to the xdg window geometry (`KwinWaylandSurface3D.qml:154-160`). The
`supportedDmabufFormats()` table (`kwingraphicshelpers.cpp:37-98`) is what the **drm format filter**
core patch feeds back to clients so they never allocate formats Qt's RHI cannot import; RGBA16
support needed an actual qtbase patch (§4).

### 2.4 The "bridge" is QML property access to KWin internals

`kwinvrbridge.cpp` itself is trivial — a singleton with `xrFailed`/`xrSessionEnded` signals. The
real bridge is that the QML scene reads KWin's live QObject model directly: `Workspace` windows,
`Window` Q_PROPERTIES (`frameGeometry`, `bufferGeometry`, `output`, `stackingOrder`, `minimized`,
`opacity`, `transientFor`, and the fork-added `vr`, `lockScreen`, `lockScreenOverlay`,
`inputMethod`, `surface`, `decoration`), `SurfaceInterface.size`/`.subSurface`,
`SubSurfaceInterface.position`. Several core patches (§3 rows 8, 12, 13) exist **only** to make
these properties QML-visible. This is the deepest privileged-API consumption imaginable — the
plugin lives inside the compositor's object model, the opposite extreme of ADR 0012's
protocol-seam approach.

### 2.5 Input: a synthetic device pointed by a 3D ray

- `kwinvrinputdevice.cpp` — `KwinVrInputDevice`, a fake `InputDevice` (keyboard+pointer) registered
  with KWin's input stack; it emits **absolute** `pointerMotionAbsolute` events in global 2D
  coordinates, plus button/axis/key events.
- Picking: `VrPicking`/`VrFocusControl` raycast the Qt Quick 3D scene; a hit on a surface model maps
  `pick.uvPosition` → surface-local → **global 2D coordinates**
  (`KwinWaylandSurface3D.qml:48-52`), which drive the fake device. Because a VR window may sit at
  2D coordinates outside every output, two core patches remove the 2D sanity checks: the **pointer
  position limiter** is replaced with identity (`kwinvr.cpp:273-275`) and the **hovered window** is
  forced from the pick result via `kwinvrhoveredwindowresolver.cpp` (bypassing stacking-order hit
  tests, which are meaningless in 3D).
- `kwintoqquick3dinputbridge.cpp` — an `InputEventFilter` at Effects priority that forwards KWin
  key/button/axis events *into* the QML scene's `QQuickDeliveryAgent`, so the plugin's own 3D UI
  (radial menu, HUD, grab interactions) is driven by real input; `kwinvrinputfilter.cpp` and
  `relativemotionblocker.cpp` gate which events reach 2D clients vs. the scene.
- **Headgaze + headscroll, keyboard-first**: the default pointer is a head ray (`VrRay`, no
  controller needed); `vrheadscroll.cpp` converts head rotation into scroll while a binding is held;
  the KCM maps keyboard keys to mouse buttons (`KeysToMouseButtonBindings.qml`,
  `kwinvrinputremap.cpp`). The author is explicit that a mouse is unsupported by design ("There is
  no such option, mouse is not supported", MR, 2026-07-06). VR controllers are optional
  (`VrInputBindings.qml` loads Qt `XrInputAction`s).
- **Focus forcing**: window menus and other internal windows are made transient
  (`d4eaef2`) so the 3D stack keeps focus and placement coherent.

### 2.6 Screens, follow mode, session lock

- **Virtual screens**: `kwinvirtualscreenhandle.cpp` creates a real KWin virtual output
  (`outputBackend()->createVirtualOutput`) with a custom modeline/scale/refresh from KCM settings —
  so ordinary Plasma (panels, wallpaper, plasmashell) has somewhere to live — while a core patch
  makes the compositor **skip rendering** virtual outputs entirely (the plugin draws their windows
  itself as 3D objects).
- **Follow mode** (`vrfollowmode.cpp`, 375 lines): moves the whole window group to keep the nearest
  window inside a configured FOV with delay/speed/world-up parameters; suppressed while grabbing,
  scrolling, hovering, or moving windows (`XrScene.qml:281-317`). Recenter/grab-all/realign
  shortcuts; an auto-realign timer at start because Monado's local origin can jump 1–4 s after
  socket-activated startup (author's MR note, 2026-01).
- **Session lock**: pure property consumption — every window/surface QML item binds
  `visible: … && (!KwinVrHelpers.screenLocked || client.lockScreen || client.lockScreenOverlay ||
  client.inputMethod)` (`KwinWaylandSurface3D.qml:44`, `KwinTransientWindow.qml:13`), replicating
  KWin's own `WindowItem` lock policy inside the 3D scene. The core patch `d8cd595` exists to
  expose exactly these three flags.

### 2.7 Ops surface: KCM, preflight, leasing

- **KCM** (`kcm/`, ~30 QML files): General (OpenXR runtime JSON, preflight toggle, threaded
  rendering, multiview, passthrough blend, ppu/distance/reset-view delay), Input (four tabs:
  key→mouse bindings, VR controller simple/analog bindings, thumbstick scroll), Headgaze setup with
  a live 3D preview, HeadScroll, Follow Mode (FOV/stop-FOV/delay/speed/world-up), Virtual Display
  (size/scale/refresh), Window Spacing, **Leasable Outputs**, Advanced.
- **`kwinvr-xrtest` preflight** (`xrtest/`): a separate `QGuiApplication` process that spins up a
  minimal `XrView`, renders 60 frames (`XrTest.qml:39-50`), and prints `OK`; `openxrtest.cpp`
  supervises it via `QProcess` and only then activates VR in-process. Crash containment evolved the
  hard way: a June-2026 tester found an xrtest SIGSEGV **took KWin itself down** through the
  finished-handler; the author's fix (PPA 6.6.4/5, *after* this pin) also added a **same-GPU
  check** — refusing to start when KWin and the OpenXR runtime render on different GPUs, the
  dual-GPU failure mode Monado hits on hybrid laptops.
- **Display leasing for AR glasses**: the KCM can mark a *desktop* connector leasable; KWin then
  offers it over `wp_drm_lease_v1` and drops it from the workspace while leased, so Monado takes the
  panel in direct mode. This is the core patch KDE explicitly disagrees with (§1).

### 2.8 Rendering and measured performance

Async render splits Qt Quick 3D XR's frame across threads (Qt patch §4;
`QT_QUICK3D_XR_ASYNC_RENDER`), enabled by the one-line KWin patch making
`EglContext::s_currentContext` `thread_local`. Multiview rendering (`OVR_multiview`) is toggleable
at runtime (Qt patch 6). Author's xrtest numbers (MR, 2026-03-27; 5376×1512@60, RayNeo Air 4 Pro,
Radeon 890M) — main-thread busy / worker:

| Case | Main busy | Worker | Main saved |
|---|---|---|---|
| Vulkan MV sync | 3,350 µs | — | — |
| **Vulkan MV async** | **300 µs** | **3,070 µs** | **91%** |
| Vulkan non-MV async | 2,200 µs | 3,440 µs | 41% |
| OpenGL MV async | 250 µs | 710 µs | 71% |

KWin itself with ~10 windows: `main≈700–990 µs, worker≈1,300–1,900 µs` per frame. Low-end floor: a
tester ran it on an Intel **UHD 600** (Celeron N4120, 12 EU) over WiVRn to a Quest 2 — 87–98% GPU,
"smooth enough to be usable" (MR, 2026-05). The missing multiview piece on AMD is a **closed Draft**
"hacked" radeonsi patch ([mesa!40629](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/40629)).

## 3. The core-patch surface

The fork = upstream master + **20 core commits** + the plugin commit. This is empirical evidence of
the minimum WM-core surface an in-process 3D mode needs from a mature 2D compositor. Table columns:
what it changes, why VR needs it, invasiveness (LOC / mechanism), and the corresponding spatial-os
authority-plane subsystem ([desktop-environment.md §3](../architecture/desktop-environment.md)) or
[component-registry](../architecture/component-registry.md) row.

| Commit | What it changes | Why VR needs it | Invasiveness | spatial-os subsystem |
|---|---|---|---|---|
| `07306c0` input: customizable hovered-window resolution | settable `HoveredWindowFinder` callback in `InputDeviceHandler` | pick result, not 2D stacking, decides hover/focus | low (26 LOC, callback + default lambda) | Input subsystem (ray routing/focus) |
| `d65d60a` input: customizable pointer position limiting | settable `PositionLimiter` in `PointerInputRedirection`; default = old confine/edge-barrier/screen-contains chain | VR windows live at 2D coords outside all outputs | low (38 LOC, callback) | Input subsystem |
| `c877221` xdgshell: customizable popup placement bounds | settable `PopupBoundsResolver` on `Workspace`; default = old clientArea | popups must be constrained to the parent's plane, not an output | low (29 LOC, callback) | Protocol server (xdg-shell popups) |
| `02db754` window: VR interactive move/resize | forks `updateInteractiveMoveResize` into Standard/Vr paths; no size limits, no electric borders/quick-tile/maximize for VR windows; non-transient VR windows can't move in 2D | 2D constraint logic is meaningless/hostile in 3D | **high** (97 LOC through `Window`'s central state machine, `isVr()` branches) | Window model / WM policy |
| `0448fdd` prevent output change during move/resize | blocks `m_output = outputAt(center)` reassignment while `vrMode && isInteractiveMoveResize`; `sendToOutput(force)` | windows "teleport" between outputs when their 2D rect crosses output geometry | **high** (touches `Window`, `WaylandWindow`, `InternalWindow`, `X11Window`; commit message lists 4 unfixed flaws) | Window model — exposes that KWin's window↔output binding is load-bearing everywhere |
| `5dd778f` window: `vr` property | `Window::setVr/isVr` + signal | the mode bit everything else keys on | trivial (24 LOC) | Window model (spatial state; our windows carry world transforms instead) |
| `7ea1940` workspace: `vrMode` state | `Workspace::vrMode` + signal | global mode bit | trivial (17 LOC) | (no analog — spatial-os has no 2D↔3D global mode) |
| `7946c59` scene: exclude VR windows from 2D rendering | `WindowItem::computeVisibility()` returns false for `isVr()` | window must not also paint on a 2D output | trivial (3 LOC) | Scene graph / composition engine |
| `d8cd595` window: expose lockScreen/lockScreenOverlay/inputMethod/surface/decoration | Q_PROPERTY plumbing only | QML lock policy + 3D model construction | trivial (21 LOC) | Lock enforcement (ADR 0007 I1–I3) + scene graph |
| `f5e8aec` surface/subcompositor properties | `SurfaceInterface.size/.subSurface`, `SubSurfaceInterface.position` as Q_PROPERTYs | recursive subsurface→3D construction | trivial (5 LOC) | Scene graph |
| `39a0dc5` window: offscreen rendering fixes | frame callbacks / `framePainted` when a window is visible but outside every output | VR windows must keep receiving frame callbacks | low (25 LOC), fixes real core assumptions | Composition engine (window-local textures) |
| `136855f` leasable-output mechanism + persistence | `leasable` flag on outputs, persisted; leasable desktop outputs offered via `wp_drm_lease_v1`, removed from workspace while leased | AR glasses are desktop connectors; Monado needs the panel in direct mode | **high** (18 files, 182 LOC across drm backend, lease protocol, output config store, kscreen integration) — the change KDE rejects on technical grounds | Output paths (spatial-os: Monado owns the HMD; lease consumed by Monado on dev profile per ADR 0006 — we never lease *desktop* outputs) |
| `3ec9802` drm: disable non-primary planes before lease | clears hw cursor etc. before handing the connector over | stale cursor plane stays visible for the lessee | low (23 LOC, drm backend) | Output paths (Monado-side concern for us) |
| `379a24d` compositor: skip virtual-output rendering | virtual outputs get no render loop | plugin renders those windows itself; avoids double work | low (9 LOC) | (no analog: our virtual outputs — spectate/mirror — are deliberately rendered, doc 17) |
| `d58ceea` pointer-lock toggle in window menu | user-facing unlock for pointer-constrained apps | games grabbing the pointer must be escapable without a real screen edge | low (49 LOC, useractions) | Input subsystem (constraint policy) |
| `15720da` `EglContext::s_currentContext` thread_local | makeCurrent from Qt's render thread | async render thread shares KWin's EGL machinery | trivial (2 LOC) but a real threading-model statement | Composition engine (renderer threading) |
| `88ef787` eglbackend: drm format filter | pluggable filter on the formats advertised via linux-dmabuf | clients must not commit formats Qt RHI can't import | low (52 LOC) | Protocol server (dmabuf feedback; our compositor negotiates its own format table) |
| `b6096fd` scripting: WindowThumbnail fixes | correct pixel ratio, damage subscription, `textureSizeLogical`/`textureFrameRect` | thumbnail render mode + previews | low (90 LOC) | Capture/preview seam (compositor-rendered previews, ADR 0012 §1) |
| `d4eaef2` InternalWindow transientness | Qt parent-child → KWin transient links for internal windows (window menu, submenus) | menus must stack on their window in 3D | low-medium (89 LOC), general improvement (also filed as kwin!8500) | Window model (internal surfaces) |
| `be38bf9` screencast: fix GL leak | resource leak fix | hygiene found along the way | trivial | Capture |

### The five the author calls "serious"

The author's own triage (MR, 2026-02-03): *"There are 5 serious changes to the KWin core and only 2
are hard"* — (1) forced hover/focus, (2) pointer beyond outputs, (3) popup placement bounds,
(4) 2D↔VR move/resize, (5) output-change prohibition; the first three *"can be covered by proper
interfaces to make it clean."* The diffs bear that out precisely:

1. **Forced hover (`07306c0`)** and **2. pointer limits (`d65d60a`)** and **3. popup bounds
   (`c877221`)** are all the same shape: KWin's hardcoded 2D policy extracted into a **settable
   std::function with the old behaviour as the default**. Each is ~30 LOC, zero behaviour change
   when unset. These are clean seams; for spatial-os they are *native properties* — our input
   subsystem routes rays and our popup placement is plane-relative from day one, no output
   rectangles exist to escape from.
2. **4. Move/resize (`02db754`)** is genuinely invasive: `Window::updateInteractiveMoveResize` is
   forked (`window.cpp:1193-1347`), `nextInteractiveMove/ResizeGeometry` grow `constrained` flags
   disabling snapping/confinement, and `finishInteractiveMoveResize` splits so VR windows skip the
   entire electric-border/quick-tile/maximize epilogue. This is the `if (isVr)` pattern Vlad calls
   a bitrot factory — the 2D constraint machine has no seam to hang a 3D policy on.
3. **5. Output-change prohibition (`0448fdd`)** is the deepest cut: `outputAt(rect.center())`
   reassignment happens in *four* window classes plus `setMoveResizeGeometry`, and the commit
   message itself lists four residual bugs (resize-end commits still switch outputs;
   maximize/restore targets the wrong output; quick-tile outlines appear on the wrong output). The
   author's own conclusion in the MR: *"Perhaps automatic output changes should be totally
   prohibited in VR mode"* / possibly *"limit VR mode to a single virtual screen."* The lesson for
   spatial-os is structural: **a 2D compositor's window↔output binding is a load-bearing invariant
   scattered through the codebase**, exactly the "no physical output a client should reason about"
   redefinition our plane model makes ([desktop-environment.md §5](../architecture/desktop-environment.md)).

## 4. Out-of-tree dependency surface

Everything in `references/kwin-vr-patches/`, with upstream status (verified 2026-09-23 where noted).
The author's own framing ([README](../../references/kwin-vr-patches/README.md)): the plugin builds
against stock Qt 6.10.2, but without patches *"a few things will be unavailable (overlay extension,
passthrough video and colors will be dull)."*

| Patch | Target | What it does | Upstream status |
|---|---|---|---|
| `qt-6.10.3/qtquick3d/0001…passthrough` | QtQuick3D XR | `passthroughEnabled` via standard env-blend modes (not just `XR_FB_passthrough`) | **approved/merged for Qt 6.11** (Reviewed-by C. Strømme, Pick-to: 6.11) |
| `qt-6.10.3/qtquick3d/0002…XR_EXTX_overlay` | QtQuick3D XR | render as OpenXR overlay above other XR apps (`QT_QUICK3D_XR_OVERLAY_PLACEMENT`) | **approved/merged for Qt 6.11** (Reviewed-by, Pick-to: 6.11) |
| `qt-6.10.3/qtquick3d/0003…SRGB swapchain` | QtQuick3D XR | GL swapchain as `GL_SRGB8_ALPHA8` — fixes washed-out colors (QTBUG-141224) | **merged** (Reviewed-by ×2) |
| `qt-6.10.3/qtquick3d/0004…async render` | QtQuick3D XR | the async render-thread split (§2.8) | **pending** (no Reviewed-by in patch header) |
| `qt-6.10.3/qtquick3d/0005…PRIMARY_MONO` | QtQuick3D XR | mono view config = 2D AR-glasses support | **pending** |
| `qt-6.10.3/qtquick3d/0006…multiview env` | QtQuick3D XR | un-cache `QT_QUICK3D_XR_DISABLE_MULTIVIEW` (runtime toggle) | Pick-to: 6.11 header, review state unclear |
| `qt-6.10.1/qtbase/rgba16.patch` (+ qtquick3d 0009) | QRhi | `QRhiTexture::RGBA16` format — high-bit-depth client buffers | **approved but merge target unclear** — author (MR, 2026-01-24): "It was approved, but it is unclear if they agree to merge it to Qt 6.11 :(" |
| `qt-6.10.1/…` extra five patches | qtbase/qtquick3d | graphics-module init, GL desktop-Linux support, buffer-manager, include fix | **obsolete** — merged by Qt 6.10.2 (the 6.10.2/6.10.3 dirs shrink to 6 patches) |
| `xwayland/0001…XYToWindow` | xorg xserver | XYToWindow returns the *Wayland-focused* window, not the window at global coords | **still open**: [xserver!2118](https://gitlab.freedesktop.org/xorg/xserver/-/merge_requests/2118) (state `opened`, checked 2026-09-23) |
| `xwayland/0001…remove pointer limits` | xorg xserver | stop clamping wayland pointer coords to root-window bounds | **still open**: [xserver!2119](https://gitlab.freedesktop.org/xorg/xserver/-/merge_requests/2119) (`opened`) |
| `plasma-desktop/transparent_desktop_edit_mode.patch`, `plasma-workspace/transparent_desktopview.patch` | Plasma | transparent desktop backgrounds (cosmetic in 3D) | not upstreamed; author: "not really important" |
| `kwin-6.5.4/` | stable KWin | `--vr` command-line startup flag + virtual-output-rendering backport | fork-only (the boot-into-VR affordance!) |
| (not in this repo) mesa radeonsi `OVR_multiview` | Mesa | multiview for the 91% main-thread saving on AMD | **closed Draft**, self-described "hacked": [mesa!40629](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/40629) |

**What a distro shipping KWin VR must carry today:** (1) the KWin fork — 20 core patches + the 16k
plugin, rebased against every KWin release (the author maintains parallel 6.5.x/6.6.x branches);
(2) a Qt patch series — currently 6 qtquick3d patches + 1 qtbase patch, tracked **per Qt point
release** (the repo has three parallel directories, 6.10.1/6.10.2/6.10.3 — this is what Qt-version
tracking costs); (3) two XWayland patches, both still unmerged upstream; (4) for VR-runtime
completeness, the author's Monado fork (§5); (5) optionally Mesa multiview and the Plasma cosmetic
patches. That is **five upstreams patched simultaneously** — strikingly like wxrc's 2019 "patch
large swaths of the ecosystem" posture ([08 §2.2](08-wxrc.md)), except most of the Qt series has an
upstream trajectory. It maps directly onto spatial-os's declared patch discipline
([repo-structure.md §Patch management](../architecture/repo-structure.md)): pinned rev + curated
series per upstream, WiVRn's `monado-rev` + `patches/monado/*.patch` pattern — the difference is
that our model plans for Monado + kernel patches only, while adopting KWin VR would add KWin, Qt,
and XWayland as permanently-patched upstreams.

## 5. The Monado `galaxyxr` fork

Branch `galaxyxr` = upstream Monado + ~45 supporting commits + three Galaxy XR commits:

- `7c90ff5` "galaxyxr: Samsung Galaxy XR driver, its dual DRM lease compositor backend and Titan
  camera passthrough" — **10,882 insertions**: `src/xrt/drivers/galaxyxr/` (hmd, ssc, profile,
  passthrough + calibration, input) and `src/xrt/compositor/main/comp_window_galaxyxr.c`
  (2,760 lines) + its dedicated pacer.
- `63ac02c` "add eye tracking support" (678 insertions).
- `6ea9442` "add eye tracking driven foveation support" (1,014 insertions,
  `galaxyxr_foveation.c`).

Supporting series worth noting: a **gfx/compute "N-layer" compositor** rework (`5fb0f04`,
`ff16acb`), compositor-side **foveation via `VK_KHR_fragment_shading_rate`** (`a979482`), generic
**camera passthrough fused into the final composition pass** (`a823a0f`), DRM-lease-device GPU
selection (`572ba05`), device-lost error plumbing, x-io Fusion AHRS + NXP mag calibration wrappers,
and new `rayneo`/`moverio` AR-glasses drivers. The driver's
[README](../../references/monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md) is an exceptional
1,200-line reverse-engineering document; facts below are from it and the code.

**What the driver actually implements** (on the SM-I610 running desktop Linux *on the headset*, the
Android rootfs mounted at `/.oldroot`):

- **Display bring-up**: the two eye panels (Sony ECX344A, 3552×3840@90) are **separate Qualcomm SDE
  DRM devices**; KWin (running under sddm) owns them and grants **one `wp_drm_lease_v1` lease per
  device**. Vulkan display WSI cannot express this display at all — each panel is fed by four
  888-px-wide SSPP plane slices — so `comp_window_galaxyxr.c` is a fully custom `comp_target`:
  UBWC (`DRM_FORMAT_MOD_QCOM_COMPRESSED`) scanout of one 7104×3840 stereo image across 4 planes ×
  2 leases, `IN_FENCE_FD` explicit sync, and a bespoke pacer built on hardware vsync timestamps
  (measured latch error ~1 µs; the README documents the downstream-kernel commit-lifetime contract
  in detail). 90/72 Hz run panel-locked; 60 Hz is broken at the display-stack level.
- **Sensors / 3DoF**: IMU via the Qualcomm SSC (ADSP) `sns_client` QMI service over `AF_QIPCRTR` —
  hand-rolled protobufs, **no Android blobs** (`galaxyxr_ssc.c`). Five LSM6DSV accel/gyro instances
  (hw 0 used), factory per-unit intrinsics from the efs profile plus the SSC's live gyro-bias
  estimator; x-io AHRS **orientation-only** fusion — **3DoF, no VIO**, though the README inventories
  all tracking-camera calibration needed for future 6DoF. Live **IPD** from the SSC `ipd` sensor
  (motorized lens travel) drives render eye separation.
- **Optics**: per-unit factory calibration from `/mnt/vendor/efs/device_profile.textproto`
  (`display_profile_v2`): per-eye per-color ray grids, module→eye extrinsics, verified FOVs —
  applied as the distortion mesh; the devkit default profile produced visible per-eye tilt,
  proving per-unit calibration is mandatory (directly supports our per-unit-calibration system
  plane root, [desktop-environment.md §6.1](../architecture/desktop-environment.md)).
- **Passthrough**: a client of Samsung's `titan-server` ISP daemon over a `SOCK_SEQPACKET` socket
  (protocol v12): stereo imx564 3000×3000@93 NV12 dma-bufs, imported once as
  `VK_FORMAT_G8_B8R8_2PLANE_420_UNORM`, sampled **directly in the final distortion-mesh pass**
  (no rectified intermediate), with curved-cover-window + KB4 camera model, capture-time IMU poses,
  and per-row rolling-shutter timewarp. Exposed as `XR_ENVIRONMENT_BLEND_MODE_ALPHA_BLEND`;
  measured steady 90.0 fps with one quad layer.
- **Eye tracking** (`63ac02c`): `XR_EXT_eye_gaze_interaction` via the OEM
  `libgalaxyxr-eyetracking` library (QNN/HVX preprocessing, four eye cameras at 30 FPS),
  demand-driven camera power, fused `display_gaze` prediction mapped to a `-Z`-forward gaze pose;
  **no per-user calibration yet**, gaze origin = head origin.
- **Foveation** (`6ea9442`): `xrt_device::get_foveation_map` draws a two-level
  `VK_KHR_fragment_shading_rate` map per frame — a 12°-radius full-rate disc *around the live
  gaze*, evaluated through the lens mapping so it is circular as perceived; 1×1 inside, 4×4
  fragments outside; incremental map updates cost 0.023 ms avg; measured GPU busy 68% vs 77%
  unfoveated. An `VK_EXT_fragment_density_map` variant was built, measured as a net loss on this
  Turnip, and removed (findings preserved on a backup branch).

**Device-side presuppositions**: root on Android (or the launch firmware's bootloader unlock, now
removed — §1); desktop Linux booted on the headset with the vendor downstream kernel; KWin under
sddm as the DRM master granting leases; `titan-server` running for cameras; read access to the efs
factory-calibration partition; evdev access to the power button; GPU governor pinning
(`gxr_perf.sh`) and rtprio limits for the compositor thread. This is a first-firmware/rooted-device
research configuration, not a shippable path — but it establishes **facts usable by
[07-device-landscape.md](07-device-landscape.md)**: Adreno 740v3 with working Turnip, dual-DRM SDE
display path proven on glass at 90 Hz, SSC sensor access without Android, per-unit efs calibration
formats decoded, all 12 cameras reachable, and eye tracking runnable from Linux via the OEM
library. For [ADR 0011](../architecture/adr/0011-eye-tracking-ipd.md): this is the first
demonstrated Linux-side Galaxy XR gaze source *and* the first working gaze→foveation consumer in
Monado — both on the Monado side of our plane boundary, exactly where ADR 0008/0011 place them.

## 6. Maintainer objections, mapped onto the spatial-os plane model

| Objection (quoted in §1) | Applies to spatial-os? |
|---|---|
| **Size/maintenance**: "bigger than the Xorg session support… I'm still puzzled how we would maintain such a plugin" | **Partially applies.** Our compositor is also one big process, and zxr-shell-v2 + composition engine is comparable engineering mass. The difference is audience: KDE must maintain VR *beside* a 2D desktop product most maintainers can't test; for spatial-os the XR compositor *is* the product — there is no second product to burden. |
| **`if (isVr)` bitrot**: special-cased modes rot when most developers never exercise them | **Dissolved by construction.** spatial-os has no 2D-desktop mode to fork against; the desktop dev window is an *output path* of the same code ([composition doc §7.1](../architecture/zxr-shell-v2-composition.md)), not a second policy regime. The fork's two "hard" patches (move/resize, window↔output) are precisely the code we never write: our window model has world transforms and no outputs. |
| **Output-model invasiveness**: "we arrived at the current design after a painstaking process… we'd rather leave core things as is"; disagreement on "leasing regular outputs" | **Dissolved.** [desktop-environment.md §5](../architecture/desktop-environment.md): no physical output a client reasons about; Monado owns the HMD display; `wp_drm_lease_v1` is consumed by Monado on the dev profile (ADR 0006), never served for *desktop* connectors by our compositor. The entire `0448fdd`/`136855f` pain is a 2D-compositor-retrofit artifact. |
| **"We'd rather provide windows/thumbnails/input and let them compose overlays"** | **This is our architecture.** Vlad's preferred integration shape — compositor exports window content + input, an external XR process composes — is the seam family ADR 0012 §4 defines from the other side (we *are* the XR-native authority plane; a 2D guest would consume our seams, not vice versa). The author's rebuttal ("this is the almost exact description of the VR plugin today") is half-true: the plugin does consume window content generically, but only by living inside KWin's process and QObject model — the un-seamed version of the idea. |
| **David's "what is KWin providing?"** | The honest answer from the code: the entire mature window model — xdg-shell/popups/subsurfaces, Xwayland, decorations, session lock, screen management, settings/i18n infra. That *is* the value, and it is exactly the mass [component-registry.md §8](../architecture/component-registry.md) says spatial-os is missing. The question cuts both ways. |
| **(implicit) crash domain**: the xrtest crash that took KWin down | **Applies to us.** One compositor process means XR-session setup and GPU work share the session's fate. The fork's mitigations (separate preflight process, same-GPU check, deferred plugin activation) are directly reusable patterns; ours additionally include session supervision (`spatial-session.target`, ADR 0007). |

## 7. Capability matrix vs. the zxr-shell-v2 plan

Honest side-by-side against [ADR 0006](../architecture/adr/0006-compositor-strategy.md) /
[zxr-shell-v2-composition.md §7](../architecture/zxr-shell-v2-composition.md).

**What KWin VR has today that our plan lacks (all of it working, with users):**

- A mature WM: correct popups/menus (transient stacks in 3D), subsurfaces, decorations and shadows
  as real geometry, window rules, session lock behaviour, Xwayland (with two patches).
- Session infrastructure: a whole Plasma session, settings KCM with ~10 pages, notifications,
  i18n, crash containment, a PPA a Kubuntu user can install today, and a `--vr` boot flag.
- Interaction maturity: headgaze + headscroll + keyboard-first input that users report as
  "surprisingly polished", follow mode, grab/recenter/realign, radial menu, VR-controller bindings.
- Hardware breadth: six+ device reports including a working low-end (UHD 600) data point, AR-glasses
  display leasing, and the Galaxy XR native demo.

**What it structurally cannot do, that zxr-shell-v2 is designed for:**

- **Client-rendered 3D content in one depth-tested space.** Every window is an opaque-ish textured
  quad in a Qt Quick 3D scene graph. There is no client colour+**depth** ingestion, no cross-client
  depth test — the Motorcar/zxr core ([08 §1.4](08-wxrc.md)) has no home here; `XrView` even sets
  `depthSubmissionEnabled: false`, so not even the compositor's own depth reaches the runtime.
- **Sort-last composition.** Our composition pass consumes raw dmabuf colour+depth per client and
  resolves visibility in a fullscreen pass; KWin VR routes every pixel through Qt's material/scene
  system (hence the RGBA16 qtbase patch and the format-filter core patch just to widen what Qt can
  ingest).
- **Owning the frame loop.** QtQuick3DXr owns the OpenXR session, swapchains, and pacing; the
  compositor cannot distribute predicted-display-time snapshots to clients or implement our
  deadline/placeholder scheduling rule (composition §7.4). We run the `xrWaitFrame` loop directly.
- **Boot-into-XR appliance.** It is a plugin inside a full desktop session by design (Plasma is its
  UI); our appliance profile boots the compositor as the session (ADR 0007).
- **Monado-side perception layers.** Passthrough is a blend-mode toggle passed to the runtime;
  there is no view-corrected passthrough/hand-cutout layer pipeline (ADR 0008) — though the
  author's *Monado* fork (§5) implements exactly such compositor-side passthrough for one device,
  on the correct side of our plane boundary.

**Where the topology is the SAME** — and this is the strongest external validation our plan has
received: **one compositor process composes every window and submits ONE OpenXR projection layer**
(plus optional overlay placement), exactly ADR 0006's shape; input is **ray → plane intersection →
window-local 2D events through a synthetic seat device**, exactly composition §7.3's 2D-tier
design; client buffers reach the 3D scene **zero-copy via dmabuf import**; and the lock is enforced
by composition policy (don't draw non-lock surfaces), the same mechanism as our
lock-as-composition-mode. KWin VR is proof that this topology yields a *usable daily-driver*
desktop at sub-millisecond main-thread cost on mid-range hardware.

**Where it differs:** Qt Quick 3D scene graph vs. our explicit sort-last colour+depth pass;
QtQuick3DXr-owned session vs. our direct loop; retrofit of a 2D window/output model (the two "hard"
patches) vs. a native spatial window model; in-process QML-over-internals bridge vs. ADR 0012's
protocol seams.

## 8. Verdict-input summary (for ADR 0013 — no decision here)

1. **The minimum core surface is small and now enumerated.** 20 commits; three of the five
   "serious" ones are ~30-LOC settable-callback seams (hover resolution, pointer limits, popup
   bounds); only move/resize (`02db754`) and window↔output (`0448fdd`) are invasive, and both
   exist *only because* a 2D compositor binds windows to outputs — a problem our architecture
   deletes rather than patches.
2. **The topology is validated.** One process, one projection layer, ray→plane→wl_pointer input,
   dmabuf-zero-copy windows, lock-as-composition-policy: a shipping implementation of ADR 0006's
   shape, running daily on real hardware down to an Intel UHD 600.
3. **KDE will likely not merge it.** Both maintainers' quoted positions (size, bitrot, output
   model) plus the author's own "naturally grows into a fork" reading mean KWin VR should be
   treated as a *fork/reference*, not an upstream feature spatial-os could ride.
4. **Vlad's preferred integration shape — "provide windows, thumbnails, input; let them compose" —
   is a description of spatial-os's authority-plane + seam architecture** (ADR 0012), argued
   independently by the KWin maintainer from the opposite direction.
5. **The patch-carry cost of adopting KWin VR is five upstreams** (KWin fork, Qt per-point-release
   series, XWayland ×2 unmerged, Monado fork, optional Mesa/Plasma) — wxrc-2019-like breadth,
   though with a real upstream trajectory for the Qt half (passthrough/overlay/sRGB approved for
   6.11; RGBA16 approved-but-unscheduled; async render + mono pending; XWayland MRs open; radeonsi
   multiview closed-draft).
6. **It cannot become our 3D tier.** No client depth path, no sort-last composition, no owned frame
   loop — zxr-shell-v2's reason to exist is precisely what the Qt Quick 3D XR substrate forecloses.
7. **It is the best available session alternative for 2D-in-VR on KDE-class maturity** — stronger
   than WayVR on WM completeness (popups, Xwayland, lock, settings), weaker on process isolation —
   and is packageable today from the PPA recipe (KWin fork + Qt patches + Monado).
8. **The Monado `galaxyxr` fork is independently valuable regardless of the KWin verdict**: a
   dual-DRM-lease direct-mode backend, SSC sensor access without Android blobs, per-unit efs
   calibration decoding, titan-server passthrough fused into composition, Linux-side eye tracking,
   and gaze-driven FSR foveation — all Monado-side, all on our side of the ADR 0008 plane boundary,
   and all directly citable by doc 07 (Galaxy XR) and ADR 0011 (eye tracking).
9. **Reusable patterns regardless of adoption**: the xrtest preflight-in-a-separate-process with
   same-GPU check (crash containment for in-process XR init); the drm-format-filter idea (advertise
   only importable formats); decoration/shadow-as-geometry construction; the follow-mode/headgaze
   interaction design; the KCM's settings taxonomy as a checklist for our HMD settings API
   (ADR 0012 §4 item 5).
10. **One process-model caution transfers**: the xrtest crash cascading into KWin shows what
    sharing a process with XR session setup costs; our supervision story (ADR 0007) plus the same
    preflight pattern is the mitigation.
