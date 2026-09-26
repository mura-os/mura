# Window and workspace management — the manager's design

**Status: DRAFT, rev 0 (2026-09-26).** Derived from
[research/64](../research/64-window-workspace-management-from-comparables.md) (twelve rows, the
matrix, twelve verdicts) under ADR 0012's amendment (c) and ADR 0016. Six items are **open** and
carried here as such with the comparables' positions and a labelled read (§13); nothing in
those items is a decision until the owner rules. Design docs specify; ordering and deferral live
only in [implementation-path.md §5](implementation-path.md).

**What this document is.** The compositor's `policy` module (spec §3) has two faces: the
**in-process default manager** every session runs, and the **bounded seam** —
`protocols/zxr-window-management-v1.xml` — over which a separate program may replace the
policy while the compositor keeps the invariants a manager may not override. This document
specifies both: what the default manager does for the wearer, and which verbs the seam carries.
It consumes the input workstream's outputs (targeting, hover, commit, grab mechanics, focus and
activation *rules*, cursor, peripherals — research/63, spatial-input.md) as given; where a
gesture and an arrangement rule touch, this doc states the arrangement rule and names the other.

**Grounding.** Frames, places, members, currency and the reparent verbs are
[places-model.md](places-model.md)'s (ADR 0016); the scene arenas that hold them are
[specs/zxr-core.md §5a](../../specs/zxr-core.md); layers are spec §4; the `xdg_*` protocol
namespace keeps its upstream meanings (ADR 0012 §4). "Environment" means the spec §4 environment
layer. Reference spaces and views are OpenXR's (`LOCAL`, `VIEW`, `xrLocateSpaces`).

**Budget impact** (overview invariant 9): the default manager is a set of pure functions over
the scene arenas, run on the state loop when a window maps, a verb arrives, or a place's engine
is asked to arrange — no per-tick work beyond the follow/tether constraint of §7, which is one
pose comparison per followed place per tick. The external seam is one Wayland client and one
manage/render transaction per policy change (river's shape: "frame perfect", double-buffered).
No new thread, no polling. Preferences come from `org.mura.Settings1` (D7), read at start and on
signal.

## 1. Principles the evidence fixed (research/64 §14)

1. **Apps do not place or size their own windows in space** (verdict 1). They express size
   hints (min/max/default, resizability, aspect lock — through `xdg_toplevel` today) and
   *relative* wishes (parent, popup, "utility panel near this window"), which the places model
   expresses as membership and children, never as coordinates.
2. **Head-relative spawn at a contract-declared distance, slightly below the eye line, turned to
   face the wearer; a second window offset in front of/adjacent to the first, never on top**
   (verdict 2). Numbers come from the device contract and settings, never from code.
3. **Planes scale angularly; volumes scale physically** (verdict 3). A plane keeps its apparent
   size as it moves in depth so legibility and target size — angular quantities — hold; a 3D
   client volume keeps its metres because it is a physical object.
4. **No window cap** (verdict 6, AGENTS.md rule 3). Off-view planes are budgeted by the
   compositor (frame-callback throttling, texture GC); clutter is managed (§8), not prevented.
5. **World-fixed by default; follow is opt-in with threshold, hysteresis and a rate limit;
   recenter is the compositor's one gesture, exempting pinned content** (verdict 8; research/36
   §8).
6. **Surface snapping is the spatial snap** (verdict 9): pinning to a detected surface is the
   places-model `pin` verb onto an anchor frame; there is no window-to-window snap.
7. **Safety occlusion is not the manager's** (verdict 11): proximity dimming, boundary fade and
   the hand cutout are perception-layer invariants no manager can touch.
8. **The environment is the wearer's; apps request, the compositor may refuse** (verdict 12).

## 2. The manager's state and verbs

The manager operates only through the scene's mutation API (spec §5a): `add`, `remove`,
`reparent` (place → frame; member → place), `set_local` (pose), `set_flags`, `focus` — plus the
place-level `set_engine` and `arrange` this document adds. The in-process default calls it
directly; the seam exposes the same verbs on the wire (§11). Anything the manager cannot do
through these verbs it cannot do at all — that boundary *is* the compositor's invariant set.

