# 76 — Grab mechanics from comparables: how a ray or hand moves, faces, pushes and resizes a window

**Status:** research, 2026-09-27. Closes the gap research/64 §0 left to "the input workstream" and
research/63/68/70 stopped at (`Slot::Grabs = noop`): the mechanics of grabbing a plane in 3D. Every
claim cites the pinned clones (`references/<clone>/path:line`, MANIFEST.json; **g3k** pinned today
— xrdesktop's drag math lives there, not in `xrdesktop/`) or is marked [external]. Determinations
(§4) feed window-workspace-management.md §4 and the `Grabs` stage; the one genuine fork is an owner
item (§5).

**Question.** A wearer wants to move a window, turn it, bring it closer or push it away, and make
it bigger or smaller. What do they grab, with what, how does the window follow, what happens when
they let go — and what happens to the client's own `xdg_toplevel.move`/`resize` requests?

## 1. Subjects

| clone | what it is | grab code |
|---|---|---|
| kwin-vr | KWin's VR plugin (Qt Quick 3D scene; screens and detached windows as planes) | `src/plugins/vr/qml/Xray.qml`, `VrWindowManipulation.qml`, `Main.qml` |
| wayvr (= wlx-overlay-s' `wayvr` crate) | overlay compositor over OpenVR/OpenXR; every panel an overlay | `wayvr/src/backend/input.rs`, `windowing/window.rs`, `res/config.yaml` |
| xrdesktop + g3k + wxrd | GNOME/KDE mirroring shell; g3k is its scene/object manager; wxrd the wlroots compositor | `xrdesktop/src/xrd-shell.c`, `res/org.xrdesktop.gschema.xml`; `g3k/src/g3k-controller.c`; `wxrd/src/view.c` |
| StereoKit | XR UI toolkit whose windows are grabbable handles | `StereoKitC/ui/ui_core.cpp`, `ui/stereokit_ui.cpp`, `stereokit_ui.h` |
| flatland (Stardust) | panel shell for Stardust XR | `src/grab_ball.rs`, `src/resize_handles.rs` |
| simula, motorcar, wxrc, zen | the lineage and the small compositors | `Grab.hs`; `sixdofpointingdevice.cpp`; `input.c`, `xdg-shell.c`; `zns/src/ray-grab/move.c`, `bounded-nameplate.c` |
| MRTK3 | Microsoft's XR interaction toolkit | `ObjectManipulator.cs`, `MoveLogics/UnifiedMoveLogic.cs`, `BoundsControl.cs` |
| GNOME / KWin | the desktops' move-with-modifier | `org.gnome.desktop.wm.preferences.gschema.xml.in:6-10`; `kwin/src/kwin.kcfg:12-14,48-50` |
| visionOS, Horizon, HoloLens 2, Android XR | platforms [external] — mechanism evidence only | — |

## 2. What each does

### 2.1 kwin-vr — the head ray grabs the whole node; relative pose; stick push/pull; keep roll

- **What is grabbed:** the window node itself (`grabHandle`, default the root node,
  `VRWindow.qml:17`). **How it starts:** a global shortcut (`Main.qml:117-121` `onGrabWindowTriggered`
  → `xrView.grab`), *or* KWin's own move — when a VR window's `window.move` begins (a titlebar drag,
  the move shortcut, a context-menu "Move"), the shell grabs it: `VrWindowManipulation.qml:60-84`
  `if (window.vr) { … root.xray.grabAndAlign(appWin); root.alignGrabbedWindowToRayAtCursor(…) }`,
  with the comment that movement "might begin by a lot of reason, not only when the user moves the
  window directly … so we need to align it". **Ends:** any button press releases (`Main.qml:68-77`
  "Release grabbed object on any button press") or KWin's move ending (`:78-82`).
- **Move math:** the grabbed node's pose is stored **relative to the ray** at grab time and
  re-applied whenever the ray moves: `grab(): grabbedObjectPose = getRelativePose(root, obj)`
  (`Xray.qml:56-62`); `onSceneTransformChanged: applyRelativePose(root, grabbedObject, grabbedObjectPose)`
  (`:155-159`). The ray is the head gaze (`Xray` sits at the headgaze offsets, `:17-21`).
- **Facing:** `grabAndAlign` turns the window to face the camera **keeping its roll**
  (`turnToFaceKeepRoll`, `:64-71`); `rotateGrabbedObjectAroundCameraToRay` moves it on the sphere
  around the camera preserving distance and roll (`:107-151`).
- **Depth:** while grabbed, Up/Down keys (or the stick, `Main.qml:47-61`) set `pushGrabbed`/`pullGrabbed`;
  a `FrameAnimation` calls `grabMove(frameTime * 90 * ±1)` — a constant rate along the ray's local Z
  (`Xray.qml:44-51,78-80`).
- **Resize:** none in VR; KWin's 2D decoration resize remains for attached windows. **Thresholds:**
  `windowDetachMargin: 80` px before a window pulled past a screen edge detaches into VR
  (`VrWindowManipulation.qml:28`); a detached window that was maximized is re-maximized because "it
  improves usability" (`:222-226`).
- **Why:** the head ray is the one pointing device kwin-vr can count on; keeping roll avoids the
  window flipping when the head tilts.

### 2.2 wayvr / wlx-overlay-s — hand grab of the whole overlay; offset transform; billboard; scroll depth and scale

- **What/how:** the whole overlay when `grabbable`; the *grab* action (a controller grip / hand
  grab), distinct from *click*; `grab_start = pointer.now.grab && !pointer.before.grab`
  (`backend/input.rs:611-627`). Release on `!now.grab` (`:1040-1042`).
- **Move math:** `offset = pointer.pose.inverse() * transform` at grab (`:870-878`); each frame
  `transform.translation = pointer.pose.transform_point3a(offset.translation)` (`:1024-1030`).
  With the right modifier held the full 6DoF pose is applied instead (`:1024-1025` `transform =
  pointer.pose * grab_data.offset`).
- **Facing:** default `realign(&transform, &hmd, scale, snap_angle_deg)` — the plane looks at the
  HMD, upright, with pitch/roll snapped to `snap_angle_deg` steps when configured
  (`windowing/window.rs:261-307`: "Snap upright" when the head is tilted, else world up).
- **Depth:** scroll while grabbed: `offset.translation.z -= scroll_y * (90·Δt) * 0.02 * grab_dist`
  with `grab_dist` clamped to `[0.5, 5.0]` m and z kept `≤ −0.05` (`:1017-1022`) — the rate is
  proportional to the distance; `allow_sliding` is a user option (`res/config.yaml:206-207` "Enable
  / disable sliding windows back and forth with the scroll action").
- **Scale:** click + scroll while grabbed scales the *transform* (metres; pixels unchanged) by
  `1 − 0.025·scroll`, clamped to `[0.1, 20]` (`:937-948`, `:1005-1016`).
- **While grabbed:** `overlay.config.pause_movement = true` (`:1037`) — the follow/anchor logic
  yields to the hand.
- **Why:** an overlay shell with controllers/hands in hand: the grab is its own button, the plane
  is the affordance, the HMD is the reference for "upright".

### 2.3 xrdesktop + g3k + wxrd — trigger threshold grab; grab point as pivot; stick X scale / Y push with axis lock

- **What/how:** any `draggable` window (`xrd-shell.c:210-233`); the grab action is a digital event
  derived from the trigger at `grab-window-threshold` **0.25** ("The trigger is resting at 0.0 and
  fully pulled at 1.0", `org.xrdesktop.gschema.xml:79-85`); `_action_grab_cb` →
  `g3k_controller_check_grab` / `check_release` (`xrd-shell.c:848-859`; `g3k-controller.c:406-421`).
- **Move math (g3k):** at grab, store the object's rotation, the controller's inverse rotation and
  the **2D grab point** as an offset (`g3k-controller.c:226-256`, "translate such that the grab
  point is pivot point", `:312-316`); each update rebuild the tip transform at the hover distance
  along the controller ray and rotate the window by the *difference* of controller rotation since
  the grab ("so the window does not change its rotation when being grabbed", `:274-301`).
- **Facing:** an orientation reset animates the window back to the ray-aligned orientation over
  0.2 s (`xrd-shell.c:905-930` `transition_duration = 0.2f`).
- **Depth and scale, one stick:** `_action_push_pull_scale_cb` locks to whichever axis exceeds
  `analog-threshold` first (`XRD_TRANSFORM_LOCK_SCALE` for X, `PUSH_PULL` for Y) and unlocks when
  both return under it, "to allow switching actions without letting go of the window"
  (`xrd-shell.c:775-821`). Push/pull is **multiplicative**: `new_dist = dist * (1 + ratio * y * Δt)`
  with `scroll-to-push-ratio` **3.0** ("How much the push/pull gesture should be amplified",
  `gschema.xml:53-58`), clamped to `[0.05, 15]` m (`xrd-shell.c:14-15,735-736`); scale likewise with
  `scroll-to-scale-ratio` 1.0 (`:59-64`; `_scale_object`, `:750-773`).
- **Client requests (wxrd):** `xdg_toplevel.move` → `wxrd_view_begin_move` → the compositor's own
  move seat-op (`wxrd/src/view.c:122-130`, `xwayland.c:418-434`) — the client's request becomes the
  compositor's grab.
- **Why (their words):** the schema descriptions above — thresholds and ratios exposed as user
  settings because trigger travel and stick feel differ per controller.

### 2.4 StereoKit — pinch on the window's head/body handle; one-frame focus delay; face the user; lerped follow

- **What/how:** a window is a **handle**: `ui_win_normal` = head **and** body by default, move type
  `ui_move_face_user` by default (`stereokit_ui.h:289-290`); the handle volume is the title line
  plus the body, expanded by a grab aura and 1–2 cm of depth "so that it's easier to grab"
  (`stereokit_ui.cpp:1069-1091`); the gesture is the **pinch** (`_ui_handle_begin(… ui_gesture_pinch)`,
  `:1091`). Near vs far: a hand within 0.2 m or a head within 0.65 m of the handle discards the ray
  and interacts directly; otherwise the far ray (`ui_core.cpp:350-363`).
- **Start rule:** the grab begins only on the frame *after* the handle gained focus — "This waits
  until the window has been focused for a frame, otherwise the handle UI may try and use a frame of
  focus to move around a bit" (`ui_core.cpp:378-391`).
- **Move math:** one hand — the handle keeps its start offset from the palm, rotated by the palm's
  rotation delta (`ui_move_exact`) or by a look-at from the grab point toward the head
  (`ui_move_face_user`, `:454-464`; the head target is lowered 12 cm); the result is **lerped**:
  position 0.6, orientation 0.4 per frame (`:469-471`). Two hands — the midpoint and the hands'
  relative pose (`:402-436`).
- **Depth/scale:** implicit in the hand's depth; two-hand distance changes the pose, not a
  separate scale API. **Why:** a toolkit for hand-tracked apps: the hand *is* the affordance, the
  window follows it directly, facing the user by default.

### 2.5 flatland — corner grab-balls resize; pinch > 0.90; the resize owns the pose

- Grab balls float `0.025` m off the panel (`resize_handles.rs:30`), radius `0.02`, padding `0.05`
  (`grab_ball.rs:31-33`); a hand grabs when `pinch_strength > 0.90`, a controller when `grab > 0.90`
  (`grab_ball.rs:107-109`, `resize_handles.rs:147-148`); the ball follows the pinch midpoint
  (`grab_ball.rs:143-152`); top/bottom handles resize the panel (`resize_handles.rs:400-430`), and
  "a resize owns the pose while it lasts, so anything that came in meanwhile is dropped" (`:428-429`).
- **Why:** Stardust panels are the app's; the shell adds *handles outside the content* rather than
  claiming the body.

### 2.6 The small compositors and the lineage

- **wxrc:** `xdg_toplevel.request_move` → `wxrc_view_begin_move` (`xdg-shell.c:111-115`); the move
  seat-op re-places the view on the **gaze ray at its current distance** and copies the view's
  orientation (`input.c:231-250`); resize is a 2D size delta from the pointer's plane-local motion
  (`:253-265`). The client's request is the whole grab; no compositor-side affordance.
- **zen:** the **nameplate** below a bounded object starts a move (`bounded-nameplate.c:140-142`),
  as does the client's protocol `move` (`bounded.c:207-221`); motion is on the **seat capsule**
  (Δpolar/Δazimuthal, polar clamped `[0, π]`, `zns/src/ray-grab/move.c:15-45`) — "not supporting
  6DoF devices yet" (`:19-21`); no scale, no depth.
- **motorcar:** the controller's bumper grabs the surface under its cursor and attaches it 6DoF
  (`sixensecontrollernode.cpp:57-65`, `sixdofpointingdevice.cpp:123-141`). **simula:** controller
  press → manipulating; two controllers → resize by their distance ratio (`Grab.hs:26-71`); windows
  continuously oriented toward the gaze (`Types.hs:1672-1683`).

### 2.7 MRTK3 — whole-object manipulation with an attach point; constraints; handles for bounds

- `ObjectManipulator`: near or far select grabs the whole object; per frame `ScaleLogic`,
  `RotateLogic`, `MoveLogic` then constraints (`ObjectManipulator.cs:730-869`); the move keeps the
  **attach point** — `attachToObject = target − attachCentroid` at start, then
  `attachCentroid + attachToObject` (or the object-local attach point re-projected) each frame
  (`MoveLogics/UnifiedMoveLogic.cs:26-58`); rotation about the grab point or the centre is an
  option (`ObjectManipulator.cs:44-64`); smoothing lerp times default `0.001` (`:360-400`).
- `BoundsControl`: corner handles scale, edge handles rotate, faces translate
  (`BoundsControl.cs:254,789-901`) — the *bounds*, not the content, are the affordance.

### 2.8 The desktops' modifier, and the platforms [external]

- GNOME `mouse-button-modifier` `<Super>`: "Clicking a window while holding down this modifier key
  will move the window" (`org.gnome.desktop.wm.preferences.gschema.xml.in:6-10`); KWin
  `CommandAllKey = Meta`, `CommandAll1 = "Activate, raise and move"` (`kwin.kcfg:12-14,48-50`). A
  2D window's body is the client's; the desktops reach past it with a modifier.
- **visionOS** [external]: the **window bar** under every window (look at the bottom edge and it
  appears; pinch-drag it to move); the corner appears on look for resize; apps cannot position or
  size their windows after they appear. **Horizon** [external]: a grab bar under each panel;
  scale by two-hand or the bar's handles. **HoloLens 2** [external]: the app bar (adjust/remove)
  and a bounding box with corner scale / edge rotate handles when adjusting; "never closer than 40
  cm" as the comfort floor. **Android XR** [external]: a panel handle below the window, title-bar
  drag. All four put the affordance **outside the client's content**, at the bottom, and show it
  on hover/look.

## 3. What converges, what forks

**Converges (every implementation):**

1. **The grabbed pose is expressed in the grabbing device's frame at grab time and re-applied as
   the device moves** — kwin-vr `getRelativePose`/`applyRelativePose`, wayvr `offset =
   pose⁻¹·transform`, g3k's tip transform with the grab point as pivot, MRTK3's attach point,
   StereoKit's start offsets, motorcar's `parent⁻¹ · controller · grabOffset`. There is no other
   move math in the field.
2. **Depth is a separate axis while grabbed** — stick or scroll pushes/pulls along the ray, at a
   rate proportional to distance (wayvr `0.02·dist·scroll`, xrdesktop multiplicative `(1 + 3.0·y·Δt)`),
   clamped (wayvr `[0.5, 5]`, xrdesktop `[0.05, 15]`, HoloLens ≥ 0.4 m).
3. **Facing the head while moving, upright** — wayvr `realign`, StereoKit `ui_move_face_user`,
   simula, xrdesktop's reset; kwin-vr keeps roll but still turns to face. Exact 6DoF (motorcar,
   wayvr's modifier, StereoKit `ui_move_exact`) exists as the *alternative*, never the default.
4. **Release keeps the pose** — nobody snaps a released window back (zen's capsule is the one
   engine that constrains where it can be; that is arrangement, research/64 §3, not grab).
5. **Client `move`/`resize` requests become the compositor's own grab** — kwin-vr on KWin's
   `window.move`, wxrd/wxrc/river/zen on the protocol request. Nobody honours the request as a
   client-driven position.
6. **The grabbed window receives no input from the grabbing device while grabbed** (wayvr
   `pause_movement`, KWin's move seat-op, MRTK's select) and follow/anchor logic is suspended.
7. **A small settle before the move begins**: StereoKit's one-frame focus delay; g3k rotates by
   the delta *since* the grab so nothing jumps at grab time.

**Forks:**

- **A. What is grabbed.** (i) *the whole body*, with a grab input that is not the client's commit —
  a grip/grab button (wayvr, xrdesktop, motorcar), a shortcut or the desktop's modifier (kwin-vr,
  GNOME/KWin `Super`+drag), a pinch on a toolkit handle that is not app content (StereoKit); or
  (ii) *a bar or handles outside the content* — zen's nameplate, flatland's balls, MRTK3's bounds,
  and every consumer platform (visionOS, Horizon, HoloLens, Android XR). The reason each side
  gives: (i) the plane *is* the affordance when the shell owns it (overlays) or a separate button
  exists; (ii) the body belongs to the app's touch/pointer input, so the grab must live beside it.
  For Mura the body of a 2D plane is the client's `wl_pointer`/`wl_touch` surface — the (ii) reason
  applies exactly, and the (i) mechanisms exist too (controller `grasp`, the desktop modifier for
  a mouse). Both are needed; the question is what the *default* commit-class gesture on the floor
  (head ray + select, hand pinch) grabs — see §5.
- **B. Scale versus resize.** Overlay shells *scale* the plane in metres (wayvr `[0.1, 20]`,
  xrdesktop, simula two-hand, MRTK3); 2D compositors *resize* in pixels (wxrc, KWin, zen's
  `set_size`). window-workspace-management §3 principle 3 already rules this for Mura: "resize
  changes pixels and move in depth changes metres" — apparent size is angular, density is one
  number. Scale is therefore **not a Mura verb**; the two-hand scale gesture is not adopted, with
  that reason.
- **C. Keep roll or snap upright.** kwin-vr keeps roll; wayvr/StereoKit/simula use world up (wayvr
  snapping upright when the head is tilted past 0.2). Majority and the design's "billboard while
  moving" (§7): upright.

## 4. Determinations (evidence-driven; no owner item needed)

- **D1 — Move math:** the grab stores the plane's pose in the grabbing ray's frame (kind: hand
  aim / controller aim / head / the mouse's plane point) and re-applies it each tick as the ray
  moves; the grab point is the pivot (g3k; MRTK3 attach point). Identical for every kind — the
  head ray is just another ray (kwin-vr's Xray *is* the head gaze).
- **D2 — Facing:** while grabbed and `wm.move.billboard` (ruled true, §7), the plane yaws to face
  the head and stays upright (wayvr `realign`, StereoKit `face_user`); with `billboard = false`
  the ray's rotation delta applies (g3k, `ui_move_exact`). Release keeps the pose (`free`).
- **D3 — Depth:** the grabbing device's secondary axis (controller stick Y, mouse wheel, touchpad
  scroll) pushes/pulls along the ray **multiplicatively**, `d ← d·(1 + rate·axis·Δt)`, xrdesktop's
  shape and its `scroll-to-push-ratio` **3.0** as the default of a preference `wm.grab.depth_rate`
  (xrdesktop and wayvr both expose the rate); clamped by the compositor's `limits` (§11):
  `min_distance_m` **0.4** (HoloLens' comfort floor, the one *reasoned* number [external];
  wayvr 0.5) and `max_distance_m` **5.0** (wayvr; HoloLens comfort zone 1.25–5 m [external]) —
  display-comfort calibrations, immutable (`hardware.input.comfort.*`), stand-ins until the
  first hardware.
- **D4 — Resize:** in logical pixels through `xdg_toplevel` configure, from the plane-local motion
  of the grabbing ray on the grabbed edge/corner (wxrc `input.c:253-265`, KWin's decoration); the
  compositor's `limits.max_angular_deg` clamps the result at the current distance; the client's
  `xdg_toplevel.resize(edges)` starts the same grab (D6). No scale verb (§3 fork B).
- **D5 — Start/end and settle:** the grab begins on the frame after the affordance is *focused*
  (StereoKit) and the commit is held; no drag-start distance (none of the XR comparables has one —
  the desktops' 8 px threshold protects click-vs-drag on a 2D body, which the bar/handle already
  separates); it ends on the commit's release (or a tier loss — the loss tracker's release,
  research/70). The grabbing kind's samples are `Flow::Consumed` from the `Grabs` slot for the
  grab's duration (convergence 6); follow is paused (§7 already says so).
- **D6 — Client requests:** `xdg_toplevel.move` / `resize(edges)` with a valid serial start the
  same grab on the requesting seat device (convergence 5; design rule 5 in §4).
- **D7 — Grab inputs (fork A(i), all adopted):** controller `grasp` (the profile's `squeeze`,
  already sampled as `values.grasp`; xrdesktop/wayvr/motorcar), a mouse with the desktop modifier
  (`Super`+button, GNOME/KWin), a hand's `grasp_ext` when the runtime provides it (StereoKit's
  pinch-on-handle is the hand equivalent of a grip) — each grabs the **body**.
- **D8 — Not adopted, with reasons:** two-hand scale (fork B); zen's capsule constraint (an
  arrangement, research/64 §3); kwin-vr's "any button releases" (a release is the commit's release,
  or a client's input would end grabs); haptics on grab (xrdesktop/wayvr) — no haptic path in zxr
  yet, recorded.

## 5. The one owner item (rule 8) — what the commit-class gesture grabs

**Decided:** what the *default* commit (head ray + `hmdButtons.select`, a hand's pinch, a mouse's
plain button) grabs, given that the body is the client's.

**Options — the comparables' actual positions:**

- **(a) A bar below the plane, shown on hover/look** — visionOS, Horizon, Android XR, HoloLens'
  app bar; zen's nameplate [external + `bounded-nameplate.c:140-142`]. The affordance is a
  compositor-drawn hit volume under the plane's bottom edge (ADR 0012: "in 3D the decoration *is*
  a set of trusted hit volumes"), invisible until the ray is near it, one quad while shown (the
  cursor's one-layer shape, research/70 §9). Consequence: every kind can grab with its ordinary
  commit; a bar costs one hit volume per plane and one drawn quad for the hovered one; its height
  is a stand-in (**2°** of visual angle, research/42 §5's effective target width; Meta's ≥ 2.5–3°
  hit targets [external]) — a preference, since it is a target size (`wm.grab.bar_deg`).
- **(b) No bar: grab inputs only** — wayvr/xrdesktop/kwin-vr's shape (grip button, shortcut,
  modifier). Consequence: nothing to draw, but the **input floor cannot grab** (head ray + one
  select button has no second control; a hand without `grasp_ext` cannot either) — the floor
  would depend on the client's own titlebar drag (a CSD titlebar; the design mandates SSD).
- **(c) Both** — (a) as the floor's path and D7's grab inputs as the power path (kwin-vr keeps
  both its shortcut and KWin's titlebar move).

**My read (flagged, rule 4):** (c). (a) is the only shape that works at the input floor and is
what every shipping headset does; (b)'s mechanisms cost nothing extra and are what controller and
mouse users expect. Built under (c) below; the bar's *look* (a strip, its colour) is the decoration
chrome renderer's — registry row "zero design" — and here is the plainest possible thing: the
cursor ring's material, a translucent strip. Say so if you want (a) or (b) alone, or a different
bar geometry.

## 6. Keys this research adds

| key | kind | default | source |
|---|---|---|---|
| `wm.grab.depth_rate` | preference, double [0.5, 10] | 3.0 /s | xrdesktop `scroll-to-push-ratio`; wayvr's `scroll_speed` |
| `wm.grab.bar_deg` | preference, double [0.5, 6] | 2.0° | research/42 §5 target width; Meta ≥ 2.5–3° [external] (stand-in) |
| `hardware.input.comfort.min_distance_m` | calibration | 0.4 m | HoloLens comfort floor [external]; wayvr 0.5 |
| `hardware.input.comfort.max_distance_m` | calibration | 5.0 m | wayvr; HoloLens comfort zone [external] |
| `hardware.input.comfort.max_angular_deg` | calibration | 90° | stand-in (no comparable states one; the fov is the ceiling) |

`wm.move.billboard` (already declared) is D2's switch. `input.pointer.stick_deadzone` already
gates the stick axis D3 reads.

## 7. Sources

`references/kwin-vr/src/plugins/vr/qml/{Xray,VrWindowManipulation,Main,VRWindow}.qml`;
`references/wayvr/wayvr/src/{backend/input.rs,windowing/window.rs,res/config.yaml}`;
`references/xrdesktop/{src/xrd-shell.c,res/org.xrdesktop.gschema.xml}`; `references/g3k/src/g3k-controller.c`;
`references/wxrd/src/{view.c,xwayland.c}`; `references/stereokit/StereoKitC/{ui/ui_core.cpp,ui/stereokit_ui.cpp,stereokit_ui.h}`;
`references/flatland/src/{grab_ball.rs,resize_handles.rs}`; `references/simula/addons/godot-haskell-plugin/src/Plugin/Input/Grab.hs`;
`references/motorcar/src/device/sixensecontrollernode.cpp`, `src/compositor/scenegraph/input/sixdofpointingdevice.cpp`;
`references/wxrc/src/{input.c,xdg-shell.c}`; `references/zen/zns/src/ray-grab/move.c`, `zns/src/bounded-nameplate.c`;
`references/mrtk3/org.mixedrealitytoolkit.spatialmanipulation/{ObjectManipulator/ObjectManipulator.cs,ObjectManipulator/MoveLogics/UnifiedMoveLogic.cs,BoundsControl/BoundsControl.cs}`;
`references/gsettings-desktop-schemas/schemas/org.gnome.desktop.wm.preferences.gschema.xml.in`; `references/kwin/src/kwin.kcfg`.
[external]: Apple visionOS HIG "Windows" and "Positioning and sizing windows"; Meta Horizon OS
panel design guidance; Microsoft MR "Comfort" and "App bar and bounding box"; Android XR spatial UI
guidance.
