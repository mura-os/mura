# 64 — Window and workspace management: how users manage windows in XR and on the desktop, from comparables

**Research date:** 2026-09-26. **Question:** ADR 0012's amendment (c) makes window-management
policy in-process by default with a bounded `zxr_window_management` protocol for anyone who
wants their own manager. That names the *seam*; it does not say what the manager *does* for
the wearer — where windows appear and how big, how they are arranged, what closing and hiding
mean, how sets of windows are grouped and switched, what follows the head and what stays put,
what happens when there are too many, what an "environment" or a 3D scene is to the manager, and
which of these verbs an external manager may issue. This document reads how every comparable
answered those questions and why, so that
[window-workspace-management.md](../architecture/window-workspace-management.md) and the draft
protocol are derived, not invented. **Boundary:** the *gestures* by which a user moves, resizes,
targets and commits — grab mechanics, hover, cursor, peripherals, focus/activation rules — belong
to the input workstream (research/63); this document consumes them as given and owns what a
movement *means for the arrangement*.
**Already covered, linked not redone:** workspace *models* and `ext-workspace` mechanics
([research/34](34-workspace-models.md); its five open questions are answered by ADR 0016 and
[places-model.md](../architecture/places-model.md)); initial-placement, launcher, notification,
keyboard and recenter *patterns* ([research/36](36-vr-shell-interaction-patterns.md) §2–§8);
kwin-vr's detach/reattach field evidence ([research/31](31-kwin-vr.md) §2.9–§2.12); the scene
data model ([research/62](62-scene-data-model-from-comparables.md); spec §5a).
**Method (AGENTS.md rules 2, 7, 8):** lineage first — motorcar's `WindowManager` and wxrc's map
handler — then comparables *with their reasons*: what each chose, why (HIG rationale, module
docs, kcfg/gschema defaults, commit messages), its assumptions (eye tracking? controllers?
seated? one user? one app?), whether they transfer to a device-contract-tiered, room-scale,
multi-user, Free-Software Mura, and the trade-off. **visionOS is studied in depth as
engineering evidence** (rule 2): mechanisms, numbers and Apple's own stated reasons, never
"Apple does X" as policy; every behaviour the design adopts from it is justified on engineering
grounds in §14. Evidence classes as research/36 §1: **A** pinned clone read directly, **B**
vendor first-party documentation, **C** reputable press, **D** community (corroboration only).
Papers are pinned in `references/PAPERS.json` (topic `xr-windowing`). **Budget impact:** a
research document; the design it feeds charges the `policy` module (spec §3) and one optional
external client.

## 0. Subjects

| Subject | What it is | Source | Class |
|---|---|---|---|
| motorcar / wxrc | the lineage: scene-graph WM; wlroots prototype | `references/motorcar`, `references/wxrc` | A |
| kwin-vr | KWin's in-process 3D mode, daily-driven by its author | `references/kwin-vr/src/plugins/vr/`, research/31 | A |
| WayVR (wlx-overlay-s) | OpenXR overlay + Wayland app runner | `references/wlx-overlay-s/wayvr/`, `wlx-common/` | A |
| xrdesktop | Collabora's desktop-mirror library, 2019–21 | `references/xrdesktop/` | A |
| Simula | Godot/Haskell VR window manager, keyboard-first, motorcar fork | `references/simula/` | A |
| StardustXR + Flatland | spatial display server + its 2D-panel manager *client* | `references/stardustxr-server`, `references/flatland` | A |
| Breezy Desktop | AR-glasses virtual monitors for GNOME/KDE | `references/breezy-desktop/` | A |
| WiVRn lobby | streaming runtime's in-headset GUI | `references/wivrn/client/` | A |
| KWin, mutter/gnome-shell, cosmic-comp, niri, Hyprland, river, PaperWM | the 2D desktops' arrangement, lifecycle and pluggability | pinned clones | A |
| Apple visionOS | consumer standalone, gaze+pinch-first | HIG, developer docs, WWDC transcripts, support pages | B (C where Apple is silent) |
| Meta Horizon OS (Quest 3, v67–OS 2.1) | consumer standalone | Meta help/developer docs | B (C flagged) |
| Google/Samsung Android XR | consumer standalone | Android XR design/dev docs, Samsung support | B (C flagged) |
| HoloLens 2 / Windows Mixed Reality | archived first-party docs | learn.microsoft.com | B |
| macOS/iPadOS Stage Manager, Windows 11 Snap, GNOME design history | 2D "group by task" and "snap" | first-party docs; Federighi interviews flagged C | B/C |
| Papers | Ethereal Planes; Personal Cockpit; Lindlbauer 2019; SemanticAdapt; Biener 2022; McGill 2020; Pavanatto 2021; Feiner 1993; Task Gallery | `references/papers/xr-windowing/`, PAPERS.json | [external] |

**Gaps found and not papered over:** no vendor publishes a hard window cap for visionOS or
Android XR, nor Apple any FOV comfort zone in degrees, nor any thermal/performance rationale for
placement limits; Meta does not document how v81's "12 apps" split docked/detached, nor Travel
Mode's anchoring mechanism; Microsoft never published the shell's Follow-me parameters. Where a
number is needed and absent, the row says so.

## 1. Multitasking shape — shared space, exclusive space, caps

**Lineage.** motorcar: one scene, every surface a node, no notion of exclusivity; a 3D client
gets a clipped volume (`CUBOID`) or an unbounded `PORTAL` (`waylandsurface.h:53-57`) but never
hides the others. wxrc: same, one list.