Member flags carry xrdesktop's vocabulary (`xrd-shell.c:205-236`): `draggable` (the user may
move it), `managed` (the engine may move it), `hoverable`, `pinned` (excluded from tidy and
recenter). Children (popups, transients, xdg dialogs) are never `draggable` or `managed`
independently — "position is managed by its parent, not the WM".

## 3. Placement and sizing (the default policy)

- **First window of an app** (no parent, no place hint): into the *current* place (currency:
  places-model §4 — the place the wearer is located in; if none, the head frame's transient
  place), at the spawn pose the place's engine returns (§4). For the free engine that pose is:
  distance `spawn.distance` along the head's forward projected to the world horizontal,
  elevation `spawn.elevation` below the eye line, yaw to face the head; if that pose is occupied
  beyond `spawn.overlap`, the engine's allocator finds the nearest free angular slot (kwin-vr
  `SpaceAllocator3D`'s search, research/36's selection).
- **Subsequent windows of the same app** and **windows with a parent**: same place as the
  parent/sibling, offset in front and to the side by `spawn.sibling_offset` (WayVR `Spread`'s
  shape: left, down, closer), never fully covering it (KWin `cascadeIfCovering`'s test).
- **Popups, transients, dialogs**: children of their parent plane at the child z-gap (spec §5a
  stand-in 0.5 mm; motorcar 0.05 m); positioned by their `xdg_positioner`/parent relation, as
  smithay already computes; xdg dialogs centred over the parent with more room below than above
  (mutter `place.c:997-1004`).
- **Size at spawn**: the client's `xdg_toplevel` requested/default size, clamped to
  `[size.min, size.max]` from the contract; density `density.px_per_cm` (stand-in 20 px/cm — the
  two Linux XR shells' value) converts logical pixels to metres *at the spawn distance*; from
  then on the plane's apparent size is angular (§3 principle 3), so "resize" changes pixels and
  "move in depth" changes metres. Volumes (M2) take their metres from the client.
- **Keys** (`org.mura.Settings1`, schema owned by this doc; values from the device contract's
  `display` group as defaults): `wm.spawn.distance` (m), `wm.spawn.elevation` (deg below eye
  line), `wm.spawn.overlap` (0–1), `wm.spawn.sibling_offset` (m), `wm.density.px_per_cm`,
  `wm.size.min`, `wm.size.max` (logical px). Comparable values recorded beside each in
  research/64 §2 so the contract author picks from evidence: distance WiVRn 0.5 / Horizon 1.0 /
  Breezy 1.05 / kwin-vr 1.0 / Android XR 1.75 / visionOS ~2.0 m; elevation Android XR 5°,
  Microsoft 10–20° resting gaze.

## 4. Layer-3 engines and their selection

A place has exactly one engine (places-model layer 3). The default manager ships four; each
implements `spawn(member) -> pose`, `arrange(place)`, `on_member_moved(member)` and may refuse
nothing — a member the user drags stays where the user put it unless `managed` and the engine
is asked to `arrange`.

| engine | spawn | arrange (one-shot) | when a member is moved | precedent |
|---|---|---|---|---|
| `free` | head-relative + angular free slot (§3) | tidy: the N most recently used members onto free angular slots at spawn distance, oldest first, unmanaged/pinned untouched | keeps the user's pose | visionOS, Android XR Home Space (+Tidy), Horizon detached |
| `arc` | next free slot on an arc of radius `spawn.distance`, angular spacing `arc.spacing`, elevation band `arc.band` | re-pack the arc by recency | snaps to the nearest slot on release | kwin-vr `SpaceAllocator3D`, zen seat capsule, xrdesktop `arrange_sphere`, motorcar's fan |
| `dock` | into the next of `dock.slots` slots on a bar angled toward the head; overflow displaces the least recent | none needed | detaching from the bar reparents the member to the place's sibling `free` place | Horizon Navigator dock, HoloLens Start, Stage Manager |
| `band` | one curved band (`band.width_deg`, `band.curvature`) the members tile into left-to-right | re-tile | members slide along the band | Breezy, Mac Virtual Display, Horizon theater; the foreign-session mode-3 quad is a one-member band |

Engine parameters are per place, persisted with a pinned place (places-model §7). **Which engine
a fresh transient place gets is open (§13 Q2).** Additional engines (tiling, scrollable strip,
adaptive) arrive only over the seam as external managers or as in-process modules once evidence
exists; the seam's `set_engine` carries a `custom` value for the former.

## 5. Lifecycle

States a member can be in, and their protocol meaning:

| state | meaning | `xdg_toplevel` | who decides |
|---|---|---|---|
| mapped | a plane in a place | — | client commit + manager placement |
| hidden | not rendered, keeps place and pose, no frame callbacks | none (a compositor-side state, river's `hide`) | manager |
| minimized | *open item Q1*: either "hidden + shown on the launcher/dock with state" or "does not exist; close is the verb" | `set_minimized` from clients is an event to the manager (river), never applied by the compositor | manager |
| maximized | fills the place's engine slot (`arc`/`dock`/`band`) or the spawn size × `wm.size.maximized` (`free`) | `maximized` configure state | manager on request |
| fullscreen | fills the *band* of its place; other members of the place hidden while fullscreen | `fullscreen` configure state | manager on request |
| exclusive | *open item Q4*: the client's scene takes the environment layer | none today; a `zxr_` request if ruled | compositor policy + wearer |
| closed | `xdg_toplevel` destroyed | — | client; the manager's `close` sends `xdg_toplevel.close` |

Restore: places-model §7 — a pinned place remembers member identities and poses; an app that
relaunches into a remembered slot is placed there (visionOS scene restoration, HoloLens tiles,
Windows re-dock — universal). Closing the last member of a transient place evaporates the place
after a grace period (GNOME's 1 s dialog grace, `windowManager.js:173-179`) so an app's splash →
main-window sequence does not lose its place.

## 6. Grouping and switching

The model is places-model §4–§6 (currency decomposed into located / presented / pager-active;
`ext-workspace-v1` + `zxr-workspace-v1` for the pager). The manager's contribution is what the
switching *surfaces* read and may request:

- **Attention is the primary switch.** The place the wearer is located in is current; the
  member under the gaze/ray is the candidate for activation (the input workstream owns the
  rule). No explicit switcher is required for this (visionOS has none) — but Mura's tiers
  without eye tracking get the explicit one below.
- **The launcher/dock client** (ADR 0012: `foreign-toplevel-list` + `xdg-activation`, later
  `ext-workspace`) lists running apps and, under Q1(b), minimized members with an indicator
  (Horizon's dot). It requests `activate`; the manager honours it under the focus rules.
- **The pager/overview client** reads `ext-workspace` + `zxr-workspace` (places-model §6) and
  requests `assign` (reparent a place to a frame) and `activate`; it is "a facilitator and a
  mediator, not a destination" (GNOME) — it never holds authority.
- **Summon** (places-model `summon`): presents a pinned place at the wearer's position without
  moving its home; `recenter` never touches pinned places.
- **Walking**: a pinned place on a map anchor appears when its room is entered (visionOS 26's
  per-room locked scenes are the shipping precedent); currency `located` flips; nothing is
  reparented.

## 7. Follow and tether (layer-2 defaults)

Attachment constraints per places-model layer 2: `rigid` (default for every place on a world or
anchor frame), `lazy-follow`, `billboard`, `tether`. Defaults and parameters:

- **Default: `rigid`.** Nothing follows the head unless the wearer or the app's place asks.
- **`lazy-follow`** (opt-in per place, one gesture on the place's bar — HoloLens "Follow me",
  Horizon "move with you"): the place re-seats toward the head's forward when the head has been
  more than `follow.threshold` degrees off the place's centre for `follow.delay` seconds, moving
  at most `follow.rate` degrees/second and stopping within `follow.stop` degrees; never while a
  member of the place is grabbed (WayVR `pause_movement`). Comparable values: HoloLens
  MaxViewDegrees/MinDistance; Breezy 15°/1 s; kwin-vr 40°/20° start, 4° stop, 0.5 s, 2.0 —
  contract-declared, with research/36's caps as the ceiling.
- **Out-of-view fallback** (Lindlbauer 2019): a member the wearer must not lose (an alert, a
  call) is *presented* in the head frame while its home is out of view and returns when the
  wearer turns toward it — a presentation, never a reparent; the overlay-class place of
  places-model §4.3.
- **`billboard`**: planes turn to face the head *while being moved* and hold on release
  (visionOS, HoloLens, kwin-vr `turnToFaceKeepRoll`); volumes present their nearest viewpoint
  (visionOS volumes) rather than rotating.
- **Recenter** (research/36 §8 determination): rigid re-seat of every head-relative place;
  pinned places exempt; the compositor's gesture, never a client's.

## 8. Clutter

- **Tidy** is the engine's `arrange` on the current place, exposed as one named action to the
  shell (Android XR's Tidy, xrdesktop `arrange_sphere`); it never moves `pinned` members.
- **Focus mode** (Horizon theater; visionOS inactive-window recession): the manager asks the
  compositor to *emphasize* one member — the environment layer dims by `focus.dim`, siblings
  drop to `focus.sibling_alpha` — a presentation state on the place, not a reparent; leaving it
  restores.
- **Safety occlusion** — proximity dimming, boundary fade, breakthrough — is the perception
  layers' (spec §4; research/62 §3.5) and is not reachable from the manager or the seam.
- **Adaptive layout** (Lindlbauer 2019; SemanticAdapt) is recorded as an engine candidate over
  the seam; no shipping platform does it and the default manager does not.

## 9. Environments and exclusivity

The environment layer's content is chosen by the wearer (a wallpaper client on layer-shell
`background`; the passthrough producer over the perception intake) and requested by apps as a
preference: an app may ask to *replace* the environment with its own scene or to *coexist*
with it (visionOS `.immersiveEnvironmentBehavior`; Android XR's `SpatialEnvironment` in Full
Space), and the compositor may refuse. **Whether a client may request exclusivity — one scene
taking the environment layer, other apps' windows hidden, the shell's own windows and the
wearer's exit gesture preserved — is open (§13 Q4).** If ruled in, it is a `zxr_` request on
the seam and on zxr-shell-v2, at most one at a time, always reversible by the compositor's
recenter/summon gesture, and never hiding layers 4–6 (visionOS's own rule that windows render in
front of progressive/full content).

## 10. Scene kinds

| kind | scale | placement | resize | facing | precedent |
|---|---|---|---|---|---|
| plane (2D window) | angular | §3 | pixels (client), apparent size held | billboard while moving | visionOS window, Horizon panel, Android XR spatial panel |
| volume (3D client, M2) | physical (metres) | §3 spawn pose; clipped to its half-size box | scale handle only | viewpoints, no rotation | visionOS volume, motorcar `CUBOID`, zen `bounded` |
| environment | — | the environment layer | — | — | visionOS immersive space, motorcar `PORTAL`, zen `expansive` |

## 11. The bounded seam — `zxr_window_management_v1`

**Shape.** river's (`river-window-management-v1.xml`): one manager client; two disjoint state
categories — *management state* (what the compositor tells windows: dimensions, fullscreen,
place assignment, focus hint), mutable only inside a `manage_start … manage_finish` sequence,
and *rendering state* (pose, hide/show, flags, engine, arrange), applied at `render_finish` —
so multi-window changes land in one frame. Stardust's `set_parent`/`set_transform` supply the
spatial verbs river's 2D `set_position` lacks. The wire text is in the XML; this section states
the boundary.

**Delegated (the manager's):** which place a member belongs to (`assign`); its local pose
(`set_pose`, clamped); proposed dimensions (`propose_dimensions` — a proposal the client may not
honour, river's semantics); hide/show; flags; maximize/fullscreen; a place's engine and
`arrange`; focus *hints* (§13 Q5); responses to client requests the compositor re-emits
(`move_requested`, `resize_requested`, `maximize_requested`, `fullscreen_requested`,
`minimize_requested`, `exclusive_requested`).

**Kept (the compositor's, never on the wire):** the existence and poses of frames (runtime-
located); the boundary; the comfort limits every pose is clamped to and reported back in
`limits` (`min_distance`, `max_distance`, `max_angular_size`, head-anchoring only on the overlay
place); focus *rules* (stealing prevention, activation tokens — input workstream); the perception
layers and safety occlusion; hit-testing and input routing; recenter; composition. A proposal
that violates a limit is clamped, not rejected, and the applied state is reported — the manager
learns the truth from `state`, as river's WM learns real dimensions from `dimensions`.

**Disconnect** (§13 Q5): the design intent is *revert to the in-process default policy for new
events, leave existing placements untouched* — Mura has the default river lacks; the XML states
only that behaviour on disconnect is compositor policy until ruled.

**Unavailability:** the global is advertised only to the manager process (the session's
configured WM client, spawned by the session like any shell component); a second binder gets
`unavailable`.

## 12. Settings

Schema keys this document owns (defaults from the device contract; every key a stand-in until
M1 measurement): `wm.spawn.distance`, `wm.spawn.elevation`, `wm.spawn.overlap`,
`wm.spawn.sibling_offset`, `wm.density.px_per_cm`, `wm.size.min`, `wm.size.max`,
`wm.size.maximized`, `wm.engine.default` (Q2), `wm.minimize` (Q1), `wm.follow.threshold`,
`wm.follow.delay`, `wm.follow.rate`, `wm.follow.stop`, `wm.focus.dim`,
`wm.focus.sibling_alpha`, `wm.external_manager` (the executable of an external manager, empty =
in-process default). The `ownership` of each is declarative by default (settings-schema.md).

## 13. Open items (deciders named; research/64 §15 has the full positions)

- **Q1 — minimize by default.** (a) none — close is background, relaunch into place (visionOS,
  niri); (b) minimize keeps state on the launcher/dock with an indicator (Horizon, GNOME,
  cosmic). The protocol is neutral (river's shape). My read, labelled: (b) for the in-process
  default because the dock client exists; (a) when no dock client runs. Owner.
- **Q2 — the default engine for a fresh place.** (a) free + offset spawn + tidy (visionOS,
  Android XR, Horizon); (b) angular slots (kwin-vr, zen, xrdesktop, research/36); (c) dock;
  (d) band. My read, labelled: `free` whose spawn/tidy allocator is the angular-slot search —
  (b) inside (a). Owner.
- **Q3 — a hard window cap.** None anywhere (visionOS, Android XR, kwin-vr, niri) vs slot/activity
  caps (Horizon 6/12, HoloLens 3 active, iPadOS 4). Rule 3 forbids the cap; the compositor
  budgets off-view planes. Recorded as a determination in research/64 unless the owner objects.
- **Q4 — client-requested exclusivity.** visionOS (one immersive space, user can always reduce
  immersion), Android XR (`requestFullSpace` + cue to leave), Horizon (immersive app + 3
  windows), Linux (none beyond per-output fullscreen). My read, labelled: visionOS's shape on the
  environment layer with the shell's layers always visible. Owner — touches zxr-shell-v2 and the
  protocol posture.
- **Q5 — focus on the seam, and disconnect.** river's manager owns focus; ADR 0012's amendment
  keeps "focus rules" in the compositor. My read, labelled: the manager *hints*, the compositor's
  rules decide; on disconnect the compositor reverts to the default policy. Owner (joint with
  the input workstream, which owns the rules themselves).
- **Q6 — follow default.** never (visionOS) / per-window opt-in (HoloLens, Horizon) / follow-all
  (kwin-vr). My read, labelled: opt-in per place with the out-of-view fallback as presentation;
  near a determination (verdict 8), flagged because kwin-vr chose otherwise. Owner.
