# Window and workspace management — the manager's design

**Status: DRAFT, rev 0.1 (2026-09-26; six items ruled the same day, one — the exclusive-scene exit input — sent to research).** Derived from
[research/64](../research/64-window-workspace-management-from-comparables.md) (twelve rows, the
matrix, twelve verdicts) under ADR 0012's amendment (c) and ADR 0016. The six forks it raised
were brought to the owner and are ruled (§13; ADR 0012 amendment (ii)); the one item still
open — the reserved system input that leaves an exclusive scene — names its research. Design docs
specify; ordering and deferral live only in [implementation-path.md §5](implementation-path.md).

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
4. **No window cap — ruled by the owner (2026-09-26): "the user's responsibility."** Off-view
   planes are budgeted by the compositor (frame-callback throttling, texture GC); clutter is
   managed (§8), not prevented.
5. **World-fixed by default; follow is never the default and is opt-in per window/application —
   ruled by the owner (2026-09-26): "if I'm at my desk and walk away from it, my monitor doesn't
   follow me."** Opt-in follow has threshold, hysteresis and a rate limit; recenter exempts
   pinned content (verdict 8; research/36 §8).
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

A place has exactly one engine (places-model layer 3). Each engine implements
`spawn(member) -> pose`, `arrange(place)`, `on_member_moved(member)` and may refuse nothing — a
member the user drags stays where the user put it unless `managed` and the engine is asked to
`arrange`.

**Ruled by the owner (2026-09-26): the in-process default is minimal — it ships `free` only
(with the angular-slot allocator for spawn and tidy). `arc`, `dock`, `band` and everything after
them are built by Mura as *default external managers* over the seam and shipped with the
system** — river's posture for arrangement, with a working in-process floor the compositor can
always fall back to (§11 disconnect). The table below therefore describes engines of two kinds:
the one the compositor carries, and the ones Mura ships as manager programs.

| engine | where it lives | spawn | arrange (one-shot) | when a member is moved | precedent |
|---|---|---|---|---|---|
| `free` | **in-process (the floor)** | head-relative + angular free slot (§3) | tidy: the N most recently used members onto free angular slots at spawn distance, oldest first, unmanaged/pinned untouched | keeps the user's pose | visionOS, Android XR Home Space (+Tidy), Horizon detached |
| `arc` | shipped external manager | next free slot on an arc of radius `spawn.distance`, angular spacing `arc.spacing`, elevation band `arc.band` | re-pack the arc by recency | snaps to the nearest slot on release | kwin-vr `SpaceAllocator3D`, zen seat capsule, xrdesktop `arrange_sphere`, motorcar's fan |
| `dock` | shipped external manager | into the next of `dock.slots` slots on a bar angled toward the head; overflow displaces the least recent | none needed | detaching from the bar reparents the member to the place's sibling `free` place | Horizon Navigator dock, HoloLens Start, Stage Manager |
| `band` | shipped external manager | one curved band (`band.width_deg`, `band.curvature`) the members tile into left-to-right | re-tile | members slide along the band | Breezy, Mac Virtual Display, Horizon theater; the foreign-session mode-3 quad is a one-member band |

Engine parameters are per place, persisted with a pinned place (places-model §7). A fresh
transient place gets `free` unless a manager is running and assigns another. Every shipped
external manager is an ordinary session component (ADR 0012): its own process, started by the
session, selected by `wm.external_manager`, replaceable by the user's own; the seam's
`set_engine` carries `custom` for managers whose arrangement the compositor does not know.

## 5. Lifecycle

States a member can be in, and their protocol meaning:

| state | meaning | `xdg_toplevel` | who decides |
|---|---|---|---|
| mapped | a plane in a place | — | client commit + manager placement |
| hidden | not rendered, keeps place and pose, no frame callbacks | none (a compositor-side state, river's `hide`) | manager |
| minimized | **ruled (2026-09-26):** hidden, state kept, shown on the launcher/dock client with an indicator ("transitions the app to the background without quitting"); when no dock client runs the system degrades to close-is-the-verb with relaunch-into-place | `set_minimized` from clients is an event to the manager (river), never applied by the compositor | manager |
| maximized | fills the place's engine slot (`arc`/`dock`/`band`) or the spawn size × `wm.size.maximized` (`free`) | `maximized` configure state | manager on request |
| fullscreen | fills the *band* of its place; other members of the place hidden while fullscreen | `fullscreen` configure state | manager on request |
| exclusive | **ruled:** the client's scene takes the environment layer (§9 mechanism 1); native OpenXR apps are Monado's primary session with zxr as overlay (§9 mechanism 2) | `exclusive_requested` → `grant_exclusive` on the seam; a zxr-shell-v2 request for 3D clients | manager grants; the wearer's reserved system input always returns the shell (exit path: research pending) |
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

- **Default: `rigid` — ruled (2026-09-26).** Nothing follows the head; "apps stay in the
  workplace they originated". The wearer opts a window or application in, never the app itself.
- **`lazy-follow`** (opt-in per window or per application, from the place's bar or the
  launcher — HoloLens "Follow me", Horizon "move with you"): the place re-seats toward the head's forward when the head has been
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
Space), and the compositor may refuse.

**Exclusivity — ruled by the owner (2026-09-26): yes.** "This is analogous to fullscreen on the
desktop and most games will require this." Two mechanisms exist and both are Mura's:

1. **A zxr client's scene takes the environment layer** — a zxr-shell-v2 3D client (or a plane
   client's fullscreen scene) is *granted* the environment layer (`grant_exclusive` on the seam;
   the request arrives as `exclusive_requested`); at most one grant at a time; other apps'
   planes may be hidden by the manager; layers 4–6 (shell, overlay, foreground) stay presented
   in front (visionOS's rule for progressive/full: "helps people avoid losing track of windows
   behind virtual content").
2. **A native OpenXR application** (a game with its own OpenXR session) is not zxr's client at
   all: Monado's multi-client compositor decides which session is *primary* and *visible*
   (`ipc_handle_system_set_primary_client`, `monado-ctl -p <id>`; `xrt_syscomp_set_z_order`,
   `set_main_app_visibility` — `monado/src/xrt/ipc/server/ipc_server_handler.c:1563-1570`,
   `xrt_compositor.h:2395-2420`), and zxr stays presented as an **overlay session**
   (`XR_EXTX_overlay`, `XRT_FEATURE_OPENXR_OVERLAY`, `monado/CMakeLists.txt:417`) — the shape
   kwin-vr (research/31 §2.2, its Qt patch 0002) and WayVR already use. zxr is then the *shell
   that switches Monado's primary client*, which is what Monado's IPC hook is for.

**The exit path is open and needs its own research (§13 Q4-exit).** The owner: a "gesture" is
underdefined and should not be assumed; what must exist is a way to bring the shell's layers
back and leave the exclusive scene or quit the app. Every platform reserves a *system input* for
exactly this — OpenXR itself marks `/input/system/click` "may not be available for application
use" on every profile that has it (`openxr-docs/…/semantic_paths.adoc:716, 759, 886`), and the
device contract already names HMD-body buttons by role (`hmdButtons`, `selectRole`). The
research question is which reserved input(s) per device tier, what a short/long/double press
does (return shell / recenter / passthrough are the three actions the platforms split across
one button), and how that composes with the input floor — research/64 §16 collects the
platform facts; the decider is the owner, jointly with the input workstream.

## 10. Scene kinds

| kind | scale | placement | resize | facing | precedent |
|---|---|---|---|---|---|
| plane (2D window) | angular | §3 | pixels (client), apparent size held | billboard while moving | visionOS window, Horizon panel, Android XR spatial panel |
| volume (3D client, M2) | physical (metres) | §3 spawn pose; clipped to its half-size box | scale handle only | viewpoints, no rotation | visionOS volume, motorcar `CUBOID`, zen `bounded` |
| environment | — | the environment layer | — | — | visionOS immersive space, motorcar `PORTAL`, zen `expansive` |

## 11. The bounded seam — `zxr_window_management_v1`

**Shape.** river's (`river-window-management-v1.xml`): one manager client; two disjoint state
categories — *management state* (what the compositor tells windows: dimensions, fullscreen,
place assignment, focus requests), mutable only inside a `manage_start … manage_finish` sequence,
and *rendering state* (pose, hide/show, flags, engine, arrange), applied at `render_finish` —
so multi-window changes land in one frame. Stardust's `set_parent`/`set_transform` supply the
spatial verbs river's 2D `set_position` lacks. The wire text is in the XML; this section states
the boundary.

**Delegated (the manager's):** which place a member belongs to (`assign`); its local pose
(`set_pose`, clamped); proposed dimensions (`propose_dimensions` — a proposal the client may not
honour, river's semantics); hide/show; flags; maximize/fullscreen; a place's engine and
`arrange`; **focus requests backed by a user interaction** — ruled (2026-09-26): the compositor
tells the manager of every commit on a managed window with its serial (`interaction`); a
`focus(window, serial)` is honoured under [spatial-input.md §6](spatial-input.md)'s rule (the
serial is a user interaction at least as recent as the seat's last commit) and is otherwise
**urgency-only** — the window is marked as demanding attention, never raised or focused. The
manager thus has exactly an application's standing under `xdg-activation` and can no more break
the wearer's focus than an app can; it may build a focus *policy* on the user's actions ("focus
what was just pinched into", "focus what was just docked") but cannot originate focus. visionOS
is the same rule with no manager at all: the active window follows the eyes and typing needs the
tap, and nothing an app can call changes either. Also delegated: responses to client requests the
compositor re-emits (`move_requested`, `resize_requested`, `maximize_requested`,
`fullscreen_requested`, `minimize_requested`, `exclusive_requested`).

**Kept (the compositor's, never on the wire):** the existence and poses of frames (runtime-
located); the boundary; the comfort limits every pose is clamped to and reported back in
`limits` (`min_distance`, `max_distance`, `max_angular_size`, head-anchoring only on the overlay
place); focus *rules* (stealing prevention, activation tokens — input workstream); the perception
layers and safety occlusion; hit-testing and input routing; recenter; composition. A proposal
that violates a limit is clamped, not rejected, and the applied state is reported — the manager
learns the truth from `state`, as river's WM learns real dimensions from `dimensions`.

**Disconnect — ruled by the owner (2026-09-26, "Hyprland's shape").** When the manager's
connection ends — crash, exit, or unload — the compositor *continues with its built-ins*: the
in-process `free` floor takes over for new events, existing placements are left exactly as they
are, nothing becomes inert (Hyprland ejects a crashed plugin and keeps running; contrast river,
which makes every object inert and waits). A manager that reconnects — restarted by its unit,
or a different one — binds the global and takes over from the current state (river's hot-swap),
receiving the full state in its first manage sequence. The XML states this.

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

## 13. Rulings and open items (research/64 §15 has the full positions)

**Ruled by the owner, 2026-09-26** (recorded in the sections named):

- **Q1 minimize** — keeps state, parks on the launcher/dock client with an indicator; degrades
  to close-is-the-verb when no dock client runs (§5). The tray/launcher component of ADR 0012 is
  the dock.
- **Q3 cap** — none; "the user's responsibility" (§1.4).
- **Q4 exclusivity** — yes, fullscreen's analogue; both mechanisms of §9 are Mura's. **The exit
  path is not a gesture to be assumed; it is open** — see below.
- **Q6 follow** — never by default; opt-in per window/application (§1.5, §7).

**Open (deciders named):**

- **Q2 — ruled (2026-09-26): minimal in-process.** The compositor carries `free` only; `arc`,
  `dock`, `band` and later engines are built by Mura as default external managers and shipped
  (§4).
- **Q5-disconnect — ruled (2026-09-26): Hyprland's shape.** The compositor continues with its
  built-ins; placements untouched; a reconnecting manager takes over (§11).
- **Q4-exit — how the wearer leaves an exclusive scene and brings the shell back.** Not a
  gesture by assumption; a reserved system input per device tier (OpenXR's `/input/system/click`
  is reserved from applications for this; every platform binds one physical control to
  shell/recenter/passthrough by press length). Needs its own short research pass; research/64
  §16 holds the platform facts gathered so far. Decider: the owner, jointly with the input
  workstream.
- **Q5-focus — ruled (2026-09-26): a manager requests focus only on the back of a user
  interaction.** "A window manager should not be able to break the user's experience of focus."
  The manager receives every commit's serial (`interaction`) and may `focus(window, serial)`;
  the compositor applies spatial-input.md §6's rule and, on refusal, marks urgency (§11). Same
  standing as an application under `xdg-activation`; consistent with visionOS, where the active
  window follows the eyes, typing needs the tap, and no program can set either.