**Comparables.** *visionOS*: two modes with names — the **Shared Space** "where multiple apps
can run side-by-side and people can open, close, and relocate windows" and a **Full Space**
"where it's the only app running" ([HIG designing-for-visionos]). Opening an `ImmersiveSpace`
hides every *other* app's windows and restores them on dismiss; the opening app's own windows
stay ([presenting-windows-and-spaces]). Exactly one immersive space may exist system-wide
("If you try to open a space when one is already open, the system logs a runtime error"). Three
immersion styles: `mixed` (depth-sorted against windows), `progressive` (a radial portal the
user sizes with the Crown, default "around half of a person's field of view", 120°–360°; windows
*always* render in front — "This helps people avoid losing track of windows behind virtual
content when passthrough is off"), `full`. **No numeric window cap**; the only limit is
comfort guidance: "Avoid displaying too many windows. Too many windows can obscure people's
surroundings, making them feel overwhelmed, constricted, and even uncomfortable. It can also make
it cumbersome for people to relocate an app because it means moving a lot of windows" ([HIG
spatial-layout]). Apple's stated reason for defaulting to the Shared Space: "gives people more
control, letting them choose when to increase immersion" ([HIG immersive-experiences]).
*Horizon OS*: **three windows attached to the Navigator plus three detached** (six); a fourth
launch minimizes one docked window ([Meta 542427545314119]); v81 raised concurrency to "up to
12 apps at once" without documenting the split (C: UploadVR measured 3+3 still); since v69–v74
"up to three windows open during immersive experiences" ("seamless multitasking"). *Android
XR*: **Home Space** ("Multiple apps run side by side… Any compatible mobile or large screen
Android app can operate in Home Space with no additional development") vs **Full Space** ("One
app runs at a time, with no space boundaries. All other apps are hidden"); the app requests the
switch (`requestFullSpace()`), and the design guidance is to "Add clear visual cues to let users
quickly switch" ([Android XR foundations]); no published Home Space cap. *HoloLens 2*: "Up to
three app windows can be active in mixed reality home at a time. You can open more, but only
three will remain active" (inactive ones show darkened content); "when you open a new immersive
app, all other running apps will immediately become inactive" ([holographic-home]) — the cap is
an *activity* cap with a resource reason. *2D desktops*: no exclusivity beyond per-output
fullscreen; Stage Manager's **four** windows per stage is Apple's UX argument that four is the
number at which the system can "automatically nudge them in ways that make sure that they all
stay accessible" (C: Federighi, Forbes 2022) plus a responsiveness floor (C: Ars 2025 — "It is a
foundational requirement that if you touch the screen and start to move something, that it
responds"). GNOME's measured baseline: "most people had around 8 open windows… most people were
only using a single workspace" ([GNOME 40 UX research]).

**Transfer.** Two mechanisms are being mixed by the consumer platforms and must be separated
for Mura. (i) *Exclusivity*: a 3D/immersive scene that hides other apps. Mura's layer model
already has the slot — the **environment** layer (spec §4) is what an immersive scene takes
over, and the window tiers remain (visionOS's own rule: windows stay visible in front of
immersive content). Whether an app may *request* exclusivity is a policy question the platforms
answer differently (visionOS: yes, one at a time, user can always reduce immersion with the
Crown; Android XR: yes via `requestFullSpace`; Linux: fullscreen is per output, never hides other
outputs). (ii) *Caps*: HoloLens's three-active and Stage Manager's four are **resource/activity
budgets** expressed as user-visible limits; Horizon's six is a layout-slot count. AGENTS.md rule
3 forbids importing caps as policy; the *engineering content* transfers as a compositor
budget — off-view windows get throttled frame callbacks (spec §6.6 already sends callbacks per
refresh only to mapped windows; M1 can stop them for planes outside the view frustum) — never as
a count the user hits. **Trade-off.** No cap means clutter is a *management* problem (§8), not a
prevention; that is where every no-cap platform (visionOS, Android XR's Tidy) puts it.

## 2. Initial placement and sizing

**Lineage.** motorcar: toplevel *n* at translate(0,0,1)·rotate((n−1)·−30°)·translate(0,0,−1.5),
i.e. a fan on a 1.5 m arc; popups/transients as children of the surface under the pointer at
+0.05 m (`windowmanager.cpp:147-197`); pixels at a hard-coded 8 px/cm
(`waylandsurfacenode.cpp:237-241`). wxrc: (0,0,−2) rotated into the first view
(`xdg-shell.c:75-95`).

**Comparables — where.** research/36 §2 already recorded the convergence "head-relative spawn at
a declared distance, facing the user; second window adjacent". New evidence sharpens it:
*visionOS* — "the system places the app's window where they're looking… about two meters in
front of the wearer, giving it an apparent width of about three meters"; a running app's new
window goes "in front of one of the app's existing windows, offsetting each additional window by
a small amount to avoid fully obscuring existing windows" ([positioning-and-sizing-windows]);
placement is head-relative "regardless of the person's height or whether they're sitting,
standing, or lying down" ([HIG designing-for-visionos]); **apps cannot position windows** — the
first window's `defaultWindowPlacement` is ignored, later ones may only be placed *relative* to
existing ones (`.leading/.trailing`, `.utilityPanel` "generally within direct touch range",
`.replacing(_:)`), and "you can't directly manipulate window position or size after the window
appears. This ensures that people have full control over their workspace". *Android XR*: 1.75 m,
"vertical center 5° below a user's eye level… as users tend to look downward"; comfort zone "the
center 41° of a user's field of view" ([Android XR spatial-ui]). *Horizon*: "roughly 1 meter
distance slightly below the user's line of sight"; 45 cm for direct-hand apps, 1 m for indirect,
70 cm when both ([Meta mr-design-guideline]). *HoloLens*: gaze-placed at a fixed distance,
"automatically adjust (in size and position) to conform to the space where you place it"; optical
focus 2 m, comfort zone 1.25–5 m, "never… closer than 40 cm" ([MR comfort]). *WiVRn*: 0.5 m,
0.1 m below eye line. *Breezy*: 1.05 m. *kwin-vr*: distance 100 cm, screens placed by
`SpaceAllocator3D` (angular free-slot search; spacing 0.05 rad default, 0.1 in QML) then
`turnToFaceKeepRoll`; detached windows keep the pose they were grabbed at. *WayVR*: −0.95 m;
a second window of the same process spawns as `Parent`, otherwise `Spread` — 0.08 m left, 0.08 m
down, 0.06 m closer than the most recent panel (`window.rs:335-350`). *Flatland*: 0.25 m when
launched "without a sense of space", else at the launcher's pose rotated to face the user.
*Simula*: `setInFrontOfUser` then a per-app slot (`center|left|right|top|bottom`) from launch
token / window class. *Personal Cockpit*: body-fixed at 50 cm from the right shoulder because a
~40° FoV makes fixed layouts the memorable ones. *2D*: KWin's default is **Centered** (with
decorations) cascading by area/48 if it would fully cover another window (`placement.cpp:338-355,
526-574`); mutter centres dialogs over their parent with "twice as much space below as on top",
else first-fit then cascade at 50 px/15 px fuzz, and auto-maximizes anything over 80 % of the
work area (`place.c:840-1106`); cosmic floats at ≤ ⅔ of the output, cascading 48 px; niri opens
into the scrolling column next to the active one with `ActivateWindow::Smart`.

**Comparables — how big.** *visionOS*: default **1280×720 pt**; a point is *an angle*, so the
size is angular; `windowResizability` = `automatic` → `contentMinSize` for windows,
`contentSize` for Settings and volumes; volumes in metres, fixed scale; the user picks a global
default size (Small–Extra Large) in Settings ([Apple 118515]). *Horizon*: 1024×640 dp default,
min 384×500, max 1440×1000 ([Meta styles_layouts]); v81 **Rescale** — "continuously enlarge
applications without increasing the pixel allocation… Rescale now takes over from application
resize constraints" — and **Ratio Locking**. *Android XR*: 1024×720 dp default, min 385×595,
max 2560×1800 (Home Space); Full Space no minimum ([foundations], [spatial-ui]). *HoloLens*: fixed
1280×720 @150 % ("853×480 effective pixels") because "TV-like viewing distances are recommended to
produce the best readability" and it "matches the fixed DPI and effective pixels for UWP apps
running on Xbox One" ([building-2d-apps]). *kwin-vr*: ppu **20 px/cm** (physical = logical ÷ ppu;
resizing changes pixels, physical size follows). *WayVR*: `PIXELS_TO_METERS = 1/2000` — also
20 px/cm — with `RELATIVE_SIZE = 1440` scaling. *Flatland*: 0.21×0.16 m at 3000 px/m (30 px/cm).
*Simula*: 900×900 px default; "zoom" changes pixels *and* scale while height ∈ (500, 1500).

