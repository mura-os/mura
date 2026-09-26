# 68 — Input architecture from comparables: where input lives, and how it is built

**Date:** 2026-09-26. **What this is:** the comparables pass that precedes the input
implementation plan. [research/63](63-xr-input-focus-selection-from-comparables.md) settled what
zxr's input *does* (tiers, transports, focus); [spatial-input.md](../architecture/spatial-input.md)
rev 0 recorded it. Two architectural questions were asserted in fragments and never read for
their reasons: (1) **where** input lives — the compositor's state loop, a dedicated thread, the
OpenXR runtime, or a separate process; (2) **how it is built inside** wherever it lives — whether
input *sources* are a plugin seam, how the pipeline is cut, and what each shape costs per tick on
a battery SoC. This document reads the shipping designs that put input in each of those places
and says why they do, whether the reasons transfer to Mura, and what adopting each trades off
(AGENTS.md rule 7). Every cite is `<clone>/<path>:<line>` under `references/` (MANIFEST.json;
two clones added for this pass: `aosp-frameworks-native`, `openvr`) or marked [external] with
the source named. Consumer XR platforms appear as mechanism evidence only (rule 2). Every clone
is depth-1, so *stated* reasons come from in-tree comments, docs and headers, and from the
upstream merge requests and platform documentation where cited [external]; where a project
states no reason, the row says so. **Budget impact** (overview invariant 9): a research
document; §7 costs the shapes on zxr's tick.

## 0. Summary

- **Where the seat lives is not a fork.** Every Wayland compositor read — weston, wlroots and
  its users, smithay and its users (niri, cosmic-comp), Hyprland, gamescope, mutter, KWin, Mir —
  owns the seat, the hit test and client routing in the compositor process. The only shipping
  design with the dispatcher in a separate process is Android's InputFlinger, whose reasons (a
  Java policy layer, per-app ANR accounting, a window-info handoff from a compositor it does not
  own) do not exist here; and Wayland gives the compositor no alternative — `wl_seat` is a
  compositor global and no external-seat protocol exists (§1.1). **Determination: in-compositor,
  confirmed with reasons.**
