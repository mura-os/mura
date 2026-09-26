# 65 — The embedded frame path: where zxr spends on a battery SoC, from comparables and measurement

**Research date:** 2026-09-26. **Question:** the four costs of zxr's per-frame path on a
battery-powered SoC — runtime IPC, GPU, memory, wake-ups — what the comparables and the runtime do
about each, what the dev host can measure, and which decisions follow. Companion to
[research/59](59-xr-compositor-architecture-from-comparables.md) (the program's mechanisms) and
[research/62](62-scene-data-model-from-comparables.md) (the scene). Plan:
`embedded_frame-path_efficiency` (2026-09-26).
**Constraint:** no target hardware. Every number below carries one of three labels —
**measured (host)**: the dev host (AMD Strix Halo, RADV, x86 Unix sockets, Monado simulated HMD
at 60 Hz, `XRT_COMPOSITOR_NULL`), valid on device as *structure* (counts, ratios, A/B deltas),
not as absolute time; **analytic**: computed from resolution × format × pass structure, the
number the hardware would confirm; **hardware-deferred**: cannot be known without the device.
The hardware-deferred set is the first-hardware verification list in
[implementation-path.md §5](../architecture/implementation-path.md).
**Method (AGENTS.md rule 7):** for each cost, the comparables' mechanism *and their reasons*
(`references/<clone>/path:line`; [external] where no clone), the runtime's own behaviour (Monado,
the OpenXR spec), then the measurement. **Budget impact:** a research document; its findings feed
[budgets.md](../architecture/budgets.md) and spec rev 2.

## 0. Instrumentation (Phase 0) and the baseline

Added to zxr's frame journal (`pkgs/zxr/src/journal.rs`, printed on SIGUSR1/exit): a latency
histogram per runtime call (count, mean, max, log2 buckets 16 µs … ≥ 1 ms) for `xrWaitFrame`,
`xrBeginFrame`, `xrLocateViews`, `xrLocateSpaces`, acquire/wait/release per swapchain,
`xrEndFrame`, `xrPollEvent`; the per-frame call census; the analytic attachment traffic of our
pass; frame callbacks split visible/occluded by a frustum test. Harness: strace census;
per-thread wake-ups from `/proc/<pid>/task/*/schedstat`; CPU ms/s from `/proc/<pid>/stat`;
isolated `XDG_RUNTIME_DIR` so a concurrent dev-session is untouched.

### 0.1 Baseline — measured (host), release build, Monado **null compositor** (`XRT_COMPOSITOR_NULL`, which paces at ≈ 19.4 Hz — 281 frames per 14.5 s; the real compositor in §2.3 paces at 58 Hz). Per-second figures here are therefore at 19.4 frames/s; per-frame figures are the comparable ones.

