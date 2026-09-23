# 34 — Workspace/places models across desktop and spatial OSes

**Status:** research complete
**Date:** 2026-09-23
**Scope:** evidence + mapping for the registry's structural gap #1 (the spatial-workspace/space
model, [component-registry.md §8](../architecture/component-registry.md)). This document makes
**no design decision**. Companions it extends, not repeats:
[desktop-environment.md §5](../architecture/desktop-environment.md) (workspace = place),
[doc 30 §2.1](30-wayland-de-anatomy-protocol-seams.md) (`ext-workspace-v1`),
[ADR 0009](../architecture/adr/0009-spatial-mapping-architecture.md) (anchors),
[spatial-sharing.md §5](../architecture/spatial-sharing.md) (mode-5 join),
[ADR 0015](../architecture/adr/0015-docked-desktop-mode.md) (docked mode), and
[foreign-session-integration.md §2](../architecture/foreign-session-integration.md) (delegation).

**Vocabulary note (which "XDG").** Per [desktop-environment.md §2 trap 3](../architecture/desktop-environment.md)
and [doc 30 A6](30-wayland-de-anatomy-protocol-seams.md): everything protocol-shaped here is
sense (b), the `wayland-protocols` namespaces — `ext-workspace-v1` (staging/ext, not `xdg_*`)
and `xdg-session-management-v1` (staging/xdg). The CDG spec family (a) has **no workspace
concept at all** — no freedesktop specification defines virtual desktops; the nearest artifact
is EWMH's X11 `_NET_NUMBER_OF_DESKTOPS`/`_NET_WM_DESKTOP` root properties, which is exactly why
every Wayland compositor's model diverged freely. Portals (c) are uninvolved. "Workspace" below
means the compositor-model object `ext-workspace-v1` exports; "place" means the
[desktop-environment.md §5](../architecture/desktop-environment.md) redefinition: a named set of
windows bound to a spatial anchor or portable layout.

## 0. Executive result

- **No shipping desktop model is the anchored-places model**, but two systems bracket it:
  **COSMIC** is the closest *mechanism* precedent (dynamic workspace objects with optional
  session-stable IDs and pinned persistence, per-output groups, all exported over ext-workspace +
  a small private extension), and **visionOS 26** the closest *semantics* precedent (windows
  locked to rooms via anchors, restored per room across reboots — but with *no grouping object*:
  persistence is per-scene, keyed by room). An anchored place is structurally "a COSMIC pinned
  workspace whose group is a room instead of an output."
- **The two-axis lesson from KWin is a warning, not a template.** Desktops and Activities are
  orthogonal axes multiplied against every window (visibility = on-current-desktop ∧
  on-current-activity), and the axis KDE built for session-ish semantics (Activities) is being
  cut back: per-activity subsession save/restore was dropped for Plasma 6.5 because window↔axis
  cardinality (a window on N activities) is at odds with per-axis session restore. Places should
  start single-axis; the room *is* the context Activities tried to synthesize.
- **Topology is compositor policy, proven divergent** (KWin grid, GNOME dynamic linear, COSMIC
  per-output dynamic + pinned, niri per-output scrollable strips) — and `ext-workspace-v1`
  round-trips only identity/name/state/coordinates, so places can diverge just as far while
  keeping pagers working.
- **The sharpest open question:** what "active" and "switch" mean when a place is a location —
  ext-workspace assumes activation is a compositor-effected state change, but reaching an
  anchored place can be a physical walk the compositor observes rather than performs.

## 1. KWin: the two-axis model (virtual desktops × Activities)

### 1.1 Axis one: virtual desktops

Code-level anatomy (pinned clone):

- A desktop is an object with a **generated stable string `id`, a mutable human `name`, and an
  X11 ordinal** ([`virtualdesktops.h`](../../references/kwin/src/virtualdesktops.h) class
  `VirtualDesktop`). The manager owns an ordered list (max 25), arranged into a **user-configured
  x/y grid** (`VirtualDesktopGrid`, `m_rows` default 2) used only for directional navigation and
  pager layout — the grid is presentation-order, not membership structure.