- **The thread question is a real fork among the comparables**, 6 : 3 for the state loop
  (weston, wlroots, smithay, niri, cosmic-comp, Hyprland, gamescope) against a dedicated input
  thread (mutter, KWin, Mir). mutter's reasons are stated upstream and are the only ones stated
  anywhere: no missed libinput events, no blocked cursor when the main thread stalls, high-rate
  devices, a path to a KMS cursor plane driven without the main thread (§1.2). Every one of
  those rests on a main thread that stalls for frames (gnome-shell's JS and GL) and on a KMS
  cursor plane; zxr has neither — its steady-state tick is microseconds, its cursor is a runtime
  layer, and its XR sources are polled per frame by design. **Determination, flagged for the
  owner (§9.1): the state loop, with a measured trigger** — an input→display latency gate at
  M1 that would justify the thread if it fails.
- **What the runtime owns is settled by the standard and confirmed by every XR shell**: device
  sensing, interaction-profile binding, aim/pinch/poke synthesis, the reserved `system` click,
  per-session focus. What the shell owns: `xrSyncActions`, the hit test, the seat. **The
  system gesture is runtime-side on every platform that has one** and is already OpenXR-shaped
  (`XR_FB_hand_tracking_aim`'s `SYSTEM_GESTURE_BIT` / `MENU_PRESSED_BIT`, §3.5); Monado lacks it
  → the same bridge pattern spatial-input §10 already uses for aim/pinch. **Determination.**
- **Inside the module, the comparables agree more than the question implied.** The two seams
  every design has are (a) a *device/source* abstraction that intake backends produce into and
  (b) an *ordered policy chain* (grabs or filters) between the seat's raw events and the client.
  KWin is the fullest shape and the one that makes sources and filters *loadable plugins*
  (`InputDevice` subclasses from `gamecontroller`/`dwellclicker`; `InputEventFilter` plugins for
  sticky/slow/bounce/mouse keys, dwell click, button rebinds, EIS capture — §5.2); StereoKit
  argues the opposite for a fixed set and says why (§5.4). For zxr the XR side of (a) is the
  OpenXR action layer itself — every XR shell adds a device by adding bindings, not code — and
  the non-XR side is smithay's `InputBackend` trait it already depends on. **Determination
  (§9.4): a closed enum of source kinds + one action set, an ordered static filter list; not
  loadable plugins** — with the cost argument in §7.
- **One number that changes the plan**: on Monado, `xrSyncActions` costs **one IPC round trip
  per device**, not zero — `oxr_input.c` calls `xrt_device_update_inputs` for every static xdev
  and the IPC client sends a request and receives the reply plus the whole inputs array each
  time (§3.2). With head + two hands/controllers that is 3–5 extra RPCs per tick on top of the
  13 research/65 counted; the shared-memory inputs path the IPC header still documents no
  longer exists. Recorded as an upstream item and a budget line (§7).

## 1. The Wayland model — the seat's process and thread

### 1.1 The seat is the compositor's: the protocol reason, then the practice

`wl_seat` is a global the compositor advertises; every input event a client receives is sent by
the compositor's seat object on the client's connection. No protocol in `wayland-protocols` or
`xdg-specs` lets another process be the seat: the two things that *do* let another process
produce input — `input-method-v2`/`virtual-keyboard-v1` and libei — both terminate in the
compositor, which re-emits the events as its own seat's (§2). That is why the placement is not
argued in any compositor's tree: it is the model. What each does with libinput:

| compositor | libinput runs on | source cite |
|---|---|---|
| weston | the compositor's `wl_event_loop` (`wl_event_loop_add_fd` on `libinput_get_fd`) | `weston/libweston/libinput-seat.c:285-289` |
| wlroots | the same loop (`backend/libinput/backend.c:106-119`) | |
| smithay | a calloop `EventSource` — `LibinputInputBackend::process_events` dispatches the context and maps to `InputEvent` | `smithay/src/backend/libinput/mod.rs:707-740` |
| niri | calloop, `insert_source` on the libinput backend | `niri/src/backend/tty.rs:452-457` |
| cosmic-comp | calloop (`backend/kms/mod.rs:194-225`) | |
| Hyprland | the Aquamarine backend's loop; no input thread of its own (`Compositor.cpp:469-497`) | |
| gamescope | wlroots' libinput on the wlserver loop, or its own epoll `CLibInputHandler` "in contexts where we don't have a session and can't use the wlroots libinput stuff" | `gamescope/src/LibInputHandler.cpp:13-14`, `wlserver.cpp:2183-2188` |
| **mutter** | **a dedicated "Mutter Input Thread"** with its own `GMainContext`; events cross to the UI thread through `queue_event` → an idle source on the main context | `mutter/src/backends/native/meta-seat-impl.c:3286-3290` (thread), `:409-424`, `:452-463` (crossing) |
| **KWin** | **a `QThread` named `libinput-connection`**; the connection object is moved to it and events reach the main thread over a `Qt::QueuedConnection` | `kwin/src/backends/libinput/libinputbackend.cpp:17-25` |
| **Mir** | **a `ThreadedDispatcher` "Mir/Input Reader"**; the evdev platform notes "no threadsafety issues as everything is done from the dispatch thread" | `mir/src/server/input/default_input_manager.cpp:168-177`, `mir/src/platforms/evdev/platform.cpp:175-182` |

### 1.2 The dedicated-thread camp and its reasons

**mutter** is the only project whose reasons are written down, in the merge request that added
the thread [external, GNOME/mutter !1403 "Handle input on a thread", merged Nov 2020, Carlos
Garnacho; the Linux Plumbers 2020 slides "A Look Inside Mutter"; the GNOME Shell blog "Threaded
input adventures", Jan 2021]:

- "No missed libinput events" / "Libinput events are always dispatched ASAP, so this will mean
  less 'client bug: event processing lagging behind by XXms' messages in the journal."
- "No blocked cursor pointer" — the cursor "can visually stall" when the main thread is busy; the
  thread "sets the seed so that the MetaKmsCursorRenderer can possibly be managed there, so
  that KMS and the input threads could talk between themselves without interaction from the main
  thread."
- "Better handling of high frequency devices"; "Reuse of cursor plane (e.g. for tablets)"; "In
  general, peace of mind."
- And the author's own limits: "It does not perform KMS updates in the input thread… It does
  not change how wayland nor clutter receive the events. These are handled in the main thread,
  are still motion-compressed."

**Assumptions those reasons rest on:** a UI thread that stalls for whole frames (gnome-shell's
JavaScript and GL work run on mutter's main loop — the stall the blog describes), pointer
motion drawn by a KMS cursor plane the compositor programs, and libinput as the *only* intake.
**Transfer to zxr:** none of the three hold. zxr's state loop does no rendering in steady state
(spec §5a: passes only on commit; the flatten is microseconds — research/62 §8), the cursor is
a runtime quad layer the runtime re-samples every display frame (spatial-input §7), and the
sources that matter most — head, hands, controllers, gaze — are *polled once per frame from the
runtime* (§3), so a thread would not make them arrive sooner. What does transfer is the
*failure mode*: a client protocol storm on the state loop (research/62 §8: 300 ms/s CPU at
13.5 k commits/s) delays libinput dispatch on the same loop. That is measurable, and it is the
trigger §9.1 names.

**KWin** and **Mir** state no reason in tree (depth-1 clones; the only KWin comment concerns
not emitting `deviceRemoved` from the connection thread, `libinputbackend.cpp:35-36`). Mir's
invariant — one dispatch thread owns all devices — is a threading *discipline*, not a latency
claim.

### 1.3 The state-loop camp and its reasons

None of weston, wlroots, smithay, niri, cosmic-comp, Hyprland or gamescope states a reason in
tree either; the shape is the default of the event-loop model each is built on, and smithay's
is the one zxr inherits by dependency: `LibinputInputBackend` *is* a calloop `EventSource`
(`smithay/src/backend/libinput/mod.rs:707-740`), the same loop that owns every Wayland object
(spec §2's ruling). Two facts favour it beyond convention: calloop lets the fd source carry a
higher priority than client sources, and the one thing the thread camp gained that zxr could
want — un-stalled dispatch — is what the priority gives when the loop is not itself blocked.

**Latency evidence:** no project measured input→frame in tree; mutter's claims are qualitative.
The number Mura needs is its own (§7).

## 2. What is externalised, and why it is a protocol rather than a library

| externalised part | mechanism | who terminates it | stated reason |
|---|---|---|---|
| input method / on-screen keyboard | `input-method-v2` + `virtual-keyboard-v1` (squeekboard `README.md:22-24`; wvkbd `main.c:45-50, 1233-1258`); phosh drives the OSK over D-Bus `sm.puri.OSK0` (`phosh/src/osk-manager.c:18-19`) | the compositor's seat re-emits the keys | not stated in the OSK trees; KWin's is *operational*: it spawns the IM as a `QProcess` with its own `WAYLAND_SOCKET` and restarts it up to five times on crash (`kwin/src/inputmethod.cpp:864-925`); weston spawns and rate-limits respawn of its text backend client (`weston/frontend/text-backend.c:964-1018`) — an IM is third-party code that must not take the compositor down |
| emulated / remote input | libei (EI client) → EIS server **inside the compositor**; the portal only brokers the fd (`libei/README.md:211-241`) | mutter (`meta-remote-desktop-session.c:1279+`), KWin (`xdg-desktop-portal-kde/src/remotedesktop.cpp:517-529`), cosmic-comp (`cosmic-comp/src/libei.rs`); wlr's portal has no RemoteDesktop at all (`xdg-desktop-portal-wlr/README.md:8`) | **stated**: libei exists for "separation… distinction… control" of emulated from real input — the compositor must be able to tell them apart, gate them (portal consent) and treat them differently (`libei/README.md:32-71`) |
| accessibility transforms | *in-compositor*: KWin's sticky/slow/bounce/mouse keys and dwell click are `InputEventFilter` **plugins** ahead of the lock screen in the chain (`kwin/src/input.h:366-393`; `kwin/src/plugins/{stickykeys,slowkeys,bouncekeys,mousekeys,dwellclicker}`); mutter's keyboard/pointer a11y runs on the input thread; gnome-shell only shows state (`js/ui/status/dwellClick.js:52-81`) | the compositor | not stated; the placement follows from what the transforms need — raw events before policy |
| assistive technology | AT-SPI over D-Bus (gnome-shell's caret tracker, `js/ui/focusCaretTracker.js:33-61`) | toolkits and the AT | the desktop's standard; not an input *source* |
| Android apps (Waydroid) | the host Wayland socket bind-mounted into the container; the container is a Wayland client (`waydroid/tools/helpers/lxc.py:202-206`) | the compositor's seat | not stated |

The rule that falls out: **producers of input that are third-party or untrusted (IMs, remote
control, automation) are processes, joined by a protocol the compositor terminates; transforms
that must see raw events before policy (a11y) are in-compositor, and KWin makes them plugins.**
ADR 0012's rejection of an external decoration renderer is the same rule from the other side.

## 3. XR: what the runtime owns, what a shell owns

### 3.1 The standard's placement statements (`openxr-docs`)

- Apps *suggest* bindings per interaction profile; the runtime binds: "The bindings suggested by
  this system are only a hint to the runtime. Some runtimes may: choose to use a different device
  binding depending on user preference, accessibility settings, or for any other reason"
  (`specification/sources/chapters/input.adoc:499-505`); profile changes happen only at
  `xrSyncActions` (`:478-491`).
- Action state is frozen between syncs: "The result of calls to `xrGetActionState*`… must: not
  change between calls to `xrSyncActions`" (`input.adoc:826-830`); not focused → `isActive`
  false (`:839-842`). Sync is per frame by convention (the spec's own example, `:112`).
- `/input/system/click` "may: not be available for application use" on every profile that has
  it (`semantic_paths.adoc:716-717` and siblings).
- `XR_EXT_hand_interaction`: "The following four action poses… enable a hand and finger
  interaction model, whether the tracking inputs are provided by a hand tracking device or a
  motion controller device. The runtime must: support all of the following action subpaths"
  (`extensions/ext/ext_hand_interaction.adoc:48-61`; `ready_ext` flags `:298-338`).
- Eye gaze is an action-bound pose (`ext_eye_gaze_interaction.adoc:115-125`).
- `XR_EXTX_overlay`: "a runtime… must: separate input tracking on a per-session basis"
  (`extx_overlay.adoc:194-199`); core: "The runtime should: only give one session XR input
  focus at any given time" (`session.adoc:66-72, 598`).
- The **system gesture** has an OpenXR shape: `XR_FB_hand_tracking_aim`'s aim flags carry
  `XR_HAND_TRACKING_AIM_SYSTEM_GESTURE_BIT_FB` "System gesture is active",
  `DOMINANT_HAND_BIT_FB` and `MENU_PRESSED_BIT_FB` "System menu gesture is active"
  (`specification/registry/xr.xml:8374-8376`; the struct chains onto `xrLocateHandJointsEXT`,
  `extensions/fb/fb_hand_tracking_aim.adoc:40-60`). The runtime reports the gesture; the app is
  told to stand down.

### 3.2 Monado — the runtime's side, read

- Devices are `xrt_device` with an `xrt_input` array and `update_inputs` / `get_tracked_pose`
  (`monado/src/xrt/include/xrt/xrt_device.h:225-233, 385-433`); interaction profiles are
  generated binding tables (`state_trackers/oxr/bindings/oxr_bindings.py`,
  `oxr_generated_bindings.h.template:1-10`).
- **`xrSyncActions` loops every static device and calls `xrt_device_update_inputs`**
  (`state_trackers/oxr/actions/oxr_input.c:2045-2050`). In the IPC client that is one round
  trip per device — lock the connection, send `device_update_input`, receive the reply, then
  receive the whole `inputs[]` and `outputs[]` arrays as varlen data
  (`ipc/client/ipc_client_xdev.c:37-56, 60-70`). The `ipc_shared_memory` header still
  documents indexing `ism->inputs[]` (`ipc/shared/ipc_protocol.h:251-259`) but the struct no
  longer has that member: **inputs are not read from shared memory in this revision.** Poses
  are separate RPCs (`ipc_client_xdev.c:66-78`; batched by `xrLocateSpacesKHR`, research/65 §1).
  The simulated target registers head + left + right (`targets/common/target_builder_simulated.c:124-129`)
  → 3 update RPCs per sync there; a device with hands, controllers and eyes has more.
- Device composition is a plugin seam on the runtime side: `ht_ctrl_emu` — "Driver to emulate
  controllers from hand-tracking input" (`drivers/ht_ctrl_emu/ht_ctrl_emu.cpp:6-8`),
  `multi_wrapper` — "Combination of multiple `xrt_device`" (`drivers/multi_wrapper/multi.h:6-7,
  28-40`), builders registered in `targets/common/target_lists.c` over `xrt_builder`
  (`include/xrt/xrt_prober.h:594-651`).
- System buttons: `XR_MNDX_system_buttons` is a Monado preview extension
  (`src/external/openxr_includes/openxr/XR_MNDX_system_buttons.h`, bindings in
  `auxiliary/bindings/bindings.json:86-128`) that *exposes* system buttons to a client — the
  opposite of a reservation (research/66 §14's upstream item stands).
- Focus among clients: `ipc_client_io_blocks` (`ipc/shared/ipc_protocol.h:374-380`), filtered
  on send (`ipc/server/ipc_server_handler.c:229-291`); `set_focused_client` "UNIMPLEMENTED"
  (`:1573-1575`).
- Monado implements neither `XR_FB_hand_tracking_aim` nor any system gesture (no hit for
  `hand_tracking_aim` / `SYSTEM_GESTURE` in `src/xrt`).

### 3.3 SteamVR / OpenVR — the runtime as the shell's hit-tester

OpenVR puts overlay *input* in the runtime: `VROverlayInputMethod_Mouse` — "Tracked controllers
will get mouse events automatically" (`openvr/headers/openvr.h:4050-4053`); the overlay reads
`VREvent_MouseMove/ButtonDown/Up` from `PollNextOverlayEvent` (`:887-889, 4450-4464`);
`IsInputAvailable` is "false if system-related functionality is consuming the input stream"
(`:2593-2596`). The driver docs give the reason for the reserved click: "`/input/system/click`…
used to bring up or close the SteamVR dashboard… will not be available to applications"
(`openvr/docs/Driver_API_Documentation.md:1975-1990`). **This is the strongest "input in the
runtime" precedent**: one hit-tester for every overlay, the shell being one more overlay. Its
assumption — the runtime owns the overlays' geometry — is false for zxr, whose planes are its
own scene (spec §5a) and whose runtime (Monado) has no overlay input API. The two OpenVR-on-
OpenXR layers confirm there is nothing to inherit: xrizer answers `SetOverlayInputMethod` with
`warn_unimplemented!` and `PollNextOverlayEvent` with `false` (`xrizer/src/overlay.rs:968-1007`);
OpenComposite stores the method with `// TODO fire events` (`opencomposite/OpenOVR/Reimpl/BaseOverlay.cpp:33, 616-670`).

### 3.4 XR shells — where each polls, hit-tests and routes

| shell | intake | hit test / seat | source abstraction | how a source is added |
|---|---|---|---|---|
| wayvr / wlx-overlay-s | `session.sync_actions` then per-hand update, once per loop (`wayvr/src/backend/openxr/input.rs:214-240`) | in the shell; smithay seat (`backend/wayvr/comp.rs:8-9`) | `InputState { pointers: [Pointer; 2] }` + roles (`backend/input.rs:69-72`) | a new field/role; no plugin API |
| xrdesktop + gxr | `gxr_action_sets_poll` → `xrSyncActions` (`gxr/src/gxr-action-set.c:149-171`); shell polls in `_input_poll_cb` (`xrdesktop/src/xrd-shell.c:695-721`) | shell; synth to the primary controller (`xrd-input-synth.c`) | GObject `GxrActionSet`/`GxrAction`, per-controller objects | a new action + manifest binding |
| WiVRn lobby | `xr_session.sync_actions` in the app loop (`wivrn/client/application.cpp:1872-1875`) | shell → ImGui pointer from the aim pose (`client/scenes/lobby.cpp:544-567`) | actions on `xr::actionset` | a new action |
| kwin-vr | frame/QML-driven; `KwinVrInputDevice` is an emulated KWin `InputDevice` (`kwin-vr/src/plugins/vr/kwinvrinputdevice.cpp:16-40`); the bridge asks itself "Maybe we should pass pointer input events through kwin?" (`kwintoqquick3dinputbridge.cpp:49-50`) | QML ray–plane (`qml/VrPointerHandler.qml:1-80`) → KWin's seat | a KWin `InputDevice` subclass — the plugin seam of §5.2 | a new device object |
| motorcar | `handleFrameBegin` raycasts the scene (`motorcar/src/compositor/scenegraph/input/sixdofpointingdevice.cpp:82-93`) | compositor; `Seat` | `SixDOFPointingDevice : PhysicalNode` — a source is a scene node | a node subclass |
| Simula | Godot `_process` (`simula/Plugin/SimulaController.hs:109-110, 250-271`) | wlroots seat (`SimulaViewSprite.hs:1097-1100`) | a controller node | a node |
| zen / zwin | wlr *pointer events* mapped to a `zn_ray` (`zen/zen/input/pointer.c:14-29`); no OpenXR controller path in the clone | `wlr_seat` + `zwnr_seat` ray (`zwnroot/include/zwnr/seat.h:25-45`) | grab objects | a grab implementation |
| StardustXR | frame-driven; **server-owned OpenXR devices produce `InputMethod`s** (`stardustxr-server/src/objects/input/oxr_controller.rs`, `oxr_hand.rs`) as data variants `Tip`/`Hand`/`Pointer` (`localize.rs:29-31`); **`InputHandler`s are client-provided** via a query interface (`objects/input/mod.rs:44-80`) | the server orders methods against handlers per frame | data variant + method helper (server); handler (client) | server: a method helper; client: a handler |
| StereoKit | `update_frame` over a **fixed priority table** `hand_sources[]` (`stereokit/StereoKitC/hands/input_hand.cpp:60-110`) | app-side | `hand_source_` enum; **reason stated**: the variants exist to "distinguish true articulated vs simulated vs override" (`StereoKitC/stereokit.h:1953-1970`) | a table row + init/update functions |
| MRTK3 | XRI update | app-side | `IPoseSource` (Command pattern, `Utilities/PoseSource/IPoseSource.cs:11`), interactors per source (`MRTKRayInteractor`, `GazePinchInteractor`, `PokeInteractor`), `InteractionModeManager` enabling/disabling interactors "to ensure that… interactors for a 'controller' aren't clashing" (`InteractionModes/InteractionModeManager.cs:14, 473-514`) | a pose source / interactor + a mode |
| godot-xr-tools | node `_process` + controller signals | app-side physics ray | composable `XRToolsFunctionPointer` nodes under `XRController3D` (`function_pointer.gd:3-8, 116-172`) | another node |

**Convergence:** every XR shell polls actions once per frame and hit-tests in its own process
against its own scene; none runs input on a second thread; none receives the hit from the
runtime except SteamVR overlays (§3.3). Adding a source is adding an *action binding* in the
three shells built on OpenXR actions (wayvr, xrdesktop, WiVRn), a *node* in the scene-graph
designs (motorcar, Simula, godot-xr-tools), a *data variant* in StardustXR, a *table row* in
StereoKit, and a *plugin object* only in MRTK3 and kwin-vr (through KWin's seam).

### 3.5 Consumer platforms [external, mechanism only]

- **Horizon OS**: the system gesture is detected by the *runtime*: "If the runtime detects the
  user is performing a system gesture on either hand, the `InputStateStatus` field… will have
  the `SystemGestureProcessing` bit set… the app can suspend its own gesture processing"; the
  non-dominant hand's gesture "results in… `MenuPressed`… to signal a menu button press"
  [developers.meta.com, Hand Tracking (Native)]. Apps cannot rebind or suppress it
  (VRC.Quest.Input.8: "the system gesture is reserved"). Under OpenXR this is exactly
  `XR_FB_hand_tracking_aim`'s flags (§3.1). Horizon OS is Android; its dispatcher is
  InputFlinger's (§4).
- **visionOS**: gaze processing and hover effects are applied "outside your app's process, so
  your app doesn't know where people are looking" [WWDC25 303; WWDC24 10152: "Custom hover
  effects were designed from the ground up to preserve privacy. They're applied by the system,
  outside the app process"]; the app receives a tap as a gesture event. The system layer is the
  input authority; apps get committed events — the shape spatial-input §5/§9 already adopted in
  Wayland terms.
- **Android XR**: the dominant-hand palm-pinch opens the system menu; "When the user is
  interacting with the system navigation menu, the application will only respond to head
  tracking events"; apps read `AimFlags.SystemGesture` — the FB aim flags again [developer.android.com,
  Develop with Unity for Android XR].
- **HoloLens 2**: the Start gesture (wrist tap; one-handed palm-up pinch while looking at the
  wrist icon) is the shell's, controllable only by MDM policy (`EnableStartMenuWristTap`,
  `RequireStartIconHold`) [learn.microsoft.com, HoloLens release notes; Mixed Reality design
  "System gesture"].
- **SteamVR**: `IsInputAvailable` false while "system-related functionality is consuming the
  input stream" (`openvr.h:2593-2596`); the dashboard is the runtime's.

**Every platform recognises the reserved gesture system-side and tells the app to stand down
through a flag on the hand data.** None runs it in the app.

## 4. Android InputFlinger — the separate-process design, read for its reasons

`aosp-frameworks-native/services/inputflinger/`. The native `InputManager` is "the core of the
system event processing" and "never makes any calls into Java itself" — the Java
`InputManagerService` in `system_server` supplies policy (`InputManager.h:214-239`;
`docs/pointer_capture.md:5, 26`). The pipeline is a one-way stage chain assembled in
`InputManager.cpp:127-163`: `InputReader` → `UnwantedInteractionBlocker` (palm rejection) →
`InputFilter` → `InputDeviceMetricsCollector` → `InputProcessor` (classification through the
`IInputProcessor` HAL) → `PointerChoreographer` → `InputDispatcher`; "none of the stages share
any internal state" (`InputManager.h:219-235`). Devices enter through one `InputMapper` per
device class — cursor, keyboard, joystick, single/multi-touch, touchpad, rotary encoder, switch,
sensor, vibrator, external stylus (`reader/mapper/InputMapper.h:346-363`: "A single input device
can have multiple associated input mappers"). The dispatcher learns window geometry from the
compositor over `SurfaceComposerClient::addWindowInfosListener`
(`dispatcher/InputDispatcher.cpp:960-962`; `InputDispatcher.h:151-152, 376-404`) and enforces
ANR timeouts per window (`docs/anr.md`).

**Why it is a separate process, and why that does not transfer.** The reasons that are stated
or structural: the policy layer is Java in `system_server`; apps are untrusted processes that
must be individually accountable (ANR per window, focus-wait); the compositor (SurfaceFlinger)
is a display server with no window-management role, so window geometry has to be *handed* to
the dispatcher; and the dispatcher predates and outlives any particular compositor. Mura's
compositor *is* the window manager and the display authority (ADR 0006/0012), its clients speak
a protocol whose seat semantics assume the compositor sends the events, and there is no Java
policy layer. What does transfer is the **stage cut** — reader/mapper → blockers and filters →
classifier → dispatcher — which is the same order KWin's filter chain and Mir's filter chain
express in-process (§5).

## 5. Inside the module — source seams and pipeline shapes

### 5.1 The two seams every design has

Reading across §1–§4, each compositor has (a) a **source/device seam**: something intake
backends produce into and the pipeline consumes from; and (b) a **policy chain**: an ordered
set of things that may consume an event before the client does. The shapes:

| design | source seam | policy chain | loadable at runtime? |
|---|---|---|---|
| weston | `evdev_device` over libinput capabilities (`libinput-seat.c:93-122`) | a single active `weston_pointer_grab_interface` vtable per device class (`input.c:936+`, `2365+`) | no |
| wlroots | `wlr_input_device` + per-capability types (`backend/libinput/events.c:83-120`) | `wlr_seat_*_grab` interfaces (`types/seat/wlr_seat_pointer.c:134-189`) | no |
| smithay | **`InputBackend` / `InputEvent<B>` traits** — implemented by libinput, EI, X11 and winit backends (`src/backend/input/mod.rs:57-89, 661-721`) | `PointerGrab`/keyboard/touch grabs (`src/input/pointer/grab.rs:37+`) | compile-time generic |
| niri / cosmic-comp | smithay's; cosmic adds `InputBackendId::{Normal, Ei, VirtualKeyboard}` (`cosmic-comp/src/input/mod.rs:101-106`) | `match` arms over `InputEvent` + grabs (`niri/src/input/mod.rs:183-210`) | no |
| Hyprland | `IHID`/`IPointer`/`IKeyboard` over Aquamarine | procedural `InputManager` + `CSeatGrab` (`SeatManager.hpp:46-56`) | no |
| mutter | `MetaInputDeviceNative` on the input thread → immutable `ClutterInputDevice` | `ClutterEventFilter` list (`clutter-event.c:1253-1263`) → `meta_display_handle_event` | no |
| **KWin** | **abstract `InputDevice`** (`core/inputdevice.h:94-120`); the libinput backend's `deviceAdded` → `addInputDevice` (`input.cpp:3247, 3372`) — **and plugins call `input()->addInputDevice` themselves** (`plugins/gamecontroller/gamecontroller.cpp:94`, `plugins/dwellclicker/dwellclicker.cpp:194`) | **ordered `InputEventFilter` chain with explicit weights** `InputFilterOrder::Order` (`input.h:366-393`); "task oriented filters… As soon as a filter returns true the processing is stopped" (`input.h:397-411`); walked by `processFilters` over a copy (`input.h:164-177`) | **yes** — `KWin::Plugin` + `InputEventFilter` (`plugins/stickykeys/stickykeys.h:14`, `bouncekeys`, `slowkeys`, `mousekeys`, `buttonrebinds`, `eis/eisinputcapturefilter.h:20`) |
| Mir | `LibInputDevice` + `InputDeviceRegistry` | `EventFilterChainDispatcher` append/prepend (`server/input/event_filter_chain_dispatcher.h:31-52`) | shell-side (`miral::AppendEventFilter`) |
| Android | `InputMapper` per device class | the stage chain of §4 | HAL-pluggable classifier only |
| OpenXR / Monado | interaction profiles + `xrt_device` drivers/builders (§3.2) | the runtime's binding + `io_blocks` | drivers, yes |
| StardustXR | `InputDataType` variants + server methods | handlers ordered by distance | clients provide handlers |
| MRTK3 | `IPoseSource` + interactors | `InteractionModeManager` | Unity components |
| StereoKit | a fixed `hand_source_` table | — | **no, by stated design** |

### 5.2 KWin's order — the fullest in-process pipeline, and what it places where

`InputFilterOrder::Order` (`kwin/src/input.h:366-393`), first to last: `PlaceholderOutput`,
`Dpms`, `ButtonRebind`, `SlowKeys`, `BounceKeys`, `StickyKeys`, `MouseKeys`, `DwellClicker`,
`EisInput`, `VirtualTerminal`, `LockScreen`, `ScreenEdge`, `DragAndDrop`, `WindowSelector`,
`TabBox`, `GlobalShortcut`, `Effects`, `InteractiveMoveResize`, `Popup`, `Decoration`,
`WindowAction`, `XWayland`, `InternalWindow`, `InputMethod`, `Forward`. Read as a placement
statement: **device rebinding and every accessibility transform come first, before the lock
screen** (they must shape raw events; a locked screen must still see sticky-keys output);
**emulated input (EIS) enters as a filter at the same stage**, not as a device; the compositor's
own non-maskable chords (`VirtualTerminal`, `LockScreen`) come next; then window-management
grabs (`DragAndDrop`, `MoveResize`, `Popup`, `Decoration`); the IM sits **last before
forwarding** — it sees only what nothing else consumed. The `gamecontroller` plugin (2025) adds
a *source* through the same seam: it reads evdev itself and injects an `EmulatedInputDevice :
InputDevice` (`plugins/gamecontroller/emulatedinputdevice.h:19-41`). **This is the desktop's
existing answer to "are input methods plugins": the transforms and the extra sources are
plugins; the seat, the hit test and the forward stage are not.**

### 5.3 The OpenXR action layer as the XR source seam

Every XR shell built on OpenXR actions (§3.4) adds a device by adding suggested bindings for
its interaction profile; the runtime binds and may rebind (`input.adoc:499-505`). Hands,
controllers, gaze and (on Meta/Android XR) the system gesture all surface through it or through
extensions chained onto it. For zxr this means the XR half of the source seam already exists
outside the compositor: **one action set, one action per semantic input (aim pose, select,
grip, menu, system, gaze pose, pinch/poke values), suggested bindings per profile** — a new
controller is a bindings entry, not code. What the standard does *not* give is the tier rule
(which source targets now) and the class (touch vs pointer) — those are zxr's, over a small
fixed set of *source kinds*.

### 5.4 StereoKit's argument for a fixed set, and MRTK3's for plugins

StereoKit keeps a fixed, prioritised table of hand sources and documents the enum's purpose:
to distinguish articulated tracking from simulated hands from an override — the *kinds* are
semantically different and the code that consumes them must know which it has
(`stereokit/StereoKitC/stereokit.h:1953-1970`; `hands/input_hand.cpp:60-110`). MRTK3's
interactor plugins exist because Unity content authors add interaction *behaviours* per
project, and the `InteractionModeManager` exists because those plugins would otherwise clash
(`InteractionModeManager.cs:14`). KWin's plugins exist because distributions and users add
transforms (a11y, rebinding) and sources (game controllers) after the fact. **Transfer:** zxr
is one binary whose device set is the hardware contract's; its tier rule is the thing that
must not clash; the reasons for loadable plugins (third-party authorship, post-hoc addition)
are absent, and StereoKit's reason for a closed set of kinds (the consumer must know which
kind it has) is present in spatial-input §3's tier rule and §5's class split.

### 5.5 Accessibility as a source

KWin: in-compositor filter plugins (`DwellClicker`, `MouseKeys`, `StickyKeys`…) that also *add
a device* when they synthesise input (`DwellClickerInputDevice`,
`plugins/dwellclicker/dwellclicker.h:23`). GNOME: the same transforms inside mutter's input
thread, shell UI for state only. External trackers and switches: as libinput devices where
they are HID, otherwise as libei clients through the portal (§2). No desktop treats an a11y
source as a separate seat or process. Research/42 §5's dwell-as-commit floor is a *commit
method* in spatial-input §13, which lands in the same place — a stage between targeting and
transport, not a source.

## 6. Matrix

| comparable | intake (loop / thread / process) | synthesis owner | seat owner | external seams | per-frame vs event | source abstraction | policy-hook shape | stated reason |
|---|---|---|---|---|---|---|---|---|
| weston | loop | — | compositor | IM client (spawned) | event | evdev device | single grab vtable | none in tree |
| wlroots | loop | — | compositor | protocol helpers | event | per-capability types | grab interfaces | none |
| smithay | loop (calloop source) | — | compositor | IM/VK/EI states | event | **trait** (`InputBackend`) | grabs | none |
| niri / cosmic-comp | loop | — | compositor | IM client; cosmic = EIS server | event | smithay + enum id | match arms + grabs | none |
| Hyprland | backend loop | — | compositor | IME relay | event | interface classes | procedural + `CSeatGrab` | none |
| gamescope | loop / own epoll waiter | — | compositor | EIS waiter | event | wlroots or raw libinput | hotkeys → seat | no-session path (`LibInputHandler.cpp:13-14`) |
| mutter | **thread** | a11y in thread | compositor | IM via shell; EIS in mutter | event | native → Clutter device | filter list | **stated upstream** (§1.2) |
| KWin | **thread** (libinput) | a11y filters | compositor | IM `QProcess`; EIS filter | event | **abstract `InputDevice`, plugin-addable** | **weighted filter chain, plugin-addable** | filter docs (`input.h:397-411`); thread: none |
| Mir | **thread** | — | compositor | shell filters | event | device registry | filter chain dispatcher | dispatch-thread invariant |
| OpenXR spec | — | **runtime** (hand interaction, gaze, system gesture flags) | app/shell | — | **per `xrSyncActions`** | interaction profiles | runtime binding | hint/rebind (`input.adoc:499-505`) |
| Monado | service | runtime (`ht_ctrl_emu`) | client | `io_blocks` | per sync, **1 RPC/device** | `xrt_device` drivers/builders | binding tables | driver comments |
| SteamVR/OpenVR | **runtime hit-tests overlays** | runtime | runtime | dashboard | events from runtime | overlay input method | `IsInputAvailable` | dashboard reason (`Driver_API_Documentation.md:1975-1990`) |
| wayvr, xrdesktop, WiVRn | shell loop | runtime | shell | — | per frame | actions / per-hand structs | — | none |
| motorcar, Simula, godot-xr-tools | shell frame | — | shell | — | per frame | scene node | — | none |
| StardustXR | server frame | server | server | **client handlers** | per frame | data variants | distance-ordered handlers | query interface (`objects/input/mod.rs:44-80`) |
| StereoKit | app frame | app | app | — | per frame | **fixed table** | — | kinds differ (`stereokit.h:1953-1970`) |
| MRTK3 | app frame | app | app | — | per frame | **plugin pose sources / interactors** | mode manager | avoid clashing interactors |
| Android InputFlinger | **separate process** (+ reader thread) | HAL classifier | dispatcher (not the compositor) | Java policy | event | `InputMapper` per class | one-way stage chain | one-way, no shared state (`InputManager.h:233-235`) |
| Horizon OS / Android XR / visionOS / HoloLens [external] | system | **system/runtime** (system gesture, gaze) | system | apps get flags/events | per frame | — | reserved gesture | privacy (Apple), reservation (Meta, MS) |

**No comparable** exists for: a Wayland compositor whose libinput intake is event-driven while
its XR intake is per-frame — every XR shell has only the latter, every desktop only the former.
zxr has both; §7 prices the join.

## 7. The embedded budget of each shape on zxr's tick

- **A: state loop (as today).** libinput as a calloop fd source at a priority above client
  sources; XR actions synced once per tick after `xrLocateViews`. Cost: 0 extra threads, 0
  extra wake-ups (the fd wakes the loop that would run anyway), one `xrSyncActions` = **N_devices
  RPCs** on Monado (§3.2) + the batched pose locate zxr already makes. Latency: libinput event →
  seat = loop dispatch time (µs when idle; bounded by the worst client storm on the same loop —
  research/62 §8's 13.5 k commits/s case); XR source → display = one frame by construction.
- **B: dedicated input thread (mutter/KWin shape).** +1 thread, +1 cross-thread queue and wake
  per event (mutter's `queue_event` → idle source; KWin's `QueuedConnection`), and the pointer
  position still consumed on the state loop (mutter's own caveat: "these are handled in the
  main thread, are still motion-compressed"). Gains nothing for XR sources (polled per frame).
  Gains for libinput only when the loop is stalled — the trigger, not the default.
- **C: separate input service (InputFlinger shape).** +1 process, +1 IPC hop per event, a
  window-geometry handoff the compositor would have to publish, and every seat event still sent
  by the compositor to the client (protocol). No comparable on Wayland; the assumptions of §4
  absent. Rejected.
- **Source seam — enum vs trait objects.** N ≤ 8 source kinds (head, gaze, hand L/R, controller
  L/R, pointer device, keyboard); a closed `enum` dispatches at zero cost and lets the tier rule
  be a `match`; trait objects buy runtime addition zxr does not need (§5.4) for a vtable call
  per event — small, but the *code* cost (object lifetimes, registration, ordering) is what
  StereoKit avoided. Enum.
- **Policy chain — static ordered list (KWin's order without the plugin loader).** Reserved
  system input → lock/greeter mode → a11y transforms (dwell, sticky) → WM grabs (move/resize,
  popup, decoration, DnD) → IM → forward. Each stage is a function on a by-value event; the
  chain length is ≤ 8 and short-circuits; under a 14 k-commits/s client the chain runs only per
  *input* event, not per commit. Cost: negligible; the value is the explicit order KWin proved
  is the one that keeps a11y and the lock correct.
- **Upstream item (Monado):** batch `update_inputs` across devices in one IPC exchange (the
  shape `xrLocateSpaces` has) or restore the shared-memory inputs the header still documents;
  until then the census line is `runtime_calls_per_frame` + N_devices at M1, measured at the
  input gate.

## 8. Verdicts against what the tree already says

| existing statement | verdict |
|---|---|
| registry: "Input authority — in-compositor" | **confirmed, with reasons** (§1.1, §4): the protocol model and every Wayland comparable; the one separate-process design's assumptions are absent |
| research/59 §6: ray→seat in-process, per frame | confirmed (§3.4) |
| spatial-input §10: aim/pinch/poke on the runtime; zxr bridge until Monado has it | **confirmed and extended**: the *system gesture* is runtime-side on every platform and already OpenXR-shaped (`FB_hand_tracking_aim` flags); Monado has neither → the same bridge pattern (§3.1, §3.5) |
| spatial-input §2: sources arrive through the one session; libinput for peripherals and HMD buttons | confirmed; **refined**: the XR source seam is the action set (one action per semantic input, bindings per profile) — spatial-input §2's table is the action manifest in prose (§5.3) |
| ADR 0012: IM and emulated input external; decoration renderer never inside dispatch | confirmed and generalised: third-party producers are processes joined by protocols the compositor terminates; transforms that need raw events are in-compositor (§2) |
| spatial-input §13: switch access a shell client over the seat; dwell a commit method | **refined**: a11y *transforms* (dwell, sticky, slow) are in-compositor pipeline stages on every desktop (KWin: filter plugins ahead of the lock screen; mutter: input thread); switch *scanning UI* is a client; external trackers enter as libinput devices or libei clients (§5.5) |
| spec §2 ruling: calloop owns the thread; the wait thread is the only other | **holds** for input — determination A (§7) with the trigger of §9.1 |
| research/65 §1 census: 13 RPCs per tick | **extended**: + N_devices per `xrSyncActions` on Monado (§3.2); an upstream item |

## 9. Determinations and owner items (rule 8)

Determinations, recorded (converging comparables with reasons that transfer):

- **D1 — the seat, hit test, focus and routing live in the compositor** (§1.1, §4).
- **D2 — device sensing, binding, aim/pinch/poke synthesis and the system-gesture recogniser are
  the runtime's; zxr carries a bridge behind the same interface for what Monado lacks** — today
  `EXT_hand_interaction` values (spatial-input §10) and the `FB_hand_tracking_aim`-shaped system
  gesture flags (§3.1, §3.5). Both are Monado upstream items on ADR 0013's list.
- **D3 — third-party producers are processes over protocols the compositor terminates** (IM over
  `input-method-v2`/`virtual-keyboard-v1`, emulation over libei with zxr as EIS server); **a11y
  transforms are in-compositor stages** ahead of the lock/greeter mode (§2, §5.5).
- **D4 — inside the module**: XR sources are one OpenXR action set (new device = new bindings);
  non-XR sources are smithay's `InputBackend` (libinput, EI); zxr's own seam is a **closed enum
  of source kinds** the tier rule matches over, and an **ordered static filter list** in KWin's
  order (reserved input → mode → a11y → WM grabs → IM → forward); not loadable plugins (§5.3–5.4,
  §7). *Judgment flagged (rule 4):* enum over trait objects is the budget call; the comparables
  split on it for reasons (post-hoc third-party addition) Mura does not have.

Owner items — one each, the comparables' actual positions as the options:

**9.1 — libinput on the state loop, or on a dedicated input thread?** *Why it is a decision:*
the comparables split 6 : 3 and the only stated reasons are the thread camp's (§1.2). *Options:*
(a) the state loop as a prioritised calloop source — weston, wlroots, smithay, niri, cosmic-comp,
Hyprland, gamescope; zxr's current shape; no thread, no queue; libinput latency bounded by the
loop's worst dispatch. (b) a dedicated input thread — mutter, KWin, Mir; +1 thread and a
cross-thread queue per event; un-stalled dispatch when the main thread blocks; no gain for the
per-frame XR sources; mutter's reasons rest on a stalling UI thread and a KMS cursor plane zxr
lacks. *My read, labelled:* (a), with a **trigger** written into the M1 input gate — measured
libinput-event→`xrEndFrame` latency under the 13.5 k-commits/s client; if it exceeds one display
period, (b) is adopted for the libinput source only. *Consequence:* (a) keeps spec §2's
two-thread ruling and the research/65 wake-up budget; (b) adds a thread the budget must carry
and the mutter caveat that pointer consumption stays on the loop anyway.

**9.2 — confirm the closed-enum source seam (D4) over trait-object plugins.** *Why it is a
decision:* KWin, MRTK3 and StardustXR make sources plugins; StereoKit, niri, cosmic-comp and
Hyprland do not; the reasons on the plugin side (third-party, post-hoc) do not apply and the
reason on the fixed side (the consumer must know the kind) does. *Options:* (a) closed enum of
kinds + action set + static filter list (StereoKit's and the smithay compositors' shape, KWin's
order); (b) `InputDevice`-style trait objects registered at runtime (KWin's shape) with the
tier rule as a mode manager over them (MRTK3's shape). *My read:* (a); (b) costs registration,
lifetime and ordering code for a device set the hardware contract fixes. *Consequence:* (a) —
adding a source class not in the contract (an external eye tracker that is not an OpenXR
device) is a code change to the enum and a bindings entry; (b) — it is a plugin, at the price
above.

Nothing else forked: the system-gesture recogniser's placement (runtime, bridged) and the a11y
injection path (transforms in-compositor; emulation over libei) converged with reasons and are
recorded as D2 and D3.

### 9.3 The efficiency review (2026-09-26) — the embedded reading of §9.1 and §9.2, and the refinements it adds

The frame-path/efficiency pass reviewed this document and reached the same two reads on cost
grounds; its refinements are recorded here because they change what M1 measures.

- **§9.1, loop:** on a battery SoC the unit of cost is wake-ups, and shape B adds one per input
  event — the libinput thread wakes on the fd, then wakes the state loop again through the
  cross-thread queue (mutter's `queue_event` → idle source, KWin's `QueuedConnection`); a 1 kHz
  mouse becomes ≈ 2 000 wake-ups/s across two threads instead of ≤ 1 000 on one, with the
  pointer still consumed on the loop. Two refinements: (a) **the storm case is a back-pressure
  problem, not an input-placement problem** — the adversarial bound measured in research/67 §6
  (a callback-ignoring MAILBOX client at 25 k commits/s, 488 ms/s) is throttled only by a
  withheld `wl_buffer.release`, i.e. the open buffer-hold policy; a thread would keep libinput
  *dispatch* alive while the seat emission and hit test it feeds stay stuck behind the same
  storm. **Order: resolve the hold policy first; the §9.1 trigger then measures what remains.**
  (b) A shape cheaper than any comparable exists if wake-ups ever outweigh pointer latency:
  read the libinput fd **only at the tick**, as `xrSyncActions` already is — a 1 kHz device
  costs 90 wake-ups/s instead of 1 000, at up to one frame of client responsiveness (a client
  cannot render a mid-frame motion into the current tick's commit). No comparable does it, so
  under rule 7 it is a **rethink candidate recorded, not a default**; on an XR device the
  high-rate pointer is the rare peripheral and per-event dispatch (epoll coalesces when the loop
  is busy) is the starting point.
- **§9.2, closed enum:** the vtable call is not the cost; the infrastructure is — dlopen, a
  stable plugin ABI, runtime registration and ordering, separate `.so` page-ins, code the
  compiler cannot inline or dead-strip — all paid for third-party post-hoc addition the device
  set does not have. A11y transforms needing runtime enable/disable is a `bool` on a static
  stage. Both extensibility seams (action bindings; smithay `InputBackend`) sit *outside* zxr's
  dispatch, which is where extensibility belongs on an embedded target.
- **The larger number is neither item.** `xrSyncActions` at N RPCs per tick (§3.2) is 3–5
  round trips per tick for head + two hands/controllers — at 90 Hz, 270–450 RPCs/s, each ≈ 20 µs
  on the host and more on device cores — more than either decision above costs. It is the
  runtime's shape; the fix is the upstream batch (§7). Two zxr-side consequences: (a) **the
  quiet-mode footprint grows with the joint bridge**: until Monado reports `SYSTEM_GESTURE_BIT`
  itself, detecting the reserved palm gesture while a game is primary needs the hand-joint
  locate every tick — two more RPCs in exactly the state where zxr is meant to cost nothing
  (research/67: 8 ms/s). The runtime-side recogniser (D2) is the efficient answer; the bridge is
  the interim *with its per-tick cost recorded*, and **the bridge's sampling rate while quiet is
  an M1 measurement** (a hold gesture may not need 90 Hz; 30 Hz is the candidate). (b) **Monado
  does not honour the active action-set subset**: the device loop in `oxr_input.c:2045-2050`
  runs over every static device *before* the requested action sets are examined (`:2035-2060`),
  so passing only the active sets saves nothing today — part of the upstream item.

*Status of §9.1 and §9.2:* two independent reads (this pass and the efficiency review) converge
on (a) for both; **ruled by the owner, 2026-09-26: (a) for both** — §9.1 the state loop with the M1 latency trigger (after the buffer-hold policy is resolved), §9.2 the closed enum of source kinds and the static stage list.

## 10. Sources

Pinned (`references/`, MANIFEST.json): weston, wlroots, smithay, mutter, kwin, kwin-vr,
kwin-vr-patches, niri, cosmic-comp, hyprland, gamescope, mir, libinput, libei,
xdg-desktop-portal, xdg-desktop-portal-{gnome,kde,wlr,cosmic}, squeekboard, wvkbd, phosh,
gnome-shell, waydroid, openxr-docs, monado, openvr (new), xrizer, opencomposite, wayvr,
wlx-overlay-s, xrdesktop, gxr, wivrn, simula, zen, zwin, motorcar, stardustxr-server, stereokit,
mrtk3, godot-xr-tools, illixr, aosp-frameworks-native (new, `main` @ `4f463a6b`).
[external]: GNOME/mutter !1403 and the 2020 LPC slides / 2021 GNOME Shell blog on the input
thread; developers.meta.com Hand Tracking (Native, Unity, Unreal) and VRC.Quest.Input.8; Apple
WWDC24 10152 and WWDC25 303, HIG "Eyes"; developer.android.com "Develop with Unity for Android
XR", XR design foundations; learn.microsoft.com HoloLens release notes (Start-gesture MDM
policies) and Mixed Reality design "System gesture".
