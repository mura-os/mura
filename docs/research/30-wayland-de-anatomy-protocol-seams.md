# 30 — Wayland DE anatomy: privileged protocol seams and precedent

**Status:** research complete  
**Date:** 2026-09-22  
**Scope:** protocol and implementation evidence for ADR 0012; no architecture decision is made here.
**Scope rule (added after a coverage failure):** protocol surveys in this repo must sweep the full `staging/` + `unstable/` + `experimental/` directories of the pinned wayland-protocols clone, never a named list. The addenda below close the gaps that rule found.

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

For Mura, COSMIC is therefore the closest implementation precedent. None of these protocols knows what an
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

For Mura, ADR 0007 stands: idle notify/inhibit are public compatibility mechanisms; ext-session-lock is
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
([repository](https://github.com/pop-os/cosmic-greeter/)). This is a useful modular precedent, though Mura ADR
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

## Addendum A1 — The colour pipeline seam

### A1.1 Two protocols, two different questions

`color-management-v1` answers **what colorimetry the sample values mean and how content should be mapped**. It gives
each output an immutable image description, gives each surface a compositor-selected preferred description, and lets
a client attach its own description plus a rendering intent as double-buffered surface state
([XML](../../references/wayland-protocols/staging/color-management/color-management-v1.xml)). Descriptions may be:

- **parametric** — primaries/white point, named or power transfer function, primary and target luminance ranges,
  mastering primaries/luminance, MaxCLL and MaxFALL;
- **ICC v2/v4** — a bounded, readable profile supplied by file descriptor; or
- predefined Windows-scRGB / Windows-BT.2100 descriptions where advertised.

The compositor must support perceptual intent; relative, saturation, absolute, relative+BPC, and
absolute-without-adaptation are optional. An untagged surface is implementation-defined, though the XML recommends
treating it as sRGB. The protocol makes the compositor the conversion authority: a ready client description must be
accepted, but the compositor may transform it for the actual output
([surface semantics](../../references/wayland-protocols/staging/color-management/color-management-v1.xml)).

`color-representation-v1` answers the lower-level **how to reconstruct channels from this buffer** question. It does
not define RGB colorimetry. It supplies electrical/optical/straight alpha mode, H.273 matrix coefficients, full or
limited quantization range, and 4:2:0 chroma sample location—especially the missing metadata for YCbCr dma-bufs
([XML](../../references/wayland-protocols/staging/color-representation/color-representation-v1.xml)). Correct handling
requires both protocols: representation converts stored channels to RGB tristimulus values; image description gives
those values colorimetric meaning.

Both remain staging/testing in the pinned 2026-09-09 clone. Color management entered
wayland-protocols 1.41 on 2025-02-17
([release](https://lists.freedesktop.org/archives/wayland-devel/2025-February/043980.html)); color representation entered
1.44 on 2025-04-27
([release](https://mail-archive.com/wayland-devel@lists.freedesktop.org/msg43457.html)).

### A1.2 Shipping evidence and the Smithay gap

KWin shipped user-selectable HDR in Plasma 6.0 using a temporary KDE protocol, then switched its server to the
upstream color-management XML in October 2024
([Plasma 6 account](https://zamundaaa.github.io/wayland/2023/12/18/update-on-hdr-and-colormanagement-in-plasma.html),
[upstream switch](https://invent.kde.org/plasma/kwin/-/merge_requests/6711)). The pinned tree advertises v3 and
implements ICC/parametric descriptions and intents in
[`src/wayland/colormanagement_v1.cpp`](../../references/kwin/src/wayland/colormanagement_v1.cpp), and implements
alpha/coefficient/range/chroma state in
[`src/wayland/colorrepresentation_v1.cpp`](../../references/kwin/src/wayland/colorrepresentation_v1.cpp).

Mutter shipped `wp_color_management_v1` and experimental HDR controls in GNOME 48 (48.0 released 2025-03-19);
Mutter 48 NEWS names the protocol and HDR DisplayConfig support
([NEWS](https://github.com/GNOME/mutter/blob/86097755798e96b10ae167086acbd0eaf2688804/NEWS)).
The pinned tree contains its server in
[`meta-wayland-color-management.c`](../../references/mutter/src/wayland/meta-wayland-color-management.c) and the
representation global plus YCbCr tests in
[`meta-wayland-color-representation.c`](../../references/mutter/src/wayland/meta-wayland-color-representation.c).

wlroots now has a real `wlr_color_manager_v1` API with advertised features, descriptions, outputs, and surface
feedback—not merely generated bindings
([API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_color_management_v1.h.html)). Smithay/wayland-rs
exposes generated staging bindings, but the pinned `cosmic-comp` has no color-management handler; generated bindings
are not an end-to-end renderer/color pipeline
([bindings](https://smithay.github.io/smithay/src/wayland_protocols/wp.rs.html),
[`cosmic-comp`](../../references/cosmic-comp/)). A Smithay-leaning zxr therefore owns integration work even though it
does not need to invent the wire protocol.

### A1.3 OpenXR is the final output leg, not a replacement

`XR_FB_color_space` lets an application enumerate runtime-supported spaces and call `xrSetColorSpaceFB`; if it does
not call, the runtime chooses a default
([Khronos reference](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrSetColorSpaceFB.html)). It is an optional
vendor extension and is not implemented in the pinned Monado tree: a full-tree search finds no
`XR_FB_color_space`, while Monado's compositor currently defaults its Vulkan target to
`VK_COLOR_SPACE_SRGB_NONLINEAR_KHR`
([settings](../../references/monado/src/xrt/compositor/main/comp_settings.c),
[published extension list](https://monado.freedesktop.org/)).

That makes zxr's baseline explicit: decode every client buffer according to representation + image description,
blend and tone-map in a known linear working space, then encode exactly once for the OpenXR swapchain/runtime path.
Sampling sRGB bytes through a UNORM view omits decode; sampling linear bytes through an sRGB view adds one; blending
in encoded sRGB is wrong. Monado itself carries separate sRGB/UNORM image views and conversion paths, showing that
the distinction is operational rather than terminology
([swapchain code](../../references/monado/src/xrt/compositor/util/comp_swapchain.c),
[OpenVR bridge](../../references/monado/src/xrt/state_trackers/openvr/compositor/openvr_compositor_vulkan.cpp)).

Panel primaries, transfer response, black level, brightness limits, lenses, and camera response vary per headset.
Consequently panel/camera characterization is device adaptation data, available before login like the lens/IPD
calibration already required by ADR 0007—not an application preference
([ADR 0007](../architecture/adr/0007-session-greeter-lock.md)). Passthrough matching is a second transform:
camera raw/ISP output → characterized scene/display space → panel output. Matching rendered white to passthrough
white requires camera exposure/white-balance metadata and panel calibration; neither Wayland protocol describes
camera radiometry.

| Boundary | Rating | zxr responsibility |
|---|---|---|
| Client surface → compositor | **standard-seam** | Serve both staging protocols; accept ICC/parametric descriptions and representation metadata; publish preferred descriptions. |
| Compositor working space → HMD | **authority-only** | Own linearization, gamut/tone mapping, blend order, OpenXR swapchain encoding, panel calibration, and camera/display passthrough matching. `XR_FB_color_space` may optimize the final leg when Monado implements it, but cannot delegate policy. |

## Addendum A2 — Application session restoration

### A2.1 What `xdg-session-management-v1` restores

The manager creates or reopens an application session using an opaque UTF-8 identity string and one of `launch`,
`recover`, or `session_restore`. A new session emits `created(session_id)`; a recognized one emits `restored`; taking
the same identity from another client emits `replaced`. Sessions persist across application and compositor restarts,
subject to compositor retention/eviction policy
([XML](../../references/wayland-protocols/staging/xdg-session-management/xdg-session-management-v1.xml)).

Within a session, the application assigns stable names to individual `xdg_toplevel`s. `add_toplevel` starts tracking
a new name; `restore_toplevel` must be sent before the toplevel's first surface commit and asks the compositor to
apply its stored window-management state during the initial configure. Unknown names degrade to add, and clients
must tolerate missing or partial state. The compositor chooses what “state” means
([toplevel semantics](../../references/wayland-protocols/staging/xdg-session-management/xdg-session-management-v1.xml)).

There is intentionally no executable, desktop-file ID, command line, document URI, application checkpoint payload,
or relaunch request in the protocol. It restores compositor-owned state for a returning app instance; it does not
restart that app or restore its internal tabs/documents. That negative boundary follows directly from the protocol's
only objects—sessions and `xdg_toplevel`s—and is also the limit KDE documented for its initial implementation
([KDE account](https://blogs.kde.org/2025/04/12/this-week-in-plasma-the-beginnings-of-wayland-session-restore/)).

The old `xx-session-management-v1` experimental draft remains in the pinned clone for archaeology
([old XML](../../references/wayland-protocols/experimental/xx-session-management/xx-session-management-v1.xml)).
The finalized `xdg_` protocol graduated to staging in wayland-protocols **1.48 on 2026-04-01**
([release announcement](https://lists.freedesktop.org/hyperkitty/list/wayland-devel@lists.freedesktop.org/thread/5PO3FZFL2EF4SKJTWX6J2IQ2S2DUX62O/)).

### A2.2 Adoption and the necessary second component

KWin first shipped opt-in draft support in Plasma 6.4, then merged the final `xdg_` spelling in March 2026
([draft implementation](https://invent.kde.org/plasma/kwin/-/merge_requests/7475),
[final implementation](https://invent.kde.org/plasma/kwin/-/merge_requests/8985)). The pinned implementation stores
frame geometry and other compositor state in a bounded `KSharedDataCache`, rejects restore after initial configure,
and exposes the final global
([server](../../references/kwin/src/wayland/xdgsession_v1.cpp),
[storage API](../../references/kwin/src/wayland/xdgsession_v1.h)). Qt 6.10 and Chromium had draft client work; broad
application adoption still cannot be assumed, and no pinned niri implementation was found.

Full desktop restore is therefore two cooperating mechanisms:

1. **zxr implements the Wayland protocol** and stores placement/state keyed by session ID + toplevel name.
2. **A session restore manager records and relaunches applications**, carrying each session ID back to the returning
   process through toolkit/application integration.

Plasma demonstrates that split. `ksmserver` still speaks XSMP to cooperating X11 clients, while its Wayland startup
also asks KWin to load compositor state. A fallback saver records running application IDs; its restorer resolves
desktop entries, skips entries already covered by autostart/ksmserver, and relaunches with `KIO::ApplicationLauncherJob`
([startup overview](../../references/plasma-workspace/startkde/README.md),
[restore code](../../references/plasma-workspace/startkde/session-restore/restore.cpp),
[XSMP server](../../references/plasma-workspace/ksmserver/server.cpp)).

GNOME removed legacy restore in GNOME 49 because it was dead under systemd-managed sessions and XSMP clients could
not be reliably mapped to desktop files
([removal](https://github.com/GNOME/gnome-session/commit/586db75b6ec75d3e52998a00f52ac64e9d9da2b1)).
A replacement initiative based on `xdg_session_management_v1`, Mutter/toolkit support, and explicit app relaunch is
in development, but was not a complete user-facing feature as of the pinned date
([initiative](https://discourse.gnome.org/t/introducing-the-session-save-restore-initiative/33127)).

### A2.3 XR amplification and verdict

For Mura, a “place” is persistent compositor state, not application state. The compositor record should add
`place_id`, anchor identifier/version, transform relative to that anchor, bounds, presentation kind, and a safe
fallback when an anchor cannot be resolved. The restore manager must relaunch the app into the intended place and
deliver its opaque session identity; zxr then decides whether the old transform is still safe. This extends the
existing workspace/anchor model without putting executable launch authority in the Wayland protocol.

| Boundary | Rating | Mura responsibility |
|---|---|---|
| Returning toplevel → old state | **standard-seam** | Implement `xdg-session-management-v1`; extend stored compositor data with place/anchor IDs and degrade safely when anchors disappear. |
| Login/session → application relaunch | **private-seam** | A supervised `mura-session-restore` service uses desktop entries and activation, deduplicates autostart, and hands session IDs back to apps/toolkits. It never grants apps arbitrary placement. |

## Addendum A3 — Status items and toplevel icons

### A3.1 SNI is a deployed D-Bus seam with draft governance

The StatusNotifierItem document defines three session-bus roles: applications export `StatusNotifierItem`s; the
single `StatusNotifierWatcher` tracks them; one or more `StatusNotifierHost`s render them. Items expose status,
named/pixmap icons, tooltip, DBusMenu path, activation, secondary activation, and scroll
([local spec](../../references/xdg-specs/status-notifier-item/status-notifier-item-spec.xml)). The clone's index marks
SNI **draft**, version 0.1; the document itself still says `TBD`, so this is not a finished freedesktop standard even
though `org.kde.StatusNotifier*` is de facto interoperable
([index](../../references/xdg-specs/spec-index.toml),
[revisions](../../references/xdg-specs/spec-revs.toml)).

Plasma hosts it in the `plasma-workspace` system-tray applet and runs the watcher as a KDED module
([host](../../references/plasma-workspace/applets/systemtray/statusnotifieritemhost.cpp),
[watcher](../../references/plasma-workspace/statusnotifierwatcher/statusnotifierwatcher.cpp)). COSMIC hosts it in
**`cosmic-applets`, component `cosmic-applet-status-area`**, not `cosmic-panel`; that component also installs a
socket-activated watcher service
([source](https://github.com/pop-os/cosmic-applets/blob/ae3f7225/cosmic-applet-status-area/src/status_notifier_watcher.rs),
[installed component](https://archlinux.org/packages/extra/x86_64/cosmic-applets/files/)). In wlroots desktops,
Waybar's tray and sfwbar's tray act as hosts
([Waybar](https://github.com/Alexays/Waybar/blob/master/src/modules/sni/tray.cpp),
[sfwbar](https://github.com/LBCrion/sfwbar/blob/main/doc/sfwbar.rst)).

GNOME removed its built-in legacy tray in GNOME 3.26 (2017) and recommends that applications not require status
icons; SNI/AppIndicator support is supplied by an extension, not core Shell
([removal](https://lists.gnome.org/archives/commits-list/2017-August/msg02952.html),
[extension](https://extensions.gnome.org/extension/615/appindicator-support/)). Mura should support SNI for
compatibility, but must not make essential settings or safety state available only through it.

### A3.2 `xdg-toplevel-icon-v1` is a different icon

`xdg-toplevel-icon-v1` lets a client assign an icon to one particular toplevel, by XDG icon-theme name and/or one or
more immutable square wl_shm buffers. The compositor advertises preferred sizes and may choose name or pixels; this
is switcher/overview/taskbar input, not an application status item
([XML](../../references/wayland-protocols/staging/xdg-toplevel-icon/xdg-toplevel-icon-v1.xml)).
It entered staging in wayland-protocols 1.37 on 2024-08-31
([release](https://lists.freedesktop.org/archives/wayland-devel/2024-August/043774.html)).

Adoption is still narrow in the surveyed set: pinned/current KWin implements it, while the support matrix reports no
Mutter or COSMIC global in the surveyed releases
([matrix](https://wayland.app/protocols/xdg-toplevel-icon-v1),
[COSMIC request](https://github.com/pop-os/cosmic-comp/issues/1958)). zxr should implement it because a window icon
can differ from the launcher icon and because Wine/SDL/Qt windows may lack a useful desktop-entry mapping.

| Facility | Rating | Mura use |
|---|---|---|
| SNI watcher/host | **standard-seam (de facto D-Bus)** | A panel applet hosts icons/menus; a supervised watcher owns the bus name. Keep actions focus-safe and map their 2D coordinates only as hints. |
| `xdg-toplevel-icon-v1` | **standard-seam** | zxr stores the icon on the toplevel model; external switcher/taskbar reads the compositor's chosen icon through its trusted model/control seam. |

## Addendum A4 — Effects and animation factoring

KWin's C++ effects are same-process plugins loaded from `src/plugins/`. `Effect` exposes the global
`EffectsHandler`, window/workspace properties, chained screen/window pre-paint, paint, and post-paint hooks,
transform/opacity/brightness/saturation controls, custom drawing, and repaint scheduling
([API source](../../references/kwin/src/effect/effect.h),
[plugin inventory](../../references/kwin/src/plugins/)). `windowview` (Present Windows) is itself a `QuickSceneEffect`
with compositor window IDs, modes, shortcuts, gestures, and activation state—not an external shell client
([windowview](../../references/kwin/src/plugins/windowview/windowvieweffect.h)).

One premise needs correction: modern KWin effects are **not unable to access input**. They have grabbed-keyboard,
touch/tablet hooks and, in the pinned KWin 6.7 API, pointer motion/button/axis hooks. What they do not receive is a
portable Wayland client API or ownership of KWin's protocol resources; their power comes precisely from running
inside compositor authority. The API explicitly has no binary compatibility and plugins must match KWin
([input/API warning](../../references/kwin/src/effect/effect.h)).

The policy/eye-candy line is contextual. Blur, translucency, wobbly windows, and zoom primarily alter presentation;
overview/windowview, tile editor, magnifier/accessibility, and system-bell visualization also mediate selection,
input, or safety-visible state. KWin keeps both classes in process because they need live scene and frame hooks
([plugins](../../references/kwin/src/plugins/)).

COSMIC uses no comparable public plugin ABI. Its animation state is embedded in `cosmic-comp`: floating windows keep
`Tiled`, `Minimize`, and `Unminimize` variants with fixed durations and geometry interpolation, while workspace
gestures use an in-tree spring implementation
([floating layout](../../references/cosmic-comp/src/shell/layout/floating/mod.rs),
[spring](../../references/cosmic-comp/src/backend/render/animations/spring.rs)). GNOME brackets the other end:
GNOME Shell is an in-process Mutter `MetaPlugin`, and overview/window animations are GJS actors/easing inside that
process
([plugin](../../references/gnome-shell/src/gnome-shell-plugin.c),
[workspace animation](../../references/gnome-shell/js/ui/workspaceAnimation.js)).

XR raises effects from taste to comfort policy. Large high-contrast surfaces moving or scaling across much of the
field of view create optic flow and vection; Meta explicitly correlates discomfort with amount/speed of optic flow
and recommends predictable movement, limiting acceleration, and comfort alternatives
([optic flow](https://developers.meta.com/horizon/resources/locomotion-design-reduce-optic-flow/),
[comfort](https://developers.meta.com/horizon/design/comfort/)). The shell must also preserve a head-tracked stable
frame when an animation misses its deadline.

**Verdict — effects/animations: `authority-only`.** Implement an in-process Rust module interface over scene
handles, declared animation intent, and a small set of curves. zxr owns hard caps on angular velocity/acceleration,
scale change, occupied field of view, duration, flashing, passthrough occlusion, and frame deadline. Configuration
and theme assets may be external; an ordinary Wayland client must never receive scene-wide paint/input authority.

## Addendum A5 — Systematic staging/unstable/experimental sweep

The following is the complete remainder after subtracting protocols already treated in §§2/6 and A1–A4 from the
pinned clone's actual XML inventory
([staging](../../references/wayland-protocols/staging/),
[unstable](../../references/wayland-protocols/unstable/),
[experimental](../../references/wayland-protocols/experimental/)).

| Protocol(s) | Mura relevance verdict |
|---|---|
| `alpha-modifier-v1` | Implement for correct translucent 2D composition and scanout hints; zxr still resolves final alpha in its linear composition pass. |
| `commit-timing-v1`, `fifo-v1`, `tearing-control-v1` | Implement timing/fifo for compatible 2D clients, but translate them into zxr's OpenXR-paced scheduler. Commit timing is a desired earliest presentation time; FIFO prevents superseding queued commits; tearing is only a hint and must never tear the HMD projection. “Async” can reduce a client's queue latency, not bypass `xrWaitFrame`/one-layer composition. |
| `content-type-v1` | Useful hint (`photo`, `video`, `game`) for scaling/color/power policy; never trust it to relax security or comfort limits. |
| `ext-background-effect-v1` | Optional blur/background sampling request. Implement only with bounded compositor-owned kernels; it exposes no pixels to the client. |
| `ext-transient-seat-v1` | Privileged creation of short-lived virtual seats; useful to portal-mediated remote/VM sessions, hidden from ordinary clients. |
| `pointer-warp-v1` | Compatibility for relative-input games after compositor validation; never warp a controller ray or head pose. |
| `single-pixel-buffer-v1` | Cheap solid-color surfaces for panels/backgrounds; straightforward and useful. |
| `xdg-dialog-v1`, `xdg-system-bell-v1` | Implement modal-dialog relationship and system-bell request; zxr chooses spatial attention cues and caps flash/audio intensity. |
| `xdg-toplevel-drag-v1` | Implement for dragging a toplevel with DnD. It directly bears on the open 3D DnD question: base protocol authenticates the drag/toplevel relationship, while zxr must add ray/grab pose, 3D target volumes, and place transfer. |
| `xdg-toplevel-tag-v1` | Privileged tag assignment for shell/task routing; potentially useful for restore/place policy, so expose only to trusted launch/session components. |
| `xwayland-shell-v1` | Required rootless Xwayland association/serial path; compositor-private to the Xwayland instance. |
| `drm-lease-v1` | Already decided: Monado consumes leases in desktop/dev mode and appliance direct display bypasses them ([research 10](10-xr-wayland-protocol-comparison.md), [ADR 0006](../architecture/adr/0006-compositor-strategy.md)). |
| `fractional-scale-v1` | Required 2D compatibility; advertise preferred buffer scale for spatial quads while zxr owns metric/angular size. |
| `linux-drm-syncobj-v1` | Already a zxr-shell-v2 transport assumption for color/depth dma-bufs; implement and qualify driver timelines ([composition note](../architecture/zxr-shell-v2-composition.md)). |
| `cursor-shape-v1` | Already covered in §2.10/§6; implement for 2D pointer imagery, not hand/ray authority. |
| `relative-pointer-v1` + `pointer-constraints-v1` | Required together for 2D games and remote/VM viewers embedded in XR. Lock/confine the emulated 2D pointer only; provide an explicit compositor escape gesture and never constrain head/controller tracking. |
| `keyboard-shortcuts-inhibit-v1` | Required for games/VMs to request unmodified keyboard delivery, but zxr retains reserved safety, lock, recenter, and escape chords. |
| `pointer-gestures-v1` | Implement touchpad swipe/pinch/hold compatibility; do not reinterpret them as hand-tracking gestures. |
| `primary-selection-v1` | Implement middle-click/primary-selection compatibility for Linux apps; keep it separate from clipboard history and shared-space exposure. |
| `xdg-foreign-v1/v2`, `xdg-output-v1` | Implement v2 foreign parent/export handles where toolkits need them; xdg-output is legacy logical-output metadata now largely folded into `wl_output` v4, retained for compatibility. |
| `tablet-v1/v2`, `input-timestamps-v1` | Implement tablet v2 and input timestamps for creative apps/latency accounting; v1 is legacy. Tablet coordinates remain on a focused 2D plane unless a future spatial stylus protocol exists. |
| `linux-dmabuf-v1`, `linux-explicit-synchronization-v1` | linux-dmabuf is load-bearing. Keep legacy sync-file explicit synchronization for clients while preferring staging syncobj timelines for zxr native color/depth transport. |
| `xwayland-keyboard-grab-v1` | Xwayland-only compatibility for fullscreen games/VMs; filter reserved XR system chords. |
| `xdg-shell-v5/v6`, `text-input-v1`, `input-method-v1`, `fullscreen-shell-v1` | Historical/legacy XMLs: do not design new code around them. Serve only where toolkit/Xwayland compatibility evidence requires it; current xdg-shell/text-input-v3/input-method-v2 paths win. |
| `xx-session-management-v1` | Superseded by staging `xdg-session-management-v1`; do not advertise in a new zxr session. |
| `xx-text-input-v3` + `xx-input-method-v2` + `xx-keyboard-filter-v1` | Active experimental redesign cluster. Track upstream, but ship current text-input-v3/input-method-v2 first; keyboard filtering is privileged IME authority and must be global-filtered. |
| `xx-hotkey-v1` | Promising protocol-level registration model, still experimental. Prefer the GlobalShortcuts portal for applications; reserve raw XR gestures to zxr. |
| `xx-fractional-scale-v2` | Experimental two-coordinate-space replacement; track, but ship staging v1 until governance/adoption settles. |
| `xx-cutouts-v1` | 2D display notches/corners; mostly irrelevant to HMD optics. Could describe companion-display cutouts, never lens hidden-area meshes. |
| `xx-zones-v1` | Experimental client-specific positioning zones are relevant precedent for “places,” but are 2D and client-positioning-oriented. Do not overload them with metres/anchors; use zxr place extensions. |

The timing cluster has one governing rule: **OpenXR owns physical presentation cadence**. A 2D client may request
when its next commit becomes eligible and whether old commits may be superseded; zxr samples the newest eligible,
ready buffer before its XR cutoff. No Wayland timing request may add an unsignaled dependency to the headset frame or
cause asynchronous scanout into the single OpenXR projection layer.

## Addendum A6 — “XDG” disambiguation and cross-desktop specs

“XDG” names three unrelated families and must not be used without a qualifier:

1. the freedesktop **Cross-Desktop Group specification family** (files, directories, icons, MIME, D-Bus services);
2. the **`xdg_*` Wayland protocol namespace** (`xdg-shell`, activation, session management, etc.); and
3. **xdg-desktop-portal**, a D-Bus permission/API broker with desktop-specific backends.

This mirrors the architecture distinction: implementing an `xdg_` Wayland global says nothing about desktop-entry
parsing or portal coverage. The local spec catalog explicitly mixes local, external, draft, and X11-only documents
([catalog](../../references/xdg-specs/spec-index.toml)); versions below come from its revision manifest
([revisions](../../references/xdg-specs/spec-revs.toml)).

| CDG specification | Status and KDE/GNOME reality | Mura contract |
|---|---|---|
| [Base Directory](../../references/xdg-specs/basedir/basedir-spec.xml) | 0.8; tiny, old, and load-bearing. Both stacks use `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_CACHE_HOME`, state/runtime dirs through GLib/Qt/KF. Divergence is mostly fallback paths. | Use it everywhere; immutable Nix store assets do not erase per-user config/state/cache semantics. |
| [Desktop Entry](../../references/xdg-specs/desktop-entry/desktop-entry-spec.xml) | 1.5; load-bearing launcher/app identity format. KDE `KService` and GNOME `GDesktopAppInfo` implement core keys, visibility, actions, MIME declarations, and `Exec`; vendor keys differ. GNOME does not “ignore desktop entries,” but does not use the menu hierarchy for its overview. | Launcher must parse via a mature library, honor `Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`, `TryExec`, field-code quoting, DBus activation, actions, and desktop-file ID. Never hand-roll `Exec`. |
| Icon Theme / Icon Naming | 0.13/0.8-era, externally managed in the catalog rather than cloned under this tree ([index](../../references/xdg-specs/spec-index.toml)). KDE and GNOME both rely on name lookup but ship different themes/fallbacks. | Ship `hicolor` fallback plus spatial theme; use spec lookup for launcher, notifications, SNI, and toplevel-icon names. Missing names must degrade to a placeholder. |
| [MIME Applications](../../references/xdg-specs/mime-apps/mime-apps-spec.xml) + Shared MIME Info | MIME-apps 1.0.1; load-bearing default/recommended application mapping. KDE and GNOME both implement it, with UI/policy differences and shared-mime-info maintained externally. | Use for “open with” and defaults; portals remain the sandbox-aware chooser/launch path. |
| [Autostart](../../references/xdg-specs/autostart/autostart-spec.xml) | 0.5 and effectively mature/frozen. Both desktops support `.desktop` autostart, but modern sessions increasingly translate it to systemd user services; the generator handles visibility/`TryExec` but skips `X-GNOME-Autostart-Phase` ([generator](https://man7.org/linux/man-pages/man8/systemd-xdg-autostart-generator.8.html)). | `mura-session.target` owns native shell services. Start third-party XDG autostart via `xdg-desktop-autostart.target`; mark native units `X-systemd-skip=true`; deduplicate them during restore. |
| [Desktop Menu](../../references/xdg-specs/menu/menu-spec.xml) | 1.1, elaborate XML merge/query hierarchy. KDE still consumes menu/category structure; upstream GNOME Shell explicitly stopped using the menu spec for Overview organization ([GNOME statement](https://lists.freedesktop.org/archives/xdg/2013-December/013060.html)). Effectively dead as a universal shell UI contract. | Do not build launcher architecture around `.menu` layout. Index desktop entries/categories directly; optional compatibility importer only. |
| [Trash](../../references/xdg-specs/trash/index.rst) | 1.0; stable filesystem convention implemented by KDE KIO and GNOME GIO, including per-mount trash when permissions allow. | Use GIO/KIO-compatible trash semantics in file UI; not a compositor service. |
| [Desktop Notifications](../../references/xdg-specs/notification/notification-spec.xml) | Active D-Bus spec 1.3 dated 2024-08-18, not frozen: it added activation-token signaling. Plasma and GNOME Shell both provide session-scoped `org.freedesktop.Notifications` servers but differ in hints, persistence, actions, and presentation. | Notification daemon implements mandatory calls/signals and capability negotiation; use the 1.3 activation token before `ActionInvoked`; treat all hints as optional and let zxr enforce spatial comfort/DND. |
| [StatusNotifierItem](../../references/xdg-specs/status-notifier-item/status-notifier-item-spec.xml) | Draft 0.1/TBD despite KDE, COSMIC, Waybar and sfwbar deployment; GNOME core intentionally does not host it. The `org.kde.*` names and DBusMenu dependency are de facto, not a completed standard. | Compatibility panel feature, never the sole route to critical controls. Run watcher + host separately as in A3. |

The design-changing divergences are concrete. The launcher can rely on desktop entries, basedir, icon lookup, and
MIME defaults, but **not** on the menu spec producing one cross-desktop hierarchy. Notifications can rely on the D-Bus
method/signals, but must capability-test actions, persistence, markup, sound, and activation tokens. Autostart is an
input compatibility format, while `mura-session.target` is the authority and supervision graph. SNI is optional
compatibility UI, not notification delivery and not a safety/status authority.
