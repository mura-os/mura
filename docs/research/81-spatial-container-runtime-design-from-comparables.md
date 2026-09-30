# 81 — The spatial-container runtime design (C1), from Monado's own seams and from comparables

**Date:** 2026-09-30. **What this is:** the design pass C1 requires before it is written (ADR 0006
amendment 4 D1–D3, D13; [specs/composition.md §3](../../specs/composition.md), the app-facing
contract; [implementation-path.md §3 "The C-track"](../architecture/implementation-path.md)).
The contract is done: every rule an `XR_EXT_spatial_container` runtime must meet is line-cited
in composition §3 against the pinned specification, and the specification's own Issues section
(`ext_spatial_container.adoc:1210-1315`) carries its authors' reasons. What does not exist is
the **runtime-internal design** — where container state lives across Monado's process boundary,
how a session is held at `IDLE` for life without forking the state machine, how six new events
travel, what the runtime does when no controller is connected, what a header bump touches, and
what the C1 gate concretely asserts. Research/79 §4b sized that gap file by file and did not
decide shape. This document decides shape (AGENTS rule 7: comparables with their reasons before
invention; rule 8: the owner rules what the comparables leave open). It builds no code. Its
normative product is [specs/monado-containers.md](../../specs/monado-containers.md).

**Nine questions, stated up front.** The five the owner named on 2026-09-30, and four the
baseline read surfaced.

- **Q1 — session-state mapping.** How a compositor-side state change becomes
  `XrEventDataSessionStateChanged` today, and how a container session is held at `IDLE` while
  `xrWaitFrame` still paces.
- **Q2 — where per-container state lives**, and the hook C2 (self rendering) binds to.
- **Q3 — the no-controller default**: what the runtime does with visibility, interactability,
  bounds and pose when no lease holder (C0) is connected.
- **Q4 — Godot without `_self_rendering`**: does the only open client degrade or assert against
  a runtime that advertises the base extension alone.
- **Q5 — `maxSpatialContainerCount`**: a budget number or a memory-limited rule.
- **Q6 — the header/registry prerequisite**: Monado's vendored OpenXR headers predate the pair.
- **Q7 — the container space in the space overseer**: runtime-movable, unlocatable while hidden.
- **Q8 — events, IPC, locking and budget**: six event types or one; coalescing; queue bounds.
- **Q9 — conformance and the probe**: what asserts composition §7.6 when CTS has no tests.

