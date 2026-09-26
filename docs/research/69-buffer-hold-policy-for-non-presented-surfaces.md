# 69 — The buffers of a surface zxr is not sampling: release moment, from comparables and measurement

**Research date:** 2026-09-26. **Question:** research/67 §6 found that while zxr is quiet (a native
OpenXR app is Monado's primary) a Vulkan MAILBOX client that ignores frame callbacks *doubles* its
commit rate, because zxr releases every buffer at replacement and the client always has a free
swapchain image; zxr's CPU went 290 → 488 ms/s. The only back-pressure such a client can feel is a
withheld `wl_buffer.release` (or its explicit-sync release point). Whether zxr should withhold it,
and until when, was left open (spec §7 rev 3.2; research/67 §9) rather than patched from one number.
This pass answers it the AGENTS.md rule-7 way: what the 2D compositors do with the buffers of a
surface they are not presenting and *why*, what the protocol says, what the client side actually
does while it waits, then an A/B on zxr — and the determination follows from all of it.

**States in scope.** (i) *quiet* — a game is primary, zxr composes nothing; (ii) *not composed while
zxr presents* — on the arenas build this is only an **unmapped member or a member in a hidden place**
(window-workspace-management.md "hidden"): an out-of-view member that commits is dirty, is walked,
gets its panel pass and holds its buffers to the fence, because the runtime samples the quad
wherever the head points and skipping the pass would show a stale tick when it swings back
(`main.rs` dirty walk has no frustum term; only frame callbacks are visibility-gated); (iii)
*composed* — today's hold-until-fence (§6.5), the control. Whether the frustum should also gate the
panel pass is **not decided here** (§4.1).

**Method.** Comparables from the pinned clones in `references/` (all shallow, so reasons are
in-tree comments plus [external] upstream sources where marked). Bench: xrgears as Monado's primary,
zxr as the `XR_EXTX_overlay` session (research/67 harness), clients taken from the dev-session PATH
(`fa52b2d`, `d73a580` — never a store path), GPU logged per run (`AMD Radeon 8060S, RADV`), 10–15 s
per state, `/proc` CPU and `schedstat` wake-ups, zxr's journal. Scripts under
`/run/media/j/tinystore/tmp/bh-*.sh`.

## 0. The realistic bound (measured, host)

The research/67 row was a *trivial* renderer (vkcube: 0.05 ms of GPU work per frame). What a client
that ignores frame callbacks costs zxr while quiet, by client shape, release at replacement (today):

| client (all from the dev-session PATH) | not quiet (composed) | quiet | quiet commits/s vs composed |
|---|---|---|---|
| vkcube MAILBOX, Vulkan WSI, trivial | 289 ms/s at 13.9 k commits/s (21 µs/commit) | **500 ms/s at 29.5 k** | ×2.1 — release-bound |
| vkmark `shading:phong` 1280×720 MAILBOX (still trivial on this GPU: 13–17 k fps) | 257 at 11.3 k (22.8 µs) | 495 at 19.1 k (26 µs) | ×1.7 |
| glmark2 `terrain` 1920×1080, EGL swap interval 0 (**GPU-bound: 1.1–1.4 k fps**) | 453 at 1 161 (390 µs/commit — §4.2) | **49 ms/s at 1 358 (36 µs)** | ×1.17 — GPU-bound |
| glmark2 `terrain` FIFO (respects frame callbacks) | 20 at 59 | **5 at 1** | idles at the fallback cadence |

The doubling exists only for a client whose frame costs less than a dispatch. A GPU-bound client's
rate is set by its GPU work, not by the release moment: +17 % under quiet, 36 µs of zxr per commit,
≈ 3.6 ms/s per 100 fps. A real game at 100–300 fps ignoring frame callbacks therefore costs zxr
4–11 ms/s while quiet — a budget line, not a design driver. The 29 k-commits/s row is the adversarial
bound and the client itself pays 915 ms/s of CPU to reach it.

## 1. Comparables — what is released when, and why

### 1.1 The release moment (dmabuf, composited, no direct scanout)

Three real positions exist; none is chosen *as* a policy toward the client — each falls out of who
holds a reference:

| compositor | previous buffer released… | mechanism (file:line) | stated reason |
|---|---|---|---|
| **wlroots** (+sway, river) | **at the replacing commit** | `types/wlr_compositor.c:441-444` new `wlr_client_buffer`, old unlocked; the scene drops its lock synchronously in the commit listener (`types/scene/surface.c:360-364`); render passes lock only the *target* (`render/gles2/pass.c:332`) | "Release the buffer after emitting the commit event… Don't leave the buffer locked so that wl_shm buffers can be released immediately on commit when they are uploaded to the GPU" (`wlr_compositor.c:567-572`); correctness of the early dmabuf release rests on **kernel implicit sync** |
| **smithay** (niri, cosmic-comp, zxr) | **at the replacing commit** | `RendererSurfaceState::update_buffer` overwrites the `Arc<InnerBuffer>` (`backend/renderer/utils/wayland.rs:168-178`); `InnerBuffer::drop` sends `release` and signals the syncobj release point (`:68-79`); "The release fence is signalled when all references to a `Buffer` are dropped" (`wayland/drm_syncobj/mod.rs:13-14`) | no reason stated; fence-bound explicit release is *left to the consumer* (`sync_point.rs:190-200` `import_sync_file` — nothing in smithay calls it) |
| **mutter** | **at the replacing commit** (`buffer_held`) | `meta-wayland-surface.c:915-937`; use count to zero → `wl_buffer_send_release` (`meta-wayland-buffer.c:691-705`) | "If the newly attached buffer is going to be accessed directly without making a copy, such as an EGL buffer, mark it as in-use don't release it until is replaced by a subsequent wl_surface.commit or when the wl_surface is destroyed" (`:929-933`); shm is copied at attach and released at once |
| **KWin** | **at the replacing commit** | `SurfaceInterfacePrivate::bufferRef` and `SurfaceItem::m_bufferRef` both reassigned at commit (`wayland/surface.cpp:648-652`, `scene/surfaceitem_wayland.cpp:102-105`); `GraphicsBuffer::unref` → `released()` (`core/graphicsbuffer.cpp:46-59`); the GL texture takes **no** ref (`scene/opengl/texture.cpp:243-245`) | "While the reference exists, the graphics buffer cannot be destroyed and the client cannot modify it" (`graphicsbuffer.h:144-146`); no reason for the unfenced dmabuf release — not found |
| **Hyprland** | at the replacing commit / state merge (dmabuf); at commit after upload (shm); render refs to the EGLSync | `SurfaceState.cpp:185, 201-207`; `GLRenderer.cpp:124-160` | "dmabuf doesnt drop it until we recieve a new one"; without explicit sync "release all buffer refs and hope implicit sync works" (`GLRenderer.cpp:132`) |
| **Louvre** | at the commit of a *different* buffer | `LSurfacePrivate.cpp:948-961` | "Unlike SHM buffers, the current buffer is released after a different buffer is commited" (`:253-254`) |
| **weston** | **at the next repaint that samples the newer buffer** | core ref downgraded after each repaint (`compositor.c:3719-3726`); gl-renderer holds `gs->buffer_ref` until `attach` of a different buffer (`gl-renderer.c:4350-4351, 4391-4393`) | "drop the core reference now, and allow early buffer release. This enables clients to use single-buffering" (`compositor.c:3708-3713`) — the *hold* is the renderer's, the *policy* is early release |
| **Mir** | when the compositor acquires a newer buffer; an unconsumed one at replacement | `multi_monitor_arbiter.cpp:94-135` | "no buffer is going to be released back to the client till both of those containers get destroyed (end of the function)" (`default_display_buffer_compositor.cpp:77-84`) |
| **gamescope** | when the *next fenced* commit retires it; GPU/scanout refs to completion | `commit_t` owns the `wlr_buffer` lock (`commit.h:41`, `commit.cpp:31`); `handle_done_commit` erases all older commits (`steamcompmgr.cpp:7982-7986`) | "Committing without buffer state… Mutter and Weston have forward progress on the frame callback in this situation, so let the commit go through" (`wlserver.cpp:226-228`) |

Explicit-sync release *points* are fence-bound where the compositor sampled the buffer (weston
`gl-renderer.c:2839-2902`; wlroots merger `drm_syncobj_merger.c:60-79`; KWin `itemrenderer_opengl.cpp:86-93`;
Hyprland `SyncReleaser.cpp:12-22`; mutter imports Cogl's latest sync fd, `meta-wayland-buffer.c:652-688`)
and signalled at once where it never was — the protocol anticipates exactly that: "compositors may
release buffers without ever reading from them" (`linux-drm-syncobj-v1.xml:210-234`). Direct scanout
holds until the flip everywhere ((c)-like); zxr has no scanout path.

### 1.2 What changes for a surface that is not being presented

**Nothing about buffers, everything about frame callbacks.** In all eleven, buffer release is a
refcount consequence that never consults visibility (wlroots `surface_reconfigure` runs on every
commit "regardless of enabled state"; mutter and KWin update the refs on commit "regardless of
visibility"; smithay `update_buffer` likewise). Visibility acts on `wl_surface.frame`:

| compositor | frame callbacks for hidden / minimized / other-workspace surfaces | stated reason |
|---|---|---|
| weston | **withheld** (occluded paint nodes skipped, `compositor.c:4186-4192`; minimized = unmapped view) | [external] Paalanen 2018: "there is no point for the client to update surface as it is not visible… when the surface becomes visible again, it would take a frame cycle to have the client send an updated buffer" |
| wlroots / sway / river | **withheld** (`frame_done` only to enabled nodes with non-empty `node.visible`, `wlr_scene.c:1091-1096, 2642-2646`) | "otherwise the frame done events would never reach the surface anyway" (`scene/surface.c:377-388`) |
| Mir | **withheld** (`on_consumed` never fires while occluded; callbacks accumulate, `wl_surface.cpp:492-494`) | Firefox heartbeat exception for callbacks without a buffer (`:580-603`) |
| Louvre | **withheld** (default `paintGL` skips unmapped/minimized, `LOutputDefault.cpp:83`) | "If not called, the given surface should not update its content" (`LSurface.h:527-534`) |
| Hyprland | **withheld** for invisible workspaces; opt-in `renderunfocused` at 15 fps (`Renderer.cpp:188-222`, `ConfigValues.cpp:594`) | per-window opt-in "for background rendering" |
| mutter | **withheld** (not view-primary, `meta-wayland.c:181-220`; `is_view_primary` false off-stage, `meta-surface-actor-wayland.c:115-123`); **flushed once on configure** | "Clients might be waiting for frame callbacks before updating, so to unblock them even if we're not normally sending them e.g. due to being hidden, force flush any pending ones after having sent the configuration" (`meta-window-wayland.c:223-227`) |
| KWin | **withheld** (`Item::scheduleFrame` returns for `!isVisible()`, `item.cpp:499-517`); synthesised at refresh rate only for thumbnails/screencast (`window.cpp:4408-4442`) | "to be able to paint hidden items for things like screncasts or thumbnails" (`item.cpp:723-724`) |
| smithay (helper) / niri / cosmic-comp | **throttled to ≈ 1 Hz** (995 ms) for surfaces on no primary output; niri adds a 1 Hz fallback timer over *all* windows; cosmic-comp 60 Hz for captured windows | [external] cmeissl (smithay #1908): "some clients might get blocked in their main loop and become unresponsive when never receiving frame callbacks. The 1fps was taken as the minimum update rate from x11"; [external] niri `ed8a6afe`: "gamescope + Minecraft with NeoForge throws an error upon starting if there are no frame callbacks… Veloren disconnects from server with VSync and no frame callbacks" |
| gamescope | identical to the focused window (vblank latch + FPS limiter, `steamcompmgr.cpp:10336-10365`) | none stated; Steam suspends unfocused games, not the compositor |

Two of them add the protocol's own word for "not being repainted": **KWin sets
`xdg_toplevel.suspended`** on the visibility change (`scene/windowitem.cpp:195-203` →
`xdgshellwindow.cpp:772-781`) and **mutter sets it 3 s after the window hides**
(`core/window.c:110` `SUSPEND_HIDDEN_TIMEOUT_S 3`, state machine `:2286-2335`, inhibited while the
actor is mapped, `meta-window-actor.c:160-172`). xdg-shell v6: "The surface is currently not
ordinarily being repainted; for example because its content is occluded by another window, or its
outputs are switched off due to screen locking" (`stable/xdg-shell/xdg-shell.xml:909-914`). wlroots
(`wlr_xdg_toplevel.c:101-102`) and smithay (`shell/xdg/mod.rs:168, 966`) carry the state for their
compositors to set.

**Deliberate back-pressure by withholding releases: none, in eleven compositors.** Hyprland's one
hold on invisible surfaces is the fifo-v1 barrier state (`render:not_shown_fifo_lock`, default
`always`, `ConfigValues.cpp:672-675`) — and the option exists because it stalls hidden clients.
weston's and Mir's "last sampled buffer stays held while hidden" is a side-effect they did not
design for (a 2-buffer client stalls, ≥ 3 free-run).

### 1.3 The protocol

- `wl_surface.attach`: "The compositor may access the pixels at any time after the wl_surface.commit
  request. When the compositor will not access the pixels anymore, it will send the
  wl_buffer.release event" — no deadline either way [external, wayland 1.25 `wayland.xml:1537-1563`].
- `wl_surface.frame`: "A server should avoid signaling the frame callbacks if the surface is not
  visible in any way, e.g. the surface is off-screen, or completely obscured by other opaque
  surfaces" (`:1612-1637`) — **frame callbacks are the protocol's throttle for invisible surfaces**.
- `fifo-v1` `wait_barrier`: "If the surface is not being updated by the compositor (off-screen,
  occluded) the compositor may ignore the constraint. Clients must use an additional mechanism such
  as frame callbacks or timestamps to ensure throttling occurs under all conditions"
  (`staging/fifo/fifo-v1.xml:110-124`) — **throttling of an invisible surface is the client's duty.**
- `linux-drm-syncobj-v1`: while the surface has a syncobj object "the delivery of wl_buffer.release
  events… becomes undefined" (`:140-143`); the release point is signalled "when it has finished its
  usage of the buffer… for the relevant commit" and "compositors may release buffers without ever
  reading from them" (`:210-234`).

### 1.4 The client side — what a blocked client does [external, Mesa main]

- **Vulkan WSI** (`src/vulkan/wsi/wsi_common_wayland.c` at `9f42ea12`, 2026-09-16): MAILBOX uses
  **4 images** ("1) One to scan out from 2) One to have queued for scan-out 3) One to be currently
  held by the Wayland compositor 4) One to render to", `wsi_wl_surface_get_min_image_count`), never
  requests `wl_surface.frame` for pacing and never uses fifo-v1 barriers; it paces **solely** on the
  buffer pool draining via `wl_buffer.release` (implicit) or the release timeline point (explicit,
  `WAIT_AVAILABLE`). Acquire **blocks** — `wl_display_dispatch_queue` or `timeline_wait` — with no
  socket traffic beyond the dispatch. FIFO with fifo-v1 blocks in acquire; legacy FIFO blocks in
  present on the frame callback.
- **EGL** (`src/egl/drivers/dri2/platform_wayland.c` `wait_for_free_buffer`): when no colour buffer
  is free the client **spins on `wl_display_roundtrip_queue`** — "not all servers flush on issuing a
  buffer release event. So, we spam the server with roundtrips as they always cause a client
  flush." Every roundtrip is a `wl_display.sync` the compositor must answer.

That last line is the decisive engineering fact of this pass (§2).

## 2. The A/B on zxr (measured, host)

Worktree `zxr/buffer-hold`: `--debug-hold replacement|tick|callback|fence` selects the release
moment for buffers committed to a surface zxr will not sample this tick (quiet, hidden, unmapped);
the hold is taken in the commit handler (the one place a non-sampled buffer is ever held) and
released at the top of the next tick (`tick`), when the member next receives its fallback frame
callback (`callback`, ≈ 1 Hz), or with the slot fence like a sampled buffer (`fence`, two ticks).
`hide on|off` puts the focused member in the hidden state (excluded from the flatten and the dirty
walk; fallback-cadence frame callbacks). Composed members are untouched — the §6.5 fence hold is the
control. Clients: vkcube FIFO (respects frame callbacks), vkcube MAILBOX (Vulkan WSI, trivial),
glmark2 `terrain` 1080p swap-interval 0 (EGL, GPU-bound, ≈ 1.1 k fps). 10 s per state; zxr CPU ms/s,
loop wake-ups/s, commits/s, most buffers held at once.

| policy | client | composed (control) | **quiet** | **hidden** | held at once |
|---|---|---|---|---|---|
| replacement | vkcube FIFO | 24 ms/s, 60 c/s | 8, 1 c/s | 5, 1 c/s | 0 |
| replacement | vkcube MAILBOX | 268, 12.7 k | **538, 22.3 k** | 517, 16.6 k | 0 |
| replacement | glmark2 EGL | 457, 1 059 (75 k wake/s) | **59, 1 422** | 62, 1 431 | 0 |
| tick | vkcube FIFO | 24, 60 | 13, 1 | 9, 1 | 1 |
| tick | vkcube MAILBOX | 320, 11.7 k | **17, 240** | 25, 240 | 4 |
| tick | glmark2 EGL | 446, 1 049 | **445, 235 (58 k wake/s)** | 452, 235 | 4 |
| callback | vkcube FIFO | 30, 60 | 10, 1 | 10, 1 | 1 |
| callback | vkcube MAILBOX | 387, 9.6 k | **7, 4** | 7, 4 | 4 |
| callback | glmark2 EGL | 457, 1 094 | **547, 4 (106 k wake/s)** | 554, 4 | 4 |
| fence | vkcube FIFO | 19, 60 | 6, 1 | 7, 1 | 1 |
| fence | vkcube MAILBOX | 265, 16.8 k | 9, 120 | 8, 120 | 4 |
| fence | glmark2 EGL | 445, 1 041 | 501, 119 (84 k wake/s) | 516, 119 | 4 |

Readings:

- **FIFO: every policy is a no-op** — 1 commit/s, 1 buffer held, 5–13 ms/s. A client that waits for
  frame callbacks never has a second buffer in flight; the fallback cadence is the whole mechanism.
  This is the client shape of every toolkit and of every game that vsyncs.
- **Vulkan MAILBOX: holding works as the arithmetic says** — `tick` bounds the client to N−1 = 4
  commits per tick (240/s), `fence` to 120/s, `callback` to 4/s; zxr 7–25 ms/s instead of 517–538;
  the client's own CPU falls from 915 to 15 ms/s because Mesa's WSI blocks properly in acquire.
- **EGL MAILBOX-shaped (swap interval 0): holding backfires, badly.** The commit rate is bounded as
  intended (235/s, 4/s) but zxr's CPU *rises* to 445–554 ms/s with 58–106 k wake-ups/s, against
  59 ms/s when releasing at replacement. `strace -c` over the run: ≈ 8 k/s each of `sendmsg`,
  `epoll_ctl`×2, `recvmsg`×4 (half `EAGAIN`), `timerfd_settime`, `read(EAGAIN)`, `ppoll` — a loop
  iteration per client roundtrip. It is Mesa's `wait_for_free_buffer` (§1.4): a blocked EGL client
  spams `wl_display.sync`, and the compositor holding its buffers pays for every one. **The stronger
  the hold, the worse the cost**: `callback` holds the longest and costs the most.
- **Staleness on return** (cost of holding, as frames): under every policy the buffer composed on
  return is the client's *latest* commit (0 frames behind), but its age is bounded by the throttle
  interval — ≤ 4 ms under `tick`, ≤ 8 ms under `fence`, **≤ 250 ms under `callback`** (4 commits/s),
  fresh under `replacement`. Pinned memory: 4 × the buffer (33 MB at 1080p RGBA) per held client
  under `tick`/`callback`/`fence`; 0 under `replacement`. Both release paths were exercised: vkcube
  is explicit-sync (release timeline point signalled on `Buffer` drop), glmark2's release went
  through `wl_buffer.release`; the throttle worked identically on both — the difference between the
  Vulkan and EGL rows is the client's wait loop, not the release path.

## 3. Determinations (rule 8: converging comparables with reasons that transfer, and the numbers agree)

1. **Release at replacement for every surface zxr is not sampling — quiet, hidden, unmapped.**
   Eleven of eleven comparables release the previous buffer at (or by) the replacing commit for a
   surface they are not presenting, none withholds releases as back-pressure, the protocol permits
   releasing a buffer it never read and puts the throttling of invisible surfaces on frame callbacks
   and on the client (§1.3), and the one client family that would be throttled by a hold — Mesa EGL —
   turns the hold into a roundtrip storm that costs more than the commits it prevents (§2). The
   `--debug-hold` flag stays for measurement, default `replacement`; spec §5a's "a member whose tree
   did not commit is not touched and holds no buffer" and §7's quiet shape now say this for
   committing members too.
2. **The throttle is the protocol's: frame callbacks and `xdg_toplevel.suspended`.** Non-composed
   members receive frame callbacks on the fallback cadence (the smithay family's ≈ 1 Hz floor, already
   zxr's rule for out-of-view members — research/65 §4.2), and their toplevels now carry the
   `suspended` state while quiet or hidden, cleared on return (KWin's shape; mutter's 3 s hysteresis
   is flagged below). Verified on the host: foot (xdg_wm_base v7) receives `configure` with
   `activated|suspended` at `quiet on` and `activated` at `quiet off`; vkcube binds v1 and smithay
   filters the state. A client that ignores both is exercising a right the protocol grants it; its
   cost to zxr is §0's budget line.
3. **The research/67 §6 number is a budget note, not a design driver.** The doubling needs a client
   whose frame costs less than a dispatch; a GPU-bound client's rate is its GPU's, and a
   frame-callback-respecting client's is the fallback cadence. Recorded in budgets.md §5.

**Flagged judgments (rule 4 — not forced by the evidence, stated for the owner):**

- **Hidden members: fallback callbacks (≈ 1 Hz) or none?** The comparables split 7 (none: weston,
  wlroots, Mir, Louvre, Hyprland, mutter, KWin) : 3 (≈ 1 Hz: smithay, niri, cosmic-comp), with the
  1 Hz side's reason being compatibility ("clients might get blocked in their main loop", Minecraft,
  Veloren) and the none side's being the protocol text. zxr already chose the 1 Hz fallback for
  out-of-view members on the same evidence; the A/B kept it for hidden members for consistency.
  [window-workspace-management.md](../architecture/window-workspace-management.md) §"hidden" says
  "no frame callbacks" (river's `hide`). The two should agree; which way is the owner's call — the
  cost difference is 1 dispatch per second per hidden member.
- **`suspended` immediately (KWin) or after a delay (mutter, 3 s)?** mutter's reason is not stated
  in its tree; the plausible one — not flapping the client's render state across brief occlusions
  and workspace animations — does not apply to quiet mode (a game start/stop) or an explicit hide,
  so zxr sets it immediately. If a transient state ever drives it (fullscreen band, overview), the
  hysteresis becomes a real question.

## 4. Recorded items (not decided here)

### 4.1 Frustum-gating the panel pass

Whether an out-of-view member's commit should skip the panel pass (one stale tick when it swings
back, GPU saved) is a separate question — the 2D compositors' *occluded-window repaint* policy:
weston schedules no repaint for an occluded surface but still uploads shm for any node on the
primary plane (`surface-state.c:638-642`, `gl-renderer.c:3464-3465`); wlroots imports at commit,
intersects damage with `node.visible` (`wlr_scene.c:1000-1006`); mutter attaches and uploads shm at
commit regardless (`meta-wayland-surface.c:1164, 999`), skips only the stage repaint; **KWin defers
the texture work to `SurfaceItem::preprocess`, which runs only for painted items**
(`itemrenderer_opengl.cpp:210`) — the one comparable with no GPU work for a hidden commit; Hyprland
damages only where `shouldRenderWindow` (`Renderer.cpp:2848`); gamescope imports and fences every
unfocused commit but never paints it; niri and anvil `early_import` on every commit. KWin's shape
is the candidate; the XR-specific difference (the runtime samples the quad wherever the head
points) is what makes it a question rather than a copy.

### 4.2 Sampled-buffer retention vs Mesa EGL's roundtrip spin (state iii — the frame path)

Found on the way and outside this pass's scope: a **composed** GPU-bound EGL client costs zxr
445–457 ms/s and 72–79 k wake-ups/s at ≈ 1 050 commits/s (§0, §2 control column), against
265–320 ms/s for a Vulkan client at 12–17 k commits/s. The syscall signature is the §2 roundtrip
storm: zxr holds each sampled buffer to its slot fence (§6.5, retention max 2 frames) and smithay
holds the current one, so a 4-buffer EGL client at > display rate runs out of free buffers every
tick and spins `wl_display.sync` until the fence releases one. wlroots, mutter and KWin never hold
a composited dmabuf past the replacing commit (§1.1) — they rely on kernel implicit sync for the
read-after-release, and fence only the explicit-sync release *point*. Whether zxr should do the
same (release `wl_buffer` at replacement, fence only the syncobj release point — the comparables'
converging shape) or keep the §6.5 hold is a frame-path question for research/65's follow-up, with
this measurement as its starting number. Until then a fast EGL client on a composed plane is the
most expensive thing zxr can host.

## 5. Findings by label

**Measured (host):** §0 and §2 tables; `suspended` delivery to a v7 client. **Read:** §1.1–1.2
positions and reasons, file:line in the pinned clones. **[external]:** wayland.xml (nix store
1.25.0), Mesa WSI and EGL (gitlab main), Paalanen 2018, cmeissl smithay #1908, niri `ed8a6afe`.
**Analytic:** staleness bounds and pinned memory in §2; the per-100-fps budget line in §0.
**Hardware-deferred:** none — the mechanisms are protocol- and client-side; the device changes the
µs, not the shape.

## 6. Runs referenced

`bh-bound.sh` (§0), `bh-ab.sh` (§2 matrix, `bh/matrix.txt`), `bh-strace.sh` (§2 syscall signature,
`bh/strace-*.txt`), `bh/zxr-susp*.log` (`WAYLAND_DEBUG` capture of the `suspended` configure). zxr
`zxr/buffer-hold` at the commit that lands this document; xrgears, Monado `f07dd13`, dev-session
`d73a580`.
