# 66 — Native OpenXR applications beside zxr, and the reserved system input

**Research date:** 2026-09-26. **Question:** [overview.md](../architecture/overview.md) lists
"OpenXR apps" as first-class clients and the owner ruled that an application may take the
display exclusively ("analogous to fullscreen on the desktop and most games will require this",
[window-workspace-management.md §9](../architecture/window-workspace-management.md)). A native
OpenXR application — a game with its own session — is not zxr's client at all: it is a second
client of Monado. Nothing in Mura's documents says who is Monado's *primary* client, how zxr
stays presentable and reachable while a game runs, what the game sees of the wearer's input,
how it is launched and closed, and — the item that started this — **how the wearer gets the
shell back**. The owner's framing governs: zxr is a desktop environment's compositor and a
native app is what a **fullscreen game** is to GNOME/KDE — the DE gets out of the way, limits
its rendering, and keeps a reserved escape the game can never take (Alt+Tab, Alt+F4, Super).
This document reads that mechanism and its reasons in the 2D desktops first, then the runtime
(OpenXR spec, Monado), the Linux shells that already run as overlays beside games, SteamOS and
the Steam Frame as the appliance precedent, and the consumer platforms as engineering evidence.
**Boundary:** focus/hover/commit rules are the input workstream's ([spatial-input.md](../architecture/spatial-input.md));
this document adds one rule there — a reserved input that never reaches a client.
**Method (AGENTS.md rules 2, 7, 8):** lineage first (motorcar/wxrc: one client, no coexistence —
the gap is theirs too), then comparables with reasons: what each chose, why (comments, docs,
commit messages), assumptions, transfer, trade-off. Classes as research/36 §1: **A** pinned
clone read directly, **B** vendor first-party documentation, **C** press, **D** community.
**Budget impact:** a research document; the design it feeds charges zxr an overlay session and
one libmonado control connection, and *removes* zxr's per-frame rendering while a game is
primary.

## 0. Subjects

| Subject | What it is | Source | Class |
|---|---|---|---|
| mutter, KWin, wlroots, niri, cosmic-comp | the 2D desktops with a fullscreen game: direct scanout / unredirect, what still draws, reserved chords | pinned clones | A |
| gamescope + Jovian + steamos-manager + the Steam Frame image | the appliance compositor and session with a reserved Steam button | pinned clones (`archive-steam-frame` rootfs read with `btrfs restore`) | A |
| OpenXR spec | `XR_EXTX_overlay`, `XrSessionState`, `/input/system/click` | `references/openxr-docs` | A |
| Monado | multi-client compositor, IPC primary/focus/visibility, `libmonado`, `bindings.json` | `references/monado` | A |
| wlx-overlay-s/WayVR, kwin-vr, xrdesktop/gxr, WiVRn, Envision | Linux shells as overlay sessions; launchers | pinned clones | A |
| xrizer, OpenComposite | OpenVR titles as OpenXR sessions | pinned 2026-09-26 | A |
| visionOS, Horizon OS, Android XR, HoloLens/WMR, PICO, SteamVR/OpenVR | what each does *while* an immersive app runs | vendor docs; press flagged | B (C/D flagged) |
| device tiers | the contract's `hmdButtons` per target | research/42 §4.3a; `devices/valve-steam-frame` | A |

**Gaps found and left honest:** Valve is silent on what the Frame's Steam/Aux button does
*in-game* (the setup page documents Aux only as a login aid; SteamVR's controller profile is
not on the rootfs); Meta's "performance penalty" for seamless multitasking is press-only; Android
XR does not document what a Full Space app receives when the launcher opens; the exact moment
HoloLens suspends an immersive app when Start opens is not stated; PICO's current developer
portal did not surface (legacy SDK docs used).

## 1. The session model — who is primary

**Lineage.** motorcar and wxrc are single OpenXR (OpenHMD/OpenXR) clients; Monado's multi-client
compositor postdates them. **The 2D desktop.** A fullscreen game is one more toplevel; the
compositor is always the only owner of the display and decides per frame whether to composite
or to scan the game's buffer out directly (§3). **The runtime.** OpenXR: "The main session's
composition layers will always be composited first, resulting in any overlay content being
composited on top of the main application's content" (`extx_overlay.adoc:65-67`); overlays order
among themselves by `sessionLayersPlacement` (`:60-72`); "The runtime should: only give one
session XR input focus at any given time" (`session.adoc:593-605`). **Monado**: the primary is
the `active_client_index`; `primary_application` is true iff a client's thread is that index
(`ipc_server_process.c:699-707`); it is set over IPC by `ipc_handle_system_set_primary_client`
(`ipc_server_handler.c:1563-1570`) — the `libmonado` call `mnd_root_set_client_primary`
(`monado.h:271-282`) and `monado-ctl -p <id>` (`ctl/main.c:152-156, 364-366`) — and falls back on
the primary's death to "the first non-overlay active session", else idle
(`ipc_server_process.c:615-651`). **Overlay sessions are always visible and focused** in Monado
(`handle_focused_client_events`, `ipc_server_process.c:562-567`), with their own `z_order`; a
non-overlay client that is not primary is neither (`:555-560`) and its layers are dropped
(`comp_multi_system.c:282-287`). `set_focused_client` is **unimplemented**
(`ipc_server_handler.c:1572-1577`: "UNIMPLEMENTED"). **The shells**: WayVR creates its session
with `XR_EXTX_overlay`, `session_layers_placement: 5` (`wayvr/src/backend/openxr/helpers.rs:203-208`);
kwin-vr `overlayPlacement` default 20 through its Qt patch ("XR_EXTX_overlay extension allows
OpenXR applications to be rendered on top of other OpenXR applications. Transparency is also
applied", `kwin-vr-patches/…/0002-XR-OpenXR-add-support-for-XR_EXTX_overlay.patch:4-18`;
`kwinvr.kcfg:83-86`); gxr placement 1 with "TODO: session layer placement should be
configurable" (`gxr-context.c:894-904`). All three are *only ever* overlay sessions and all
three run with no game present (WayVR's README: "run alongside VR games and experiences while
having as little performance impact as possible", `README.md:9`). **The Steam Frame** does the
same with Valve's own shell: gamescope runs `--backend openvr … --vr-session-manager` and
presents Steam's gamepad UI as a SteamVR **dashboard overlay** (`valve.steam.gamepadui.fallback`,
rootfs `/usr/lib/steamos/gamescope-session:86-121`); even Desktop Mode is Plasma nested inside
that overlay (`steamos-nested-desktop:31-43`).