- **Window membership is a set**: `Window::m_desktops` is a `QList<VirtualDesktop*>`; the empty
  list means on-all-desktops ([`window.h`](../../references/kwin/src/window.h) ~L435, L1002–1008,
  L2034). A window can be on several desktops without being on all — strictly richer than
  GNOME's model (§2).
- **Per-output currency is a toggle, not the structure**: one shared desktop list, but
  `m_currentDesktops` maps each output to its own current desktop when
  `perOutputVirtualDesktops` is enabled ([`virtualdesktops.h`](../../references/kwin/src/virtualdesktops.h)
  L185, L592–593). Desktops are global objects; only *which is current* becomes per-output.
- **What switching does** (`Workspace::updateWindowVisibilityAndActivateOnDesktopChange`,
  [`workspace.cpp`](../../references/kwin/src/workspace.cpp) ~L1104): close popups, update
  visibility for windows entering/leaving (visibility test =
  `isOnDesktop(new) && isOnCurrentActivity() && isOnOutput(output)`), re-request each window's
  tile for the new desktop, restore focus from the per-desktop focus chain. Two details worth
  stealing: an **in-progress interactive move carries the window to the target desktop**
  (`m_moveResizeWindow->setDesktops({newDesktop})`), and switching emits a *realtime offset*
  signal (`currentChanging`, gesture-driven) — switching is an animatable scalar, not a boolean.
- **What persists:** desktop count/names/rows in config (`VirtualDesktopManager::save()`); the
  X11 session-management path stores **per-window desktop and activity membership**
  ([`sm.cpp`](../../references/kwin/src/sm.cpp) L164, L181); the Wayland path is
  `xdg-session-management-v1` (doc 30 A2). Desktops themselves always exist at start (recreated
  from config), independent of windows.
- The pager seam is **private**: `org_kde_plasma_virtual_desktop_management`
  ([`wayland/plasmavirtualdesktop.h`](../../references/kwin/src/wayland/plasmavirtualdesktop.h));
  KWin 6.7 still does not implement `ext-workspace-v1` (doc 30 §3.5).

### 1.2 Axis two: Activities

