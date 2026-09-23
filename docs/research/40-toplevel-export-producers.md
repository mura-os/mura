# 40: Toplevel-export producer seams — KWin, Mutter, smithay/COSMIC, wlroots

**Question.** What exactly does each producer compositor need to implement
[`zspatial-toplevel-export-v1`](../../protocols/zspatial-toplevel-export-v1.xml) (requirements R1–R24,
[32 §8](32-toplevel-export-prior-art.md)) — verified against the code, replacing
[ADR 0014](../architecture/adr/0014-toplevel-delegation-protocol.md) M-B's unverified "5–8 kLOC
building on GraphicsBufferRef, thumbnail infra, krdp/EIS plumbing" estimate.

**Method/sources.** Three code studies (2026-09-23) over the pinned checkouts:
`references/kwin` (master @ d84a316, shallow — "merged since the VR MR" claims compare code
state, not history), `references/kwin-vr` (branch `vr` @ ccdd46e, full fork history),
`references/mutter` (51.0 @ 888a7b7), `references/gnome-shell`, `references/gnome-remote-desktop`,
`references/smithay` (vendored), `references/cosmic-comp`, `references/cosmic-protocols`,
`references/wlroots`, `references/libei`, `references/krdp` (@ 939a907; cloned mid-study — the
KWin input findings rely on KWin's own EIS plugin, which is the machinery KRdp reaches through
the RemoteDesktop portal anyway, [32 §5.1](32-toplevel-export-prior-art.md)). Paths below are
relative to `references/`. Normative outputs: the producer conformance spec
([specs/toplevel-export-producer.md](../../specs/toplevel-export-producer.md)) and the per-DE
briefs ([producers/kwin.md](../architecture/producers/kwin.md),
[producers/mutter.md](../architecture/producers/mutter.md)).

---

## 1. KWin

### 1.1 The headline: the release join is existing library code

`SurfaceInterface::bufferReleasePoint()` publicly exposes a `shared_ptr<SyncObjReleasePoint>`
per commit (`kwin/src/wayland/surface.cpp:1299-1301`); the point's **destructor** signals the
client — or transfers the merged fence — only when the last holder drops it, and
`addReleaseFence(fd)` merges any number of GPU-completion fences with `SYNC_IOC_MERGE`
(`kwin/src/core/syncobjtimeline.cpp:104-123`). KWin's own scene holds the same shared_ptr per
texture (`kwin/src/scene/surfaceitem_wayland.cpp:71-72,226-234`). The exporter therefore takes,
per commit: a `GraphicsBufferRef` on `surface->buffer()` plus a copy of the release-point
shared_ptr; on the consumer's `release` it calls `addReleaseFence(consumer_syncfile)` and drops
both. The AND-join of "producer done ∧ consumer done" (conformance §3.3) is computed by the
existing refcount + fence-merge semantics — **zero new lifetime machinery**. Per-buffer
out-of-order release is inherent (one release-point object per commit); implicit-sync clients use
the existing `GraphicsBufferReleasePoint`/`SyncReferencer` (`syncobjtimeline.cpp:49-101`).

`DmaBufAttributes` already carries per-plane fds/offsets/pitches/fourcc/modifier **and `dev_t
device`** (`kwin/src/core/graphicsbuffer.h:22-34`) — the whole R5 payload including device
honesty; `mainDevice(wl_client)` is queryable (`kwin/src/wayland/linuxdmabufv1clientbuffer.h:81`)
and per-surface dmabuf-feedback tranche steering exists
(`linuxdmabufv1clientbuffer.h:49-58`, driven from `surfaceitem_wayland.cpp:169-186`) — the
upstream-clean version of the fork's unmerged egl format filter.

### 1.2 Acquire: commits are latched GPU-idle

KWin CPU-waits acquire points (or dmabuf-exported sync files) **before** applying commits
(`kwin/src/wayland/transaction.cpp:207-290`), so at `SurfaceInterface::committed`
(`surface.cpp:782`) buffers are already GPU-idle. v1 producers send an already-signalled acquire
point — zero new sync code, at the cost of one producer-side latch of latency KWin's own scene
pays too. Forwarding *unsignalled* acquires (consumer overlap) would need a pre-latch
`Transaction` hook: deliberately out of v1.

