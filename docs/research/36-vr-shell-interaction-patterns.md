# 36 — VR shell interaction patterns: comparative analysis

**Date:** 2026-09-23. Comparative study of how existing VR/XR shells handle the interaction
problems [component-registry.md §5](../architecture/component-registry.md) marks underspecified
or missing: initial window placement, launcher, notifications, consent/permission UI,
boundary setup, virtual keyboard, and recenter/summon. The question is **which patterns are
dominant across ecosystems and which divergences are genuine** (device class, input hardware,
posture) — so the five missing/partial shell-plane rows and the window-management policy row can
be designed from evidence rather than taste. KWin VR is *not* re-studied here: doc
[31 §2.5–2.12](31-kwin-vr.md) is cited as-is and only its shell gaps (radial menu, OSD windows,
space allocator) are newly read from the pin. Every subject is scored against
[zxr-shell-v2-composition.md §7.3](../architecture/zxr-shell-v2-composition.md) interaction
constraints 6–9 in §9.2.

Feeds: registry §5 rows *app launcher*, *notification presentation*, *in-space consent picker*,
*boundary setup UX*, *virtual keyboard*; registry §3 *window model* (initial 3D placement);
[ADR 0012 §1](../architecture/adr/0012-de-modularity-spinout-seams.md) seam choices;
[ADR 0007](../architecture/adr/0007-session-greeter-lock.md) PIN-pad/keyboard precedent;
[desktop-environment.md §5](../architecture/desktop-environment.md) XR redefinitions.

## 1. Subjects and evidence strength

Evidence classes: **A** = pinned local clone, code read directly; **B** = vendor first-party
documentation (support/dev docs); **C** = reputable press / dev-channel datamining; **D** =
community forum/anecdote (used only as corroboration, never load-bearing).

