# 77 — Shell-layer mechanics from comparables: how a compositor serves layer-shell, and what that becomes on a head

**Date:** 2026-09-27. **What this is:** the server-side mechanics pass that precedes zxr's
shell-layer half — the research/68 shape applied to `wlr-layer-shell` + `zxr-layer-anchoring-v1`,
`ext-session-lock`, `security-context-v1` and the socketpair admission.
[research/75](75-shell-plane-from-comparables.md) and [shell-plane.md](../architecture/shell-plane.md)
§2 *name* the compositor's half (bands 2/4/5 with exclusive angular bands, the binding filter, the
restricted-mode admission, the still-pointer idle rule) but nothing yet says **how**: how the
shipping compositors arrange layer surfaces and in what order, who sends the initial configure and
when, what `exclusive`/`on_demand` do to keyboard focus and how that composes with a
focus-follows-the-commit design, what happens to layer surfaces and the cursor while the session is
locked, how a global is hidden from a client and at what moment the decision is taken, and how a
pre-connected fd becomes a client. This document reads those mechanisms in the pinned compositors
(wlroots and its users sway/phoc/river; smithay and its users niri/cosmic-comp; Hyprland; KWin) and
the clients that must map on zxr unmodified (squeekboard, mako, waybar/gtk-layer-shell,
layer-shell-qt, wvkbd, phosh), says why each does what it does, whether the reason transfers to a
head-mounted display with no output edge, and what adopting it trades off (AGENTS.md rule 7).
Four clones were pinned for this pass: `sway`, `phoc`, `gtk-layer-shell`, `layer-shell-qt`
(`references/clone.sh`, MANIFEST.json). Every cite is `<clone>/<path>:<line>` under `references/`
or a repo path; the two wayland-rs crates are cited from the build's vendored sources and marked
[crate]. Consumer XR platforms appear as mechanism evidence only (rule 2). **Budget impact**
(overview invariant 9): a research document; §7 costs the mechanics on zxr's tick.

*Numbering note:* the plan that produced this document named it research/76; 76 was taken by the
parallel WM workstream (`76-grab-mechanics-from-comparables.md`) before this one landed.

## 0. Summary

- **Arrangement is one algorithm everywhere, and it is wlroots'.** sway, phoc, river, Hyprland,
  KWin and smithay all place a layer surface by the same arithmetic — bounds = usable area (or
  the full output when the zone is −1), size 0 on an axis stretches between the two anchors,
  anchored edges pin, the rest centre, margins inset — and shrink the usable area by
  `zone + margin` on the one *exclusive edge* a surface's anchors imply (a single edge, or the
  odd edge of a three-edge bar; corners and full anchors reserve nothing). Two passes: every
  surface with a positive zone first, then the rest; the layer order inside a pass is
  overlay→top→bottom→background in wlroots' users and KWin, background→overlay in river,
  and *not by layer at all* in smithay (§2.1). Only the output rectangle is 2D; the algorithm
  itself is edge arithmetic and transfers to an angular rectangle unchanged (§3).
- **The initial configure is the handler's and it is sent on the first commit, after
  arranging.** wlroots hands the compositor an `initial_commit` flag; smithay refuses to
  configure an unconfigured surface from `arrange` and documents why (the protocol forbids a
  configure before the client's first commit, and the client's own size must be respected);
  niri arranges *then* configures; cosmic-comp keeps the surface pending until that commit.
  zxr's xdg path already has this shape (`state.rs` `commit`).