### 1.3 Tree, popups, screencast contrast

- The whole model is plugin-walkable with zero core hooks (plugins are in-process C++,
  `kwin/src/pluginmanager.cpp:80-121`; screencast includes `workspace.h`/`input.h` freely):
  `Workspace::windows()/stackingOrder()/findWindow(QUuid)`, `Window::internalId()`
  (`window.h:1119`), `Window::transients()`, `SurfaceInterface::below()/above()/subSurface()`
  (`surface.h:175-185`). Synchronized subtrees apply recursively before `committed` fires
  (`surface.cpp:770-782`) — the natural atomic `done` boundary. All R4 metadata is `SurfaceState`
  merged atomically in `mergeInto` (`surface.cpp:557-621`).
- The screencast window source is pure render-copy (`renderItem` into compositor-allocated
  buffers, hardcoded ARGB8888 — `kwin/src/plugins/screencast/windowscreencastsource.cpp:87-90,
  126-145`) — it proves plugin integration, not forwarding. Two upstream moves since the VR MR:
  the window stream now **auto-follows popup transients** (`windowscreencastsource.cpp:28-73`,
  Redondo 2025 — upstream already accepted "toplevel capture includes its popup tree", flattened)
  and screencast PipeWire buffers can carry **syncobj acquire/release**
  (`screencastbuffer.cpp:87-108`) — the team already exports syncobj fds out-of-process.
- `ext-foreign-toplevel-list` is absent from master (grep-verified); the only enumeration
  protocol is KDE-private `org_kde_plasma_window_management`. R1 = a new ~500 LOC
  standard-protocol implementation hung on `Window::internalId()` — independently upstreamable.
- Popups: the entire flip/slide/resize algorithm is already parameterized on an arbitrary bounds
  rect (`XdgPositioner::placement(const RectF &bounds)`, `kwin/src/wayland/xdgshell.cpp:1221`);
  the *source* of that rect is one hardcoded expression
  (`kwin/src/xdgshellwindow.cpp:1891-1898`). The fork's 28-LOC settable
  `Workspace::PopupBoundsResolver` (commit c877221, unmerged) is resubmittable nearly verbatim.

### 1.4 Input: the EIS plugin is the template; per-window targeting is the gap

- `EisBackend` is a KWin `InputBackend` behind D-Bus `org.kde.KWin.EIS.RemoteDesktop`
  (`kwin/src/plugins/eis/eisbackend.cpp:45-46,102-149`), with per-output absolute regions
  (`mapping_id` = output name, `:171-193`), 1:1 event conversion into `InputDevice` signals
  (`eiscontext.cpp:268-360`), and a safety timer force-releasing held buttons/keys/touches on
  sender stall (`eisdevice.cpp:31-57`) — the conformance §5.6 cancellation pattern in-tree.
  Notably, **keysym/text injection was added by David Edmundson in 2026**
  (`eisdevice.cpp:1-6,159-214`) — active investment in exactly the direction he stated.
- Per-window targeting does not exist: injected events go through the global hit test
  (`InputDeviceHandler::update()` hardcodes `findToplevel(position())`,
  `kwin/src/input.cpp:3697-3708`) and positions clamp to output geometry
  (`pointer_input.cpp:842,883-885`). The fork's two ~30-LOC callback seams — hovered-window
  resolver (07306c0) and position limiter (d65d60a) — remain unmerged and are the missing core
  pieces for node-addressed delivery.
- Serials/grabs/activation are producer-owned already: all serials from `display->nextSerial()`
  (`kwin/src/wayland/seat.cpp:444+`), implicit-grab focus blocking
  (`pointer_input.cpp:558-585`), popup grab/dismissal (`xdgshell.cpp:854`,
  `xdgshellwindow.cpp:1918-1922`), and a **pluggable activation token creator**
  (`kwin/src/wayland/xdgactivation_v1.h:38-41`, serial-validated minting
  `xdgactivationv1.cpp:61-112`) — R17/R18 need no core change.

