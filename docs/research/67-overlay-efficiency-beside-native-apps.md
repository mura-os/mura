# 67 — zxr as Monado's overlay beside a native OpenXR game: the seven efficiency issues, from comparables and measurement

**Research date:** 2026-09-26. **Question:** [native-openxr-apps.md](../architecture/native-openxr-apps.md)
makes zxr an `XR_EXTX_overlay` session that goes quiet while a game is Monado's primary. Seven
efficiency issues follow from how Monado composites and paces (research/65 §2, §4): any overlay
layer costs the game its single-layer fast path; the cutout's shape and default over games;
the quiet frame loop; zero-layer behaviour; clients committing while quiet; the summoned shell's
footprint; and the small items (client-list polling, `io_blocks`, display-time correlation,
blend). Comparables first (AGENTS.md rule 7), then a test bench — xrgears as the main session,
zxr as a real overlay session — on the dev host.
**Owner's framing recorded as said:** the cutout's on-by-default over games is not a hard
constraint; off-by-default is acceptable *provided* the reserved system input always gives the
wearer summon and quit. Q-D(b) stands as ruled until the numbers below (§3) price it; the default
is §9's owner item.
**Labels:** measured (host — Strix Halo/RADV, Monado simulated HMD 60 Hz, 896×1007 per view;
GPU time per process from amdgpu `fdinfo` `drm-engine-*`, noisy to ±5 ms/s at this load because
the GPU downclocks; medians of three 20 s runs where stated) / analytic / hardware-deferred.
Plan: `overlay_efficiency_beside_games` (2026-09-26). **Budget impact:** a research document.

## 1. Comparables — who solved "an overlay beside a running game", and why they chose what they chose

*(filled from the source passes: Monado's history; wlx-overlay-s, xrdesktop/gxr, WiVRn,
kwin-vr, OpenComposite/xrizer; gamescope, mutter, niri, KWin; SteamVR and Quest as external
mechanism evidence.)*

### 1.1 Two shipping shapes for compositing overlays — and SteamVR abandoned the one we imagined

- **Quest (Meta Horizon OS)** [external, mechanism]: compositor layers are composited by the
  TimeWarp compositor in **one pass** — "the texture is sampled only once (source to screen)
  instead of twice (source to eye buffer, then eye buffer to screen)"; published costs on Quest 2
  at CPU/GPU level 4: **~0.1 ms flat per additional layer, ~0.6 ms for a fullscreen layer**, a
  per-pixel cost "even if the layer doesn't render to those pixels", 16 layers max, head-locked
  quads merged into one layer at no extra per-layer cost, and "setting a layer texture to
  0-alpha still incurs the full rendering cost — destroy layers you don't need instead of hiding
  them" (Meta, *Compositor layers*; *OVROverlay*).
- **SteamVR / OpenVR** [external, mechanism]: standard overlays are "rasteriz[ed] into each
  eye's render texture first" — a layer squasher, Monado's shape. The one path that composited an
  overlay "during the distortion pass … at a higher quality as it samples the source texture
  directly", `SetHighQualityOverlay`, supported a single overlay, no mouse input, no dashboard
  use — and **Valve removed it in OpenVR SDK 1.7.15**: "This approach to rendering overlays also
  didn't scale to modern displays" (commit `5aa6c5f`). The "fast path with overlays inside the
  distortion pass" idea has therefore been tried by a shipping runtime and withdrawn; the
  scalable shape Valve kept is the squasher.
- **Monado**: squasher then distortion, with the single-projection-layer fast path
  (`comp_compositor.c:272-303`; research/65 §2.1). The fast-path condition is evaluated on the
  *merged* layer count after `comp_multi` has combined every visible client
  (`comp_multi_system.c:265-305`), so one overlay layer from zxr is enough to leave it.