- Activity **lifecycle lives outside the compositor**: KWin's `Activities` wraps
  `KActivities::Controller`, a D-Bus client of `kactivitymanagerd`
  ([`activities.h`](../../references/kwin/src/activities.h) — the controller member;
  [`activities.cpp`](../../references/kwin/src/activities.cpp) constructor). The compositor
  mirrors the list and current activity; creation/deletion/metadata belong to the daemon, which
  also keys per-activity favorites and document history (with per-activity privacy switches —
  [KDE activities KCM docs](https://docs.kde.org/trunk_kf6/en/plasma-desktop/kcontrol/kcmactivities/index.html)).
- **Window membership mirrors the desktop shape**: `QStringList m_activityList`, empty = all
  activities ([`window.cpp`](../../references/kwin/src/window.cpp) ~L4455 — "all activities ===
  no activities"). Visibility is the **conjunction of the two axes** (§1.1's test), so the model
  is a desktop×activity matrix per window with "all" wildcards on each axis.
- **What activity switching does:** almost nothing mechanical in KWin — update `m_current`, then
  *restore that activity's last-current desktop per output* from persisted state
  (`m_lastVirtualDesktop[activity][outputUuid]`,
  [`activities.cpp`](../../references/kwin/src/activities.cpp) `slotCurrentChanged`), then let
  the ordinary visibility machinery react. The axes are coupled exactly once: each activity
  remembers its desktop-cursor.
- **The session-ish semantics are being removed.** KWin's constructor still deletes legacy
  "SubSession:" config groups ([`activities.cpp`](../../references/kwin/src/activities.cpp));
  per-activity start/stop (freeze apps of a stopped activity, restore on start) was dropped for
  Plasma 6.5 with a stated structural reason: XSMP/Wayland support decay, sandbox holes, and —
  decisive for us — **"a window/app can be on two activities at once… The concept of session
  restoration is at odds with the cardinality of windows to activities"**
  ([Edmundson, 2025-09-19](https://planet.kde.org/david-edmundson-2025-09-19-upcoming-changes-to-activities-in-plasma-6-5/)).
- **The two-axis lesson:** the second axis earns its complexity only where the first cannot
  express *context* (same monitor, different project). Every window pays the membership-matrix
  cost, shells expose two switchers, and the axis with session ambitions collapsed under restore
  cardinality. An anchored place already *is* a context (the kitchen is not the office) — the
  burden of proof is on adding a second axis, not omitting it (§8.1).

## 2. GNOME: dynamic workspaces, shell as the lifecycle controller

- **Mechanism (Mutter):** `MetaWorkspaceManager` holds a single ordered list; grid fields exist
  for EWMH pager compat but default to one row
  ([`meta-workspace-manager.c`](../../references/mutter/src/core/meta-workspace-manager.c) L219–221).
  **A window is on exactly one workspace or on-all-workspaces** — a boolean plus a single
  pointer, not a set ([`window.c`](../../references/mutter/src/core/window.c)
  `set_workspace_state`, `should_be_on_all_workspaces`). Strictly poorer than KWin, and enough
  for GNOME.
- **The per-monitor toggle inverts KWin's:** with `workspaces-only-on-primary`, windows on
  secondary monitors are simply forced on-all-workspaces
  ([`window.c`](../../references/mutter/src/core/window.c) L5301–5313) — secondary monitors
  opt *out* of the workspace dimension entirely rather than getting their own currency.
- **Policy (the shell, not the compositor):** dynamic append/trim is GNOME Shell JS —
  `_checkWorkspaces` keeps ≥2 workspaces, guarantees one trailing empty workspace, removes empty
  middles (with keep-alive grace, splash/dialog exceptions, startup-sequence reservations)
  ([`windowManager.js`](../../references/gnome-shell/js/ui/windowManager.js) L221–293,
  `MIN_NUM_WORKSPACES = 2` L42). Mutter honours `dynamic-workspaces` merely as a pref
  ([`prefs.c`](../../references/mutter/src/core/prefs.c) L303); the overview is the controller UI
  ([`workspaceThumbnail.js`](../../references/gnome-shell/js/ui/workspaceThumbnail.js) L653–699,
  [`workspacesView.js`](../../references/gnome-shell/js/ui/workspacesView.js) L742–760). Because
  shell and compositor share one process (doc 30 §4.5), this "policy outside the mechanism"
  split cost GNOME no protocol — for us it requires one (§8.2).
- **Nothing persists.** Workspaces are anonymous indices; there are no stable IDs; GNOME removed
  X11 session restore in GNOME 49 and its `xdg-session-management` replacement is an initiative,
  not a feature (doc 30 A2.2). GNOME therefore demonstrates the pure opposite pole from places:
  workspaces as **ephemeral overflow structure**, identity-free by design.

## 3. COSMIC: per-output groups, dynamic + pinned, the ext-workspace producer

- **Structure:** one `WorkspaceSet` per output, each owning an ext-workspace **group handle**, a
  dynamic `Vec<Workspace>`, and a per-set sticky layer
  ([`shell/mod.rs`](../../references/cosmic-comp/src/shell/mod.rs) L370–381). A `Workspace` is
  tiling + floating layouts, fullscreen, minimized windows, focus stack, `handle`, optional
  `name`/`id` ([`shell/workspace.rs`](../../references/cosmic-comp/src/shell/workspace.rs)
  L104–121). A window belongs to exactly one workspace's layout (or the sticky layer).
- **Two global modes**, `OutputBound` (default) vs `Global`
  ([`cosmic-comp-config/src/workspace.rs`](../../references/cosmic-comp/cosmic-comp-config/src/workspace.rs)
  L34–38): the same model presented as independent per-output sets or as one synchronized set —
  runtime-switchable policy over unchanged structure (`shell/mod.rs` L1190ff).
- **Persistence = pinning, and only pinning.** A pinned workspace serializes as
  `PinnedWorkspace { output: OutputMatch, tiling_enabled, id, name }` to cosmic-config
  (`shell/mod.rs` `persist()` L1505ff; struct in `cosmic-comp-config/src/workspace.rs` L62–67)
  and is recreated at start with its saved ext-workspace `id`
  (`create_workspace_from_pinned`, `shell/mod.rs` L423ff) — the workspace's **existence and
  identity** survive reboot; its window contents do not (the session-restore problem, §7).
  Unpinned dynamic workspaces get no `id` (`shell/mod.rs` L400 — `None`), matching the
  protocol's contract that ID-less workspaces are temporary.
- **What actually round-trips over `ext-workspace-v1`** (extending doc 30 §2.1 with the producer
  view): per-workspace `name`, optional `id`, **1-D coordinates = index in group**
  (`shell/mod.rs` L5173 `set_workspace_coordinates(handle, &[idx])`), states (active, urgent,
  hidden; pinned via the v2 extension), and capabilities `Activate | SetTilingState | Pin | Move`
  (`shell/mod.rs` L407–413). Renaming, pin/unpin, and reordering (`move_before`/`move_after`)
  ride the private `zcosmic_workspace_handle_v2`
  ([XML](../../references/cosmic-protocols/unstable/cosmic-workspace-unstable-v2.xml)).
- **The missing upstream link is toplevel↔workspace membership.** `ext-foreign-toplevel-list`
  carries no workspace; COSMIC adds `ext_workspace_enter/leave` on its private toplevel-info
  handle ([XML](../../references/cosmic-protocols/unstable/cosmic-toplevel-info-unstable-v1.xml)
  v3, L245ff). Any pager that shows *which windows are where* — ours included — needs this
  extension shape; upstream ext-workspace alone cannot express it.
- The overview is a separate client (`cosmic-workspaces`) consuming ext workspace + foreign
  handles + private extensions + capture for thumbnails (doc 30 §4.2) — the working existence
  proof for our shell-plane pager row.

## 4. niri: topology as policy (the divergence datapoint)

niri's model shares *no structure* with the three above yet speaks the same protocol: each
output has an independent, vertically ordered list of workspaces, and **each workspace is an
infinite horizontal strip of columns** (scrollable tiling). Unnamed workspaces are fully dynamic
(a trailing empty one always exists; empty middles vanish on switch-away); *named* workspaces
are declared in config, always exist, and stick to their configured output; numeric indices are
positional, explicitly not identity
([Workspaces wiki](https://github.com/niri-wm/niri/wiki/Workspaces),
[Named Workspaces](https://github.com/niri-wm/niri/wiki/Configuration:-Named-Workspaces)). niri
implements `ext-workspace-v1` ([PR #1800](https://github.com/YaLTeR/niri/pull/1800), also the
source of the multi-group client-interop caveats doc 30 §2.1 cites).

The load-bearing point: a compositor whose "workspace" is an unbounded 1-D content strip, whose
identity model is name-or-nothing, and whose lifecycle is GNOME-dynamic still exports a
conformant pager seam. **Workspace topology, lifecycle, and identity discipline are policy
choices compositors already diverge on**; a place model with metric anchors is not a bigger
divergence than niri already is — it only adds *attributes* no 2D compositor has (§8.2).

## 5. `ext-workspace-v1` mechanics: what a places model must expose (extends doc 30 §2.1)

Facts from the pinned XML
([`ext-workspace-v1.xml`](../../references/wayland-protocols/staging/ext-workspace/ext-workspace-v1.xml))
that constrain a places mapping, beyond doc 30's summary:

- **Groups have no identity.** A group handle carries only capabilities, `output_enter/leave`,
  and workspace membership — **no name, no id, no coordinates** (interface
  `ext_workspace_group_handle_v1`). Groups are distinguishable only by their output sets. If zxr
  mapped *rooms* to groups, an upstream-only pager could not label them; room identity would need
  the zxr extension or a virtual-output-per-room hack.
- **A workspace belongs to at most one group, and may be ungrouped**; the compositor may only
  `removed` an ungrouped workspace. `assign` (workspace→group) is an advisory client request —
  the wire already permits "move this place to another room" gestures from a pager.
- **`id` is the persistence hook**: sent at most once, stable and unique within a lifetime, and
  the spec instructs compositors to emit ids only for workspaces "likely stable across multiple
  sessions" and clients to **delete stored data for id-less workspaces**. This maps exactly onto
  anchored (persistent id = place id) vs transient (no id) places — the discipline COSMIC's
  pinned/unpinned split already exercises (§3).
- **Coordinates are uint32, N-dim, unique per group, orderless-by-default**: explicitly "no
  guarantee about the grid being filled or bounded," and an empty array withdraws geometric
  ordering. They can carry a pager *sort key* for places (e.g. 1-D room-ordinal) but, per doc 30
  §2.1's verdict, never metres — uint32 grid positions cannot encode transforms, and clients are
  told not to infer geometry from 1-D values.
- **State is three bits** (`active`, `urgent`, `hidden`); activation "may or may not deactivate
  all other workspaces in the same group" — cross-group concurrency is unstated (COSMIC keeps one
  active per output-group). `hidden` marks model objects a pager should not display — usable for
  places whose anchor is currently unresolved (§7) without destroying the handle.
- **Atomicity**: changes batch under manager `done`, requests under `commit`. A physical-walk
  "switch" (§6.5) arriving as one atomic active/hidden flip is protocol-clean.

What the protocol cannot say (zxr-extension territory, confirming doc 30 §5.1): toplevel
membership (§3), metric transform/bounds/anchor binding, place kind (anchored / portable /
head-relative), anchor resolution state, compositor-rendered previews.

## 6. Spatial and commercial precedents

### 6.1 visionOS: no workspaces; scenes restored per room

Apple's model has **no user-facing workspace/desktop object whatsoever**. Apps present scenes —
windows, volumes, and at most one open immersive Full Space (which hides other apps' content) —
in one shared coordinate space. Persistence is **scene restoration**: visionOS 26 lets users
snap windows/volumes/widgets to physical surfaces and *lock* them; locked scenes are keyed to
the **room** and restored when the user re-enters it, across doff/don and reboots
([WWDC25 "Set the scene"](https://developer.apple.com/videos/play/wwdc2025/290/),
[persistent-UI best practices](https://developer.apple.com/documentation/visionos/adopting-best-practices-for-scene-restoration)).
Apps opt scenes out (`restorationBehavior(.disabled)`, `defaultLaunchBehavior(.suppressed)`);
immersive spaces are never restored. This is the **degenerate case of places: grouping
cardinality one** — every window is its own "place," the room is the implicit group, and there
is no named set a user can summon, share, or switch to. The restoration half (system relaunches
the app, which recreates the scene) is exactly the restore-manager + `xdg-session-management`
split of doc 30 A2, shipped at OS scale.

### 6.2 Horizon OS: fixed home, docked panels, capped anchored windows

Quest's shell is a **fixed home** (passthrough home or the v81 Immersive Home) with a dock: up
to three docked panels plus three free-floating 2D windows, and "seamless multitasking" keeping
the menu + three windows available over immersive apps
([release notes](https://www.meta.com/help/quest/172903867975450/)). v81 added **persistent
window anchoring**: pin up to three windows per modality (passthrough and Immersive Home keep
*separate* pinned sets — six total, three visible), surviving reboots, snapping to detected walls
([UploadVR v81](https://www.uploadvr.com/quest-v81-new-immersive-home-window-anchoring-quickplay/)),
on the spatial-anchor service (world-locked frames, save/query across sessions, ~3 m coverage —
[anchors overview](https://developers.meta.com/horizon/documentation/unity/unity-spatial-anchors-overview/)).
Horizon Workrooms adds the **desk as an anchored workspace**: a calibrated virtual desk aligned
to the physical desk, with a persistent personal office re-entered each session
([Workrooms release notes](https://www.meta.com/en-gb/help/quest/3541815596055151/)). Reading:
one *implicit singleton place* (the home), hard small-N window caps, and per-modality set
duplication instead of a workspace abstraction — the anti-model whose limits (no named sets, no
second room, no set switching) are what a real place model exists to remove.

### 6.3 Android XR: a per-app mode switch, not workspaces

Android XR partitions by **app mode**: Home Space (multiple apps side by side as panels,
constrained bounds, mobile apps unmodified) vs Full Space (one app owns everything, spatial
panels/3D/environments; all other apps hidden), declared via `PROPERTY_XR_ACTIVITY_START_MODE`
and switched at runtime with `scene.requestHomeSpace()/requestFullSpace()`
([foundations](https://developer.android.com/design/ui/xr/guides/foundations),
[transition guide](https://developer.android.com/develop/xr/jetpack-xr-sdk/transition-home-space-to-full-space)).
The per-app `ActivitySpace` is the system-managed container, with a `keyEntity` continuity hint
preserving pose across mode transitions
([Scene API](https://developer.android.com/reference/kotlin/androidx/xr/scenecore/Scene)). Same
lesson as §6.1: shipping XR platforms chose *immersion level*, not workspace multiplicity, as
their primary mode — user-defined window sets are the unclaimed territory.

### 6.4 macOS: Spaces and Stage Manager (grouping-by-task on a desktop)

macOS runs **two grouping mechanisms simultaneously**: Spaces — linear per-display desktops
(with "Displays have separate Spaces"), per-app assignment rules (this desktop / all / display N
/ none), auto-switch-to-app's-space
([Spaces guide](https://support.apple.com/guide/mac-help/work-in-multiple-spaces-mh14112/14.0/mac/14.0)) —
and Stage Manager, which groups *windows into task sets* by direct manipulation, configured
**per Space**
([Stage Manager guide](https://support.apple.com/guide/mac-help/use-stage-manager-mchl534ba392/mac)).
Stage Manager is thus a sub-workspace grouping *within* the workspace axis — the closest desktop
analog to "a named set of windows" being the primary object rather than the container. Field
reports show the composition is fragile ("All Desktops" apps breaking auto-hide, arrangements
forgotten across sleep/reboot on multi-monitor —
[discussions](https://discussions.apple.com/thread/254322247)): two overlapping grouping systems
without one model underneath produce state users cannot predict — an argument for places being
**one** model with multiple presentations, not stacked mechanisms.

### 6.5 What breaks when "switch" can be a physical walk

Synthesizing the XR precedents against the desktop assumption set:

1. **Exclusivity fails.** Desktop switching is a zero-sum visibility flip (§1.1, §2). Two
   anchored places three metres apart are *both visible*; the user standing between them is "on"
   neither or both. visionOS/Horizon resolve this by having no exclusive container at all.
2. **Switching splits into three verbs** ([desktop-environment.md §5](../architecture/desktop-environment.md)):
   walk (compositor *observes* currency change), teleport/recenter (re-seat the reference
   frame), summon (the *place* moves — nearest desktop analog is niri moving a workspace between
   outputs, §4).
3. **The pager's projection problem:** places need compositor-rendered spatial previews (doc 30
   §5.1's preview-source extension) since a client cannot re-render a scene it cannot see.
4. **Boundedness inverts.** A desktop workspace is unbounded logical area; a room-anchored place
   has *metric* extent (Horizon's ~3 m anchor coverage, visionOS surface snapping). Overflow
   ("too many windows for the kitchen") is a policy question no desktop model has.
5. **Resolution state exists.** A desktop always "is"; an anchored place can be present-but-
   unresolved (ADR 0009's `LOCALIZED_TENTATIVE` ladder). The `hidden` state bit (§5) is the only
   upstream vocabulary for this.

## 7. Session persistence intersections

Extending doc 30 A2 (not repeating it): how each *workspace model* interacts with restore.

- **KWin:** desktops exist independently of sessions (config-persisted, §1.1); X11 session data
  stores each window's desktop set + activity list ([`sm.cpp`](../../references/kwin/src/sm.cpp)
  L164/L181), and the Wayland `xdg-session-management-v1` implementation stores window-manager
  state keyed by session+toplevel name (doc 30 A2.2). So restore *re-attaches windows to
  containers that never died*. The per-activity subsession variant died on cardinality
  (§1.2) — restore semantics were only coherent when a window belonged to exactly one restorable
  container.
- **GNOME:** dynamic workspaces make container persistence meaningless (indices, no ids), and
  GNOME has no session restore (doc 30 A2.2) — internally consistent: both halves ephemeral.
- **COSMIC:** persists container existence/identity (pinned workspaces, §3) but not contents —
  the halfway point. Its ext-workspace `id` discipline is the wire contract for that split.
- **Mura:** anchored places are the *strong* version of COSMIC's half — the place, its
  anchor binding, and each member window's transform survive reboot in compositor/mapping state
  (mapping M1 "shell pins windows" is the intra-session mechanism; M2 adds the store —
  [spatial-mapping.md §11](../architecture/spatial-mapping.md)); doc 30 A2.3 already fixes the
  record shape (`place_id`, anchor id/version, transform, bounds, presentation kind, fallback).
  What the model must additionally answer:
  - **Membership of windows the restore manager cannot relaunch.** A place may contain
    *delegated* toplevels (a KWin session's window dragged into space —
    [foreign-session-integration.md §3.7](../architecture/foreign-session-integration.md)
    R23/R24) and proxied (mode-4) windows: placement is ours, lifecycle is a foreign producer's
    or remote end's. Restore must degrade to "reserved slot until the producer re-exports" — a
    state no desktop restore path has (KWin assumes desktop-entry relaunch, doc 30 A2.2).
  - **Anchor-unresolved restore.** Hold windows hidden (§5's state bit) or materialize
    head-relative and re-seat on resolution (ADR 0009's corrections-move-anchors contract makes
    late re-seating legal).
  - **The cardinality warning applied:** if a window can be in N places (KWin-style sets),
    place-scoped restore inherits exactly the ambiguity that killed Activities subsessions
    (§1.2). Exclusive membership (mutter-style, §2) keeps restore well-defined.

## 8. Mapping to Mura

Evidence-to-options mapping only; the design decision belongs to a future design doc + ADR.

### 8.1 Candidate model shapes

- **(A) Single-axis anchored sets.** Place ≈ COSMIC pinned workspace; group axis = rooms/frames
  instead of outputs; membership exclusive-or-sticky (mutter shape) or sets (KWin shape).
  Evidence for: COSMIC proves the object model + protocol discipline (§3); visionOS proves
  room-keyed persistence UX (§6.1); Edmundson's cardinality argument favors exclusive
  membership (§1.2, §7); the room already supplies the context a second axis would add.
- **(B) Two-axis place × activity.** KWin's shape with places as the desktop analog. Evidence
  for: genuinely orthogonal needs exist — Horizon's separate passthrough-home vs immersive-home
  pinned sets (§6.2) are a shipped, if accidental, two-axis instance (same room, two contexts).
  Evidence against: the full matrix cost and the axis KDE is now shrinking (§1.2). A cheaper
  variant exists inside (A): several places bound to the *same anchor*, at most one summoned —
  context switching as place swap, no second axis in the model.
- **(C) Scrollable-in-place.** niri's lesson applied *inside* a place: a place's internal layout
  (free 3D, curved band, flat virtual-screen quad, docked flat layout) is per-place policy, not
  model structure (§4). This composes with (A)/(B) rather than competing — and ADR 0015 already
  treats the docked output as presentation policy over an unchanged model.
- **The degenerate baseline to keep:** visionOS-style per-window anchoring with no grouping
  (§6.1) falls out of any of these as "place of one window" and is what mapping M1 ships first
  ("shell pins windows", no workspace semantics — the registry gap's own wording).

### 8.2 What the pager/overview consumes

Per doc 30 §5.1 (standard-seam verdict) plus this document's additions:

- **Base:** `ext-workspace-v1` — place → `ext_workspace_handle_v1`; anchored/persistent places
  emit `id`, transient ones do not (§5's discipline); `name` = user's place name; `active` /
  `hidden` per open question 2; coordinates at most a 1-D sort key.
- **Group mapping is genuinely open** (open question 3): one global group (degenerate but legal);
  group-per-room (natural, but groups carry no identity upstream — §5 — so room labels need the
  zxr extension anyway); or group-per-reference-frame (world / head / hand), which would make
  `assign` express "make this place portable."
- **zxr extension carries** (COSMIC workspace-v2 as the shape precedent, §3): place kind, anchor
  id + resolution state, metric transform/bounds, preview source (compositor-rendered, capture
  stack), pin/rename/reorder-class operations if not upstreamed by then.
- **Toplevel membership needs the cosmic-info extension shape** (§3) on our foreign-toplevel
  handles — without it the overview cannot draw windows-in-places.
- KWin's realtime `currentChanging` offset (§1.1) is the precedent for exposing *partial*
  transitions (a walk is a continuous switch) if the pager should animate them.

### 8.3 What session restore needs from the model

From §7: stable place ids that survive the compositor (ext-workspace `id` = the restore record's
`place_id`, doc 30 A2.3); membership cardinality fixed *before* restore is built (the Activities
post-mortem); a defined unresolved-anchor restore state; and slot semantics for delegated/proxied
members the restore manager cannot relaunch (R23/R24 windows re-appear only when their producer
re-exports them). The restore manager (service plane, registry §6) binds `xdg-session-management`
session ids to place ids — a toplevel's place membership must be recordable at `add_toplevel`
time and applied at `restore_toplevel` time within the initial configure (KWin's
reject-after-first-commit rule, doc 30 A2.2, applies unchanged).

### 8.4 Docked mode and mode-5 join

- **Docked (ADR 0015):** the docked window-set is a *presentation* of model state, never model
  ownership — the flat-layout slot is naturally **per-place** (each place may define its docked
  flat arrangement, the way COSMIC keeps per-workspace tiling state and niri per-workspace
  layout blocks, §3/§4). Whether the docked output appears as an ext-workspace *group* (so
  pagers can move places onto the monitor with `assign`) is a free protocol choice the group
  mapping (8.2) must answer consistently.
- **Mode-5 join ([spatial-sharing.md §5](../architecture/spatial-sharing.md)):** the join unit is
  a place; the placement graph mode 5 replicates is the place's membership + transforms — the
  model *is* the replication schema, arguing for a state-sync-shaped internal representation
  (registry §9's spin-out note). Joining maps the host place's anchor frame into the visitor's
  local frame; private windows are absent members (redaction = membership filtering). A joined
  remote place also needs a pager representation — plausibly a workspace in a "shared" group —
  so the group question (8.2) recurs.

### 8.5 The open questions the design doc must answer

*Status (2026-09-23): all five answered in [ADR 0016](../architecture/adr/0016-places-model.md) /
[places-model.md](../architecture/places-model.md) — exclusive+overlay cardinality; decomposed
currency (no single active bit); groups = reference frames; transient+pin-to-persist lifecycle
with entry policy; no second axis.*

1. **Membership cardinality.** Exactly-one-place (mutter), set-of-places (KWin), or
   one-place-plus-sticky (COSMIC)? Restore and mode-5 both get simpler with exclusivity (§7);
   follow-me/head-relative windows need a sticky escape hatch either way. What is the XR meaning
   of "on all places"?
2. **What is "active" when switching can be a walk?** One active place per… what? (KWin: per
   output; COSMIC: per group.) Is currency derived from head pose, focus, or explicit selection —
   and does a walk emit activation events, partial-transition offsets (§1.1), or nothing? This
   decides the pager UX and the ext-workspace `active` bit's honesty.
3. **The group axis.** Rooms→groups, frames→groups, or one flat group — given upstream groups
   are identity-less (§5) and both docked mode and mode-5 want a group-shaped answer (8.4).
4. **Lifecycle policy.** GNOME-dynamic (places appear when windows are placed, vanish when
   empty), COSMIC-pinned (explicit persistence upgrade), or always-explicit creation? Coupled:
   does an *empty* anchored place persist (a named kitchen layout with no windows), and who owns
   the trailing-empty affordance the dynamic models rely on (§2, §4)?
5. **Second axis or not.** Does any context separation remain that neither distinct places nor
   same-anchor place-swap (§8.1's B-variant) expresses — per-user contexts on a shared appliance,
   work/personal partitions of one desk (Horizon's two pinned sets, §6.2)? If yes, is it an axis
   (KWin), a mode (Android XR §6.3), or just more places?

## 9. Source index

Local clones: [kwin](../../references/kwin/src/), [mutter](../../references/mutter/src/core/),
[gnome-shell](../../references/gnome-shell/js/ui/), [cosmic-comp](../../references/cosmic-comp/src/),
[cosmic-protocols](../../references/cosmic-protocols/unstable/),
[wayland-protocols](../../references/wayland-protocols/staging/) — specific files cited inline.
Web: niri wiki/PR #1800; Apple WWDC25 + visionOS docs; Meta Quest release notes, UploadVR v81,
anchors + Workrooms docs; Android XR developer docs; Apple macOS guides; KDE Planet (Edmundson
2025-09-19) and KDE docs — all linked inline above.