| Subject | What it is | Source | Strength |
|---|---|---|---|
| KWin VR | in-process 3D mode for KWin, daily-driven | [31-kwin-vr.md](31-kwin-vr.md) + `references/kwin-vr/src/plugins/vr/` | A |
| WayVR (ex-WlxOverlay-S) | OpenXR/OpenVR desktop overlay + Wayland app runner, the most-used Linux VR shell | `references/wayvr/` | A |
| StardustXR | Wayland-native 3D display server + client ecosystem (flatland, protostar, gravity) | `references/stardustxr-server/` + [stardustxr.org docs](https://stardustxr.org/docs/dev-setup/overview), [flatland](https://github.com/StardustXR/flatland/), [protostar](https://github.com/StardustXR/protostar), [non-spatial-input](https://github.com/StardustXR/non-spatial-input) | A (server) / B (clients) |
| xrdesktop (+ gxr, wxrd) | Collabora's desktop-mirror library for GNOME/KDE, 2019–2021 | `references/xrdesktop/`, `references/gxr/`, `references/wxrd/` | A |
| Simula | Godot/Haskell standalone VR window manager, keyboard-first | `references/simula/` | A |
| Breezy Desktop + XRLinuxDriver | AR-glasses (3DoF) virtual monitors for GNOME/KDE | `references/breezy-desktop/`, `references/xr-linux-driver/` | A |
| WiVRn (+ Monado) | streaming runtime; shell-relevant: in-headset lobby/launcher/keyboard, dashboard | `references/wivrn/` | A |
| Meta Horizon OS (Quest 3 era) | consumer standalone; v67–v81 window/Navigator churn | Meta help/dev docs (B), UploadVR/Verge (C), forums (D) | B/C |
| Apple visionOS | consumer standalone, eye+pinch-first | Apple support/HIG/WWDC (B), press (C) | B |
| Google/Samsung Android XR (Galaxy XR) | consumer standalone, Android-derived | Android dev design docs + Samsung support (B) | B |

Coverage caveats: StardustXR's shell behaviour lives in clients pinned only as the server;
client claims cite upstream docs/READMEs (B). Horizon OS "Navigator" claims are partially from
PTC datamining (C) and are flagged. Android XR recenter evidence was too weak to state (nothing
first-party found); that cell is left open.

## 2. Initial window placement

| Subject | New window appears | Second window | Follow rules |
|---|---|---|---|
| KWin VR | on the *virtual screen* at KWin's 2D placement; enters 3D only by explicit edge-barrier detach (31 §2.9) | same screen, normal 2D policy | opt-out follow mode, whole-group rigid rotation (31 §2.10) |
| WayVR | `SpawnPos::{Fixed, FixedNoRealign, Spread, Parent}` — new app windows `Spread` (auto spread-out) or offset from the last-spawned panel (`wayvr/src/backend/task.rs:102-111`, `windowing/manager.rs:987-1017`) | offset from parent/most recent | working set re-centers head-relative on show (README "Working Set") |
| StardustXR | flatland panel at the launcher-icon's drag-out position; hexagon launcher itself at a fixed head-relative offset (`gravity 0 0 -0.3`, [startup script](https://stardustxr.org/docs/dev-setup/startup-script)) | wherever dragged | none; `black-hole` minimizes |
| xrdesktop | mirrors the 2D desktop's own positions into 3D; explicit "arrange sphere" / "arrange reset" buttons (`xrdesktop/src/xrd-shell.c:1100-1121`) | as mirrored | none |
| Simula | head-relative in front, small downward offset, **oriented toward gaze**, placement deferred until surface dimensions stabilize (`SimulaViewSprite.hs:190-267`); config declares five starting-app slots — center/right/bottom/left/top relative to the first app (`config/config.dhall`) | adjacent slot or default offset | none automatic; `grabWindows` summons all |
| Breezy | virtual monitor(s) at configured distance/size; multi-monitor wrap schemes (`ui/data/com.xronlinux.BreezyDesktop.gschema.xml`: `monitor-wrapping-scheme`, `monitor-spacing`) | wrapped beside the first | smooth-follow state machine (§9.2) |
| WiVRn lobby | GUI panel at **0.5 m** in front of the head, yaw-facing the user, pitch from a gaze-elevation lookup table (`client/constants.h:103`, `lobby.cpp:114-141`) | n/a (single GUI; keyboard/popup are child layers at fixed offsets) | summon-on-gesture, not follow |
| Horizon OS | into the **dock**: three "hinged" side-by-side windows angled toward the user; up to three more detachable free-floating (six total) — v67 experimental, v69 default ([UploadVR v67](https://www.uploadvr.com/quest-v67-freely-position-windows-quest-pro-eye-tracking-wi-fi-qr-codes/), [v69](https://www.uploadvr.com/quest-v69-update/)) | next dock slot | dock repositions as a unit; free windows stay |
| visionOS | centered in front of where the user is looking, ~arm's-length+; apps **cannot set position**, only `defaultSize` ([Varrall](https://varrall.substack.com/p/windowing-on-the-vision-pro), [Apple support](https://support.apple.com/en-mo/guide/apple-vision-pro/dev009366408/visionos)) | opens in front; prior windows keep world positions | none; windows are world-fixed until recenter |
| Android XR | Home Space panels launch **1.75 m** from the user, default 1024×720 dp, clamped 385×595–2560×1800 dp ([Android XR foundations](https://developer.android.com/design/ui/xr/guides/foundations)) | side by side; "Tidy" button re-arranges all ([Samsung navigation](https://www.samsung.com/us/support/answer/ANS10007517/)) | none documented |

Prose: the dominant shape is **head-relative spawn in front of the user at a fixed comfortable
distance (0.5–1.75 m), facing the user**, with the *second* window placed adjacent rather than
stacked — either by a dock/slot structure (Quest, Simula's slots, Breezy's wrap) or by a
free-position search (WayVR `Spread`; KWin VR's `SpaceAllocator3D` does an
azimuth/elevation angular-overlap search at fixed radius with angular spacing —
`spaceallocator3d.cpp:139-200` — used for screens, per-session only). Two systems document
numeric defaults (WiVRn 0.5 m; Android XR 1.75 m + dp size bounds); everyone else leaves distance
to config. Only visionOS forbids apps from choosing placement — a hard policy Mura should
copy (matches ADR 0012 §2: placement policy never delegated to clients). Simula contributes the
one subtle mechanism nobody else has: **defer placement until the surface's committed dimensions
are stable for N frames**, avoiding placing against a first-frame configure size
(`spriteReadyToMove`, `SimulaViewSprite.hs:213-233`).

## 3. Launcher pattern

| Subject | Pattern | XDG desktop-entry consumption |
|---|---|---|
| KWin VR | none XR-specific — Plasma's kickoff lives on the virtual screen; the fork's `RadialMenu.qml` is a *system-action* menu (Park Ray / Recenter / Grab All / Follow / Blend), not an app launcher (`qml/XrScene.qml:218-231`) | via Plasma (indirect) |
| WayVR | dashboard tab with app grid + per-app launch options: native-Wayland vs `cage` (Xwayland kiosk), resolution presets (480p–1440p), aspect, pin-to-favourites, autostart (`dash-frontend/src/views/app_launcher.rs:397-483`) | **yes, own scanner**: WalkDir over applications dirs, INI parse, icon+category extraction, command/category blocklists (`wlx-common/src/desktop_finder.rs`) |
| StardustXR | `protostar` hexagon-launcher: hex grid of icons; **drag-and-drop launching** — drag an icon out and the app spawns there; pinch-drag on hand tracking ([protostar README](https://github.com/StardustXR/protostar)) | yes (protostar library) |
| xrdesktop | none — mirrors the desktop; the desktop's own launcher appears as a mirrored window | via host DE |
| Simula | `launchAppLauncher` (Meta+A) literally spawns **`synapse`**, a 2D desktop launcher app, head-relative (`SimulaServer.hs:322-327`); plus dhall-configured app shortcuts and five starting-app slots | via synapse (indirect) |
| Breezy | none (host DE's launcher on the virtual monitor) | via host DE |
| WiVRn | in-headset grid of app tiles (small/med/large icons) (`client/scenes/app_launcher.cpp`); list produced server-side from **XDG desktop entries** (own INI parser following the desktop-file-id spec) **plus Steam library scan** (`common/application.cpp:133-241`) | **yes, own scanner + Steam** |
| Horizon OS | Universal Menu dock/taskbar → **Navigator** (v77 PTC): overlay launcher on the Meta button / look-at-palm pinch, Library with up to 10 pinned apps, notifications/quick-controls integrated; **v81 reverted the default back to Universal Menu** ([UploadVR](https://www.uploadvr.com/meta-teases-next-evolution-quest-horizon-os-navigator-system-ui/), [datamining C](https://www.uploadvr.com/luna-quest-77-ptc-datamining/), UX complaints [D](https://www.androidcentral.com/gaming/virtual-reality/meta-quest-new-navigator-ui-how-to)) | n/a (store model) |
| visionOS | Home View: floating paginated icon grid, Digital-Crown press or gesture; folders; jiggle-mode rearrange; iPad apps auto-grouped in "More Apps" ([Apple support](https://support.apple.com/en-euro/118506)) | n/a |
| Android XR | Launcher overlay on Top button or palm-gesture pinch: pinned quick panel + paginated Apps tray with search, drag-rearrange, Play Store ([Samsung](https://www.samsung.com/us/support/answer/ANS10007594/)) | n/a (Android model) |

Prose: every platform converged on a **flat 2D grid/tray of app icons summoned by one reserved
gesture or button** — nobody ships a spatial-metaphor launcher (no rooms of icons, no 3D
shelves); protostar's hexagon grid is still a flat grid, its one novelty being *drag-out-to-place*
(launch position = drop position, collapsing "launch" and "initial placement" into one gesture —
directly relevant to §2). All three OSS shells that own launching (WayVR, WiVRn, protostar)
independently wrote XDG desktop-entry scanners; WayVR's is the most complete (icons, categories,
blocklists, per-app launch profiles) and is the best code precedent for the registry's launcher
row (desktop-entry + icon-theme consumption per the
[design-backlog scope note](../architecture/design-backlog.md)). The Navigator v77→v81 revert is
the field's most expensive recent lesson: relocating system functions between "dock as windows"
and "overlay launcher" mid-product, with hide/show semantics split across single/double button
presses, confused users enough that Meta rolled the default back — shell-mode structure is hard
to retrofit (feeds constraint 8's one-state-machine argument, §9.2).

## 4. Notifications

| Subject | Presentation | Interruption model / DND |
|---|---|---|
| KWin VR | Plasma notification windows appear on the virtual screen quad (world/screen-anchored, not special-cased); **OSD-type windows are re-parented under the `XrCamera`** — hard head-lock, above center, closer, non-pickable (`qml/XrScene.qml:165-175`, `VrOsdWindows.qml`, filter on `WindowType::OnScreenDisplay`, `windowmodelfilter.cpp:147-165`) | Plasma's own DND |
| WayVR | mirrors desktop notifications by **D-Bus `BecomeMonitor`** on `org.freedesktop.Notifications` (eavesdrop fallback) — it is *not* the server; also speaks the XSOverlay UDP protocol (port 42069) for VR-ecosystem apps (`wayvr/src/subsystem/notifications.rs:48-152`) | per-topic routing: `Hide` / `Center` (head-follow with lerp 0.1 at (0,−0.2,−0.5) m) / `Watch` (left wrist) (`overlays/toast.rs:178-205`); 3–5 s timeout, optional sound; DND via a swaync-style D-Bus client (`subsystem/dbus/notifications.rs`) |
| StardustXR | none | — |
| xrdesktop | none (desktop mirror carries them) | host DE |
| Simula | none (HUD is i3status system stats, `config/HUD.config`) | — |
| Breezy | host DE's, on the virtual monitor | host DE |
| WiVRn | in-stream ImGui toasts with fade (`constants::stream::fade_delay`) | n/a |
| Horizon OS | toast pop-ups; grouped by source; actions from the toast ([Meta help](https://www.meta.com/en-gb/help/quest/1507132083785589/)) | **immersion-aware DND**: v49 added DND that silences non-critical toasts *only inside immersive apps* — critical ones (battery, party invite) still show ([Meta blog](https://www.meta.com/blog/meta-quest-v49-do-not-disturb-family-center-abstract-home/)); v68 made DND duration-based; per-app toggles ([help](https://www.meta.com/help/quest/127824822656024/)) |
| visionOS | notifications surface near the user; **"look to see notifications"** — glance expands, pinch-hold acts without opening the app (visionOS 27, [MacRumors](https://www.macrumors.com/2026/09/14/apple-releases-visionos-27/)) | Focus modes (iOS-inherited) |
| Android XR | notification inbox inside the Launcher overlay panel, alongside Quick Settings ([Samsung](https://www.samsung.com/us/support/answer/ANS10007517/)) | Android notification model |

Prose: three presentation families exist — **head-locked/head-follow transient toast** (WayVR
Center, Quest, kwin-vr OSD), **wrist/body-anchored summary** (WayVR Watch; the watch itself
carries battery/status), and **pull-model inbox in the launcher overlay** (Android XR, Navigator).
No platform world-anchors notifications near their app. The two load-bearing convergences: (a)
**do-not-disturb is coupled to immersion state**, with a *critical-class exemption* that bypasses
it (Quest's battery/party-invite carve-out is exactly the shape Mura's
boundary/tracking-loss notifications need); (b) small transients are allowed to be hard
head-locked (kwin-vr parents OSDs to the camera with zero smoothing) — the comfort discipline of
constraint 6 is applied to *large* surfaces, not toasts, everywhere. WayVR's monitor-not-server
choice is a compat trick Mura doesn't need (we own the session and will run the spec
service per registry §6), but its **per-topic routing table** (each toast topic → Hide/Center/
Watch) is the right policy shape for the presentation row.

## 5. Consent / permission UI

| Subject | Pattern | Evidence |
|---|---|---|
| KWin VR / xrdesktop / Breezy | none XR-specific — portal/polkit dialogs are ordinary windows on the (virtual) screen | A |
| WayVR | inherits the *desktop's* portal flow at startup — the user must answer a PipeWire screen-share picker per screen, guided by desktop notifications/terminal ("check your notifications … select the screens in the order it requests", README First Start) — a documented pain point | A |
| WiVRn | pairing consent = **6-digit PIN displayed dashboard-side, typed in-headset** on the lobby keyboard (`client/scenes/lobby_gui.cpp:150-167`, `dashboard/wivrn_server.cpp:335`) | A |
| Horizon OS | Android runtime-permission model; requesting e.g. `RECORD_AUDIO` launches a **system permission activity over the app — the immersive activity is paused** (`onPause`/`InputFocusLost`) and cannot prevent it; Meta's own design guidance: "Render permission requests in-situ … DON'T have permissions render in the gray void" and don't ask at app launch ([Designing for privacy](https://developers.meta.com/horizon/design/safetyprivacy/), [keyboard-overlay focus loss](https://developers.meta.com/horizon/documentation/unity/unity-keyboard-overlay/) documents the same focus-steal mechanism) | B |
| visionOS | per-feature permission dialogs (camera/Persona, microphone, **Surroundings**, hand structure); Settings ▸ Privacy & Security review; always-on **green/orange indicator dots** for camera/mic use ([Apple support](https://support.apple.com/en-sg/guide/apple-vision-pro/tane3312ebe/visionos)); interaction is the standard look-and-pinch on a system-layer dialog (gaze data itself never reaches apps — selection is delivered only on pinch) | B |
| Android XR | Android runtime permissions; per-app Permissions page in the launcher's app management ([Samsung](https://www.samsung.com/us/support/answer/ANS10007594/)) | B |

Prose: verified as asked — on Quest a permission request **does** yank the user into a system
layer and pauses the immersive app; Meta's design docs treat this as a known cost and push
developers to pre-contextualize the request ("in-situ") because the system dialog itself cannot
be re-styled or embedded. The consumer platforms agree on three properties Mura should
treat as normative: (1) permission UI is rendered by a **system layer no app can draw over or
imitate**; (2) sensor-use is disclosed by a **persistent trusted indicator** (visionOS dots ≙
our active-share badges row, registry §5); (3) consent prompts *withdraw input from the app*
while shown (the focus-steal is the security feature, not a bug — matching lock invariant I1's
"seat routes only to the trusted scene" shape, ADR 0007). visionOS's "look-to-approve" is not a
distinct mechanism — approval is ordinary gaze+pinch — but its *gaze privacy* posture (dwell
highlighting rendered out-of-process, gaze never delivered to the app) is the part worth copying
into the consent-picker design. WiVRn's PIN pairing is the OSS proof that ADR 0007's
PIN-pad-in-headset flow works for *device-to-device* consent too.

## 6. Boundary / guardian setup UX

| Subject | Pattern |
|---|---|
| Horizon OS | **auto-created stationary boundary** (1×1 m, floor height set) on entering VR; user-drawn **roomscale** boundary via controller-on-floor painting; separate saved preference per VR-home vs apps; boundary suppressed while in passthrough ([Meta help](https://www.meta.com/help/quest/463504908043519/); flapping/confusion evidence D: [forums](https://communityforums.atmeta.com/discussions/OtherTroubleshooting/quest-3-suddenly-wont-saveuse-roomscale-boundaries/1302331)) |
| visionOS | **no user boundary at all**: system defines ~1.5 m radius from initial head position for progressive/full immersion; approaching it fades content into passthrough, crossing it replaces the app with its icon; moving faster than a brisk walk force-clears immersion; **none of it configurable** ([Apple support](https://support.apple.com/guide/apple-vision-pro/adjust-immersion-tan899d290e4/visionos), [HIG](https://developer.apple.com/design/human-interface-guidelines/immersive-experiences)) |
| Android XR | preset **Small/Large boundary sizes** + explicit "Adjust floor level" step; in Settings, not a first-run draw ritual ([Samsung](https://www.samsung.com/us/support/answer/ANS10007517/)) |
| StardustXR | none of its own — re-exports the OpenXR STAGE bounds over D-Bus (`org.stardustxr.PlaySpace` property `bounds`, `src/objects/play_space.rs:134-150`) for clients to draw |
| WayVR / KWin VR / Simula / xrdesktop / WiVRn client | none — defer to the runtime (Chaperone/Monado); WayVR has playspace *drag/reset* (moving the stage), not boundary editing |
| Breezy | n/a (seated 3DoF glasses) |

Prose: this is the **no-convergence topic** (§9.1). Three shipped philosophies — draw-it-yourself
(Quest), auto-zone-you-cannot-touch (visionOS), preset-plus-floor-confirm (Android XR) — and the
entire OSS field ships nothing, treating boundary as runtime property. The divergence tracks
device posture honestly: roomscale gaming hardware needs user-authored fences; a seated
productivity device with high-quality passthrough can replace fences with fade-to-passthrough;
Android XR splits the difference. Note also *where* enforcement feedback lives: Quest draws a
grid fence (world overlay), visionOS *dims the app itself* — the latter is exactly our
compositor-owned breach response (spatial-mapping §7). Quest's dual saved preferences
(home vs apps) generating user-visible mode flapping is a §9.2 constraint-9 data point:
two stores of the same policy confuse users.

## 7. Virtual keyboard

| Subject | Layout/placement | Input methods | Dictation |
|---|---|---|---|
| Horizon OS | system keyboard, floats in FOV; **Near/Far suggested placements from the OpenXR virtual-keyboard API**; user-movable/scalable ([VK sample](https://developers.meta.com/horizon/documentation/unity/VK-unity-sample/)) | ray+trigger, **swipe typing** (hold and sweep over letters, both ray and direct), direct touch with **poke limiting** (hand model stopped at key plane), readout preview bar ([VK design](https://developers.meta.com/horizon/design/virtual-keyboard/)); tracked physical keyboard rendered via **passthrough cutout** ([tracked kb](https://developers.meta.com/horizon/documentation/unity/unity-tracked-keyboard/)); experimental desk "surface keyboard" ([help](https://www.meta.com/help/quest/1589938228821220/)) | yes, mic key |
| visionOS | auto-appears on text-field focus, floats; movable by window bar, resizable at corners; **recenter also summons a lost keyboard** ([Apple support](https://support.apple.com/guide/apple-vision-pro/enter-text-and-use-dictation-tana14220eef/visionos)) | look-at-key + pinch, or direct touch (one finger per hand); long-press for accents; password preview obscured from shared views | yes; "Look to Dictate" (gaze at mic key), voice edit commands |
| Android XR | floating panel; move by pinching its edge; dismiss X ([Samsung](https://www.samsung.com/us/support/answer/ANS10007517/)) | poke or aim+pinch | Gemini/voice present platform-wide (keyboard-level dictation not explicitly documented) |
| WayVR | own keyboard overlay, **anchored in the working set** at (0,−0.65,−0.5) m, tilted −10°, width from `keyboard_scale` (`overlays/keyboard/mod.rs:98-116`); custom layout file, taskbar on top | laser-pointer typing; **modifier = laser colour** (orange laser types shifted); sticky modifiers; fcitx5/IME D-Bus integration; keys route to the last-clicked window | yes — local **Whisper** push-to-talk STT (`overlays/whisper.rs`) |
| WiVRn lobby | keyboard is a separate composition layer at fixed offset below the GUI ((0,−0.3,0.1) m, pitch −0.6 rad, `constants.h:87-88`); layouts configurable | **touch-vs-ray hysteresis**: direct touch within 0.18 m, ray beyond 0.22 m (`constants.h:46-47`) | no |
| xrdesktop | G3k keyboard shown per focused window; hides when that window closes (`xrd-shell.c:570-578`) | controller ray | no |
| KWin VR | Plasma's `inputMethod` window — exempted from lock-hiding alongside lock surfaces (`KwinWaylandSurface3D.qml:44`), otherwise an ordinary window; keyboard-first *hardware* input is the fork's design instead | physical keyboard first | no |
| Simula / StardustXR / Breezy | none — physical keyboard piped in (Stardust's `manifold`/`eclipse` → `simular` routes hw keys **to the window you're looking at**, [non-spatial-input](https://github.com/StardustXR/non-spatial-input)) | — | — |

Prose: consumer platforms have converged hard: **a floating system-owned keyboard bound to
text-field focus, dual-mode input (near = direct touch with poke limiting, far = ray/gaze +
pinch, switched by distance), a readout preview above the keys, and dictation as first-class
fallback**. WiVRn independently reimplemented the same distance-switched dual mode with numeric
hysteresis — the strongest OSS confirmation. Placement is *bound to the focused panel*
(WiVRn fixed offset; xrdesktop per-window; visionOS/Quest float near the field with
suggested-position APIs) rather than gaze-following. The security detail to keep: visionOS
obscures the password readout *from shared/captured views* — a capture-policy duty landing in
our spectate suppression (spatial-sharing §2.1). WayVR's laser-colour-as-modifier and Whisper
STT are the local-first curiosities; the latter is the only offline dictation precedent in the
field.

## 8. Recenter / summon conventions

| Subject | Convention |
|---|---|
| Horizon OS | long-press Meta/Oculus button recenters view+interface; dock repositionable as a unit |
| visionOS | press-and-hold Digital Crown: **entire window layout is rigidly re-seated around the user, relative positions preserved, last-opened window in front** ([TidBITS D](https://talk.tidbits.com/t/apple-s-vision-pro-is-compelling-in-the-future/26863), [HIG B](https://developer.apple.com/design/human-interface-guidelines/immersive-experiences)); locked/anchored apps stay put per room ([support](https://support.apple.com/en-mo/guide/apple-vision-pro/dev009366408/visionos)) |
| Android XR | Tidy re-layout on demand; recenter gesture/button not documented first-party (left open) |
| KWin VR | radial-menu **Recenter** (rigid group re-seat) + **Grab All** (drag the whole layout) + follow mode — three overlapping answers with only suppression flags as arbitration, author-acknowledged conflict (31 §2.10) |
| WayVR | hide-then-show the working set = re-center in front of head; `Positioning::Floating` windows re-seat relative to HMD, `Anchored` relative to the movable anchor (`wlx-common/src/windowing.rs:8-27`); playspace drag/reset moves the world |
| Simula | `grabWindows` (Meta+M) summons all windows; `orientWindowTowardsGaze` (Meta+F) realigns one; workspaces summon window sets (`config/config.dhall`) |
| WiVRn | **open-palm gesture** (flat hand, palm up) or controller long-action summons the GUI to a palm-relative offset; every placement recomputes yaw to face the user and pitch from the gaze-elevation table (`lobby.cpp:438-540`) |
| Breezy | recenter as a configurable keyboard shortcut (`recenter-display-shortcut`); follow modes with 2°/0.5°/20–40° thresholds (`xr-linux-driver/src/plugins/smooth_follow.c:27-55`) |
| StardustXR | none global; per-panel grab handles only |

Prose: recenter is universally **a rigid transform of the whole layout about the user — never a
per-window reflow** (visionOS, kwin-vr, WayVR, Simula's grab-all, Quest). The reserved
physical trigger (dedicated button long-press on all three consumer platforms) is the second
invariant; hand-tracking systems add a palm gesture (WiVRn's open-palm, Quest's look-at-palm
pinch). Anchored windows are exempt from recenter on visionOS — anchored places must survive a
recenter in our model too (ADR 0009 anchors; recentering redefines LOCAL, not the map). The
kwin-vr finding stands as the cautionary tale: recenter, grab-all, and follow are the *same*
layout authority in three costumes and must live in one state machine (constraint 8).

## 9. The pattern verdict

### 9.1 Per-topic verdicts

1. **Initial placement — dominant pattern exists.** Head-relative spawn in front at fixed
   distance facing the user; second window adjacent (slot/dock or angular free-slot search);
   clients never choose placement. Divergence is only in *structure* (dock vs free float) and
   tracks input hardware: controller platforms tolerate free placement, hands/gaze-first
   platforms pre-structure it. Mura's free-floating T1 windows + anchored places select:
   head-relative arc spawn with an angular-slot search (SpaceAllocator3D's shape), dimension-
   stability deferral (Simula), and the visionOS rule that apps cannot self-place. A dock is a
   *layout policy* option, not architecture.
2. **Launcher — dominant pattern exists.** Flat icon grid/tray on a reserved summon; pinning;
   search. OSS unanimously consumes XDG desktop entries with a custom scanner. No divergence of
   substance; drag-out-to-place (protostar) is the one spatial enrichment worth adopting since
   it feeds placement (§2). Matches ADR 0012 §1's launcher seam (layer-shell + xdg-activation)
   unchanged.
3. **Notifications — partial convergence.** Transient head-follow toast + immersion-coupled DND
   with a critical-class bypass is common; *where* the toast lives (head vs wrist vs launcher
   inbox) genuinely diverges with body-tracking hardware (wrist anchor needs reliable hands/
   wrists — WayVR's watch presumes controllers/hands; consumer HMDs keep it head-space).
   Mura: per-topic routing table (WayVR's shape) under a compositor-capped head-follow
   default; wrist is a policy option gated on hand tracking; critical class reserved for
   authority-plane events (boundary, tracking, battery, lock).
4. **Consent — dominant pattern exists (structurally).** System-layer dialog no app can draw
   over, input withdrawn from the app while shown, persistent sensor-use indicator. The 10.4
   fork input: both consumer platforms render consent *in the compositor's trust domain*, not in
   a delegable client — see §10.4.
5. **Boundary — NO convergence** (§6 prose): draw-your-own vs invisible-auto-zone vs presets;
   OSS ships nothing. The divergence is honest (posture + passthrough quality + locomotion
   expectations), so Mura must *choose per profile* rather than copy: see §10.5.
6. **Virtual keyboard — dominant pattern exists.** Floating focus-bound system keyboard,
   distance-switched direct-touch/ray dual mode, readout bar, dictation fallback,
   physical-keyboard escape hatch (passthrough cutout). Divergences are input-hardware-driven
   only (swipe needs a continuous pointer; look-to-dictate needs eye tracking).
7. **Recenter/summon — dominant pattern exists.** Rigid whole-layout re-seat on a reserved
   long-press + optional palm gesture; anchored content exempt; summon = same operation applied
   to a window subset. Divergence only in trigger hardware.

### 9.2 Scoring against composition §7.3 constraints 6–9

| Subject | 6: capped autonomous motion | 7: stabilize-then-arbitrate | 8: one arbitration machine | 9: one source of defaults/sizing |
|---|---|---|---|---|
| KWin VR | **violates** — uncapped exponential slerp follow (31 §2.10) | **violates** — raw distance-ordered picking, 3 user requests unmet (31 §2.11) | **violates** — follow/grab-all/recenter suppression-flag pile (31 §2.10) | **violates** — kcfg vs compiled defaults; ppu double-booking (31 §2.10/2.12) |
| WayVR | partial — head-follow uses lerp easing, no explicit cap | partial — no gaze; laser + click-precedence rule | partial — Positioning enum is a de-facto small machine | config.yaml single store — ok |
| Breezy/XRLinuxDriver | **violates** — exponential-approach slerp, no velocity cap, three preset threshold sets (`smooth_follow.c:27-55`) | n/a (3DoF, mouse input) | partial — explicit NONE/INIT/WAITING/SLERPING states (good) but split across 3 plugins (author's own TODO admits the mess) | dual config stores (driver config + gschema) — weak |
| WiVRn | **supports** — explicit max speed constants for GUI/foveation moves (`constants.h:129-137`) | **supports** — touch/ray hysteresis bands; recenter gesture requires sustained pose | supports — single lobby state | single constants header + one config — ok |
| xrdesktop | n/a (no autonomous motion) | **supports** — shake compensation ON by default (180 ms window, threshold 2.0, `res/org.xrdesktop.gschema.xml:5-30`): click-time pointer-motion queue replay/discard rather than continuous smoothing — a cheap alternative stage worth noting | n/a | gsettings single store — ok |
| visionOS | **supports** — system-only autonomous transitions are fades/opacity, not motion; speed-triggered immersion clearing | **supports** — gaze dwell highlighting out-of-process; pinch confirm | supports (opaque but no user-visible mode conflicts) | **supports** — boundary/comfort not configurable at all: one source, zero knobs |
| Horizon OS | supports — dock/window moves are user-initiated | **supports** — poke limiting, swipe recognizer, suggested keyboard placements | **violates (field evidence)** — Navigator v77/v81 churn; boundary home-vs-app preference flapping | **violates** — two saved boundary preferences producing user-visible flapping (D) |
| Android XR | supports — Tidy is user-invoked | supports (poke or aim+pinch) | n/a (young) | dp-based size contract is a single declared mapping — ok |

Nothing in the field *contradicts* constraints 6–9; the two shells that ship follow modes
without caps (KWin VR, Breezy) are precisely the ones whose users/authors report the resulting
problems, and the two systems with explicit numeric caps or zero knobs (WiVRn, visionOS) have no
such reports. One refinement the evidence suggests: constraint 6's cap should be expressed as a
**clamp on top of exponential easing** (every implementation uses exponential approach; the bug
is only its unbounded onset velocity), and constraint 7 should admit **event-time compensation**
(xrdesktop's click-shake replay) as a legitimate stage beside continuous filtering. Constraint 9
gains a new argument: visionOS demonstrates that *deleting* a knob (boundary size) is a valid
single-source strategy for safety-relevant defaults.

## 10. Mapping to the registry rows

### 10.1 Initial-placement policy → window-management policy row (registry §3, partial)

Evidence selects: head-relative spawn arc at declared distance (contract-declared per
constraint 9, cf. WiVRn 0.5 m / Android XR 1.75 m), angular-slot search for siblings
(SpaceAllocator3D shape), dimension-stability deferral (Simula), no client-chosen placement
(visionOS), launch-gesture position override (protostar drag-out). Open questions the design doc
must answer: interaction with anchored places (does a place's layout override the spawn arc?);
popup/transient placement volumes (constraint 3) vs the spawn arc; whether a dock-like
"hinged" group is offered as a layout policy; how placement composes with docked-mode flat
presentation (ADR 0015).

### 10.2 App launcher (registry §5, missing)

Evidence selects: separate client on layer-shell + `xdg-activation` (ADR 0012 §1 unchanged);
flat pinned-grid + search UI; XDG desktop-entry + icon-theme scanner (WayVR
`desktop_finder.rs` as the code precedent — blocklists included); per-app launch profile
(WayVR's native-vs-cage, resolution/aspect) generalizes to our per-app compositor-mode choice;
drag-out-to-place feeding 10.1. Open: launch-token → placement handoff semantics (activation
token carrying a spawn transform); whether pinning state lives in the settings model
(research/35) or the launcher.

### 10.3 Notification presentation (registry §5, missing)

Evidence selects: spec service (separate daemon) + presentation policy: per-topic routing
table (WayVR), head-follow lerped toast default with angular-size/depth caps enforced by zxr
(ADR 0012 §1's notification row), immersion-coupled DND with an authority-reserved critical
class that bypasses it (Quest), glance-to-expand as a gaze option (visionOS 27), capture
suppression (spatial-sharing §2.1). Open: does the wrist anchor ship at all without reliable
wrist pose; is the inbox a launcher-overlay panel (Android XR) or separate surface; who renders
critical-class toasts when the notification daemon is dead (authority-plane fallback, cf. lock
scene reasoning).

### 10.4 In-space consent picker (registry §5 partial — the §10.4 placement fork)

The cross-ecosystem evidence bears directly on the open fork (registry §10.4: portal-backend
chooser in-process vs Mutter-style private compositor API): both consumer platforms render
consent in the **compositor's trust domain** — un-drawable-over, un-imitable, input withdrawn
from the requesting app while shown (Quest pauses the immersive activity; Meta documents the
focus steal as intended). That is strong precedent for the chooser being **compositor-rendered
(in-process or over a private trusted API), never an ordinary client window** — the picker needs
the same guarantees as the lock scene (ADR 0007 I1's mechanism reused for a scoped consent
grab). Plus: persistent sensor-use indicators (visionOS dots ≙ our badges row) and PIN-in-
headset for cross-device consent (WiVRn ≙ ADR 0007 PIN pad). Open: scoped input grab semantics
(app keeps rendering but loses input — a new composition-policy state between "normal" and
"locked"); gaze privacy in the picker (dwell highlight compositor-side, gaze never to the
requester); wording/scope taxonomy from spatial-sharing §2.2.

### 10.5 Boundary setup UX (registry §5, missing)

No convergence to copy (§6), so choose per posture profile: **seated/desk profile** takes the
visionOS shape — auto zone around the session start pose, fade-to-passthrough breach response
(already our compositor-owned response, spatial-mapping §7), *no user-facing setup at all*;
**roomscale profile** takes Quest's draw-on-floor with our geometry service pre-proposing the
polygon (auto-detect + confirm — Android XR's preset+floor-confirm is the midpoint), floor-height
confirm step mandatory (both Quest and Android XR ship it explicitly). One stored boundary per
place — not Quest's home-vs-app dual preference (constraint 9 violation with field evidence).
Open: how a drawn boundary binds to a mapping-service anchor/place; re-confirm UX after
relocalization to a changed room; whether passthrough mode suppresses the fence (Quest) or the
fence merely fades.

### 10.6 Virtual keyboard (registry §5 partial + IM framework, registry §6 missing)

Evidence selects: system-owned floating keyboard as a trusted surface (generalizing ADR 0007's
lock PIN pad — same binding as the lock exemption kwin-vr had to add for `inputMethod`
windows), summoned by text-input focus (`text-input-v3`/`input-method-v2` per ADR 0012), placed
bound to the focused panel at a declared offset with user override (WiVRn/xrdesktop), dual-mode
input with distance hysteresis (WiVRn's 0.18/0.22 m; Quest's poke limiting on the near path),
readout preview bar with capture-suppressed secret fields (visionOS), dictation as fallback
(local Whisper precedent in WayVR fits the no-cloud posture), physical-keyboard escape hatch via
passthrough cutout (Quest tracked keyboard — needs only a passthrough region, ADR 0008).
Open: keyboard-in-lock-scene vs general keyboard (one component in two trust modes, or two
components); swipe typing (needs continuous pointer — controller/ray yes, gaze no); layout/IME
model (WayVR's fcitx5 D-Bus bridge is the only OSS precedent for CJK in VR).

---

**Report-back summary.** (1) Strongest convergences: head-relative spawn/summon-in-front at a
declared distance with rigid-group recenter preserving relative layout; flat pinned-grid
launcher on a reserved gesture with XDG desktop-entry consumption in every OSS shell; floating
focus-bound system keyboard with distance-switched touch/ray dual mode and dictation fallback.
(2) No convergence on boundary setup — draw-your-own (Quest) vs non-configurable auto-zone
(visionOS) vs presets+floor-confirm (Android XR) vs nothing (all OSS) — because the topic is
posture- and passthrough-quality-dependent; Mura should branch per profile rather than
pick one. (3) Nothing contradicts constraints 6–9; the shells lacking caps/stabilization are
the ones with documented user pain, and WiVRn/visionOS demonstrate the compliant shapes. Three
refinements: express the constraint-6 cap as a clamp over exponential easing; admit event-time
compensation (xrdesktop's click-shake replay) as a constraint-7 stage; and note that hard
head-locking small transient OSDs is universal practice — comfort caps govern large surfaces,
not toasts.
