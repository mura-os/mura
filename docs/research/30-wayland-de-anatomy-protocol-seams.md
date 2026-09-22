# 30 — Wayland DE anatomy: privileged protocol seams and precedent

**Status:** research complete  
**Date:** 2026-09-22  
**Scope:** protocol and implementation evidence for ADR 0012; no architecture decision is made here.

## 0. Executive result
The useful dividing line is not “UI versus compositor.” It is **presentation versus authority**.
A pager, launcher, panel, notification popup, OSD, settings UI, portal backend, and clipboard-history UI can all be
separate Wayland clients when the compositor exports a narrow protocol and remains free to reject requests. Focus
assignment, hit testing, secure lock state, input routing, placement invariants, and final application of output or
window state remain compositor authority even when an external client presents the controls.

The standardized surface is substantially better than the historical “Wayland has no DE protocols” account:

- `ext-workspace-v1` is in `wayland-protocols` staging and is implemented by COSMIC, Hyprland, labwc, Jay, niri,
  and newer Sway work. It is the decisive seam for an external pager or overview
  ([1.39 announcement](https://lists.freedesktop.org/archives/wayland-devel/2024-December/043920.html),
  [implementation survey](https://github.com/Alexays/Waybar/pull/4016)).
- `ext-foreign-toplevel-list-v1` standardizes foreign-toplevel identity and enumeration, but deliberately does not
  standardize state or control. A complete taskbar still needs `wlr-foreign-toplevel-management`, COSMIC extensions,
  or another privileged control channel
  ([ext protocol](https://wayland.app/protocols/ext-foreign-toplevel-list-v1),
  [wlr protocol](https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1)).
- `wlr-layer-shell` remains the deployed panel/overlay seam. COSMIC, KWin, wlroots/smithay desktops, and Mir support
  it, but Mutter does not; its proposed ext successor is still a draft
  ([support](https://wayland.app/protocols/wlr-layer-shell-unstable-v1),
  [draft MR](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/merge_requests/28)).
- Capture crossed the standardization boundary: `ext-image-capture-source-v1` plus
  `ext-image-copy-capture-v1` supersede `wlr-screencopy`, while the portal and PipeWire remain the normal consent and
  transport path ([1.37 announcement](https://lists.freedesktop.org/archives/wayland-devel/2024-August/043774.html),
  [wlr deprecation](https://wayland.app/protocols/wlr-screencopy-unstable-v1)).
- Clipboard management has `ext-data-control-v1`; display configuration still has only the widely deployed
  `wlr-output-management` family, with no merged ext successor
  ([1.39 announcement](https://lists.freedesktop.org/archives/wayland-devel/2024-December/043920.html),
  [output management](https://wayland.app/protocols/wlr-output-management-unstable-v1)).

Plasma and COSMIC are importantly different precedents. Plasma 6 factors presentation through in-process KWin
extension points and private Plasma protocols; surveyed KWin 6.7 has **not** adopted `ext-workspace-v1` or
`ext-foreign-toplevel-list-v1` ([workspace matrix](https://wayland.app/protocols/ext-workspace-v1),
[toplevel matrix](https://wayland.app/protocols/ext-foreign-toplevel-list-v1)). COSMIC actually runs its pager,
panel, launcher, OSD, notifications, greeter, settings, and portal as separate Rust components. It migrated its
workspace, foreign-handle, and capture bases to new ext protocols while retaining small COSMIC extensions where
upstream is incomplete ([workspace PR](https://github.com/pop-os/cosmic-comp/pull/1213),
[capture PR](https://github.com/pop-os/cosmic-comp/pull/1280)).

For spatial-os, COSMIC is therefore the closest implementation precedent. None of these protocols knows what an
OpenXR view, world anchor, depth buffer, 3D bounds, or spatial workspace thumbnail is. The safe pattern is
“upstream base object plus zxr extension,” not silently giving 2D protocol words new wire meanings.

## 1. Governance, privilege, and evidence
`wayland-protocols` now distinguishes development, staging/testing, and stable phases. Staging protocols are released
and implementation is encouraged; existing interfaces cannot change incompatibly, but the whole protocol may still
be superseded by a new major version. The `unstable/` directory is the older governance model, not a synonym for
“private” ([governance summary](https://github.com/wayland-mirror/wayland-protocols)).

This document uses:

- **staging/ext** — merged, released `wayland-protocols`, in testing rather than declared stable.
- **unstable/wp or xdg** — a released legacy `wayland-protocols` protocol with `z*` names.
- **wlr** — maintained outside `wayland-protocols`; often broadly deployed, but not governed as an ext/wp standard.
- **private** — a compositor/DE namespace such as `org_kde_*` or `zcosmic_*`.
- **adopted** — a compositor advertises and implements the global, not merely generated bindings. Linked support
  matrices are point-in-time evidence.

Privilege is orthogonal to namespace. Foreign-toplevel list, data control, session lock, capture, output management,
virtual keyboard, and input method expose sensitive information or authority even when their XML lives upstream.
Their globals should be filtered to trusted shell clients, portal backends, or explicitly authorized connections.
`security-context-v1` supplies sandbox identity, not authorization by itself
(`references/wayland-protocols/staging/security-context/security-context-v1.xml`).

## 2. Privileged and desktop protocol inventory
### 2.1 `ext-workspace-v1`: the decisive pager seam
`ext-workspace-v1` entered staging in `wayland-protocols` 1.39 on 2024-12-20
([announcement](https://lists.freedesktop.org/archives/wayland-devel/2024-December/043920.html)). It is released
upstream, not a proposal and not wlroots-only.

Its model is richer than a flat desktop-number list:

- A manager advertises workspace groups and workspaces, with manager-wide `done` events for atomic snapshots.
- A group owns a set of `wl_output`s and workspaces, allowing per-output, globally shared, or arbitrary grouping.
- A workspace carries a human name, optional session-stable opaque ID, optional N-dimensional integer coordinates,
  and `active`, `urgent`, and `hidden` state.
- Group capability is `create_workspace`; workspace capabilities are `activate`, `deactivate`, `remove`, and
  `assign`. Requests are advisory and committed atomically; unsupported requests are ignored.

These semantics are specified in
`references/wayland-protocols/staging/ext-workspace/ext-workspace-v1.xml`. Coordinates may conventionally include
X, Y, Z, and higher dimensions, but they do not define metric 3D transforms, workspace contents, or thumbnails.

Adoption is real: COSMIC merged support in February 2025; Hyprland, labwc, Jay, and niri followed; Sway merged an
implementation in March 2026 ([COSMIC](https://github.com/pop-os/cosmic-comp/pull/1213),
[Hyprland](https://github.com/hyprwm/Hyprland/pull/10818),
[niri](https://github.com/YaLTeR/niri/pull/1800), [Sway](https://github.com/swaywm/sway/pull/9064)).
Waybar, sfwbar, and xfce4-panel provide client evidence; early multi-group bugs show why clients must not flatten
groups ([Waybar](https://github.com/Alexays/Waybar/pull/4016),
[interoperability notes](https://github.com/YaLTeR/niri/pull/1800)).

**Separate component enabled:** pager, workspace indicator, and workspace-model half of an overview. A full overview
also needs foreign-toplevel and capture protocols.

**XR consequence:** use ext workspaces as the stable identity/lifecycle base. Add a zxr extension for space type,
metric transform/bounds, anchor policy, and compositor-produced stereo/depth preview source. Do not reinterpret
integer coordinates as metres.
### 2.2 Foreign toplevels: upstream list, de facto control
`ext-foreign-toplevel-list-v1` intentionally only enumerates mapped toplevels and provides handles with identifier,
title, app ID, atomic `done`, and `closed`. It expects extension protocols to add state, capture, and actions, and
permits restricting the global to a chosen client
(`references/wayland-protocols/staging/ext-foreign-toplevel-list/ext-foreign-toplevel-list-v1.xml`).

The deployed split is:

- `ext-foreign-toplevel-list-v1`: upstream handle/identity base, adopted by COSMIC, Sway, labwc, Mir, phoc, river,
  Jay, Treeland, and niri, but not surveyed KWin or Mutter
  ([matrix](https://wayland.app/protocols/ext-foreign-toplevel-list-v1)).
- `wlr-foreign-toplevel-management-unstable-v1`: older combined list, output membership, state, and control. It
  requests activate, close, maximize, minimize, and fullscreen. Sway, Hyprland, labwc, niri, river, phoc, Wayfire,
  Mir, and others implement it; COSMIC, KWin, and Mutter do not in surveyed versions
  ([protocol/matrix](https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1)).
- Proposed ext state and management protocols remain follow-up work rather than a merged control standard. Ext-list
  is therefore not a complete wlr replacement
  ([upstream sequence](https://github.com/pop-os/cosmic-epoch/issues/100),
  [niri discussion](https://github.com/YaLTeR/niri/pull/2044)).

**Separate component enabled:** taskbar/dock and switcher model. Read-only task lists can use ext alone; interactive
switchers need a control extension. zxr can implement mature wlr control for compatibility or a narrow zxr extension
until ext management lands.

**XR consequence:** a toplevel may be a 2D quad or native 3D scene client. Extend its handle with presentation kind
and spatial attention state; never treat client-authored depth as a trusted preview.
### 2.3 `wlr-layer-shell`: deployed shell surfaces, no merged ext replacement
`wlr-layer-shell-unstable-v1` gives surfaces a desktop-layer role, selects background/bottom/top/overlay order,
anchors them to output edges/corners, reserves an exclusive zone, and controls keyboard interactivity. It supports
panels, docks, wallpaper, notifications, OSDs, and launchers
([protocol](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)).

Deployment is wider than wlroots: COSMIC, KWin, Mir, smithay/wlroots compositors, and many independents advertise it;
Mutter and Weston do not ([matrix](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)). The
`ext-layer-shell` MR has remained a draft since 2020 with unresolved namespace/ordering issues, so there is no
standard successor today ([draft](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/merge_requests/28)).

**Separate component enabled:** panel, dock, wallpaper, notification surface, OSD, launcher, and shell chrome.

**XR consequence:** output edge, exclusive zone, and 2D z-order do not define headset placement. A zxr extension must
select head-, body-, world-, or space-anchored placement and define angular/metric exclusion. Keep the base role for
ordinary 2D compatibility.
### 2.4 `xdg-activation-v1`: focus-safe launch and handoff
An initiating client asks for a token, optionally binds it to a recent seat serial, requesting surface, and target
app ID, then transfers it out of band (commonly `XDG_ACTIVATION_TOKEN`). The target asks to activate its own surface.
The compositor may issue an ineffective token or reject activation to prevent focus stealing
(`references/wayland-protocols/staging/xdg-activation/xdg-activation-v1.xml`).

It is staging and broadly adopted by COSMIC, KWin, Mutter, Sway, Hyprland, labwc, niri, and phoc
([matrix](https://wayland.app/protocols/xdg-activation-v1)). It is a launch/handoff mechanism, not arbitrary foreign
window activation.

**Separate component enabled:** launcher and notification action starting or raising its own app.

**XR consequence:** validate the controller/hand-ray seat serial and spatial focus context. Successful activation may
draw attention or bring a space into view rather than teleport a surface in front of the user.
### 2.5 `xdg-decoration-unstable-v1`: negotiation, not a renderer seam
This protocol negotiates whether an `xdg_toplevel` uses client- or server-side decorations. The compositor selects
the effective mode through `xdg_surface.configure`; no third process can render the server frame
(`references/wayland-protocols/unstable/xdg-decoration/xdg-decoration-unstable-v1.xml`).
KWin, COSMIC, Sway, Hyprland, labwc, niri, Mir, and others support it; Mutter does not
([matrix](https://wayland.app/protocols/xdg-decoration-unstable-v1)).

**Separate component enabled:** none by itself. It enables SSD/CSD interoperability; replaceable SSD rendering remains
in-process or needs private IPC.

**XR consequence:** use it for ordinary 2D quads. Native 3D clients need zxr manipulation affordances; titlebar mode
says nothing about grasp volumes or 6DoF resize handles.
### 2.6 Idle and lock status
`ext-idle-notify-v1` exposes per-seat idle/resume. Version 2 distinguishes ordinary idle, which honors visible-surface
inhibitors, from input-only idle, which ignores them; user activity may include a presence sensor by compositor policy
(`references/wayland-protocols/staging/ext-idle-notify/ext-idle-notify-v1.xml`).
`zwp_idle_inhibit_manager_v1` lets a visible client inhibit idle
(`references/wayland-protocols/unstable/idle-inhibit/idle-inhibit-unstable-v1.xml`). Both are broadly adopted;
ext idle notify is present in COSMIC, KWin, and many wlroots/smithay compositors
([matrix](https://wayland.app/protocols/ext-idle-notify-v1)).

`ext-session-lock-v1` is the staging privileged-lock protocol. The compositor must stop normal rendering/input,
present a safe frame before reporting `locked`, and stay locked if the client dies
(`references/wayland-protocols/staging/ext-session-lock/ext-session-lock-v1.xml`). COSMIC, Sway, Hyprland, labwc,
niri, river, Mir, and others implement it; KWin and Mutter use integrated lockers and do not expose it
([matrix](https://wayland.app/protocols/ext-session-lock-v1)).

For spatial-os, ADR 0007 stands: idle notify/inhibit are public compatibility mechanisms; ext-session-lock is
dev/third-party only; secure built-in lock is compositor state.
### 2.7 `security-context-v1`: identity for attenuation, not permission
A sandbox engine creates a listening Wayland socket, labels connections with engine, app ID, and instance ID, and
commits the context. Nested contexts are forbidden because they create privilege-escalation ambiguity
(`references/wayland-protocols/staging/security-context/security-context-v1.xml`).

Flatpak adopted it to give compositors reliable sandbox identity
([Flatpak PR](https://github.com/flatpak/flatpak/pull/4920)). COSMIC, KWin, Sway, Hyprland, labwc, niri, river,
phoc, and others implement it; surveyed Mutter does not
([matrix](https://wayland.app/protocols/security-context-v1)).

The compositor still needs global-filter policy. zxr should hide foreign-toplevel, workspace control, capture,
data-control, session-lock, input-method, virtual-keyboard, output-management, and zxr privileged globals from
sandboxed connections unless separately authorized. The wlroots implementation explicitly combines context lookup
with a display global filter
([wlroots API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_security_context_v1.h.html)).
### 2.8 Text input, input method, virtual keyboard: three directions
The family is complementary, not three versions of one operation:

- `text-input-unstable-v3` is the app side. Editable clients send enabled state, surrounding text, content purpose/
  hints, and cursor rectangle; the compositor returns preedit, committed text, and deletion. Focus follows keyboard
  focus (`references/wayland-protocols/unstable/text-input/text-input-unstable-v3.xml`).
- `input-method-unstable-v2` is the privileged IME side. One client serves a seat, receives field state, submits
  preedit/commit text, may create an IME popup, and may request a hardware-keyboard grab
  ([protocol](https://wayland.app/protocols/input-method-unstable-v2)).
- `virtual-keyboard-unstable-v1` injects synthetic keymap/key/modifier events. It can accompany an IME for actions and
  legacy apps, but is generic input injection and must be privilege-filtered
  ([protocol](https://wayland.app/protocols/virtual-keyboard-unstable-v1)).

Text-input-v3 is supported by KWin, Mutter, COSMIC, and many wlroots/smithay compositors. Input-method-v2 and
virtual-keyboard-v1 are implemented by COSMIC, Sway, Hyprland, niri, river, phoc, labwc, Mir, and others, but not
surveyed KWin or Mutter ([text matrix](https://wayland.app/protocols/text-input-unstable-v3),
[IME matrix](https://wayland.app/protocols/input-method-unstable-v2),
[VK matrix](https://wayland.app/protocols/virtual-keyboard-unstable-v1)). Toolkit versions and environment-selected
GTK/Qt/Fcitx paths remain messy; COSMIC still fixes input-method-v2 popup placement
([fix](https://github.com/pop-os/cosmic-comp/pull/2320)).

**Separate component enabled:** IME and on-screen keyboard, with the compositor as trusted relay and focus authority.

**XR consequence:** the cursor rectangle is surface-local 2D. zxr must transform it into world/head space before
placing keyboards or candidates. Virtual keyboard must not synthesize unrestricted global XR actions.
### 2.9 Capture: standard pixels, portal consent, PipeWire transport
`ext-image-capture-source-v1` defines opaque sources and factories for outputs/foreign toplevels.
`ext-image-copy-capture-v1` negotiates SHM/dmabuf constraints, accepts client-owned buffers, reports damage and
presentation time, and can capture cursor imagery separately
(`references/wayland-protocols/staging/ext-image-capture-source/ext-image-capture-source-v1.xml`,
`references/wayland-protocols/staging/ext-image-copy-capture/ext-image-copy-capture-v1.xml`).

They entered staging in 1.37 and supersede deprecated `wlr-screencopy`
([release](https://lists.freedesktop.org/archives/wayland-devel/2024-August/043774.html),
[deprecation](https://wayland.app/protocols/wlr-screencopy-unstable-v1)). COSMIC, Sway/wlroots, Jay, labwc, Mir,
phoc, Treeland, and newer Wayfire implement at least output capture; foreign-toplevel sources are a smaller subset.
KWin and Mutter use their own capture integration in surveyed releases
([copy matrix](https://wayland.app/protocols/ext-image-copy-capture-v1),
[source matrix](https://wayland.app/protocols/ext-image-capture-source-v1),
[Wayfire](https://wayfire.org/2026/07/24/Wayfire-0-11.html)).

Direct binding answers “how to copy pixels,” not “who consented.” The normal path is:

1. App creates `org.freedesktop.portal.ScreenCast` and selects source kinds.
2. Desktop portal backend presents selection/consent and asks the compositor for an authorized source.
3. Backend produces PipeWire streams.
4. `OpenPipeWireRemote` returns an FD whose permissions expose only those nodes
   ([ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html),
   [PipeWire control](https://flatpak.github.io/xdg-desktop-portal/docs/pipewire.html)).
5. `RemoteDesktop` adds authorized libei/EIS input and may share that ScreenCast session
   ([RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)).

The compositor authorizes/enforces source access; the portal mediates consent/selection; PipeWire transports frames.

**XR consequence:** these protocols copy a 2D image. A headset “screen” is not raw distorted eye images, and native
zxr scenes need color, depth, pose, view, and timing. Expose a compositor-rendered spectator source for compatibility,
then a privileged zxr capture-source extension for authorized stereo/depth. Never expose passthrough as an ordinary
output.
### 2.10 Data control, output management, cursor shape, global shortcuts
`ext-data-control-v1`, added in 1.39, gives a privileged client current/primary selection offers and authority to set
them: exactly the clipboard-manager mechanism
(`references/wayland-protocols/staging/ext-data-control/ext-data-control-v1.xml`). COSMIC, KWin, Sway, Hyprland,
labwc, niri, Mir, and others implement it, but not surveyed Mutter; deprecated wlr data-control remains a fallback
([ext matrix](https://wayland.app/protocols/ext-data-control-v1),
[wlr status](https://wayland.app/protocols/wlr-data-control-unstable-v1)).

`wlr-output-management-unstable-v1` enumerates physical heads/modes and lets an external settings UI atomically test
and apply configurations. COSMIC, Sway, Hyprland, labwc, niri, river, phoc, Wayfire, and others implement it; KWin
and Mutter use private mechanisms, and no merged ext replacement exists
([protocol/matrix](https://wayland.app/protocols/wlr-output-management-unstable-v1)). COSMIC privately extends it for
explicit mirroring ([extension](https://wayland.app/protocols/cosmic-output-management-unstable-v1)).

`cursor-shape-v1` requests a named cursor rather than uploading pixels, allowing correctly themed/scaled compositor
rendering (`references/wayland-protocols/staging/cursor-shape/cursor-shape-v1.xml`,
[KDE explanation](https://blogs.kde.org/2024/10/09/cursor-size-problems-in-wayland-explained/)). It is not privileged,
but is useful for consistent controller-ray pointers in the 2D tier; zxr still owns 3D ray/hand visuals.

Global shortcuts are load-bearing, but raw global input interception is compositor authority. Applications should use
the GlobalShortcuts portal or a compositor-owned registration service, not a global Wayland keylogger. Plasma’s
migration below is the concrete precedent.

## 3. KWin: modular presentation, compositor-owned authority
### 3.1 KDecoration3 is an in-process renderer boundary
KDecoration3 plugins supply border/titlebar geometry, buttons, shadows, and painting. A plugin sets border/titlebar
rectangles and implements `paint()`; the framework schedules repaints and exposes managed-window state
([API](https://api.kde.org/legacy/plasma/kdecoration/html/classKDecoration3_1_1Decoration.html)).

This is modular, but not a separate client. KWin owns the window, frame geometry, input routing, resize/move,
compositor scene, and final hit-testing. Its input filter delivers local decoration events then falls back to
compositor move/button handling; tests validate compositor-owned resize-edge cursor behavior
([input](https://invent.kde.org/rmauchin/kwin/-/blob/overview_scroll/src/input.cpp),
[tests](https://invent.kde.org/myli/kwin/-/blob/master/autotests/integration/decoration_input_test.cpp)).

Lesson: make decoration rendering replaceable **inside** zxr while retaining geometry and 6DoF hit testing there.
A renderer process would require private buffer/input IPC and add little policy separation.
### 3.2 Task switcher layouts externalize presentation only
KWin loads QML task-switcher layouts. `KWin.Switcher` supplies a compositor-created window model, index,
captions/icons/minimized state, desktops, and thumbnails; QML chooses visual layout and requests activation through
the model ([guide](https://develop.kde.org/docs/plasma/windowswitcher/),
[activation](https://invent.kde.org/plasma/kwin/-/commit/f6447ad188fa77cefd3b948c5e274e5a70d58d6f)).

This is in-process presentation factoring. Enumeration, filtering, thumbnail production, shortcut grabs, ordering,
and final activation remain in KWin. It validates model/view separation, not a public task-switcher protocol.
### 3.3 Scripting moves policy code, not authority
KWin JavaScript/QML scripts access `workspace` and `options`. They observe windows/signals, change writable window
state, query client areas, and access per-output tiling managers/tiles
([tutorial](https://develop.kde.org/docs/plasma/kwin/),
[KWin 6 API](https://develop.kde.org/docs/plasma/kwin/api/)). This supports many automatic-layout, keep-above,
focus, placement, and tiling policies. Declarative window rules remain a KWin facility, not portable Wayland.

Scripts run inside the manager against live KWin objects: an extension model, not process isolation or an external
policy client. zxr can copy a restricted scripting model without exporting unrestricted mutation to arbitrary clients.
### 3.4 Global shortcuts moved into KWin because interception is authority
On X11, `kglobalacceld` could observe global key events as a daemon. Under Wayland, KWin legitimately receives all
input, so KGlobalAccel was refactored into a library linked into KWin; `kwin_wayland` provides its D-Bus interface
([design](https://blog.martin-graesslin.com/blog/2015/06/global-shortcut-handling-in-a-plasma-wayland-session/),
[architecture note](https://mail.kde.org/pipermail/plasma-devel/2023-February/122956.html)).

Registration UI and dispatch can be external, but matching global chords against unconsumed seat input belongs in
the compositor. XR equivalents are controller chords, hand gestures, and system gaze gestures.
### 3.5 Plasma private protocols versus ext migration
Plasma’s private `plasma-shell` assigns desktop/panel/OSD roles; `plasma-window-management` lists and controls
windows and explicitly calls itself a DE implementation detail regular clients must not use
([protocol](https://wayland.app/protocols/kde-plasma-window-management)). Plasma’s shell works through those private
protocols and in-process APIs.

KWin has adopted cross-desktop mechanisms including `wlr-layer-shell`, `xdg-activation`, `ext-idle-notify`,
`security-context`, and `ext-data-control`
([layer](https://wayland.app/protocols/wlr-layer-shell-unstable-v1),
[idle commit](https://invent.kde.org/plasma/kwin/-/commit/0c28de5b42d6d472a2c2c58e8a17c2f451e40860),
[data control](https://wayland.app/protocols/ext-data-control-v1)). But KWin 6.7 remains without ext workspace and
foreign-toplevel-list; wlr foreign-toplevel support was still a wishlist bug
([workspace](https://wayland.app/protocols/ext-workspace-v1),
[toplevel](https://wayland.app/protocols/ext-foreign-toplevel-list-v1),
[bug](https://www.mail-archive.com/kde-bugs-dist@kde.org/msg1043914.html)).

**Conclusion:** Plasma 6 has not adopted the two decisive public seams for an out-of-process pager and switcher.

## 4. COSMIC: the Rust/smithay modular-shell precedent
### 4.1 Actual process split
`cosmic-comp` is the Rust/Smithay compositor authority. Separate components include `cosmic-panel`,
`cosmic-launcher`, `cosmic-applets`, `cosmic-osd`, `cosmic-workspaces`, `cosmic-notifications`,
`cosmic-greeter`, `cosmic-settings`, `cosmic-settings-daemon`, and `xdg-desktop-portal-cosmic`
([COSMIC](https://system76.com/cosmic), [packages](https://wiki.archlinux.org/title/COSMIC),
[component map](https://deepwiki.com/pop-os/cosmic-epoch/1.2.2-desktop-shell-components)). `cosmic-session` starts
and supervises them, so a shell UI can restart without moving authority into that client
([services](https://deepwiki.com/pop-os/cosmic-epoch/1.2.1-core-system-services)).

The greeter is a libcosmic app for greetd that can run inside cosmic-comp, with a separate privileged user-data daemon
([repository](https://github.com/pop-os/cosmic-greeter/)). This is a useful modular precedent, though spatial-os ADR
0007 deliberately chooses compositor-internal appliance lock plus an auth helper.
### 4.2 Standard seams actually used
COSMIC uses wlr layer-shell for panel/dock; client and Smithay server handlers are visible in source
([panel](https://github.com/pop-os/cosmic-panel/blob/master/cosmic-panel-bin/src/space_container/wrapper_space.rs),
[server](https://github.com/pop-os/cosmic-comp/blob/9feaa865/src/wayland/handlers/layer_shell.rs)).
Its launcher is a layer-shell frontend and obtains xdg activation tokens before spawning apps
([launcher](https://github.com/pop-os/cosmic-launcher/),
[token source](https://github.com/pop-os/cosmic-launcher/blob/bb0ed1c5/src/app.rs)).
Notifications are a separate layer-shell daemon
([repository](https://github.com/pop-os/cosmic-notifications)).

`cosmic-workspaces` consumes upstream `ext_workspace_handle_v1`, ext foreign handles, COSMIC extension handles, and
wlr layer-shell. It activates workspaces/toplevels, closes toplevels, and performs drag-to-workspace operations
([source](https://github.com/pop-os/cosmic-workspaces-epoch/blob/master/src/main.rs)).

COSMIC also migrated its compositor, shell clients, and portal to standard ext capture
([migration](https://github.com/pop-os/cosmic-comp/pull/1280),
[client toolkit](https://pop-os.github.io/libcosmic/src/cosmic_client_toolkit/screencopy/mod.rs.html)). This proves
the protocols are usable from a Smithay compositor and real shell clients, not merely XML designs.
### 4.3 Private seams retained where upstream is incomplete
COSMIC’s private repository still has workspace-v2, toplevel-info/management, output-management, overlap,
corner-radius, accessibility, and image-source extensions
([list](https://github.com/pop-os/cosmic-protocols/tree/main/unstable)). The useful design is extension, not replacement:

- `zcosmic_workspace_v2` extends `ext_workspace_handle_v1` with COSMIC-specific state/operations
  ([protocol](https://wayland.app/protocols/cosmic-workspace-unstable-v2)).
- `zcosmic_toplevel_info_v1` extends `ext_foreign_toplevel_handle_v1`; `zcosmic_toplevel_manager_v1` adds
  capability-gated close, activate, maximize, minimize, fullscreen, sticky, and move-to-workspace
  ([info](https://wayland.app/protocols/cosmic-toplevel-info-unstable-v1),
  [management](https://wayland.app/protocols/cosmic-toplevel-management-unstable-v1)).
- COSMIC output management extends wlr output management mainly for mirroring
  ([protocol](https://wayland.app/protocols/cosmic-output-management-unstable-v1)).
- Workspace preview capture adds a private workspace source factory while retaining upstream image-copy mechanics
  ([handler](https://github.com/pop-os/cosmic-comp/blob/9feaa865/src/wayland/handlers/image_copy_capture/mod.rs)).

COSMIC does **not** advertise old wlr foreign-toplevel-management; it uses ext-list plus COSMIC info/management
([fallback evidence](https://github.com/ActivityWatch/aw-watcher-window-wayland/pull/45),
[matrix](https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1)).
### 4.4 What COSMIC learned and upstreamed
Its repeatable pattern is: build modular clients against a private protocol; participate upstream; implement the ext
base in server and clients; keep only missing DE state in a private extension object.

COSMIC implemented ext workspace in February 2025 while retaining workspace-v2 additions
([PR](https://github.com/pop-os/cosmic-comp/pull/1213)). It replaced its screencopy base with ext image-copy in April
2025; the upstream author credits COSMIC implementation feedback
([PR](https://github.com/pop-os/cosmic-comp/pull/1280),
[history](https://andri.yngvason.is/making-a-wayland-screen-capturing-protocol.html)). Its toplevel protocols track
proposed upstream state/management splits
([discussion](https://github.com/pop-os/cosmic-protocols/issues/8)).

For zxr: implement upstream identity first, make spatial additions small extensions, and delete private duplication
when ext grows the capability.
### 4.5 Bracketing architectures
**GNOME:** Mutter is the compositor/window-manager library and GNOME Shell loads as a Mutter `MetaPlugin`; Shell’s
GJS UI/extensions directly access Mutter, Clutter, and Shell objects in the same process
([plugin](https://github.com/GNOME/gnome-shell/blob/master/src/gnome-shell-plugin.c),
[architecture](https://gjs.guide/extensions/overview/architecture.html)). This makes overview composition easy but
is the opposite of independently restartable shell clients. It is a valid endpoint for tightly coupled XR overview,
not the default modularity target.

**wlroots/Sway:** Sway is a compositor/window manager on modular wlroots
([project](https://swaywm.org/)); swaybar/Waybar, swaylock, swayidle, swaybg, mako, and fuzzel/wmenu are separate
programs ([add-ons](https://github-wiki-see.page/m/swaywm/sway/wiki/Useful-add-ons-for-sway),
[guide](https://wiki.archlinux.org/title/Sway)). It externalizes nearly every presentation component and uses
protocols or Sway IPC for requests.

COSMIC occupies the useful middle: external, supervised presentation processes; a cohesive toolkit/private extension
set; and a compositor retaining trusted mechanism.

## 5. ADR 0012 spin-out verdicts
Ratings:

- **standard-seam** — a named, adopted cross-desktop protocol/API carries the essential seam; compositor applies policy.
- **private-seam** — a separate process is plausible, but essential semantics require zxr-private IPC.
- **authority-only** — keep it in the compositor/trusted in-process module because it consumes raw input/composition
  authority or maintains invariants that cannot be delegated.

### 5.1 Workspace/space-model presentation — `standard-seam`
Use ext workspace for groups, lifecycle, names, IDs, coordinates, state, capabilities, and activation. Pair it with
ext foreign handles and capture for a conventional 2D overview
([workspace XML](../../references/wayland-protocols/staging/ext-workspace/ext-workspace-v1.xml),
[capture XML](../../references/wayland-protocols/staging/ext-image-copy-capture/ext-image-copy-capture-v1.xml)).

Add a zxr workspace extension for spatial bounds/anchors and a zxr image-source factory for compositor-rendered space
previews, following COSMIC workspace-v2/capture
([workspace](https://wayland.app/protocols/cosmic-workspace-unstable-v2),
[capture](https://github.com/pop-os/cosmic-comp/blob/9feaa865/src/wayland/handlers/image_copy_capture/mod.rs)).
The UI is a client; membership, rendering, and activation remain compositor-owned.
### 5.2 Task-switcher UI — `standard-seam` with a temporary control gap
Use ext foreign-toplevel-list for identity, image-capture-source for previews, and capability-gated management for
actions. Today that means wlr management or a small zxr extension because ext management is not merged
([ext](https://wayland.app/protocols/ext-foreign-toplevel-list-v1),
[wlr](https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1)).

XR switchers should display compositor-rendered previews rather than reconstruct 3D scenes. Activation is a request;
zxr chooses focus, attention cue, and placement.
### 5.3 Window-management policy: rules/tiling/placement — `authority-only`
Keep policy in zxr as declarative rules plus a restricted in-process scripting API. KWin proves scripts can alter
layout policy while sharing compositor objects, but no portable protocol represents initial placement, tiling trees,
focus prevention, or 3D collision constraints ([KWin API](https://develop.kde.org/docs/plasma/kwin/api/)).

External settings editors may write validated configuration. An all-powerful policy client would control field of
view, safety bounds, focus, and possibly passthrough occlusion.
### 5.4 Decoration renderer — `authority-only`
Make rendering/theme an in-process plugin modeled on KDecoration3; keep frame geometry, hit-testing, move/resize
grabs, scene insertion, and input in zxr
([KDecoration3](https://api.kde.org/legacy/plasma/kdecoration/html/classKDecoration3_1_1Decoration.html)).
Xdg decoration negotiates SSD/CSD but cannot host an external SSD renderer
([protocol](https://wayland.app/protocols/xdg-decoration-unstable-v1)).

Native 3D decorations become grasp/resize affordances with trusted hit volumes, making an untrusted renderer even
less appropriate than on a 2D desktop.
### 5.5 Notification daemon — `standard-seam`
Run a separate `org.freedesktop.Notifications` daemon and render via layer-shell, as COSMIC does
([COSMIC](https://github.com/pop-os/cosmic-notifications),
[layer-shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)). Actions launching apps use xdg activation.

XR needs zxr placement metadata: head-locked notifications risk discomfort and world-locked ones can be missed. zxr
should cap angular size/depth, enforce DND/safety, and turn the layer surface into a comfortable spatial quad.
### 5.6 Portal backend — `standard-seam`
Run `xdg-desktop-portal-spatial` out of process. Use ext image-copy/source, PipeWire frames, RemoteDesktop/libei
control, and compositor-created opaque sources
([ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html),
[RemoteDesktop](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)).

The backend presents consent; zxr enforces it and withholds privileged globals. XR needs a private portal/compositor
extension for spectator view, stereo/depth, passthrough exclusion, and per-observer 3D capture. Authorization remains
compositor authority.
### 5.7 App launcher — `standard-seam`
Use layer-shell for presentation and xdg activation to transfer a recent gesture, matching COSMIC
([launcher](https://github.com/pop-os/cosmic-launcher/),
[activation XML](../../references/wayland-protocols/staging/xdg-activation/xdg-activation-v1.xml)).

The zxr extension chooses head/body/world anchoring and maps controller/hand serials into token validation. Search and
results stay in the client; focus transition stays in zxr.
### 5.8 OSDs — `standard-seam`
Use layer-shell for a separate non-focus-stealing OSD
([COSMIC map](https://deepwiki.com/pop-os/cosmic-epoch/1.2.2-desktop-shell-components),
[layer-shell](https://wayland.app/protocols/wlr-layer-shell-unstable-v1)).

Add zxr constraints for angular size, distance, persistence, and gaze avoidance. Values should arrive through a narrow
service/compositor event channel; the OSD must not receive raw global controller input.
### 5.9 Display-config UI — `standard-seam` for monitors, `private-seam` for XR
Use wlr output management for conventional monitor heads and atomic test/apply; it is deployed despite not being ext
([protocol](https://wayland.app/protocols/wlr-output-management-unstable-v1)). The UI is separate; zxr validates and
applies every configuration.

HMD per-eye OpenXR views are not `wl_output` heads and users must not set raw modes, transforms, or lens geometry
through monitor UI. IPD, render scale, refresh policy, passthrough, recentering, and spatial topology need a narrow
zxr settings API backed by Monado/OpenXR capability checks.
### 5.10 Clipboard manager — `standard-seam`
Use ext data-control for a privileged, supervised clipboard-history client
([XML](../../references/wayland-protocols/staging/ext-data-control/ext-data-control-v1.xml),
[adoption](https://wayland.app/protocols/ext-data-control-v1)). Hide it from sandboxed/ordinary clients using
connection policy informed by security-context.

XR adds no transport problem, but clipboard previews may contain secrets and must not float in a public shared space.
Presentation belongs in the client; compositor visibility/sharing policy remains authoritative.

## 6. Recommended zxr boundary
Implement early in the 2D tier:

- `ext-workspace-v1` and `ext-foreign-toplevel-list-v1`;
- `wlr-foreign-toplevel-management-v1` for compatibility until ext management exists;
- `wlr-layer-shell-v1`, `xdg-activation-v1`, and `xdg-decoration-v1`;
- `ext-idle-notify-v1`, idle-inhibit, and dev-profile `ext-session-lock-v1`;
- `security-context-v1` plus per-client global filtering;
- text-input-v3, input-method-v2, and privileged virtual-keyboard-v1;
- ext image-capture-source/image-copy-capture and `ext-data-control-v1`;
- wlr output management for non-HMD outputs and cursor-shape-v1.

Define zxr-private extensions only for:

- spatial workspace metadata and compositor-rendered space preview sources;
- spatial toplevel state/actions not expressible by upstream foreign handles;
- world/head/body anchoring and angular/metric exclusion for layer surfaces;
- spectator/stereo/depth capture and passthrough redaction;
- HMD/OpenXR configuration that is not monitor output management.

Keep inside the compositor:

- final focus/activation decisions and global-shortcut matching;
- 2D/3D hit testing, input routing, grabs, and gesture recognition;
- window/space placement invariants and the restricted policy runtime;
- decoration geometry and native-3D manipulation affordances;
- secure lock state and every “presented safe frame” transition;
- capture authorization, passthrough privacy, and final display configuration.

This follows COSMIC’s successful Smithay process split without copying temporary private duplicates, preserves
Plasma’s proven in-process extension points where authority is inseparable, and concentrates zxr’s novel protocol work
on genuinely spatial semantics.
