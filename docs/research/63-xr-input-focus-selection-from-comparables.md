# 63 — XR input: targeting, hover, selection, focus, movement, cursors, peripherals and text fields, from comparables

**Research date:** 2026-09-26. **Question:** the compositor's `input` module (spec §8) has a
transport — ray → plane → `wl_pointer` — and an input floor (research/42), but no *interaction
model*: what is targeted and how (gaze, hand ray, controller ray, head ray, direct touch — and
switching between them), what hover is and who sees it, what commits, how keyboard focus and
activation are decided, how many pointers two hands are, how a window is moved, resized and
closed in space, what a cursor is, how a Bluetooth mouse travels between planes, how a text
field summons a keyboard, and whether an application ever sees where the wearer looks. The
registry row "Focus/activation authority" is **missing**. This document derives all of it from
comparables so that [spatial-input.md](../architecture/spatial-input.md) and spec §8 rev are
written from evidence.
**Already ruled, built on here, not restated:** the input floor (research/42 §4–§5: head-aim +
`hmdButtons.<selectRole>`, dwell timings, pointing-without-hands baselines); the interaction
patterns (research/36 §9: keyboard dual mode with WiVRn's 0.18/0.22 m hysteresis, dictation
fallback, gaze never to the client in the consent picker, recenter conventions); composition
§7.3 constraint 7 (stabilization — deadzone + smoothing + dwell, magnetism as policy,
event-time compensation — *before* class-aware arbitration) and constraint 2 (unbounded pointer
space); ADR 0013's five seams; the ray→`wl_pointer` transport (research/59 §6).
**Method (AGENTS.md rules 2, 7, 8):** lineage first (motorcar, wxrc), then the pinned XR shells
(kwin-vr, wayvr, xrdesktop/gxr, StardustXR, Simula, WiVRn's lobby), the standard (OpenXR core,
`XR_EXT_hand_interaction`, `XR_EXT_eye_gaze_interaction`, Monado's status), the Wayland model
(core seat, the protocol families, libinput, smithay), the 2D desktops' focus policy (KWin,
mutter/gnome-shell, niri, cosmic-comp), three open toolkits (MRTK3, StereoKit, godot-xr-tools —
newly pinned) and the platform documentation [external] — visionOS in depth as the most complete
shipping gaze+hands design, Horizon OS, Android XR, HoloLens 2 — under rule 2: mechanism and
stated rationale only, never authority. Citations are `references/<clone>/path:line` or
[external] with URL. Hardware tiering per the owner: eyes+hands where eye tracking exists
(three targets — research/29), hands-only, controllers, and Bluetooth mouse/keyboard/trackpad as
a peer on every tier; the tier comes from the device contract.
**Budget impact:** a research document; §14's design consumes the per-tick cost of one
stabilization filter per active source and a hit test over the scene's member pass — the same
pass the renderer already makes — and no thread.

## 1. Input model taxonomy and tier switching

**Lineage.** motorcar: one `SixDOFPointingDevice` per controller casts a ray along its −Z every
frame through `scene->intersectWithSurfaces` (`motorcar/src/compositor/scenegraph/input/sixdofpointingdevice.cpp:86-88`);
2D surfaces get mouse events, 3D clients `SixDofEvent`s. wxrc: the ray is the **head** pose plus
a mouse-driven rotation clamped to FOV ± 20° (`wxrc/src/input.c:135-150, 300-307`) — a
head-ray-plus-mouse hybrid, the prototype's tier.
**Comparables — what the tiers are and how they switch.**
- visionOS [external]: "indirect gesture by looking at an object to target it, and then
  manipulating that object from a distance … with their hands"; direct gestures exist for
  everything but "people may find it tiring to keep their arms raised … direct gestures are
  best for infrequent use" (HIG Gestures). Eyes target, hands act: "Eyes are the primary
  targeting mechanism … Combined with eyes for targeting, hand gestures are the primary way to
  interact across the system" (WWDC23 10073).
- Horizon OS [external]: an explicit **priority hierarchy** — "Default priority: Controller/Hand
  Ray > Head ray. With eye tracking enabled (controllers not in hand): Gaze > Hand Ray > Head
  ray"; near/far: "Ray casting is enabled when the user's hand or controller is far from an
  interactable. If the hand or controller gets closer, the ray is automatically turned off, and
  direct interaction is prioritized"; and a rule: "Do not combine gaze and ray casting as
  simultaneous targeting modes" (Interactions input hierarchy; Hands UI best practices).
