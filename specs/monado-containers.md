# specs/monado-containers: the runtime-internal design of `XR_EXT_spatial_container` on Mura's Monado (C1)

**Status:** rev 0 (2026-09-30). Normative for the `spatial-container` series on the
`mura-os/monado` fork (C1 in the C-track,
[implementation-path.md §3](../docs/architecture/implementation-path.md)) and for its
prerequisite, the `openxr-headers-1.1.63` series. **App-facing behaviour is not restated here**:
[composition.md §3](composition.md) is the contract Monado must meet (every rule there is the
OpenXR text's, line-cited); this spec says *where inside Monado* each of those rules is
honoured, by which process, under which lock, through which existing seam — so the series can be
written without re-deriving shape. C2 (self rendering: `comp_multi` slots, squasher,
`xrLocateSpatialContainerViewsEXT`) and C3 (the seam's verbs) get their own revisions; §9 names
the one hook C1 leaves for them.
**Design sources:** [research/81](../docs/research/81-spatial-container-runtime-design-from-comparables.md)
(the comparables record — Monado's own seams, upstream's stated direction, Godot's module, the
Wayland/Android/OpenVR/PipeWire comparables; its §6 determinations and §7 owner items are this
spec's rulings and open items), [research/79 §4b](../docs/research/79-openxr-extensions-and-zxr.md)
(the first sizing), [ADR 0006 amendment 4](../docs/architecture/adr/0006-compositor-strategy.md)
(the ruling that Monado composes containers), composition.md §3–§5.
**Grounding:** "XDG" does not occur; every space is an OpenXR `XrSpace` or an `xrt_space`;
"layer" is OpenXR's composition layer; "the service" is `monado-service` (`ipc_server` +
`comp_multi`), "the state tracker" is `src/xrt/state_trackers/oxr` in the application's process,
"the in-process target" is `targets/openxr` with IPC off; the OpenXR text is the pinned
`references/openxr-docs` (1.1.63), Monado the pinned `references/monado` (`b9883f235`; the fork's
`mura` at `dd8ec00cc` = upstream `ccae7f3c1` + the landed C0 series, `oxr_*` lines identical,
`ipc_server_*`/`comp_multi_*` shifted — research/81 §0).
**Budget impact** (overview invariant 9): nothing per frame. Per container: one ≈64-byte record
in the service and one ≈96-byte handle in the state tracker; one `xrt_space` in the overseer
(≈120 B, as any offset space). Per event: one ≈48-byte `xrt_session_event` node in the
existing per-session queue and one `oxr_event` node in the instance queue — both paths that
exist today for every state change. Per `xrPollEvent`: the one IPC round-trip per session that
exists today (research/81 §1.1 step 5); no new polling loop, thread, timer or socket. Per
`xrCreateSpatialContainerEXT`: one synchronous IPC round-trip (the spec's own resolution,
`ext_spatial_container.adoc:1300-1309`). The frame loop of a container session costs what an
IDLE session's would cost today plus one paced `xrWaitFrame` (§3.4) — no render, no layers, no
swapchain.

## 1. Scope and the two build targets

1.1 **What C1 delivers.** `XR_EXT_spatial_container` (811) advertised and conformant to
composition §3.1–§3.5 and §3.9 for a session that never renders: a container session at IDLE
with a paced frame loop and zero layers, N container handles per session with
`{visible, interactable, boundsMode, bounds}` state, six events, a container space, capability
reporting, and the default policy with no controller present (§7). **Not** delivered: any
rendering, `XR_EXT_spatial_container_self_rendering` (814), a `comp_multi` slot per container,
seam verbs, depth (§12).

1.2 **Two targets, one design.** Monado builds the state tracker against two `xrt` providers:
the IPC client (`ipc_client_instance.c:143-147`, `:222-230` — every `xrt_*` object is a proxy
over `ipc_call_*`) and the in-process `target_instance.c:69-161` (the same objects implemented
directly: `b_system`, `b_space_overseer_create` via `targets/helpers/target_builder_helpers.c:73`, `comp_multi`
wrapping the native compositor at `comp_compositor.c:1316`). The design's stated intent for that
boundary is interchangeability — the space graph "isn't exposed there is no need to synchronise
it across the app process and the service process" (`xrt_space.h:86-91`). **Rule:** every
container operation the state tracker needs is a function on one `xrt` vtable
(`struct xrt_spatial_container_control`, §2.2), implemented once as an IPC proxy
(`ipc_client_spatial_container.c`) and once directly (`b_spatial_container.c`). The state
tracker contains no `#ifdef` on the target and no policy.

1.3 **The one asymmetry, made explicit.** Today `xrt_multi_compositor_control` is implemented
only by `comp_multi` (`comp_multi_system.c:843-850`); the IPC client never sets `xmcc`
(`ipc_client_compositor.c:1018-1036`), so `oxr_session.c:1327-1330`'s "grant visible + focused"
runs only in-process. The in-process container implementation follows that posture: it is the
policy (grant every request, one interactable container = the last shown), because there is no
service to be. This is not a second policy; it is the absence of a controller (§7) with no
`ipc_server_process.c` to host the default.

## 2. Authoritative state and its owner

2.1 **The record lives in the service.** Precedent: every shell-decided fact today —
`primary_application`, `session_visible/focused/overlay`, `io_blocks`, `z_order` — is stored in
`ipc_client_state.client_state` (`struct ipc_app_state`, `ipc_protocol.h:387-401`) under
`s->global_state.lock` (`ipc_server.h:449-458`), computed by `handle_focused_client_events`
(`ipc_server_process.c:546-577`) and `update_server_state_locked` (`:598-656`), *then* pushed to
`comp_multi` (`:573-576`), whose `mc->state` (`comp_multi_private.h:124-133`) is a render-side
mirror written without a lock (`comp_multi_system.c:648` `//! @todo Locking?`). The container
record follows the *owner*, not the mirror. **Rule:** per client, `ipc_client_state` gains
`struct ipc_spatial_container containers[IPC_MAX_CLIENT_CONTAINERS]` (a fixed per-client array
like `xspcs[IPC_MAX_CLIENT_SPACES]`, `ipc_server.h:192`, `:68`) holding, per slot:
`{in_use, closed, requested_visible_once, requested = {visible, bounds_mode}, state = {visible,
interactable, bounds_mode, bounds}, suggested_bounds, graphics_presentation, pose, xspc}`.
`requested` is the app's last ask, recorded and never applied by the record itself — wlroots'
`requested` beside `current` ("Intended to be checked by the compositor on surface map and
state change requests … and handled accordingly", `wlr_xdg_shell.h:203-206`); the spec's
"Runtimes may: defer any request … do not queue events when a request is deferred"
(`ext_spatial_container.adoc:366-371`) is that field awaiting a decision by the controller or
§7. Every read and write of the array happens under `global_state.lock`, the lock
`update_server_state_locked` already holds; the C3 seam and §7's default mutate the same
record through the same lock.

2.2 **The vtable.** New `xrt/xrt_spatial_container.h`:

```c
struct xrt_spatial_container_state { bool visible; bool interactable; enum xrt_spatial_container_bounds_mode bounds_mode; struct xrt_vec3 bounds; };
struct xrt_spatial_container_control {
    xrt_result_t (*get_properties)(…, uint32_t *max_count, bool *bounded, bool *immersive);
    xrt_result_t (*create)(…, enum presentation, const struct xrt_vec3 *suggested, uint32_t *out_id);
    xrt_result_t (*destroy)(…, uint32_t id);
    xrt_result_t (*request_visible)(…, uint32_t id, bool visible);
    xrt_result_t (*request_bounds_mode)(…, uint32_t id, enum bounds_mode);
    xrt_result_t (*get_state)(…, uint32_t id, struct xrt_spatial_container_state *out);
    xrt_result_t (*create_space)(…, uint32_t id, struct xrt_space **out_xspc);
};
```

Containers cross IPC as `uint32_t` per-client indices, the pattern of `xdev_id`, `xtrack_id`
and `space_id` (`ipc_server_objects.c:31-205`; client wrapper `ipc_client_space_overseer.c:23-30`).
Seven proto calls in `50-spatial-container.json` (generated by `proto.py`,
`ipc/CMakeLists.txt:22-38`; handler pattern `ipc_server_handler.c:650-681`). No call is per
frame. The vtable hangs off `xrt_session` (the container is a child of the session,
`xr.xml:6966`), reached from `oxr_session::xs`.

2.3 **The state tracker is a latch, not an owner.** `struct oxr_spatial_container` (child handle
of `oxr_session`, `oxr_objects.h` beside `:1325-1422`) holds `{id, closed, state}` where `state`
is written **only** from events drained in `oxr_session_poll` (§6.3) and from the synchronous
`create` return — the `compositor_visible/focused` pattern (`oxr_session.c:702-710`).
`xrGetSpatialContainerStateEXT`/`xrGetSpatialContainerBoundsEXT` answer from the latch, so they
cost no IPC and are coherent with the events the app has already seen (Godot reads state
immediately after `IDLE`, `main.gd:213-221`; composition §7.3). **Invariant:** a state the app
can observe through `xrGet*` was delivered, or is queued, as an event — the latch is updated in
the same drain step that queues the event, which is also what `:770-772` requires ("continue
to report the old bounds … until the runtime queues a resize event").

2.4 **Invariants the record enforces (all under `global_state.lock`).**
- `interactable ⇒ visible` (`ext_spatial_container.adoc:589-591`).
- `visible ⇒ requested_visible_once` — the runtime never shows a container before the app's
  first request (`:482-484`).
- `closed ⇒ ¬visible`, and the transition to `closed` queues `VisibleChanged(false)` first if
  it was visible (`:227-229`).
- At most one container per client is `interactable` in C1 (the shipping default in §7; the
  spec allows many, `:618-624`); a hand-off queues the loser's event before the winner's, in
  one critical section.
- A slot past `IPC_MAX_CLIENT_CONTAINERS` returns `XRT_ERROR_ALLOCATION` → `XR_ERROR_LIMIT_REACHED`
  (`:155-157`), matching `maxSpatialContainerCount` (§8).

## 3. The session in container mode

3.1 **Opt-in.** `oxr_session_create` reads the chain (beside the overlay read,
`oxr_session.c:1577-1583`) for `XR_TYPE_SESSION_CREATE_INFO_SPATIAL_CONTAINERS_EXT` — only when
`inst->extensions.EXT_spatial_container` (the guard pattern `oxr_system.c:658-660`) — and sets
`sess->spatial_containers_enabled` and `xsi.spatial_containers_enabled` (`xrt_compositor.h:977-983`;
the IPC server reads it at `ipc_server_handler.c:424-486` and marks the client). Without the
chain the session is ordinary even if the extension is enabled on the instance (Godot's C1-time
behaviour, research/81 §3.1); every container function on it returns
`XR_ERROR_SPATIAL_CONTAINERS_NOT_ENABLED_EXT` (`:158-160`).

3.2 **Verification.** `oxr_verify_XrSessionCreateInfo` (`oxr_verify.c:548-652`) accepts a chain
with no graphics binding when the container struct is present (beside the headless acceptance,
`:643-647`; `ext_spatial_container.adoc:1090-1093`) and rejects the container struct combined
with `XR_MND_headless` enabled on the instance with `XR_ERROR_VALIDATION_FAILURE` (`:1095-1099`).
A graphics binding *may* be present (C2 needs one; Godot chains its Vulkan binding).

3.3 **Create: a compositor, IDLE, and stop.** The session is created through
`oxr_session_create_impl`'s **compositor** branch, never the headless one (`oxr_session.c:1543-1561`
is not taken — `MND_headless` behaviour must not be enabled, `:1095-1099`): the service creates
a native compositor for it as it does for every session in the default build
(`ipc_server_handler.c:438-452`), so `mc` exists for C2 and `xrWaitFrame` can pace (§3.4). The
state push at `oxr_session.c:1605-1606` becomes: push IDLE; **if container mode, return**
(`:1020-1030` — READY, SYNCHRONIZED, VISIBLE, FOCUSED, STOPPING, EXITING are never queued).

3.4 **Running without `xrBeginSession`.** The spec: "treat the session as running when it is in
the state IDLE" (`:1031-1032`). Monado already separates *running* (calls made) from *state*
(events polled): `oxr_frame_sync` (`oxr_frame_sync.h:36-44`; `running` set by
`oxr_frame_sync_begin_session`, `oxr_frame_sync.c:83-95`; `OXR_VERIFY_SESSION_RUNNING` reads it,
`oxr_api_verify.h:349-354`) and `xrt_comp_begin_session` (which increments `comp_multi`'s
`active_count` and starts the timing feed, `comp_multi_compositor.c:473-487`,
`comp_multi_system.c:478-493`, `:549-554`; `wait_frame` itself needs no begin,
`comp_multi_compositor.c:558-591`). **Rule:** a container session calls both internally —
`xrt_comp_begin_session(sess->compositor, view_config)` with the system's primary view
configuration and `oxr_frame_sync_begin_session(&sess->frame_sync)` — and never calls their
end counterparts until destroy. *When* they are called (at create, or at the first container's
first `request_visible(true)`) is [research/81 §7.2](../docs/research/81-spatial-container-runtime-design-from-comparables.md)'s
owner item; rev 0 writes **at create** as the assumption under which the guards below are
simplest (`OXR_VERIFY_SESSION_RUNNING` is then always true for a container session) and marks
the alternative as a one-line move. `xrt_comp_begin_session` also sets
`sess->current_view_config_type` (`oxr_session.c:1284` is the MAX_ENUM default) so
`xrLocateViews`' pre-check (`oxr_api_session.c:247-252`) is never the error path — the container
error is (§3.5).

3.5 **Guards — the container branch at each site.** Each is an `if (sess->spatial_containers_enabled)`
before the existing check; the existing check is untouched for ordinary sessions.

| Call | Site | Container mode returns | Spec |
|---|---|---|---|
| `xrBeginSession` | `oxr_api_session.c:91-113` | `XR_ERROR_SPATIAL_CONTAINERS_ENABLED_EXT` | `:1036-1039`, `:1112-1114` |
| `xrEndSession` | `:116-127` | same | `:1036-1039` |
| `xrRequestExitSession` | `:213-224` | same | `:1041-1043` |
| `xrLocateViews` | `:227-277`, before `:247` | same | `:1070-1072` |
| `xrWaitFrame` | `oxr_session.c:1077-1163` | paces (§3.4); `shouldRender = false` — already true for IDLE via `should_render` (`:126-133`); `predictedDisplayTime`/`Period` real | `:1075-1076` |
| `xrBeginFrame` | `:1166-1216` | unchanged | — |
| `xrEndFrame` | `oxr_api_session.c:173-210`, `oxr_session_frame_end.c:1764-2001` | `layerCount != 0` → `XR_ERROR_VALIDATION_FAILURE`; `layerCount == 0` → `xrt_comp_discard_frame` (`:1840-1854`) with **`do_synchronize_state_change` skipped** (`:1627-1633`, called at `:1808`, `:1851`, `:1920`) | `:1079-1082`, `:1027-1029` |
| focus-keyed results (`xrSyncActions`, `xrGetActionState*`, `xrApplyHapticFeedback`) | `oxr_session_success_focused_result` (`oxr_objects.h:1444-1452`), used at `oxr_api_action.c:966`, `oxr_input.c:1965,2006`, `oxr_set_haptic.c:89-90,145` | `XR_SUCCESS` when `sess->spatial_containers_enabled && sess->any_interactable`, else `XR_SESSION_NOT_FOCUSED`; `any_interactable` is a latch updated from `InteractableChanged` events (§2.3) | `:1033-1035` |
| `xrCreateSession` chain | `oxr_verify.c:548-652` | §3.2 | `:1090-1099` |

`XR_ERROR_SESSION_RUNNING`/`NOT_RUNNING` are never returned by a container session
(`:1107-1108`): the internal begin makes `OXR_VERIFY_SESSION_RUNNING` true for its lifetime, and
the three begin/end/exit sites return the container error before reaching the running check.

3.6 **Events the session still receives.** `oxr_session_poll` (`oxr_session.c:644-805`) in
container mode: `STATE_CHANGE` (`:702-710`) still latches `compositor_visible/focused` (the
service pushes them for the client as a whole — §7 uses them) but the auto-transitions at
`:788-802` are skipped; `OVERLAY_CHANGE` (`:711-715`) is not applicable and is ignored;
`REQUEST_EXIT` (`:722`) does **not** call `oxr_session_request_exit` — the spec's exit path is
hide-then-destroy (`:1044-1046`), so the service expresses "please exit" as `Closed` for every
container (§6.4) and the session state stays IDLE; `LOSS_PENDING` (`:716-720`) is preceded by
one `Closed` per open container, queued by the **service** before it pushes the loss event
(`:1085-1089`) — the state tracker does not synthesise them, so ordering is the producer's
(§6.4). The six container event types are drained in the same loop (§6.3).

3.7 **Destroy.** `oxr_session_destroy` (`oxr_session.c:1219-1239`) destroys child container
handles first (the handle tree does this, `oxr_handle_base`), which destroys their spaces
(§5.4) and calls `xrt_comp_end_session` + `oxr_frame_sync_end_session` before the compositor is
destroyed. The service side marks every slot closed on client disconnect
(`ipc_server_per_client_thread.c:207` area) so a crashed client's containers vanish with its
`multi_compositor`.

## 4. Handles and lifetime

4.1 **`xrCreateSpatialContainerEXT`** (`oxr_api_spatial_container.c`, entry-point pattern
`oxr_api_session.c:539-596`): verify session + container mode (`XR_ERROR_SPATIAL_CONTAINERS_NOT_ENABLED_EXT`,
`:158-160`); `graphicsPresentation` must be a value the runtime enumerates (§8.3 — in C1 the
list is empty; the result for an unenumerated value is [research/81 §7.4]'s owner item; rev 0
assumes `XR_ERROR_VALIDATION_FAILURE`, the core rule for an enum value outside the enumerated
set); call `xsc->create` synchronously (`:1300-1309`: "Synchronously as part of
`xrCreateSpatialContainerEXT`… the actual creation of a container is generally expected to be
cheap"); on `XRT_ERROR_ALLOCATION` return `XR_ERROR_LIMIT_REACHED` (`:155-157`); allocate the
`oxr_spatial_container` handle with `state = {false, false, BOUNDED, bounds}` where `bounds` is
what the service returned (`:144-146`; the service may ignore `suggestedBounds`, `:181-187`).
`suggestedBounds == {0,0,0}` is "no preference" and is passed through unchanged.

4.2 **`xrDestroySpatialContainerEXT`.** Legal in every state including closed (`:222-224`).
Order: the state tracker destroys the container's `oxr_space` children (they become
unlocatable, §5.4), calls `xsc->destroy(id)` (the service marks the slot free, drops its
`xrt_space` reference, and queues **no** event — destroy is the app's act), then frees the
handle. Events already queued for the id are delivered and, where they carry the handle,
dropped by `is_session_link_to_event`'s container cases (`oxr_event.c:120-158`) on handle
destroy — the same rule that drops a destroyed session's events (`:385-425`).

4.3 **Closed.** The service sets `closed`, queues `VisibleChanged(false)` if visible, then
`Closed` (`:214-229`). After the state tracker drains `Closed` it sets the latch's `closed` and
every function on that handle except destroy returns `XR_ERROR_SPATIAL_CONTAINER_CLOSED_EXT`
(`:222-224`) **without IPC** — the check is on the latch. The service also refuses the same
calls with `XRT_ERROR_SPATIAL_CONTAINER_CLOSED` for the window between the service's write and
the app's drain.

4.4 **Result mapping** (`xrt_results.h` gains one range beside C0's `XRT_ERROR_IPC_NOT_CONTROLLER`):
`XRT_ERROR_SPATIAL_CONTAINER_CLOSED` → `XR_ERROR_SPATIAL_CONTAINER_CLOSED_EXT`;
`XRT_ERROR_ALLOCATION` on create → `XR_ERROR_LIMIT_REACHED`; `XRT_ERROR_IPC_FAILURE` →
`XR_ERROR_RUNTIME_FAILURE` (the existing mapping). `xrResultToString` gains the four new codes
through the header bump (`oxr_pretty_print.c`; Godot prints them, research/81 §3.1).

## 5. The container space

5.1 **A server-created `xrt_space`, wrapped, not a new reference space.** `xsc->create_space(id)`
returns an `xrt_space *` (over IPC, a `space_id` the client wraps exactly as
`ipc_client_space_overseer.c:134-158` does); `oxr_space` gains `OXR_SPACE_TYPE_SPATIAL_CONTAINER`
beside `OXR_SPACE_TYPE_XDEV_POSE` (`oxr_objects.h:1710-1734`, `:1727`) with the `xrt_space` in
its union (`oxr_space.c:100-130` `get_xrt_space`; create modelled on `:254-284`). `xrLocateSpace`
needs **no new IPC**: `space_locate_space` (`ipc_server_handler.c:684-710`) resolves any two
`space_id`s. Each `xrCreateSpatialContainerSpaceEXT` returns a new `XrSpace` handle over the
*same* `xrt_space` (a reference, `xrt_space_reference`), as reference spaces do.

5.2 **Pose ownership.** The pose is the runtime's: "may: set the pose … to any arbitrary pose
when that container becomes visible" and may change it at any time (`:278-304`); its origin is
the bounds centre (`:290-299` — "There is no location that does not move in most of the ways a
spatial container resize UX is expected"). In the overseer (`b_space_overseer.c`) the pose is
stored server-side and set only by the service (C3's seam verb; §7's default) through a new
exported setter modelled on `update_offset_write_locked` (`:265-277`, write-locked on
`bso->lock` `:112`). Which shape — (a) an `OFFSET` space parented to `LOCAL` plus a
`locatable` flag, or (b) a new `U_SPACE_TYPE_CONTAINER` (`:44-64`) with its own `locate` branch
— is [research/81 §7.5]'s owner item; rev 0 assumes **(b)** as the shape that does not
overload `OFFSET`'s asserted invariants, noting upstream's stated preference against special
types (!2284 [external]).

5.3 **Unlocatable while hidden.** "While a spatial container is not visible or destroyed, the
spatial container space must: be unlocatable" (`:309-313`; the why: "the app is expected to
stop any CPU / GPU load that is used for that spatial container", `:1281-1288`). The overseer
already has the outcome: `U_SPACE_TYPE_ZOMBIE` returns `XRT_SPACE_RELATION_ZERO`
(`b_space_overseer.c:391-393`, `:424-426`; `xrt_defines.h:721`), and `oxr_space_locate` maps
`relation_flags == 0` to `locationFlags = 0` with an identity pose (`oxr_space.c:520-538`).
**Rule:** the container space's `locate` branch returns the zero relation whenever the
record's `visible` is false; `visible` is the *service's* bit, read under the overseer's
read-lock from the same record, so a space is unlocatable in the same instant its
`VisibleChanged(false)` is queued.

5.4 **Lifetime.** The `xrt_space` is refcounted (`xrt_space.h`) and held by the service's slot
and by each app-side `oxr_space`. Destroying the container (§4.2) or `Closed` (§4.3) flips the
slot to closed → the space is permanently unlocatable (zero relation), whatever references
remain — the `ZOMBIE` posture, which is what `oxr_space_destroy` → `xrt_space_reference(…, NULL)`
already tolerates. No new `xrt_space_overseer` verb is added to the `xrt_` interface; the setter
and the create are `b_space_overseer_*` exports used by the service and the in-process direct
implementation only (upstream's boundary: nothing mutable in `xrt_`, !2573 NAK [external]).

## 6. Events

6.1 **Six new `xrt_session_event` types, one struct each.** Beside the eleven in
`xrt_session.h:38-72` and the union at `:203-215`, following the rule that each struct starts
with its `type` (`:198`) and the convention that `STATE_CHANGE` and `OVERLAY_CHANGE` are distinct
types even though both carry booleans:

| `xrt_session_event_type` | payload | → `XrEventData…` | bytes |
|---|---|---|---|
| `SPATIAL_CONTAINER_VISIBLE_CHANGED` | `id, visible` | `SpatialContainerVisibleChangedEXT` | 8 |
| `SPATIAL_CONTAINER_INTERACTABLE_CHANGED` | `id, interactable` | `…InteractableChangedEXT` | 8 |
| `SPATIAL_CONTAINER_BOUNDS_CHANGED` | `id, bounds_mode, bounds` | `…BoundsChangedEXT` | 24 |
| `SPATIAL_CONTAINER_CLOSED` | `id` | `…ClosedEXT` | 8 |
| `SPATIAL_CONTAINER_VISIBLE_REQUEST_DENIED` | `id, visible` | `…VisibleRequestDeniedEXT` | 8 |
| `SPATIAL_CONTAINER_BOUNDS_MODE_REQUEST_DENIED` | `id, bounds_mode` | `…BoundsModeRequestDeniedEXT` | 12 |

All fit inside today's ≈48-byte union (sized by `reference_space_change_pending`,
`xrt_session.h:146-153`); `sizeof(union xrt_session_event)` does not grow, so the IPC reply of
`session_poll_events` (`50-session.json:11-15`) is unchanged.

6.2 **Producer: the service, through the session's sink.** Precedent and upstream's sanctioned
route: `OVERLAY_CHANGE` (`comp_multi_system.c:697-711` → `multi_compositor_push_event`
`comp_multi_compositor.c:108-113` → `xrt_session_event_sink_push(mc->xses)`), !2062's purpose
("events could flow directly from a session object instead of the compositor"), Jakob's !1354
review (no side channel from the native compositor to the client) and the !2573 NAK on
listener registries in `xrt_` [external, research/81 §2.2]. The handlers in
`ipc_server_handler_spatial_container.c` and the policy in `ipc_server_process.c` push into
`ics->xs`'s `b_session` sink (`b_session.h:40-55`; reached through a `b_session_from_xrt()`
accessor added to `b_session.h` — one function, no `xrt_` change) **while holding
`global_state.lock`**, so the record write and the event push are one critical section.
Delivery is the existing path: `b_session_event_push` (`b_session.c:123-140`) →
`ipc_handle_session_poll_events` (`ipc_server_handler.c:489-497`) → `ipc_client_session.c:49-57`.

6.3 **Consumer: `oxr_session_poll`.** Six cases beside `:697-762`; each updates the container's
latch (§2.3) then calls `oxr_event_push_XrEventDataSpatialContainer*EXT` (six functions in
`oxr_event.c`, pattern `:265-286`, ≈20 lines each) which allocates one `oxr_event` node on the
instance queue (`:79-118`). Each event carries the `XrSpatialContainerEXT` handle, so
`is_session_link_to_event` (`:120-158`) gains six cases and destroyed-handle events are dropped
(§4.2). Unknown `id`s (a race with destroy) are dropped silently.

6.4 **Ordering rules (the producer's, in one critical section).**
- Interactable hand-off: loser's `InteractableChanged(false)` before winner's `(true)`
  (`:618-624`).
- Close: `VisibleChanged(false)` (if visible) before `Closed` (`:227-229`).
- Loss: every open container's close sequence before `STATE_CHANGE(LOSS_PENDING)`
  (`:1085-1089`); the service produces both, so no reordering is possible on the consumer side.
- Show: `BoundsChanged` (if bounds changed on placement) before `VisibleChanged(true)` before
  `InteractableChanged(true)` — the order Godot's C2 flow expects (research/81 §3.1).
- No-op requests queue nothing; every denied request queues its own denial (`:1218-1223`:
  "Noop requests do not queue a change event, this just generates noise… If multiple visible
  requests get denied, runtimes must: each generate a corresponding denied event").

6.5 **Coalescing: exactly one MUST, met in the instance queue.** "Runtimes must: coalesce
multiple unpolled bounds changed events … report only a single event containing the last
bounds change" (`:773-777`), and "continue to report the old bounds from
`xrGetSpatialContainerBoundsEXT` until the runtime queues a resize event" (`:770-772`).
"Unpolled" is measured at `xrPollEvent`, so the place is the instance queue in the app
process (`oxr_event.c:79-118`), which may hold events from several drains. **Rule:**
`oxr_spatial_container` holds one pointer to its unpolled `BoundsChanged` node; the drain
(§6.3) overwrites that node's payload in place when the pointer is set, else appends and sets
it; `oxr_poll_event`'s pop clears it (one type check on the popped node). The latch (§2.3) is
updated in the same drain step, so `xrGet*Bounds` and the queued event always agree
(`:770-772`). Every 2D comparable coalesces per surface in the same way (wlroots' one idle
source per surface, `wlr_xdg_surface.c:164-179`; weston `xdg-shell.c:1585-1620`; smithay
`has_pending_changes`, `mod.rs:1472-1478` — research/81 §4.1–§4.4). No other event type is
coalesced: `VisibleChanged`/`InteractableChanged` toggles are each a real change the app must
see (the spec suppresses only *non*-changes, `:375-377`), and `Closed`/`*RequestDenied` are
one per occurrence (`:1218-1223`).

6.6 **No queue bound — and why that is the precedent, not an omission.** Neither Monado
queue is bounded (`b_session.c:123-140`; `oxr_event.c:79-118`; `IPC_EVENT_QUEUE_SIZE 32` at
`ipc_protocol.h:43` is unused), upstream has no MR or issue for one (research/81 §2.2), and
no comparable bounds a per-client outstanding queue — wlroots, smithay and weston keep
unbounded configure lists; PipeWire grows its per-client buffer by `realloc` and disconnects
only on socket error (`connection.c:134-160`, `module-protocol-native.c:501-509`) — research/81
§5 Q8. The producers of container events are the C3 seam (WM cadence) and §7 (a handful of
transitions per client lifetime) — never the render thread (`multi_main_loop`,
`comp_multi_system.c:526-625`, pushes no session events); `BoundsChanged`, the only
frame-cadence candidate, is bounded to one node per container by 6.5. A numeric cap with
disconnect-on-overflow would be a rule-7 "no comparable" invention and is a non-goal (§12).

## 7. The no-controller default

7.1 **Where it runs.** Beside `handle_focused_client_events` in `ipc_server_process.c:546-577`,
under `global_state.lock`, on the same triggers (a client's `session_active` change, a
primary change, a request handler) — and only while C0's lease has **no holder**
(`ipc_server_lease.h`; composition §5.3.2 "No holder → no new code" is extended here by
exactly this section, because containers have no fall-through to inherit). With a holder, the
record is mutated only by C3's verbs and this section is inert. In the in-process target it is
the direct implementation's only policy (§1.3).

7.2 **The shape (research/81 §6.8 — determined).** Every shipping spatial shell in the corpus
places a new client at a fixed pose in front of the head and then lets the user or WM move it
(motorcar `windowmanager.cpp:157-162`, wayvr `windowing/window.rs:100-105`, simula
`SimulaViewSprite.hs:1214`, xrdesktop `examples/shell.c:483-489`); smithay's default is to
answer a request, not withhold the capability (`shell/xdg/mod.rs:1129-1140`); Monado's own
no-shell rule is "usable with no shell" (composition §5.3.4). Rules:

- **Visible.** The first `request_visible(true)` is granted: `BoundsChanged` (if the placed
  bounds differ from create), then `VisibleChanged(true)` (`ext_spatial_container.adoc:481-483`
  is honoured because nothing is shown before it). Later requests, either way, are granted.
  Nothing is denied while no controller holds the lease, so `VisibleRequestDenied` is never
  queued by this section.
- **Interactable.** Monado's one-focus rule at container granularity: the **primary client's
  most recently shown visible container** is interactable, and only it. Primary is
  `update_server_state_locked`'s choice (`ipc_server_process.c:598-656`). A change of primary,
  a show, a hide or a close re-evaluates it and queues the loser's `InteractableChanged(false)`
  before the winner's `(true)` (§6.4).
- **Bounds mode.** `IMMERSIVE` requests are granted (Mura's fullscreen-game model, composition
  §3.4); one immersive container at a time — a newer grant hides the older immersive container
  (`VisibleChanged(false)`, its app may re-request). `BOUNDED` requests are granted. Bounds for
  a bounded container are `suggestedBounds` if non-zero, else `DEFAULT_BOUNDS`.
- **Pose.** A bounded container is placed head-relative in `LOCAL` at create-of-visibility:
  `DEFAULT_DISTANCE` ahead along the head's forward projected to the horizontal plane, centre
  at eye height, facing the head; each further visible bounded container of the same client is
  rotated `DEFAULT_FAN_DEG` about the head's vertical axis (motorcar's rule). The pose is set
  once and left; there is no follow.
- **Close.** Only on the client's disconnect (§3.7) and on `LOSS_PENDING` (§3.6). No user
  close exists without a shell.

7.3 **The constants are the owner's** ([research/81 §7.1](../docs/research/81-spatial-container-runtime-design-from-comparables.md)):
`DEFAULT_DISTANCE` — motorcar 1.5 m, wayvr 1 m / 0.95 m, simula and the xrdesktop example
3 m; `DEFAULT_FAN_DEG` — motorcar 30°, wayvr an 8 cm/8 cm/6 cm offset instead of a fan;
`DEFAULT_BOUNDS` — motorcar 0.5 m cube. Rev 0 writes **1.5 m, 30°, 0.5 m** as motorcar's set
(the design's ancestor) and marks them; they are compile-time constants in
`ipc_server_process.c`, not settings — a Mura with zxr never reaches them, and a Mura without
zxr has no settings daemon to read.

7.4 **The alternative, recorded and not taken in rev 0** (research/81 §7.1 (ii)): weston's
shape — withhold `supportsBounded` while no controller exists and deny bounded requests
(`ext_spatial_container.adoc:490-494` permits it), so a shell-less 3D app is always immersive,
as today's focused session is. It removes every constant above from Monado but makes the
capability depend on the controller's presence, which OpenXR fixes per instance
(`:966-990`) — a connecting zxr could not flip it for a running app. The owner decides; the
code shape differs by one branch in this section and one bit in §8.1.

## 8. Capabilities

8.1 **`XrSystemSpatialContainerPropertiesEXT`** is filled in `oxr_system_fill_in`
(`oxr_system.c:650-…`, pattern `:658-668`) from `xsc->get_properties` (one IPC call at
`xrGetSystemProperties`, never per frame). `supportsBounded = true`, `supportsImmersive = true`
(composition §3.5: Mura reports both; at least one is required, `:958-960`; self rendering will
require immersive, `self_rendering.adoc:50-53`).

8.2 **`maxSpatialContainerCount = IPC_MAX_CLIENT_CONTAINERS`**, a fixed per-client array like
every other Monado bound (`IPC_MAX_CLIENT_SPACES 128`, `MULTI_MAX_CLIENTS 64`, `IPC_MAX_CLIENTS
32`, `XRT_MAX_LAYERS` 128/32) — the value is [research/81 §7.3]'s owner item; rev 0 assumes
**16** (research/81 §4/§5 Q5 records the comparables' positions: the spec's "limited by memory
allocation", `:944-945`, versus Monado's fixed arrays and Android's smaller layer bound). The
*displayed* count is separate and is the controller's (`:946-956`); with no controller §7's
default displays exactly one. A budgets.md line records the record size × count.

8.3 **`xrEnumerateSupportedSpatialContainerGraphicsPresentationsEXT`** returns
`*countOutput = 0` in C1: the base extension declares the enum empty (`xr.xml:9656-9657`) and
the only value belongs to 814 (`:24560`). The list is stable for the instance (`:966-990`). C2
adds `SELF_RENDERING` here and nowhere else.

## 9. The C2 hook — and nothing more

9.1 C1 leaves `comp_multi` untouched except that `multi_compositor` learns whether its session is
in container mode (`xrt_session_info::spatial_containers_enabled`, read at
`comp_multi_compositor.c:980-981` beside `is_overlay`). C2 will bind per-container render state
to the service's record (§2.1) by `id`: each `XrSpatialContainerLayerEXT` in
`XrSpatialContainerLayerFrameEndInfoEXT` becomes a per-container layer set inside the client's
existing `multi_layer_slot`s (`comp_multi_private.h:86-92`), sorted and delivered by the render
thread that already reads `mc->state` (`comp_multi_system.c:253-318`). The record's `pose`,
`bounds` and `visible` are what C2 reads for FOV fitting and clipping — which is why they are
service-owned in C1.

9.2 Not decided here (C2's revision): whether a container is a `multi_compositor` slot of its
own or a layer group within its session's slot; latch/retain semantics; decay; the recommended
extent. Composition §4 states the ordering policy they must implement.

## 10. Budget (overview invariant 9)

| Item | Cost | Path |
|---|---|---|
| Per container, service | one slot in `ipc_client_state.containers[]` (≈64 B) + one `xrt_space` (≈120 B, as any offset space, `b_space_overseer.c:70-102`) | static array; no allocation on create except the space |
| Per container, app | one `oxr_spatial_container` (≈96 B) + one `oxr_space` per `xrCreateSpatialContainerSpaceEXT` | `U_TYPED_CALLOC` at create |
| Per event | one `b_session_event` node (≈64 B) + one `oxr_event` node (≈48 B + payload); `BoundsChanged` at most one node per container at any time (§6.5) | existing queues, freed on pop |
| Per `xrPollEvent` | one `session_poll_events` IPC round-trip per session (4 B out, ≈52 B back) — **as today** | `oxr_event.c:427-455` → `oxr_session_poll` |
| Per frame | `xrWaitFrame` pacing (as any session) + `xrEndFrame` → `xrt_comp_discard_frame` — no layers, no swapchain, no render | §3.4–§3.5 |
| Per create/destroy/request | one synchronous IPC round-trip | §2.2 |
| New threads, timers, sockets, polling loops | **none** | — |

## 11. Conformance — the C1 gate

The gate is the **probe**, not the Godot sample: Godot's module degrades to an ordinary
immersive session when 814 is absent and exercises no C1 surface (research/81 §3.1; the
registry offers no valid `graphicsPresentation` without 814, §3.2). The probe is a Rust binary
in `pkgs/` on `openxrs` regenerated from the pinned 1.1.63 `xr.xml` (`generator/src/main.rs:24-42`),
shaped like the CTS's policy-dependent tests (`test_XR_EXT_user_presence.cpp`: skip on absent
capability, assert guarantees, never assert a policy *choice*). **One dependency stated up
front:** items 4–9 need a *creatable* container, and under rev 0's §4.1 assumption (an
unenumerated `graphicsPresentation` is `XR_ERROR_VALIDATION_FAILURE`) a runtime advertising
#811 alone has none — the registry gives the base extension no presentation value
(`xr.xml:9656-9657`). So either the owner takes research/81 §7.4 (b) (accept the #814 value as
an opaque tag in C1) or items 4–9 run when C2's advertisement lands and items 1–3 and 10–11 are
C1's gate on its own. Asserted, against Mura's Monado with the series applied:

1. **Advertised.** `xrEnumerateInstanceExtensionProperties` lists `XR_EXT_spatial_container`;
   `XrSystemSpatialContainerPropertiesEXT` reports `maxSpatialContainerCount ≥ 1`,
   `supportsBounded`, `supportsImmersive`;
   `xrEnumerateSupportedSpatialContainerGraphicsPresentationsEXT` returns 0 (§8.3).
2. **Not opted in.** An ordinary session with the extension enabled: `xrCreateSpatialContainerEXT`
   → `XR_ERROR_SPATIAL_CONTAINERS_NOT_ENABLED_EXT`; the session reaches FOCUSED as today (§3.1).
3. **Session shape** (composition §7.6, restated): a session created with the chained struct
   and **no** graphics binding receives exactly one `SessionStateChanged(IDLE)` and no other
   state event within 2 s of polling; `xrBeginSession`, `xrEndSession`, `xrRequestExitSession`
   → `XR_ERROR_SPATIAL_CONTAINERS_ENABLED_EXT`; `xrLocateViews` → the same; `xrWaitFrame`
   succeeds with `shouldRender == false` and a monotonically increasing `predictedDisplayTime`
   over 10 frames; `xrBeginFrame` + `xrEndFrame(layerCount = 0)` succeed; `xrEndFrame(layerCount
   = 1)` → `XR_ERROR_VALIDATION_FAILURE`; `xrSyncActions` → `XR_SESSION_NOT_FOCUSED` while no
   container is interactable.
4. **Handles.** `xrCreateSpatialContainerEXT` × `maxSpatialContainerCount` succeed and the next →
   `XR_ERROR_LIMIT_REACHED`; `xrGetSpatialContainerStateEXT` on a new container reads
   `{false, false, BOUNDED}`; `xrGetSpatialContainerBoundsEXT` returns finite bounds;
   destroy succeeds; a call on a destroyed handle → `XR_ERROR_HANDLE_INVALID`.
5. **Space.** `xrCreateSpatialContainerSpaceEXT` succeeds; `xrLocateSpace(container, LOCAL)`
   returns `locationFlags == 0` while not visible (§5.3).
6. **Requests and events (no controller — §7).** The probe asserts the spec's guarantees and
   the *shape* of §7, never a constant. `xrRequestSpatialContainerVisibleEXT(true)` → within
   1 s either `VisibleChanged(true)` (any `BoundsChanged` for that container precedes it) and
   then `xrLocateSpace` valid, **or** `VisibleRequestDenied` — one of the two, never neither;
   under rev 0's §7 the probe records which. `xrSyncActions` → `XR_SUCCESS` iff an
   `InteractableChanged(true)` has been seen and not revoked. `xrRequestSpatialContainerVisibleEXT(false)`
   on a visible container → `VisibleChanged(false)` and the space unlocatable again. A
   repeated identical request queues nothing within 500 ms (`:375-377`).
   `xrRequestSpatialContainerBoundsModeEXT(IMMERSIVE)` → `BoundsChanged(IMMERSIVE)` or
   `BoundsModeRequestDenied`; `xrGetSpatialContainerBoundsEXT` reports the old bounds until the
   event is queued (`:770-772`).
7. **Ordering without a controller.** Two containers shown in turn: the second's grant yields
   `InteractableChanged(false)` for the first **before** `InteractableChanged(true)` for the
   second (§6.4, `:618-624`). Two containers granted `IMMERSIVE` in turn: the first receives
   `VisibleChanged(false)` (§7.2). Coalescing: with events left unpolled across two
   bounds-mode grants for one container, exactly one `BoundsChanged` for it is polled, carrying
   the last mode (`:773-777`).
8. **Loss and close.** Service stop while two containers are open and visible: for each,
   `VisibleChanged(false)` then `Closed`, all before `SessionStateChanged(LOSS_PENDING)` in the
   poll order (`:227-229`, `:1085-1089`); afterwards every call on either but destroy →
   `XR_ERROR_SPATIAL_CONTAINER_CLOSED_EXT`. (A user close needs a shell — C3's probe.)
9. **In-process target.** Items 3–6 pass against `targets/openxr` built with IPC off, with
   item 6 resolving to the grant branch (§1.3).
10. **Godot sample** (`pkgs/spatial-container-sample`, its README §3): with `--verbose`,
    `OpenXR: Enabled extension XR_EXT_spatial_container` appears (`openxr_api.cpp:661-665`),
    then the run is byte-for-byte the C0 baseline — `SCS ext absent` (the singleton's
    `is_enabled()` requires #814), `SCS session begun/visible/focused`, the robot as an
    immersive session. It proves item 2 (no regression for enabled-but-not-opted-in) and
    nothing else until C2.
11. **Budget.** `sizeof(union xrt_session_event)` unchanged (a static assert in the series);
    no new thread or fd in `monado-service` (`ls /proc/<pid>/task`, `/proc/<pid>/fd` before and
    after a container session).

## 12. Non-goals (rev 0)

No rendering of any container; no `comp_multi` slot, layer or squasher change; no
`xrLocateSpatialContainerViewsEXT`, FOV fitting, decay, retain or recommended extent (C2); no
seam verb, no event to the controller, no `libmonado` API (C3 — the record in §2.1 is what those
verbs will mutate); no depth (composition §4.3); no coalescing beyond §6.5's one MUST and no
numeric bound on either event queue (§6.6); no change to `xrt_space_overseer`'s public
interface; no user-initiated close without a shell (§7.2); no upstreaming decision (the series
is upstream-shaped so that one is possible); no support for more than one interactable
container per client in the default policy.