**Sources.** Pinned clones in `references/` (MANIFEST.json), depth-1 unless noted: `monado`
(b9883f235, the upstream study pin; the fork `mura-os/monado` branch `mura` at `ccae7f3c1` is
18 upstream commits ahead of it — byte-identical on every `oxr_*`, `xrt_session.h`, `xrt_space.h`,
`b_session.c`, `b_space_overseer.c` and proto line cited here, shifted by +10..+30 lines in
`xrt_compositor.h`, `comp_multi_*` and `ipc_server_*` where upstream `7da6703f8` added
`xmcc->session_get_running_state`; mechanisms unchanged), `monado-galaxyxr` (6ea94427f — `lightofmysoul/monado`
branch `galaxyxr`, Stanislav Aleksandrov's Galaxy XR bring-up, *not* a vendor runtime),
`godot` (941ea1816), `openxr-docs` (5a82d45bc — the 1.1.63 registry and spec sources),
`openxrs` (eba4c6a75), `wayland-protocols` (819004adb), `wlroots` (297e01d2d), `smithay`
(79bbed5e1), `weston` (3c6ce8da6), `openvr` (092406431), `pipewire` (2943841e3),
`aosp-frameworks-native` (4f463a6b1), `xrizer` (0989a7fac), `opencomposite` (cff07db75),
`stardustxr-server` (cf20614f3), `wayvr` (0eab39070), `wlx-overlay-s` (54713b95f), `xrdesktop`
(dbf3dbaa0), `simula` (a08ca31fa), `motorcar` (e1cb943d2); **added by this pass** (§7.7):
`openxr-sdk` (f2448a879, tag `release-1.1.63`) and `openxr-cts` (c4d9194d6, tag
`openxr-cts-1.1.63.0`). Upstream Monado branches and merge requests, Godot pull requests,
`openxrs` main and the WebXR specification were fetched read-only and are marked
**[external]**. Line numbers were re-read against the pins
while writing. Four reading groups (Monado internals; upstream direction; clients and tooling;
comparables outside Monado) ran in parallel with a fixed frame — problem, choice, *why*,
assumptions, transfer, trade-off — and were reconciled here.

## 0. Baselines — Monado as it is, the clients as they are

- **Headers predate the pair.** Monado's vendored OpenXR headers are **1.1.58**
  (`monado/src/external/openxr_includes/openxr/openxr.h:29`
  `#define XR_CURRENT_API_VERSION XR_MAKE_VERSION(1, 1, 58)`; the fork is identical); the pair
  is registered in **1.1.63** (`openxr-docs/specification/registry/xr.xml:24456`
  `XR_EXT_spatial_container` number 811, `:24536` `_self_rendering` number 814, `depends` on
  811). Extension support is generated
  (`monado/src/xrt/state_trackers/oxr/extension_support/oxr_extension_support.py`). The Rust
  bindings pinned for probe tooling are also 1.1.58 (`openxrs/sys/src/generated.rs:16`
  `CURRENT_API_VERSION: Version = Version::new(1u16, 1u16, 58u32)`; crate `sys` 0.14.0,
  `openxr` 0.22.0). **A header/registry bump precedes C1** (Q6).
- **The state tracker runs in the app.** `src/xrt/state_trackers/oxr/` is linked into the
  client; `monado-service` holds `comp_multi` and `ipc_server`. Session events cross by poll:
  `monado/src/xrt/ipc/client/ipc_client_session.c:50-56` `ipc_client_session_poll_events` →
  `ipc_call_session_poll_events`. `union xrt_session_event` has eleven types
  (`monado/src/xrt/include/xrt/xrt_session.h:41-71`, `NONE` … `REQUEST_EXIT`); the closest
  in-tree precedent for "runtime-controlled visibility delivered as an event" is
  `XRT_SESSION_EVENT_OVERLAY_CHANGE` (`:47`) surfacing as
  `XrEventDataMainSessionVisibilityChangedEXTX` (`oxr_session.c:712-713`;
  `oxr_event.c:265-286`).
- **One `xrt_session` and one native compositor per `XrSession`.** `oxr_session.c:1319`
  and `:1552` call `xrt_system_create_session`; `comp_multi_system.c:768`
  `system_compositor_create_native_compositor` → `multi_compositor_create`
  (`comp_multi_private.h:105` `struct multi_compositor`, `:212`). A container in C2 is one such
  slot; in C1 there is nothing to render.
- **Frame calls are guarded on "running".** `oxr_api_verify.h:349-352`
  `OXR_VERIFY_SESSION_RUNNING` → `oxr_frame_sync_is_session_running` →
  `XR_ERROR_SESSION_NOT_RUNNING`; used at `oxr_api_session.c:124,138,155,181,221`.
  `oxr_session_frame_wait` (`oxr_session.c:1077`) already has a compositor-less path —
  `if (xc == NULL) { frameState->shouldRender = XR_FALSE; return … }` (`:1084-1087`) — which is
  the `XR_MND_headless` session (`oxr_session.c:1543-1544`). A container session's
  `shouldRender = false` (`ext_spatial_container.adoc:1075-1076`) has an in-tree shape.
- **Overlay sessions are created by chain inspection.** `oxr_session.c:1577-1583` reads
  `XrSessionCreateInfoOverlayEXTX` and sets `xsi.is_overlay`, `xsi.flags`, `xsi.z_order`
  (`xrt_compositor.h:979,982` `struct xrt_session_info`). `XrSessionCreateInfoSpatialContainersEXT`
  (`ext_spatial_container.adoc:1000-1002`) is the same kind of chain and lands in the same
  function.
- **The event queue is unbounded.** `oxr_event.c:29-35` `struct oxr_event { next; length;
  result; }`, a singly linked list on the instance (`:61-89` pop/push).
- **Spaces are the overseer's.** `xrt_space.h:125` `create_offset_space`, `:143`
  `create_pose_space`, `:161` `locate_space`, `:183` `locate_spaces`, `:225/:234`
  `ref_space_inc/dec`, `:244` `recenter_local_spaces`, `:303` `create_local_space`.
- **Godot is the only open client** and it registers both wrappers together
  (`godot/modules/openxr/register_types.cpp:83-85,236`); it creates a container only when
  `maxSpatialContainerCount > 0` (`openxr_spatial_container_extension.cpp:62-65`), which is why
  today's run logs `Max spatial container count: 0` then `SCS ext absent`
  (`pkgs/spatial-container-sample/README.md` §2).
- **The Galaxy XR branch is a negative finding.** `monado-galaxyxr` adds a headset driver,
  a dual-DRM-lease compositor backend, camera passthrough, eye tracking and compositor-side
  foveation (`git log`: `7c90ff5`, `a979482`, `a823a0f`, `63ac02c`, `6ea9442`); no
  `spatial_container` symbol exists in it. It is read only for how a bring-up branch extends
  `oxr_session.c`/`oxr_objects.h` without breaking upstream shape.

## 1. Monado's own seams — what exists, hop by hop

Everything C1 needs has an in-tree precedent except the container space's mutability. Read
under the frame: what the mechanism is for, what it chose, why (in-tree comments), and what
that implies for a container session.

### 1.1 How a compositor decision becomes an OpenXR event today (Q1a)

1. **The service decides.** `ipc_server_process.c:546-577` `handle_focused_client_events`
   computes `visible`/`focused`/`z_order` per client — the primary is visible + focused with
   `z_order = INT64_MIN` (`:556-560`), overlays are always visible + focused with their own
   `z_order` (`:563-567`) — stores them in `ics->client_state.session_visible/focused/z_order`
   (`:569-571`) and calls `xrt_syscomp_set_state` / `xrt_syscomp_set_z_order` when `ics->xc`
   exists (`:573-576`). It runs under `s->global_state.lock` (`ipc_server.h:449-458`) on
   **whichever thread calls it**: the per-client IPC thread (`ipc_server_activate_session` from
   `ipc_handle_compositor_predict_frame`, `ipc_server_handler.c:992-996`; deactivate from
   `ipc_server_per_client_thread.c:207`; `set_primary_client` from *another* client's thread,
   `ipc_server_handler.c:1563-1569`) or the mainloop (`ipc_server_update_state`,
   `ipc_server_process.c:920-928`).
2. **The control interface.** `xrt_compositor.h:2380-2445` `struct xrt_multi_compositor_control`
   (`xmcc`): `set_state` `:2387-2391`, `set_z_order` `:2399`, `set_main_app_visibility`
   `:2420-2422`, `notify_loss_pending` `:2429-2431`, `notify_lost` `:2436`,
   `notify_display_refresh_changed` `:2441-2444`. It is an *optional aspect* of the system
   compositor (`:2458-2464`); the inline helpers return `XRT_ERROR_MULTI_SESSION_NOT_IMPLEMENTED`
   when it is absent (`:2508-2522`). **Why** (`:2382-2386`): "Sets the state of the
   compositor, generating any events to the client if the state is actually changed. Input
   focus is enforced/handled by a different component but is still signaled by the compositor."
3. **`comp_multi` records and emits.** `comp_multi_system.c:640-663` `system_compositor_set_state`
   compares against `mc->state.visible/focused` (`:649`), stores (`:650-651`), builds a
   `union xrt_session_event` of type `XRT_SESSION_EVENT_STATE_CHANGE` (`:653-657`) and calls
   `multi_compositor_push_event` (`:659`) → `xrt_session_event_sink_push(mc->xses, …)`
   (`comp_multi_compositor.c:108-113`). The write is **unlocked** — `:648` `//! @todo Locking?`
   — while the render thread reads `mc->state` in `transfer_layers_locked`
   (`comp_multi_system.c:253-318`). Table wiring `:843-849`.
4. **The sink is the session's.** `b_system.c:53-91` `create_session` creates a `b_session`
   (`:67`) and hands `&bs->sink` to `xrt_syscomp_create_native_compositor` (`:74-78`);
   `b_session.c:123-140` `b_session_event_push` callocs a node (`:126`) and appends to the tail
   under `bs->events.mutex` (`:129-139`) — **singly linked, O(n) append, unbounded, no overflow
   handling**. Broadcasts (reference-space change) go through `b_system::broadcast` →
   `b_system_broadcast_event` (`b_system.c:43-51`, `:240-254`) to every registered session
   (`b_system_add_session` `:165-190`). **Why** (`xrt_session.h:217-219`): sinks exist because
   "some sinks might multiplex events to multiple sessions"; `ipc_server_handler.c:439-447`
   creates a native compositor even for headless sessions "since the IPC layer can not … tell
   the multi compositor about it" — the `multi_compositor` is the only address the service has
   for per-session events.
5. **The app pulls.** `ipc_client_session.c:49-57` → `ipc_call_session_poll_events` (proto
   `50-session.json:11-15`, `out: union xrt_session_event`); server
   `ipc_server_handler.c:489-497` → `xrt_session_poll_events` → `b_session.c:44-52` →
   `b_session_event_pop` (`:142-159`). Cost per call: a 4-byte `ipc_command_msg`
   (`ipc_protocol_generated.h.template:30-33`) out, ≈52 packed bytes back (`xrt_result_t` +
   the ≈48-byte union — sized by `reference_space_change_pending`, `xrt_session.h:146-153`), one
   `ipc_send` + one blocking `ipc_receive` under `ipc_c->mutex` (`proto.py:71-74,78,121-129`).
6. **`oxr` translates.** `oxr_event.c:427-455` `oxr_poll_event` walks every session on **every
   `xrPollEvent`** (`:430-440`); `oxr_session.c:644-805` `oxr_session_poll` drains
   `xrt_session_poll_events` until `NONE` (`:690-701`) — so at least one IPC round-trip per
   session per `xrPollEvent`, idle or not — and `STATE_CHANGE` only *latches*
   `sess->compositor_visible/focused` (`:702-710`; comment `:706-708`: "server side focused /
   visible state does not correspond 1:1 to the cycle we tell the app"). The `XrSessionState`
   transitions happen afterwards at `:788-802`: SYNCHRONIZED→VISIBLE when `compositor_visible`,
   VISIBLE→FOCUSED when `compositor_focused`, and back. `oxr_session_change_state` (`:365-376`)
   warns and no-ops on a same-state change (`:367-372`, upstream !2530), else pushes
   `XrEventDataSessionStateChanged` (`:374`, `oxr_event.c:167-191`) and sets `sess->state`
   *immediately* (`:375`). The instance queue is a tail-pointer linked list of
   `U_CALLOC_WITH_CAST(struct oxr_event, sizeof + size)` nodes (`oxr_event.c:79-118`),
   unbounded; teardown removes only events `is_session_link_to_event` recognises (`:120-158`,
   `:385-425`) — the `EXTX_overlay` event is not among them.

**The overlay precedent (Q1c).** `XR_EXTX_overlay` is a one-container-per-session prototype of
"runtime-controlled visibility as an event": `ipc_server_process.c:516-543`
`handle_overlay_client_events` → `xrt_syscomp_set_main_app_visibility` →
`comp_multi_system.c:697-711` (sets `mc->state.is_base_session`, pushes
`XRT_SESSION_EVENT_OVERLAY_CHANGE` with `xse.overlay.visible`) → `oxr_session.c:711-715` →
`oxr_event.c:265-286` `oxr_event_push_XrEventDataMainSessionVisibilityChangedEXTX`. Footprint:
`oxr_session.c:1577-1583` (chain read into `xsi.is_overlay/flags/z_order`,
`xrt_compositor.h:977-983`), `oxr_objects.h:741-746`, `xrt_session.h:47,95-99,206`,
`comp_multi_compositor.c:980-981`, `ipc_protocol.h:396`, `ipc_server_handler.c:462-463` —
≈120 lines and **no new IPC calls**. Two quirks to not copy: the union member `overlay` is typed
`struct xrt_session_event_state_change`, not `xrt_session_event_overlay` (`xrt_session.h:206`
vs `:95-99`), and the event carries no session handle so it survives session teardown.

### 1.2 Every guard a container session touches (Q1b)

| Site | Lines | Today | Container mode (spec) |
|---|---|---|---|
| `OXR_VERIFY_SESSION_RUNNING` | `oxr_api_verify.h:349-354` → `oxr_frame_sync_is_session_running` (`oxr_frame_sync.c:112-118`, reads `ofs->running`) | `XR_ERROR_SESSION_NOT_RUNNING` unless `xrBeginSession` ran (`oxr_frame_sync.c:83-95`) | "treat the session as running when it is in the state IDLE" (`ext_spatial_container.adoc:1031-1032`); runtimes "no longer return `XR_ERROR_SESSION_RUNNING` or `XR_ERROR_SESSION_NOT_RUNNING`" (`:1107-1108`) |
| `xrBeginSession` | `oxr_api_session.c:91-113` → `oxr_session_begin` `oxr_session.c:414-516` | `NOT_READY` (state is IDLE) | `XR_ERROR_SPATIAL_CONTAINERS_ENABLED_EXT` (`:1036-1039`, `:1112-1114`) |
| `xrEndSession` | `oxr_api_session.c:116-127` → `oxr_session_end` `:519-592` | `NOT_RUNNING` | `SPATIAL_CONTAINERS_ENABLED_EXT` (`:1036-1039`) |
| `xrRequestExitSession` | `oxr_api_session.c:213-224` → `oxr_session_request_exit` `:595-620` | `NOT_RUNNING` | `SPATIAL_CONTAINERS_ENABLED_EXT` (`:1041-1043`); `oxr_session_poll:722` also calls it on `XRT_SESSION_EVENT_REQUEST_EXIT` — needs the same branch (`:1044-1046`: hide containers, destroy the session) |
| `xrLocateViews` | `oxr_api_session.c:227-277`; no RUNNING guard; `:247-252` rejects a mismatched view config (`MAX_ENUM` until begin, `oxr_session.c:1284`) | `VALIDATION_FAILURE` | `SPATIAL_CONTAINERS_ENABLED_EXT` (`:1070-1072`) |
| `xrWaitFrame` | `oxr_api_session.c:130-144` → `oxr_session_frame_wait` `oxr_session.c:1077-1163`; headless `xc == NULL` `:1083-1087` returns `shouldRender = false` with **no timing**; else frame-sync wait `:1099-1103`, `do_wait_frame_and_checks` `:1114-1129`, `shouldRender = should_render(state)` `:1145` (`:126-133`: true only for VISIBLE/FOCUSED/STOPPING — **already false for IDLE**) | works | `shouldRender` always false (`:1075-1076`) — met by `should_render(IDLE)`; real `predictedDisplayTime` needs `xc != NULL` (§1.3) |
| `xrBeginFrame` | `oxr_api_session.c:147-170` → `oxr_session_frame_begin` `oxr_session.c:1166-1216` | legal with `xc == NULL` | unchanged |
| `xrEndFrame` | `oxr_api_session.c:173-210` (`:185-196` `layerCount > max_layers`) → `oxr_session_frame_end.c:1764-2001`; headless early-out `:1801-1811` **ignores** layers; `layerCount == 0` → `xrt_comp_discard_frame` `:1840-1854`; `do_synchronize_state_change` (`:1627-1633`) pushes SYNCHRONIZED when `state < VISIBLE` | accepts | `layerCount != 0` → `XR_ERROR_VALIDATION_FAILURE` (`:1079-1082`); SYNCHRONIZED must never be queued (`:1027-1029`) |
| focus-dependent results | `oxr_objects.h:1444-1452` `oxr_session_success_focused_result` (`XR_SESSION_NOT_FOCUSED` unless `state == FOCUSED`); used by `oxr_api_action.c:966`, `oxr_input.c:1965,2006`, `oxr_set_haptic.c:89-90,145` | state-keyed | "focused when IDLE and at least one spatial container exists which is interactable" (`:1033-1035`) — a session-level predicate, not `state` |
| create verification | `oxr_verify.c:643-647` accepts a chain with no graphics binding only under `MND_headless` | — | a graphics binding is optional (`:1090-1093`); `MND_headless` behaviour must **not** be enabled for the session (`:1095-1099`) |
| create | `oxr_session.c:1569-1611`: chain read `:1577-1583`; `oxr_session_create_impl` `:1341-1566` (headless branch `:1543-1561` sets `compositor = NULL`); **`:1605-1606` pushes IDLE then READY unconditionally** | — | stop after IDLE (`:1020-1030`) |
| loss | `oxr_session_poll:716-720` (`LOSS_PENDING`/`LOST`) | — | one `ClosedEXT` per container **before** `LOSS_PENDING` (`:1085-1089`) |

**Headless as the nearest in-tree analogue — and why it does not transfer whole.** A
`MND_headless` session (`sess->compositor == NULL`) has a legal frame loop once `xrBeginSession`
has run: begin pretends visible + focused and pushes SYNCHRONIZED/VISIBLE/FOCUSED
(`oxr_session.c:469-486`), wait returns immediately without timing (`:1083-1087`), end ignores
layers (`oxr_session_frame_end.c:1801-1811`). It is the only in-tree session whose state machine
is decoupled from compositor visibility, and it is where upstream found the class of bug a
container session will meet — !2530 fixed a SYNCHRONIZED→SYNCHRONIZED emission because
`has_ended_once` is set only in `frame_end`, "which is never called by the headless path"
[external, §2.1]. The spec's own text says headless behaviour must not be enabled for a
container session, and a container session *wants* `predictedDisplayTime`, so it is a third
shape: a session with a compositor whose state never leaves IDLE.

### 1.3 Frame pacing without `xrBeginSession` (Q1d)

- `oxr_frame_sync` (`oxr_frame_sync.h:36-44`: mutex, cond, `canWaitFrameReturn`, `running`) is
  pure app-process state: `wait_frame` loops `while (ofs->running)` and returns
  `XR_ERROR_SESSION_NOT_RUNNING` otherwise (`oxr_frame_sync.c:40-64`); `begin_session` sets
  `running = true` (`:83-95`). Nothing in it touches `xrt`; it can be started from a create path.
  It exists because upstream separated "session running" from "session state" (!2344, merged
  2024-11-07 — Rylie: "session running depends entirely on what calls have been made, and not
  on the session state events that were polled" [external]).
- The `xrt` side needs no begin for pacing: `multi_compositor_wait_frame`
  (`comp_multi_compositor.c:558-591`) predicts (`:505-532`, under `list_and_timing_lock`), sleeps
  and marks, and **does not check `mc->state.session_active`**. On the IPC path the service
  marks the client `session_active` on the first `predict_frame`, not on begin
  (`ipc_server_handler.c:992-996`; `ipc_server_process.c:874-902`, `:888`) — the comment: "We
  use this to signal that the session has started, this is needed to make this client/session
  active/visible/focused."
- What *does* need a begin: the multi main loop's timing feed. `broadcast_timings_to_clients`
  (`comp_multi_system.c:375-393`) runs only while `msc->sessions.state != STOPPED`
  (`:549-554`), which requires `active_count > 0` (`:478-493`), incremented only by
  `multi_compositor_begin_session` (`comp_multi_compositor.c:473-487` →
  `multi_system_compositor_update_session_status`, `:812-829`). Without any begun session the
  pacer runs on the once-seeded `last_timings` ("wild guess" 16 ms, `comp_multi_system.c:860-864`).
  `xrt_compositor.h:2472-2473` states the split: `create_native_compositor` "implicitly brings
  up a new session. Does not 'call' xrBeginSession." The IPC begin handler needs `ics->xc`
  (`ipc_server_handler.c:511-523`), which the default build always creates (`:438-452`,
  `XRT_FEATURE_NO_COMPOSITOR_FOR_HEADLESS_SESSIONS` off; upstream !2606).

So a container session can call `xrt_comp_begin_session` and `oxr_frame_sync_begin_session`
internally — at `xrCreateSession` or at the first container — and keep `xrWaitFrame` returning
real timing while the state stays IDLE.

### 1.4 Where per-client state lives, and what the in-process build changes (Q2)

- **Two structs already split one fact.** `struct ipc_client_state` (`ipc_server.h:99-218`)
  holds `xs` `:149`, `xc` `:152`, `xspcs[IPC_MAX_CLIENT_SPACES=128]` `:192` (`:68`), and
  `client_state` — `struct ipc_app_state` (`ipc_protocol.h:387-401`: `primary_application`,
  `session_active/visible/focused/overlay`, `io_blocks`, `z_order`, `pid`, `info`), the record
  `libmonado`/`monado-ctl` read. `struct multi_compositor::state`
  (`comp_multi_private.h:124-133`: `visible`, `focused`, `z_order`, `session_active`,
  `is_base_session`) is the compositor's copy. `session_visible` (IPC, under `global_state.lock`)
  and `mc->state.visible` (comp_multi, unlocked) are the same fact in two places today.
- **The one-primary rule.** `update_server_state_locked` (`ipc_server_process.c:598-656`):
  early-out if unchanged (`:604-611`); fallback = last non-overlay client with `session_active`
  (`:627-634`); demote a non-displayable active client (`:638-644`); `-1` = idle wallpaper
  (`:649-651`); `flush_state_to_all_clients_locked` (`:580-595`). **Why** (`:615-620`): "our
  active application has changed — this would typically be switched by the monado-ctl
  application or other app making a 'set active application' ipc call, or it could be a
  connection loss resulting in us needing to 'fall through' to the first active application, or
  finally to the idle 'wallpaper' images." This is *the* shell-decides mechanism in tree; it is
  one-primary-plus-overlays, not N slots.
- **Slots.** `msc->clients[MULTI_MAX_CLIENTS=64]` (`comp_multi_private.h:376`, `:36`, todo
  "make dynamic" `:33`) under `list_and_timing_lock` (`:362-366`); insertion at the first NULL
  slot, **silently ignored when full** (`comp_multi_compositor.c:1028-1037`, `:1030`). Each slot
  owns three `multi_layer_slot`s (`:86-92`; `progress`/`scheduled`/`delivered` `:175-188`), a
  pacer (`u_paf_create`, `:1026`) and a wait thread (`:1014`, `:1048-1057`). The render thread's
  `transfer_layers_locked` delivers or retires per visibility (`comp_multi_system.c:266-302`),
  sorts by `z_order` (`:305`, `overlay_sort_func` `:210-225`) and picks the base session
  (`:307-318`).
- **The in-process target is a real second implementation, with one asymmetry.**
  `target_instance.c:69-161` builds `b_system`, the devices and the space overseer
  (`targets/helpers/target_builder_helpers.c:73` `b_space_overseer_create(broadcast)`) and wraps the main
  compositor in `comp_multi` (`comp_compositor.c:1316`); the service does the same through
  `xrt_instance_create_system` (`ipc_server.h:396-406`); the IPC *client* implements the same
  vtables (`ipc_client_instance.c:143-147`, `:222-230`). `xrt_space_overseer`, `xrt_system`
  and `xrt_session` each have a direct (`b_*`) and a proxy (`ipc_client_*`) implementation.
  **`xrt_multi_compositor_control` does not**: only `comp_multi` implements it
  (`comp_multi_system.c:843-850`); `ipc_client_create_system_compositor`
  (`ipc_client_compositor.c:1018-1036`) never sets `xmcc`, so it is NULL in every app process,
  and `oxr_session.c:1327-1330`'s "grant visible + focused, z 0" fires **only in-process**. In
  the in-process build there is no `ipc_server_process.c`: `oxr` itself is the policy. **Why**
  the boundary is shaped this way (`xrt_space.h:86-91`): the space graph "isn't exposed there is
  no need to synchronise it across the app process and the service process" — proxy and direct
  implementations are meant to be interchangeable.
- **Adding IPC calls** is mechanical: `50-*.json` + `proto.py` (`src/xrt/ipc/CMakeLists.txt:22-38`;
  `proto.py:61-135` client stubs, `:256-430` dispatch) → `ipc_handle_<name>` in a handler file.
  Worked example `space_create_pose`: `50-space.json:25-33` → `ipc_client_space_overseer.c:115-132`
  → `ipc_server_handler.c:650-681` (`GET_XDEV_OR_RETURN` `:660`, real call `:663`, id
  `ipc_server_objects_get_xspc_id_or_add` `:669`). Handles cross IPC as `uint32_t` indices into
  per-client arrays (`ipc_server_objects.c:160-200`; client wrapper `ipc_client_space_overseer.c:23-30`).
  Since !2879 (merged 2026-09-15) there is a per-application system-level object,
  `xrt_app_system` (`xrt_app_policy.h`, `b_app_policy.c`) — "hold per-application state … which
  needs the per-application data to exist at a system level, rather than a session level"
  [external, §2.4].

### 1.5 The container space in the overseer (Q7)

`b_space_overseer.c` is the only implementation: `enum u_space_type {NULL, POSE, OFFSET, ROOT,
ATTACHABLE, ZOMBIE}` (`:44-64`); `struct u_space` (`:70-102`); a `pthread_rwlock_t` (`:112`) and
the `broadcast` sink (`:124`). `create_offset_space` (`:630-653`) stores `offset.pose`;
`create_pose_space` (`:655-698`) binds an `xdev`. **Poses are mutable server-side but only through
internal helpers**: `update_offset_write_locked` (`:265-277`, asserts type NULL/OFFSET) is used
by `set_reference_space_offset` (`:1163-1240`, semantic spaces only, then broadcasts
`REFERENCE_SPACE_CHANGE_PENDING` `:1205-1235`) and `recenter_local_spaces` (`:929`). There is
**no public setter for an arbitrary offset space**; `xrt_space.h` is silent. "Unlocatable" is
already a first-class outcome: `U_SPACE_TYPE_ZOMBIE` pushes `XRT_SPACE_RELATION_ZERO`
(`:391-393`, `:424-426`; `xrt_defines.h:721`, flags `BITMASK_NONE = 0` `:691`), and
`oxr_space_locate` (`oxr_space.c:464-538`) maps `relation_flags == 0` to `locationFlags = 0`
with an identity pose (`:520-538`) — exactly `ext_spatial_container.adoc:309-313`. Over IPC a
space is a `space_id` (`ipc_client_space_overseer.c:134-158`; server
`ipc_server_handler.c:684-710`), so a server-created container space needs **no new space IPC**:
`oxr_space` wraps it with a new `oxr_space_type` beside `OXR_SPACE_TYPE_XDEV_POSE`
(`oxr_objects.h:1710-1734`, `:1727`; `oxr_space.c:100-130`, `:254-284`). Upstream's stance on
overseer growth (!2284 review, Jakob): "work on the semantic level rather then the `xrt_space`,
the less special they are the better" [external].

### 1.6 Locking, threads and the event union (Q8, mechanism side)

- `comp_multi`: `list_and_timing_lock` guards `clients[]` and timings; `slot_lock` guards
  `scheduled`; `oth` guards `sessions.state/active_count` (`comp_multi_private.h:168`,
  `:346-366`). `set_state`/`set_z_order` write `mc->state` **without a lock**
  (`comp_multi_system.c:648`, `:672`). The render thread (`multi_main_loop`, `:526-625`) pushes
  no session events.
- IPC server: `global_state.lock` around every policy mutation (`ipc_server_process.c:836,846,886,910,923`).
- `oxr`: no per-session state mutex; `active_wait_frames_lock`, `frame_sync.mutex`, instance
  `event.mutex` (`oxr_objects.h:1368,1379,1205`); `oxr_session.c:1079-1080` `//! @todo this
  should be carefully synchronized, because there may be more than one session per instance.`
- `union xrt_session_event` (`xrt_session.h:38-72`, `:203-215`): eleven types; payloads 4–48
  bytes; the rule (`:198`) "Each event struct must start with a xrt_session_event_type field".
  One struct per type is the convention (`STATE_CHANGE` and `OVERLAY_CHANGE` are separate even
  though both are booleans). Nothing in tree bounds or coalesces either queue; the one
  deduplication is !2530's same-state no-op.

### 1.7 The Galaxy XR branch — a negative, and one transferable idea

`monado-galaxyxr` = upstream `2f194e366` (2026-07-06) + 38 commits by Stanislav Aleksandrov
(Feb–Aug 2026): the headset driver, a dual-DRM-lease compositor backend and camera passthrough
(`7c90ff5`, 31 files), eye tracking (`63ac02c`), eye-tracked foveation (`6ea9442`). An
independent bring-up, not Samsung's runtime; `README.md` is upstream's. It adds **no** OpenXR
extension (`oxr_extension_support.py` and `oxr_api_negotiate.c` byte-identical to the pin) and
contains no `spatial_container` symbol. Its one session-state-adjacent delta —
`XRT_ERROR_DEVICE_LOST` mapped to `LOSS_PENDING` through an `xrt_syscomp_error_listener`
(`monado-galaxyxr/src/xrt/include/xrt/xrt_compositor.h:2538-2547`;
`oxr_session.c:1299-1311`) — is the path on which the spec's "one `ClosedEXT` per container
before `LOSS_PENDING`" (`:1085-1089`) would sit (`oxr_session_poll:716-720`).

## 2. Upstream direction — what the maintainers have already said [external]

Read so the series lands where Monado is going, not where it was. GitLab notes are 401
unauthenticated; discussions were read via `…/merge_requests/<N>/discussions.json`.

### 2.1 Session state: "running" is what calls were made, not what events were polled

- **!2344** (Rylie Pavlik, merged 2024-11-07) introduced `oxr_frame_sync` after !1934 (2023,
  closed) showed the semaphore could not be reset by `xrEndSession` (Jakob: "We might have to
  replace the semaphore with a `os_mutex`, `os_cond` and a flag"). Rylie: "session running
  depends entirely on what calls have been made, and not on the session state events that were
  polled"; "I verified with the WG that my semantics here are correct, CTS verifying this will
  be coming soon." https://gitlab.freedesktop.org/monado/monado/-/merge_requests/2344
- **`deferred-session-state`** (Rylie, two title-only commits 2024-11-08, `fde09384`
  "st/oxr: Support callbacks associated with polling events", `9525ddd2` "st/oxr: Adjust how
  session state changes take effect"; no MR, never merged; `main` has no `target_state` or
  poll callback). Its sketch: `sess->state` "is updated when a state change event is polled",
  `target_state` "is updated when the event is queued"; auto-transitions in `oxr_session_poll`
  only when `state == target_state`. **A signal, not a license** (rule 7): it names the
  weakness — `sess->state` is not trustworthy while events are queued — without upstream having
  adopted a fix. https://gitlab.freedesktop.org/api/v4/projects/monado%2Fmonado/repository/compare?from=main&to=deferred-session-state
- **!2530** (Simon Zeni, merged 2025-07-15): same-state change is a no-op; triggered by CTS
  PR #110 on `XR_MND_headless`, whose `has_ended_once` "is never called by the headless path".
  **!2598/!2647** (Christoph Haag, 2025-10/11): real timestamps in state-change events — "the
  client compositor does not pass it on to the app as the service side compositor cycle is
  different to the client compositor cycle."
- **Headless history**: !1808 (Jakob, merged 2023-05-18) "Always create the system compositor"
  — "we never enforced that if you gave headless you couldn't use any graphics API … we would
  need to create the system compositor"; !2606 (Jakob, merged 2026-04-03) made *not* creating a
  native compositor for headless an opt-in build feature.

### 2.2 Events: the sanctioned channel, and the NAK on listeners

- **!1354** "Draft: Bubble compositor events through the multi" (Rylie, opened 2022-05-27,
  still a draft; 2026-07-15: "too hard to rebase I think"). Jakob's review (2022-05-27): "we
  shouldn't be pushing the events from the native compositor directly to the client. The
  end-goal 'proper' solution would be to add a `poll_events` function to
  `xrt_multi_compositor_control`." https://gitlab.freedesktop.org/monado/monado/-/merge_requests/1354
- **!2062** "Add xrt_system and xrt_session" (Jakob, merged 2023-12-11) is what shipped
  instead: "The changes was done so that events could flow directly from a session object
  instead of the compositor. Letting us more easily add space events." This is §1.1's
  `xrt_session_event_sink` / `b_session` path; !2830 (merged 2026-06-23) fixed the
  destroy-vs-push race. **!2343** (Rylie, open) carries a sink into the compositor so it can
  report "they unplugged the headset"; Jakob's one note is naming: the broadcast sink is called
  `broadcast`.
- **`wallbraker/monado-collabora:jakob/comp/multi-interface`** (2023-05-17, one commit, one
  file): `struct comp_multi_listener { client_connected; client_disconnected; sesion_begin;
  sesion_end; shutdown; }` in `comp_multi_interface.h` — compositor→service lifecycle
  notifications, never wired, no MR. It is the "listener interface" composition §5.2(ii)
  names; its content is narrower than the seam needs and it is abandoned.
- **!2573** "xrt: introduce list & signal" (Simon Zeni, 2025-09-03, open) — Jakob: "I'm not too
  enthused about having mutable things in the `xrt_` interface … This list would probably
  require locking, and I really don't want to have to include `os_mutex` in the `xrt_`
  interface … Consider this post a NAK." Proposed instead: a `register_pre_destroy_callback`
  on the owning object. https://gitlab.freedesktop.org/monado/monado/-/merge_requests/2573
- **Queue bounds / coalescing**: no MR or issue exists. Overlay pacing issue #345 (2024-03):
  "zero layers means display black (or transparent) for at least a frame, it should be treated
  just like submitting."

### 2.3 Header bumps: one commit, seven files, a changelog fragment

The last six (`!2814` 1.1.53→58 2026-04-02; `!2603` →53; `!2578` →51; `!2546` →50; `!2453`
1.1.42→47; `!2353` 1.1.36→42) each replace exactly the OpenXR-SDK `include/openxr/` set —
`openxr.h`, `openxr_loader_negotiation.h`, `openxr_platform.h`, `openxr_platform_defines.h`,
`openxr_reflection.h`, `openxr_reflection_parent_structs.h`, `openxr_reflection_structs.h` —
plus `doc/changes/state_trackers/mr.NNNN.md` ("Update OpenXR headers to latest SDK version"),
with `OPENXR_REV_ID:` naming the SDK commit (!2453). Monado tracks the **SDK release headers**,
not `xr.xml`. The only non-header edits were forced by new enum values in exhaustive switches
(!2814: `oxr_defines.h`, `oxr_conversions.h`, `oxr_objects.h`, `oxr_pretty_print.c`,
`oxr_space.c`; Jakob: "Once you have added the missing enum entry in the switch case feel free
to assign to marge"). **None** touched `oxr_extension_support.py`, `oxr_api_negotiate.c`,
`oxr_api_funcs.h` or CMake; those belong to extension-enable MRs (e.g. !2860
`XR_EXT_composition_layer_inverted_alpha`: `MonadoSetOptions.cmake`, `xrt_config_build.h.cmake_in`,
`scripts/mapping.imp`, `oxr_extension_support.py`, the `oxr_*` and `xrt_*` files). SDK releases
in between: 1.1.59 (2026-04-30), .60, .61, .62, **1.1.63 (2026-09-02)**, whose `openxr.h`
carries both extensions (lines 1076-1100, 14576-14831). Five of the six bumps merged with no
discussion.

### 2.4 Nothing upstream on containers; one new home for per-app state

- Issues and MRs (all states) for `spatial container`, `spatial_container`, `EXT_spatial`,
  `spatial`, `container`: **none relevant**; the 50 most recently active forks' branches for
  `container`/`spatial`: **none** (method check: `rpavlik/monado?search=bubble` finds
  `bubble-comp-events`). Monado implements no `XR_EXT_spatial_*` at all.
- **!2879** "xrt: Add per-application instance/system policy" (Beyley Cardellio, merged
  2026-09-15): `xrt_app_instance`/`xrt_app_system` (`xrt_app_policy.h`, `b_app_policy.c`) — "hold
  per-application state such as resolution … which needs the per-application data to exist at
  a *system* level, rather than a session level"; "'application' as a concept is intentionally
  up to the state trackers (in OpenXR we are kinda defining this to be equivalent to an
  `XrInstance`)". Simon Zeni: "I like this design, I wish we could have more interface objects
  in Monado." https://gitlab.freedesktop.org/monado/monado/-/merge_requests/2879
- `XR_EXTX_overlay`'s original MR (!398, Jakob, merged 2020-06-25) has no written rationale;
  its design is the header comments quoted in §1.1. !2341 raised the overlay limit 16→128.

### 2.5 Tooling upstream

- **`Ralith/openxrs`** master: `CURRENT_API_VERSION` 1.1.58, zero `spatial_container` hits;
  `sys/OpenXR-SDK` submodule at SDK 1.1.58; last bump PR #214 (2026-04-09) documents generator
  pitfalls (digit-leading enum variants, ANDROID `cfg`); no 1.1.6x PR or issue. Regeneration is
  one command (`generator/src/main.rs:24-42`: `cargo run -- <path/to/xr.xml>`, overwriting
  both `generated.rs` files).
- **OpenXR-CTS** — pinned during this pass as `references/openxr-cts` at tag
  `openxr-cts-1.1.63.0` (`c4d9194d6`, 2026-09-10): no `spatial_container` test
  (`src/conformance/conformance_test/` has `test_XR_EXT_spatial_{anchor,marker_tracking,persistence,persistence_operations,plane_tracking}.cpp`
  only). The policy-dependent test shape is `test_XR_EXT_user_presence.cpp` (92 lines):
  `SKIP` if the extension is not enabled (`:38`) or the system-properties bool is false
  (`:49`), `FrameIterator::RunToSessionState(READY)` (`:54-55`), `xrBeginSession` (`:59`),
  then assert only the *existence* of the guaranteed event (`:90`) — "We don't require a user
  to be present for running automated tests" (`:84`). `test_XR_MND_headless.cpp` is the
  no-graphics precedent: `AutoBasicSession::skipGraphics` (`:41`), `RunToSessionState` through
  READY → SYNCHRONIZED/VISIBLE/FOCUSED → STOPPING (`:43`, `:67-69`, `:73`) — a container probe
  needs the opposite: assert IDLE is reached and nothing else arrives.
- **OpenXR-SDK** — pinned as `references/openxr-sdk` at `release-1.1.63` (`f2448a879`):
  `include/openxr/openxr.h:29` is `XR_MAKE_VERSION(1, 1, 63)`; both extensions present
  (`:14576-14580`, `:14756-14759`). This is the directory the header-bump series copies.

## 3. The client and the registry — Godot cannot gate C1 (Q4, Q6, Q9)

### 3.1 Godot's module is hard-coupled to `_self_rendering`

```54:56:references/godot/modules/openxr/extensions/spatial_container/openxr_spatial_container_extension.cpp
bool OpenXRSpatialContainerExtension::is_enabled() const {
	return spatial_container_ext && rendering_mechanism && rendering_mechanism->is_enabled();
}
```

`rendering_mechanism` is the self-rendering singleton (`:121`); its `is_enabled()` is the
extension bool (`…_self_rendering_extension.cpp:56-58`). The header says why: "For now we are
hardcoding self rendering as the rendering mechanism" (`openxr_spatial_container_extension.h:148-149`).
Every other path is gated on that predicate: `XrSystemSpatialContainerPropertiesEXT` is not
chained (`:169-176`), `XrSessionCreateInfoSpatialContainersEXT` is not chained (`:160-167`),
`can_create_spatial_container` prints the unconditional `Max spatial container count: 0` and
returns false (`:62-65`), `on_event_polled` returns before its switch (`:178-181`), and
`OpenXRAPI::is_spatial_container_enabled()` (`openxr_api.cpp:3206-3209`) routes `xrBeginSession`
(`:1459`), `xrLocateViews` (`:2051`) and `xrEndFrame` (`:3131-3132`) down the core path.
Negotiation itself succeeds: `openxr_api.cpp:573-608` enables `XR_EXT_spatial_container` on the
instance whenever the runtime lists it. **Verdict: degrades** — and exercises none of the C1
surface beyond `xrGetInstanceProcAddr` of eight names (`:93-104`; one of them,
`xrEnumerateSupportedSpatialContainerGraphicsPresentationsEXT`, is loaded and **never called**
anywhere in the module).
A C1-only runtime therefore sees Godot create an *ordinary* session with the extension
*enabled* on the instance, and must accept that combination (the opt-in is per session,
`ext_spatial_container.adoc:158-160`).

**What Godot will do at C2** (so C1 gets it right in advance): chain the system properties and
the session struct on top of its Vulkan binding; in `on_session_created`, *before any
session-state event*, `xrCreateSpatialContainerEXT(graphicsPresentation = SELF_RENDERING,
suggestedBounds = project setting)` (`:428-434`), `xrCreateSpatialContainerSpaceEXT`
(`:441-447`), `set_custom_play_space` (`:454`), `xrRequestSpatialContainerBoundsModeEXT(BOUNDED)`
(`:457-460`), `xrRequestSpatialContainerVisibleEXT(true)` (`:463-465`); on IDLE synthesise
ready + synchronized without `xrBeginSession` (`openxr_api.cpp:2341-2350`); map
`VISIBLE_CHANGED` → `on_state_visible/synchronized`, `INTERACTABLE_CHANGED` →
`on_state_focused/visible`, `CLOSED` → `on_state_exiting` + destroy
(`openxr_spatial_container_extension.cpp:216-273`); on LOSS_PENDING synthesise stopping without
`xrEndSession` (`:2367-2375`); read state via `xrGetSpatialContainerStateEXT` right after IDLE
(Mura's `main.gd:213-221`), so the record must be coherent at create. It never sets
`retainPreviousSubmission` (`…_self_rendering_extension.h:112`, `.cpp:236`, both `false`) and
never uses volume clipping. Its error handling is `print_line`/`ERR_PRINT` throughout
(`openxr_api.cpp:1467-1470`, `:2096-2099`, `:3135-3138`); `get_error_string` goes through
`xrResultToString` (`:325-336`), so the runtime's string table must carry the four new codes.

### 3.2 The registry defines the surface — and one hole

`xr.xml:24456-24520` (`XR_EXT_spatial_container`, 811): handle `XrSpatialContainerEXT` parent
`XrSession` (`:6966`); eight commands (protos `:13160-13198`); seventeen structs (`:6972-7071`,
`XrStructureType` offsets 0–16 with **offset 2 unassigned**, `:24484-24499`); enum
`XrSpatialContainerBoundsModeEXT` `BOUNDED=1`, `IMMERSIVE=2` (`:9658-9661`); four result codes
(`:24501-24504`: `SPATIAL_CONTAINER_CLOSED`, `SPATIAL_CONTAINERS_ENABLED`,
`SPATIAL_CONTAINERS_NOT_ENABLED`, `COMPATIBLE_SPATIAL_CONTAINER_MISSING`); `xrBeginSession`,
`xrEndSession`, `xrRequestExitSession`, `xrLocateViews` extended with `SPATIAL_CONTAINERS_ENABLED`
(`:24515-24518`). **`XrSpatialContainerGraphicsPresentationEXT` is declared empty**
(`:9656-9657`); its only value, `…_SELF_RENDERING_EXT` (1000813000), is contributed by 814
(`:24560`). `xr.xml:24536-24573` (814): three commands (`:13208-13225`), nine structs
(`:7077-7137`), five result codes (`:24561-24565`).

So a runtime advertising 811 alone offers **no spec-valid `graphicsPresentation` value** for
`xrCreateSpatialContainerEXT`, and the spec text states no rule for an unenumerated value at
create — only `xrBeginSpatialContainerRenderingEXT` has `GRAPHICS_PRESENTATION_MISSING`
(`ext_spatial_container_self_rendering.adoc:118-121`). This is the base extension's own
statement that presentation is "handled by other extensions" (`ext_spatial_container.adoc:339-357`)
taken to its conclusion: **the base extension is not independently exercisable by a client.**

### 3.3 Probe tooling

- Monado's `tests/` are catch2 unit tests linked against internal libraries
  (`tests/CMakeLists.txt:59-68`); none drives the OpenXR API end to end (the only `oxr` test,
  `tests_input_transform.cpp`, includes internal headers). Upstream's end-to-end story is
  `hello_xr` and the CTS (`doc/howto-remote-driver.md:38`; `CHANGELOG.md:378,393,3308`).
- The raw-entry-point pattern in the corpus: opencomposite's `XR_BIND` (`OpenOVR/Misc/xrutil.cpp:44-45`
  — `xrGetInstanceProcAddr` into a `class XrExt` function table, `xr_ext.h:55-87`, because "the
  SDK provides no prototypes for extension functions"); xrizer's `fakexr/src/lib.rs:191-230`
  (the loader side — match the C name, `mem::transmute` to the `pfn` type) and
  `Entry::from_get_instance_proc_addr` (`src/openxr_data.rs:118-120`) for pointing the crate at
  any GIPA; `openxrs`' `ExtensionSet::other` for unknown names (`generated.rs:351-352`).
- Options, costed: **(i)** a Rust binary in `pkgs/` on `openxrs` regenerated from the pinned
  1.1.63 `xr.xml` (one command; a vendored crate until upstream bumps); tests the full surface
  and the §7.6 negatives with typed `Result` constants; grows into the C2 probe by three
  functions. **(ii)** Rust with hand-written FFI for 8+3 entry points and ~20 `#[repr(C)]`
  structs — smallest, fragile, throwaway. **(iii)** a C client inside the fork — the natural
  home for *internal-state* assertions, but a new harness category upstream has never had.

## 4. Comparables outside Monado — the same problem in 2D, and in the other 3D shells

All clones are depth-1, so "why" is in-tree text (protocol descriptions, header and code
comments, READMEs). Read under the frame; the target's own text is the anchor: per-app limit
normative (`ext_spatial_container.adoc:155-157`); deferrals queue no event and no-op changes
queue none (`:366-377`); not shown before the first request, afterwards the runtime's, "system
UI like a taskbar" (`:481-489`); bounds events per-frame *or* one at the end, old bounds
reported until the event is queued, and **"Runtimes must: coalesce multiple unpolled bounds
changed events"** (`:762-778`).

### 4.1 xdg-shell — the protocol (`wayland-protocols/stable/xdg-shell/xdg-shell.xml`)

- **Mapping is compositor-gated and "mapped ≠ visible".** "the client must perform an initial
  commit without any buffer attached. The compositor will reply with … an xdg_surface.configure
  event. The client must acknowledge it and is then allowed to attach a buffer to map the
  surface" (`:436-442`); "a mapped surface is not guaranteed to be visible once it is mapped"
  (`:444-445`). The container rule "must not show before the app's first request"
  (`ext_spatial_container.adoc:481-483`) is this gate with the direction reversed: xdg needs a
  buffer before showing, containers need an explicit request.
- **Every request is request-and-the-compositor-decides.** `set_fullscreen`: "Whether the
  client is actually put into a fullscreen state is subject to compositor policies"
  (`:1084-1087`); `set_maximized` likewise (`:1034-1036`); `set_minimized`: "There is no way to
  know if the surface is currently minimized, nor is there any way to unset minimization"
  (`:1131-1135`); move/resize "The server may ignore" (`:761-763`, `:800-801`); size hints
  "The compositor may decide to ignore the values" (`:969-971`). `IMMERSIVE` ↔ fullscreen,
  deniable, is exactly this posture.
- **States as events, with a warning against over-reading them.** `activated`: "Do not assume
  this means that the window actually has keyboard or pointer focus" (`:866-871`) — the
  `interactable` analogue; `suspended` (v6): "not ordinarily being repainted … occluded … or
  its outputs are switched off" (`:909-914`) — the "hidden ⇒ stop rendering" analogue.
- **Configure is atomic and the client may drop stale ones.** Role events before
  `xdg_surface.configure` are "a set of atomically applied configuration states" (`:603-608`);
  "If the client receives multiple configure events before it can respond to one, it is free to
  discard all but the last event it received" (`:614-615`). The container spec puts that
  coalescing on the **runtime** (`:773-777`) — stronger than xdg.
- **Capabilities, and their change.** `wm_capabilities` (v5): "If a capability isn't supported,
  clients should hide or disable the UI elements … The compositor will ignore requests it
  doesn't support" (`:1220-1230`); "When the capabilities change, compositors must send this
  event again" (`:1233-1235`). `configure_bounds` (v4): a recommended maximum "so that a surface
  isn't created in a way that it cannot fit" (`:1189-1208`). The geometry anchor on resize is
  the compositor's (`:820-823`); the client owns only surface-local `set_window_geometry`
  (`:511-558`). `close` is "only a request … The client may choose to ignore" (`:1174-1185`)
  — weaker than `ClosedEXT`, which is a fact.
- **Why (as far as the text says):** bounds → "cannot fit"; capabilities → dead UI; atomic
  configure → synchronisation. Version markers only (`:1187`, `:1211`); no changelog.

### 4.2 wlroots (`types/xdg_shell/`, `include/wlr/types/wlr_xdg_shell.h`)

- **Four-way state on the object, thin per-client.** `struct wlr_xdg_toplevel { current,
  pending; scheduled; requested }` (`wlr_xdg_shell.h:192-206`): `scheduled` = "Properties to be
  sent to the client in the next configure event" (`:200-201`); `requested` = "Properties that
  the client has requested. Intended to be checked by the compositor on surface map and state
  change requests … and handled accordingly" (`:203-206`). `struct wlr_xdg_client` is a surface
  list plus ping (`:39-48`).
- **The library applies nothing.** `set_fullscreen` records `requested.fullscreen` and emits a
  signal (`wlr_xdg_toplevel.c:385-424`); the header: "the compositor has to handle state
  requests by sending a configure event, even if it didn't actually change the state … every
  compositor … *must* listen to these signals and schedule a configure event … not doing so is
  a protocol violation" (`wlr_xdg_shell.h:214-220`).
- **Coalescing = one idle source per surface.** `wlr_xdg_surface_schedule_configure`: if
  `configure_idle == NULL` take a serial and `wl_event_loop_add_idle(…)`, else return the
  scheduled serial (`wlr_xdg_surface.c:164-179`); every `wlr_xdg_toplevel_set_*` writes
  `scheduled.*` and schedules (`wlr_xdg_toplevel.c:593-666`). Outstanding configures live in an
  unbounded `configure_list` (`wlr_xdg_surface.c:137`); ack destroys older entries (`:92-99`);
  a wrong serial is a protocol error (`:86-90`). Bound: **silent**.
- Initial commit and unmap reset (`:319-328`, `:26-44`); buffer before configure rejected
  (`:286-290`); default `wm_capabilities` = all four (`wlr_xdg_toplevel.c:501-509`).

### 4.3 smithay (`src/wayland/shell/xdg/`, `src/wayland/compositor/`) — the base zxr sits on

- **Same split, in Rust, stored on the surface.** `initial_configure_sent` (`mod.rs:195-200`),
  `pending_configures: Vec<…>` — "All pending configures that are older than the acknowledged
  one will be discarded" (`:201-206`), `server_pending` (`:207-208`), `last_acked` — "should be
  cloned to the current during a commit" (`:209-212`); `current_server_state` prefers the
  newest unacked configure so as not to "loose some state that was previously configured and
  sent, but not acked" (`:253-270`). `with_states(surface, |SurfaceData| …)`
  (`compositor/mod.rs:400-406`) is "a general purpose container for associating state to a
  surface, double-buffered or not" (`:70-73`).
- **Coalescing is the handler's.** `with_pending_state` mutates `server_pending`
  (`:1698-1716`); `send_pending_configure` emits only if `has_pending_changes()`
  (`:1472-1478`); no idle source — the compositor calls send at a frame boundary. `pending_configures`
  bound: **silent**.
- **Defaults with no policy.** `fullscreen_request` and `maximize_request` default to
  `surface.send_configure()` — an *unchanged* configure (`:1129-1140`); `minimize_request`,
  `unfullscreen_request`, `unmaximize_request` default to no-op (`:1134-1146`); on
  `SetMinimized`: "This has to be handled by the compositor, may not be supported and just
  ignored" (`handlers/surface/toplevel.rs:156-161`). On unmap the pending list is kept "because
  there's no way for a surface to tell an in-flight configure apart from our next initial
  configure" (`:1633-1655`).
- `ShellClientData { pending_ping, data: UserDataMap }` (`:1301-1305`); module scope: "the
  positioning of windows … is out of its scope" (`compositor/mod.rs:19-21`).

### 4.4 weston libweston-desktop (`libweston/desktop/`, `include/libweston/desktop.h`)

- **Mechanism/policy split as a callback table.** `struct weston_desktop_api { …, committed,
  move, resize, fullscreen_requested, maximized_requested, minimized_requested, … }`
  (`desktop.h:59-128`); only `surface_added/removed` are asserted (`libweston-desktop.c:61-62`);
  the table is copied by `struct_size`, so a shell may omit the rest (`:68-70`). Every request
  delegates (`xdg-shell.c:921-941`, `:958-968`) through NULL-checked wrappers (`libweston-desktop.c:221-229`).
- **No handler ⇒ the capability is withheld, not defaulted.** `weston_desktop_fullscreen_supported()
  = api.fullscreen_requested != NULL` (`:231-234`); `wm_capabilities` on `get_toplevel` is
  built from these predicates (`xdg-shell.c:1658-1685`), so an unhandled request falls to xdg's
  "The compositor will ignore requests it doesn't support" (`xdg-shell.xml:1228-1230`). The
  library itself validates committed geometry against configured maximized/fullscreen size
  (`xdg-shell.c:1119-1145`).
- **Coalescing with cancellation.** One `configure_idle` per surface (`:80-81`);
  `schedule_configure` compares pending against the last configured state and *cancels* a
  scheduled idle if the state became equal again (`:1536-1620`, `:1605-1618`). List bound:
  **silent**.
- **Default placement is the shell's, and random.** desktop-shell: output under the pointer,
  else random; within the work area, `x += random() % range_x` (`desktop-shell/shell.c:3880-3942`,
  `:3928-3941`). "positioning is driven by the shell alone" (`desktop.h:96-98`, on Xwayland).
- **Assumption that does not transfer:** the shell is linked in; "no shell" means "callback
  NULL", never "shell disconnected at runtime". xdg's capability-change rule (`:1233-1235`)
  is what would bridge that gap.

### 4.5 SurfaceFlinger (`aosp-frameworks-native`) — engineering evidence only (rule 2)

- **Server-side state, per-frame snapshot.** "RequestedLayerState is a simple data class that
  stores the server side layer state. Transactions are merged into this state … The states can
  always be reconstructed from LayerCreationArgs and a list of transactions"
  (`FrontEnd/readme.md:102-108`); it "does not store any other states or states pertaining to
  other layers" (`RequestedLayerState.h:31-35`) but tags `ownerUid/ownerPid` (`:108-113`).
  Why: "optimize for predictability and performance because state generation is on the hotpath
  … avoiding contention" (`readme.md:80-85`).
- **Last buffer stays.** `externalTexture` is replaced only on `eBufferChanged`
  (`RequestedLayerState.cpp:182-184`); one buffer per vsync, "If multiple buffers are queued,
  the prior ones will be dropped" (`Layer.h:141-144`).
- **The only limit is a global leak fuse.** `static const size_t MAX_LAYERS = 4096`
  (`SurfaceFlinger.h:537`); `checkLayerLeaks()` logs and dumps at most every 10 s
  (`SurfaceFlinger.cpp:5371-5393`). Per-app limit: **silent**. WindowManager, where app-window
  policy lives, is not in the corpus.

### 4.6 OpenVR `IVROverlay` (`openvr/headers/openvr.h`) — the app-owns-pose contrast

- "Creates a new named overlay. All overlays start hidden and with default settings"
  (`:4273-4274`); `VREvent_OverlayShown` "now visible to someone and should be rendering
  normally", `OverlayHidden` "doesn't need to render frames" (`:921-922`);
  `VREvent_OverlayClosed` on the close button, apps "are responsible for responding to the
  event with something that approximates 'closing' behavior" (`:959`, `:4138-4142`).
- **The transform is the app's** — `SetOverlayTransformAbsolute` (`:4393-4394`),
  `SetOverlayTransformTrackedDeviceRelative` (`:4399-4400`), width "By default … 1 meter across"
  (`:4358-4359`); runtime-owned transforms exist only for dashboard/mountable types (`:4058-4071`).
  This is the design the container spec rejects (container space is the runtime's,
  `ext_spatial_container.adoc:279-296`), and the reason a shared WM is impossible on OpenVR.
- **Limit: global, small, advertised by error.** "The maximum number of overlays that can exist
  in the system at one time." `k_unMaxOverlayCount = 128` (`:4043-4044`);
  `VROverlayError_OverlayLimitExceeded` (`:1665`). Per-process: **silent**; no rationale for 128.

### 4.7 PipeWire native protocol (`src/modules/module-protocol-native/`)

- `MAX_BUFFER_SIZE (1024*32)`, `MAX_FDS 1024`, `MAX_FDS_MSG 28` (`connection.c:30-32`).
  `connection_ensure_size` **grows the outgoing buffer by `realloc` with no upper bound**
  (rounded to 32 KiB multiples, `:134-160`); only `realloc` failure frees the buffer and emits
  `error` (`:143-155`). Single-message cap `0xffffff → -ENOSPC` (`:708-709`).
- Flush is `MSG_DONTWAIT`, `EAGAIN` re-arms `SPA_IO_OUT` (`module-protocol-native.c:569-580`);
  any other error → `pw_impl_client_destroy` (`:469-478`, `:501-509`). A client that never
  drains is never killed for queue size — only on socket error or ENOMEM. Input flow control
  exists ("when the client is busy processing an async action, stop processing messages for the
  client", `:357-359`); output coalescing: **silent** — none.
- The only in-corpus daemon on Mura's device class chose *no per-client message cap*. Rule 6
  marks that the weakest thing to copy; rule 7 marks any cap as a no-comparable rethink.

### 4.8 The spatial shells — where a new client goes

| shell | default placement | what the client is told |
|---|---|---|
| **motorcar** (the design's ancestor) | `translate(0,0,1) · rotY((n−1)·−30°) · translate(0,0,−1.5)` — 1.5 m ahead, fanned 30° per window (`windowmanager.cpp:123-125`, `:157-162`); default 3D size 0.5 m cube (`:131-137`) | its **world** `transform_matrix` (`protocol/motorcar.xml:67-76`) because it renders with the compositor's `view_matrix` (`:107-117`); size is request/decide — "the compositor cannot set this explicitly, rather, it can only request" (`:78-85`); the window "has its own 3D space whose origin is at the center of the window" (`:41-43`) |
| **wayvr** / **wlx-overlay-s** (shared crate) | `Affine3A::from_translation(Vec3::NEG_Z)` — 1 m ahead (`wayvr/wayvr/src/windowing/window.rs:100-105`); Wayland windows `z_dist = -0.95` unless anchored (`wayvr/wayvr/src/overlays/wayvr.rs:92-96`); successive windows spread `LEFT 0.08, DOWN 0.08, CLOSER 0.06` from the last (`windowing/window.rs:330-355`; "TODO: this just uses the last spawned overlay as parent", `windowing/manager.rs:1003`) | xdg only: a 1920×1080 suggestion, `Activated`, on a 530×300 mm virtual output (`backend/wayvr/mod.rs:428-433`, `:210-213`); never a pose |
| **xrdesktop** | not the library's — the embedding shell's; example: `z = -3` grid (`examples/shell.c:483-489`); `arrange_sphere` on a 5 m sphere (`g3k-object-manager.c:237-279`) | nothing (texture mirror); "The child window's position is managed by its parent, not the WM" (`xrd-shell.c:230-233`) |
| **simula** | `setInFrontOfUser gsvs (-3)` (`SimulaViewSprite.hs:1214`), then offset by own size and face the gaze (`:235-265`) | nothing |
| **stardustxr** | root spatial = identity at the world origin unless launched with a token (`core/client_state.rs:88-89`, `:121`, `:43-50`) | only what it holds a ref to: `create_spatial` needs an owned parent (`nodes/spatial.rs:590-602`), `get_relative_transform` two owned refs (`:630-651`); the HMD is an acquirable `SpatialRef` (`objects/hmd.rs:36-49`) |

Convergence: a fixed pose in front of the head (1 m / 0.95 m / 1.5 m / 3 m), successive
windows fanned or offset, and — except motorcar and stardust — the client learns nothing about
its pose. Motorcar tells the client because the client renders in world space; the container
spec removed that need (container-local rendering), so its box *minus* the world matrix is what
`XR_EXT_spatial_container` standardised.

### 4.9 WebXR `visibilityState` [external] (immersive-web.github.io/webxr §4.1, §12.6, §13.6)

Three UA-authoritative values delivered as `visibilitychange`: `visible` (rAF at native rate,
input processed), `visible-blurred` ("may be seen by the user, but is not the primary focus.
requestAnimationFrame() callbacks MAY be throttled. Input is not processed"), `hidden`
("callbacks will not be processed until the visibility state changes"); "MAY be changed by the
user agent at any time other than during the processing of an XR animation frame"; poses are
not reported while hidden. Why (§13.6): trusted UI — "to prevent a malicious page from being
able to monitor input on other pages the user agent MUST set the XRSession's visibility state
to 'hidden' if the currently focused area does not belong to the document". Changelog: "Change
blur/focus to visibilitychange (#687)". `visible-blurred` ≈ `visible && !interactable`; "hidden
⇒ poses unreportable" ≈ unlocatable container space. The app has no request path at all —
stricter than containers.

## 5. Comparison, Q1–Q9

Each bullet: where the sources converge, where they split, and what that leaves to decide.

**Q1 Session-state mapping.**
- Converge: the state machine is driven by *polled events* and the `xrt` side is driven by
  *calls*, and upstream has already separated the two (`oxr_frame_sync`, !2344). A container
  session needs the call-side started without the event-side moving — exactly the split that
  exists. Headless proves a session with a decoupled state machine is legal; container mode is
  headless's opposite (compositor present, state frozen).
- Converge: `should_render` is already false for IDLE (`oxr_session.c:126-133`); the compositor
  side needs no begin for `wait_frame` timing, only for the multi main loop's timing feed
  (§1.3).
- Split: none between sources — but `deferred-session-state` is a warning that `sess->state` is
  a "queued" state, not a "polled" one. For a container session that never changes state after
  IDLE the warning is moot, which is a point in favour of freezing at IDLE at create.
- WebXR (§4.6): the only other spec with UA-controlled visibility as an event, and it too
  keeps the frame loop running while hidden (rAF continues at reduced rate, or not at all,
  by UA choice) — the same "paced but not rendering" shape.

**Q2 Where per-container state lives.**
- Converge (Monado): every fact the shell decides — primary, focused, io-blocked, overlay, z —
  is *stored* in `ipc_client_state.client_state` under `global_state.lock` and *pushed* to
  `comp_multi`, which keeps an unlocked copy for the render thread and emits the event. The
  service is the owner; `comp_multi` is the render-side mirror; `oxr` is the app-side latch
  (`compositor_visible/focused`). Nothing is owned by `oxr` that the service needs.
- Converge (Wayland, §4.1–4.3): the compositor owns the surface state; the client holds a
  *pending → committed* double-buffer of its *requests*, and the compositor's configure is
  authoritative. wlroots/smithay/weston all keep the toplevel record in the compositor process
  and the shell's policy decides the configure; no comparable lets the client own mapped state.
- Split (Monado, in-process build): there is no service; `oxr` grants everything
  (`oxr_session.c:1327-1330`). The comparables' answer is that the "server" is whichever
  process holds the compositor — in-process, that is the app — so the record lives behind a
  vtable implemented twice, not in `oxr` twice.
- Split (comp_multi vs ipc_server as the record's home): `EXTX_overlay` put its one bit in both
  (`ipc_app_state.session_overlay`, `mc->state.is_base_session`). The unlocked `mc->state`
  write (`comp_multi_system.c:648`) and the lease's home (`ipc_server.h`, C0) both argue for the
  IPC server as the single record and `comp_multi` as a consumer (C2). The C1/C2 boundary
  research/79 §4b drew ("no container state inside `comp_multi` in C1") holds — with the
  correction that the *hook* C1 leaves is a per-container `xrt_compositor`-side struct, not
  a comment.

**Q3 No-controller default.**
- Converge: nothing auto-shows — xdg's buffer-after-ack (`xdg-shell.xml:436-442`), OpenVR's
  "All overlays start hidden" (`openvr.h:4273`), the spec's own MUST (`:481-483`). And every
  shipping spatial shell places a new client at a fixed pose in front of the head — motorcar
  1.5 m with a 30° fan, wayvr 1 m / 0.95 m, simula 3 m, xrdesktop's example 3 m (§4.8) — then
  lets the user/WM move it.
- Split on what happens to a *request* nobody has a policy for: **weston** withholds the
  capability up front and the request is ignored (`libweston-desktop.c:231-234`,
  `xdg-shell.c:1658-1685`); **smithay** answers with an unchanged configure (`mod.rs:1129-1140`);
  **wlroots** makes answering the compositor's duty (`wlr_xdg_shell.h:214-220`); **WebXR**
  gives the app no request path at all (§4.9).
- Monado's own rule is C0's: with no lease holder, `update_server_state_locked` falls through to
  the first displayable session or the wallpaper (composition §5.3.4, `ipc_server_process.c:598-656`)
  — "the runtime is usable with no shell". Applied to containers that means the smithay/shell
  shape, not weston's: grant, and place. What converges is the *shape* (grant on first request;
  in front of the head; successive containers offset; interactable = the primary client's
  last-shown container, Monado's one-focus rule at container granularity); what has no single
  precedent is the *numbers* (1 m vs 1.5 m vs 3 m; fan vs offset). Determination §6.8 for the
  shape; owner item §7.1 for the numbers and for weston-style denial as the alternative.

**Q3a Request-and-decide vocabulary.** Every comparable is request-and-the-compositor-decides
(`xdg-shell.xml:1084-1087`, `:1131-1135`; `wlr_xdg_shell.h:203-206` `requested`; weston's
callback table). The container spec's deferral rule (`:366-371`) is wlroots' `requested` field
awaiting a decision: the record keeps `requested_visible`/`requested_bounds_mode` beside
`state`, and a controller (or §7's default) resolves them.

**Q4 Godot without `_self_rendering`.**
- Godot degrades (§3.1) and exercises no C1 surface. Neither the registry (§3.2) nor the CTS
  (§2.5) offers a client for 811 alone. Converge: **C1 cannot be gated by the sample**; the
  gate is the probe (Q9), and the sample's C1 evidence is limited to Godot's `--verbose`
  "Enabled extension XR_EXT_spatial_container" line (`openxr_api.cpp:661-665`) plus an
  unchanged C0 run — `main.gd:65` asks the singleton's `is_enabled()`, so even `SCS ext` stays
  `absent`. The C1/C2 landing question is therefore not forced by Godot
  asserting — it is forced by there being *no* client of the base extension. Owner item §7.4.

**Q5 `maxSpatialContainerCount`.**
- Monado's own per-client bounds are fixed arrays chosen for the IPC layout: `IPC_MAX_CLIENTS
  32` (`ipc_protocol.h:41`), `IPC_MAX_CLIENT_SPACES 128` (`ipc_server.h:68`), `MULTI_MAX_CLIENTS
  64` (`comp_multi_private.h:36`, "make dynamic" `:33`), `IPC_MAX_LAYERS = XRT_MAX_LAYERS`
  (`ipc_protocol.h:39`; 128 on Linux, 32 on Android, `xrt_limits.h:83-86`) — all "fixed,
  silently ignore/reject past the end", none derived from memory, and the one platform split
  is Android's smaller value.
- The spec's own reasoning (`ext_spatial_container.adoc:944-956`): the count is "limited by
  memory allocation" and the *displayed* count is the runtime's, for "highly constrained
  devices". Godot reads it once and creates exactly one.
- No comparable has a *per-client* surface/window/overlay limit: wlroots, smithay and weston
  keep unbounded per-client lists (silent); SurfaceFlinger's `MAX_LAYERS 4096` is a **global
  leak fuse** with a throttled dump (`SurfaceFlinger.h:537`, `SurfaceFlinger.cpp:5371-5393`);
  OpenVR's `k_unMaxOverlayCount 128` is **global** with a dedicated error (`openvr.h:4043-4044`,
  `:1665`). The spec forces a per-app number that nothing in the corpus computes. The only
  reasoning that transfers is SF's: the number is a leak guard, generous, and logs when hit.
  Owner item §7.3 — the shape is "a fixed per-client array like every other Monado bound
  (with a log on hit)" vs "report memory-limited, cap displayed"; any value is discretionary.

**Q6 Header prerequisite.**
- Converge: one commit, seven files, one changelog fragment, `OPENXR_REV_ID` named; switch
  fixes in the same commit; nothing else. `openxrs` is at 1.1.58 and regeneration is one
  command. Determination §6.5.

**Q7 Container space.**
- Converge: the overseer already has (i) server-mutable offset poses through an internal
  setter and (ii) a first-class "unlocatable" outcome (`ZOMBIE` → zero relation). What is
  missing is a public verb to *set* and to *toggle locatability* on one space, and a name for
  the type. Two shapes, both in-tree-shaped: (a) an `OFFSET` space under `LOCAL` with two new
  exported `b_space_overseer_*` setters and a `locatable` flag, or (b) a new `U_SPACE_TYPE_CONTAINER`
  carrying `{pose, locatable}` and its own `locate` branch. Upstream's stated preference
  (!2284) is against special space types; the counter-weight is that (a) overloads `OFFSET`
  (whose invariants `update_offset_write_locked` asserts) with a runtime-toggled flag. Owner
  item §7.5 (small; no policy content).
- Converge (spec vs overseer): the overseer's `xrt_space` is refcounted and process-local; the
  container space's lifetime must follow the container's (destroy → `ZOMBIE`-like), not the
  refcount, which is what `oxr_space_destroy` → `xrt_space_reference(NULL)` already tolerates.

**Q8 Events, IPC, budget.**
- Converge (Monado): one struct per event type, eleven types today, no bound, no coalescing,
  one per-poll IPC round-trip per session regardless of activity, `os_mutex` around both queues.
  Six new types follow the convention exactly; one discriminated type would be the first of its
  kind. Payload sizes: `VisibleChanged/InteractableChanged` 4+1 B, `BoundsChanged`
  4+4+12 B, `Closed` 4 B, `*RequestDenied` 4(+4) B — all inside the existing ≈48-byte union.
- **Correction to the plan's premise.** The spec does not merely *permit* coalescing — it
  requires it for one event: "Runtimes must: coalesce multiple unpolled bounds changed events …
  report only a single event containing the last bounds change" (`:773-777`), and "continue to
  report the old bounds from `xrGetSpatialContainerBoundsEXT` until the runtime queues a resize
  event" (`:770-772`). "Unpolled" is measured at `xrPollEvent`, i.e. the **instance queue in
  the app process** (`oxr_event.c:79-118`), which can hold several drains' worth of events.
- Converge (coalescing, 2D): one idle source per surface merges same-frame changes into one
  configure (wlroots `wlr_xdg_surface.c:164-179`; weston `xdg-shell.c:1585-1620`, cancelling
  when state reverts); smithay accumulates into `server_pending` and suppresses no-ops
  (`mod.rs:1472-1478`). All suppress no-op configures — the spec's `:375-377`. The 2D protocols
  push the *cross-frame* coalescing onto the client ("discard all but the last",
  `xdg-shell.xml:614-615`); the container spec puts it on the runtime, closer to SF's "one
  buffer per vsync, prior ones dropped" (`Layer.h:141-144`).
- Converge (bounds): no comparable bounds the per-client outstanding queue — wlroots
  `configure_list`, smithay `pending_configures`, weston `configure_list` (all silent), PipeWire
  (unbounded `realloc`, kill only on socket error, §4.7), Monado (`oxr_event.c:80-91`,
  `b_session.c:123-140`; `IPC_EVENT_QUEUE_SIZE 32` is defined at `ipc_protocol.h:43` and used
  nowhere). A numeric per-client event cap with disconnect-on-overflow has *no* comparable.
- What transfers: a **latest-wins slot per container for `BoundsChanged`** — one queued node
  that is overwritten in place while unpolled — satisfies the MUST and bounds that event kind
  by construction (≤ 1 per container). Visible/interactable toggles are real changes each
  (true→false→true is two events the app must see) and stay uncoalesced, as today's
  `STATE_CHANGE`. The seam (C3) and §7's default produce at WM cadence, so the remaining
  unbounded path is the one every comparable accepted. Determination §6.3 amended.

**Q9 Conformance and the probe.**
- Converge: the CTS shape for policy-dependent behaviour is "assert what the spec guarantees,
  skip on absent capability, never assert the runtime's *choice*"
  (`test_XR_EXT_user_presence.cpp`). Monado has no in-tree e2e harness; upstream tests via CTS.
  Rust on regenerated `openxrs` is the only option that gives typed constants for the new
  result codes and grows into the C2 probe. Determination §6.6; placement is an owner item only
  in the sense of *where in the tree* (§7.6).


## 6. Determinations (confident; precedent named)

6.1 **The record lives in the service, behind an `xrt` vtable implemented twice.** Precedent:
every shell-decided fact today (`ipc_client_state.client_state`, `ipc_server_process.c:546-577`,
`:598-656`) and every Wayland compositor in the corpus (§4). The in-process target implements
the same vtable directly with the grant-all policy `oxr_session.c:1327-1330` already applies.
`oxr` keeps only what it must answer synchronously (`xrGetSpatialContainerStateEXT`,
`xrGetSpatialContainerBoundsEXT`) as a latch updated from the events it drains — the
`compositor_visible/focused` pattern (`oxr_session.c:702-710`).

6.2 **Container mode is a third session shape, not a headless variant.** The session is created
with a compositor (the default build creates one even for headless, `ipc_server_handler.c:438-452`;
`oxr_verify.c:643-647` extended to accept the chained struct without a graphics binding);
`oxr_session.c:1605-1606` pushes IDLE and stops; `xrt_comp_begin_session` +
`oxr_frame_sync_begin_session` are called internally so `xrWaitFrame` paces with real
`predictedDisplayTime` (§1.3); `shouldRender` is false because `should_render(IDLE)` already is;
`do_synchronize_state_change` is skipped (`oxr_session_frame_end.c:1627-1633`); the headless
branch (`oxr_session.c:1083-1087`, `:469-486`, `oxr_session_frame_end.c:1801-1811`) is not
taken (`ext_spatial_container.adoc:1095-1099`). *When* the internal begin happens (create vs
first container) is a battery judgement — §7.2.

6.3 **Events are new `xrt_session_event` types through the session's sink.** Precedent:
`OVERLAY_CHANGE` (§1.1), !2062's stated purpose, Jakob's !1354 review (events through the
multi/session path, not a side channel), the !2573 NAK on listener registries. Six types, one
struct each, matching the eleven existing (`xrt_session.h:38-72`); pushed from the IPC server
via `ics->xs`'s sink, which needs one small addition: today the session's sink is reached only
as `mc->xses` inside `comp_multi` (`b_system.c:74-78` hands it over at create) — `b_session`
has no callers of its own push outside `b_session.c`, and `ipc_server` holds `ics->xs` as a bare
`struct xrt_session *` (`ipc_server.h:149`). The handler needs the sink: the `b_session`
container (`b_session.h:40-55`, `base` first) or a `b_session_from_xrt()` accessor in
`b_session.h` — one function, no `xrt_` interface change. **Coalescing exactly where the spec
demands it and nowhere else:** `BoundsChanged` is a latest-wins node per container in the
instance queue (`:773-777` — the MUST; the 2D comparables all coalesce per surface, §5 Q8),
implemented as one pointer in `oxr_spatial_container` to its unpolled bounds node, overwritten
in place by the drain and cleared by the pop; every other type is appended as today. No queue
bound (no comparable has one; upstream has none; the producers run at WM cadence). Ordering:
the interactable hand-off rule (`ext_spatial_container.adoc:618-624`) is a *producer* rule —
push the losing container's event first, in one critical section under `global_state.lock`.

6.4 **Handles cross IPC as per-client indices; the container space is a server-created
`xrt_space`.** Precedent: `xspcs[]` and `space_id` (`ipc_server_objects.c:160-200`,
`ipc_client_space_overseer.c:23-30`); `oxr_space` gains `OXR_SPACE_TYPE_SPATIAL_CONTAINER`
beside `XDEV_POSE` (`oxr_objects.h:1727`), so `xrLocateSpace` needs **no new IPC** — the
existing `space_locate_space` resolves it (`ipc_server_handler.c:684-710`). Unlocatable =
zero relation (`b_space_overseer.c:391-393`), which `oxr_space_locate` already maps to
`locationFlags 0` (`oxr_space.c:520-538`).

6.5 **The header bump is its own single-commit series, upstream-shaped.** Seven SDK 1.1.63
headers + `doc/changes` fragment naming `OPENXR_REV_ID`, plus only the exhaustive-switch fixes
the compiler forces (`oxr_conversions.h`, `oxr_pretty_print.c`, `oxr_space.c`, `oxr_objects.h`,
`oxr_defines.h` — the !2814 set). Nothing from `oxr_extension_support.py` or `oxr_api_negotiate.c`.
Corpus: pin `KhronosGroup/OpenXR-SDK` at `release-1.1.63` (flagged addition, §7.7).

6.6 **The C1 gate is a probe, not the sample.** A Rust binary in `pkgs/` on `openxrs`
regenerated from the pinned 1.1.63 `xr.xml`, shaped like `test_XR_EXT_user_presence.cpp`: skip
on absent extension; assert every *guaranteed* behaviour of `ext_spatial_container.adoc:996-1118`
and the create/destroy/space/state rules; never assert a policy *choice* (whether a visibility
request is granted). The sample's C1 evidence shrinks to Godot's verbose "Enabled extension"
line plus an unchanged C0 run (§3.1; `SCS ext` stays `absent` because `main.gd:65` asks the
singleton, which requires #814). Composition §7.6 is restated as this probe's assertion list
in the spec §11.

6.7 **Godot's enabled-but-not-opted-in combination is legal and must work.** A C1 runtime will
see `XR_EXT_spatial_container` enabled on the instance with an ordinary session (§3.1); the
opt-in is per session (`:158-160`), so every container function on such a session returns
`XR_ERROR_SPATIAL_CONTAINERS_NOT_ENABLED_EXT` and nothing else changes.

6.8 **With no controller the runtime grants and places — the shape, not the numbers.**
Precedent: composition §5.3.4's ruling (usable with no shell; Monado's fall-through,
`ipc_server_process.c:598-656`), every shipping spatial shell in the corpus (§4.8: a fixed pose
in front of the head, successive windows offset, then the user/WM moves them), and smithay's
default of answering a request rather than withholding the capability (`mod.rs:1129-1140`).
Hence: a container's first `request_visible(true)` is granted and the container is placed
head-relative in `LOCAL`; `IMMERSIVE` requests are granted (Mura's fullscreen-game model,
composition §3.4) — one immersive container at a time, the newest wins; `interactable` follows
Monado's one-focus rule at container granularity: the primary client's most recently shown
container. The alternative — weston's "withhold the capability and deny" (§4.4) — is recorded
as the other precedent-backed shape in §7.1, together with the distance/offset numbers, which
no two shells agree on.

## 7. Owner items (rule 8 — each with the comparables' actual positions)

7.1 **The no-controller default: numbers and the denial alternative.** *Decided by the
research:* the shape (§6.8). *Open:* (i) the placement constants — motorcar 1.5 m ahead with a
30° fan per additional window (`windowmanager.cpp:124-125`, `:157-159`; no comment states why —
the fan is the only in-corpus rule that keeps N windows disjoint without measuring them),
wayvr 1 m / 0.95 m with an 8 cm left/down, 6 cm
closer spread from the last window (`window.rs:100-105`, `:330-355`), simula 3 m facing the
gaze (`SimulaViewSprite.hs:1214`), the xrdesktop example 3 m (`examples/shell.c:483`); Mura's
own `wm.*` spawn rule for 2D windows (window-workspace-management §3, "below the eye line") is
the in-house comparable and the consistent choice, but it lives in zxr, not the runtime.
Consequence: this is the pose of every 3D app on a Mura with no zxr running (recovery, a bare
`monado-service` dev loop) and the pose the probe sees; nothing else. (ii) Whether a runtime
with no controller should instead **withhold** — advertise `supportsBounded = false` and deny
bounded requests (weston, §4.4; the spec allows exactly this, `:490-494`) — so that a 3D app
without a shell is always immersive, which is today's Monado behaviour (the focused session
fills the display). Consequence: simpler runtime, no placement constant in Monado, but a
bounded-only app (`supportsImmersive` is Mura's anyway) has no picture without zxr, and the
capability must flip when a controller connects (xdg permits this, `xdg-shell.xml:1233-1235`;
the OpenXR enumeration is "stable for the instance", `ext_spatial_container.adoc:966-990`, so
the flip can only affect *new* instances — a real cost).

7.2 **When the internal begin happens.** (a) At `xrCreateSession` — simplest guards
(`OXR_VERIFY_SESSION_RUNNING` always true), the multi main loop's `active_count` is nonzero
from create, so a container app with nothing shown keeps the compositor's timing feed
running (`comp_multi_system.c:549-554`); today an IDLE ordinary session costs nothing there.
(b) At the first container's first granted visibility — the IDLE cost of an ordinary session,
but `xrWaitFrame` before that returns the pacer's guess (`:860-864`) and the guards need a
second predicate. No comparable: WebXR keeps rAF alive while hidden at the UA's discretion
(§4.9); OpenVR says a hidden overlay "doesn't need to render frames" (`openvr.h:922`) but the
frame loop is the app's. Rev 0 of the spec assumes (a) and marks the move.

7.3 **`maxSpatialContainerCount`.** (a) A fixed per-client array, as every Monado bound
(`IPC_MAX_CLIENT_SPACES 128`, `MULTI_MAX_CLIENTS 64`, `IPC_MAX_CLIENTS 32`, `XRT_MAX_LAYERS`
128/32), with SF's leak-guard posture (log on hit, `SurfaceFlinger.cpp:5371-5393`); value
discretionary — the spec suggests "many" (`:944-945`), Godot creates one, `XRT_MAX_LAYERS` on
Android is 32. (b) Report a large memory-limited number and cap the *displayed* count (the
spec's own split, `:946-956`), which needs dynamic per-client storage Monado has nowhere today
(`comp_multi_private.h:33` "make dynamic" is a todo). Rev 0 assumes (a) with 16.

7.4 **`graphicsPresentation` without #814.** The enum is empty in the base extension
(`xr.xml:9656-9657`) and the spec states no create-time rule for an unenumerated value. (a)
`XR_ERROR_VALIDATION_FAILURE` — the core rule for an enum value outside the enumerated set,
what a CTS would assert; a C1-only runtime therefore has *no* creatable container, which is
honest (§3.2). (b) Accept `SELF_RENDERING`'s numeric value as an opaque tag and create — lets
a hand-written probe exercise C1's handles before C2 exists, at the cost of a behaviour #814
will later have to own. Rev 0 assumes (a); the probe (§6.6) then tests handles only from C2,
or C1 and C2 land adjacent — which is the real question: **C1 as a separate landed series
with a probe that can only test the session shape, or C1+C2 as one series gated together.**
Godot cannot decide it (§3.1); the registry makes #811 alone unexercisable by any client.

7.5 **Container space shape in the overseer.** (a) `OFFSET` under `LOCAL` + exported pose
setter + a `locatable` flag — reuses `update_offset_write_locked`'s path (`b_space_overseer.c:265-277`)
but adds a runtime-toggled flag to a type whose invariants that function asserts; (b) a new
`U_SPACE_TYPE_CONTAINER` with `{pose, locatable}` and its own `locate` branch beside `ZOMBIE`
(`:391-393`) — one more enum value against upstream's stated preference for fewer special
types (!2284). Either is ≈80 lines and has no policy content. Rev 0 assumes (b).

7.6 **Probe placement.** `pkgs/spatial-container-probe` (Rust on regenerated `openxrs`; the
CTS shape, `test_XR_EXT_user_presence.cpp`; grows into C2's probe) vs a C client in the fork
(no upstream harness slot exists — `tests/` is catch2 unit tests, `tests/CMakeLists.txt:59-68`;
upstream tests end-to-end with `hello_xr` and the CTS). Rev 0 assumes `pkgs/`.

7.7 **Corpus.** Added by this pass (rule 8, overwhelming: six bump MRs copy the SDK headers
verbatim; the CTS is the only policy-test shape): `openxr-sdk` @ `release-1.1.63`,
`openxr-cts` @ `openxr-cts-1.1.63.0`. *Not* added, for the owner: androidx SceneCore (the
app-side semantics on Android XR — rule-2 evidence only, and the open client Godot already
covers the app side).

7.8 **C0 overlap (information, not a decision).** C1 edits four files C0 just landed in
(§8.2). Series order is forced: `openxr-headers-1.1.63` → `spatial-container` rebased on
`dd8ec00cc`.

## 8. What the two series become

### 8.1 `openxr-headers-1.1.63` (prerequisite, one commit, upstream-shaped — §6.5)

| File | Change |
|---|---|
| `src/external/openxr_includes/openxr/{openxr,openxr_loader_negotiation,openxr_platform,openxr_platform_defines,openxr_reflection,openxr_reflection_parent_structs,openxr_reflection_structs}.h` | replace with OpenXR-SDK `release-1.1.63` `include/openxr/` |
| `oxr_conversions.h`, `oxr_pretty_print.c`, `oxr_space.c`, `oxr_objects.h`, `oxr_defines.h` | only the `switch` cases the new `XrReferenceSpaceType`/`XrStructureType` values force (the !2814 set; `oxr_session.c:137-150` `to_string` unchanged — no new session state) |
| `doc/changes/state_trackers/mr.<N>.md` | "Update OpenXR headers to 1.1.63", `OPENXR_REV_ID:` |

Not touched: `oxr_extension_support.py`, `oxr_api_negotiate.c`, `oxr_api_funcs.h`, CMake.
Estimate: 5–10 lines outside `src/external`. **Overlap with the landed C0 series
(`39eb2a6d5..dd8ec00cc` on the fork's `mura`): none.**

### 8.2 `spatial-container` (C1 — the base extension; no rendering, no seam verbs)

Yardsticks (Group A): `XR_KHR_visibility_mask` 14 files for one function + one event + one
device call; `XR_MND_query_egl_device` (`a55b19d08`) 10 files / +231 lines for one function;
`XR_EXTX_overlay` ≈120 lines, no new IPC. C1 = 8 functions, 6 events, 1 handle, 1 space type,
1 properties struct, 1 create-chain struct → ≈2,100–2,300 lines over ~30 files, ≈8× overlay.

| Area | File | Change | ≈lines |
|---|---|---|---|
| build | `CMakeLists.txt:414-422`, `xrt_config_build.h.cmake_in:71-107`, `scripts/mapping.imp` | `XRT_FEATURE_OPENXR_SPATIAL_CONTAINER` option + cmakedefine (pattern `:417`/`:101`) | 5 |
| oxr | `extension_support/oxr_extension_support.py` | one list entry (pattern `:120`) | 1 |
| oxr | `oxr_api_funcs.h`, `oxr_api_negotiate.c:186-…` | 8 decls; 8× `ENTRY_IF_EXT(…, EXT_spatial_container)` (`:167-168`, `:244`) | 70 |
| oxr | `oxr_api_session.c` | container-mode branches at begin (`:91-113`), end (`:116-127`), exit (`:213-224`), `xrLocateViews` (`:227-277`), `xrEndFrame` `layerCount != 0` (`:173-210`) | 40 |
| oxr | new `oxr_api_spatial_container.c` | 8 entry points with `OXR_VERIFY_*` (pattern `oxr_api_session.c:539-596`) | 300 |
| oxr | new `oxr_spatial_container.c` | handle create/destroy, request forwarding, latch, event translation, container-space wrapping | 350 |
| oxr | `oxr_objects.h` | `struct oxr_spatial_container`; `oxr_session::{spatial_containers_enabled, containers…}`; `OXR_SPACE_TYPE_SPATIAL_CONTAINER` + `oxr_space::spatial_container`; 6 push decls (pattern `:741-746`); debug tag | 90 |
| oxr | `oxr_session.c` | chain read (`:1577-1583`), IDLE-only (`:1605-1606`), internal begin (§1.3), `oxr_session_poll` cases (`:697-762`), `REQUEST_EXIT` branch (`:722`), `Closed` before `LOSS_PENDING` (`:716-720`), begin/end/exit early errors (`:414`, `:519`, `:595`), destroy (`:1219-1239`) | 150 |
| oxr | `oxr_session_frame_end.c` | `layerCount != 0` error; skip `do_synchronize_state_change` (`:1627-1633`, `:1808`, `:1851`, `:1920`) | 15 |
| oxr | `oxr_event.c` | 6 push functions (pattern `:265-286`); 6 cases in `is_session_link_to_event` (`:120-158`) | 140 |
| oxr | `oxr_space.c`, `oxr_pretty_print.c`, `oxr_conversions.h` | new type in `get_xrt_space` (`:100-130`), create (copy `:254-284`), printing | 50 |
| oxr | `oxr_system.c:650-…` | `XrSystemSpatialContainerPropertiesEXT` via `OXR_GET_OUTPUT_FROM_CHAIN` (pattern `:658-668`) | 15 |
| oxr | `oxr_verify.c:548-652` | accept the chained struct without a graphics binding (beside `:643-647`); reject with `MND_headless` | 15 |
| oxr | `oxr/CMakeLists.txt` | 2 sources | 4 |
| xrt | `xrt_session.h` | 6 event types + 6 payload structs + union members (`:38-72`, `:203-215`) | 60 |
| xrt | `xrt_compositor.h:977-983` | `xrt_session_info::spatial_containers_enabled` | 5 |
| xrt | new `xrt_spatial_container.h` | `struct xrt_spatial_container_control` vtable (create/destroy/request_visible/request_bounds_mode/get_state/create_space, get_properties); `struct xrt_spatial_container_state {visible, interactable, bounds_mode, bounds}` | 90 |
| ipc | `proto/50-spatial-container.json` (+`.license`), `ipc_protocol.h` | 7 calls; aggregate structs (`common.py:115` rule) | 80 |
| ipc | `ipc_server.h:99-218` | `containers[IPC_MAX_CLIENT_CONTAINERS]` + count in `ipc_client_state`; `#define` beside `:68` | 20 |
| ipc | new `ipc_server_handler_spatial_container.c` | 7 handlers (pattern `ipc_server_handler.c:650-681`); policy hook; event push via the session sink (§6.3 accessor in `b_session.h`) | 220 |
| ipc | `ipc_server_process.c` | no-controller default (§7.1) beside `handle_focused_client_events` `:546-577`; `Closed` on client disconnect (`ipc_server_per_client_thread.c:207` area) | 80 |
| ipc | `ipc_server_handler.c:424-486` | honour `xsi.spatial_containers_enabled` | 10 |
| ipc | new `ipc_client_spatial_container.c`, `ipc_client.h`, `ipc_client_instance.c:222-230` | proxy vtable (pattern `ipc_client_space_overseer.c:96-158`) | 160 |
| ipc | `ipc/CMakeLists.txt`, server/client CMake | sources | 6 |
| base | `b_space_overseer.{h,c}` | container-space create + pose setter + locatable toggle (option (a) or (b), §7.5) | 80 |
| base | new `b_spatial_container.c` (in-process direct impl) | same vtable, grant-all policy (`oxr_session.c:1327-1330`'s posture) | 150 |
| doc | `doc/changes/…` | fragment | 3 |

**Overlap with the landed C0 series** (`git diff --stat ccae7f3c1..dd8ec00cc`, 36 files): C1
edits `ipc_server.h` (C0 +121), `ipc_server_handler.c` (+105), `ipc_server_process.c` (+74),
`ipc_server_per_client_thread.c` (+16), `ipc_protocol.h` (+78), `ipc/CMakeLists.txt`,
`xrt_config_build.h.cmake_in`, `CMakeLists.txt`, `scripts/mapping.imp`. The earlier claim
"C1 touches `oxr`, C0 touches IPC — no overlap" was wrong; C1 must be rebased on C0's tip, and
the no-controller default (§7.1) is written *against* C0's lease (`ipc_server_lease.h`), not
beside it.

## 9. Sources

Pinned (`references/MANIFEST.json`): `monado` b9883f235; `monado-galaxyxr` 6ea94427f;
`godot` 941ea1816; `openxr-docs` 5a82d45bc; `openxr-sdk` f2448a879 (`release-1.1.63`);
`openxr-cts` c4d9194d6 (`openxr-cts-1.1.63.0`); `openxrs` eba4c6a75; `wayland-protocols`
819004adb; `wlroots` 297e01d2d; `smithay` 79bbed5e1; `weston` 3c6ce8da6; `openvr` 092406431;
`pipewire` 2943841e3; `aosp-frameworks-native` 4f463a6b1; `xrizer` 0989a7fac; `opencomposite`
cff07db75; `stardustxr-server` cf20614f3; `wayvr` 0eab39070; `wlx-overlay-s` 54713b95f;
`xrdesktop` dbf3dbaa0; `simula` a08ca31fa; `motorcar` e1cb943d2. The fork:
`/run/media/j/tinystore/experiments/monado` branch `mura` at `dd8ec00cc` (= upstream
`ccae7f3c1` + C0's four commits).

[external], read 2026-09-30: gitlab.freedesktop.org/monado/monado merge requests !1354, !1808,
!1934, !2062, !2194, !2284, !2343, !2344, !2353, !2453, !2530, !2546, !2573, !2578, !2598,
!2603, !2606, !2647, !2814, !2830, !2860, !2879, !398, !2341; issues #345, #492, #591; branch
`deferred-session-state` (compare API); `wallbraker/monado-collabora` branch
`jakob/comp/multi-interface`; `rpavlik/monado` branch `bubble-comp-events`;
github.com/Ralith/openxrs master and PR #214; github.com/KhronosGroup/OpenXR-SDK releases
1.1.59–1.1.63; github.com/KhronosGroup/OpenXR-CTS PR #110; immersive-web.github.io/webxr
(§4.1, §12.6, §13.5.2, §13.6). GitLab discussion notes were read through
`…/merge_requests/<N>/discussions.json` (the HTML is 401 unauthenticated).

Reading groups (this session's subagents, read-only): Monado internals; upstream direction;
clients and tooling; comparables outside Monado. Every `file:line` in this document was
re-read against the pins during synthesis; corrections made in the process are noted inline
(§0 fork identity, §3.1 Godot counts and paths, §5 Q8 the coalescing MUST).