- **The `XR_EXTX_overlay` spec** has no issues section (`extx_overlay.adoc:310-311` "Issues:
  None"); revision 5 "Remove[d] bit requesting synchronized display times" — the authors chose
  *not* to promise correlated display times to overlays (§7.3).

### 1.2 Monado's own history (the pinned clone unshallowed for the read)

- The layer renderer (`aedd4d9f`, 2020, Lubosz Sarnecki: "a layer renderer capable of handling
  multiple quad and projection layers rendered in it's own Vulkan pipeline") always rendered into
  its own framebuffer which distortion then sampled; the **fast path** came in 2021 (`60024efb`,
  Jakob Bornecrantz) to "skip the layer renderer … **avoiding one copy**" (`doc/CHANGELOG.md:2186-2188`,
  !959). The compute squasher (2022, `95fb034b`/`f6821402`) and the per-view refactor (2023,
  `fed360e9`: "very marginally slower, around 0.05ms and 0.1ms slower on average") kept the shape:
  squash to scratch, then distort. **No commit argues against compositing in the distortion pass;
  none proposes it either** — the distortion descriptor layout has exactly one colour source per
  view in both GFX and CS paths (`render_resources.c:43-56, 249-274`), and CS never merges squash
  and distortion into one dispatch (`comp_render_cs.c:868-889`). The one cost remark about many
  layers is the CS sampler ceiling on weak GPUs (`comp_render_cs.c:627-632`, RPi4's 16 samplers).
- Overlays: Pete Black's 2020 multi-client work (`bd5aa244`: "if we are an overlay, we are always
  visible if we have a primary application"); the always-visible-and-focused rule for overlay
  sessions (`ipc_server_process.c:555-567`, `bcf9b62f`); `z_order` sort (`comp_multi_system.c:210-225`);
  the layer array bumped 16 → 128 in 2024 with "virtually no difference in CPU and GPU
  performance even when increasing RENDER_MAX_LAYERS to 1024" (`cef70d03`, !2341 — the array
  size, not the per-layer sampling cost); `io_blocks` in 2026 (`7674253d`, !2727).
- **Zero layers:** "Fixes layers from the previous frame being displayed when an app submits 0
  layers" (`a232d15e`, 2026-02-27, Sapphire, !2769, closes #591) — WayVR's "Monado freaks out"
  is that bug, fixed in the pinned Monado (§5).
- **Display time:** one `predicted_display_time_ns` per system frame is broadcast to every client's
  pacer, overlays included (`comp_multi_system.c:375-420, 560-592`) — overlays get the *same*
  display time on Monado although the extension promises nothing (§7.3).
- **No idle mode for a silent client:** `calc_app_period` lengthens an app's period only when its
  measured frame cost exceeds a display period (`u_pacing_app.c:291-318`); submitting nothing does
  not slow the loop. `libmonado`: `mnd_root_update_client_list` is one IPC snapshot with no
  change notification (`monado.c:264-274`, `monado.h:217-258`); `set_client_primary` and
  `set_client_io_blocks` are one IPC each (`:348-359, 393-409`).

### 1.3 The overlay shells that live over games

| shell | session while a game runs | placement | what it does to be cheap | stated reasons |
|---|---|---|---|---|
| **WayVR / wlx-overlay-s** | `XR_EXTX_overlay` | 5 (`helpers.rs:203-208`, no comment on the value) | `xrWaitFrame`/`Begin`/`End` every frame (`mod.rs:248-279`); renders an overlay only when dirty, else "showing stale frame" (`:406-424`); hidden overlays (α < 0.01) skipped (`:391-397`); skybox only when the main session is *not* visible (`:219-228, 385-387`); optional `io_blocks` on other clients while hovered (`blocker.rs:33-97`, config "Do not send controller input to other VR apps while WayVR is being hovered"); the 1 mm dummy watch layer against the zero-layer bug (`:366-373`) | README: "run alongside VR games … with as little performance impact as possible … rendering techniques are kept as simple and efficient as possible" (`README.md:9`) |
| **xrdesktop / gxr** | `XR_EXTX_overlay` | 1 ("TODO: session layer placement should be configurable", `gxr-context.c:894-904`) | **zero layers** when `!shouldRender` (`gxr-context.c:2132-2141`); background hidden when the main session is visible (`gxr-demo.c:532-535`, `xrd-shell.c:1728-1732`); the scene-vs-overlay client split is gone from this pin (docs still name it) | not recoverable from the pin |
| **WiVRn lobby** | **not an overlay** — the main scene, replaced by the stream scene when an app connects (`lobby.cpp:896-897`, `application.cpp:1702-1734`) | — | client sleeps 250 ms per loop only when the session is not running ("Throttle loop since xrWaitFrame won't be called", `application.cpp:1703-1712`) | — |
| **kwin-vr** | `XR_EXTX_overlay` via its Qt patch | 20, configurable (`kwinvr.kcfg:83-86`) | none: the whole desktop scene keeps rendering over the game; the only cost comments concern thumbnails | patch message: "allows OpenXR applications to be rendered on top of other OpenXR applications" |
| **OpenComposite / xrizer** (what games expect) | overlays become quads **inside the game's own session** (`BaseOverlay.cpp:91-156`, `XrBackend.cpp:536-539`; `overlay.rs:186-269`) | — | invisible overlays skipped (`BaseOverlay.cpp:118-120`) | the **dashboard is stubbed**: `IsDashboardVisible` always false, `ShowDashboard` `STUBBED()`/`todo!()` (`BaseOverlay.cpp:783-807`, `overlay.rs:837-868`) — SteamVR-era games treat system UI as the shell's; OpenVR's own contract: `VREvent_OverlayHidden` "doesn't need to render frames" (`openvr-2.5.1.h:842-843`) |

Converging: the shells stay an overlay session at all times and pay the frame loop; they save by
**not rendering** (dirty tracking, stale re-submit, zero layers) and by hiding their own
backdrop when a game is visible. None stops its session. None runs at a reduced rate.

### 1.4 The 2D analogue — a small overlay over a fullscreen game

| compositor | cheap path | when a small overlay appears | reason stated |
|---|---|---|---|
| **gamescope** | direct scanout of the game on the primary plane, **overlays on hardware planes via libliftoff**; GPU compositing only when a condition forces it or liftoff fails (`DRMBackend.cpp:3902-3983`, `3090-3094`) | the Steam overlay / notification is a *layer*, not a force-composite condition; a static overlay does not repaint (overlay commits set `hasRepaintNonBasePlane`, `steamcompmgr.cpp:7922-7944, 10613-10615`); a blank texture is kept on the overlay plane "to avoid stutter when toggling the overlay on" (`:3331-3357`) | partial composition ("keep the game on its plane, composite only overlays") exists but is disabled "until we get composite priorities working in libliftoff" (`gamescope_shared.h:66-69`); "changing [LUTs/blend] in DRM … is incredibly expensive!! … This avoids stutter" (`:4005-4012`) |
| **mutter / GNOME Shell** | direct scanout of an unobscured fullscreen window (`meta-compositor-view-native.c:128-207`) | every piece of shell chrome **inhibits unredirect for as long as it is up** — OSD (~1.5 s), notification banners, overview, modal grabs, OSK, close dialog, popups (`compositor.c:1335-1382`; gnome-shell `osdWindow.js:95/130`, `messageTray.js:1149/1290`, `overview.js:252-254`, …) | "reduces the overhead for apps like games" (`compositor.c:1359`) — accepted as a transient |
| **niri** | primary-plane scanout of a focused, stationary fullscreen window; overlay planes **off by default** — "cause weird performance issues on my system" (`tty.rs:1917-1934`) | the Top layer hides under a focused fullscreen window; the **Overlay** layer is always composited above it (`scrolling.rs:2931-2944`, `niri.rs:4510-4512`; wiki "if you want notifications … over fullscreen windows, configure … the overlay layer") | design principle: fullscreen hides the shell |
| **KWin** | per-item direct scanout with overlay `OutputLayer`s; effects may block it (`workspacescene.cpp:390-416`, `compositor.cpp:461-495, 720-723`) | a notification effect declares it does not block scanout (`SlidingNotificationsEffect::blocksDirectScanout → false`) | the cursor "always treated as the highest priority item for an overlay" |

The 2D world's answer to "small overlay over a game" is **hardware planes** (gamescope, KWin,
niri opt-in) — the runtime-composited quad layer is the XR equivalent, and Quest's compositor
is the XR equivalent of liftoff. Where planes are unavailable (mutter), compositing for the
overlay's lifetime is accepted as a transient — the same acceptance zxr makes for the summoned
shell (§7).

### 1.5 Matrix — issue × position

| issue | Monado | Quest [ext] | SteamVR [ext] | WayVR | xrdesktop | gamescope | mutter | → Mura |
|---|---|---|---|---|---|---|---|---|
| 1 fast path lost by any overlay layer | yes (merged count) | no such cliff: per-layer 0.1 ms | overlays squashed; in-distortion path removed | pays it | pays it | planes avoid it | composite for the overlay's life | zero layers when quiet; few when summoned; no in-distortion patch |
| 2 cutout over games | — | system composites hands above content | — | — | — | — | — | §3: layer only while a hand is in view; default = owner item |
| 3 quiet loop | no idle mode | — | — | WaitFrame always | WaitFrame always | vblank-gated paint | — | §4: loop vs recreate, owner item |
| 4 zero layers | fixed (!2769) | 0-α layers still cost — destroy them | `OverlayHidden` = stop rendering | dummy layer (historical) | zero layers | — | — | zero layers; placeholder forbidden |
| 5 clients while quiet | — | — | — | — | — | overlay commits don't repaint the base | — | §6 implemented |
| 6 summoned footprint | — | 16 layers, merge head-locked quads | — | — | — | few planes | — | one panel + one per notification |
| 7 display time | same for all clients | — | — | — | — | — | — | §7.3 |

## 2. Issue 1 — the fast path and what an overlay layer costs the game

### 2.1 Measured (host): xrgears alone vs with zxr's overlay session

Medians of three 20 s runs, real Monado compositor, head rotating (`fdinfo` GPU ms/s per
process; ±5 ms/s noise band):

| configuration | Monado GPU ms/s | Monado CPU ms/s | xrgears GPU / CPU ms/s | zxr CPU ms/s | zxr GPU ms/s | Monado frame shape |
|---|---|---|---|---|---|---|
| xrgears alone | **32** | 30 | 25 / 27 | — | — | 1 layer → fast path |
| + zxr overlay, quiet (0 layers) | 37 | 35 | 20 / 29 | **8** | 0 | still 1 layer → fast path (§5) |
| + zxr, 1 quad (foot) | 37 | 34 | 27 / 25 | 11 | 0 | 2 layers → squasher |
| + zxr, 4 quads | 37 | 45 | 29 / 31 | 20 | 0 | 5 layers → squasher |
| + zxr, 16 quads | **52** | 43 | 29 / 27 | 28 | 0 | 17 layers → squasher |
| + zxr, full-view projection layer (`--debug-panels projection`, 1 plane) | 34 | 37 | 25 / 26 | 25 | 9–14 | 2 layers → squasher |

Reading: on this desktop GPU the squasher's scratch round trip at 896×1007 is **inside the
noise** for 1–4 layers (+0–5 ms/s ≈ ≤ 0.08 ms/frame at 60 Hz); it shows at 16 quads (+20 ms/s ≈
0.33 ms/frame, ≈ 0.02 ms per quad — Quest publishes 0.1 ms per layer on a Quest 2, five times
this desktop GPU's figure, which is the order of magnitude a tiler should be expected to show).
Monado's CPU rises ~5 ms/s per connected overlay client regardless of layers (its per-client
thread, 450 wake-ups/s here — §4). **Hardware-deferred:** the scratch round trip's cost on a
tiler at 2 × 1832×1920 — analytically 28 MB stored + 28 MB read per frame, 5 GB/s at 90 Hz, the
number Quest's 0.6 ms fullscreen-layer figure is the closest published proxy for.

### 2.2 The "fast path with overlays" candidate — contradicted by SteamVR's history

Valve shipped exactly this (overlay composited in the distortion pass) and removed it for not
scaling (§1.1). Quest's single-pass compositor is the other shipping answer, but it is the
*whole* compositor's design, not a fast path bolted onto a squasher. Monado's history (§1.2)
shows the fast path was added to avoid one copy for the single-projection case and the squasher
was kept for everything else; the distortion pass has one colour source per view in both
back-ends, so "N quads inside distortion" means new descriptor layouts, a per-layer UBO and a
new distortion shader per back-end — the read's estimate is *large*, and it would be a second
squasher in disguise. **Position (determination):** not a Mura-carried patch and not an upstream
ask; the lever zxr owns is the **layer count** — zero layers while quiet (§5), few layers when
summoned (§7), a cutout layer only while a hand is in view (§3) — which is also what WayVR,
gxr, gamescope and mutter do in their own idioms (§1.3–1.4).

## 3. Issue 2 — the cutout over games: shape, lifetime, default

**Stand-ins measured (host, 20 s each, ±5 ms/s):** beside xrgears, two 300×300 quads as the
per-hand billboards (shape ii) — Monado GPU **42 ms/s** steady; quiet (no layer, "hands out of
view") **37**; the billboards at a 50 % on/off duty cycle **39**; a full-view layer (shape i,
§2.1 row 6) **34–45**. On this GPU all four are within the noise band of the game alone (32);
what the numbers show is the *shape* of the cost: it exists only while the layer exists, and a
lifetime rule that drops the layer when no hand is in view returns the fast path for that
fraction of time.

**Analytic (device):** the cost of any cutout layer over a game = the squasher round trip
(one scratch store + load per eye per frame; Quest's published proxy is 0.6 ms for a fullscreen
layer, 0.1 ms flat per layer on a Quest 2) **plus** the layer's own sampling: shape (i) a
full-view layer per eye (Quest: the 0.6 ms class), shape (ii) two ~384² quads (Quest: the 0.1 ms
class). Both pay the round trip; (ii) pays almost nothing on top. Quest also warns that a
0-alpha layer costs in full — the lifetime rule must **destroy** (submit nothing), not fade.

**The lifetime rule (determination — Quest's "destroy layers you don't need" and WayVR's
zero-render idle converge):** the cutout layer exists only while the perception service reports
a non-empty matte; with no hand in view zxr submits no cutout layer and the game is back on the
fast path. The fraction of a gaming session with hands in the camera view is unknown here
(hardware-deferred: the first device with the matte pipeline measures it); the cost of
on-by-default is that fraction × the round trip, not the whole session.

**The default — recorded, not ruled (owner 2026-09-26).** The options and their costs are
recorded here; the decision is taken on the first device that runs the real matte pipeline over a
real game — the stand-ins above are billboards, not segmentation, and the hands-in-view fraction
that prices "on" cannot be measured without hands in a camera. Q-D(b) stands as written until
then; the reserved input (native-openxr-apps §6) is the guarantee of summon and quit under every
option, so nothing below can strand the wearer. The deferral is placed in
[implementation-path.md §5.1](../architecture/implementation-path.md).

| Default over a game | Host stand-in (Monado GPU ms/s, xrgears; game alone 32, ±5) | Device measurement still owed | What it trades |
|---|---|---|---|
| **Off** — no cutout layer; the wearer turns hands on per game from the summoned shell (OSD toggle) | 32–37 (fast path kept for the whole session) | none beyond the baseline | hands invisible in immersive games until the wearer asks; every other platform's opt-in posture except visionOS |
| **On, always** — cutout layer whenever the game is primary | 42 (two 300² billboards, shape ii, steady) | round trip on the tiler at panel resolution; shape (i)/(ii)/(iii) per perception-passthrough-hands §1a | hands always visible; the game is on the squasher for the whole session |
| **On, with the lifetime rule** — cutout layer exists only while the matte is non-empty | 37 with no hand in view, 39 at a 50 % duty cycle | the hands-in-view fraction of a gaming session; matte latency to layer create/destroy | fast path lost only while hands are in view; cost = fraction × round trip; the layer must be destroyed, not faded (Quest's 0-alpha warning) |

Whichever "on" is chosen, shape (ii) with the lifetime rule is the engineering candidate; the
lifetime rule itself is a determination (above) and applies to any "on". The choice between the
rows is the owner's, made with the device numbers in the third column filled in.

## 4. Issue 3 — the quiet frame loop vs a stopped session

**Measured (host):** quiet zxr beside xrgears: zxr 8 ms/s CPU (median of three), 629 loop
wake-ups/s (≈ 10/frame: the 5 RPCs of a zero-layer tick — `xrWaitFrame` ×2 on the wait thread,
begin, end, poll — plus the tick and Monado's replies), 0 GPU; Monado +5 ms/s CPU and one
service thread at ~450 wake-ups/s for the extra client. Per hour at 60 Hz that is ≈ 30 s of
zxr CPU and ≈ 18 s of Monado CPU on this host; at 90 Hz ×1.5, on device cores more.

**The alternative — no session while quiet.** An app cannot voluntarily end a running session:
`xrEndSession` is legal only after `STOPPING`, which the runtime sends (spec `session.adoc`), so
"stop the loop" means **destroy the session and recreate it on summon**. Measured (host) from
three runs: Vulkan device selected → session + swapchains created → `READY` → `FOCUSED` takes
**37–41 ms**, then the first frame lands at the next display period (≤ 16.7 ms at 60 Hz): a
summon of ≈ 45–60 ms before zxr's first layer is visible, against the platforms' dashboard
summon feel [external, ≈ 100–200 ms]. **Hardware-deferred:** the same interval on the device
(swapchain allocation is the variable part). **Comparables (§1.3):** every overlay shell keeps
its session and its `xrWaitFrame` loop while a game runs — WayVR, gxr, kwin-vr — and none
recreates it on summon; the 2D analogue (mutter, gamescope) likewise keeps the compositor
running and pays for what it draws, not for existing. **Determination:** keep the quiet loop
(zero layers, 5 RPCs, ≈ 8 + 5 ms/s here); session recreation is recorded as the measured
alternative with no precedent behind it — a rethink candidate if the device's per-hour number
turns out large, not a design.

## 5. Issue 4 — zero layers works; the transparent-quad fallback is forbidden

**Measured (host):** zxr's overlay session submitting `layerCount == 0` beside xrgears for 852
ticks — the game's picture intact, no ghost of zxr's last frame, Monado's frame single-layer
(fast path kept; §2.1 row 2). **Why (read):** a zero-layer `xrEndFrame` is
`xrt_comp_discard_frame` (`oxr_session_frame_end.c:1840-1852`), and the multi-compositor
retires the client's delivered frame (`comp_multi_compositor.c:609-623`) — "Need to drop
delivered frame as it shouldn't be reused." WayVR's "Monado freaks out if no layers are
submitted" workaround (`mod.rs:367-373`) is historical on this Monado; adopting it would
silently reinstate the squasher for every frame. **Determination:** zero layers is the quiet
shape; a transparent placeholder layer is forbidden.

## 6. Issue 5 — clients committing while zxr is quiet

**Implemented on the branch (the 2D compositors' unredirect behaviour):** in quiet mode zxr
walks no surface trees, updates no textures, holds no client buffers (smithay releases at
replacement), runs no panel or projection pass, and frame callbacks fall to the ~1 s fallback
cadence. **Measured (host):** vkcube in MAILBOX (≈ 1.9 k commits/s) beside xrgears — zxr
**117–137 ms/s → 55 ms/s** when quiet; panel passes 0; wake-ups unchanged at ≈ 3.1 k/s because
each commit is still a protocol dispatch (the per-commit acquire cost of research/61, ≈ 21 µs).
What remains is the protocol's: a client that ignores frame callbacks keeps committing at its
own rate, and no compositor can stop it short of not reading its socket. **Position:** the
implemented rule is the whole of what zxr can do; the residual is the client's.

## 7. Issue 6 — the summoned shell's footprint

**Measured (host), §2.1 rows:** 1 quad costs the squasher path (noise here, ≈ 0.1 ms on Quest);
4 quads no more than 1; 16 quads +0.33 ms/frame on this GPU (Quest: ≈ 1.6 ms). **Rule
(determination, from Quest's published per-layer cost and gamescope/mutter's few-surfaces
posture):** the summoned scene is one panel for its own UI, one quad per notification, one for
the affordance — a handful, never the desktop; layer 5 surfaces that are not visible are
*destroyed*, not hidden (Quest: 0-alpha layers cost in full).

### 7.3 The small items (issue 7)

- **Overlay display-time correlation:** the extension deliberately dropped the synchronized-
  display-time bit (rev 5); on Monado both sessions are paced by the same compositor pacer
  (`comp_multi_system.c:566-594`) — measured correlation is §1's read item; the panel-pose
  consequence is nil either way because the runtime re-samples quads at display time.
  On Monado, overlays receive the **same** `predicted_display_time_ns` as the main session — one
  value per system frame is broadcast to every client's pacer (`comp_multi_system.c:375-420,
  560-592`) — so zxr's quad poses are located for the same instant the game renders for.
- **`libmonado` client list:** `mnd_root_update_client_list` is one IPC snapshot
  (`monado.c:264-274`); there is no change notification (`monado.h:217-258`). **Determination:**
  poll at 1 Hz while a game is primary and on every reserved-input press and session-state event
  zxr itself receives — the shell never needs to learn of a new client faster than a wearer can
  act on it. Cost: one RPC per second.
- **`io_blocks` vs a focus switch:** one IPC to set (`monado.c:393-409`), then a flag check in
  the service's input/pose/output paths (`ipc_server_handler.c:2059, 2084, 2395`) — no per-frame
  cost to zxr. The upstream `set_focused_client` remains the correct shape (research/66 §14);
  WayVR uses `io_blocks` for the same purpose today (`blocker.rs:33-97`).
- **Blend over an opaque game:** the squasher blends every layer by its source alpha in
  submission order (`render_gfx.c:409, 767-776`, `comp_render_gfx.c:236, 848-867`); zxr's quads
  and a cutout layer composite over the game's opaque projection layer with their own alpha. The
  environment blend mode stays the game's (`comp_multi_system.c:227-250`) — zxr cannot ask for
  passthrough behind an opaque game without being the focused client, as designed (§2 of the
  design).
- **Placement value:** WayVR 5, gxr 1 ("should be configurable"), kwin-vr 20 configurable — no
  comparable states a reason; zxr's placement is a stand-in until a second Mura overlay exists.

## 8. Findings by label

**Measured (host):** §2.1 table; quiet loop 8 ms/s zxr + 5 ms/s Monado; summon-by-recreation
45–60 ms; zero layers keeps the fast path; quiet mode halves zxr's CPU under a committing client.
**Analytic:** the tiler scratch round trip (28 + 28 MB per frame at XR2-class); Quest's
published per-layer figures as the device-side proxy.
**Hardware-deferred:** the squasher's cost on a tiler; session recreation time on device; the
cutout stand-in's cost at panel resolution.

## 9. Determinations and owner items

Determinations: zero layers while quiet, placeholder forbidden (§5); no fast-path-with-overlays
patch or ask (§2.2); the quiet loop kept, recreation recorded as the alternative (§4); the
quiet-mode client rule as implemented (§6); the summoned footprint rule (§7); the cutout
lifetime rule (§3); `libmonado` polled at 1 Hz + on events (§7.3). **Owner item, recorded and
held** (rule 8; options and costs tabled in §3): the cutout default over games — the owner
ruled on 2026-09-26 that it is decided on the first device with the real matte pipeline over a
real game, not on the host stand-ins; the deferral lives in implementation-path §5.1.
