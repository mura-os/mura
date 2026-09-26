# 59 — The XR compositor's architecture from comparables: the lineage tested mechanism by mechanism

**Research date:** 2026-09-26. **Question:** zxr's base and shape were decided (ADR 0006,
[research/39](39-compositor-base-landscape.md), composition §7) before AGENTS rules 6–8, and no
specification of zxr *as a program* exists. Before one is written, every mechanism it must fix is
re-derived from source in the rule-7 form — what each comparable chose, **why** (its own words),
on which assumptions, whether they transfer, what adopting it costs — **with the lineage read
first**: zxr descends from **motorcar** (Reiling's thesis and compositor, 2014) and **wxrc** (the
owner's commissioned wlroots compositor with Drew DeVault and Simon Ser, 2019–21; `zxr-shell-v1`),
which the owner holds conceptually more correct than the alternatives. Each mechanism closes with
one of three verdicts: **lineage confirmed** (the evidence agrees), **lineage refined** (a
comparable improved the mechanism without changing the model), **lineage contradicted** (a
Phase-C item for the owner, evidence laid out). Nothing here re-opens "motorcar/wxrc vs
server-side rendering vs overlay compositors" as a question.
**Method:** pinned clones (`references/`, `path:line`): the lineage (`motorcar`, `motorcar-thesis`,
`wxrc`), the XR compositors (`wayvr` = wlx-overlay-s lineage, `stardustxr-server`, `simula`,
`xrdesktop`/`wxrd`, `gamescope`), the 2D bases (`smithay`, `niri`, `cosmic-comp`, `wlroots`,
`weston`, `mutter`, `kwin`, `cage`, `swaylock`, `gnome-shell`, `phosh`), the runtime (`monado`,
`openxrs`, `openxr-docs`, `wivrn`, `alvr`). Budgets measured on this host (§13). [external] where
no clone exists. **Budget impact** (overview invariant 9): a research document; the program it
informs is fenced in §13 and in [specs/zxr-core.md](../../specs/zxr-core.md).

## 0. The lineage, stated from its sources

**motorcar's model** (the design centre): clients keep rendering into ordinary Wayland buffers;
the compositor never receives geometry; consistency comes from synchronised view/projection
matrices and **depth-buffer compositing** of the clients' images into one scene. The thesis
argues it against the alternative directly: sending geometry to the compositor "would either
require that a full featured 3D graphics API be presented over the display server protocol … or
it would seriously limit the flexibility clients have", whereas "having the clients send their
depth buffers to the compositor and composite the geometry in 2D in the same way that it is done
in the traditional 3D graphics pipeline … allows clients … even … GPU accelerated ray tracing,
provided that they fill the depth buffer correctly … keeps the display server protocol extensions
simple" (`motorcar-thesis/chapters/Design.tex:80-82`). Wayland rather than X because the
compositor *is* the display server, so 3D-transformed input can be routed correctly
(`TechnicalBackground.tex:97-99`). Its protocol: `motorcar_surface` (clipping mode, depth
compositing), `motorcar_viewpoint` (split view/projection matrices, a colour and a depth
viewport — depth packed into colour because "EGL does not support a color mode that includes
depth", `motorcar/src/protocol/motorcar.xml:140-143`), `motorcar_six_dof_pointer`. Assumptions of
its time: OpenGL clients, no dmabuf modifiers, no explicit sync, no OpenXR (Rift DK1 SDK + Razer
Hydra, `Implementation.tex:140`), one double-wide stereo buffer.

**wxrc** realised the same split on wlroots + OpenXR: `zxr_shell_v1.get_xr_surface`, `zxr_view_v1`
(N views), `zxr_surface_view_v1.mvp_matrix`, `zxr_composite_buffer_v1` with typed pixel/depth
buffers ("It is a protocol error to attach a 2D buffer to an XR surface",
`wxrc/protocol/zxr-shell-unstable-v1.xml:104`). Single-threaded loop: `xrWaitFrame` →
`xrPollEvent` → `wl_event_loop_dispatch(…, 1)` → render → `xrEndFrame` → matrices + `frame_done`
to clients (`wxrc/src/main.c:477-531`). GLES2 through wlroots + `XR_MNDX_egl_enable`; the depth
buffer type is on the wire but the renderer draws a flat quad — depth composition was never
wired (`wxrc/src/render.c:331-355`). Assumptions: wlroots 0.8, implicit sync, `glFlush`.

**What research/08 Part 3 recorded of the derivation** motorcar → v1 → v2 (N views kept; typed
buffers kept; folded MVP dropped as a regression; input, clipping, size negotiation and frame
timing resurrected; atomic frame snapshot added) is supported by the sources, with two
corrections: research/10's "motorcar hardcodes a viewpoint per eye" overstates it — the example
is stereo, the protocol allows N (`Implementation.tex:28`); and research/08's Mesa depth-patch
provenance was already withdrawn by research/09 (`09-wxrc-ecosystem-gap-2026.md:197-207`).

Licences: motorcar BSD-2-Clause, wxrc MIT.

## 1. Process and thread model

**Lineage.** motorcar: Qt's event loop (QtWayland), frames driven as draw → end → begin-update →
send matrices and frame callbacks, i.e. one loop, one thread. wxrc: one thread, the OpenXR loop
owns it, Wayland dispatched with a 1 ms timeout inside each frame (`main.c:477-531`).
**XR comparables.** wayvr — the one shipping Rust/smithay XR compositor — does exactly wxrc's
shape in production: one thread, `frame_wait.wait()` then `display.dispatch_clients` +
`flush_clients` once per frame (`wayvr/wayvr/src/backend/openxr/mod.rs:166-251`,
`backend/wayvr/client.rs:210-219`). gamescope is the counter-shape: a Wayland thread
(`wlserver_run`, `main.cpp:1112`), a compositor thread (`steamcompmgr`), and a backend thread
(`gamescope-vrflip`: `WaitFrameSync` → mark vblank → `nudge_steamcompmgr`,
`OpenVRBackend.cpp:482-526`), joined by `wlserver_lock` (`wlserver.cpp:2368-2390`). stardustxr:
Bevy + tokio, `xrWaitFrame` as a schedule stage (`main.rs:517-614`). xrdesktop/wxrd: GLib loop +
`wl_event_loop_dispatch` under a render mutex (`wxrd/src/main.c:848-865`).
**2D bases.** niri and cosmic-comp: one calloop on the main thread (`niri/src/main.rs:175,278`;
`cosmic-comp/src/lib.rs:168`), cosmic adding a calloop **per output** for KMS present
(`backend/kms/surface/mod.rs:260-277`). KWin: a `RenderLoop` per output and a realtime
`DrmCommitThread` because "the kernel rejects commits that happen during vblank"
(`drm_commit_thread.cpp:371-372`). mutter: a KMS thread beside the Clutter frame clock
(`meta-kms.c:45-64`). The pattern in every mature 2D compositor: **one Wayland/state loop,
with the thing that blocks on the display moved to its own thread.**
**The runtime's contract.** `xrWaitFrame` blocks ("throttles the application frame loop … block
until the beginning of the next frame interval", `openxr-docs/…/rendering.adoc:785-803`), must be
externally synchronised but "must: be callable from any thread, including a different thread
than xrBeginFrame/xrEndFrame" (`:818-822`); "a pipelined system may call xrWaitFrame on a
separate thread from xrBeginFrame and xrEndFrame" (`:1008-1014`). Monado's own compositor runs a
render thread with raised priority beside its IPC threads (`comp_multi_system.c:47-49,529-533`).
**Transfer.** The lineage's single loop is what the only shipping Rust/smithay XR compositor
does; the 2D bases and the runtime both point at moving the blocking wait off the state loop.
smithay is built around owning `calloop`; wxrc's "dispatch Wayland with a timeout from inside the
XR loop" inverts that. **Verdict: lineage refined — and the refinement forks (§15 Q1):** (a) the
wxrc/wayvr shape, the XR loop owns the thread and dispatches calloop with a bounded timeout each
frame; (b) the gamescope/KWin shape, calloop owns the thread and a dedicated thread blocks in
`xrWaitFrame` and posts the frame state into the loop. Both have comparables with reasons.

## 2. Frame pacing

**Lineage.** motorcar's stated policy: "draw the scene graph … with the current data at the
beginning of the frame … then send the new matrices to the clients" — never wait for clients, so
no client can drop the compositor's rate; the alternative (wait for all 3D clients, lower
motion-to-photon) is discussed and rejected (`Implementation.tex:99-101`). wxrc: `frame_done`
after `xrEndFrame` (`main.c:524-531`), with TODOs to use `predictedDisplayTime`.
**Comparables.** wayvr sends frame callbacks after the overlay's GPU render, one retained buffer,
a full CPU fence wait before `xrEndFrame` (`overlays/wayvr.rs:722-745`, `graphics/mod.rs:689-699`).
niri/cosmic send **at most one callback per refresh** — "to prevent clients busy-looping with
frame callbacks that result in empty damage" (`niri/src/niri.rs:485-487`) — sequenced on submit,
not on vblank (`:492-495`). weston reserves a `repaint-window` because repainting "immediately
after presentation leads to … forcing an extra frame of latency" (`compositor.c:4287-4290`); KWin
estimates next-frame cost (`RenderJournal`) plus a shrinking safety margin (`renderloop.cpp:61`,
`drm_commit_thread.cpp:175-177`); mutter's frame clock enforces a submit deadline before
presentation (`clutter-frame-clock.c:139-141`). Monado, for its part, releases an app from
`xrWaitFrame` at `wake_up = predicted_display − (app + compositor time)` and grows the app period
when the app misses (`u_pacing_app.c:505-524, 291-318`); `predictedDisplayTime` is "the midpoint
of the interval during which the frame is displayed" (`rendering.adoc:891-892`).
**Transfer.** The lineage's rule (draw what is there, then tell clients, never block on them) is
every 2D compositor's rule and Monado's expectation of its clients. What the lineage lacked is
now given by the runtime: the frame callback's timestamp and the clients' target become
`predictedDisplayTime` of the *next* frame; the once-per-refresh throttle bounds a fast client;
bounded buffering is the release-point contract (§5). **Verdict: lineage confirmed, refined by
the runtime's prediction and the niri/cosmic throttle.**

## 3. Renderer and device ownership; who composites depth

**Lineage.** motorcar: the compositor composites depth from client depth buffers (packed into
colour) with stencil clipping (`Implementation.tex:37-47,110-130`). wxrc: GLES2 via wlroots,
`XR_KHR_opengl_es_enable` + `XR_MNDX_egl_enable` sharing wlroots' EGL (`backend.c:83-84,281-287`);
depth unused.
**Comparables.** Runtime-created device: wayvr `create_vulkan_instance` / `vulkan_graphics_device`
/ `create_vulkan_device` under `XR_KHR_vulkan_enable2`, wrapped in vulkano (`graphics/mod.rs:214-357`);
xrdesktop's gulkan the same via gxr (`gxr-context.c:1059-1072,1203-1274`). gamescope owns its
device and presents to OpenVR as an **overlay texture**, not a projection layer
(`OpenVRBackend.cpp:2412-2427`) — as does wayvr (quad/cylinder overlays sorted by the runtime,
`openxr/mod.rs:443-467`). stardustxr composites depth in a Bevy scene. Monado requires of a
`vulkan_enable2` app: external memory/fence/semaphore capabilities on the instance, dedicated
allocation + external memory **fd** on the device (`oxr_vulkan.c:107-132`), binds the app's chosen
queue family (`oxr_session_gfx_vk.c:142-156`), and hands swapchain images over as exported fds
(`ipc_client_compositor.c:268-306`). openxrs exposes the three calls and returns `VkImage`
handles (`openxrs/openxr/src/instance.rs:227-253,304+,344+`, `graphics/vulkan.rs:15-95`).
**Does the runtime composite depth?** No. The spec: layers "must: be drawn with a 'painter's
algorithm,' … whether or not the new layers are virtually closer to the viewer"
(`rendering.adoc:1143-1147`); `XR_KHR_composition_layer_depth` "does not affect the order of
layer composition" (`khr_composition_layer_depth.adoc:23-26`). Monado's renderer: "the graphics
version disregards depth data, while the compute shader does use it somewhat" — for reprojection
(`monado/…/compositor/util/comp_render.h:42-43`); `XR_FB_composition_layer_depth_test` is parsed
and never consumed.
**Transfer.** The thesis's central claim — the compositor must composite depth itself because the
display stack will not — is *more* true under OpenXR than it was under a bare HMD SDK. The
overlay compositors (wayvr, gamescope-VR) are a different product: they do not need depth
because the runtime sorts their quads. **Verdict: lineage confirmed** on the model; **refined**
on the realisation — Vulkan on a runtime-created device (wayvr, gxr; ADR 0006), one stereo
projection layer submitted (depth to Monado optional, reprojection only), inter-client depth
composed in zxr (M2's job; the 2D tier sorts planes and writes depth so M2 slots in).

## 4. Client buffer import and the format/modifier table

**Lineage.** motorcar: EGL image import via QtWayland; wxrc: `wlr_gles2_texture_get_attribs`
(`render.c`). Neither had modifiers or dmabuf feedback.
**Comparables.** wayvr builds `DmabufFeedback` from Vulkan DRM-format-modifier queries, v4 with
fallback (`backend/wayvr/mod.rs:241-260`) and imports into vulkano images
(`image_importer.rs:102-136`). gamescope derives formats from wlroots' set and gates modifiers
on the device (`rendervulkan.cpp:131-132,464`). wlroots' Vulkan renderer queries
`VkDrmFormatModifierProperties` per format (`render/vulkan/pixel_format.c`). smithay:
`backend::vulkan` offers instance/physical-device utilities and modifier *queries* — "does not
provide abstractions for logical devices, rendering or memory allocation"
(`smithay/src/backend/vulkan/mod.rs:6-7`) — and `wayland::dmabuf` lets the compositor build the
feedback table (`DmabufFeedbackBuilder`, `dmabuf/mod.rs:86-132`); dmabuf → `VkImage` import is
the compositor's own code. wxrd's shm path is a CPU memcpy with a TODO (`wxrd-renderer.c:463-467`)
— the copy to count and forbid.
**Transfer.** The table comes from the device the runtime created (wayvr's exact shape); import
is ash code zxr writes; shm gets one upload, dmabuf none. **Verdict: refined** (the mechanism did
not exist for the lineage; the comparables converge).

## 5. Explicit sync

**Lineage.** None — implicit sync and `glFlush`. **Comparables.** gamescope implements
`wp_linux_drm_syncobj_v1` ("Using explicit sync when available", `wlserver.cpp:2243-2246`),
timelines ↔ sync files through DRM and `vkImportSemaphoreFdKHR` (`Timeline.cpp`,
`rendervulkan.cpp:1517-1535`). wayvr has none (CPU fence wait each frame). smithay's
`wayland::drm_syncobj`: acquire is a **CPU-side blocker** on a syncobj eventfd — "the
implementation here assumes acquire fences are already signalled when the surface transaction is
ready. Use `DrmSyncPointBlocker`" (`drm_syncobj/mod.rs:6-7`, `sync_point.rs:240-254`); release
points are signalled when the compositor drops its last `Buffer` reference
(`renderer/utils/wayland.rs:68-77`). wlroots blocks the commit until the fence materialises
(`wlr_linux_drm_syncobj_v1.c:236-238`); mutter imports COGL's latest sync fd into the release
point (`meta-wayland-buffer.c:669-683`); KWin signals on the last `SyncObjReleasePoint` drop
(`syncobjtimeline.cpp:104-109`). stardustxr imports timeline syncobjs into wgpu semaphores for
its dmatex path (`dmatex.rs:149-177`); the perception intake harness in this tree found the same
release-obligation subtlety (`specs/perception-intake.md` rev 3 §4.4).
**Transfer.** For a compositor whose GPU work is a Vulkan queue: acquire may start as smithay's
CPU blocker (correct, simple) and become a GPU semaphore wait (`export_sync_file` → import) as
an optimisation; release must be signalled from the compositor's *GPU completion*, i.e. a sync
file exported from the queue submission that read the buffer (gamescope/mutter shape) — not on
CPU-side drop — or the client re-renders into a buffer still being sampled. **Verdict: refined**
(new mechanism; the comparables converge; the perception harness's finding transfers).

## 6. Input

**Lineage.** motorcar: `motorcar_six_dof_pointer` (enter/leave/motion/button with position +
orientation, surface-local) for 3D clients; ray→plane → ordinary pointer events for 2D
(`motorcar.xml:159-226`; `Design.tex:50`); its reason: "3D equivalent of mouse events"
(`Implementation.tex:50`). wxrc: pose ray → plane hit → `wlr_seat_pointer_notify_*`
(`input.c:135-228`); no 3D input events in v1.
**Comparables.** wayvr: rays → nearest plane/cylinder hit → hosted apps via smithay seat, host
screens via virtual pointer (`backend/input.rs:762-829`). xrdesktop synthesises cursor/click from
controller rays (`xrd-input-synth.c`). stardustxr: spatial input nodes, no `wl_pointer` (a
different model). gamescope: SDL/libinput → wlserver seat. Research/42 fixed Mura's input floor
(head-aim + one button; dwell) and research/36 the interaction patterns.
**Transfer.** Ray-to-`wl_pointer` for the 2D tier is universal among the XR compositors; 6DoF
events for 3D clients are motorcar's and `zxr-shell-v2` restores them (`zxr_pointer_6dof_v2`,
`zxr_ray_v2`). **Verdict: lineage confirmed.**

## 7. Placement and scene state

**Lineage.** motorcar: a scene graph — `SceneGraphNode`, `PhysicalNode` vs `VirtualNode` so
hardware nodes cannot parent under virtual content, `Display`/`ViewPoint`, `WindowManager` for
map/layout (`scenegraphnode.h:55-70`, `Implementation.tex:80-87`); 2D surfaces as textured
quads fanned out on map (`windowmanager.cpp:147-159`); no persistence. wxrc: a flat `wl_list` of
views with position/rotation, spawn at `z = −2` in front of the view (`view.h:30-38`,
`xdg-shell.c:83-92`).
**Comparables.** wayvr: a flat overlay list with FollowHead/Hand/Anchored/Floating transforms,
persisted to json5 (`windowing/window.rs:158-165`, `config.rs:297-315`). stardustxr and xrdesktop:
scene graphs (spatial tree; g3k). niri/cosmic: layout state in memory, config declarative.
Mura's [places-model.md](../architecture/places-model.md) already fixes a **frame graph** (map /
LOCAL·STAGE / VIEW / hand / docked / shared) with attachment constraints and intra-place layouts,
and D7's settings store owns persisted preferences.
**Transfer.** motorcar's physical/virtual node split is the places model's frame graph in an
earlier vocabulary; the flat list (wxrc, wayvr) suffices for an overlay but not for frames with
different anchors. **Verdict: lineage confirmed (motorcar's graph), with persistence delegated to
D7's store rather than a compositor-private file (wayvr's json5).**

## 8. The windowed development backend

**Lineage.** motorcar ran nested/X/KMS through Qt; wxrc needed a runtime. **Comparables.**
stardustxr `--force-flatscreen` (winit window, no XR plugins, `main.rs:373-440`); wayvr `uidev`
(a separate winit+vulkano GUI testbed) and `--headless`; gamescope's nested SDL backend;
niri/cosmic winit backends — GLES-coupled in smithay (research/39's friction). **The runtime
side changes the question:** Monado's `simulated` HMD driver plus its desktop window targets
(`comp_window_wayland.c`, `comp_window_xcb.c`) run a complete OpenXR session in a window with
canned head motion (`SIMULATED_ENABLE`, `SIMULATED_ROTATE`, `simulated_hmd.c:112-135,221-235`) at
a fake 60 Hz display timing ("The display timing code hasn't been tested on Wayland and may be
broken", `comp_window_wayland.c:105-106`; `XRT_COMPOSITOR_DEFAULT_FRAMERATE` 60). This is
exactly what `pkgs/dev-session` already runs.
**Transfer.** The XR path needs **no compositor-side windowed backend at all**: zxr is an OpenXR
client of a Monado that draws into a window. A flat non-XR mode (stardust's `-f`) is a debugging
convenience, not R0's. This answers research/39's open output (winit vs direct ash swapchain):
neither, for R0. **Verdict: refined — the runtime provides the dev backend; composition §7.1's
"windowed desktop output mode (mouse-driven camera)" is satisfied by Monado's simulated HMD and
window, not by a zxr backend.**

## 9. Xwayland

**Lineage.** motorcar: none. wxrc: none; wxrd (its descendant) uses `wlr_xwayland` in-process
(`wxrd/src/main.c:816`). **Comparables.** niri: `xwayland-satellite` "because X11 is very cursed …
giving niri normal Wayland windows to manage" (`niri/docs/wiki/Xwayland.md:27-28`); its FAQ's
reasons: implementing an X11 WM is "quite a bit of work", "niri doesn't have a good global
coordinate system required by X11", "an endless stream of X11 bugs" (`FAQ.md:64-79`). wayvr —
the XR case — spawns `xwayland-satellite` against its socket (`backend/wayvr/client.rs:101-108`).
cosmic-comp: in-process smithay `X11Wm` (`cosmic-comp/src/xwayland.rs:111-175`); smithay's docs:
"You need to … play the role of an X11 Window Manager" (`xwayland/mod.rs:11-13`). gamescope is
Xwayland-centric by design (`README.md:5-11`). sway/wlroots: built-in XWM.
**Transfer.** niri's decisive reason — no global 2D coordinate system for X11 to live in — holds
*a fortiori* for a compositor whose windows are planes in a 3D frame graph; and the one XR
comparable chose satellite for the same reason. cosmic's in-process choice buys control a 2D
desktop wants and zxr does not (X11 windows are ordinary planes). **Verdict: determination
(converging: niri + wayvr, reasons transfer): xwayland-satellite for R0 gate 4; smithay `X11Wm`
recorded as the fallback if satellite's constraints bite** — this closes research/39's "R0
output" by evidence rather than by the spike.

## 10. The restricted (greeter/lock) mode in the same binary

**Lineage.** None. **Comparables.** gnome-shell: one binary, `--mode=gdm` selects a `SessionMode`
whose base `restrictive` has `hasWindows: false, hasOverview: false, allowSettings: false …`
(`gnome-shell/js/ui/sessionMode.js:21-99`, `src/main.c:519-637`) — feature flags, the listening
socket stays. cage: one client, but `wl_display_add_socket_auto` still (`cage.c:664`); kiosk is
enforced by spawning one child and exiting on its death (`cage.c:199-216`). KWin: the lock
greeter is a **separate process** given a **private socketpair**, not the public socket
(`wayland_server.cpp:616-675`), and a wrapper restarts KWin while keeping the socket fd
(`kwin_wrapper.cpp:10-18`). swaylock: an `ext-session-lock-v1` client; the protocol's own
warning: "If the client dies while the session is locked the session remains locked, possibly
permanently" (`ext-session-lock-v1.xml:25-36`). phosh: lock screen in-process.
**Transfer.** ADR 0007 chose the GNOME shape (same binary, restricted mode, PAM out of process —
`mura-authd`, landed) plus one hardening no comparable has: **no Wayland listening socket in
greeter mode**. KWin's socketpair discipline is the comparable for the *helper* channel
(`mura-authd` already uses a seqpacket pair). **Verdict: not the lineage's; ADR 0007 stands
(GNOME precedent, KWin socketpair for helpers); the no-socket rule is Mura's, already ruled.**

## 11. Crash, restart, readiness

**Lineage.** Silent. **Comparables.** niri and cosmic-comp: `Type=notify`, `sd_notify(READY=1)`
(`niri/resources/niri.service:13-14`, `cosmic-comp/data/cosmic-comp.service:9`, `systemd.rs:32`);
cosmic `Restart=never`. weston: a `systemd-notify` plugin. KWin: a wrapper restarts on non-zero
exit and preserves the socket fd so *new* clients connect — in-flight clients still die
(`kwin_wrapper.cpp:13-15`). Nobody has client survival across a compositor crash. Mura's D4
already fixed `RestartMode=direct` in the same logind session and `sd_notify` after the socket is
bound (session-bootstrap rev 3).
**Transfer.** Converging; nothing to add beyond what D4 landed. **Verdict: confirmed by the
2D comparables; zxr implements session-bootstrap's contract as written.**

## 12. Instrumentation

**Comparables.** Monado: `u_metrics` protobuf when `XRT_METRICS_FILE` is set (`u_metrics.c:28-29`),
Tracy plots "App CPU/Draw/GPU (ms)", "late" spans (`u_pacing_app.c:412-461`). niri/cosmic:
`tracy-client`/`profiling` spans (`niri/Cargo.toml:139-144`, `cosmic-comp/Cargo.toml:78`). KWin:
`RenderJournal` (next-frame cost estimate) and `KWIN_PERF_FTRACE`. mutter: `COGL_TRACE_*`.
gamescope: `--stats-path`. weston: timeline points. **Transfer.** R0's gates need four numbers:
compositor GPU time per frame (Vulkan timestamps), missed `xrWaitFrame` deadlines
(`predictedDisplayTime` vs actual `xrEndFrame`), per-buffer retention (commit → release-point
signal), and CPU copies on the client-buffer path (a counter that must read 0). **Verdict:
determination — tracing spans + a frame journal, the union of what the comparables measure.**

## 13. Budget (rule 6), measured

Nested on this host (Wayland session, RADV on Strix Halo; nixpkgs builds, stripped):

| compositor | binary | RSS (nested, idle) | threads | notes |
|---|---|---|---|---|
| niri (Rust, smithay) | 35.5 MB | 30 MB | 1 | calloop only; `Cargo.toml`: no tokio, no `backend_vulkan`/`xwayland` features |
| cosmic-comp (Rust, smithay) | 34.7 MB | 115 MB | 28 | per-output KMS threads + winit nested; `backend_vulkan`, `xwayland` |
| gamescope (C++) | 4.8 MB | 144 MB | 17 | Vulkan + own Xwayland; three loops + helpers |
| mura-settingsd (this tree, Rust) | 1.8 MB | 3.5 MB | 4 | for scale: zbus + serde |

wayvr and stardustxr-server are not in nixpkgs at the pin and were not built (their dependency
lists: wayvr — smithay frontend-only features, vulkano, ash, openxr, smol; stardustxr — Bevy, tokio
multi-thread, wgpu; `wayvr/wayvr/Cargo.toml`, `stardustxr-server/Cargo.toml`). Reading: a
smithay compositor is ~35 MB of stripped Rust before any renderer of its own (LTO and `opt-level
= "s"` are not what nixpkgs applies; the tree's crates use them); resident cost is dominated by
the GPU stack, not the compositor; thread count is a design choice (niri 1, cosmic 28). The
fence the spec proposes for R0: binary ≤ the niri class, RSS ≤ 2× niri nested with one client,
threads ≤ 4 (state loop, XR wait, and whatever the ruling in §15 Q1 adds).

## 14. The base, re-checked with reasons

**Lineage.** motorcar chose QtWayland because it "handles almost all of the behavior needed to
correctly interact with 2D clients" and isolates the 3D work (`Implementation.tex:72-74`). wxrc
chose wlroots + C in 2019 — the era's only serious library — and the pin (`wlroots >=0.8.1,<0.9`,
`meson.build:48`) no longer preprocesses against 0.19 (`09-wxrc-ecosystem-gap-2026.md:459-478`):
the API churn of a C library that gives no stability promise.
**Comparables.** niri and cosmic-comp pin smithay to a git rev with `default-features = false`
and an explicit feature list (`niri/Cargo.toml:28-32,100-116`; `cosmic-comp/Cargo.toml:94-149`).
wayvr chose smithay as the Rust Wayland frontend that coexists with a **runtime-owned Vulkan
device** — its `backend_vulkan` and `xwayland` features are enabled but unused for rendering and
XWM, satellite instead (research/39 §2). stardustxr is not a Wayland compositor at all any more
(Wayland moved out of process to Flatland; `waynest` is Verdi's, not stardust's — a correction to
research/39 §4). gamescope is C++ over a wlroots fork for SteamOS's copy/latency goals
(`README.md:3-11`). wlroots' Vulkan renderer cannot adopt an external device: the only public
constructor is `wlr_vk_renderer_create_with_drm_fd` (`include/wlr/render/vulkan.h:21`) — research/39
§3 re-verified at the pin.
**Transfer.** motorcar's reason for QtWayland is smithay's selling point today: a renderer-free
Wayland frontend (research/39 §1.1) that leaves the 3D compositing to zxr. wxrc's fate is the
argument for a pinned Rust dependency over a C library tracked by hand. **Verdict: ADR 0006's
base re-affirmed with the lineage's own reasons on record: Rust + smithay (niri's pinning
convention), openxrs for the runtime, ash for the renderer; wlroots fallback only on a structural
frontend defect.**

## 15. Validation matrix and what falls out

| Assertion (ADR 0006 / composition §7 / research/39 §5) | Lineage | Comparables' positions | Verdict |
|---|---|---|---|
| clients render, compositor composites depth into one scene | motorcar thesis | Monado/OpenXR: painter's algorithm, no cross-layer depth; overlay compositors need none | **confirmed** |
| one OpenXR client, one stereo projection layer | wxrc | wayvr/gamescope submit overlays; stardust/gxr projection; `XR_EXTX_overlay` provisional | **confirmed** |
| Vulkan on a runtime-created device (`vulkan_enable2`) | (GL era) | wayvr, gxr; gamescope owns its device (overlay path) | **refined** |
| single loop driven by `xrWaitFrame` | wxrc, motorcar | wayvr same; gamescope/KWin/mutter move the blocking wait to a thread; spec allows both | **refined — Q1** |
| frame callbacks after submit, never wait for clients | motorcar §99 | niri/cosmic once per refresh; weston/KWin/mutter deadlines; Monado's app pacer | **confirmed** |
| dmabuf import, device-derived modifier table, zero copies | — | wayvr, gamescope, wlroots; smithay leaves import to the compositor | **refined** |
| `wp_linux_drm_syncobj_v1` acquire/release end to end | — | gamescope; smithay blocker + drop-release; mutter/KWin GPU-done release | **refined** |
| ray → `wl_pointer` for 2D; 6DoF events for 3D clients | motorcar | wayvr, xrdesktop, wxrc | **confirmed** |
| scene graph with frames (places model) | motorcar | stardust, xrdesktop graphs; wxrc/wayvr flat lists | **confirmed** |
| windowed dev backend in the compositor | — | Monado simulated HMD + window targets already provide it | **refined: none needed** |
| Xwayland via smithay `X11Wm` vs satellite (R0 output) | — | niri + wayvr satellite with transferable reasons; cosmic in-process | **determined: satellite** |
| same-binary restricted greeter, no listening socket | — | gnome-shell mode; KWin socketpair for helpers; no-socket is Mura's | ADR 0007 stands |
| `sd_notify` + `RestartMode=direct` | — | niri, cosmic, KWin wrapper | **confirmed** |
| Rust + smithay pinned; wlroots fallback | QtWayland's reason | niri/cosmic/wayvr; wlroots cannot adopt an external device | **re-affirmed** |

**For the owner (Phase C):**

- **Q1 — loop ownership.** (a) wxrc/wayvr: the OpenXR loop owns the thread; `xrWaitFrame` blocks
  there and calloop is dispatched with a bounded timeout once per frame — simplest, one thread,
  what the lineage and the one shipping Rust XR compositor do; cost: Wayland input latency is
  quantised to the frame, and calloop is used against its grain. (b) gamescope/KWin: calloop owns
  the thread; a dedicated thread blocks in `xrWaitFrame` and posts `XrFrameState` into the loop;
  cost: one more thread and a handoff, gain: smithay used as designed and input handled between
  frames. The spec's Q1 answer fixes the module boundary; R0 gate 1 measures the missed-deadline
  count either way.

Everything else above converged or refined without a fork, and is applied in
[specs/zxr-core.md](../../specs/zxr-core.md). The DE-level abstractions (layers, places,
workspaces, foreign sessions, waypipe, 3D processes, notifications, tray, launcher, OSD, lock,
capture, a11y, the protocol sweep) are the companion document's,
[research/60](60-de-abstractions-mapped-to-xr.md).