### 1.5 Pacing: a dual-driver system that needs one ownership seam

Frame callbacks flush via `SurfaceInterface::frameRendered` (`surface.cpp:497-507`), driven
either by paint (`surfaceitem_wayland.cpp:117-122,254-271`) or — for invisible windows — by a
per-window offscreen timer at output refresh (`Window::refOffscreenRendering`,
`kwin/src/window.cpp:4408-4441`; screencast and thumbnails rely on it, and
`WindowItem::updateVisibility` keeps such clients unsuspended, `windowitem.cpp:196-203`). The
switch between drivers is visibility — i.e. KWin already has two coexisting pacing modes with an
implicit owner. Consumer pacing = make the owner explicit: suppress paint dispatch for exported
trees (the fork's 3-LOC visibility shape), suppress the timer, and let the export plugin call the
public `framePainted`/`frameRendered` at consumer cutoffs. Commit-timing/fifo integration exists
(`tryApplyState(timestamp)`, `surface.h:376`; `transaction.cpp:190-205,300-314`). Presentation
feedback has the honest zero-copy flag already (scanout-only,
`surfaceitem_wayland.cpp:260-265`); the exporter holds the commit's `PresentationFeedback`
shared_ptr and reports consumer outcomes (one ~10-LOC accessor widening: `presentationFeedback()`
is gated on the surface's primary output, `surface.cpp:509-515`).

### 1.6 Interactive move: one hard patch dissolves; the other is relocated

*(Revised per the KWin-persona review: the original "both dissolve" claim was half right.)*

- The active move is globally identifiable (`workspace()->moveResizeWindow()`, consulted by the
  move filter at `input.cpp:679`), and **the cursor anchor already exists**:
  `interactiveMoveOffset()` (`window.h:1872-1878`) is the normalized in-window press position —
  the R23 anchor is `offset × frame size`.
- `finishInteractiveMoveResize` runs the placement epilogue R23 must skip — output snap,
  electric-border maximize/tile, shift-tile (`window.cpp:1078-1097`) — and the cancel path
  restores initial geometry (also wrong). R23 = a third finish mode, **~20–40 LOC**: teardown +
  signals, no epilogue. The fork instead forked the move state machine with `if (isVr)`
  (02db754, 97 LOC — the bitrot pattern Vlad rejected) because VR windows keep *living* in the
  2D move machinery; delegation *ends* the move at the boundary, so the move-machine fork
  genuinely dissolves.
- **The output-reassignment patch (0448fdd) does not dissolve — it is relocated and enlarged.**
  A delegated window remains a live `Window` in KWin: unhandled, it would render at its parked
  rectangle, be hit-testable by the local pointer (`findToplevel`), appear in alt-tab/taskbars,
  and be moved/resized by output hotplug and `checkWorkspacePosition` for the *entire delegation
  lifetime* (the fork suppressed this only during moves). The producer conformance spec §2.8
  therefore defines the **parked-window model** as a normative obligation (not presented
  locally, no local input, topology-frozen geometry, session-state exclusion, switcher policy),
  and the KWin series carries it as its own core MR (~200–400 LOC, the hardest one) — a single
  narrow window state (minimized-window analog plus geometry freeze) rather than scattered
  `isVr` checks.
- R24's recipe is verbatim in-tree: the xdg-toplevel-drag input filter moves the window under the
  cursor and starts an interactive move via `performMousePressCommand(Options::MouseMove, …)`
  (`input.cpp:2853-2882`), ends with `endInteractiveMoveResize()` + raise + focus (`:2629-2639`);
  `PointerInputRedirection::warp` is public (`pointer_input.cpp:921-926`).

### 1.7 Privilege

`FilteredDisplay::allowInterface` denies a restricted-interface set to sandboxed clients
(`kwin/src/wayland_server.cpp:123-165`; sandbox detection `clientconnection.cpp:31-58`;
`wp_security_context_manager_v1` served, `wayland_server.cpp:362`). The strongest R2 shape is the
**dedicated-connection pattern**: `Display::createClient(fd)` over a socketpair, identity-checked
by pointer — how Xwayland, input-method, and the screen locker get special rights
(`wayland_server.cpp:195-208,653-657`). A plugin can own its global outright (screencast does:
`screencastmanager.cpp:33`) and bind-filter inside its own bind handler — no core change strictly
required.

### 1.8 KWin verdict table (condensed; LOC excl. tests)

| Req | Verdict | Core cost | Plugin cost |
|---|---|---|---|
| R1 handle | NEW (standard protocol impl on `internalId`) | 400–600 (core-adjacent, standalone MR) | — |
| R2 privilege | ADAPT (dedicated connection / bind filter) | 0 | 100–200 |
| R3/R4 tree + atomic wire | EXISTS (model) + NEW (serialization) | 0 | 1000–1500 |
| R5/R6 dmabuf + fallback | EXISTS (attrs incl. dev_t; tranche steering) | 0 | 350–650 |
| R7–R9 sync + release join | **EXISTS** (§1.1–1.2) | 0 | 150–300 |
| R10 flow control | NEW | 0 | 150–300 |
| R11/R12 pacing | ADAPT (dual-driver → explicit owner) | 70–180 | 300–500 |
| R13/R14 feedback | ADAPT (held feedback; accessor widening) | ~10 | 100–200 |
| R15 popup bounds | ADAPT (fork seam c877221 resubmitted) | ~30 | 100–150 |
| R16–R18 input | ADAPT (EIS template; 2 fork seams) | ~70 | 400–700 |
| R19 dnd off | EXISTS (omission) | 0 | ~10 |
| R20/R21 revocation + policy | EXISTS (triggers: unmapped/closed/lockStateChanged/GPU-reset `eglbackend.cpp:48-90`) + NEW (policy) | 0 | 350–700 |
| R22 zero-copy honesty | EXISTS (scanout-only flag) | 0 | 0 |
| R23 detach | ADAPT (epilogue-skip finish variant) | 20–40 | 150–250 |
| R24 adopt | EXISTS (toplevel-drag recipe) | 0 | 150–250 |

**Totals (as revised by the KWin-persona review): core ≈ 450–800 LOC across seven patches**
(bounds resolver, hovered-window resolver, position limiter, pacing ownership, move-finish
variant, feedback accessor, **plus the delegated-window-state patch the original count missed**,
~200–400) **plus a ~500 LOC standalone ext-foreign-toplevel-list implementation;
plugin ≈ 4.2–6.2 kLOC.** Still inside doc 32 §6.1's envelope, chiefly because the release join
and transactions are fully reusable. The 9-MR series shaped from this lives in
[producers/kwin.md](../architecture/producers/kwin.md).

### 1.9 Master movement since the VR MR (code-state comparison)

Moved toward export: popup-following window screencast (2025); syncobj in screencast PipeWire
buffers; Edmundson's EIS keysym/text injection (2026). Not merged: all five fork seams (popup
bounds, hovered-window resolver, position limiter, offscreen-callback fixes, format filter) plus
InternalWindow transientness (kwin!8500 still pending). Still absent: ext-foreign-toplevel-list,
ext-image-capture-source/copy-capture, any per-window input targeting.

---

## 2. Mutter / GNOME

### 2.1 The window stream copies; the dmabuf seam exists one layer down

`RecordWindow` (private D-Bus, `org.gnome.Mutter.ScreenCast` — "This API is private…" in its own
XML) re-renders the flattened actor tree into consumer allocations
(`meta_window_actor_paint_to_bitmap` / full `clutter_actor_paint`,
`mutter/src/compositor/meta-window-actor.c:1436-1584`), monitor-sized ("windows can be resized,
whereas streams cannot", `mutter/src/backends/meta-stream-window.c:195-202`), BGRA-only
(`meta-stream-source-window.c:851-865`), hardcoded 60 Hz (`:400-402`). Copy-capture, per
[32 §1](32-toplevel-export-prior-art.md) — and the quantified producer-side pain an upstream
pitch leads with.

One layer down, the forwarding seam exists: `MetaWaylandDmaBufBuffer` retains client plane
fds/fourcc/modifier for the buffer's lifetime (`mutter/src/wayland/meta-wayland-dma-buf.c:126-140`,
freed only in finalize `:1969-1979`; `meta_wayland_dma_buf_from_buffer` public); per-commit attach
is visible in `MetaWaylandSurfaceState.buffer` and held (`buffer_held = TRUE`,
`meta-wayland-surface.c:915-938`). Release is a **per-buffer use count that signals all
accumulated syncobj release points at zero** (`meta-wayland-buffer.c:651-705`) — Mutter's analog
of KWin's mechanism; the producer-owned join is "hold one extra `inc_use_count` per in-flight
commit, dec when the consumer's fence signals", with one small ADAPT: `handle_release_points`
merges only cogl's latest sync fd today and must also merge the consumer's. drm-syncobj is fully
implemented (645 LOC, since 46.1); commits wait acquire points before application
(`meta-wayland-transaction.c:401-415,470-474`) — the same "send signalled acquires in v1"
simplification as KWin.

### 2.2 Tree/commit state: native and observable

GNode subsurface trees per state generation with traversal macros
(`meta-wayland-surface-private.h:217-251,471-508`); synchronized subsurfaces are transactions
(R4's model natively); `MetaWaylandSurfaceState` carries everything R4 lists (`:73-159`);
`pre-state-applied`/`applied` signals fire synchronously at commit application, independent of
paint (`meta-wayland-surface.c:898,1094-1097`). **No ext-foreign-toplevel-list** anywhere
(`meta-wayland-versions.h:35-73`) — R1 must be written (~600 LOC).

### 2.3 Popups, input, pacing, move (condensed)

- **Popups**: positioner → `MetaPlacementRule` → `constrain_custom_rule` flip/slide/resize
  against `info->work_area_monitor` (`mutter/src/core/constraints.c:485-487,856-864,927-1150`).
  Consumer bounds = a per-exported-window work-area override, ~100–200 LOC, the same seam KWin
  VR patched. (`MetaExternalConstraint` exists but doesn't cover custom-rule flips.)
- **Input**: per-window EIS *coordinates* exist (`MetaStreamWindow` is a standalone EIS viewport
  with its own mapping id, `meta-stream-window.c:215-282`; g-r-d targets it by mapping id,
  `gnome-remote-desktop/src/grd-session.c:1397-1415,703-755`) but *delivery* is a global
  `ClutterVirtualInputDevice` + stage pick (`meta-eis-client.c:382-506`) — wrong for occluded
  exported windows. Forced-target routing into `MetaWaylandSeat`/pointer/keyboard/touch is the
  largest genuinely NEW piece (~0.8–1.5 kLOC, input core). Serials/grabs stay native (R17 free);
  xdg-activation mints producer tokens (`meta-wayland-activation.c:38-121`).
- **Pacing**: the correct template is the per-stage-view timerfd `FrameCallbackSource`
  (`meta-wayland.c:222-346,440-482`) rekeyed to a consumer clock; the `is_streaming` view-primacy
  exemption (`meta-surface-actor-wayland.c:90-153`) only proves obscured windows can keep
  callbacks — dispatch stays on the local view clock (Mutter-persona correction). Callback lists
  are spliced view-keyed at role-apply time, so the atomic owner switch intercepts there, and
  fifo-v1 barrier clearing (view-transaction-driven) must be taken over for consumer-paced trees
  or fifo clients hang. presentation-time v2 feedback lists exist; a consumer-fed frame-info
  source is new. The per-surface `applied` signal has no transaction-complete boundary — the
  protocol's atomic `done` needs a new hook in `meta-wayland-transaction.c` (core seam 6 in the
  brief).
- **Move**: side-effects live only in `end_grab_op` (`meta-window-drag.c:1795-1862`);
  **`meta_window_drag_end` is already the side-effect-free teardown** (`:385-424`) — R23 is a
  new entry point, not new suppression logic. Anchor data present (`:66,500-503`); active drag
  discoverable (`compositor.c:1773`); pointer warp + public `begin_grab_op` + in-tree
  `xdg-toplevel-drag` (478 LOC, Igalia) make R24 glue.

### 2.4 Privilege and the shell split

Per-client global filtering exists (`MetaWaylandFilterManager`,
`meta-wayland-filter-manager.c:34-101`), granted via the **ServiceChannel trusted-connection
pattern** (pidfd-verified dedicated Wayland fd + client caps, `meta-service-channel.c:115-209`;
x11-interop is the worked example). No wp_security_context in tree. gnome-shell participates only
at the policy layer (session-lock revocation via the remote-access inhibition chain,
`gnome-shell/js/ui/main.js:138-147` → `meta-remote-access-controller.c:146-184`); everything else
is Mutter C — no extension-based implementation is possible (confirming 32 §6.2).

### 2.5 Sizing and posture

**~5–9 kLOC total, ~0.7–1.5 kLOC core-touching** — KWin-sized; Mutter is not structurally
harder. Upstream-sized MRs (a) ext-foreign-toplevel-list, (b) release-join + plane accessors,
(c) popup bounds override, (d) pacing hook are individually defensible; the export module and
input targeting only after upstream standing. Adoption record (NEWS): externally-sponsored,
post-standardization protocols only — syncobj 46.1 (!3300), toplevel-drag **48.0** (!4107),
commit-timing/fifo 48.0 (!3355); NEWS records versions/MRs only — the sponsor attributions
(NVIDIA, Igalia, Valve respectively) are external knowledge of those MRs, corrected and flagged
by the Mutter-persona review. Meanwhile ext-session-lock/foreign-toplevel-list/copy-capture are
deliberately skipped where private D-Bus + portals cover GNOME's own need. **Plan Mutter as the
third producer**; engagement conditions in [producers/mutter.md](../architecture/producers/mutter.md).

---

## 3. smithay / COSMIC (reference producer) and wlroots

### 3.1 smithay: the reference producer's free list

- **R1–R2 free**: complete ext-foreign-toplevel-list impl with `from_resource` handle recovery
  (`smithay/src/wayland/foreign_toplevel_list/mod.rs:158-161,306-320,490`); per-global
  `can_view` filter closures on every privileged module (dmabuf `mod.rs:616-639`, syncobj
  `mod.rs:145-156`).
- **R8 idiomatic**: committed buffers are wrapped in a refcounted `Buffer` whose last-clone drop
  sends `wl_buffer.release` **and** signals the syncobj release point
  (`smithay/src/backend/renderer/utils/wayland.rs:60-79`); the join = hold a clone, drop on
  consumer + local completion. The vendored tree carries a local `DrmSyncPoint::import_sync_file`
  patch (mirroring wlroots) for injecting consumer fences (`drm_syncobj/sync_point.rs:181-238`).
  Sharp edge: acquire points are `pub(crate)` and consumed at commit — clone them from
  `DrmSyncobjCachedState` in the commit handler, or carry a one-line accessor patch.
- **R5** free via `get_dmabuf` + `Dmabuf` plane accessors (`dmabuf/mod.rs:1025-1027`,
  `allocator/dmabuf.rs:214-226`); **R15** free via
  `PositionerState::get_unconstrained_geometry(target_rect)` — full flip/slide/resize against an
  arbitrary rect (`shell/xdg/mod.rs:695-800`); **R16–R18**: seat handles take arbitrary
  focus-target types and own serials/grabs internally (`input/pointer/mod.rs:228-299`,
  `keyboard/mod.rs:1434-1443`); **pacing hook**: `send_frames_surface_tree` with caller-controlled
  clock/throttle (`desktop/wayland/utils.rs:221`).
- **Scope: ~2–4 kLOC hand-written** in an existing smithay compositor, dominated by the
  green-field pieces — calibrated against real modules (foreign-toplevel-list 542 LOC, cosmic's
  zcosmic info server 724, cosmic capture handler ≈1.4 k).

### 3.2 COSMIC prior art

`zcosmic_toplevel_info_v1` v2+ *extends the standard ext handle* (`get_cosmic_toplevel`,
`cosmic-protocols/unstable/cosmic-toplevel-info-unstable-v1.xml:84-91`) — the exact shape of
`export_toplevel`. Privilege is two predicates (`client_has_no_security_context`/
`client_not_sandboxed`, `cosmic-comp/src/state.rs:643-653`) passed to every privileged global;
a zspatial producer adds one line. `PendingImageCopyData` holds smithay `Buffer` clones until the
copy's GPU fence lands, with the in-code comment saying exactly that
(`cosmic-comp/src/wayland/handlers/image_copy_capture/render.rs:78-118`) — the hold-and-release
pattern minus the render copy zspatial eliminates. Toplevel management's `activate` routes consumer
requests through cosmic's own focus machinery (`handlers/toplevel_management.rs:31-114`) — the
R18 shape.

### 3.3 wlroots (annex)

Neutral handle from `wlr_ext_foreign_toplevel_list_v1` (292 LOC); the ext toplevel
image-capture-source manager's **request/accept split** (compositor explicitly accepts each
source request, `types/ext_image_capture_source_v1/foreign_toplevel.c:19-45`) is a
deny-by-default shape worth imitating. `wlr_buffer_lock` refcounting holds client dmabufs past
commit (`types/buffer/buffer.c:35-73`; capture frames do it), with two caveats: a held lock
disables the in-place shm texture-update path (`client.c:136-146` — long exports force
multi-buffering), and **plane fds must be dup'd eagerly** because the source is nulled on client
`wl_buffer` destroy (`client.c:87-94`). Privilege = `wl_display_set_global_filter` +
`wlr_security_context_manager_v1_lookup_client` (documented in
`include/wlr/types/wlr_security_context_v1.h:15-20`).
`wlr_xdg_positioner_rules_unconstrain_box` takes an arbitrary constraint box
(`types/xdg_shell/wlr_xdg_positioner.c:500-518`) — R15 with zero new geometry.

---

## 4. Cross-compositor conclusions

1. **The R8 producer-owned release join is an existing idiom in all four stacks**: KWin's
   shared_ptr release point + fence merge, Mutter's use_count + release_points array, smithay's
   `Buffer` drop, wlroots' lock count. The conformance spec (§3.3) demands nothing novel on
   buffer lifetime — decisive for upstream credibility, and it validates ADR 0014's §2 verdict
   (producer-owned join) as *the* shape every compositor already implements internally.
2. **What is green-field everywhere**: consumer pacing (R11–R14), tree wire-mirroring (R3/R4 as
   protocol), the node-addressed input back-path (R16), and detach/adopt (R23/R24). These are the
   protocol's actual contribution and dominate every LOC estimate.
3. **Verified sizing** (replacing ADR 0014 M-B's guess): KWin ≈ 250–450 core LOC + 3.5–5.5 k
   plugin (+~500 standalone foreign-toplevel-list); Mutter ≈ 5–9 k total, 0.7–1.5 k core;
   smithay reference ≈ 2–4 k. Doc 32 §6.1's 5–8 k envelope holds, with less core surface than
   estimated.
4. **The structural KWin argument, stated precisely** (revised per the KWin-persona review):
   the move/resize fork *dissolves* (delegation ends the 2D move at the boundary), but the
   output-reassignment problem is *relocated* into the delegated window's parked 2D state and
   must be designed, not assumed away — conformance §2.8 defines it, and every producer carries
   one narrow "delegated" window state instead of the fork's scattered mode checks. The honest
   sentence for Vlad Zahorodnii is: one of your two hard patches disappears, the other becomes a
   single window state we specify and test.
5. **Privilege converges** on "trusted dedicated connection + per-global filter" (KWin
   `Display::createClient` identity checks; Mutter ServiceChannel caps; smithay/cosmic filter
   closures; wlroots display filter + security-context lookup). The conformance spec's §2.1
   wording deliberately admits all four.
6. **Institutional sequencing confirmed**: KWin first (maintainers on record wanting the
   function; five of six enabling core seams are resubmissions of fork commits the author
   already triaged as clean-interface material), smithay/COSMIC as reference, Mutter third,
   post-standardization with a sponsor.