**Transfer.** zxr as a permanent overlay session is the shape every Linux XR shell and Valve's
appliance converge on, and it maps onto the owner's framing exactly: the game is the main
session (the fullscreen window), the shell's layers are always composited above it (the
platforms' "windows render in front of immersive content"), and zxr switches Monado's primary
over `libmonado` when the launcher starts a game — Monado's IPC hook exists for precisely this
("dashboards", `doc/packaging-notes.md:56`). The alternative — zxr as a normal session that
*yields* primary — makes zxr invisible while a game runs (non-primary non-overlay layers are
dropped) and would need a second session for the shell layers. **Trade-off:** an overlay
session's layers are always on top and always "focused" in Monado; both are what the shell
wants and both are what §4 must manage for input. Fork for the owner (§13 Q-A) with the
comparables' positions.

## 2. Launching a native app

**2D.** The launcher spawns a process; it connects to the compositor's socket; its toplevel is
placed; fullscreen is a request. **Envision** (the Linux XR launcher): on service start it writes
`~/.config/openxr/1/active_runtime.json` pointing at the profile's `libopenxr_monado.so`
(`active_runtime_json.rs:60-126`) and `~/.config/openvr/openvrpaths.vrpath` pointing at the
OpenVR-compat runtime (xrizer/OpenComposite) (`openvrpaths_vrpath.rs:82-106`), starts
`monado-service`, and injects `PRESSURE_VESSEL_IMPORT_OPENXR_1_RUNTIMES=1` into the Steam Linux
Runtime entry point so containerised titles see the runtime (`profile.rs:496-503`,
`steam_linux_runtime_injector.rs:108-122`); it uses `libmonado` only to *list* clients
(`xr_clients.rs:28-39`), never to switch primary. **OpenVR titles**: xrizer and OpenComposite
become ordinary (non-overlay) OpenXR sessions — neither requests `XR_EXTX_overlay`
(`xrizer/openxr_data.rs:125-149`; `opencomposite/DrvOpenXR.cpp:179-230`); their OpenVR
`IVROverlay` is composited *inside* the game's own session; the SteamVR dashboard and the quit
handshake are stubbed (`xrizer/overlay.rs:837-858`, `system.rs:319-320`; OpenComposite
`BaseOverlay.cpp:787-807`, `BaseSystem.cpp:808-810` "No implementation"). **Monado**: the client
finds the service through `active_runtime.json`/`XR_RUNTIME_JSON` and the socket
`$XDG_RUNTIME_DIR/monado_comp_ipc` (`CMakeLists.txt:393`; `doc/CHANGELOG.md:1747-1748`); a new
non-overlay session that activates becomes primary (`ipc_server_process.c:873-901`). **The
Steam Frame**: games run in their own cgroup scope (`STEAM_LAUNCH_WRAPPER_SCOPE=true`) with a CPU
affinity mask leaving cores to the system (`steam.service:22-28`). **Platforms**: Android XR's
OpenXR apps "launch in Full Space and must use `XR_ACTIVITY_START_MODE_FULL_SPACE_UNMANAGED`"
[B]; visionOS immersive-first apps must re-open their space themselves after a system dismissal
(Unity staff, [C]).

**Transfer.** The launcher client (ADR 0012's `foreign-toplevel-list` + `xdg-activation` client)
asks zxr to start the app; zxr spawns it as a transient systemd scope (the Frame's shape; the
same unit posture as any session component) with the runtime environment set; when the new
session activates, zxr sets it primary over `libmonado` and applies §3. An OpenVR title needs
xrizer or OpenComposite on the `openvrpaths` path — a packaging item, not a design one. **What
does not transfer:** Envision's file-writing (Mura's runtime is declared; `active_runtime.json`
is system-owned) and the PRESSURE_VESSEL detail (Steam-specific; recorded for the Steam-client
case).

## 3. Getting out of the way — unredirect's XR analogue