**Transfer.** (1) Placement: the head-relative spawn at a contract-declared distance is
research/36's determination; the new material adds two rules with reasons that transfer —
**vertical bias below the eye line** (Android XR 5°; Horizon "slightly below"; Microsoft's
resting gaze 10–20° below; Apple "along a natural line of sight… slightly below") and **the
second window of the same app goes in front of/adjacent to the first, offset, never on top**
(visionOS, WayVR `Spread`, KWin/mutter cascade). (2) **Apps do not place their own windows.**
visionOS forbids it outright and gives its reason; Linux desktops have refused client-positioned
toplevels since xdg-shell was written (no position request exists); mutter honours only
X11-era `USER_POSITION`. Converging with reasons that transfer — recorded as a determination in
§14 (it is also ADR 0012 §2's existing stance). The relative-placement escape valve
(visionOS `defaultWindowPlacement` relative to an *existing* window; motorcar's popup parenting)
is what a *place* provides in Mura. (3) Sizing: two camps — **angular** (visionOS points, Horizon
Rescale, Android XR beyond 1.75 m) and **fixed physical density** (kwin-vr, WayVR, Flatland,
HoloLens's fixed resolution). §4 takes this up. The 20 px/cm two independent Linux shells chose
(kwin-vr, WayVR) is a stand-in candidate for the device contract's default; the spec's R0 value
(8.3 px/cm) is far coarser and should not survive M1. **Trade-off.** Forbidding app placement
costs the visionOS-style *relative* escape valve, which the places model supplies as membership,
not as coordinates.

## 3. Arrangement models — what a place's layout engine can be

**Lineage.** motorcar's fan; nothing else. **Comparables.** Six shapes exist:

| engine | who | evidence | why they chose it |
|---|---|---|---|
| **free** — user-placed, offset spawn, no auto-arrangement | visionOS; Android XR Home Space; Horizon detached windows; xrdesktop; Flatland; kwin-vr's `vr` state | [positioning-and-sizing-windows]; [Android XR foundations]; `xrd-shell.c:227-233`; `XrScene.qml:398-432` | "people have full control over their workspace" (Apple); Android XR adds a one-shot **Tidy** for the 3 most recent ([Google 16638859]) |
| **angular slots** — free-position search on a sphere/capsule | kwin-vr `SpaceAllocator3D` ("projected onto a sphere… checks for overlap in angular space (azimuth/elevation) to prevent occlusion"); zen's seat capsule (`zns/include/zns/bounded.h:18-24`); xrdesktop `arrange_sphere`; research/36's selected slot search | `spaceallocator3d.h:25-29` | occlusion-free placement without a grid |
| **dock / hinged group** — N slots attached to a bar, angled toward the user | Horizon's 3-window Navigator dock and "hinged panels… connected at their left and/or right edges"; HoloLens Start-menu tag-along; iPadOS/macOS Stage Manager's centre group + recents strip | [Meta panels]; [Apple Stage Manager] | Meta: "the display bar allows groups of windows… to be repositioned… to accommodate egocentric movement, user handedness"; Apple: keep every window in the group "accessible… at the same time" |
| **virtual monitor / curved band** | Breezy (`monitor-wrapping-scheme` horizontal/vertical/flat, `curved-display`); Mac Virtual Display (16:9 / 21:9 / 32:9, "curved, and can wrap around your workspace"); Horizon theater view (flat/curved toggle); Biener's study setup | `virtualdisplaysactor.js:232-370`; [Apple 118521] | compatibility with a 2D session; reading comfort |
| **tiling / scrollable** | niri columns (presets ⅓ ½ ⅔), cosmic's tiling tree (split by aspect), Hyprland dwindle/master, PaperWM | `layout/mod.rs`, `tiling/mod.rs:563-612` | niri: "Actions should apply immediately… keeping the code sane"; 2D screen real estate |
| **surface-snapped** | visionOS 26 (windows to vertical surfaces, volumes to horizontal — "The bottom of volumes may snap to horizontal surfaces and the back of windows may snap to vertical surfaces"); Horizon OS 2.1 wall snapping ("You can always override this"); Android XR anchoring "to the floor, chair, wall, ceiling, or table" in passthrough | [surfacesnappinginfo]; [Meta 172903867975450]; [Android XR spatial-ui] | Apple: "lets people attach apps to specific places in their real world" |

**Transfer.** The places model already makes this a **layer-3 choice per place**
("free 3D, curved band, screen quad, docked flat" — places-model §1). The evidence settles what
the *shipped* engines should be and what the *default* is: every consumer XR platform defaults
to **free placement with an offset spawn and a one-shot tidy/arrange** and offers a dock/group
as a second structure; angular-slot allocation is what the Linux shells and research/36 chose
for automatic placement *within* free; virtual monitor is the compatibility artifact
(foreign-session mode 3); tiling is a 2D screen-real-estate answer whose reason ("screen") does
not exist in a room, but PaperWM-style scrollable strips *do* recur as a spatial band. The
**default engine for a fresh place** is a fork for the owner (§15 Q2). **Trade-off.** Free +
tidy leaves arrangement to the user (visionOS's explicit stance); slots/docks give predictability
at the cost of the user's freedom, which is why every platform that has a dock also lets windows
be detached from it.

## 4. Movement semantics on the arrangement side — depth, scale, facing, snapping, overlap

**Lineage.** motorcar: none beyond node transforms. **Comparables.** *Depth ↔ scale.* visionOS
**dynamic scale**: "visionOS automatically increases a window's scale as it moves away from the
wearer and decreases it as the window moves closer, making the window appear to maintain the
same size at all distances… visionOS defines a point as an angle"; volumes are fixed-scale
"because they are most commonly used for 3D content which is meant to behave with greater
physical accuracy"; the input reason: "Dynamic scale keeps your UI and the target areas at the
same size, while fixed scale… makes the target areas too small" ([HIG spatial-layout];
[defaultWorldScaling]; WWDC23 10073). Android XR: "The size stays consistent between 0.75 meters
and 1.75 meters. Then the scaling rate grows at 0.5 meters per meter"; move limits 0.75–5 m "To
avoid conflicts with the system UI" ([spatial-ui]). Horizon: Rescale (v81) and OS 2.1 "Windows
now smoothly rescale in real time as you move them closer or further away"; Meta's MR guideline
recommends "angular scaling… to keep the legibility and target size". kwin-vr, WayVR, Flatland:
fixed density; HoloLens: fixed resolution, user scales the slate. *Facing.* visionOS: windows
"always turn to face someone when moving… When they release, the window stays in its final
position" (WWDC23 10072); volumes' bar and close button "shift position to face the viewer";
HoloLens: "the app window automatically turns to face you as it moves"; kwin-vr:
`turnToFaceKeepRoll` for screens; Simula: `orientWindowTowardsGaze` on demand; Microsoft's
general term is **billboarding**, "can also be constrained to a single axis". *Snapping.* 2D:
KWin border/window/centre snap zones 10/10/0 px, `SnapOnlyWhenOverlapping=false`
(`kwin.kcfg:158-169`); quick-tile at the 20 px edge zone; Windows 11 Snap layouts with "guided
snap assist" and linked resize. XR: surface snapping (§3) is the spatial successor; no XR platform
snaps window-to-window. *Overlap and collision.* visionOS allows overlap, shows "what might be
behind a window" through glass, makes the non-active window "more translucent and appears to
recede along the z-axis", lets a window pass through furniture while moving ("With windows, you
don't need to worry about how they fit into someone's space since the system handles this");
kwin-vr avoids occlusion at *allocation* only; Horizon's minimum transient spacing 4 cm
(`kwinvr.kcfg:111-115` is kwin-vr's equivalent). *Client-initiated move/resize.* Under xdg-shell
these are `xdg_toplevel.move/resize` requests with a serial; every 2D compositor turns them into
its own grab; kwin-vr and WayVR ignore them in 3D (movement is the bar); river re-emits them to
the WM as `pointer_move_requested`/`pointer_resize_requested` events
(`river-window-management-v1.xml`, window events).

**Transfer.** (1) **Dynamic (angular) scale for planes, fixed scale for volumes** is the
position of all three consumer platforms, each with an engineering reason (legibility and target
size are angular quantities; a 3D object is a physical one), and it is what a point-as-angle
unit system implies. The Linux shells' fixed density is the *absence* of a policy, not a
position (kwin-vr's ppu is a single global). Recorded as a determination in §14 with the
justification stated, since it is imported from consumer platforms. Android XR's *piecewise*
rule (constant to 1.75 m, then 0.5 m/m) is one platform's tuning and stays a stand-in until the
device contract's panel numbers say what the eye resolves. (2) **Turn-to-face while moving,
hold on release** — visionOS, HoloLens, kwin-vr converge; motorcar had no facing rule. (3)
**Surface snapping** is the spatial snap; window-to-window snap zones do not transfer (no shared
plane to snap in). (4) **Overlap allowed, non-active windows recede** — visionOS's mechanism; the
"recede" is presentation, not arrangement, and belongs to the shell's plane-emphasis (input
workstream). (5) Client `move/resize` requests become the *same* operations the bar performs;
river's re-emission to the WM is the model for the external seam. **Trade-off.** Dynamic scale
makes "how big is this window" a function of distance the user cannot read off directly; Apple
accepts this for UI and refuses it for volumes; Mura's two shapes map one-to-one.

## 5. Lifecycle — close, hide, minimize, restore

**Lineage.** motorcar: map/unmap; wxrc: `mapped` flag. **Comparables.** *visionOS*: **no
minimize** exists anywhere in its window vocabulary ("close, move, and resize"; visionOS 26 adds
"lock"); "Closing an app window in the Shared Space transitions the app to the background
without quitting it" ([HIG multitasking]); "Hide other apps" is defined by Apple as closing them;
force quit is a chord. Programmatic dismiss of the last scene is ignored. Relaunch restores
"windows to their previous position and size"; `defaultLaunchBehavior(.suppressed)` for secondary
scenes "to avoid getting people stuck in an unexpected state"; immersive spaces "are not
restored". *Horizon*: minimize **retains state**, minimized apps sit on the Navigator with "a
white dot underneath the app icon"; the fourth docked launch auto-minimizes one. *Android XR*: no
per-window minimize documented; close via Recents. *HoloLens*: tiles persist as launchers;
"Suspended apps leave a screenshot of the app's last state on its app tile"; removing a tile
closes the process. *2D*: KWin's `isShown = !deleted && !hidden && !hiddenByShowDesktop &&
!minimized` and `activateNextWindow` on close (`window.cpp:4449-4451`, `activation.cpp:432-486`);
GNOME minimizes toward the dash icon and treats the closed-last-dialog case with a 1 s grace
before removing the workspace ("we assume that it might be an initial window shown before the
main window", `windowManager.js:173-179`); cosmic pushes to a `minimized_windows` list with a
panel-icon animation target; **niri makes `set_minimized` a no-op** (`foreign_toplevel.rs:574-575`)
— its lifecycle is scrolling columns + overview; Hyprland's Wayland backend `setMinimized` is
empty; **river makes minimize an event to the WM** ("minimize_requested") and defines hide/show
as *rendering* state the WM controls (`river-window-management-v1.xml:476-500`).

**Transfer.** The 2D three-state model (shown / minimized / hidden) exists to reclaim *screen*;
the platforms built for a room either dropped minimize (visionOS, Android XR, niri, river's
compositor) or kept it as "put it on the dock with its state" (Horizon, GNOME's dash). Both
positions are defensible in a room; what is *not* in question is that the compositor must not
own the semantics — river's split (minimize = event; hide/show = manager-controlled render
state) is exactly ADR 0012 (c)'s shape and transfers as the protocol's form regardless of the
default policy. **Fork for the owner (§15 Q1)** with the comparables' positions. Restore —
"windows come back where they were" — is universal (visionOS, HoloLens, Windows re-dock, GNOME
session) and is already the places model's §7; visionOS 26's per-room restore of *locked* scenes
is the same as pinned places on map anchors. **Trade-off.** No-minimize is simpler and matches
"close = background" only if apps relaunch fast and into place; a dock-with-state costs a shell
component (the launcher/dock client of ADR 0012) a state indicator.

## 6. Grouping and switching — places, rooms, docks, overviews

**Lineage.** none. **Comparables.** research/34 §6 recorded the model differences (visionOS: no
workspaces, scenes per room; Horizon: fixed home + docked panels; Android XR: per-app mode; macOS
Spaces/Stage Manager). The *interaction* of switching: *visionOS* — switching is by **gaze**
("When people look from one window to another, the window they're currently looking at becomes
active") and by the **Home View** (Crown press or palm tap); there is **no app switcher**
(absent from every Apple control inventory); Control Center is search + toggles. *Horizon* —
the **Navigator** (v77→default): "brings your apps, friends, controls, and more together in one
place", double-tap hides/shows all windows; OS 2.1: "essential information… remains visible
within your field of view so that you aren't distracted from your current task". *Android XR* —
launcher bar with **Tidy**, **Recents** ("Users can open, close, and switch apps"), Home. *HoloLens*
— Start menu (tag-along), tiles placed in the world as launchers, "Snap to app" teleport to the
ideal viewing spot for a window. *macOS Stage Manager* — one group centre-stage, recents strip;
drag between to (un)group; per-app "All at Once / One at a Time". *GNOME* — the overview "is a
facilitator and a mediator, not a destination"; dynamic workspaces with a trailing empty one and
`MIN_NUM_WORKSPACES = 2`; the GNOME 40 study found "most people were only using a single
workspace" and "New users could easily understand workspaces as 'screens'". *WayVR* — **sets**:
switch stores/restores per-window `active_state`. *Simula* — 10 numbered workspaces + a
persistent one, Super+1–8 to switch, Super+Shift+0 "Pin window to all workspaces". *kwin-vr* —
KWin's virtual desktops unchanged behind screen mirrors; grab-all moves the cluster.

**Transfer.** The places model (ADR 0016) already fixed the *model*: places on frames, no second
axis, transient-by-default. The *interaction* evidence adds: (1) **gaze/attention is the primary
switch** on every gaze-capable platform and "look at it" is the Mura input floor's attention
signal too — so *currency* (places-model §4) is the switch, and an overview/pager is the
secondary, explicit switch — GNOME's "facilitator, not a destination" framing transfers word for
word to a pager client. (2) A **Home/launcher surface** that also lists open windows (Horizon
Navigator, Android XR Recents, macOS recents strip) is the shape three platforms converged on;
in Mura that is the launcher/dock client over `foreign-toplevel-list` + `ext-workspace`
(research/60 §8, ADR 0012), and the WM design only has to guarantee the model it reads. (3)
Switching **by walking** (research/34 §6.5) has one shipping precedent now: visionOS 26's
per-room locked scenes ("When moving between rooms, locked content from the previous room fades
out, and locked content in the new room appears") — pinned places on map anchors behave the same
way for free. **Trade-off.** No explicit switcher (visionOS) works because gaze *is* the switcher
and windows are few; Mura's device tiers without eye tracking need the explicit pager, which the
design keeps as a client.

## 7. Follow, tether, recenter — as layer-2 policy

**Lineage.** motorcar: none (fixed world). **Comparables.** *visionOS*: windows are world-fixed;
head-anchoring is discouraged in the strongest HIG language ("can make them feel stuck, confined,
and uncomfortable… Instead, anchor content in people's space"); WWDC23 10078 names the
alternative "a lazy-follow animation"; recenter = Crown long-press, moves "apps, Environments,
and interactive experiences" but **not locked content**; "Your app doesn't need to provide a
special way to bring back windows or reset the scene". *Horizon*: v77 "windows that follow you
around… removes the need for you to constantly reset your view… you can only move with one window
at a time"; v81 pin-to-world; recenter = hold the button. *HoloLens*: per-window **Follow me**
toggle next to Close; windows opened *inside* an immersive app open in Follow me automatically;
the tag-along definition — "attempts to stay in a range that allows the user to interact
comfortably… content attempts to stay within the user's periphery by sliding towards the edge of
the view… 'a glance away'"; parameters (MRTK solvers) `MaxViewDegrees`, `MinDistance`/
`MaxDistance`, `MoveLerpTime`; warning "can prove overwhelming or nauseating if they move wildly
or spring too much". *kwin-vr*: follow **on by default** for the whole cluster
(`followEnabled=true`, FOV 40°/20° start, 4° stop, 0.5 s delay, speed 2.0 — `kwinvr.kcfg:37-68`;
class comment: "Rotates and moves the rotationTarget if no tracked objects are inside FOV to bring
the closest tracked object into the center of the camera's view"); research/31 §2.10 flags its
uncapped slerp. *Breezy*: `follow-threshold` 15°, 1000 ms slerp, per-display focus hysteresis.
*WayVR*: `Positioning::{Floating (recenters relative to HMD), Anchored, Static (no recentering),
FollowHead, FollowHand}` with `lerp`; grab pauses following. *Lindlbauer 2019*: world-anchored
by default, **view-anchored fallback only when the world-anchored element is out of view or
occluded**, transitioning back when the user turns toward it. *Personal Cockpit*: body-fixed
(50 cm off the shoulder) for memorability under a narrow FoV.

**Transfer.** The places model already types this as **layer-2 attachment constraints**
("rigid, lazy-follow, billboard, tether"). The evidence fixes the *defaults*: **world-fixed by
default; follow is per-window/per-place and opt-in** (visionOS: never; HoloLens: toggle; Horizon:
one window; kwin-vr is the outlier with follow-all-by-default and the field complaints to show
for it). Tether parameters converge on a *threshold + hysteresis + rate limit* shape (HoloLens
degrees/distance; Breezy 15°; kwin-vr 40/20 → 4; research/36's caps). Lindlbauer's
"view-anchored only while out of view" is the refinement worth naming: a follow that is a
*fallback presentation*, not a reparent. **Recenter** is universally a rigid re-seat of
everything head-relative, with anchored/locked content exempt — already research/36 §8's
determination; visionOS's "apps must not reinvent it" transfers as: recenter is the
compositor's, never a client's. **Trade-off.** Opt-in follow means a user who walks loses their
windows until they recenter or summon — the platforms accept this and give one gesture for it.

## 8. Clutter and occlusion management

**Lineage.** none. **Comparables.** *visionOS*: no auto-arrange; glass shows what is behind;
inactive windows recede and go translucent; proximity to physical objects makes "nearby content
semi-opaque" (mixed) and the 1.5 m boundary fades immersion (progressive/full) — none of it
configurable; **breakthrough** for people (default on in Environments; Settings: Environments /
+ Immersive Apps / Everything) and keyboards/controllers (Never / Only When Near Hands / Always);
"Avoid displaying too many windows" guidance. *Android XR*: **Tidy** — "automatically arrange up
to 3 of your most recently used apps"; orbiters "should be used sparingly… can lead to content
fatigue". *Horizon*: theater/**Focus mode** — one window larger, surroundings dimmed; hinged
groups; auto-minimize on the fourth docked launch. *HoloLens*: three-active with darkened
inactive windows; "designers with complex applications often overload the holographic frame".
*kwin-vr*: occlusion avoided at allocation (angular overlap search). *xrdesktop*: "pinned only"
mode hides everything unpinned; `arrange_sphere` / `arrange_reset`. *2D*: KWin Smart placement
minimizes overlap (kwm/fvwm lineage); GNOME overview lays thumbnails in rows with disjoint cells
and a 1.5→1 small-window boost so nothing is too small to see (`workspace.js:166-181`); Stage
Manager's four "so none is hidden". *Lindlbauer 2019*: adapt *which* apps are shown and at what
level of detail to task and cognitive load (−36 % secondary interactions); *SemanticAdapt*: keep
layouts consistent across rooms by virtual↔physical semantic association (−33 % manual
adaptations), user edits fed back.

**Transfer.** Three mechanisms, three owners. (1) **Tidy/arrange as a one-shot verb** on a place
(Android XR, xrdesktop, GNOME's overview layout) — the layer-3 engine's `arrange`, exposed to the
user by the shell and to an external WM by the protocol. (2) **Focus/theater mode** — one member
emphasized, the rest dimmed — is a *presentation* state the compositor owns (it dims the
environment layer) and the WM requests. (3) **Safety occlusion** (proximity dimming, boundary
fade, breakthrough of people/hands) is the perception layers' business (spec §4 environment and
foreground; research/62 §3.5) and *not* the WM's — visionOS makes it unconfigurable for the same
reason Mura makes it a compositor invariant an external WM cannot override. The adaptive-layout
research is real evidence that automation reduces manual work, but no shipping platform does
it; recorded as an M-later `policy` engine candidate, not a default. **Trade-off.** Not capping
windows (§1) makes (1) and (2) load-bearing.

## 9. Environments — the immersive background as a managed object

**Lineage.** none. **Comparables.** *visionOS*: system **Environments** the user opens with the
Crown, coexisting with windows ("You can use Environments while you're using apps"); an app's
`mixed` immersive space replaces the Environment by default, `.immersiveEnvironmentBehavior(.coexist)`
(visionOS 26) asks to keep it — "a preference and does not always have to be honored by the
system"; apps may tint/dim passthrough without a Full Space. *Android XR*: system environments in
either space; an app `SpatialEnvironment` (skybox + glTF, ≤ 80 MB) in Full Space only; passthrough
opacity is a *preference* gated by the `PASSTHROUGH_CONTROL` capability. *Horizon*: Home
environments; theater dimming. *2D*: wallpaper (layer-shell `background`) — research/60 §1.

**Transfer.** Mura's **environment layer** (spec §4) is the object; the environment is a *client
or service on that layer* (wallpaper client; passthrough producer over the perception intake),
never a member of a place. The platforms agree on the policy shape: the **user** chooses the
environment (Crown, launcher); an **app** may *request* to replace or coexist with it, and the
request is a preference the compositor may refuse. That is exactly how layer-shell `background`
plus an "immersive scene" request would compose; the design records it. **Trade-off.** Treating
an app's immersive scene as "take over the environment layer" keeps windows visible in front
(visionOS's progressive/full rule) at the cost of true depth mixing with the scene — which
zxr-shell-v2's colour+depth volumes (M2) provide for the *mixed* case instead.

## 10. Scene kinds — window, volume, immersive; plane, volume, environment

**Lineage.** motorcar: 2D surface vs 3D client with `CUBOID`/`PORTAL` clipping — already the
two-kind model. **Comparables.** *visionOS*: three scene types — window (glass plane, dynamic
scale, "the system clips it if the content extends too far from the window's surface"),
volume (metres, fixed scale, "content must remain within the bounds of the volume", baseplate
glow "can help people become aware of the volume's edges", tilts toward the viewer by default,
viewpoints), immersive space (one at a time). *Android XR*: spatial panel; `Subspace` for 3D
("rendered only when spatialization is enabled"); orbiters as attached UI; environment.
*Horizon*: panels (flat/curved/hinged/theater); Spatial SDK panels sized in metres. *zen*:
`bounded` (half-size box) / `expansive` (region) roles — motorcar's two modes under other names
(research/62 §2.2).

**Transfer.** Mura's kinds map one-to-one and were already there: **plane** (2D window; angular
scale), **volume** (zxr-shell-v2 3D client at M2; metres, fixed scale, clipped to its bounds —
motorcar `CUBOID`, zen `bounded`, visionOS volume), **environment** (a client/service on the
environment layer — motorcar `PORTAL`, zen `expansive`, visionOS immersive space). The manager's
job per kind: planes get placement/arrangement; volumes get placement only (no resize semantics
beyond scale, viewpoints instead of facing); environments get the request/coexist policy of §9.
**Trade-off.** none new.

## 11. Pluggability — the verbs, the withheld invariants, the crash contract

**Lineage.** motorcar's `WindowManager` is a C++ class inside the compositor; wxrc's is the map
handler. **Comparables** (full detail in the pass; the six answers per surface):

| surface | transport | verbs a policy issues | events it gets | compositor keeps | crash / disconnect | language |
|---|---|---|---|---|---|---|
| **river** `river-window-management-v1` | Wayland protocol, one WM client | `propose_dimensions`, `hide/show`, `fullscreen/exit`, `set_borders/tiled`, `use_csd/ssd`, decorations, `close`; node `set_position`, `place_top/bottom/above/below`; seat `focus_window/clear_focus`, pointer ops, bindings; `manage_finish`/`render_finish` | new window/output/seat; `dimensions_hint`, `dimensions`, app_id/title/parent, move/resize/maximize/fullscreen/**minimize requested**, seat interactions | frame-perfect double-buffering and sequencing (protocol error if violated); actual dimensions ("the window may not take the exact dimensions proposed"); fullscreen geometry; pointer warp clamped to outputs; VT switch keys; Xwayland | WM object destroyed → every window/output/seat made **inert**, in-flight sequences auto-finished; **no built-in layout fallback** — windows keep their last state until a new WM binds (hot-swap is a design goal) | any |
| **Flatland** (StardustXR) | server IPC; Flatland is a client | `set_parent(_in_place)`, `set_local/relative_transform`, panel `request_toplevel_resize`, `close_toplevel` | panel resolution/min/max, app_id/title, children | parent loops refused ("would cause a loop"); zero scale clamped ("break everything") | client dies → its objects drop; "Flatland is required for desktop apps to display correctly" — panels stop being presented | any |
| **KWin scripting** | in-process QJS/QML | property writes (`frameGeometry` → moveResize, `minimized`, `fullScreen`, `desktops`, `tile`), `setMaximize`, `sendToOutput`, `closeWindow`, ~60 `slotWindow*`, `TileManager`/`CustomTile` split/pick/resize | `windowAdded/Removed/Activated`, property NOTIFYs | nothing structurally — same process | script error = KWin error | JS/QML |
| **GNOME extensions / PaperWM** | in-process GJS monkey-patching | Meta API (`move_resize_frame`, `minimize`, `change_workspace`, `make_above`, …); override `WindowManager` animators, `_checkWorkspaces`, overview layout | Meta signals | nothing — "extensions can break the shell" (session restart to recover) | shell crash → `gnome-shell-disable-extensions` runtime file | GJS |
| **Hyprland** | `dlopen` C++ plugins + Lua + IPC socket | dispatchers (`hl.dsp.window.{close,float,fullscreen,move,swap,center,pin,…}`), tiled/floating **algorithm plugins** (`newTarget`, `movedTarget`, `recalculate`, …), decorations, hyprctl commands | event bus | API version match; "no ABI compatibility is guaranteed" | init crash caught with `setjmp` → plugin ejected | C++ (+Lua); IPC any |
| **niri** | JSON IPC + KDL config; **no plugin system** by design | the `Action` enum (~120 verbs: focus/move/consume/expel/resize/preset/workspace/overview/…), window rules | event stream (`WindowOpenedOrChanged`, `WorkspaceActivated`, …) | the layout engine itself ("isn't directly pluggable"); "Actions should apply immediately" | connection close ends the stream | any |
| **wlr-foreign-toplevel** | Wayland protocol for taskbars | `activate`, `close`, `set/unset_{maximized,minimized,fullscreen}`, `set_rectangle` | title/app_id/state/output/parent | everything — "There is no guarantee the toplevel will be actually activated" | advisory only | any |
| **Simula** | Dhall config | key actions (`grabWindow(s)`, `orientWindowTowardsGaze`, `scaleWindow*`, `push/pullWindow`, workspaces) | — | the model | — | Dhall |

**Transfer.** River is the only *bounded, frame-perfect, out-of-process* WM seam that exists,
and its reasons are Mura's: "Significantly lower the barrier to entry… Allow implementing Wayland
window managers in high-level garbage collected languages without impacting compositor
performance and latency… Allow hot-swapping… Promote diversity and experimentation"
(`river/README.md:51-58`). Its two-state split — *management state* (what the compositor tells
windows) mutable only in a manage sequence, *rendering state* (position, order) applied at
`render_finish` — is the transaction shape the draft protocol continues. What river delegates
that Mura's ruling **withholds** must be explicit: river's WM owns keyboard focus, hide/show,
borders and all placement; ADR 0012's amendment keeps "frame limits, boundary, focus rules, the
comfort caps" in the compositor. Focus is the contested item (§15 Q5). Stardust's `set_parent`
is the spatial verb river lacks (river is 2D: `set_position`); Mura's places model needs
`reparent` (frame/place) plus `set_local` — the same API research/62 §7 already names for the
in-process policy, which is the point: **the external protocol exposes the in-process API**.
niri's `Action` enum and river's window events together are a complete inventory of what a
manager needs to *hear* (new/closed, size hints, title/app_id/parent, client requests) and
*say*. Hyprland's and GNOME's in-process models show the cost of no boundary (ABI churn, "can
break the shell"); KWin's shows a stable in-process scripting shape — the *default* side of (c).
**Crash contract**: river's inert-and-wait is right for a hot-swappable WM but leaves windows
unmanaged; Mura has an in-process default to fall back to, so the contract can be *revert to
default policy on disconnect* — flag as a design choice in §15 Q5. **Trade-off.** A bounded
protocol cannot express everything KWin scripts can; that is its purpose.

## 12. Accessibility of management (only what is new over research/37, /42)

*visionOS*: "Because visionOS brings content to people — instead of making people move to reach
the content — people can remain at rest while engaging with apps and games"; "Let people use your
app with minimal or no physical movement"; the global window-size setting (Small–Extra Large) and
Two-Handed Window Zoom; "Rely on the Digital Crown to help people recenter" — one gesture returns
everything. *HoloLens*: voice verbs "Bigger", "Smaller", "Close", "Face me"; "Snap to app"
teleports the user to the ideal spot. *Android XR*: Tidy as a single action. *Horizon*: "move
with you" for one window; the display bar "positioned below the center… to accommodate
egocentric movement, user handedness". **Transfer:** every arrangement verb the design defines
must be reachable as a *single named action* (voice/switch/keyboard) — recenter, summon, tidy,
face-me, bigger/smaller — which is also what an external WM and the a11y stack (research/37)
need from the protocol: named verbs, not gestures.

## 13. Matrix — Ethereal Planes dimensions plus Mura's

Ethereal Planes' seven dimensions (Ens 2014, Table 1: perspective, movability, proximity, input
mode, tangibility, visibility, discretization) describe a *single* information plane; the
manager's questions are the same dimensions asked of a *set* of planes plus three Mura adds
(frame, layer, currency). Values below are the platforms' defaults.

| | perspective (frame) | movability | proximity | visibility of the set | discretization (grouping) | exclusivity | apps place? | scale | minimize | follow default | pluggable |
|---|---|---|---|---|---|---|---|---|---|---|---|
| motorcar | exocentric (world) | movable | far (1.5 m) | high | none | none | no (WM fans) | fixed 8 px/cm | no | none | C++ class |
| kwin-vr | exo, screens as groups | movable; detach/reattach | 1 m | high | screens ↔ free | none | no | fixed 20 px/cm | KWin's | **follow-all on** | KWin scripting |
| WayVR | exo (Floating recenters to HMD) | movable | 0.95 m | high | **sets** | none | no | fixed 20 px/cm | hide | opt-in FollowHead/Hand | config |
| xrdesktop | exo | flags: draggable/managed/pinned | g3k pose | pinned-only mode | none | none | no | user scale, unclamped | hide unpinned | containers on head/hand | g3k flags |
| Simula | ego (spawn in front) | movable | −3 m default | high | 10 workspaces + persistent | none | slots per app | zoom = px+scale | no | orient-to-gaze on demand | Dhall |
| Flatland | ego spawn 0.25 m | movable, resize handles | near | high | none | none | no | 30 px/cm fixed | close | none | client (any) |
| **visionOS** | exo; head-relative spawn ~2 m | movable, turn-to-face | far | high, inactive recede | **none** (rooms via lock) | **one immersive space** | **no** (relative only) | **dynamic (angular)** | **none** | **none** | none |
| Horizon | exo; dock is head-relative | movable; detach from dock | 1 m (45/70 cm) | high | dock 3 + 3 free (6; 12 apps v81) | immersive app + 3 windows | no | Rescale (v81) | to Navigator, state kept | opt-in, one window | Panel/Spatial SDK |
| Android XR | exo; 1.75 m, 5° below | movable 0.75–5 m | far | high | Home Space; Tidy 3 | Full Space hides all | `MovePolicy`; no first placement | constant to 1.75 m then 0.5 m/m | not documented | none documented | Jetpack XR |
| HoloLens 2 | exo; gaze-placed | movable, turn-to-face | 1.25–5 m | 3 active | tiles in the world | immersive deactivates others | no | fixed res, user scale | suspend to tile | **Follow me** toggle | MRTK |
| KWin / mutter | screen | movable, snap | — | high | desktops / dynamic | fullscreen per output | no (xdg) | — | yes | — | JS/QML; GJS |
| niri | screen | columns | — | strip | dynamic workspaces | fullscreen | no | — | **no-op** | — | IPC + config |
| river | screen | WM-decided | — | WM-decided | WM-decided | fullscreen | no | — | **event to WM** | — | **protocol** |
| Personal Cockpit | **ego, body-fixed** 50 cm | mostly fixed (handles) | near | intermediate (40° FoV) | discrete windows | — | — | fixed | — | body-fixed | — |

## 14. Verdicts (lineage → confirmed / refined / contradicted)

1. **Apps do not place or size their own windows in space — confirmed, converging with
   reasons.** motorcar's WM fans them; xdg-shell has no position request; visionOS forbids it
   and says why ("This ensures that people have full control over their workspace"); Android XR
   ignores first placement; Horizon OS "manages the panel's placement relative to the user and
   other running apps". Apps express *size hints* (min/max/default, resizability, aspect lock)
   and *relative* wishes (utility panel near this window; replace this window; parent) — all of
   which the places model expresses as membership and children, not coordinates. Determination.
2. **Head-relative spawn at a declared distance, slightly below the eye line, turned to face
   the wearer; a second window offset in front of/adjacent to the first, never on top —
   refined** from research/36 §2 with the vertical bias (Android XR 5°, Microsoft 10–20° resting
   gaze, Apple "slightly below") and the offset rule (visionOS, WayVR, KWin/mutter cascade).
   Numbers stay contract-declared (research/36 constraint 9).
3. **Dynamic (angular) scale for planes; fixed physical scale for volumes — refined from the
   lineage** (motorcar/kwin-vr/WayVR/Flatland all fixed density). Adopted from visionOS, Horizon
   Rescale and Android XR **on the engineering ground** that legibility and target size are
   angular quantities (Apple: "Dynamic scale keeps your UI and the target areas at the same
   size"; Meta: "to keep the legibility and target size") and that a 3D object "is meant to
   behave with greater physical accuracy". The piecewise curve (constant near, scaled far) is
   one platform's tuning — stand-in until the device contract's panel numbers fix it. Rule 2
   satisfied: mechanism plus reason, not "Apple does it".
4. **Free placement with offset spawn and a one-shot tidy/arrange as the default layer-3 engine
   — confirmed by every consumer XR platform**, with angular-slot allocation (kwin-vr, zen,
   research/36) as the *automatic* placer inside it and dock/group, curved band, tiling as
   further engines a place may select. Which engine a fresh place gets is **Q2** below.
5. **Exclusivity is an environment-layer takeover, requested by an app as a preference the user
   controls — refined.** visionOS (one immersive space; windows stay in front; the Crown always
   reduces immersion), Android XR (`requestFullSpace`, visual cue to leave), Horizon (immersive
   app + 3 windows) converge on: at most one exclusive scene; it does not hide the shell's
   windows; the user can always step out. Mura's environment layer is the slot; whether a
   *client* may request it, and how, is **Q4**.
6. **No window cap — confirmed by the no-cap platforms and by rule 3.** HoloLens's three-active
   and Stage Manager's four are activity/responsiveness budgets; Horizon's six is a slot count.
   The engineering content transfers as compositor budgeting of off-view planes (frame-callback
   throttling), never as a count. Determination.
7. **Minimize is not the compositor's to define — confirmed by river and by the platforms'
   disagreement.** River makes it an event; niri a no-op; visionOS has none; Horizon/GNOME keep
   state on a dock. The protocol carries `minimize_requested` and the manager decides. The
   *default policy* is **Q1**.
8. **World-fixed by default; follow is opt-in per window/place with threshold + hysteresis +
   rate limit; recenter is one compositor gesture that re-seats everything head-relative and
   exempts pinned content — confirmed** (visionOS, HoloLens, Horizon, WayVR `Static`/`Anchored`;
   kwin-vr's follow-all is the contradicting datapoint and carries research/31's complaints).
   Lindlbauer's "view-anchored only while out of view" is the refinement: follow as a *fallback
   presentation*, not a reparent.
9. **Surface snapping is the spatial snap; window-to-window snap zones do not transfer —
   refined.** visionOS 26, Horizon OS 2.1 and Android XR all snap to detected surfaces and let
   the user override; none snaps window-to-window. Pinning to a surface *is* the places-model
   `pin` verb onto an anchor frame.
10. **The bounded protocol continues river's shape — determination**, with three Mura changes:
    a `reparent(frame|place)` + `set_local(pose)` spatial verb pair in place of `set_position`
    (Stardust's `set_parent`/`set_transform`); compositor-kept invariants the WM's proposals are
    *clamped* to (river has none because a 2D screen has none); and **revert-to-default-policy
    on disconnect** instead of river's inert-and-wait, because Mura has an in-process default —
    subject to **Q5**.
11. **Safety occlusion (proximity dimming, boundary fade, breakthrough) is not the manager's —
    confirmed**: visionOS makes it unconfigurable; Mura makes it a perception-layer invariant an
    external WM cannot touch (research/62 §3.5, spec §4).
12. **The environment is chosen by the user and requested by apps as a preference — confirmed**
    (visionOS `.coexist` "does not always have to be honored"; Android XR passthrough opacity "only
    sets a preference").

## 15. Questions to the owner — one item each, options are the comparables' positions

**Q1 — What does "minimize" mean by default?**
*Why a decision:* three shipping positions, none dominant; the protocol is neutral (verdict 7)
but the default policy the wearer meets is not.
(a) **No minimize; close = background; relaunch returns the window to its place** — visionOS
("Closing an app window… transitions the app to the background without quitting it"); niri
(`set_minimized` is a no-op; the overview is the lifecycle); river's compositor. Cost: needs
fast relaunch-into-place and a launcher that shows what is running.
(b) **Minimize keeps state and parks the window on the launcher/dock with an indicator** —
Horizon (white dot on the Navigator; auto-minimize on the fourth dock launch), GNOME (to the
dash icon), cosmic (to the panel icon). Cost: a dock/launcher client must carry the state (it is
already the `foreign-toplevel-list` consumer of ADR 0012).
(c) **Hide/show as a manager-controlled render state with no fixed user meaning** — river
(`hide`/`show` are rendering state; `minimize_requested` is an event). This is the *protocol*
shape under either (a) or (b), not a third default.
My read, labelled: (b) for the in-process default — Mura's shell has the dock client the
platforms with (b) have, and "put it away, it keeps its state" is the 2D expectation Linux users
bring; (a) is what a gaze-first device with few windows can afford and is the fallback when no
dock client runs.

**Q2 — Which layer-3 engine does a fresh place get?**
*Why a decision:* every platform ships one; they differ.
(a) **Free + offset spawn + one-shot tidy** — visionOS, Android XR Home Space (Tidy 3), Horizon
detached windows. (b) **Angular slots on an arc/capsule** — kwin-vr `SpaceAllocator3D`, zen's
seat capsule, xrdesktop `arrange_sphere`, motorcar's fan, research/36's selection. (c) **Dock
group of N slots attached to a bar, angled to the user** — Horizon's 3-window Navigator dock,
HoloLens Start, Stage Manager. (d) **Curved band / virtual monitor** — Breezy, Mac Virtual
Display, Horizon theater, the Biener study rig.
My read, labelled: (b) as the *automatic placer* inside (a) — i.e. free placement whose spawn
and tidy use angular-slot allocation — because that is what the two Linux XR shells and
research/36 independently arrived at and it composes with the platforms' free default; (c) and
(d) as additional engines a user picks per place, not the default.

**Q3 — Does a hard window cap exist anywhere in Mura?**
*Positions:* none (visionOS, Android XR, kwin-vr, niri); six/twelve slots (Horizon); three
active (HoloLens); four per stage (iPadOS). Rule 3 forbids importing a cap as policy; the
platforms' *reasons* are responsiveness and layout slots. My read, labelled: no cap; off-view
planes are budgeted by the compositor (frame-callback throttling, texture GC — already partly in
R0) and clutter is handled by tidy/focus mode. Recorded as a determination (verdict 6) unless
you object.

**Q4 — May a client request exclusivity (an immersive scene that takes the environment layer),
and how does the wearer leave it?**
*Positions:* (a) yes, one at a time, other apps' windows hidden, the app's own stay, the user
can always reduce immersion with a system gesture (visionOS); (b) yes, per-app mode switch with
an in-app affordance to leave (Android XR `requestFullSpace` + expand/collapse cue); (c) yes,
immersive app plus up to three shell windows kept (Horizon seamless multitasking); (d) no
exclusivity — fullscreen is per output only (Linux desktops). My read, labelled: (a)'s shape on
the environment layer — a client may *request* it (the xdg-fullscreen analogue for the
environment), windows in the tiers stay visible in front (visionOS's own rule for
progressive/full), and the compositor-owned recenter/summon gesture always returns the shell —
with (c)'s "shell windows stay" already implied by the layer model. This touches the protocol
posture (a `zxr_` request) and zxr-shell-v2, so it is yours.

**Q5 — The verb boundary of the external seam: does an external manager own focus, and what
happens when it disconnects?**
*Positions on focus:* river's WM owns keyboard focus (`focus_window`/`clear_focus`) and
hide/show; ADR 0012's amendment lists "focus rules" among compositor-kept invariants; KWin/GNOME
scripts can steal focus freely. *Positions on disconnect:* river makes everything inert and
waits for a new WM (hot-swap; "no built-in layout fallback"); Hyprland ejects a crashed plugin
and continues with built-ins; GNOME disables extensions after a crash. My read, labelled: the
manager may *propose* focus (a `focus_hint`, honoured under the compositor's focus-stealing
rules — KWin's FSP shape) but the compositor keeps the *rules* (input workstream decides them);
on disconnect the compositor **reverts to the in-process default policy** for new events and
leaves existing placements untouched — Mura has the default river lacks, and a dead WM must not
leave the wearer with unmanaged windows.

**Q6 — Follow/tether default.** Positions: never (visionOS); per-window opt-in toggle
(HoloLens Follow me; Horizon one window); follow-all on (kwin-vr, with complaints). My read,
labelled: opt-in per place with Lindlbauer's out-of-view fallback as the *presentation* form;
this matches places-model layer 2 and is close to a determination (verdict 8) — flagged only
because kwin-vr, our closest Linux precedent, chose otherwise.

## 16. Source index

Pinned clones: `references/{motorcar,wxrc,kwin-vr,kwin-vr-patches,wlx-overlay-s,xrdesktop,simula,stardustxr-server,flatland,breezy-desktop,wivrn,kwin,mutter,gnome-shell,cosmic-comp,niri,hyprland,river,paperwm,wlroots}` at the commits in `references/MANIFEST.json`. Papers: `references/PAPERS.json` topic `xr-windowing`.

External (vendor first-party unless marked): Apple HIG — windows, spatial-layout,
immersive-experiences, ornaments, multitasking, eyes, motion, designing-for-visionos
(`developer.apple.com/design/human-interface-guidelines/…`); Apple developer docs —
positioning-and-sizing-windows, presenting-windows-and-spaces,
adopting-best-practices-for-scene-restoration, creating-fully-immersive-experiences,
understanding-the-visionos-render-pipeline; SwiftUI `defaultWindowPlacement`,
`windowResizability`, `defaultSize`, `defaultWorldScaling`, `volumeWorldAlignment`,
`immersionStyle.{mixed,progressive,full}`, `immersiveEnvironmentBehavior`, `pushWindowAction`,
`surfaceSnappingInfo`, `defaultLaunchBehavior`; WWDC23 10072/10073/10076/10078/10111/10260,
WWDC24 10149/10153/10086, WWDC25 290; Apple Support 118515, 118521, 124816 and the Vision Pro
user guide pages (dev009366408, tan5f2b0eb70, tan1e2a29e00, tanb58c3cfaf, tan899d290e4,
tan3a6602fdd, tan476683e88, tan357ede966). Meta — help 542427545314119, 172903867975450,
1100645165331717, 1143157924428593, 133727602066940, 149215193811647, 1086876265726387;
developers.meta.com/horizon/design/{windows,windows_implementation,panels,styles_layouts,
mr-design-guideline,comfort}, essentials/horizon-os-panel-sizing, documentation/android-apps/
panel-sizing, spatial-sdk panel docs; [C] UploadVR quest-v67-ptc, quest-v69-update. Google —
developer.android.com/design/ui/xr/guides/{foundations,spatial-ui},
develop/xr/jetpack-xr-sdk/{transition-home-space-to-full-space,ui-compose,subspace-modifiers,
add-environments}, `SpatialPanel`, `SubspaceModifier`, `SpatialEnvironment` references;
support.google.com/android-xr/answer/16638859; Samsung ANS10007517, ANS10007502, ANS10007511.
Microsoft — learn.microsoft.com/windows/mixed-reality/design/{comfort,app-model,app-views,
billboarding-and-tag-along,spatial-anchors,coordinate-systems,holographic-frame},
develop/porting-apps/building-2d-apps, discover/navigating-the-windows-mixed-reality-home,
hololens/{holographic-home,hololens2-basic-usage}, MRTK2 solver and slate pages. 2D — Apple Mac
User Guide (Stage Manager mchl534ba392; Spaces mh14112; Mission Control mh35798), Apple Newsroom
2022-06-06 (macOS Ventura; iPadOS 16), Apple Support 125309, 105075; [C] TechCrunch 2022-06-13,
Forbes 2022-06-14, Ars Technica 2025-06-11 (Federighi); Microsoft Support "Snap Your Windows",
"Configure Multiple Desktops", Windows Experience Blog 2021-06-24, Windows Insider Blog
2021-06-28; GNOME wiki archive Projects/GnomeShell/{Tour,Design}, usability list 2004-03
"Dynamic Workspaces", mutter commit 2012-03-08 "prefs: Add dynamic-workspaces setting", GNOME
Shell & Mutter blog 2020-12-18, 2021-02-23, the uxd-gnome-40 research posts, GNOME 40 release
notes.