| | idle (no client) | 1 foot | 3 foot |
|---|---|---|---|
| runtime calls per frame | **11.16** | 11.16 | 11.16 |
| of which block the state loop | 7 (begin, locateViews, acquire ×2, release ×2, end) | 7 | 7 |
| loop time inside runtime calls, per frame | 269 µs | 273 µs | 292 µs |
| `xrBeginFrame` / `xrLocateViews` / acquire / release / `xrEndFrame` mean | 23 / 91 / 87 / 44 / 90 µs (debug, under strace) | | |
| `xrWaitSwapchainImage` | **0 µs — client-local**, not IPC | | |
| `xrWaitFrame` (wait thread, blocking = the runtime's throttle) | 16.5 ms mean = one 60 Hz period | | |
| zxr wake-ups/s: state loop / wait thread | **408 / 146** (≈ **21 / 7.5 per frame** at 19.4 Hz) | 410 / 144 | 416 / 149 |
| zxr CPU | 7 ms/s (0.7 %) | 10 ms/s | 10 ms/s |
| monado-service wake-ups/s (busiest thread) / CPU | 307 / 4 ms/s | 311 / 3 ms/s | 309 / 4 ms/s |
| GPU per frame (ours, timestamps) | 37 µs (clear only) | 54 µs | 59 µs |
| wake → `xrEndFrame` | 431 µs | 519 µs | 642 µs |
| passes per frame / attachment stores per frame (analytic) | 2 / 7.2 MB (2 × 896×1007 × 4 B) | | |
| frame callbacks sent to occluded planes | 0 (all three planes in view at the fan positions) | | |

Syscall census (strace -c, 600 frames, debug build, includes the foot child): `futex` 4217,
`ioctl` 5215 (~8.7/frame: DRM submits, fence waits), `sendmsg` 9061 / `recvmsg` 10273 (Monado
IPC + Wayland), `epoll_pwait` 1263 (~2/frame), `clock_nanosleep` 600 (**1/frame — inside the
client-side `xrWaitFrame`**, Monado's app pacer sleeps in the client), `timerfd_settime` 652.

**Reading of the baseline.** The state loop wakes ~21 times per frame: 11 of them are the blocking runtime RPCs (§1.2 — every round trip is a sleep/wake pair), one is the tick, one the fence wait, and the rest are the Vulkan submits inside Monado's acquire path and the Wayland/timer sources. The wait thread wakes ~7 times per frame (Monado's two `xrWaitFrame` RPCs plus its client-side sleep). That is the shape the embedded numbers follow: **wake-ups scale with the runtime RPCs on the loop** (§2.3 confirms: 11 RPCs → ~20 wake-ups per frame, 5 RPCs → ~10), and CPU time per frame (≈ 360 µs idle here) is dominated by those context switches, not by the scene or the draw list. GPU time is noise on this host; its structure (2 passes, 7.2 MB of stores)
is what transfers.

### 0.2 The tiler substitute — Vulkan best-practices layer, Arm + IMG + AMD checks (measured, host)

`VK_LAYER_KHRONOS_validation` with `validate_best_practices{,_arm,_img,_amd}` over a 300-frame
run (the layer's Qualcomm set does not exist in this version). **Two validation errors, both
fixed on the branch:** `VUID-VkDeviceCreateInfo-pNext-02830` — we chained
`VkPhysicalDeviceVulkan12Features` *and* Monado inserts a `VkPhysicalDeviceTimelineSemaphoreFeatures`
into the chain unless one is already present (`monado/src/xrt/state_trackers/oxr/oxr_vulkan.c:491-508`),
which the spec forbids together → use the KHR struct only; and `vkGetQueryPoolResults-None-09401`
— the timestamp pool read before its first reset (first frame) → read only after a submission.
**Performance warnings, attributed:**

| warning | whose | reading |
|---|---|---|
| `RenderPass-redundant-store` ×10 — "image was cleared as part of LOAD_OP_CLEAR, but last time … STORE_OP_STORE … wastes bandwidth on tile-based architectures" | ours, **inherent**: the swapchain image *must* be stored for the runtime; the layer cannot see the consumer | confirms the analytic model: one full store per view per frame is the floor of the projection path |
| `vkBindImageMemory/BufferMemory-small-dedicated-allocation` ×18 — sub-allocate images < 1 MB | ours | an allocator (or `gpu-allocator`-class crate) is a Phase 4 item; per-image `vkAllocateMemory` is the R0 shape |
| `Arm-vkCreateSampler-lod-clamping` — minLod = maxLod = 0 | ours, intentional (no mip chains at R0) | mip chains for far planes are a §2 item (minification bandwidth vs aliasing) |
| `Pipeline-SortAndBind` ×10 — pipeline bound twice per frame | ours, trivial (once per view pass) | multiview would remove it (§2) |
| `AMD-CreatePipelinesLayout-KeepLayoutSmall` — 80 B of push constants | ours, AMD nit | none |
| `Arm-vkBeginCommandBuffer-one-time-submit` ×10 — `SIMULTANEOUS_USE` without `ONE_TIME_SUBMIT` | **Monado's** client compositor command buffers | runtime-side; an upstream note, not ours |
| `ImageBarrierAccessLayout` ×10 — access mask vs `COLOR_ATTACHMENT_OPTIMAL` on a `vk_image_collection` image | **Monado's** | runtime-side |

Absent, and notable: no `LOAD_OP_LOAD`, no non-transient-depth, no resolve warnings — the pass
structure is already the tiler shape except that the depth image is *allocated* (Phase 4 makes
it lazily allocated; the layer's Arm set does not flag that directly).

## 1. Runtime IPC (Phase 2)

### 1.1 What Monado's IPC is (read)

Unix `SOCK_STREAM` (`ipc_client_connection.c:227`, `ipc_server_mainloop_linux.c:85`), fixed-size
generated message structs over `sendmsg`/`recvmsg` (`ipc_message_channel_unix.c:89-127`), one
mutex per connection held for the whole round trip — "Other threads must not read/write the fd
while we wait for reply" (`proto.py:72-75`). Shared memory is mapped **once** at connect
(`ipc_client_connection.c:278-298`) and carries device roles, view sizes and the **layer slots**
(`ipc_protocol.h:243-247, 265-330`); swapchain images cross as fds **once** at creation
(`ipc_server_handler_swapchain.c:142-146`; `doc/ipc-design.md:193-205` "no buffer copy in the
frame loop"). `xrEndFrame` writes its layers into a shared-memory slot and sends **one** RPC,
`compositor_layer_sync` (`ipc_client_compositor.c:574-583, 740-758`). No peer-credential check:
`IPC_CRED_SIZE 1 // auth not implemented` (`ipc_protocol.h:34`); clients self-describe
(`:338-342`). The service owns the devices, the system compositor and the display; a client
owns a socket, the mapped slots and buffer fds (`doc/understanding-targets.md:80-88`). The
stated reasons for the split are multi-client composition, multi-arch clients to one service and
the allocation quota/DoS argument (`doc/swapchains-ipc.md:35-42`) — not crash isolation as such.

### 1.2 The per-frame round trips (read + measured)

| step | RPCs | source |
|---|---|---|
| `xrWaitFrame` | **2** (`predict_frame`, then a client-side `nanosleep`, then `wait_woke`) | `ipc_client_compositor.c:540-553`; the one `clock_nanosleep` per frame of §0.1 |
| `xrBeginFrame` | 1 | `:564-570` |
| `xrLocateViews` | **2** (`get_view_poses` + `locate_device`) | `ipc_client_hmd.c:133-162`, `oxr_session.c:882-906`, `ipc_client_space_overseer.c:235-242` |
| `xrAcquireSwapchainImage` (Vulkan) | **2 + a queue submit** — the OXR Vulkan path waits inside acquire "to be fully conformant to the Vulkan spec" and records the ownership barrier there | `oxr_swapchain_vk.c:22-57` |
| `xrWaitSwapchainImage` (Vulkan) | **0** — "We have already waited in acquire" | `oxr_swapchain_vk.c:81` (matches §0.1's 0 µs) |
| `xrReleaseSwapchainImage` | 1 | `ipc_client_compositor.c:201-208` |
| `xrEndFrame` | 1 | `:740-758` |
| `xrPollEvent` | 1 | `ipc_client_session.c:50-56` |
| `xrLocateSpace` / `xrLocateSpaces` | 1 each / **1 for all** | `ipc_client_space_overseer.c:149-157` / `:161-213` |
| `xrLocateHandJointsEXT` | 1 per hand | `ipc_client_hand_tracker.c:33-50` |

So a stereo zxr tick today is **13 RPCs** (2 on the wait thread, 11 on the loop — the journal's
11.05), of which the two acquires carry a queue submit each. At M1 with head + two hands + a
handful of anchors: +1 (`xrLocateSpaces`) + 2 (hands) = 16, versus 20+ if spaces were located
one by one.

### 1.3 What it costs (measured, host; hardware-deferred on device)

Loop-side time inside runtime calls: 205–298 µs per frame across all runs (§0.1, §2.3);
per call 23–90 µs on this host. Wake-ups: **~20 per frame on the loop with 11 RPCs, ~10 with 5** (§2.3) — the RPCs, the Vulkan submits inside Monado's acquire, the fence wait and the tick; wake-ups track the RPC count. CPU: the
IPC is not where the milliseconds go on this host (7–19 ms/s total), but each RPC is a
context-switch pair, and that — not bytes — is the embedded cost. **Analytic band for an
aarch64 SoC** [external, Unix-socket RTT 20–60 µs on mobile cores]: 11 loop RPCs × 20–60 µs =
0.2–0.7 ms of an 11.1 ms (90 Hz) frame, plus ~20 wake-ups; the quad path's 5 RPCs halve both.
**Hardware-deferred:** the actual RTT and whether the service's own thread (307–930 wake-ups/s
here) competes for the same cores.

### 1.4 The in-process alternative (read)

Monado without `XRT_FEATURE_SERVICE`: `libopenxr_monado.so` links `comp_main` and the prober and
owns devices, compositor and display **inside the application** (`CMakeLists.txt:308, 331-336`;
`targets/openxr/target.c:38-44`; `doc/understanding-targets.md:70-78` "simplest architecture").
Single process, single client — no other OpenXR app can run beside it. `sdl_test` is **not** an
in-process app: it is an IPC *server* with an SDL compositor (`sdl_main.c:19-35`, links
`ipc_server`). WiVRn embeds Monado's libraries but *is* the service; StardustXR and wayvr are
ordinary IPC clients. **Verdict — determination: stay an IPC client.** The isolation is real
(the compositor does not hold the device fds or the display), every other OpenXR app on the
headset needs the service anyway, and the measured cost is ≤ 0.7 ms/frame analytic. Batch what
the API batches (`xrLocateSpaces`), avoid what can be avoided (quad mode's 6 fewer RPCs), and
record the RTT as the first hardware number to take. Not brought to the owner: no comparable
compositor runs in-process with its runtime, and the analytic band does not warrant the loss.

## 2. GPU — the composition fork (Phase 1)

### 2.1 What the runtime does with layers (read)

Monado composites by a **layer squasher then distortion** (`comp_render.h:33-46, 64-68`):
every layer is drawn per view into a scratch image, then the scratch is distorted into the
target. **One exception:** exactly one projection layer with no colour scale/bias or chroma key
takes the **fast path** — "goes directly to the distortion shader, so no need to use the layer
renderer" (`comp_compositor.c:272-303`; `comp_render_gfx.c:924-935`). Quads are drawn with a
full 6-DoF MVP against the current view (`comp_render_gfx.c:473-475`), cylinders likewise
(`:274-285`); no depth test between layers (`render_gfx.c:425-428`, `comp_render.h:42-43`);
order = submission order (`comp_render_gfx.c:764-814`), the spec's painter's algorithm
(`rendering.adoc:1143-1147`). Timewarp is **rotational only** — "rotation only so we get 3dof
timewarp" (`comp_render_gfx.c:418-420`; `render_util.c:140-172` uses orientations alone); the
depth layer is bound in the CS path but never read (`layer.comp`), so
`XR_KHR_composition_layer_depth` buys nothing on Monado today. `XRT_MAX_LAYERS` = 128 on Linux,
32 on Android (`xrt_limits.h:80-89`). The spec's own words for quads: "allowing a better match
between the resolutions of the XrSwapchain image and footprint of that image in the final
composition. This improves legibility for user interface elements … and allows optimal sampling
during any composition distortion corrections" (`rendering.adoc:1223-1230`).

**Consequence for the fork, analytically.** Projection path: our pass (sample every window,
store 2 full views) → Monado fast path (read 2 full views, distort). Quad path: Monado squasher
(sample every quad into scratch, store 2 full views) → distortion (read scratch). **The same
number of full-resolution passes**; what moves is *who* samples the windows — and the quad path
adds a **copy per commit**: OpenXR swapchain images are runtime-allocated
(`comp_swapchain.c:693-704`, 3 images, not client-choosable), so a Wayland client's buffer can
never be one; it must be blitted (our `blit_to_panel`). The bandwidth trade is therefore: quad
saves nothing per frame on Monado and costs `window_bytes × 2` per *commit*; it wins on
**legibility** (one resampling instead of two), **latency** (the runtime samples at its own,
later pose), and — the large one — **zxr does no GPU work and half the IPC when only the head
moves**. It loses depth ordering between windows (painter's order only), per-pixel cutout over
windows (the foreground layer must stay in a projection pass), and subsurface/popup composition
must happen before the blit (pre-compose per window, or one layer per surface).

### 2.2 The comparables' positions (read)

- **wayvr / wlx-overlay-s**: one `Quad` or `Cylinder` layer per overlay window
  (`backend/openxr/overlay.rs:133-199`), `xrWaitFrame` every display frame (`mod.rs:248-279`),
  renders an overlay only when dirty and otherwise **re-submits the stale swapchain image**
  ("showing stale frame", `mod.rs:406-424`; `ShouldRender::{Should, Can, Unable}`,
  `windowing/backend.rs:37-43`); layers sorted by distance and z-order (`mod.rs:449-458`); no
  layer-cap handling; no stated reason in code. Its workaround "Monado freaks out if no layers
  are submitted" (`mod.rs:367-373`) is the spec's rule that an empty `xrEndFrame` clears the
  display (`extx_overlay.adoc:184-191`).
- **kwin-vr**: one projection layer (`XrScene.qml:18-24`, `depthSubmissionEnabled: false`); no
  stated reason for either in the tree or the patches README — the choice is Qt Quick 3D XR's
  default shape.
- **motorcar, Simula, StardustXR**: one projection pass (research/62).
- **Consumer platforms** [external, mechanism only]: layer-per-panel for UI is the documented
  recommendation on Quest and visionOS for legibility and to let the runtime reproject; zxr
  takes only the mechanism claim, which the OpenXR spec makes too (`rendering.adoc:1223-1230`).

### 2.3 A/B on the host (measured) — `--panels=projection|quad|hybrid`, real Monado compositor, head rotating

| run | zxr CPU ms/s | zxr loop wake/s | zxr GPU µs/frame | RPC/frame (loop) | loop IPC µs/frame | Monado CPU ms/s | GPU busy % | blits |
|---|---|---|---|---|---|---|---|---|
| projection, 1 static foot | 19 | 1143 | 44 | 11.05 | 205 | 19 | 8 | — |
| **quad**, 1 static foot | **10** | **600** | **0** | **5.06** | **93** | 20 | **6** | 2 total |
| hybrid (quads + empty projection), 1 foot | 23 | — | 81 | 11.05 | 251 | 22 | 9 | 3 |
| projection, 3 static foot | 31 | — | 86 | 11.05 | 283 | 25 | 7 | — |
| **quad**, 3 static foot | **13** | — | 0 | 5.06 | 116 | 21 | 7 | 6 total |
| projection, vkcube (mailbox, ~14 k commits/s) | 119 | 3914 | 102 | 11.05 | 298 | 24 | 7 | — |
| quad, vkcube | 118 | 3692 | 0 | 5.06 | 118 | 25 | 6 | 837 (one per displayed frame, 1.67 GB / 14 s) |

Reading: with static UI and a moving head — the headset's common case — the quad path **halves
zxr's CPU and wake-ups, removes our GPU pass, and costs Monado nothing extra** (its CPU and the
GPU-busy figure are flat or lower: the squasher replaces our pass, it does not add to it). With
a client committing every display frame the blit appears (one per *displayed* commit — the
prototype coalesces to display rate) and the two paths are level on CPU; GPU-busy still favours
quad by a point. Hybrid pays both bills and is the worst of the three at R0 — it earns its place
only when there is depth content (3D volumes, the foreground cutout) to put in the projection
layer. **Hardware-deferred:** GPU time per path on a tiler; the blit's cost at 1832×1920-class
panel resolutions; Monado's squasher on an XR2-class GPU.

### 2.4 The fork for the owner (AGENTS.md rule 8 — the options as the comparables hold them)

*What is being decided:* whether zxr's 2D windows reach the display through zxr's projection
pass (research/59 §2's current determination, spec §7) or as runtime composition layers.

*Why it is a decision:* it changes spec §7 (the frame), the renderer's role, the scene flatten
(draw items → layer entries + per-panel swapchains), gate 2's zero-copy property (a blit per
commit is unavoidable for runtime layers), and what the foreground cutout can cover.

*Options:*
- **(a) Projection pass for everything** — motorcar, kwin-vr, Simula, StardustXR, zxr today.
  Keeps: depth ordering and intersections between windows, per-pixel cutout over everything,
  zero-copy client buffers, one code path. Costs: zxr renders every frame the head moves
  (11 RPCs, 7 wake-ups, a full pass), double resampling of window content (legibility),
  runtime timewarp only.
- **(b) Runtime quad/cylinder layer per window** — wayvr/wlx-overlay-s; the spec's and the
  consumer platforms' recommendation for UI. Keeps: legibility, runtime-side reprojection at
  the latest pose, zxr idle when only the head moves (measured: −47 % CPU, −48 % wake-ups, 0
  GPU), 5 RPCs. Costs: a copy per commit; painter's order only (no window intersections); the
  cutout cannot cover windows; popups/subsurfaces pre-composed per window; a layer cap (128 /
  32 on Android) that a busy desktop can approach.
- **(c) Hybrid** — quads for the window tiers and overlay; projection layer for environment, 3D
  volumes and the foreground cutout. Keeps (b)'s wins for windows and (a)'s depth for the rest;
  costs both paths' fixed overheads every frame (measured worst at R0, where the projection
  layer is empty) and the cutout still cannot cover windows.

*My read, labelled as such:* (b) for M1's flat panels with (c) as the shape once depth content
exists — the spec's own text and the only XR comparable that ships a desktop-like set of panels
both point there, and the measurement is unambiguous for the common case. The blit per commit
is the price; whether it is acceptable at panel resolution is a hardware number. The foreground
cutout not covering windows is the design consequence to weigh — perception-passthrough-hands.md
assumes the matte composites over *everything*.

## 3. Memory (Phase 4)

### 3.1 Client buffers (read)

Every comparable steers clients through dmabuf feedback tranches built from the render node's
formats, with scanout tranches where a plane could take the buffer (niri `tty.rs:856-861,
2835-2891` — "We limit the scan-out tranche to formats we can also render from so that there is
always a fallback render path"; cosmic `kms/mod.rs:605-638`, `surface/mod.rs:1528-1589`; anvil
`udev.rs:452-455, 721-765`). zxr has no scanout of client buffers (everything is sampled), so
the single sampling tranche it advertises today is the complete answer; the per-device
*formats* come from the driver at runtime (`sampled_modifiers`). Buffer counts are the client's:
Mesa's Wayland WSI is not in the pins [external — to be cited from upstream when the number is
needed]; Monado's own swapchains are 3 images (`comp_swapchain.c:693-704`).

### 3.2 shm (read)

Nobody imports the shm pool as GPU memory: wlroots stages and copies to an optimal-tiled image
("deferred upload by transfer; using staging buffer", `render/vulkan/texture.c:55-56, 77-147`);
KWin `glTexSubImage2D` per damage rect (`texture.cpp:181-228`); mutter — "a shared-memory buffer
will still need to be uploaded to the GPU" (`meta-wayland-buffer.c:43`, damage-driven
`_cogl_texture_set_region`, `:714-779`); smithay GLES per-damage `TexSubImage2D`
(`gles/mod.rs:975-1004`). zxr's path is wlroots' shape minus damage: it uploads the whole
buffer on every commit change. **Determination:** damage-aware upload (the four comparables
converge) is the engineering item; host-memory import stays a rethink candidate for UMA devices
with no comparable behind it, not an R0/M1 change.

### 3.3 Compositor-owned memory (analytic + measured)

Per view: swapchain images 3 × 896×1007 × 4 B = 10.8 MB (runtime-owned); depth 3.6 MB
(ours; **0 once transient/lazily allocated** on a tiler — Phase 4 change on the branch); at
XR2-class 2 × 1832×1920: 3 × 14 MB per view runtime-owned, 14 MB depth ours → 0. Quad mode adds
one 3-image swapchain per panel: a 700×500 panel = 4.2 MB runtime-owned. RSS on the host
(research/61): anon 7.5 + binary 2.7 + one ICD 4.8 MB; the fence is restated in those terms.

## 4. Wake-ups, idling, pacing, scheduling (Phase 3)

### 4.1 When may zxr not render (read)

The spec: the frame loop must run while the session runs — "keep running the frame loop to
maintain the frame synchronization … even if this requires calling xrEndFrame with all layers
omitted" (`rendering.adoc:900-905`); `shouldRender == false` → skip GPU work, submit no layers
(`:882-905`); `SYNCHRONIZED` → skip rendering; **`VISIBLE` (unfocused) → keep rendering** — "It
is important for applications to continue rendering when visible, even when they do not have
focus" (`session.adoc:559-601`). Monado sets `shouldRender` from the state alone
(`oxr_session.c:125-133`) and an empty `xrEndFrame` discards the frame while the multi-compositor
keeps showing the last delivered one (`oxr_session_frame_end.c:1836-1853`,
`comp_multi_compositor.c:937-947`). An app cannot run below display rate by skipping
`xrWaitFrame` — Monado's app pacer *itself* stretches the app period to whole display periods
when the app is too slow (`u_pacing_app.c:292-318, 476-537`) and the compositor reuses the last
layers. **What this means for zxr:** in projection mode a head movement is a re-render, always;
the only idling is `SYNCHRONIZED` and doff/idle (research/12); in quad mode the runtime
re-samples the panels and zxr renders only on commit — the A/B's 0 GPU / 5 RPC row.

### 4.2 Frame callbacks (read → implemented on the branch)

niri sends `wl_surface.frame` only to surfaces whose primary scan-out output is the presenting
output — "avoids sending frame callbacks to invisible surfaces" — once per refresh, with a
995 ms fallback timer for surfaces that need one (`niri.rs:5178-5208, 201, 5262+`); KWin notifies
only items painted in the frame and drives invisible windows' callbacks from a refresh-rate
timer (`item.cpp:739-751`, `window.cpp:4432-4441`); mutter requires the actor to be primary on
the presenting view (`meta-wayland.c:182-219`). **Determination (converging):** frame callbacks
go to planes in either view's frustum each tick; planes out of view get one on a ~1 s fallback
so clients that wait on a callback do not stall. The frustum census (§0.1) was the precondition;
the gate is on the branch.

### 4.3 Scheduling (read)

Monado: `SCHED_FIFO` at the **maximum** priority for its compositor and vblank threads
(`u_linux.c:89-117`, no rtkit); gamescope: nice −20 and, with `--rt`, `SCHED_RR` at the
**minimum** RR priority, only with `CAP_SYS_NICE` (`main.cpp:881-891`, `Process.cpp:619-636`);
KWin: `SCHED_RR | SCHED_RESET_ON_FORK` at the minimum (`realtime.cpp:17-26`); mutter: rtkit
`MakeThreadRealtime` (`meta-thread.c:254-360`). No pinned compositor unit sets
`CPUSchedulingPolicy`/`CPUAffinity`. **Determination (converging on the compositors, not the
runtime):** the *compositor* asks for the minimum RT priority with `RESET_ON_FORK` so clients
never inherit it; the runtime takes the maximum. For Mura the standard mechanism is the unit
(`CPUSchedulingPolicy=rr`, `CPUSchedulingPriority=1`, `CPUSchedulingResetOnFork=yes`) —
session-bootstrap's unit contract, not code; the in-code request is the fallback when zxr runs
outside its unit (dev-session). Affinity to the big cluster is a device-contract value
(hardware-deferred).

### 4.4 Render-minimisation techniques (Phase 3b) — the table

| technique | mechanism | owner | Monado today | cost / benefit | artefacts | comparables' defaults | Mura position |
|---|---|---|---|---|---|---|---|
| Rotational timewarp (ATW) | re-sample the last frame for head rotation at display time | runtime | **yes**, 3-DoF only (`comp_render_gfx.c:418-420`) | free to the app | none visible for rotation; translation not corrected | every runtime | baseline; nothing to do |
| Positional / depth reprojection | reproject with the app's depth (`XR_KHR_composition_layer_depth`) | app submits depth; runtime warps | extension **exposed** (`CMakeLists.txt:452`) but depth **never read** (`layer.comp`) | app: one more attachment stored per view (the tiler cost we avoid today with `DONT_CARE`) | disocclusion holes at edges | kwin-vr declines (`depthSubmissionEnabled: false`, no stated reason) | **not now**: no benefit on Monado, a full-res store per view; revisit when a runtime uses it (a setting then, since it trades bandwidth for parallax) |
| Space warp / frame synthesis | app submits motion vectors + low-res depth; runtime extrapolates frames so the app runs at half rate (`fb_space_warp.adoc:15-31`, `ext_frame_synthesis.adoc:21-35`) | app + runtime | **unsupported** (not in `oxr_extension_support.py`) | app halves render rate; must render NDC motion vectors | the controversial one: warping, edge shimmer, judder on fast content; "2D instead of 3D motion vector data may decrease the quality" (`ext_frame_synthesis.adoc:178`); SteamVR/Oculus users disable it [external] | wayvr/kwin-vr: none | **not applicable** to flat panels (no motion vectors worth having; text is the worst case) and unsupported by the runtime; if ever offered for 3D volumes, a per-app user setting, default off, artefacts named |
| Half-rate app frame rate | app submits every other display frame; runtime reuses the last layers | runtime pacer decides | **automatic** when the app is slow (`u_pacing_app.c:292-318`); not app-requestable | halves app GPU/CPU | judder of head-locked content; panels smear under head motion without positional warp | — | not a lever for zxr: with quad layers the runtime re-samples every frame anyway and zxr renders only on commit — the better version of the same saving |
| Fixed / eye-tracked foveation | `XR_FB_foveation*` profiles → fragment density map on the swapchain (`fb_foveation_vulkan.adoc:42-51`); `XR_META_foveation_eye_tracked` | app requests; runtime supplies the FDM | **unsupported** | shading-rate reduction in the periphery | text blur off-centre; eye-tracked variant needs the ET stack (research/28) | none in the pins | **later**, and only for the projection pass (3D volumes); panels as quad layers are sampled by the runtime at their own resolution, which is the legibility argument for (b) |
| Dynamic resolution | shrink `XrSwapchainSubImage::imageRect` under GPU pressure | app | plain OpenXR | linear bandwidth saving | softness | gamescope/Steam dynamic res [external] | a `render` knob for the projection pass at M2; not for panels |
| Display refresh rate | `XR_FB_display_refresh_rate` — request a lower rate (`fb_display_refresh_rate.adoc:141-164`: "lowering … can provide better thermal sustainability but at the cost of … higher latency and flickering") | app requests; runtime decides | **exposed** (`CMakeLists.txt:405`) | whole-system power | flicker, latency | — | a **user setting** (power vs comfort), default the panel's native rate, artefacts named in the description |
| Multiview | one pass renders both views (`VK_KHR_multiview`, layered swapchain) | app | plain Vulkan; Monado supports array swapchains | halves draw calls/vertex work; tile-friendly | none | Qt Quick 3D XR (kwin-vr's `QT_QUICK3D_XR_DISABLE_MULTIVIEW` toggle); Monado's squasher | **determination** for the projection pass when it carries 3D content (M2); irrelevant to quad panels |
| Late latching | re-read the view pose just before submit | app | `xrLocateViews` at predicted time is the API's form | latency | none | — | already the shape (views located per tick at the predicted time) |
| Damage-gated rendering | render only when content changed | app | — | zxr idle when only the head moves | none | wayvr's `ShouldRender` (`windowing/backend.rs:37-43`) | **only possible with (b)/(c)**; the A/B's 0-GPU row is this |

Settings-schema entries the table implies for M1: `display.refresh-rate` (enum from
`xrEnumerateDisplayRefreshRatesFB`, default native), and — only if a runtime ever reads depth or
synthesises frames — `render.depth-reprojection` / `render.frame-synthesis` (default off,
descriptions naming the artefact classes). Nothing else here is a user choice; the rest is
mechanism.

## 5. Findings by label

**Measured (host):** 13 RPCs/tick (11 on the loop), ~20 loop wake-ups per frame tracking the RPCs (~10 with quads); loop IPC
0.2–0.3 ms/frame; quad panels −47 % CPU, −48 % wake-ups, 0 GPU for static UI under head motion,
Monado flat; blit coalesced to display rate for a live client; zero validation errors after two
fixes; the perf-warning attribution of §0.2.
**Analytic:** 0.2–0.7 ms/frame IPC on an aarch64 SoC; equal full-resolution pass counts for
projection vs quad on Monado; the blit's `2 × window bytes` per commit; depth → 0 bytes with
transient attachments.
**Hardware-deferred (→ implementation-path §5):** IPC RTT on the device and core contention with
`monado-service`; tiler GPU time per pass for both panel paths; the blit's cost at panel
resolution; Monado's squasher on an XR2-class GPU; real RSS with one ICD; wake-ups and CPU under
the device's power management; big-core affinity.

## 6. Determinations and the one owner item

Determinations (converging evidence, acted on in the branch or recorded for spec rev 3): stay an
IPC client and batch `xrLocateSpaces` (§1.4); frame callbacks visibility-gated with a fallback
timer (§4.2); minimum-RT-priority with `RESET_ON_FORK` through the unit, in-code fallback (§4.3);
transient/lazily-allocated depth (§0.2, §3.3); damage-aware shm upload (§3.2); multiview for the
projection pass when it carries 3D content (§4.4); no depth-layer submission on Monado (§4.4);
display refresh rate as a user setting (§4.4). **Owner item:** the composition fork, §2.4.