**2D, with reasons.** mutter composites nothing for a fullscreen window when the window actor
yields a scanout candidate — fullscreen with exactly one visible surface actor, or two where the
background is an opaque-black single-pixel buffer, or an opaque surface whose paint box matches
the window (`meta-window-actor-wayland.c:390-435`) — and no effect is in progress, no software
cursor overlaps, and **unredirect is not inhibited** (`meta-compositor-view-native.c:128-209,
282-379`); the inhibit is a refcount any shell UI takes — the overview, the message tray, the OSD
(`gnome-shell overview.js:251-254`, `messageTray.js:1149`) — and the stated reasons are the
whole design: enabling unredirection "reduces the overhead for apps like games", disabling it is
for "situations where having unredirected windows is undesirable like when recording a video"
(`compositor.c:1335-1382`). KWin's `layerCandidates` documents "fullscreen direct scanout. One or
more items cover the entire screen, and the scene itself does not have to be rendered at all"
(`workspacescene.cpp:397-399`), admits fast-updating or opaque ≤32 bpp fullscreen items
(`:262-282`, "fullscreen with low enough bandwidth requirements are always good to scanout"),
and lets tearing through only for the active fullscreen item (`compositor.cpp:847-851`);
effects can veto (`:414-416`). wlroots scans out only when the render list has exactly one entry
(`wlr_scene.c:2419-2427`); a software cursor or a lock disables it ("Direct scan-out disabled by
software cursor", `output.c:1016-1031`). niri renders a fullscreen window **above the Top layer
but below Overlay** ("Render above the top layer if we're on a fullscreen window and the view is
stationary", `scrolling.rs:2931-2943`; `niri.rs:4510-4525`). gamescope, the appliance case, never
stops painting: the game is the base plane ("Just draw focused window as normal, be it Steam or
the game", `steamcompmgr.cpp:3231-3232`), the Steam overlay has reserved layer slots so nothing
"can push out the Steam overlay or the cursor" (`:3265-3276`), and tearing is disabled while an
overlay is up (`:10560-10598`).

**The runtime.** An overlay session that submits zero layers is simply ignored for that frame
("the runtime will just ignore the overlay session for the current frame", `extx_overlay.adoc:182-191`)
— the spec-legal way to draw nothing. WayVR carries a caveat: it keeps submitting a skybox/watch
layer because "Monado freaks out if no layers are submitted" (`mod.rs:367-373`) — to be tested,
not assumed. Blend mode is the **focused client's**, taken in ascending z-order — the main
session sorts first (`z_order = INT64_MIN`, `ipc_server_process.c:555-560`;
`find_active_blend_mode`, `comp_multi_system.c:227-250`) — so a running game's opaque/blend
choice wins over the shell's passthrough. All clients receive the same predicted times
(`broadcast_timings_to_pacers`, `:396-420`); nothing pins pacing to the primary. **The shells**:
on `MainSessionVisibilityChangedEXTX` WayVR only drops its skybox (`mod.rs:219-228, 581-583`) and
xrdesktop only hides its background (`xrd-shell.c:1728-1732`); both keep drawing their windows
over the game; kwin-vr has no handler at all. **Platforms**: HoloLens is the strict case —
"holograms from multiple apps aren't composited together… Windows Mixed Reality can't overlay
applications on top of exclusive views" [B]; Horizon's seamless multitasking keeps "the universal
menu and up to three windows open during immersive experiences" [B] with a press-only
"performance penalty" [C]; visionOS hides *other apps'* windows in a Full Space and keeps the
owning app's [B].

**Transfer.** The 2D rule transfers whole: **while a game is primary, zxr submits nothing** (the
unredirect analogue — the shell's GPU pass, its texture uploads and its frame callbacks stop;
that *is* "limit its rendering"), and it resumes only for what the DEs still draw over a
fullscreen game: overlay-class surfaces (notifications, OSD, the lock scene) and anything the
wearer summons with the reserved input. What may draw *by default* over a game is a fork
(§13 Q-D): the DEs say overlay/OSD only; Horizon says up to three windows; HoloLens one. The
Linux XR shells' "keep drawing everything" is the absence of a rule, not a position — none of
them was designed as the DE. **Trade-off:** two renderers exist whenever anything is drawn over
the game; the DEs accept that cost for notifications and refuse it for everything else.

## 4. Input while a game is primary

**2D.** Keyboard focus goes to whatever the shell shows over the game (gamescope routes input to
the Steam overlay window when Steam sets `STEAM_INPUT_FOCUS`, `steamcompmgr.cpp:5228-5239`; the
game "is never told anything; it simply stops receiving events"). **The runtime.** OpenXR:
VISIBLE means "the session is not eligible to receive XR input"; "Runtimes should: make input
actions inactive while the application is unfocused"; `xrSyncActions` on an unfocused session
returns `XR_SESSION_NOT_FOCUSED` and all actions are inactive (`session.adoc:574-605`;
`input.adoc:839-843, 1344-1346`). `XR_EXTX_overlay` requires per-session input tracking
("reading the input from one active session does not disturb the input information that can be
read by another active session", `extx_overlay.adoc:194-200`). **Monado** gives overlay sessions
FOCUSED unconditionally (§1) — the game and the shell are *both* focused, both receive every
action; the tool to take input away is the per-client **`io_blocks`** (poses except head, hand
tracking, inputs, outputs — `libmonado` `mnd_root_set_client_io_blocks`, `monado.h:321-322`;
gating in `ipc_server_handler.c:229-277, 2058-2086`). Two shells use it: WayVR's `InputBlocker`
blocks other clients' inputs while the wearer hovers its overlays (`blocker.rs:33-55`), and
WiVRn demotes the game to **VISIBLE while its GUI is interactable** by overriding the session
state it reports (`stream_gui.cpp:657-665`; server `wivrn_session.cpp:835-855, 1358-1375`).
**Platforms** — all the same shape: Meta "focus awareness" ("the app continues to run but may
pause the gameplay as the input focus is lost"; `OVRManager.InputFocusLost`; store requirement
VRC.Quest.Input.4 "continue rendering when they lose focus, hide any user hands or controllers,
and ignore all hand or controller input") [B]; Meta's OpenXR statement: "the session state will
change from XR_SESSION_STATE_FOCUSED to XR_SESSION_STATE_VISIBLE" when the system UI is up [B];
SteamVR `IsInputAvailable` "would return false if the system-related functionality is consuming
the input stream", `ShouldApplicationPause` [Valve]; Valve: "We do not currently support input…
going to applications while the dashboard is visible" [Valve, GitHub]; PICO/Android/HoloLens
equivalent.

**Transfer.** The rule every platform and the spec share: **when the shell is up over a game,
the game is VISIBLE, not FOCUSED** — it keeps rendering, its actions go inactive, it hides or
idles its hands. In Monado that is `io_blocks` on the primary (WiVRn's and WayVR's mechanism,
until Monado implements `set_focused_client`), applied by zxr the moment it starts drawing over
the game and lifted when it stops. zxr's own shell components get input as they always do (the
seat is zxr's). **Trade-off:** `io_blocks` is coarse (all inputs, not a path); a finer
runtime-side focus switch is an upstream item (§14).

## 5. The reserved escape — Alt+Tab / Super / Alt+F4 as a system input

**2D, with reasons — the compositor's own keys survive whatever the game grabs, but not
uniformly.** mutter: bindings without `META_KEY_BINDING_NON_MASKABLE` are skipped while the
focused window holds a shortcuts inhibitor (`keybindings.c:1450-1457`); the *only* non-maskable
builtins are `restore-shortcuts` (default **Super+Escape**, `org.gnome.mutter.wayland.gschema.xml.in:58-60`)
and VT switching (`:2661-2676`) — Super and Alt+Tab themselves are inhibitable (`:1588-1594`);
the schema states the escape hatch: "Users can break an existing grab by using the specific
keyboard shortcut defined by the keybinding key 'restore-shortcuts'" (`:108-109`). niri never
inhibits its hardcoded binds and says why: "In a worst-case scenario, the user has no way to
unlock the compositor and a misbehaving client has a keyboard shortcuts inhibitor, 'jailing' the
user. The user must always be able to change VTs… Hardcoded binds must never be inhibited"
(`input/mod.rs:4656-4660`); `toggle-keyboard-shortcuts-inhibit` is forced uninhibitable
("must always be uninhibitable", `binds.rs:915-918`; wiki: "so a buggy application can't hold
your session hostage"). cosmic: VT switch runs *before* the inhibit gate (`input/mod.rs:2561-2572`),
everything else is gated, inhibitors auto-activate with a TODO (`keyboard_shortcuts_inhibit.rs:13-16`).
KWin is the cautionary case: *all* global shortcuts are skipped while an inhibitor is active —
including Kill Window (`input.cpp:1079-1088`; `useractions.cpp:1015`). gamescope/SteamOS: the
Steam button never enters the compositor at all — Steam reads the HID device and manipulates
its overlay window (`STEAM_OVERLAY`, `STEAM_INPUT_FOCUS`), gamescope is policy-free
(`steamcompmgr.cpp:1541-1548, 4610-4623`; the only Steam-menu hook is an uncalled XTest helper
faking Steam's Ctrl+1/Ctrl+2 chords, `wlserver.cpp:356-369`); on the Deck "Hold down the Steam
button to see a list of all available shortcuts" and every path is "Steam > …" [Valve]; the
power button goes to Steam as `steam://shortpowerpress` via `powerbuttond`, with logind told
`HandlePowerKey=ignore` (Jovian `steam.nix:124-127`; Frame rootfs
`steamos-powerbuttond.service:3-11`).

**The runtime.** OpenXR defines the button: "system — A button with the specialised meaning that
it enables the user to access system-level functions and UI. Input data from system buttons is
generally used internally by runtimes and may: not be available to applications"
(`semantic_paths.adoc:371-374`); fourteen profiles carry `/input/system/click` with "may not be
available for application use" (`:716-1487`); `khr_generic_controller.adoc:238-242` says system
buttons are "reserved for internal system usage". OpenVR is categorical at both layers:
"`k_EButton_System` — These events are not visible to applications and are used internally to
bring up the Steam Overlay or the Steam Client" [Valve wiki]; "The component path
/input/system/click is a special case that is used to summon or dismiss the SteamVR dashboard.
The value of this component will not be available to applications" [Valve driver docs]; Valve's
brand guidelines *require* "a system button (reviewed and approved by Valve) that brings up the
Steam dashboard" [Valve]. **Monado reserves nothing**: no consumer of `system/click` exists in
the state tracker or service (grep across `oxr_binding.c`, `oxr_input.c`, `targets/`);
`bindings.json` hands `/input/system` to applications on every profile that has it, and
`XR_MNDX_system_buttons` *adds* it where OpenXR omitted it ("expose system buttons for
controllers where they have been omitted", `doc/CHANGELOG.md:1283-1285`, !1903). The one
runtime-side reservation in the corpus is the Galaxy XR fork's driver: it `EVIOCGRAB`s the HMD's
top button (`KEY_POWER`) and re-exports it as a Vive-Pro `system/click`
(`monado-galaxyxr/…/galaxyxr_hmd_input.c:23-174`, research/42 §4.3a). **The shells**: WayVR binds
show/hide to left Y/B *double-click* on Touch/Index, left `system/click` on Vive, left
`menu/click` on WMR (`openxr_actions.json5:106-331`) and asks Monado for `XR_MNDX_system_buttons`
(`helpers.rs:97-100`); xrdesktop binds B/menu (`bindings_valve_index_controller.json:48-53`);
WiVRn returns its GUI on **both thumbsticks clicked** (`stream.cpp:1185-1194`) and drops the HMD
`KEYCODE_HOME` (`hid.cpp:12`).

**Platforms — the same split everywhere** [B]: Meta: "The mobile VR runtime reserves the Home,
Volume Up, and Volume Down buttons for system input. Applications will never see Home and
Volume buttons"; short press (< 500 ms) = system UI, long (> 500 ms) = recenter, double-tap =
show/hide windows; a reserved *hand* system gesture triggers the same. Apple: Crown press =
dismiss the immersive space / Home View, hold = recenter, double-click = surroundings, top+Crown
hold = force quit; "visionOS apps don't receive direct information from the Digital Crown".
Microsoft: "Apps can't react specifically to Home actions, as these are handled by the system";
Start gesture (wrist tap) or "Go to Start"; Windows button on controllers. PICO: Home short =
exit/home, 1 s = recenter, "occupied by the system and not open by default"; device-owner
reconfiguration of single/double/long press exists (`system-key-config-runtime`). Android XR:
Top button short = Launcher, long = assistant; touchpad hold = recenter, double-tap =
passthrough; controller Launcher button short/long = Launcher/Recents. SteamVR: system button =
dashboard, game keeps rendering, input taken.

**Device tiers** (research/42 §4.3a; `devices/valve-steam-frame/default.nix:36-42`): the Frame
has an HMD-body Aux (`KEY_SELECT`, Valve: "selecting options in the menu without controllers")
*and* a system button on each controller (Valve's Frame input page lists
`/input/system/{click,touch}` on `frame_controller`, while its own OpenVR rule says the click is
never delivered — unreconciled); Quest-class has no HMD-body system button (power/volume only;
the 3S's action button is `KEY_SWITCHVIDEOMODE`, a passthrough toggle) and a controller Home;
the Galaxy XR has the top button (= `KEY_POWER`) and a touchpad; Lynx R-1's R button opens the
Lynx Menu; button-less glasses have nothing. Hands-only, every platform reserves a **gesture**
(Meta palm pinch, Apple palm tap, HoloLens wrist tap, Android XR palm-inward pinch-hold). The
owner's requirement is not "no gestures" but **"no gesture that might interrupt the user
experience"** — a specificity requirement the platforms meet the same way: the reserved gesture
is gated by a posture rare during app use (palm turned toward the face; a look at the wrist or
palm) *and* a deliberate action (pinch-and-hold, tap), and the runtime tells apps to "suspend any
custom gesture processing when the user is in the process of performing a system gesture" (Meta
[B]) so an in-app gesture can neither fire it nor be fired by it. Recorded for the ruling (§13
Q-C).

**Transfer.** (1) The escape is a **physical control the application never receives**, owned by
the shell — universal, with Valve's and Meta's reasons stated and niri's "jailing" argument as
the 2D reason; KWin's gate-everything is the failure mode to avoid. (2) The 2D chord's three
verbs map onto the platforms' press-length split with one difference: **nobody quits an app with
a button press** — Alt+F4's analogue is a menu item in the shell the button summons (Navigator
"quit and return home", SteamVR dashboard, HoloLens Start → home, PICO exit) plus a force chord
(Apple Crown+top hold; Deck Steam+B long [C]); the button's short press is Alt+Tab/Super
(summon the shell), its long press is recenter on every platform. (3) Who consumes it: HMD-body
buttons reach zxr through libinput — the contract already makes the compositor own the keys
(`device-contract.md`, `hmdButtons`, A2: logind's handling inhibited); controller system clicks
reach Monado, which today gives them to the focused app — so zxr, always focused as an overlay,
receives them too, but the *game also does* until Monado reserves them (the Galaxy XR fork's
driver-level grab is the shipping shape; a state-tracker-level reservation for a designated
system client is the upstream item, §14). (4) Press-length mapping and the hands-only case are
the owner's (§13 Q-B, Q-C).

## 6. Recenter and passthrough beside a primary app

**Platforms** [B]: long press = recenter on Meta (`> 500 ms`), Apple (hold), PICO (1 s), Samsung
(touchpad hold); passthrough toggle: Apple double-click, Quest 3 side double-tap (IMU software,
research/42), Quest 3S action button, Galaxy XR touchpad double-tap. **Runtime**: recenter is a
runtime-level re-seat of `LOCAL` — `libmonado` `mnd_root_recenter_local_spaces`
(`monado.h:332-607` block); apps see `XrEventDataReferenceSpaceChangePending`; Meta additionally
`XR_OCULUS_recenter_event`. **Transfer.** With a game primary, recenter re-seats the game's
`LOCAL` too — as on every platform — and pinned places stay (research/36 §8; places-model);
passthrough while a game runs is Monado's blend-mode decision (the game's, §3) unless zxr, as the
focused overlay when the shell is summoned, requests otherwise — this is the one place the
"focused client's blend wins" rule needs care and is recorded in §14.

## 7. Closing the app — Alt+F4's analogue

**2D.** `xdg_toplevel.close` is a request the client may ignore; the compositor's kill chord is
the last resort (KWin Meta+Ctrl+Esc, itself inhibitable). **Runtime**: OpenXR EXITING "indicates
the runtime wishes the application to terminate its XR experience, typically due to a user
request via a runtime user interface. Applications should gracefully end their process" [Khronos];
Monado `xrt_session_request_exit`, then forced loss — WiVRn's Stop does exactly this with the
tooltip "Request to quit, may be ignored by the application" (`wivrn_session.cpp:1345-1395`;
`stream_gui.cpp:594-607`); OpenVR `VREvent_Quit` → `AcknowledgeQuit_Exiting()` "extends the
timeout until the process is killed" [Valve]; launching another app first tells the current one
to quit (`VRApplicationError_OldApplicationQuitting`) [Valve]. **Platforms**: Apple force quit
chord and in-app exit controls required by the HIG ("Avoid requiring people to use system
controls to reduce immersion in your experience") [B]; Horizon: quit from the Navigator's app
icon [B]; HoloLens: return home suspends, removing the tile kills ("When you remove a placed app
tile from the world, the underlying processes closes") [B]. **Transfer.** Request exit over
Monado, then kill the app's systemd scope after a timeout — the OpenVR shape with the Frame's
per-app scope; surfaced in the shell the reserved input summons, never bound to a press.

## 8. 2D planes over a running game

**2D**: only overlay-class surfaces draw over a fullscreen game by default — GNOME's notifications
and OSD (they *are* the `disable_unredirect` callers), niri's Overlay layer above fullscreen;
windows do not. **Platforms**: Horizon "up to three windows" (opt-in feature, [C] penalty);
HoloLens exactly one Follow-me window "inside an immersive app" and it "will follow you into, and
out of, an immersive app" [B]; visionOS: the immersive app's *own* windows stay, others hidden
[B]. **Transfer**: layers 5–6 (overlay, foreground) by default; planes only when summoned or
explicitly kept — fork §13 Q-D.

## 9. The two exclusivity mechanisms, one exit

Mechanism 1 (a zxr client granted the environment layer — one renderer, zxr draws everything)
and mechanism 2 (a native app as Monado's primary — two renderers, zxr submits nothing) must feel
identical to the wearer: the same reserved input summons the same shell layers, the same shell
offers "back to it / quit it", and layers 5–6 behave the same. The design states them as one
rule with two implementations.

## 10. Accessibility

Named actions, not gestures: HoloLens voice "Go to Start", "Go Home"; Apple's Crown
accessibility settings (triple-click) and Dwell/Pointer Control; PICO's device-owner remap of the
Home key; Android XR's gesture menu with Back/Launcher/Recents. **Transfer**: "summon shell",
"recenter", "quit app" are named actions the a11y stack and any switch device can trigger
(research/37, /42), never only a physical press.

## 11. Matrix

| | main/overlay model | game while shell up | button reserved from app | short | long | double/other | quit | draws over game by default |
|---|---|---|---|---|---|---|---|---|
| mutter/GNOME | compositor; direct scanout | keeps rendering; loses keyboard focus to shell UI | Super+Escape restore only; Super/Alt+Tab inhibitable | — | — | — | Alt+F4 → close request | notifications, OSD (disable unredirect) |
| KWin | compositor; layer candidates | same | **nothing** while inhibited | — | — | — | Alt+F4; Kill Window (inhibitable) | cursor, effects |
| niri | compositor; plane scanout | same | hardcoded VT/power; `allow-inhibiting=false` | — | — | — | close bind | Overlay layer only |
| gamescope/SteamOS | game base plane; Steam overlay reserved slots | keeps rendering; input to overlay via `STEAM_INPUT_FOCUS` | Steam button never enters compositor (Steam client HID) | Steam menu | shortcut list | QAM "…" | Steam menu; Steam+B long [C] | Steam overlay, notifications |
| OpenXR spec | main first, overlays by placement | VISIBLE = no input | `system` "may not be available" | — | — | — | EXITING | — |
| Monado | primary by index; overlays always visible+focused | `io_blocks` (no focus switch) | **none** (exposes; MNDX adds more) | — | — | — | `request_exit` → loss | — |
| WayVR | overlay placement 5 | `InputBlocker` on hover | asks MNDX_system_buttons | show/hide (Y/B double, Vive system) | — | — | — | everything |
| xrdesktop | overlay placement 1 | own action set | — | menu (B) | — | — | — | everything; hides background |
| WiVRn | Monado shell | demotes game to VISIBLE while GUI up | HMD HOME dropped | both thumbsticks | — | — | Stop → request_exit + loss | GUI when summoned |
| Steam Frame | Steam = SteamVR dashboard overlay | dashboard takes input | OpenVR: hidden from apps | dashboard | — | — | dashboard; `VREvent_Quit` | dashboard |
| visionOS | Full Space hides others | space **dismissed** on Crown | Crown: apps get nothing | dismiss / Home View | recenter | double = surroundings; +top = force quit | in-app exit + force quit | own windows |
| Horizon | immersive + up to 3 windows | VISIBLE, keeps rendering | Home/Volume reserved | system UI (< 500 ms) | recenter (> 500 ms) | double-tap = show/hide windows | Navigator quit | up to 3 (opt-in) |
| Android XR | Full Space | (unsourced) | system nav "anywhere, anytime" | Launcher | assistant | touchpad hold = recenter | Recents | — |
| HoloLens | exclusive view, no compositing | suspended on return home | "Apps can't react" | Start | — | — | Start → home; remove tile | one Follow-me window |
| PICO | — | (unsourced) | "occupied by the system" | exit/home | recenter (1 s) | remappable by device owner | — | nav bar |

## 12. Verdicts (lineage → confirmed / refined / contradicted)

1. **zxr is a permanent overlay session; the game is Monado's main session — determination.**
   Every Linux XR shell (WayVR, kwin-vr, xrdesktop) and Valve's own Frame shell are overlay
   sessions and nothing else; OpenXR composites overlays above the main session by definition;
   Monado keeps overlays visible and gives the shell the primary switch. The lineage had no
   second client; this is new, with three converging implementations. The owner confirms or
   contradicts in §13 Q-A.
2. **While a game is primary, zxr submits nothing** (the unredirect analogue) **and resumes for
   overlay-class surfaces and whatever the wearer summons — determination**, from the DEs'
   mechanism and stated reason ("reduces the overhead for apps like games") and the spec's
   zero-layer rule; WayVR's "Monado freaks out" caveat is the first thing the bring-up tests.
3. **When the shell is up over a game, the game is VISIBLE, not FOCUSED — determination** (spec,
   every platform, WiVRn's implementation); in Monado via `io_blocks` on the primary until a
   focus switch exists upstream.
4. **The escape is a reserved physical control the application never receives — determination**
   (Meta, Valve, Microsoft, Apple, PICO explicit; the spec's `system` semantics; niri's
   "jailing" reason on the 2D side; KWin's gate-everything as the counter-example). Its
   *ownership* in Mura: HMD-body buttons through libinput in zxr (the contract's A2); controller
   system clicks need a runtime reservation Monado lacks (§14).
5. **Short press summons the shell; long press recenters; quitting is a shell menu item plus a
   force chord, never a press — refined** from the owner's Alt+Tab/Alt+F4 analogy by the
   platforms' unanimous split. The exact mapping is §13 Q-B.
6. **Launch = spawn as a systemd scope with the runtime environment, set primary over libmonado
   when the session activates; close = request exit, then kill the scope after a timeout —
   determination** (Frame scope + affinity; WiVRn's Stop; OpenVR's quit handshake).
7. **OpenVR titles run through xrizer/OpenComposite as ordinary sessions — confirmed**; their
   dashboard/quit stubs mean *Mura's* shell provides both.
8. **Recenter re-seats the game's LOCAL too; pinned places stay — confirmed** (research/36 §8;
   every platform's long press).
9. **Layers 5–6 draw over a game; planes do not by default — refined** by the DEs (overlay/OSD
   only) against Horizon's three and HoloLens's one; §13 Q-D.
10. **Named actions for summon / recenter / quit — confirmed** (research/37, /42; HoloLens voice,
    Android XR menu, PICO remap).

## 13. Questions to the owner — one item each, the comparables' positions as the options

*Status (2026-09-26, later the same day) — all five ruled, recorded in
[native-openxr-apps.md](../architecture/native-openxr-apps.md): **Q-A** (a) always an overlay
session, "efficient and minimally taxing when it yields" (the quiet-mode bound: the frame-loop
IPC and the Wayland loop, nothing else); **Q-B** the platforms' split as written — the control
per target is named in native-openxr-apps.md §6; **Q-C** (a)+(b)+(c): the posture-gated palm
gesture on every tier, the body button where one exists, the select long-press convention kept
— the owner: "I said I was against gestures that might interrupt the user experience";
**Q-D** layer 5 always; layer 6 (the hand cutout) over games with visionOS's default (real hands
visible over immersive content) and a wearer toggle in the OSD layer; planes only when summoned
or kept per window; **Q-E** identical semantics on the body and controller controls, the
physical control per hardware target's convention.*

**Q-A — Is zxr always an overlay session, or a main session that yields to a game?**
(a) *Always overlay* — WayVR (placement 5), kwin-vr (20), xrdesktop/gxr (1), Steam-on-Frame
(gamepad UI as the SteamVR dashboard overlay). Consequence: zxr's layers are always composited
above any game (the shell rule every platform states), zxr is always "focused" in Monado (§4
handles it), and when no game runs Monado has no main session — proven to work by every shell
above. (b) *Main session that yields* — no comparable; zxr's layers would be dropped while a game
is primary unless it also held an overlay session for layers 5–6. My read, labelled: (a); (b)
has no precedent and a structural cost.

**Q-B — The press-length map of the reserved control.** The platforms' split is unanimous:
short (< ~500 ms) = summon the shell (Alt+Tab/Super), long = recenter, a second gesture
(double-tap / double-click) = show-hide windows or passthrough; **quit is never a press** — it
is a menu item in the summoned shell plus a force chord (Apple Crown+top hold; Deck Steam+B long
[C]). The owner asked for Alt+F4's analogue: the comparables' answer is "in the menu the button
opens, with a chord for the stuck case". Options: adopt the split as is; or make long press the
quit (no comparable does this; long press is recenter everywhere, including research/36 §8's
own determination). My read, labelled: the split as is, with quit in the shell and the force
chord as the kill.

**Q-C — Hands-only tier: is a hand gesture reserved as the system control where no button
exists?** Every hands-first platform reserves one (Meta palm pinch "system gesture", Apple palm
tap = Home View, HoloLens wrist tap = Start, Android XR palm-inward pinch-hold menu); Mura's
Quest-class targets have no HMD-body system button, and button-less glasses have nothing. The
owner's requirement: no gesture that might interrupt the user experience (posture-gated and
deliberate, as the platforms'). Options: (a) a reserved gesture on hands-only
tiers, as the platforms; (b) no gesture — hands-only tiers exit via the HMD-body button where one
exists (Frame Aux, Galaxy XR top, Lynx R) and have no exit on devices without one; (c) the
input floor's select-button long-press (research/42 convention: "recenter is a long press of
select") extended to a system role. My read, labelled: (b)+(c) where a body button exists; on a
device with neither, (a) is the only exit and rule 3 (never withhold what the hardware supports)
argues for offering it — flagged, not decided.

**Q-D — What draws over a primary game by default?** (a) Overlay-class only — notifications,
OSD, the lock scene (GNOME/niri; layers 5–6) — planes only when summoned; (b) up to N planes kept
(Horizon 3, opt-in; HoloLens 1 Follow-me); (c) everything (the Linux XR shells — no rule). My
read, labelled: (a), with a per-window "keep over games" opt-in the wearer sets (HoloLens's
Follow-me toggle is the precedent) rather than a count.

**Q-E — HMD-body button vs controller system button when both exist.** PICO treats them
identically ("Home button of the Controller or VR Headset"); the Frame has Aux on the body and
system on each controller; Android XR splits (top button = Launcher; controller Launcher button
= Launcher/Recents). My read, labelled: one `system` role, satisfied by whichever exists, both
identical — near a determination; flagged because Valve documents the Frame's Aux only as a
login aid.

## 14. Items that leave this document

- **Upstream (Monado):** a state-tracker reservation of `/input/system/click` for a designated
  system client (today Monado exposes it to apps and `XR_MNDX_system_buttons` widens that); a
  real `set_focused_client` (today "UNIMPLEMENTED"; `io_blocks` is the workaround). Both are
  ADR 0013's upstream-list shape.
- **Bring-up test:** an overlay session submitting zero layers on Monado (WayVR's caveat).
- **Packaging:** xrizer / OpenComposite on the `openvrpaths` path for OpenVR titles; the
  per-app systemd scope.
- **Input workstream:** the reserved-input rule (never reaches a client, on any tier) and the
  hands-only gesture question (Q-C) belong in spatial-input.md once ruled.

## 15. Source index

Pinned: `references/{mutter,gnome-shell,kwin,wlroots,niri,cosmic-comp,gamescope,jovian-nixos,steamos-manager,archive-steam-frame,openxr-docs,monado,monado-galaxyxr,wlx-overlay-s,kwin-vr,kwin-vr-patches,xrdesktop,gxr,wivrn,envision,xrizer,opencomposite}` at `references/MANIFEST.json`; the Frame rootfs read from `archive-steam-frame/…/images/rootfs.img`.
External (vendor unless marked): Apple — HIG immersive-experiences, digital-crown; support tan1e2a29e00, 118514, 118504; developer docs adding-3d-content-to-your-app, groupactivities spatial-persona, dismissimmersivespace; WWDC23 10111, WWDC24 10153; developer-forum threads 774365, 769709, 756259 (Apple engineers); [C] Unity docs/discussions. Meta — unity-focus-awareness, unity-lifecycle, VRC.Quest.Input.4, VRC.Quest.Functional.2, mobile-overlays, dg-dash, dg-vr-focus, mobile-openxr-input, mobile-openxr-actions-actionsets-bindings, mobile-vrapi-input-api, wrist-buttons, OVRInput reference; help 172903867975450, 133727602066940, 149215193811647, 1086876265726387; [C] UploadVR v69/v74. Google/Samsung — XR foundations, jetpack-xr transition/capabilities, scenecore `Scene`, compose platform, `SpaceToggleButtonDefaults`, openxr get-started/extensions, `ManifestProperty`; Samsung ANS10007517/7502/7594. Microsoft — holographic-home, hololens2-basic-usage, app-model, app-views, motion-controllers, navigating-the-windows-mixed-reality-home. PICO — PICO 4 / 4 Ultra user guides, Unity XR SDK ch. 5, `PXR_System` reference, `system-key-config-runtime`. Valve — Steam Deck help 69E3-14AF-9764-4C28, 671A-4453-E8D2-323C; Steamworks isteaminput, steamframe/{setup,input,controllers}; OpenVR wiki VREvent_t, IVRDriverInput-Overview, IVROverlay::ShowDashboard/CreateDashboardOverlay; `openvr.h`; GitHub issues 838, 1183, 1475, 878; brand guidelines PDF. Khronos — XrSessionState man page.