- **Keyboard interactivity converges more than the protocol requires.** Every compositor read
  gives an `exclusive` surface on top/overlay the keyboard above every window while it is
  mapped, topmost layer first; every one focuses a non-`none` surface on map when it is on
  top/overlay (sway, cosmic-comp, Hyprland, phoc, river for `on_demand`) and lets a click focus
  an `on_demand` one; bottom/background exclusive surfaces get focus only when nothing else can
  (niri's explicit rule). That is a **focus stack with a layer override on top** — the shape
  `input/focus.rs` already reserved as `layer_focus_override`, and `on_demand` is exactly a
  member in the existing stack under the existing new-window and commit rules (§4.2).
- **Locked means no layer surface is composed, with one exception the comparables make
  differently.** niri renders nothing but the lock surface; river disables the whole normal tree;
  sway leaves the tree but routes input to lock surfaces only; cosmic-comp and Hyprland let a
  layer surface opt in (`show_on_lock` / `above_lock`) — for an OSK on the lock screen, which is
  exactly what phosh does with squeekboard. Mura's rule falls out of ADR 0007 I1 + the channel:
  while gated, the composed set is the trusted members (greeter + OSK), nothing else (§4.3).
- **The cursor is above the lock everywhere** (sway's root order: overlay, popups, seat,
  session_lock — with the cursor drawn last by the output; niri draws the pointer before its
  early return; cosmic-comp through the normal pointer path). zxr's band-5 cursor quad is already
  submitted after every member quad; nothing changes.
- **The filter is decided per connection at `get_registry` time**, not per bind: wayland-server
  evaluates `can_view(client_data)` when it sends the global list and again on `bind` (a bind of
  a hidden global is a protocol error) [crate]. So the "restricted" bit must be on the
  `ClientData` **at `insert_client`** — which is what every security-context implementation
  does (niri `restricted: true`, cosmic-comp `security_context: Some`, KWin `sandboxed`,
  Hyprland's display filter); it cannot be derived later. The socketpair client is the same
  mechanism with the opposite bit (§5).
- **wlroots dedupes pointer motion at the seat; smithay does not.** `wlr_seat_pointer_send_motion`
  drops a `motion` whose `wl_fixed` coordinates equal the last sent ("Ensure we don't send
  duplicate motion events"); smithay's `PointerInternal::motion` has no early-out. research/75 D3
  is therefore zxr's to implement in `pointer.rs`, at the same place wlroots does it (§2.7).
- **Determinations (§9):** arrangement algorithm and pass order (wlroots'/sway's), initial
  configure on first commit after arrange (smithay's stated rule), exclusive edge rule
  (wlroots'), zone clamping (wlroots/smithay, not river's kill), the focus rules above,
  layer surfaces as members of the existing scene in bands 2/4/5 with the existing hit classes,
  the filter as `ClientData` at insert, the socketpair as `insert_client` on an inherited fd
  with `ClientData::disconnected` as the restart trigger, motion dedupe at the transport.
  **Owner items:** the head frame's default rectangle and canonical distance for unaware
  clients (Q1), the frame-extent source (Q2), whether a `bottom`/`background` surface may exist on
  a non-world frame (Q3). The `ext-session-lock` seam stays what ADR 0007 ruled (dev/desktop
  profile), and this pass adds the mechanics smithay gives it for free.

## 1. The protocols as written

**`zwlr_layer_shell_v1` v5** (`wlroots/protocol/wlr-layer-shell-unstable-v1.xml`; smithay serves
v5, `smithay/src/wayland/shell/wlr_layer/mod.rs:232-233`).

- Role and first commit: "Creating a layer surface from a wl_surface which has a buffer attached
  or committed is a client error, and any attempts … to attach or manipulate a buffer prior to
  the first layer_surface.configure call must also be treated as errors. After creating a
  layer_surface object and setting it up, the client must perform an initial commit without any
  buffer attached. The compositor will reply with a layer_surface.configure event"
  (`:45-54`). Output may be NULL: "Generally this will be the one that the user most recently
  interacted with" (`:56-58`). Four layers "ordered by z depth, bottom-most first … ordering
  within a single layer is undefined" (`:76-90`).
- State is double-buffered on `wl_surface.commit` (`:109-111`); a null buffer unmaps and
  "returns to the state it had right after layer_shell.get_layer_surface"; remap = bufferless
  commit → configure → buffer (`:113-119`).
- `set_size`: 0 on an axis = "the compositor will assign it … You must set your anchor to
  opposite edges in the dimensions you omit; not doing so is a protocol error" (`:122-137`).
- `set_exclusive_zone`: a positive zone is honoured only when anchored to one edge or an edge
  plus both perpendicular edges; otherwise "a positive value will be treated the same as zero";
  0 = "would like to be moved to avoid occluding surfaces with a positive exclusive zone"; −1 =
  "would not like to be moved … the compositor should extend it all the way to the edges it is
  anchored to"; panel 10 / notification 0 / wallpaper-or-lock −1 are the protocol's own examples
  (`:152-187`). Margins: "The exclusive zone includes the margin" (`:190-204`).
- `keyboard_interactivity`: `none` "the compositor should never assign it the keyboard focus"
  (default); `exclusive` — "For the top and overlay layers, the seat will always give exclusive
  keyboard focus to the top-most layer which has keyboard interactivity set to exclusive … For
  the bottom and background layers, the compositor is allowed to use normal focus semantics";
  `on_demand` (v4) — "focused and unfocused by the user in an implementation-defined manner …
  Typically, the compositor will want to use its normal mechanism … (e.g. click to focus)"
  (`:206-266`). Interactivity "is inherited by child surfaces set by the get_popup request"
  (`:275`).
- `configure(serial, width, height)`: "The size is a hint … If the width or height arguments are
  zero, it means the client should decide its own window dimension" (`:327-352`); `closed` when
  "the surface will no longer be shown. The output may have been destroyed" (`:355-363`).
- `set_layer` (v2, `:382-388`); `set_exclusive_edge` (v5): the edge "will be automatically
  deduced from anchor points when possible, but when the surface is anchored to a corner, it
  will be necessary to set it explicitly … The edge must be one the surface is anchored to,
  otherwise the invalid_exclusive_edge protocol error" (`:391-400`).

**`zxr_layer_anchoring_v1`** (`protocols/zxr-layer-anchoring-v1.xml`) fixes the translation:
"a layer surface's anchor edges and margins are interpreted on the angular rectangle of its
assigned frame, and exclusive zones reserve angular bands rather than pixels" (`:10-14`); the
anchor bitfield "selects edges of that rectangle exactly as it selects output edges" (`:19-24`);
margins convert at the presentation distance, `angle = 2·atan(px / (2·d·ppm))` (`:24-29`);
"Keyboard interactivity and layer ordering keep their layer-shell meaning unchanged" (`:29`).
Frames head/body/hand_left/hand_right/world/docked (`:59-75`) with fallbacks hand→body,
docked→head (`:140-150`); unaware clients: "the compositor presents the surface in the head
frame with a compositor-chosen angular size; layer-shell clients unaware of this protocol
therefore remain usable unmodified" (`:118-120`); `set_angular_size` (horizontal, (0, 180])
(`:165-176`); `set_exclusive_angle` "the spatial analog of zwlr_layer_surface_v1.set_exclusive_zone"
(`:178-191`); `frame_extent(horizontal, vertical)` "Sent after the frame first applies and
whenever the extents change" (`:193-202`).

**`wp_security_context_manager_v1`** (`wayland-protocols/staging/security-context/security-context-v1.xml`):
"Sandbox engines attach a security context to all connections coming from inside the sandbox.
The compositor can then restrict the features that the sandboxed connections can use" (`:32-34`);
"Compositors should forbid nesting multiple security contexts by not exposing
wp_security_context_manager_v1 global to clients with a security context attached, or by
sending the nested protocol error" (`:36-40`). The protocol says *restrict the features*; which
features is every compositor's own list (§5.2).

**`ext_session_lock_v1`** (`wayland-protocols/staging/ext-session-lock/ext-session-lock-v1.xml`;
mechanics §2.4). ADR 0007 keeps it as the dev/desktop profile's seam; Mura's built-in lock is
compositor state with one trusted client (ADR 0007 amendment 2026-09-27).

## 2. What the compositors do

### 2.1 Arrangement and exclusive zones

| | order | bounds when zone −1 | size 0 | exclusive edge | zone clamp | source |
|---|---|---|---|---|---|---|
| **wlroots scene helper** (`wlr_scene_layer_surface_v1_configure`) | caller's | full area | stretch `bounds − margins` | `get_exclusive_edge`: 1 edge or 3-edge bar, else none; `exclusive_edge` overrides | usable w/h clamped ≥ 0 | `wlroots/types/scene/layer_shell_v1.c:61-114`, `:23-51`; `wlroots/types/wlr_layer_shell_v1.c:657-684` |
| **sway** | two passes (zone > 0 first), each overlay→top→bottom→background; no in-tree comment on the order | wlroots' | wlroots' | wlroots' | wlroots' | `sway/sway/desktop/layer_shell.c:56-76, 79-93` |
| **phoc** | same two passes; comment "Arrange exclusive surfaces from top→bottom" | full area | own arithmetic | `apply_exclusive` only for the three-edge/one-edge patterns | — | `phoc/src/layer-shell.c:26-82, 147-199, 211-214, 296-309` |
| **river** | two passes, background→bottom→top→overlay | wlroots' | wlroots' | wlroots' | **destroys** a client whose zone leaves < half the output: "Clients can request bogus exclusive zones larger than the output dimensions and river must handle this gracefully" | `river/river/LayerShellOutput.zig:89-150` |
| **Hyprland** | reset reserved area; stable-sort each layer by rule `order`; exclusive pass then non-exclusive | full monitor | stretch minus margins | explicit edge, single anchor, or triplet | — | `hyprland/src/render/Renderer.cpp:2609-2664, 2676-2683, 2687-2733, 2744-2776` |
| **KWin** | per output overlay→top→bottom→background exclusive, then non-exclusive; within a list by KWin layer then horizontal span | full output geometry | — | `exclusiveEdge`; `hasStrut` iff zone > 0, strut folded into the work area | invalid geometry → **close** the window | `kwin/src/layershellv1integration.cpp:80-84, 134-156, 179-205`; `kwin/src/layershellv1window.cpp:138-185` |
| **smithay `LayerMap::arrange`** | exclusive surfaces (`Exclusive(_)`) first, then the rest — **no layer order** | full `output_rect` for `DontCare` | **half the source** when unanchored on that axis; stretch when anchored both sides | `effective_exclusive_edge`: explicit or 1/3-edge implied; `Exclusive` with no edge is ordered as `Neutral` | `Saturating` arithmetic, zone ≥ 0 | `smithay/src/desktop/wayland/layer.rs:250-437, 461-478, 739-752` |

**Reasons stated.** The two-pass order is the protocol's own model: surfaces with a positive
zone define the usable area, everything else is placed inside what remains. Within the exclusive
pass, overlay-first means a lock/overlay bar reserves before a panel does — phoc says so
("from top→bottom", `phoc/src/layer-shell.c:296`), sway inherits the loop it wrote first, river
argues nothing for its reverse order. smithay's departure (no layer order; half-size default) is
undocumented in-tree. **Transfer.** The arithmetic is edge arithmetic on a rectangle; the only
2D-specific input is the rectangle itself (§3). The half-size default is a smithay quirk no
client depends on (every client read sets a size or anchors both edges — §2.6); wlroots' stretch
is the protocol's text. **Trade-off of taking wlroots' order over smithay's:** zxr writes its
own ~60-line arrange instead of calling `LayerMap::arrange`, and gains a per-frame usable
rectangle (smithay's map is per `Output`, and zxr has one output and six frames — §3.2).

**When arrange runs.** sway: on a commit with `initial_commit || current.committed || mapped
changed` (`sway/sway/desktop/layer_shell.c:259-277`); river: `initial_commit` or any committed
flag (`river/river/LayerSurface.zig:158-165`); Hyprland: map/unmap/destroy and any mapped commit
with `committed != 0` (`hyprland/src/desktop/view/LayerSurface.cpp:152, 192, 296, 299-355`);
KWin: property signals → `scheduleRearrange` (0 ms timer) (`kwin/src/layershellv1window.cpp:68-77,
98-101`); smithay: `map_layer`/`unmap_layer` arrange automatically and "Force re-arranging … when
the output size changes" (`smithay/src/desktop/wayland/layer.rs:80-111, 245-248`); niri arranges
on every layer commit (`niri/src/handlers/layer_shell.rs:103-210`); cosmic-comp arranges on
later commits and recalculates tiling when the zone changed (`cosmic-comp/src/wayland/handlers/compositor.rs:361-376`).
**Converges:** arrange on the layer surface's commit (and map/unmap), never per frame.

### 2.2 The initial configure and the commit rules

- wlroots exposes `initial_commit = !initialized` on the role commit (`wlroots/types/wlr_layer_shell_v1.c:386-406`);
  a buffer before `configured` is `ALREADY_CONSTRUCTED` "layer_surface has never been configured"
  (`:352-356`); width 0 without left+right or height 0 without top+bottom is `INVALID_SIZE`
  (`:359-376`); exclusive edge ⊄ anchors is `INVALID_EXCLUSIVE_EDGE` (`:379-383`); a null-buffer
  commit resets the role (`layer_surface_reset`, `:33-46, 393-398`).
- smithay's pre-commit hook enforces the same four errors and "must ack the initial configure
  before attaching buffer" (`smithay/src/wayland/shell/wlr_layer/mod.rs:473-566`); the shell
  module **does not send the initial configure** — `GetLayerSurface` roles the surface, installs
  the hook and calls `new_layer_surface` ("You likely need to send a `LayerSurfaceConfigure`",
  `:256-286`; `handlers.rs:66-141`); `arrange` refuses to configure an unconfigured surface:
  "The spec mandates that the initial configure has to be send in response of the initial
  commit … That also guarantees that the client is able set a size before committing … we would
  send a wrong size to the client and also violate the spec" (`smithay/src/desktop/wayland/layer.rs:414-424`).
- niri: `new_layer_surface` maps into the `LayerMap` immediately (`niri/src/handlers/layer_shell.rs:20-45`);
  on the unmapped surface's commit it **arranges first**, then sends the initial configure — "to
  respect any size the client may have sent" (`:103-210`; `compositor.rs:401-403`).
- cosmic-comp: `new_layer_surface` pushes a `PendingLayer` (`cosmic-comp/src/wayland/handlers/layer_shell.rs:21-38`);
  the first commit maps it (`map_layer` arranges) and then `send_configure` (`compositor.rs:76-87,
  420-434`; `shell/mod.rs:3058-3084`).
- KWin: a commit with a buffer marks mapped; `moveResizeInternal` sends configure and queues the
  serial; ack drains it (`kwin/src/layershellv1window.cpp:237-259, 287-294, 307-311`).

**Transfer:** the xdg path in `state.rs` `commit` (initial configure on the first commit, map on
the first buffer) is the same shape; a layer surface adds "arrange, then configure with the
arranged size" between them.

### 2.3 Keyboard interactivity and focus

| | `exclusive` on top/overlay | `on_demand` | focus on map | bottom/background | while locked | source |
|---|---|---|---|---|---|---|
| **sway** | after every arrange, the topmost mapped exclusive surface (overlay then top, reverse list) is focused on every seat; `has_exclusive_layer` | a click on any non-`none` surface calls `seat_set_focus_layer` | map: if non-`none` and top/overlay, focus all seats unless a higher layer is focused | normal semantics | `seat_set_focus_layer` is a no-op while locked | `sway/sway/desktop/layer_shell.c:103-138, 280-300, 303-314`; `sway/sway/input/seat.c:1302-1329`; `seatop_default.c:242-245, 382-386, 556-560` |
| **phoc** | `update_focus`: overlay then top; prefers zone > 0; **any truthy interactivity** | same path | same | — | — | `phoc/src/layer-shell.c:333-389`; `seat.c:1381-1383` "layers above shell can't be unfocused" |
| **river** | `checkExclusiveFocus`: "Find the topmost layer surface … which requests exclusive keyboard interactivity" → exclusive focus on all seats | on map, `non_exclusive` focus if the seat is not exclusively focused; a click on an `on_demand` surface focuses it | as left | — | `normal_tree` disabled | `river/river/LayerShell.zig:162-192`; `LayerSurface.zig:104-110`; `Seat.zig:751-760` |
| **Hyprland** | map: exclusive → `m_exclusiveKeyboardLSes` + grab; `refocusLastWindow` refuses while one exists | map: any non-`none` focuses; pointer focus refocuses if `allowKeyboardRefocus` | as left | — | `above_lock` rule | `hyprland/src/desktop/view/LayerSurface.cpp:196-213, 382-415`; `InputManager.cpp:725-727, 1845-1873` |
| **KWin** | `acceptsFocus` = interactivity ≠ 0 (exclusive and on_demand alike); top/overlay `acceptsFocusChanged` → `activateWindow` | same bit | as left | — | — | `kwin/src/wayland/layershell_v1.cpp:244-246`; `kwin/src/layershellv1window.cpp:133-136, 232-235, 314-321` |
| **niri** | `update_keyboard_focus`: lock → grabs → `focus_on_layer(Overlay)` (exclusive, else the on-demand marker) → `Top` → on-demand Bottom/Background → layout → **exclusive Bottom/Background only when there are no layout windows** | a newly mapped `OnDemand` surface sets `layer_shell_on_demand_focus`; a click sets it; many actions clear it | as left | normal, with the rule at left | `LockScreen` first | `niri/src/niri.rs:1238-1368`; `handlers/layer_shell.rs:161-180`; `niri.rs:6705-6723` |
| **cosmic-comp** | "If an exclusive layer shell surface exists (on any output), only exclusive shell surfaces can have focus, on the highest layer with exclusive surfaces. Popups are judged by their root surface" | map: focus if top/overlay and ≠ `None` | as left | — | lock surfaces or `show_on_lock` roots only | `cosmic-comp/src/shell/focus/mod.rs:636-672, 730-781`; `shell/mod.rs:3058-3084` |

**Reasons stated.** The protocol's own text is the reason for `exclusive` (a lock screen or
password prompt "need to ensure they receive all keyboard events", `:241-243`); cosmic-comp adds
the popup corollary (an exclusive surface's menus must still work); niri's bottom/background
rule protects the layout from a desktop widget that asks for exclusive focus; the map-time focus
for non-`none` surfaces is every compositor's reading of "on_demand … normal mechanism" plus the
fact that a launcher wants to type immediately (niri's comment names lxqt-runner,
`niri/src/handlers/layer_shell.rs:161-180`). **Transfer:** all of it — none of these rules
mentions an output edge. §4.2 maps them onto `focus.rs`.

### 2.4 Session lock, layer surfaces and the cursor

- **wlroots** `wlr_session_lock_v1`: `lock` → `new_lock`; the compositor must `send_locked`;
  `get_lock_surface` per output (duplicate output or pre-attached buffer are errors); a lock
  surface's commit must carry a buffer, be configured, and match the acked size;
  `unlock_and_destroy` requires `locked_sent`; destroying the lock from the compositor sends
  `finished` (`wlroots/types/wlr_session_lock_v1.c:151-178, 218-305, 308-328, 357-360, 380-393`).
- **sway**: lock surfaces live in `output->layers.session_lock`, the topmost per-output tree;
  the root order bottom→top is `shell_background, shell_bottom, tiling, floating, shell_top,
  fullscreen, fullscreen_global, [unmanaged], shell_overlay, popup, seat, session_lock`
  (`sway/sway/tree/root.c:43-56`; `include/sway/tree/root.h:19-27`; `tree/output.c:111-117`;
  `lock.c:160`). Layer surfaces are **not hidden**; input is allowed only to lock surfaces
  (`seat_is_input_allowed`, `sway/sway/input/seat.c:1064-1068`), layer focus is refused while
  locked (`:1316-1318`). A lock client that dies while locked: "session lock abandoned", every
  lock output's background goes **opaque red**, the lock stays, a new locker may replace it
  (`sway/sway/lock.c:167-171, 245-258, 265-268`).
- **river**: `locked_tree` is enabled and `normal_tree` (every layer tree, popups, the rest of
  the desktop) is **disabled** on lock; unlock reverses (`river/river/Scene.zig:25-49, 64-84`;
  `LockManager.zig:89-90, 128, 157-159, 184-188`).
- **niri**: while `is_locked()` the render path draws the lock surface then the solid lock
  colour and **returns — no layer surface, overlay included**; the pointer is drawn **before**
  that return; `contents_under` considers only the lock surface (`niri/src/niri.rs:4366-4412,
  3452-3474, 6427-6431`). Lock client death → smithay `Defunct` ("compositor policy whether
  another client may take over", `smithay/src/wayland/session_lock/lock.rs:136-143`).
- **cosmic-comp**: `lock` is refused while an existing lock client is alive, else `locker.lock()`
  immediately (`cosmic-comp/src/wayland/handlers/session_lock.rs:19-43`); the render order while
  locked walks overlay/top/bottom/background **only for surfaces with `layer_show_on_lock`**
  (cosmic's private `session_lock_layer` protocol), then the lock stage
  (`cosmic-comp/src/shell/focus/order.rs:90-178`; `wayland/protocols/session_lock_layer.rs:89-98,
  114-133`).
- **Hyprland**: `above_lock` layer rule renders a layer surface in the lockscreen pass only;
  "lock missing" after `misc:lockdead_screen_delay`; a 5 s timeout sends `locked`
  (`hyprland/src/render/Renderer.cpp:946-948, 1112-1116, 1670-1704`;
  `managers/SessionLockManager.cpp:52-132, 244-250`).
- **phosh/phoc**: no `ext-session-lock` at all — the lock screen is a layer-shell `overlay`
  surface, all anchors, zone −1, interactivity exclusive (`phosh/src/lockscreen.c:1217-1227`;
  `phoc/src/desktop.c:660-662`), and squeekboard (a `top` layer surface) is on it.
- **smithay** gives the protocol side: `SessionLockManagerState::new(display, filter)` (v1),
  `SessionLockHandler::{lock, unlock, new_surface}`, `SessionLocker::lock` sends `locked`,
  dropping the locker sends `finished`, the manager sends the lock surface's initial configure
  itself (`smithay/src/wayland/session_lock/mod.rs:72-115, 176-248`; `lock.rs:53-115`).

**Reasons.** The protocol's compositor obligations (blank opaquely, never unlock on client death)
are what sway's red screen and niri's early return implement; the `show_on_lock`/`above_lock`
opt-ins exist because an OSK or an OSD *must* be usable on a lock screen and layer-shell has no
"trusted" bit — cosmic-comp invented a private protocol for it, Hyprland a config rule, phosh
avoided the question by making the lock itself a layer surface. **Transfer:** Mura has the
trusted bit the others lack — the socketpair channel (ADR 0007 amendment) — so the composed set
while gated is a *connection* property, not a per-surface opt-in (§4.3). The cursor rule
transfers as is.

### 2.5 Popups on layer surfaces

sway unconstrains a layer popup to the **full output box** in parent-local coordinates
(`sway/sway/desktop/layer_shell.c:327-356`); phoc to the **usable area** (`phoc/src/layer-surface.c:115-133,
312-317`); smithay tracks them through `PopupManager` and includes them in `bbox_with_popups`,
`surface_under`, `send_frame` (`smithay/src/desktop/wayland/layer.rs:599-640`;
`desktop/wayland/popup/manager.rs:168-180`); interactivity is inherited by the popup (protocol
`:275`; cosmic-comp judges a popup by its root, §2.3). zxr's `popups: PopupManager` and
`unconstrain_popup` exist for xdg toplevels (`state.rs:1202-1206`); a layer popup's unconstrain
box is the frame rectangle in the layer surface's pixel space (§3.4).

### 2.6 Output selection and what the clients assume

- Output when NULL: sway → the focused workspace's output, else the first output, else destroy
  (`sway/sway/desktop/layer_shell.c:423-443`); niri → `layout.active_output()` else `send_close`
  (`niri/src/handlers/layer_shell.rs:20-45`); cosmic-comp → `seat.active_output()`
  (`cosmic-comp/src/wayland/handlers/layer_shell.rs:21-38`). zxr has one output (`XR-1`,
  `state.rs:385-389`): no choice to make.
- **squeekboard** (`top`, anchors bottom|left|right, zone = its height, interactivity **false**,
  width 0 / height H — `squeekboard/src/panel.c:63-86`) computes H from the output's **mode
  pixels and physical millimetres**: pixel density = px_width / mm_width (Librem 5's 720/65 when
  the physical size is absent), ideal button 9.48 mm × 4 rows, arrangement `Wide` for a landscape
  or ≥ 115 mm screen, landscape `Wide` height = `min(max(px_h/3, recommended), px_h/2)`
  (`squeekboard/src/state.rs:357-440`; `outputs.rs:405-416`). On zxr's `XR-1` (1920×1080,
  600×340 mm): density 3.2 px/mm, ideal 122 px, recommended 122, **panel height 360 px**
  (1080/3), exclusive zone 360. It uses `wl_output.scale` (`state.rs:438`; `outputs.rs:236-248`).
- **mako**: default layer **`top`** (not overlay), anchor top|right, never sets a zone or
  interactivity (protocol defaults 0 / none); output chosen by `wl_output.name` (v4), no
  xdg-output needed; binds compositor, shm, layer_shell, seat, wl_output, xdg_activation and
  optional `wp_cursor_shape_manager_v1` (`mako/config.c:126-130, 666-674`; `wayland.c:33-36,
  217-221, 415-441, 571-580`).
- **waybar** (gtk-layer-shell): interactivity none; default bottom + exclusive via
  `gtk_layer_auto_exclusive_zone_enable`; **requires `zxdg_output_manager_v1`** or throws
  (`waybar/src/bar.cpp:32-57, 235-236, 392-395, 435-471`; `client.cpp:26-34, 278-280`). zxr
  serves xdg-output (`OutputManagerState::new_with_xdg_output`, `state.rs:322`).
- **gtk-layer-shell**: opposite anchors → `set_size` 0 on that axis; always acks; waits up to
  1 s for the initial configure after a bufferless commit; auto exclusive zone from the
  allocation height/width plus the non-anchored margins; popups via `xdg_surface.get_popup`
  then `zwlr_layer_surface_v1.get_popup`; never reads output geometry for sizing
  (`gtk-layer-shell/src/layer-surface.c:35-45, 104-120, 181-204, 232-244, 264-293`;
  `custom-shell-surface.c:75-95`).
- **layer-shell-qt**: defaults all anchors, zone 0, **`OnDemand`**, `top`, size (0,0); constrained
  axes send 0; acks and applies the first configure (`layer-shell-qt/src/interfaces/window.cpp:31-37`;
  `window.h:64-68`; `qwaylandlayersurface.cpp:104-128, 136-158`).
- **wvkbd**: `overlay`, bottom (+left/right), zone = height, interactivity false, height from
  the configure's available size, output scale honoured (`wvkbd/main.c:63-64, 195, 216-220,
  563-583, 615-627, 813-833, 920`).
- **phosh**: lockscreen `overlay` / all anchors / exclusive / −1; top panel 32 px top|left|right
  none; home bar `top` with a zone; `set_size` 0 is a valid stretch (`phosh/src/lockscreen.c:1220-1227`;
  `top-panel.c:1122-1137`; `home.c:862-877`; `layersurface.c:370-383, 676-705`).

**What this fixes for zxr:** the clients size themselves from `wl_output` **mode + physical size +
scale** and from the configure; none reads xdg-output geometry for sizing (waybar only needs the
global to exist). The one virtual output's mode and physical size are therefore the knob that
sizes every unaware client (Q1).

### 2.7 Pointer motion when nothing moved

- **wlroots** `wlr_seat_pointer_send_motion`: "Ensure we don't send duplicate motion events. …
  chop off some precision by converting to a wl_fixed_t" — the `motion` is sent only when the
  fixed-point coordinates differ from `pointer_state.sx/sy`; the warp is always applied
  (`wlroots/types/seat/wlr_seat_pointer.c:241-258`). sway's `cursor_rebase` after every layer
  commit calls `notify_enter` + `notify_motion` + `notify_frame` even when the cursor did not
  move, and relies on that dedupe (`sway/sway/input/cursor.c:140-142`; `seatop_default.c:1105-1117`;
  `desktop/layer_shell.c:276`). `wlr_seat_pointer_send_frame` is unconditional (`:395-410`).
- **smithay** `PointerHandle::motion` → `PointerInternal::motion` always updates the location
  and calls `focus.motion` (or enter/leave/replace on a focus change) — **no early-out when the
  location is unchanged**; `frame` is unconditional (`smithay/src/input/pointer/mod.rs:220-244,
  301-313, 792-825`; `grab.rs:212-221`).
- **zxr** `PointerTransport::deliver` calls `self.pointer.motion` for every `PtrOp::Move` the
  logic plans, and plans one per head-ray sample (`pkgs/zxr/src/input/pointer.rs:288, 310,
  458-475`) — research/75 D3's 62 motions/s.

**Transfer:** wlroots' rule at wlroots' place — the seat transport, comparing the `wl_fixed`
(1/256 px) rounding of the plane-local logical point against the last one sent to the same
surface, and dropping the `frame` with it. A `Leave`, an enter on a new surface, and a locked
pointer keep their paths.

## 3. Angular bands on frames: the derivation

### 3.1 The rectangle is the only 2D input

wlroots' algorithm (§2.1) consumes: a full rectangle, a usable rectangle, the surface's desired
size, anchors, margins, exclusive zone and edge. It produces a box and shrinks the usable
rectangle. Every quantity is a length along one of two axes; nothing is a pixel *per se*. On a
head-mounted display the anchoring protocol substitutes the frame's **angular rectangle**
(`frame_extent`, `protocols/zxr-layer-anchoring-v1.xml:193-202`) for the output and angles for
lengths (`:10-29`). The algorithm is then run per frame in **frame-pixel space**: a frame with
extent (H°, V°) presented at its canonical distance d has a pixel rectangle
`W = round(H° · ppd)`, `Hpx = round(V° · ppd)` where ppd is the frame's pixels-per-degree; the
layer surface's pixel size and margins are what the client sent; the arranged box (x, y, w, h)
is converted back to angle at the end: centre azimuth `((x + w/2) − W/2) / ppd`, elevation
`(Hpx/2 − (y + h/2)) / ppd`, and plane extents in metres from the angular size at d
(`2·d·tan(angle/2)`). This keeps every client's pixel assumptions intact (§2.6) and makes
`set_exclusive_angle` a zone in the same units: `zone_px = degrees · ppd`.

### 3.2 One usable rectangle per frame, not per output

Exclusive zones are meaningful only among surfaces that share a rectangle: a body-frame panel
must not shrink the head frame, and a head-frame OSD must not shrink the body frame. So the
usable rectangle is **per frame** — six at most (head, body, hand_left, hand_right, world,
docked). This is the one place smithay's `LayerMap` (per `Output`, one output) does not fit
and the reason zxr arranges itself (§2.1 trade-off). The **window tiers** (band 3) live in
their places' frames; the only frame where windows and layer surfaces meet is whichever the
placement engine spawns into — head-relative spawn into a free angular slot (the WM
workstream's `policy/free.rs`, window-workspace-management §3). The exclusive bands of the
**head frame** are therefore the "usable area" the spawn arithmetic honours; existing windows
are not moved when a band appears — the floating-window precedent: sway's `arrange_layers`
changes the workspace's usable area for tiling containers, and floating containers keep their
position (`sway/sway/desktop/layer_shell.c:79-93` computes `usable_area`; the tiling tree consumes
it in `arrange.c`). *Flagged (rule 4):* whether an existing window that now sits inside a new
exclusive band should be nudged is a WM-policy question for the WM workstream; this pass
exposes `Shell::usable(frame)` and stops.

### 3.3 What an unaware client gets

The protocol fixes the frame (`head`) and leaves the angular size "compositor-chosen"
(`:118-120`). Read from the clients (§2.6), the *only* thing an unaware client needs is a
consistent `wl_output` — mode, physical size, scale — and a configure whose size matches the
arranged box. The head frame's rectangle is thus the virtual output's mode (1920×1080 today,
`state.rs:387`) mapped onto the head frame's angular extent; ppd = 1920 / H°. Two comparables
give the angular numbers: WiVRn's lobby GUI is a panel at **0.5 m** in front of the head
(research/36 §2, `wivrn/client/constants.h:103`, `lobby.cpp:114-141`); WayVR's keyboard at
(0, −0.65, −0.5) m, pitched −10° (`wayvr/wayvr/src/overlays/keyboard/mod.rs:109-110`, research/36 §4.1); kwin-vr parents
OSDs to the camera with zero smoothing (research/36 §4). The head-frame rectangle's extent
itself has one honest source — the runtime's view FOV from `xrLocateViews` (`xr.rs:708, 767`
already carries `v.fov` per view; the union of both eyes' angles is the "screen edge") — and one
conservative alternative, a fixed comfortable rectangle inside it. Which, and at what canonical
distance, is **Q1** (§9): it is not a mechanic and the comparables give positions, not a
convergence.

### 3.4 Popups and hit testing in frame-pixel space

A layer popup's unconstrain box is the frame's pixel rectangle in the layer surface's local
coordinates (sway's full-output rule, §2.5, applied to the frame). Hit testing needs nothing new:
a layer surface is a plane member; `hit.rs` already classifies bands 4–5 as `Shell` with the
2 cm depth epsilon and band 2 as content (`pkgs/zxr/src/input/hit.rs:32-38, 42-46`), and the
member's `surface_under` walks the tree with popups (smithay `LayerSurface::surface_under`,
`smithay/src/desktop/wayland/layer.rs:614-640`).

## 4. Reconciliation with zxr's tree

### 4.1 The scene: a layer surface is a member

`scene.rs` already models bands 2–5 as `Place.band` (`pkgs/zxr/src/scene.rs:205-209`), flattens
them by band priority into quads (`:520-562`) and hit-tests every mapped plane (`:566+`). A layer
surface becomes a `Member` whose `Place` is on its frame at band 2 (`bottom`), 4 (`top`) or 5
(`overlay`) — `background` is band 1, the environment's, and is not a quad (spec §4) — with
`local` = the arranged pose and `Shape::Plane` = the arranged size in metres. The payload's
`window: Window` (`state.rs:262`) is xdg-only (`Window::new_wayland_window(ToplevelSurface)`);
the 19 uses of it are `toplevel()`, `geometry()`, `send_frame`, `surface_under`, `on_commit`
(`state.rs:295, 550, 554, 742, 762, 828, 855, 858, 1051`; `main.rs:299, 398, 737, 786, 795,
908-909, 1047, 1054`; `input/seat.rs:96`). The parallel WM branch adds three more of the same
methods. **Shape:** an enum over the two smithay desktop types behind the same field name and
the same five methods, so neither branch's call sites change.

### 4.2 Focus: the override slot exists, `on_demand` is the stack

`focus.rs` reserved `layer_focus_override` — "when `wlr-layer-shell` is served this returns the
exclusive (or on-demand-focused) layer surface and `focus_window` defers to it"
(`pkgs/zxr/src/input/focus.rs:37-41, 152-159`) — and `Zxr::focus_window` sets `Activated` on the
focused toplevel and the keyboard focus on its root (`state.rs:822-838`). Mapping §2.3 onto it:

- **`exclusive` on top/overlay** = the override: the topmost mapped exclusive member (band 5
  before 4, most recently mapped first — the protocol's "implementation-defined" tie) is the
  keyboard focus whatever the stack says, and no toplevel is `Activated` while it exists
  (cosmic-comp's rule). It is recomputed on layer map/unmap/commit, never per tick.
- **`on_demand`** = a member in the `FocusStack`: it takes focus on map by the new-window rule
  (`new_window_takes_focus`, mutter's intervening-event check — the same rule cosmic-comp and
  niri apply to a mapped on-demand surface, sharpened by the serial), and a `down`/`button`
  commit on it is `commit_focus`. Nothing new is invented: the stack's restore-on-close is
  focus return on unmap (sway `:303-314`).
- **`none`** = never in the stack, never the override; a commit on it changes no focus (mako,
  squeekboard, waybar, phosh's panel — every non-lock component read).
- **`exclusive` on bottom/background** = niri's rule: the override only when no window is
  mapped; otherwise stack semantics.

### 4.3 The mode gate: `exclusive` is the channel

`mode.rs` gates every non-keyboard sample while `Mode != Normal` and names the exception "members
of an `exclusive` layer-shell surface", with `ModeGate::exclusive` returning `false` until a layer
shell exists (`pkgs/zxr/src/input/mode.rs:16-26, 97-104`). The comparables (§2.4) have no trusted
bit and invent one per surface; Mura's is the **connection**: a sample whose hit member belongs to
a client admitted over the socketpair (`ClientState.trusted`, §5.3) passes the gate. The greeter
program's overlay/exclusive surface and the OSK's `top` surface are both trusted members; a
`top` surface from the public socket (waybar) is not composed and not hit while gated. This
satisfies I1 ("no input reaches any client" other than the lock scene) with the amendment's
reading of "the lock scene" as the trusted members, and makes the OSK-on-lock case (phosh,
cosmic-comp's `show_on_lock`, Hyprland's `above_lock`) a consequence rather than an opt-in.

### 4.4 Composition while gated

The flatten's `mapped(&M)` predicate (`scene.rs:525`) is where "composed" is decided; while
gated it is `mapped && trusted`. The frame is otherwise the opaque scene ADR 0007 requires; no
window quad, no untrusted layer quad. The cursor quad (band 5, `state.rs` cursor panel) is
submitted after every member quad as today and stays — every comparable draws the pointer over
the lock (§2.4).

### 4.5 Tiers and hit

No change: the tier picks the targeting source; the hit stage's class hook already prefers
bands 4–5 within 2 cm (`hit.rs`); a layer member is hit like any plane. `Class::from_band`
gives band 2 `Content`, which is right for a dock behind windows.

## 5. Filter and admission mechanics

### 5.1 When the filter is evaluated

wayland-server's `GlobalDispatch::can_view(client, global_data)` is called by the backend **when
the client's registry is created** (`send_all_globals_to`), when a new global is advertised
(`send_global_to_all`), and **on `bind`** — a bind of a global the client may not view returns
`None` and is a protocol error: "If this function returns false, the client will not be told the
global exists and attempts to bind the global will raise a protocol error"
[crate: `wayland-server-0.31.14/src/global.rs:104-121`; `wayland-backend-0.3.17/src/rs/server_impl/registry.rs:112-135,
190-232`]. Its input is the client's `Arc<dyn ClientData>` — the object passed to
`insert_client` — so the predicate must be decidable at insert (§5.3). smithay wraps this as a
boxed `Fn(&Client) -> bool` in each global's data: `WlrLayerShellState::new_with_filter`
(`smithay/src/wayland/shell/wlr_layer/mod.rs:209-243`; `handlers.rs:45-47`),
`SessionLockManagerState::new(display, filter)` (`session_lock/mod.rs:72-115`),
`SecurityContextState::new(display, filter)` — "must exclude clients created through a security
context" (`security_context/mod.rs:65-84`), `InputMethodManagerState::new::<D, F>`,
`VirtualKeyboardManagerState::new` (zxr already passes `|_| true`, `state.rs:339-340`).

### 5.2 Who restricts what

| | the bit | where set | what a restricted client cannot see | source |
|---|---|---|---|---|
| **niri** | `ClientState.restricted` | `SecurityContextHandler::context_created` inserts the accepted stream with `restricted: true` | layer shell, session lock, security context, IME, virtual keyboard/pointer, foreign toplevel, workspace, output management, screencopy, capture, … (`client_is_unrestricted`) | `niri/src/niri.rs:2388-2485, 7077-7085`; `handlers/mod.rs:499-512` |
| **cosmic-comp** | `ClientState.security_context: Option<SecurityContext>`; `not_sandboxed()` = none **or** engine `com.system76.CosmicPanel` | same handler; inherits the creator's DRM node | session lock, layer shell, data control, capture, IME, VK (`client_not_sandboxed`); the security-context global itself needs *no* context (`client_has_no_security_context`) | `cosmic-comp/src/state.rs:154-173, 643-738`; `wayland/handlers/security_context.rs:12-54` |
| **Hyprland** | `isClientSandboxed` (security context) | display global filter | everything **not** on an allow-list (seat, compositor, shm, xdg-shell, viewporter, fractional scale, cursor shape, text-input, activation, presentation, dmabuf, …) — layer shell, session lock, IME, VK, foreign toplevel, data control, workspace, capture are privileged by omission | `hyprland/src/Compositor.cpp:267-271, 288`; `protocols/SecurityContext.cpp:221-222`; `managers/ProtocolManager.cpp:169-232, 348-403` |
| **KWin** | `ClientConnection::sandboxed` — systemd Flatpak/Snap units **or** a security context | `FilteredDisplay::allowInterface` | `org_kde_plasma_window_management`, fake input, screencast, activation feedback, `kde_lockscreen_overlay_v1`, `wp_security_context_manager_v1`, `ext_data_control_manager_v1`; IM interfaces only for the IM connection; **`zwlr_layer_shell_v1` is not restricted** | `kwin/src/wayland_server.cpp:123-164`; `wayland/filtered_display.cpp:23-46`; `wayland/clientconnection.cpp:31-56, 98, 191-194` |

**Reasons.** The protocol's "restrict the features" is given a list by each compositor; three
of four put layer-shell on it (a sandboxed app must not draw a panel over the shell); KWin does
not, because Plasma's shell surfaces are LayerShellQt clients it trusts by other means (the
plasmashell process). Hyprland's deny-by-default is the safest reading and the most brittle
(every new protocol must be classified); niri's and cosmic-comp's explicit predicate per global
is the shape smithay hands out. **Transfer:** research/30's rule and shell-plane §2.2's list —
the niri/cosmic-comp shape, with the privileged set enumerated. cosmic-comp's engine allow-list
(`CosmicPanel`) is the *other* trusted-channel design — a named sandbox engine instead of a
socketpair; Mura's channel carries no name to spoof.

### 5.3 The `ClientData` and the socketpair

- `ClientData` is `initialized(client_id)`, `disconnected(client_id, reason)` and a `debug`
  hook, on an `Arc<dyn ClientData>` the compositor constructs [crate:
  `wayland-backend-0.3.17/src/server_api.rs:110-122`]; `DisplayHandle::insert_client(stream,
  Arc<dyn ClientData>)` makes the client [crate: `wayland-server-0.31.14/src/display.rs:100-111`].
  zxr's `ClientState { compositor_state }` with empty hooks is the current shape
  (`pkgs/zxr/src/state.rs:59-65`); the listening socket inserts every stream with
  `ClientState::default()` (`:394-403`).
- smithay's `SecurityContextListenerSource` is a calloop source yielding `UnixStream`s from the
  sandbox's listening socket and removing itself when the engine's `close_fd` closes
  (`smithay/src/wayland/security_context/listener_source.rs:16-65`); the handler receives it
  with the `SecurityContext { sandbox_engine, app_id, instance_id, creator_client_id }` on
  `commit` (`mod.rs:51-63, 209-220`) and inserts each stream with its own data — the restricted
  bit is set by construction, before any registry exists.
- The socketpair is the same call with a stream zxr already holds: `insert_client` on
  `UnixStream::from(fd)`, with `ClientState { trusted: true, .. }` — smithay's own Xwayland
  bring-up does exactly this (`smithay/src/xwayland/xserver.rs:218`), and the module doc for the
  socket source says the callback's only job is that call (`smithay/src/wayland/socket.rs:7-32`).
  The fd reaches the child as kscreenlocker's `WAYLAND_SOCKET` (ADR 0007 amendment;
  `kscreenlocker/ksldapp.cpp:377-423`, research/75 §4.2); libwayland-client reads
  `WAYLAND_SOCKET` before `WAYLAND_DISPLAY`, so the greeter and the OSK need no socket name.
- **Disconnection** is `ClientData::disconnected` — the one hook that fires for a trusted
  client's exit (crash or clean) and the trigger for ADR 0007's "blank, never unlock, the unit
  restarts it" (sway's abandon at `lock.c:245-258` is the same event with a red rectangle as the
  policy). The hook runs on the backend's thread of dispatch — the state loop — but without
  `&mut Zxr`; the pattern is a flag on the `Arc` read at the next tick (spec §2's one-loop rule).
- **Restricted-mode admission**: in `--greeter` there is no listening socket (session-auth §5);
  while locked the public socket stays bound and the mode gate does the routing (§4.3). Both
  are already the design; the mechanics add nothing beyond the trusted bit.

### 5.4 Which globals

shell-plane §2.2's set, checked against what smithay filters today: `zwlr_layer_shell_v1`
(`new_with_filter`), `zxr_layer_anchoring_v1` (zxr's own global, same predicate),
`ext_session_lock_manager_v1` (filter arg), `zwp_input_method_manager_v2` and
`zwp_virtual_keyboard_manager_v1` (already filterable in `state.rs:339-340`),
`wp_security_context_manager_v1` (filter arg; "no context" predicate). `ext_foreign_toplevel_list_v1`,
`ext_workspace`, `data-control`, the capture managers and `zxr_window_management` are not served
yet and inherit the predicate when they land.

## 6. Matrix

| Mechanic | wlroots/sway | phoc | river | Hyprland | KWin | smithay | niri | cosmic-comp | **zxr (this pass)** |
|---|---|---|---|---|---|---|---|---|---|
| arrange passes | excl→non-excl, overlay→bg | same, commented | excl→non-excl, bg→overlay | same as sway | same as sway | excl→non-excl, no layer order | smithay's | smithay's | **sway's**, per frame |
| size 0 | stretch | stretch | stretch | stretch | — | half | smithay's | smithay's | **stretch** |
| bogus zone | clamp ≥ 0 | — | kill client | — | close window | saturate | smithay's | smithay's | **clamp** |
| initial configure | on `initial_commit` | same | same | same | on rearrange | handler's, after first commit | arrange→configure | map→configure | **arrange→configure on first commit** |
| `exclusive` top/overlay | topmost, all seats | any truthy | topmost | list + refuse refocus | `acceptsFocus` | — | override first | only exclusive may focus | **override** |
| `on_demand` | click; map focuses | map | map + click | map + click | map | — | marker on map + click | map | **stack member** |
| excl. bottom/bg | normal | — | — | — | — | — | only if no windows | — | **niri's** |
| layers while locked | kept, input refused | n/a | tree disabled | `above_lock` | — | — | none rendered | `show_on_lock` | **trusted members only** |
| cursor while locked | above lock | — | — | — | — | — | drawn before return | normal path | **band 5, unchanged** |
| lock client dies | red, keep lock | n/a | — | lockdead | — | `Defunct` | — | refuse relock while alive | **blank, unit restarts** |
| filter timing | bind-time global filter | — | — | display filter | `allowInterface` | `can_view` at registry + bind | same | same | **same** |
| restricted bit | — | — | — | `isClientSandboxed` | `sandboxed` | `ClientData` at insert | `restricted` | `security_context` | **`ClientState.restricted` / `trusted`** |
| motion dedupe | seat, `wl_fixed` | wlroots' | wlroots' | — | — | none | none | none | **transport, `wl_fixed`** |

## 7. Embedded budget on zxr's tick

- **Arrange:** runs on a layer surface's commit/map/unmap only (§2.1 convergence); ~10 surfaces ×
  ~40 integer operations, one pass over the members of that frame; **zero per tick**. The
  produced pose/size are written into the member's `local`/`shape` once.
- **Composition:** one quad per mapped layer member, inside the runtime's layer cap as today
  (`Zxr::quad_budget`, `state.rs:543-546`; band 5 and 4 are allotted first, `scene.rs:520-524`);
  a layer surface's swapchain redraws only on its commits (spec §4 rev 3) — a still panel costs
  the runtime's sampling and nothing on the CPU. Measured in the gate as the tick delta with
  three layer members mapped vs none.
- **Focus:** the override is recomputed on layer events, a scan of ≤ N members; the stack is
  untouched.
- **Filter:** one boxed predicate call per global per registry and per bind; nothing after.
- **Idle:** the dedupe removes 62 `motion`+`frame` pairs per second per resting client
  (research/75 D3) for one `wl_fixed` compare per planned `Move`.
- **Memory:** a `Shell` struct with ≤ 6 frame rectangles and per-member anchoring state (a
  frame id, an `Option` pose, two `f32`s); no map, no per-output tree.

## 8. Verdicts against the tree

- `pkgs/zxr/src/input/mode.rs:24-26, 97-104` — the hook is right and its predicate is the
  channel (§4.3); the doc comment's "members of an `exclusive` layer-shell surface" becomes
  "members of a trusted connection".
- `pkgs/zxr/src/input/focus.rs:152-159` — `layer_focus_override` gets its body (§4.2); no other
  focus rule changes. `state.rs:822-838` `focus_window` must skip `Activated` while an override
  exists.
- `pkgs/zxr/src/input/pointer.rs:458-475` — the `wl_fixed` compare before `self.pointer.motion`
  (§2.7); `PtrOp::Frame` follows only a sent event.
- `pkgs/zxr/src/input/hit.rs:32-38` — already band-aware; no change.
- `pkgs/zxr/src/scene.rs` — bands and flatten already sufficient; the head frame exists
  (`Space::Views`, `:315`); body/hand/docked frames are not built (spec §5: "M1 ships one world
  frame and one head frame") — the anchoring server presents `frames` = head|world until they
  are, and every other request falls back per the protocol (§1).
- `pkgs/zxr/src/state.rs:59-65, 262, 339-340, 394-403` — `ClientState` grows two bits; the IM/VK
  filters get the predicate; `Payload.window` becomes the enum (§4.1); one `--shell-fd`/inherited
  fd path calls `insert_client` with `trusted`.
- `pkgs/zxr/default.nix` — the source fileset is rooted at `pkgs/`; `protocols/` is outside it,
  so the anchoring XML must join the fileset (root `../..`) for the build to generate it.
- `specs/zxr-core.md` §4, §8, §9, §10; `spatial-input.md` §6; `shell-plane.md` §2 — the normative
  homes, revised from §9 below.

## 9. Determinations and owner items

**Determined by converging comparables (acting without asking, rule 8):**

1. **Arrange = wlroots' arithmetic, sway's pass order, per frame in frame-pixel space** (§2.1,
   §3.1-3.2); size 0 stretches; bogus zones clamp (wlroots/smithay; not river's kill — a
   misbehaving panel is not a reason to destroy a shell component).
2. **Initial configure on the first commit, after arrange** (smithay's stated rule, niri's order).
3. **Focus:** exclusive-on-top/overlay override; `on_demand` = stack member under the new-window
   and commit rules; `none` never; bottom/background exclusive only when no window (§4.2).
4. **Gated composition and routing = trusted members only**, cursor unchanged (§4.3-4.4).
5. **Filter = `ClientData` bits set at `insert_client`;** privileged set per shell-plane §2.2;
   the security-context global only to clients without one; the socketpair is `insert_client`
   with `trusted`; `disconnected` is the restart trigger (§5).
6. **Motion dedupe at the transport, `wl_fixed` resolution** (§2.7).
7. **Layer popups unconstrain to the frame rectangle** (sway's full-output rule on the frame).
8. **A layer surface is a scene member**; the payload's window field becomes a two-variant enum
   behind the same methods (§4.1).

**Owner items (one decision each; options are the comparables' positions):**

- **Q1 — the head frame's default rectangle and distance for unaware clients.** *Why a
  decision:* the protocol leaves the angular size "compositor-chosen"; every unaware client sizes
  itself from the virtual output's mode/physical size (§2.6), so the mapping of 1920×1080 onto an
  angular rectangle at a distance fixes how large squeekboard's 360 px keyboard (§2.6) appears.
  *Options:* (a) **the runtime's view FOV** at the canonical distance — the honest "output edge"
  (the head frame is the display; `xr.rs` has the angles); (b) **a fixed comfortable rectangle**
  inside the FOV (e.g. the composition doc's comfort band — research/36's evidence is that large
  head-locked surfaces are what constraint 6 caps, and a full-FOV panel is large). *Distance:*
  WiVRn 0.5 m (lobby, `constants.h:103`), WayVR 0.5 m (keyboard z), windows spawn at
  `wm.spawn.distance_m` 1.5 m. *Consequence:* (a) makes bars hug the edge of vision and a
  bottom-anchored OSK fill the lower third of the view; (b) keeps chrome inside the sweet spot at
  the cost of an unaware full-anchor client (a lock screen) not covering the periphery.
  *Proposed as a settings key either way* (`shell.head.{extent_deg,distance_m}`), default per the
  ruling.
- **Q2 — `frame_extent` source per frame.** Head: Q1's answer. Body/hand/docked frames do not
  exist yet (spec §5); the server advertises `frames = head|world`. *Decision:* whether the
  world frame's extent is the same rectangle as the head's at spawn distance (a world-anchored
  layer surface is a poster) or unbounded (no exclusive zones in the world frame). *Comparables:*
  none — every 2D compositor has one rectangle per output. *Proposed:* world frame = the head
  rectangle at the spawn distance, exclusive angles ignored there (flagged as invention).
- **Q3 — may `bottom`/`background` surfaces be anchored to non-world frames?** A body-frame dock
  behind windows (band 2) reads naturally; a head-frame `background` (band 1) would occlude the
  environment. *Comparables:* research/60 §9 places the dock in the body frame; the protocol
  allows any frame. *Proposed:* allowed; `background` on any frame is composed in band 1 as a
  quad before the projection layer only when the environment permits (perception-passthrough
  design's call) — until then `background` surfaces are accepted and not composed (mapped,
  frame callbacks on the fallback cadence), which is what a wallpaper client tolerates.

**Status of the owner items (2026-09-27):** put to the owner and **not ruled** (the question was
skipped). To keep the workstream moving without laundering a decision (rule 4), the
implementation takes **provisional** positions, each a one-value settings key so the ruling is a
number change, and each marked *provisional* in spec rev 3.11 and shell-plane rev 0.2:
Q1a → (b) a fixed rectangle, `shell.head.extent_deg` seeded 90×70 (the anchoring protocol's own
example, `protocols/zxr-layer-anchoring-v1.xml:31-35`); Q1b → 0.5 m, `shell.head.distance_m`
(the two XR comparables that state a number, WiVRn and WayVR, both say 0.5); Q2 → as proposed;
Q3 → as proposed. **These remain open items with the owner as decider** (spec §14).

## 10. Sources

Pinned clones (`references/MANIFEST.json`): `wlroots`, `sway`, `phoc`, `river`, `cage`, `swaylock`,
`hyprland`, `kwin`, `mutter`, `smithay`, `niri`, `cosmic-comp`, `wayland-protocols`, `squeekboard`,
`wvkbd`, `mako`, `waybar`, `gtk-layer-shell`, `layer-shell-qt`, `phosh`, `wivrn`, `wayvr`,
`kscreenlocker`. [crate] `wayland-server 0.31.14`, `wayland-backend 0.3.17` (the build's vendored
sources). Repo: `protocols/zxr-layer-anchoring-v1.xml`, `pkgs/zxr/src/{state,scene}.rs`,
`pkgs/zxr/src/input/{mode,focus,hit,pointer}.rs`, ADR 0007 (+ amendment 2026-09-27),
session-auth rev 5, shell-plane.md rev 0.1, research/30, /36, /60, /68, /75. cage's pin carries no
layer-shell (`cage/cage.c:505-560`; kiosk, `README.md:5-6`) and mutter has none (`gtk_shell1`,
`mutter/src/wayland/meta-wayland-surface.c:1912-1917`); both are recorded as absences.