- Android XR [external]: gaze + pinch is the default ("spatial equivalent of tapping on a
  touchscreen"); the interaction framework can "use head to aim rather than eyes"; hand poke,
  hand raycast, mouse "projected into your 3D scene", controllers, with "automatic modality
  transitions".
- HoloLens 2 [external]: hand rays "turned off automatically" within arm's length ("roughly
  50 cm"); MRTK 2: "if hand rays are enabled, the head or eye gaze focus pointer are disabled as
  soon as the hands come into view. If you want to support a 'look and pinch' interaction, you
  need to disable the hand ray."
- MRTK3: an `InteractionModeManager` whose job is to "ensure that … several interactors for a
  'controller' aren't clashing and firing at the same time" (`mrtk3/…/InteractionModeManager.cs:13-15`),
  a `ProximityDetector` sphere for near mode (`ProximityDetector.cs:48-54`) and a latch that
  keeps near mode while a grab continues (`NearInteractionModeDetector.cs:117-126`).
- StereoKit: ray shown only when the palm faces away from the head
  (`stereokit/StereoKitC/ui/ui_core.cpp:109`); near beats far — "within touching distance" if
  head < 0.65 m or hand < 0.2 m, far focus is discarded (`:354-358`).
- WiVRn: touch↔ray blend by palm distance 0.18–0.22 m (`wivrn/client/constants.h:45-47`,
  `imgui_impl.cpp:618-625`). godot-xr-tools: a `suppress_radius = 0.2` m area that disables the
  ray near UI (`godot-xr-tools/addons/godot-xr-tools/functions/function_pointer.gd:95-100`).
- The XR shells: kwin-vr = head ray only (`XrScene.qml:177-179`); wayvr, xrdesktop, Simula =
  controller rays; StardustXR = per-device methods (beam, tip, hand) ordered by distance.
**The standard.** `XR_EXT_hand_interaction` names the tiers as poses: **aim** "for interacting
with objects out of arm's reach … a virtual laser pointer", **pinch** "within arm's reach using a
finger and thumb", **poke** "using a fingertip to touch and push a small object … typing on a
virtual keyboard", **grip** for holding (`openxr-docs/…/ext_hand_interaction.adoc:65-68, 156-158,
218-222, 113-115`); the runtime stabilizes aim and pinch (`:86-88, 162-163`). Eye gaze is a
separate profile providing only a pose (`ext_eye_gaze_interaction.adoc:117-125`).
**Monado status.** Bindings for both EXT profiles exist (`monado/…/bindings.json:209-263,
2424-2440`) but **no driver synthesizes** `XRT_INPUT_HAND_PINCH_*`/`AIM_ACTIVATE` from hand
tracking — only `ht_ctrl_emu` turns a pinch into a simple-controller `select`
(`drivers/ht_ctrl_emu/ht_ctrl_emu.cpp:411-464`); eye gaze is provided by one driver (PSVR2,
`psvr2.c:1235-1250`); the simulated HMD has head pose only, no button (`simulated_hmd.c:194-217`).
**Transfer.** The tier order is the same in every platform that states one: gaze (when present)
> hand aim ray > controller aim ray > head ray, with direct touch overriding rays inside a
distance band, and *one* targeting mode active at a time. Mura's device contract already carries
the facts the switch needs (`controllers ∈ none|imu-3dof|optical-6dof`; eye tracking per
research/29). The one Mura-specific finding: on Monado today the *runtime* does not deliver the
hand aim/pinch the standard puts on it, so either zxr synthesizes them from joints (what
`ht_ctrl_emu`, MRTK's `ArticulatedHandController` polyfill and StereoKit all do) or Mura adds it
to Monado — the standard says the runtime's; the budget says do it once, in the runtime.
**Trade-off.** A strict hierarchy loses the "second hand does something else" case; the
comparables accept that (Meta forbids simultaneous gaze + ray). **Verdict: lineage refined** —
motorcar's per-device ray becomes a contract-tiered hierarchy with a near/far band; wxrc's
head+mouse hybrid survives as the floor tier and as the mouse model of §8.

## 2. Targeting and hover

**What is hovered.** Every comparable hovers the nearest hit along the active source's ray
(motorcar's `intersectWithSurfaces`; wayvr sorts hits by distance and walks nearest-first until a
handler consumes, `wayvr/src/backend/input.rs:800-830`; StardustXR `order_by_distance` with
exclusive capture, `objects/input/mod.rs:500-534`; MRTK3 fuzzy gaze cone-cast 10° / 0.3–10 m,
`FuzzyGazeInteractor.cs:26-48`). The two prototypes that take the *first list hit* rather than
the nearest — wxrc (`input.c:158-193`, its own TODO at `:138`) and kwin-vr's "first accepting
`rayPickAll`" (`VrPicking.qml:52-64`) — are the ones with the recorded user complaints
(research/31 §2.11).
**Hover feedback — who renders it.** visionOS [external]: the system, out of the app's process:
"the system applies your predefined hover effect in a process that's outside of your software's
process … you don't know when the system applies a custom hover effect" (HIG Eyes); feedback must
be "subtle and work on top of any content" because eyes move quickly (WWDC23 10073). Android XR
[external]: "the system displays a generic hover effect when it detects a user is looking at an
interactable element" (Design for XR foundations). HoloLens [external]: for eye gaze "Don't show
a cursor … use subtle visual highlights" that ramp "over the course of 500-1000 ms"
(Gaze-and-commit eyes); for hand rays a donut cursor. The XR shells render a cursor/reticle at
the hit and let the *client* draw its own hover styling from the pointer motion they forward
(kwin-vr draws KWin's cursor texture at the hit, `VrKwinCursor.qml:20-41`; wxrc/motorcar/Simula
place the client's cursor surface at the intersection, `wxrc/src/input.c:448-512`,
`sixdofpointingdevice.cpp:98-107`).
**Target size.** visionOS 60 pt minimum target area / 16 pt margin, rounded shapes because "eyes
tend to be drawn toward the corners" (HIG Eyes; WWDC23 10073); HoloLens eye targets "at least 2°
in visual angle", head-gaze 1–1.5° minimum, 3° for speed (Eye-gaze interaction; Gaze-and-commit
head); Android XR 48 dp min / 56 dp recommended scaled by distance ("DistanceInM × 0.868 × 48").
**Stabilization — what shipping code does.** MRTK3: a hand ray with exponential-decay
half-life 0.01 (`HandRay.cs:20-21`), **sticky hover** — once `SelectProgress > 0.5` only the
current target is considered ("Sticky hover, ADO#1941", `GazePinchInteractor.cs:81-83,175-177`)
— and a **relaxation threshold** before a new target may be hovered (0.1 for gaze-pinch, 0.5
for rays: "fewer accidental activations will occur with rays", `MRTKRayInteractor.cs:56-66`);
poke reticle magnetism 0.07 m (`ReticleMagnetism.cs:37-49`). StereoKit locks the pinch point to
the index root while pinched — "helps prevent traveling of the pinch point during release"
(`stereokit/StereoKitC/hands/input_hand.cpp:420-428`). godot-xr-tools locks the pointer's target while pressed
(`function_pointer.gd:218-223`). Meta [external]: "The pinch gesture itself causes a slight hand
shift, which often pulls the ray reticle off small targets … ISDK includes ray stabilization
that smooths the reticle path and helps the user hold the ray on a target through a pinch"
(Hands UI best practices); the pointer pose is runtime-filtered ("non-trivial task involving
filtering"). xrdesktop replays the queued pointer path from click onset to cancel press-shake
(`xrd-input-synth.c:340-362` — composition constraint 7's "event-time compensation"). HoloLens
[external]: eye-gaze accuracy "approximately within 1.5 degrees", 30 Hz; never attach UI to the
gaze point ("fleeing cursor"). None of kwin-vr, wxrc, motorcar, Simula stabilize (pass 1
absences) — and kwin-vr is the one with the field record.
**The Wayland tension.** For a Wayland client, "hover styling" *is* `wl_pointer.enter/motion`
without a button (`waypipe/protocols/wayland.xml:2083-2113`); tablet tools have the same in
`proximity_in` + `motion` without `down` (`tablet-v2.xml:61-72`). There is no way to give a
client hover feedback without giving it the position — which is exactly the information visionOS
and Android XR withhold from apps (§10). The XR shells that forward rays as pointer motion
(kwin-vr, wayvr, xrdesktop, wxrc, Simula) all leak the ray; none of them has eye tracking. This is
the first fork for the owner (§15 Q1).
**Transfer.** Nearest-hit with class-aware arbitration is already constraint 7; the numbers that
transfer as *stand-ins* are MRTK3's sticky/relaxation pattern and HoloLens' 500–1000 ms
highlight ramp; the 60 pt / 2° minimum is a *shell* rule (Mura's own components) — 2D clients
were not designed for it and the compositor cannot resize their widgets, which is another
argument that gaze alone should not be a fine pointer into arbitrary 2D content. **Verdict:
constraint 7 confirmed and given its numbers; the hover-feedback question is a fork.**

## 3. Commit

**Pinch = tap, everywhere that has hands.** visionOS: "Pinching your fingers together is an
equivalent of pressing on the screen of your phone" (WWDC23 10073); Android XR: "the spatial
equivalent of tapping on a touchscreen or pressing a mouse button"; Meta: "A successful pinch of
the index finger is equivalent to a select or trigger action on a controller" [external]. The
standard: `/input/pinch_ext/value` is "linear to tip distance", 1.0 when touching, gated by
`ready_ext` (`ext_hand_interaction.adoc:316-338`); controllers give `/input/select/click` on the
Khronos simple profile (`semantic_paths.adoc:665-686`).
**Thresholds shipping code uses** (all stand-ins, none derived from a study): MRTK3 pinch
open/closed 0.75 / 0.25 of index length, polyfill debounce 1.0 down / 0.9 up ("Debounce the
polyfill pinch action value", `ArticulatedHandController.cs:96-97`), UI select/deselect 0.9 / 0.1
(`StatefulInteractable.cs:55-71`); StereoKit 1.0 cm to activate / 1.5 cm to hold
(`stereokit/StereoKitC/hands/input_hand.cpp:395-404`); WiVRn trigger > 0.7 and fingertip within (−0.01, −0.15) of the plane
for touch (`constants.h:42-49`); godot grip 0.7. Hysteresis is universal; the numbers are not.
**Beyond tap.** visionOS's standard gestures [external]: tap = activate/select; pinch-and-drag =
move/scroll (Apple Support: "Pinch and hold to grab … then drag"; "pinch and drag to scroll");
swipe = "Pinch and quickly flick your wrist" to scroll fast, tap to stop; touch-and-hold = extra
controls; double tap = zoom; two hands pinch-and-drag apart = zoom, circular = rotate (HIG
Gestures specifications). Android XR: "A held pinch is used to scroll, move or resize windows".
Meta system gesture: palm facing you + index pinch = menu, reserved ("cannot be remapped in
experiences"); Android XR: palm inward + pinch-hold + move = Back/Launcher/Recents.
**Dwell** is the fallback everywhere: HoloLens "only … as a last fall-back if neither voice input
nor hand input is available", onset 150–250 ms then 650–850 ms to complete (Gaze and dwell);
visionOS Dwell Control is an accessibility setting; MRTK3 dwell 1.0 s + 0.3 s trigger hold
(`InteractorDwellManager.cs:21-24`); godot gaze `hold_time = 2.0` s; research/42 §5 has the
literature numbers.
**Mapping onto Wayland.** Press/release → `wl_pointer.button` (Linux button codes) *or*
`wl_touch.down/up`; drag → motion while pressed; two-hand zoom/rotate → `zwp_pointer_gestures_v1`
pinch (`scale`, `rotation`, `dx/dy`, `fingers`; intended for touchpads, `pointer-gestures-unstable-v1.xml:153-204`)
*or* two `wl_touch` contacts (toolkits' native two-finger zoom); swipe-to-scroll → `wl_pointer.axis`
with source `finger` + `axis_stop` (the protocol's own reason: "scroll events from a 'finger'
source may be in a smooth coordinate space with kinetic scrolling", `wayland.xml:2239-2244`).
The system gesture is compositor-owned (both platforms reserve it) — in Wayland terms it is never
forwarded, like a compositor keybinding.
**Transfer.** Converging: pinch = the select of the standard, hysteresis mandatory, values
stand-in until measured on Mura's hand tracker; the gesture vocabulary maps onto existing
protocol without a `zxr_` extension. **Verdict: determination** for the vocabulary; numbers are
stand-ins flagged in the design.

## 4. Focus and activation

**The 2D desktops converge.** Default policy is click-to-focus in all four: KWin `ClickToFocus`
("Clicking into a window activates it. This is also the default", `kwin/src/options.h:209-242`;
the under-mouse variants "only provided for old-fashined die-hard UNIX people"); mutter `click`
("windows must be clicked in order to focus them", `org.gnome.desktop.wm.preferences.gschema.xml.in:41-61`);
niri click, `focus-follows-mouse` off by default with a "won't focus a window if it will result
in the view scrolling more than the set amount" guard (`niri/docs/wiki/Configuration:-Input.md:341-366`);
cosmic-comp click (`input/mod.rs:849-1004`). Focus-follows-mouse where offered is *delayed*
(KWin 300 ms; mutter "focus-change-on-pointer-rest", 25 ms rest timer, `display.c:3530-3534`).
**Stealing prevention is user-time / serial based, refusal signals urgency.** KWin's levels
(`x11window.cpp:4159-4166`; Wayland `mayActivate`: transient of active, ancestor usage serial,
matching token, `activation.cpp:578-636`); mutter compares against `last_user_time` and on
refusal `meta_window_set_demands_attention` ("we just set up a pulsing indicator, rather than
move windows or workspaces", `window.c:3974-4007`); niri: "Tokens without a serial are
urgency-only" (`handlers/mod.rs:767-768`), valid if serial ≥ last keyboard or pointer enter;
cosmic: "Tokens without validation aren't allowed to steal focus" (`xdg_activation.rs:72-83`).
New windows take focus unless an intervening user action says otherwise (mutter
`window_state_on_map`, `window.c:2118-2183`; niri Smart; KWin `addWaylandWindow`,
`workspace.cpp:942-963`). The protocol itself: the compositor "might decide not to follow through
with the activation if it's considered unwanted" (`xdg-activation-v1.xml:102-107`).
**The XR shells: focus on commit, never on hover.** wxrc sets keyboard focus on button press
(`input.c:324`, `view.c:68-94`), hover only moves the pointer; Simula on button — "Keyboard
focus/activation goes to the owning surface for this hit" (`SimulaViewSprite.hs:635-641`); wayvr
on click or grab (`input.rs:479-487, 616, 647`) with follows-mouse as a config option; xrdesktop
routes the keyboard to the window the *synthesizing* controller hovers (`xrd-shell.c:1471-1478`)
— the one hover-focus case, and it is for its own virtual keyboard.
**visionOS keeps three things apart** [external]: hover (eyes; system-rendered), the *focus
system* for keyboards/controllers ("The hover effect isn't related to the focus system", HIG
Focus and selection), and pointer context: "when people shift their eyes from one window to
another, the pointer's context seamlessly transitions to the new window … If people look at an
element and then move the pointer, the system brings focus to the element under the pointer"
(HIG Pointing devices). Typing requires a tap on the field first (§9). So gaze chooses *where the
pointer materialises*; the commit (tap or pointer press) sets focus.
**Transfer.** "Focus follows the commit; gaze/hover never moves keyboard focus" is the position
of every 2D desktop's default, every XR shell, and visionOS — and Jacob's Midas-touch statement
is the reason it must be so for gaze: "Everywhere you look, another command is activated; you
cannot look anywhere without issuing a command" [external, CHI 1990]. Activation tokens carry
the commit's serial; without one they are urgency-only (niri/cosmic's reading, KWin's
`demandAttention`, mutter's pulsing indicator) — and "urgency" in space is a shell component's
presentation (notification/attention affordance), not the compositor moving anything.
**Verdict: determination** — focus-on-commit, serial-validated activation, urgency on refusal.
The one open sub-question is whether the *physical keyboard's* focus may follow gaze without a
commit; no comparable does it (visionOS requires the tap), so it is recorded as rejected.

## 5. Pointer multiplicity — two hands, one seat

**Wayland.** A seat has one pointer focus and one keyboard focus; `wl_pointer` "represents one
or more input devices, such as mice, which control the pointer location and pointer_focus of a
seat" (`wayland.xml:2024-2027`); multiple seats are first-class ("Each name is unique among all
wl_seat globals", `:1991-1997`; smithay's `SeatState` holds a `Vec<Seat>`, `input/mod.rs:346-370`);
`wl_touch` carries multiple concurrent contacts by id (`:2617-2627`); tablet tools have their own
proximity-driven focus "independent of the pointer controlled by the mouse" (`tablet-v2.xml:96-97`).
**The comparables' positions.**
- **One logical pointer, the last committing hand owns it:** kwin-vr — both controllers write
  the same synthetic device (`VrInputBindings.qml:18-21, 72-79`); xrdesktop — one primary
  controller, and "when left clicking with a controller that is *not* used to do input synth,
  make this controller do input synth" (`xrd-input-synth.c:198-205`); WiVRn — one focused
  pointer of N, switched on trigger rising edge or scroll, otherwise sticky
  (`imgui_impl.cpp:726-768`).
- **Two pointers with an ordering rule:** wayvr — two `Pointer`s, the hand with the later
  `last_click` interacts first, per-overlay primary is the lower index (`input.rs:498-506,
  582-590`).
- **Per-hand independent state:** StereoKit (focused/active per hand id, `stereokit_ui.h:199-203`),
  MRTK3 (per-interactor hover/select; two gaze-pinch interactors on one object take the centroid,
  `GazePinchInteractor.cs:374-399`), StardustXR (per-method capture).
- **Either hand, touch semantics:** visionOS — "a tap gesture with either hand generates a touch
  event on that focused item" (Adopting best practices for privacy); two-handed zoom/rotate are
  "pinch and drag together or apart" — two contacts. Android XR's `InputEvent.pointerType`
  distinguishes left/right hand.
**Transfer.** For the *pointer-class* transport (rays, mouse) the converged shape is one logical
pointer per seat handed off on commit — Wayland's model, and three XR shells' independent
choice. For the *touch-class* transport (gaze+pinch, direct poke) two hands are naturally two
contacts of one `wl_touch`, which is what visionOS's two-handed gestures already are and what
toolkits implement as two-finger zoom without any protocol invention. Two seats is legal and no
comparable uses it. **This is a fork only in combination with §2's transport question (§15
Q2).**

## 6. Window movement, resize and close

**The affordance is compositor-owned and sits at the window's edge.** visionOS [external]: a
window bar *below* the window, close button to its left, resize controls at the bottom corners,
none of it drawable by the app ("unmodifiable background material called glass and includes a
close button, window bar, and resize controls", HIG Windows); move = "Pinch and drag the window
bar side-to-side, towards you, or away from you"; resize = "Look at the bottom-right or
bottom-left corner … then pinch and drag the resize control"; close = "Look to the left of the
window bar, then tap" (Apple Support). HoloLens: "Grabbing and dragging the holobar at the top
of the 2D slate lets users move the entire slate" [external]. Meta: "click and hold the app
panel's handle, then use the scroll wheel to adjust its depth" [external]. wayvr: a 48 px title
panel plus borders as chrome (`hit_test.rs:23-24`, `overlays/wayvr.rs:80, 809-822`). StereoKit:
`ui_window_begin` with a grabbable header and `UIMove` exact / face-user (`UI.cs:1220-1292`).
xrdesktop: windows flagged `DRAGGABLE` — "expected interaction of being able to grab windows and
drag them around"; child windows and FOV/controller-attached containers are not
(`xrd-shell.c:205-236`). kwin-vr: grab anywhere with `xray.grab`, `pullGrabbed/pushGrabbed` for
depth, `turnToFaceKeepRoll` (`Xray.qml:44-97`), and "Do not start movement When we hover
something / But do not stop movement when we already started" (`XrScene.qml:283-308`).
**Depth.** Two idioms: drag toward/away (visionOS hands) and scroll/thumbstick while grabbed
(Meta mouse wheel, kwin-vr, wayvr `input.rs:951-1036`, xrdesktop `xrd-shell.c:722-841`, wxrc
Meta+axis "Move window towards/away from the camera", `input.c:372-394`). They are not
exclusive: the first is a hands idiom, the second a controller/mouse idiom.
**Face the user.** visionOS: "system windows are always oriented to face people" and "they
always turn to face someone when moving" (WWDC23 10073, 10072); StereoKit `UIMove.FaceUser`;
kwin-vr `alignGrabbedObjectToCamera`; research/36 §9 already selected this.
**Client-initiated moves.** `xdg_toplevel.move`/`resize` carry a seat and serial and "The
server may ignore move requests depending on the state of the surface" (`xdg-shell.xml:752-801`);
wxrc maps `request_move` to its spatial move (`xdg-shell.c:111-115`) and never wired
`request_resize` (pass 1 absence). A CSD title-bar drag is therefore the same operation as the
compositor's bar: begin the spatial move with the hit as anchor. `xdg_toplevel_drag` (detachable
tabs → windows) is "as if the client called xdg_toplevel.move" and not in smithay yet.
**Transfer.** Converging on: a compositor-drawn bar/handle in the plane's frame (below in
visionOS, above in HoloLens/wayvr — placement is a shell presentation choice), client `move`
requests mapped to the same spatial move, resize by corner handles or client `resize` requests
mapped to plane size (spec: the window geometry, not the plane scale — the toolkit relayouts),
depth by drag-toward/away for hands and by scroll/axis for controllers and mice, face-the-user
during a move, kwin-vr's hover-vs-move precedence rule. Detach/re-entry and the places-model
verbs are already designed (foreign-session §3.7; places-model §2). **Verdict: determination;
one presentation fork flagged (bar below vs above) that is the shell's, not the compositor's.**

## 7. The cursor

**Gaze has no cursor.** visionOS eyes: none, the hover effect is the feedback; trackpad/mouse: "A
circle-shaped pointer appears where you're looking" [external]. HoloLens eye gaze: "Don't show a
cursor … the cursor becomes quickly distracting and annoying when using eye gaze"; hand ray: a
donut cursor at the hit, dot when committing; fingertip: a donut that shrinks with proximity
[external]. **Rays have a reticle.** Meta: teardrop "pincher" mesh whose visual position is not
the ray origin ("The pointer pose origin lies close to the wrist root and is not the same as the
visual position") [external]; MRTK3 `MRTKRayReticleVisual` at the hit (`:161-166`); WiVRn a
circle, brighter when focused (`imgui_impl.cpp:921-934`); wayvr a laser line to the hit
(`input.rs:454-475`); xrdesktop `G3kCursor` at the intersection, hidden while grabbing.
**Client-supplied cursor images.** wxrc, motorcar and Simula draw the *client's* `wl_surface`
cursor (with hotspot) at the intersection on the plane (`wxrc/src/input.c:448-512`;
`sixdofpointingdevice.cpp:98-107`; `SimulaViewSprite.hs:490-537`); kwin-vr draws KWin's current
cursor texture (`VrKwinCursor.qml:20-41`). The protocol: `set_cursor` gives a surface + hotspot,
`cursor-shape-v1` gives a *name* instead — "enumerated cursors instead of a wl_surface" for "a
more consistent look and feel of the cursor across the applications" (`cursor-shape-v1.xml:27-29`;
`smithay/src/wayland/cursor_shape.rs:1-4`).
**Transfer.** Three cursor classes by input class: gaze → none (plane-level emphasis, §2);
ray/poke → a compositor reticle at the hit, sized in visual angle, plus the client's cursor *meaning*
(I-beam, resize arrows) rendered from `cursor-shape-v1` names when given, else the client's
image drawn on the plane at the hit; mouse/trackpad → the pointer-class cursor drawn on the
plane (visionOS's circle is the same thing in their theme). Sizing follows the dynamic-scale
rule — "preserve the size of the target areas, no matter where the window is positioned" (WWDC23
10073). **Verdict: determination.**

## 8. Peripherals: Bluetooth mouse, trackpad, keyboard; controllers

**How the mouse crosses planes — three positions.** visionOS [external]: gaze places it — "If
you move the mouse or trackpad while looking at a window, the pointer appears in that window";
"when people shift their eyes from one window to another, the pointer's context seamlessly
transitions". Horizon OS [external]: continuity across an *invisible extension of the panel
surface* — "When moved over empty space between or outside these panels, the cursor follows the
invisible extension of the panel surfaces". wxrc: mouse deltas rotate a **head-relative ray**
(yaw/pitch on a sphere, clamped to FOV ± 20°, `input.c:300-307`) — the pointer is a direction,
not a point on any plane; kwin-vr similarly feeds a synthetic absolute pointer with the position
limiter replaced by identity (`kwinvr.cpp:273-275`) — ADR 0013's "unbounded pointer space".
Android XR: mice supported; traversal not documented.
**The device path.** BlueZ HID → evdev → libinput; libinput has no Bluetooth-specific seat logic
(devices join via udev `ID_SEAT`/`WL_SEAT`, `libinput/src/udev-seat.c:82-99, 165-171`); pointer
acceleration profiles adaptive (default) / flat ("1:1 movement between the device and the
pointer") / custom (`doc/user/pointer-acceleration.rst:21-31, 191-194`); touchpad gestures are
libinput's, not touchscreens' (`gestures.rst:7-9`). smithay's `LibinputInputBackend` turns these
into `InputEvent`s and needs a session (`libseat`) to open devices (`backend/libinput/mod.rs:672-697`).
A mouse in XR has no screen to accelerate against — the acceleration target is angular (wxrc's
model) or plane-local (Meta's); libinput's flat profile plus a compositor gain is what a
"direction" pointer needs.
**Keyboard.** Goes to the seat's keyboard focus (§4); visionOS shows a completion overlay when a
physical keyboard connects [external]; Meta minimizes the virtual keyboard when a Bluetooth
keyboard is paired [external]; StereoKit suppresses the soft keyboard for five minutes after a
physical key (`platform.cpp:249-261`). `keyboard-shortcuts-inhibit` lets a client take the
compositor's chords, "under no obligation to disable all of its shortcuts" (the escape hatch).
**Controllers.** OpenXR actions on interaction profiles: the simple profile's `select`/`menu`
(`semantic_paths.adoc:665-686`), the aim pose as the ray (§1), `/user/head` for HMD buttons
(the Vive Pro profile is the precedent for volume/system on the head, `:985-1002`) — which is
where `hmdButtons` belongs once Monado has a generic HMD profile (today there is none; the
contract reads them through libinput, device-contract §`input`).
**Transfer.** The mouse-traversal question is a real fork with three shipping answers and
reasons that differ by hardware (gaze-warp needs eye tracking; surface extension needs planes
to be roughly coplanar; the angular ray works with nothing) — §15 Q3. The rest converges: one
libinput seat through smithay's backend, flat acceleration with compositor gain, keyboard to the
committed focus, the standard's actions for controllers.

## 9. Text fields and the keyboard

**Wayland's mechanism.** The compositor learns that a field has focus only from the client:
`zwp_text_input_v3.enable` + `commit` after the keyboard `enter` ("This request must be issued
every time the focused text input changes to a new one", `text-input-unstable-v3.xml:66-95`),
with `content_type` and `set_cursor_rectangle` ("put a window with word suggestions near the
cursor", `:266-272`); `input-method-v2` `activate`/`deactivate` and popup surfaces placed "near
the active text input area" (`input-method-unstable-v2.xml:77-108, 375-376`). smithay ships
`text_input` (focus follows the keyboard automatically) and `input_method` with a popup handler
(`wayland/input_method/…:93-104`).
**What the platforms do at the moment of focus** [external]. visionOS: "Tap a text field" → the
keyboard appears as "a separate window that people can move where they want. You don't need to
account for the location of the keyboard in your layouts" (HIG Virtual keyboards); typing is
look-and-pinch or direct touch with "up to one finger on each hand"; **Look to Dictate**: look at
the mic in a search field and speak, "If you look away from the search field while speaking, the
Dictation will end" — gaze as *intent*, consumed by the system, never by the app; text editing by
pinch-drag of the insertion point and double-tap selection. Meta minimizes the keyboard when a
physical one is paired. StereoKit's `ui_input` spawns the soft keyboard on selection and removes
it on escape/enter/other focus (`stereokit_ui.cpp:518-535`). MRTK3's keyboard must be presented
explicitly (`NonNativeKeyboard.cs:24-28`). xrdesktop shows its keyboard on an action targeting
the hovered window.
**Transfer.** research/36 §7/§9 already selected the floating focus-bound system keyboard with
the dual mode and dictation fallback; what this adds is the *trigger chain*: `text_input.enable`
→ IM `activate` → the keyboard component (a separate client, ADR 0012) is summoned into a
head-anchored place near the plane, with `set_cursor_rectangle` positioning the IM popup on the
plane; physical-keyboard presence suppresses it (Meta/StereoKit); Look-to-Dictate is a
compositor-side gaze *intent* on the shell's own mic affordance (36 §9's gaze-privacy posture).
**Verdict: determination.**

## 10. Gaze privacy and availability

**Positions.** visionOS [external]: "visionOS doesn't provide direct information about where
people are looking before they tap" (HIG Eyes); "visionOS does not share eye input with apps or
websites, or even Apple"; hover "rendered out of process from the app" (Privacy Overview). Android
XR [external]: "Android XR doesn't share raw eye tracking data with apps. Instead, the system
displays a generic hover effect"; OpenXR access needs `EYE_TRACKING_COARSE/FINE`, "considered
dangerous permissions". HoloLens [external]: raw gaze ray to apps **after** permission ("the user
needs to grant app permission"; `EyesPose::RequestAccessAsync`) — the dissent. The standard:
`XR_EXT_eye_gaze_interaction` has no trust tiering; privacy wording is about *storing or
transferring* gaze ("strongly recommended that applications … always ask the user for active and
specific acceptance"), and permission denial surfaces as `isActive = XR_FALSE`
(`ext_eye_gaze_interaction.adoc:52-73`). StereoKit exposes `Input.Eyes` when permitted; MRTK3
documents nothing. Mura's own prior ruling: gaze never to the client in the consent picker
(research/36 §9).
**Availability.** Three targets have eye tracking (research/29); Monado provides gaze from one
driver today (§1); HoloLens falls back to head gaze after "a timeout (for example, 500–1500 ms)";
MRTK3 tests assert head pose when eyes are lost; `XR_EXT_eye_gaze_interaction` distinguishes
nominal from sub-nominal tracking (`:147-158`).
**Transfer.** zxr is the OpenXR session; clients are Wayland clients and cannot bind the gaze
profile at all — so the *compositor* is the one place gaze exists, and whether it leaks is
decided entirely by what zxr forwards (§2's fork). The converging position (Apple, Android, and
Mura's own 36 §9) is that it does not, with HoloLens' permission model as the recorded
alternative for 3D clients at M2 (§11). Fallback tiers: eyes → head ray after a timeout of the
HoloLens order, surfaced as the tier change it is. **Verdict: determination for 2D clients;
the M2 3D-client permission question is recorded, decider the owner at M2.**

## 11. 3D clients (M2)

motorcar's `motorcar_six_dof_pointer` carried position + orientation and mouse-style buttons
(`motorcar.xml:159-226`, research/59 §6) — no pinch value, no grasp, no poke tip, no ready
state. The standard now has all of them: four poses and three valued gestures with `ready`
gating (`ext_hand_interaction.adoc:291-302`); MRTK3's interactor set (gaze-pinch, ray, poke,
grab) and StereoKit's per-hand state are what applications build on those; StardustXR adds
**exclusive capture** ordered by distance ("A capture is exclusive, so nobody else is in the
order", `objects/input/mod.rs:500-534`) so a client that grabbed keeps the input. **Transfer:**
`zxr-shell-v2`'s `zxr_pointer_6dof_v2`/`zxr_ray_v2` should carry the standard's shape — aim,
pinch, poke, grip poses with pinch/grasp/aim-activate values and ready flags, per hand, with
capture — rather than motorcar's mouse-shaped events. Recorded for the M2 protocol revision;
decider the owner.

## 12. Accessibility overlay (new facts only; research/42 §5 and /37 §7.4 own the rest)

visionOS [external]: Pointer Control — "You can use your eyes, head, wrist, or finger as a
pointer"; Dwell Control follows "the trackpad or system pointer" with a movement tolerance and a
dwell time; Switch Control with item scanning and point scanning ("pinpointing it with scanning
crosshairs"); AssistiveTouch accepts Bluetooth/USB pointer devices. HoloLens: dwell onset
150–250 ms + 650–850 ms; "leave before click". Meta Voice Control with a head-tracked cursor
(research/42). **Transfer:** the pointer-source tiers of §1 *are* Pointer Control's list; dwell is
a commit method on any tier (§3); switch scanning is a shell client over the seat — nothing new
in the compositor beyond exposing the source tier as a setting.

## 13. Matrix

| mechanism | motorcar / wxrc | XR shells (kwin-vr, wayvr, xrdesktop, Simula, WiVRn, Stardust) | toolkits (MRTK3, StereoKit, godot) | visionOS | Horizon / Android XR / HoloLens | 2D desktops | Wayland surface |
|---|---|---|---|---|---|---|---|
| targeting tiers | device ray / head+mouse | controller rays; kwin-vr head ray | mode manager; near discards far | eyes primary, direct secondary | Gaze > hand ray > head ray; near turns ray off | — | — |
| hover feedback | client cursor at hit | reticle/laser + client styling from motion | reticle; sticky/relaxation | system, out-of-process | Android system; HoloLens no cursor, highlight ramp | client from motion | `enter/motion`; tablet proximity |
| stabilization | Sixense EMA 0.7 | xrdesktop shake replay; others none | half-life 0.01; pinch lock; target lock | (system) | ISDK ray stabilization | — | constraint 7 |
| commit | buttons | click/trigger; pinch bool | pinch 0.75/0.25; 1.0/1.5 cm; hysteresis | pinch = tap; drag/swipe/zoom/rotate | pinch = select; held pinch = scroll/move | click | `button`/`touch.down`; gestures; axis |
| focus rule | seat focus on event | on click/grab (wayvr, wxrc, Simula) | per-hand focus/active | tap sets focus; hover ≠ focus | — | click-to-focus default; FFM delayed | keyboard focus; activation tokens |
| activation refusal | — | — | — | — | — | urgency (all four) | `xdg-activation` "may deny" |
| two hands | per-device | one pointer + handoff (3); two pointers (wayvr) | per-hand | either hand = touch; two-hand gestures | pointerType L/R | one pointer | one pointer/seat; touch ids; seats |
| move/resize/close | grab transform | grab anywhere; push/pull; bar (wayvr) | handle/header; face user | bar below, corners, close left; drag depth | holobar top; handle + wheel depth | CSD/SSD | `move`/`resize` "may ignore" |
| cursor | client surface | client cursor / reticle / laser | reticle | none (eyes); circle (trackpad) | donut / teardrop | client image / theme | `set_cursor`; `cursor-shape` |
| mouse traversal | angular ray (wxrc) | synthetic absolute (kwin-vr) | flat-screen sim | gaze-warp | surface extension (Meta) | screen | unbounded (ADR 0013) |
| text field → keyboard | — | action-triggered | select spawns soft kb | tap field; separate window; Look to Dictate | minimize when physical kb | IM | `text_input.enable` → IM `activate` |
| gaze to apps | — | ray forwarded (no eyes) | permitted (SK) | never | Android never; HoloLens after permission | — | any motion sent is gaze |

## 14. Verdicts

1. **Tiered targeting with one active mode and a near/far band — determination** (§1; Meta's
   stated hierarchy, HoloLens/MRTK/StereoKit/WiVRn/godot's near overrides, the standard's four
   poses). Mura-specific: hand aim/pinch synthesis is missing in Monado; the standard puts it in
   the runtime.
2. **Stabilize, then arbitrate nearest-hit class-aware — constraint 7 confirmed with numbers as
   stand-ins** (§2; MRTK3 sticky/relaxation; HoloLens 500–1000 ms ramp; xrdesktop replay).
3. **Pinch = select; hysteresis; the gesture vocabulary maps onto existing protocol —
   determination** (§3).
4. **Focus follows the commit, never hover; activation is serial-validated; refusal = urgency —
   determination** (§4; four desktops, four XR shells, visionOS, Jacob 1990).
5. **Compositor-owned bar/handles; client `move`/`resize` map onto the spatial operation; depth
   by drag for hands and axis for controllers/mice; face the user on move — determination** (§6).
6. **Cursor by input class: none for gaze, reticle for rays, client meaning via `cursor-shape`
   or image on the plane for pointer-class — determination** (§7).
7. **Text-field chain `text_input.enable` → IM → summoned keyboard component; physical keyboard
   suppresses; Look-to-Dictate is compositor-side intent — determination** (§9; 36 §7/§9).
8. **Gaze never reaches a 2D client — determination** (§10; Apple + Android + 36 §9; HoloLens
   the recorded dissent, relevant only to M2's permission question).
9. **3D-client input takes the standard's shape, not motorcar's — refinement of the lineage**,
   recorded for M2 (§11).
10. **Three forks remain that the comparables genuinely split on** (§15).

## 15. Questions for the owner (one item each; options are the comparables' actual positions)

**Ruled 2026-09-26** (recorded in [ADR 0013](../architecture/adr/0013-kwin-vr-disposition.md)'s
amendment; designed in [spatial-input.md](../architecture/spatial-input.md)):
- **Q1 → (a) for gaze and hands.** Gaze never drives a pointer; gaze targets, the commit device
  (pinch, trigger, HMD button, dwell) commits, and the client sees a touch. Hands are touch-class
  in every tier (gaze+pinch, hand ray+pinch, poke). Mice and trackpads drive the pointer.
- **Q2 → dissolved by Q1 for hands** (two hands are two `wl_touch` contacts — the owner's
  intuition, "the closest analogue is multi-touch"); for controllers, **one logical pointer handed
  to the controller that last committed** when controllers are the targeting source.
- **Q3 → (a) degrading to head-warp**, with the on-plane / between-planes rule: plane-local motion
  on a plane, gaze/head-warp when the look has moved, otherwise an angular ray from the head until
  it lands.
- **The tier rule** ("highest precision with eye gaze and degrade down"): with eyes, gaze targets
  whatever commits — visionOS's position, including for tracked controllers, whose triggers
  replace the pinch and sticks scroll [external, AppleInsider hands-on of visionOS 26]; without
  eyes and with controllers in hand, the controller aim ray targets, pointer-class with cursor
  (Meta, HoloLens-era MRTK, kwin-vr, wayvr, xrdesktop — inferred reasons: the laser metaphor
  predates eye tracking; a rigid aim pose has no pinch-shift; buttons and a stick map onto
  button/axis; a visible ray is the pre-commit confirmation); without eyes and with hands, the
  hand ray targets, touch-class; the floor is the head ray. Meta's opposite choice with eyes on
  (switch to the controller ray) is recorded as the dissent.
- **One specified exception to "gaze never reaches a client":** a controller stick or mouse wheel
  under gaze targeting scrolls the gazed element; Wayland scroll is `wl_pointer.axis`, so the
  compositor enters the pointer at the gaze point, sends the axis, leaves. The position is
  disclosed only on the user's scroll action — the same class of disclosure as a tap.
- **Hand aim/pinch synthesis lives in Monado** as an `XR_EXT_hand_interaction` device over
  Mercury's joints (`ht_ctrl_emu` is the shape; the standard puts stabilised aim/pinch/poke and
  `ready` on the runtime); zxr-side synthesis from `XR_EXT_hand_tracking` joints is the bridge
  only while Monado lacks it, behind the same interface. Not the perception service: it produces
  layers, never input (ADR 0008/0012).

The questions as they were put, for the record:

**Q1 — What does gaze-driven indirect input look like to a 2D client?** This decides §2, §5 and
§10 together. The tension: a Wayland client's hover styling needs `wl_pointer.motion`; sending
gaze as motion gives every app the wearer's gaze at surface resolution.
- *(a) Touch-class:* gaze + pinch becomes `wl_touch` — the client sees a position only at
  `down`, drags as touch motion, two hands as two contacts (native two-finger zoom/rotate); no
  hover styling, no tooltips, scroll by drag/flick; compositor renders plane-level emphasis.
  This is visionOS's model in Wayland terms ("a tap gesture … generates a touch event") and
  Android XR's ("spatial equivalent of tapping on a touchscreen"); gaze never leaves zxr.
- *(b) Pointer-class:* gaze drives `wl_pointer` motion continuously — hover, tooltips and
  scroll-wheel work as with a mouse; the app receives gaze. HoloLens (after permission), MRTK,
  and every ray-forwarding XR shell do this — but none of the shells has eye tracking, and the
  two platforms that do (Apple, Google) refuse it.
- *(c) Split by source:* hand/controller rays are pointer-class (they are a hand position, not
  gaze); gaze is touch-class (a). Meta's hierarchy is effectively this — ray with cursor when
  hands/controllers point, gaze targeting + pinch when eye tracking is on.
My read, labelled as such: (c). It gives 2D apps the mouse-like experience whenever a ray
exists, keeps gaze private by construction, and needs no protocol invention.

**Q2 — Two hands on the pointer-class path.** Only relevant to rays/controllers (touch-class
handles two hands as contacts).
- *(a) One logical pointer, handed to the hand that last committed:* kwin-vr, xrdesktop
  ("make this controller do input synth"), WiVRn.
- *(b) Two pointers with a precedence rule:* wayvr (later click first; lower index wins ties).
- *(c) Two `wl_seat`s:* legal, no comparable.
My read: (a) — it is also what Wayland's seat model already is.

**Q3 — How a Bluetooth mouse/trackpad pointer crosses between planes.**
- *(a) Gaze-warp:* the pointer materialises in the window the wearer looks at (visionOS). Needs
  eye tracking; degrades to head-warp without it.
- *(b) Surface extension:* the cursor follows the invisible extension of the panel it is on and
  transfers at edges (Horizon OS). Needs planes that are roughly coplanar or adjacent.
- *(c) Angular ray from the head:* mouse deltas rotate a direction; the cursor is wherever that
  ray lands (wxrc; kwin-vr's unbounded synthetic pointer). Works with any layout and no eye
  tracking; the mouse becomes a slow head ray.
These are hardware-dependent, so the honest answer may be a tier rule rather than one choice:
(a) where eyes exist, else (c), with (b) as the within-place refinement. That is my read, and it
is a judgment, not a comparable's position.

**Not asked, recorded as stand-ins in the design:** pinch thresholds (MRTK3 / StereoKit / WiVRn
numbers until measured on Mura's tracker); hover ramp 500–1000 ms; near/far band 0.18/0.22 m
(WiVRn, the one pair with both bounds); dwell 150–250 ms + 650–850 ms; eyes→head fallback
timeout 500–1500 ms; bar placement below (visionOS) vs above (HoloLens/wayvr) — a shell
presentation choice.
