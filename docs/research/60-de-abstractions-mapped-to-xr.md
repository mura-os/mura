# 60 — The desktop environment's abstractions, mapped and translated to the XR compositor

**Research date:** 2026-09-26. **Question:** a compositor is one node of a desktop environment.
Every abstraction a 2D DE has — layers, workspaces, window-management policy, foreign sessions,
proxied clients, 3D processes, notifications, the tray, launcher/dock, OSD and keyboards, idle
and lock, capture and consent, clipboard and drag-and-drop, decoration/activation/sandboxing,
accessibility, outputs — must translate to Mura **as is**, through a **spatial extension** (the
`zxr_`/`zspatial_` drafts), or be **new**. Most is translation; this document says which is
which, so the program spec ([specs/zxr-core.md](../../specs/zxr-core.md)) and the registry's
compositor-plane rows are written from evidence, not from the drafts' assumptions. Companion to
[research/59](59-xr-compositor-architecture-from-comparables.md) (the compositor's own
mechanisms); the two share the seam table in §18.
**Method:** one fixed row per abstraction — the 2D standard → how GNOME, KDE, COSMIC, niri and the
XR compositors implement it and **why** (`references/<clone>/path:line`; [external] where no
clone) → the Mura document that already owns it → translation class → owning process in ADR
0012's four seam classes (separate client over a standard seam; in-process plugin; authority-only;
zxr-private protocol) → the first milestone that needs it (R0/G1/M1/M2/G3). Lineage first where
the lineage speaks (motorcar's thesis has a scene graph and 3D input; it has no shell, no
workspaces, no notifications — `motorcar-thesis/chapters/Implementation.tex:80` mentions only
"window decorations or docks" as scene-graph content). **Budget impact:** a research document;
each row's owning process is what the budget is later charged to.

## 1. Layers

**Lineage.** motorcar: a scene graph with `PhysicalNode`s (hardware: displays, viewpoints,
pointing devices) and `VirtualNode`s (content), physical never parented under virtual
(`Implementation.tex:80-87`); 2D surfaces are textured planes; no notion of shell layers.
wxrc: a flat view list; 2D drawn depth-masked after the scene (`render.c:411-420`).
**2D standard.** `wlr-layer-shell`: four layers "ordered by z depth, bottom-most first.
Traditional shell surfaces will typically be rendered between the bottom and top layers.
Fullscreen shell surfaces are typically rendered at the top layer"
(`wlroots/protocol/wlr-layer-shell-unstable-v1.xml:76-89`), plus exclusive zones with the
panel/notification/wallpaper examples (`:152-181`). KWin's `Layer` enum adds
Notification / CriticalNotification / OnScreenDisplay above the NETWM set so that
"notifications above all but active fullscreen; critical above fullscreen; OSD above all for
immediate feedback" (`kwin/src/effect/globals.h:164-177`, `layers.cpp:30-40`). mutter's
`MetaStackLayer` "MUST be in the order of stacking" (`meta/meta-enums.h:252-273`).
**XR comparables.** OpenXR composes layers by painter's algorithm in submission order
(`openxr-docs/…/rendering.adoc:1143-1147`); Monado sorts overlay *clients* by `z_order`
(`comp_multi_system.c:210-225`); wayvr sorts its overlays by distance and z_order
(`openxr/mod.rs:443-467`). visionOS's "upper limb visibility" (the contract
`mura.xr.passthrough.handCutout.upperLimbVisibility` already mirrors) is the only shipping
precedent for a *foreground* cutout layer — [external], closed platform, engineering evidence
only.
**Mura already designed.** `zxr-layer-anchoring-v1` rev 2: frames head/body/hand/world/docked +
angular size/pose + exclusive angular bands (`protocols/zxr-layer-anchoring-v1.xml:59-75`);
`perception-passthrough-hands.md` (the environment layer and the hand cutout as compositor
layers fed by perception services over the intake protocol, `specs/perception-intake.md`).
**Translation.** The four wlr layers become **depth bands relative to the 2D/3D window tier**,
each band an anchoring frame: *environment* (background: passthrough, wallpaper, a virtual
scene — fed by a perception producer through the intake, drawn first), *bottom* and *top* shell
layers (panels, docks — separate clients over layer-shell + anchoring, in head/body/docked
frames), the window tiers between, *overlay* (OSD, notifications, the lock/greeter scene — head
frame, drawn last), and one layer with no 2D analogue: the **foreground cutout** (the wearer's
hands/limbs composited over everything by the compositor from a perception mask; name to be
settled — "cutout" says the mechanism, "foreground" the layer; the contract already says
`handCutout`). Exclusive zones translate to exclusive angular bands (the draft has them).
**Class:** spatial extension (layer-shell + anchoring) for shell layers; authority-only for the
environment and foreground layers' *composition*; separate service (perception) for their
*content*. **First bites:** environment at M1 (a wallpaper is the minimum), shell layers at M1,
foreground at M4/passthrough bring-up.

## 2. Workspaces, places and anchoring

**2D standard.** `ext-workspace-v1`: "groups of surfaces … The purpose of this protocol is to
enable the creation of taskbars and docks" (`wayland-protocols/staging/ext-workspace/…:32-47`).
**Comparables.** COSMIC: private `cosmic-workspace` v1 → `ext-workspace-v1` → a v2 that
"extends `ext-workspace-v1`" (`cosmic-protocols/unstable/cosmic-workspace-unstable-v2.xml:29-33`)
— the migration precedent `protocols/README.md` already plans for; dynamic workspaces with
pinned, session-stable identity. niri: "dynamic workspaces that can move between monitors …
workspaces do not have indices on their own", scrollable strips so "you generally need fewer
workspaces" (`niri/docs/wiki/Workspaces.md:3-50`). KWin: virtual desktops × Activities, with
session restore dropped because of "the cardinality of windows to activities" (research/34
§KWin). GNOME: ephemeral, identity-free overflow (`gnome-shell/js/ui/windowManager.js`
`_checkWorkspaces`). OpenXR reference spaces: VIEW ("will stay at a fixed point on head-mounted
displays and may be uncomfortable to view if too large"), LOCAL ("world-locked origin,
gravity-aligned"), STAGE (room-scale floor rectangle), LOCAL_FLOOR (`spaces.adoc:169-286`).
**Mura already designed.** [places-model.md](../architecture/places-model.md) / ADR 0016: a
frame graph (map / LOCAL·STAGE / VIEW / hand / docked / shared / vehicle), attachment
constraints, intra-place layouts; `zxr-workspace-v1` rev 2 beside `ext-workspace-v1` (frame
types, kind, anchor state, pose, bounds, currency, batched transitions,
`protocols/zxr-workspace-v1.xml:98-232`); research/34 for the comparables.
**Translation.** A place is an `ext-workspace-v1` workspace (so any pager/dock client works
unchanged) whose *group* is a frame; the spatial fields ride the `zxr_workspace` extension in
COSMIC's v2-extends-v1 shape. The compositor owns which OpenXR reference space each frame maps
to (VIEW = head frame, LOCAL/LOCAL_FLOOR = world frame, STAGE = the room bound where the runtime
has one); recentering is the runtime's, currency is the compositor's. **Class:** standard seam
(`ext-workspace-v1`) + spatial extension; the model is authority-only. **First bites:** M1
(one world frame + one head frame; the pager after M1 as §5.1 already stages).

## 3. User- and developer-defined window management

**2D comparables.** river (pinned): "river does not combine the compositor and window manager
into one program" — the WM is a separate process over `river-window-management-v1`, to
"Significantly lower the barrier … Allow implementing Wayland window managers in high-level GC
languages without impacting compositor performance … Allow hot-swapping … Promote diversity"
(`river/README.md:34-38,71-81`); the WM owns dimensions, fullscreen, focus, bindings *and*
position/z-order/decorations, the compositor keeps rendering, protocols, Xwayland
(`river/protocol/river-window-management-v1.xml:26-54`; the older `river-layout-v3`, which
externalised only geometry proposals, is gone from the pin). KWin: in-process JS/QML scripting
against live objects and a `TileManager` per output — "Scripting moves policy code, not
authority" (research/30 §3.3; `kwin/src/tiles/tilemanager.h:33-36`). niri: a declarative
`layout {}` in the config (`docs/wiki/Configuration:-Layout.md`). mutter: `MetaPlugin` hooks and
gnome-shell's in-process JS (`mutter/src/meta/meta-plugin.h:54-110`). cosmic-comp: tiling in
`shell/layout/`, the overview a separate client.
**Mura already designed.** research/30 §5.3 and ADR 0012 §2–§3: WM policy is **authority-only**
with "declarative rules plus a restricted in-process scripting API"; "An all-powerful policy
client would control field of view, safety bounds, focus" (`30:452-458`); external tools write
validated configuration, never hold a live policy socket (`adr/0012:63-67`).
**Translation.** The 2D world splits: KWin/GNOME/niri/cosmic keep policy in-process (scripting,
plugins, config); river alone externalises it, with reasons that are about *developer*
ergonomics (languages, hot-swap, diversity), not safety. In XR the policy decides where content
sits relative to the wearer's body and the boundary — the safety argument ADR 0012 makes has no
2D counterpart. But river's reasons are real and the owner named "how users/developers could
define their own workspace management" as a requirement. **Class:** the ruled shape is
in-process plugin (KWin) + declarative config (niri) + D7's settings for the knobs; river's
out-of-process WM is the counter-comparable. **§18 Q2** for the owner: keep ADR 0012's verdict
(policy in-process, configuration external) or open a river-shaped policy protocol *bounded by*
the compositor's safety invariants (frame limits, boundary, focus rules the WM cannot override).
**First bites:** M1 (a fixed layout); the seam at M2+.

## 4. Foreign sessions — GNOME and KDE inside the headset

**Comparables.** KWin-VR: one Qt Quick 3D XR scene inside KWin, KWin's client buffers as
textures, KWin owning the OpenXR session (research/31 §2); Vlad's stated preference: "we'd
rather integrate with something … provide info about windows … let them compose overlays"
(`31:43-47`). Portal ScreenCast: `SelectSources` monitor/window/virtual → PipeWire streams
(`xdg-desktop-portal/data/org.freedesktop.portal.ScreenCast.xml:19-25,324-330`); the GNOME
backend over mutter's `org.gnome.Mutter.ScreenCast`, KDE's over `zkde_screencast_unstable_v1`
(`xdg-desktop-portal-kde/src/waylandintegration.cpp:402-404`), COSMIC's over
`ext-image-copy-capture` (`xdg-desktop-portal-cosmic/src/wayland/mod.rs:1-36`) — capture, not
delegation: no pacing contract, no surface tree, no input (research/32 §2). gamescope: the
whole foreign session as one nested output in "its own personal Xwayland sandbox desktop"
(`gamescope/README.md:9-12`).
**Mura already designed.** [foreign-session-integration.md](../architecture/foreign-session-integration.md)'s
four modes (native; waypipe-proxied; nested compositor as one plane — "works today with zero
new protocol"; per-toplevel delegated export); ADR 0014 and `zspatial-toplevel-export-v1` rev 3
(exported tree/nodes, timelines, pacing, input; `protocols/zspatial-toplevel-export-v1.xml`);
`specs/toplevel-export-producer.md`; producer briefs for KWin and mutter.
**Translation.** Mode 3 (nested session = one plane) is gamescope's shape and needs nothing new
— it is what M1 can ship for "a GNOME session in the headset". Mode 4 is the consumer side of
the export protocol: bind the privileged manager, import dmabufs, return release points, drive
pacing from `xrWaitFrame`, rebuild the surface tree, forward input per node — ADR 0014 M-A after
M1 and a smithay reference producer. **Class:** standard seam (nested output; portal) at M1;
zxr-private/`zspatial_` protocol at M-A. **First bites:** M1 (mode 3), post-M1 (mode 4).

## 5. Off-device compute — proxied clients

**Comparables.** waypipe (Rust; `references/waypipe`): dmabufs are **replicated** to the far side
with damage diff/compression or **video-encoded** (`--video`), never passed through
(`README.md:217-222`; research/19 §141-188); `wp_linux_drm_syncobj_v1` is proxied
(research/19 §190-202); protocols it does not understand pass through, fd-carrying ones it does
not support (drm lease, export-dmabuf, `wl_drm`) are dropped (`README.md:181-197`). sommelier
(ChromeOS): "a Wayland compositor that delegates compositing to a 'host' compositor", shm often
not shareable across the VM boundary so virtwl copies are interposed
(`platform2/vm_tools/sommelier/README.md:1-76`). Runtime-level streaming (WiVRn, ALVR) is the
other place compute can sit — the whole XR frame, not a client.
**Mura already designed.** `spikes/waypipe-vm.md`, research/19.
**Translation.** A proxied client is an ordinary client if zxr (research/19 §644-691): never
assumes buffer locality or allocation identity (waypipe hands replicas); intersects, not
assumes, the modifier set; advertises the ordinary desktop set (`wl_compositor`, subcompositor,
shm, seat, output, `xdg_wm_base`, data device; dmabuf v4+, viewporter, fractional scale,
decoration, syncobj); does not depend on globals waypipe drops; gates capture behind
security-context. Placement treats it as a local `xdg-shell` plane. **Class:** as is (no
compositor feature; a discipline list in the spec). **First bites:** M1 (it is a test of M1,
not a feature).

## 6. 3D processes — `zxr-shell-v2`

**Lineage, as a derivation** (verified against the three XMLs;
`motorcar/src/protocol/motorcar.xml`, `wxrc/protocol/zxr-shell-unstable-v1.xml`,
`protocols/zxr-shell-v2.xml`):

| concept | motorcar | zxr-shell-v1 (wxrc) | zxr-shell-v2 | change and reason |
|---|---|---|---|---|
| role factory | `motorcar_shell.get_motorcar_surface(clipping, depth)` | `zxr_shell_v1.get_xr_surface` + `create_composite_buffer` | `zxr_shell_v2.get_xr_surface` + transport/depth negotiation | keep; drop the `wl_buffer`-wrapper factory |
| camera | split view + projection on `motorcar_viewpoint` | folded `mvp_matrix` on `zxr_surface_view_v1` | split matrices inside an atomic `zxr_frame_timing_v2` snapshot | the fold was a regression (08 Part 3); the snapshot forbids "latest matrix + latest colour" (composition §7.2) |
| colour + depth | depth packed into a colour viewport (EGL had no depth mode) | typed pixel/depth buffers, depth unused | typed slots with depth encoding metadata, explicit sync | typed kept; sync new |
| submit | EGL attach | attach + `get_wl_buffer` → `wl_surface.attach` | `zxr_surface_v2.submit(frame, slot)` | atomicity |
| size / clipping | `request_size_3d`/`set_size_3d`; cuboid/portal | none | configure/ack + bounds; `set_clipping_mode` | resurrected |
| input | `motorcar_six_dof_pointer` | none (TODO) | `zxr_ray_v2`, `zxr_pointer_6dof_v2` | resurrected |
| frame timing | thesis policy only | TODO | `zxr_frame_timing_v2` + feedback | new; shares vocabulary with export pacing |
| geometry on the wire | never | glTF TODO | rejected | "geometry never crosses the wire" (composition) |

**The alternatives, as evidence.** `XR_EXTX_overlay`: separate-process apps composited "on top of
the main OpenXR application" by the runtime, painter-ordered, timing "not … correlated" with the
main app; provisional since 2021 (`openxr-docs/…/extx_overlay.adoc:21-67`; Monado implements it
via `multi_compositor`). stardustxr: clients send scene nodes, the server renders — the
geometry-over-the-wire model the thesis rejected (`stardustxr-server/README.md:1-3`). wayvr: 2D
only, by choice. Monado does not depth-test across layers (research/59 §3).
**Translation.** The lineage's model stands and is the only one that gives inter-client depth
without the runtime's help. What M1 must not preclude (composition §7.3): a depth buffer written
by the 2D tier, per-view render targets, frame ids on submissions, slot-based buffer ownership.
**Class:** zxr-private protocol (ADR 0012 §4). **First bites:** M2; a reference client (the
composition doc's CPU client, M3) validates the contract.

## 7. Notifications

**2D standard.** `org.freedesktop.Notifications` on the session bus
(`xdg-specs/notification/notification-spec.xml:72-83`). **Comparables.** mako and dunst are
layer-shell daemons; the layer choice is reasoned in their man pages: "Using overlay will cause
notifications to be displayed above fullscreen windows, though this may also occur at top
depending on your compositor" (`mako/doc/mako.5.scd:300-313`, default top; dunst default
overlay, `dunst/docs/dunst.5.pod:371-377`). gnome-shell: in-process `FdoNotificationDaemon`
(`js/ui/notificationDaemon.js:15-35`). Plasma: plasmashell owns the bus name
(`plasma-workspace/shell/main.cpp:187-188`). cosmic-notifications: separate layer-shell daemon
[external clone; research/30 §4]. Research/36 §4: head-locked small transients.
**Translation.** As is: a separate daemon on the FDO interface, placed through layer-shell +
anchoring in the *overlay* band of the head frame (mako's shape, spatialised). **Class:**
separate client, standard seam. **First bites:** M1+ (shell content; §5.1 stages it).

## 8. System tray

**2D standard.** StatusNotifierItem/Watcher/Host, still a draft
(`xdg-specs/status-notifier-item/…:33-40`). **Comparables.** Plasma: host applet + KDED watcher;
COSMIC: `cosmic-applet-status-area` with a socket-activated watcher (research/30 §2, `:725-731`);
GNOME removed its legacy tray in 3.25.90 ("Remove legacy status icon tray", `gnome-shell/NEWS:3130`)
and recommends applications not require status icons — SNI lives in an extension [external
reason]. No `ext-tray` exists in `wayland-protocols`.
**Translation.** Two shipping positions with reasons: carry SNI as a separate host applet
(Plasma, COSMIC — the ecosystem of apps that expect it) or refuse it (GNOME — the icons are a
design the shell does not want). Nothing XR-specific decides it. **§18 Q3.** **Class:** separate
client, D-Bus seam either way. **First bites:** post-M1 shell content.

## 9. Launcher, dock, taskbar, overview

**2D standards.** `ext-foreign-toplevel-list-v1` ("intentionally minimalistic … additional
functionality … in extension protocols", `…/ext-foreign-toplevel-list-v1.xml:31-38`);
`wlr-foreign-toplevel-management` for control; `xdg-activation-v1` tokens ("for focus stealing
prevention. The activating client will have no way to discover the validity of the token",
`xdg-activation-v1.xml:52-56`); the desktop-entry spec. **Comparables.** cosmic-panel consumes
`ext_foreign_toplevel_handle_v1` and controls through `zcosmic_toplevel_manager_v1`
(`cosmic-panel/…/space/toplevel.rs:4,29`; `cosmic-protocols/…/cosmic-toplevel-management-unstable-v1.xml:31-34`);
cosmic-launcher is a layer-shell client that mints activation tokens before spawning
(research/30 §4). gnome-shell's overview is in-process; plasmashell uses KWin's private
`org_kde_plasma_window_management`, "a DE implementation detail regular clients must not use"
(research/30 §3.5).
**Translation.** As is, on the standard seams (list + activation; control through an extension
until `ext-foreign-toplevel-management` exists), with placement through anchoring frames: a
dock in the body frame, an overview as a place transition (`zxr-workspace-v1`). **Class:**
separate clients; the list/activation globals are the compositor's. **First bites:** M1 for the
globals (M1's acceptance needs launching a terminal and an editor), post-M1 for the clients.

## 10. OSD, virtual keyboard, input method, text input

**2D standards — three directions** (research/30 §2.8): `text-input-v3` (app → compositor),
`input-method-v2` (IME ↔ compositor), `virtual-keyboard-v1` (synthetic keys). **Comparables.**
squeekboard binds both IM-v2 and VK: "It needs to combine text-input and virtual-keyboard
protocols … The virtual-keyboard interface is always present" (`squeekboard/src/submission.rs:5-11`);
wvkbd is layer-shell + VK (`wvkbd/main.c:1-6`); phosh's OSD is in-process, "fed via the OSD …
DBus interface" (`phosh/src/osd-window.c:20-26`); KWin manages an input-method process itself and
restarts it on crash (`kwin/src/inputmethod.cpp:60-112`); cosmic-osd is a separate layer-shell
client. Research/36 §7 for the keyboard's spatial pattern; first-run §4.4 for the input floor.
**Translation.** The compositor serves all three directions (text-input-v3 to apps; IM-v2 and VK
to one privileged keyboard client — squeekboard's shape) and renders nothing itself; the
keyboard is a layer-shell client anchored in the hand/body frame; the OSD a layer-shell client
in the overlay band (cosmic's shape) fed over D-Bus (phosh's interface). **Class:** separate
clients over standard seams; the privilege filter (which client may bind IM-v2/VK) is
authority-only. **First bites:** G1 (the auth scene needs text input; the digit pad is
in-compositor per session-auth), M1 for apps.

## 11. Idle, lock, session

**2D standards.** `ext-idle-notify-v1`, `idle-inhibit`, `ext-session-lock-v1` ("If the client
dies while the session is locked the session remains locked, possibly permanently depending on
compositor policy", `ext-session-lock-v1.xml:25-36`). **Comparables.** swaylock is a lock
client; KWin runs `kscreenlocker_greet` as a separate process over a private socketpair and
restarts it on crash (`kscreenlocker/ksldapp.cpp:199-206`; research/59 §10); gnome-shell and
phosh lock in-process; COSMIC has a greeter plus a private lock-layer protocol.
**Mura already designed.** ADR 0007: the lock is **internal compositor state** with three
invariants; `ext-session-lock` only on the dev/desktop profile; PAM in `mura-authd` (landed).
**Translation.** As ruled: `ext-idle-notify` and `idle-inhibit` as is (the compositor is the idle
authority; doff is research/42's input, §3b's lifecycle); the lock scene in-compositor (GNOME's
shape); `ext-session-lock` offered only where a lock client is wanted. **Class:** authority-only
(lock), standard seams (idle). **First bites:** G1 (the greeter scene is the lock scene's
sibling), G3 (doff/lock/unlock end to end).

## 12. Capture, sharing, consent

**2D standards.** Portals (`ScreenCast`, `Screenshot`, `RemoteDesktop`) → a backend that asks
the compositor → PipeWire; the compositor-side protocols: mutter's D-Bus, KWin's
`zkde_screencast`, COSMIC's `ext-image-copy-capture` (research/30 §2.9; the three backends'
sources above); `ext-image-capture-source` + `ext-image-copy-capture` supersede `wlr-screencopy`.
**Mura already designed.** [spatial-sharing.md](../architecture/spatial-sharing.md) (implement
the ext capture pair; day one through `xdg-desktop-portal-wlr`), `specs/spatialcast-portal.md`
(spatial sources), the delegation producer spec for the export direction.
**Translation.** As is for flat capture (ext pair + an existing wlr backend); spatial extension
for spatial sources (spatialcast). Consent is the portal's, never the compositor's. **Class:**
standard seam; the portal backend is a separate process. **First bites:** post-M1 (M4 for
spatial sources).

## 13. Clipboard, drag-and-drop, primary selection, data control

**2D standards.** `wl_data_device` (DnD with a drag surface/icon role), `primary-selection`,
`ext-data-control-v1` for clipboard managers (research/30 §2.10). **Comparables.** niri and
cosmic render the DnD icon with the pointer (`niri/src/niri.rs:362,3907-3958`;
`cosmic-comp/src/backend/render/cursor.rs:419-433`). **No XR compositor implements
drag-and-drop between planes** — wayvr's "DND" is do-not-disturb (`subsystem/dbus/notifications.rs:127-129`);
simula shells out to `xclip`. Research/30 §5 warns about secret previews in XR clipboard UIs.
**Translation.** Clipboard and primary selection as is. DnD *within* a plane is ordinary
(`wl_data_device` in surface-local coordinates). DnD *across* planes has no comparable: the drag
icon would follow the ray and the drop target is whichever plane the ray hits — mechanically the
same as pointer motion across surfaces, so the 2D semantics may simply hold. **Class:** as is;
cross-plane DnD recorded as a spec open item with the owner as decider, not blocking. **First
bites:** M1 (copy/paste is in M1's acceptance).

## 14. Decoration, activation, security context, global shortcuts

**Standards and reasons.** `xdg-decoration-unstable-v1`: negotiation, the compositor selects the
mode (research/30 §2.5; the registry already says force server-side, `component-registry.md:111`).
`security-context-v1`: "intended to be used by sandboxes", nesting forbidden because it "can
potentially allow privilege escalation" (`security-context-v1.xml:27-40`); identity for
attenuation, not permission (research/30 §2.7). `xdg-activation`: tokens (§9 above).
Global shortcuts: on Wayland "KWin legitimately receives all input, so KGlobalAccel was refactored
into a library linked into KWin" (research/30 §3.4); apps use the GlobalShortcuts portal.
`ext-transient-seat` for remote-desktop-style temporary seats.
**Translation.** All as is; decoration server-side in 3D (the plane's frame *is* the decoration
and the grab handle — a planar CSD makes no sense on a tilted quad); security-context gates the
capture and privileged globals for sandboxed and proxied clients (§5). **Class:** authority-only.
**First bites:** M1 (decoration, activation), post-M1 (security-context, shortcuts).

## 15. Accessibility

**Comparables.** GNOME/KDE: AT-SPI2, with the compositor supplying focus and geometry; COSMIC:
AccessKit in libcosmic behind `a11y`, a private `cosmic_a11y_manager_v1` for shell features
(`cosmic-protocols/unstable/cosmic-a11y-unstable-v1.xml:25-28`). **Mura already designed.**
[spatial-a11y.md](../architecture/spatial-a11y.md): AT-SPI2 unchanged for apps; Newton/AccessKit
tracked, not adopted; three compositor duties (focus, poses/relations, place membership);
`zspatial-a11y` name reserved.
**Translation.** As is for apps; the compositor's duties are authority-only and start at M1
(focus and window poses are M1 state). **Class:** authority-only + a reserved protocol. **First
bites:** M1 (duties), later (carrier).

## 16. Power, docked and flat output

**Standards.** `wlr-output-management`, `xdg-output`, `wp_presentation` (research/30 §2.10).
gamescope: one logical output, headless (`wlserver.cpp:2237-2303`). **Mura already designed.**
ADR 0015 (docked mode: the same session presents flat on a docked connector — presentation
policy, not a second model); composition constraint 5: no window↔output binding; docked and
virtual screens are the recorded exceptions (`zxr-shell-v2-composition.md:266-280`).
**Translation.** The headset path has no `wl_output` in the 2D sense — zxr advertises one logical
output for clients that need it (gamescope's shape); docked mode adds a real DRM output later.
Output management is not offered on the headset (nothing to manage). **Class:** authority-only.
**First bites:** M1 (one logical output), the docked rung for the rest.

## 17. The protocol sweep — what else a compositor advertises

From the comparables' sources (`niri/src/niri.rs:81-115`; `cosmic-comp/src/state.rs` +
`handlers/`; `kwin/src/wayland/*`; `mutter/src/wayland/meta-wayland-*.c`; `weston/libweston/*`;
`gamescope/src/wlserver.cpp:2237-2303`):

| protocol | niri | cosmic | kwin | mutter | weston | gamescope | zxr |
|---|---|---|---|---|---|---|---|
| xdg-shell, viewporter, presentation-time, single-pixel-buffer | Y | Y | Y | Y | Y | partial | **M1** |
| fractional-scale | Y | Y | Y | Y | N | N | **M1** (planes are resolution-independent; clients need a scale) |
| linux-dmabuf v4+ (feedback), syncobj | Y / N | Y / Y | Y (v6) / Y | Y / Y | Y / ? | Y / Y | **R0** |
| pointer-constraints, relative-pointer | Y | Y | Y | Y | Y | Y | **M1** (games and 3D viewers) |
| cursor-shape, pointer-gestures | Y | Y | Y | Y | ? | N | **M1** |
| tablet-v2 | Y | Y | Y | Y | Y | N | not applicable in XR at v1 (no tablet); revisit at docked |
| idle-inhibit, keyboard-shortcuts-inhibit | Y | Y | Y | Y | ? | N | **M1** |
| security-context | Y | Y | Y | N | N | N | post-M1 (sandboxed and proxied clients) |
| xwayland-shell | N (satellite) | Y | Y | N | N | private | not needed with satellite (research/59 §9) |
| fifo-v1, commit-timing-v1 | N | N | Y | Y | Y | partial | later — the runtime paces; revisit at M4 |
| tearing-control | N | N | Y | N | Y | policy | not applicable (reprojection, not tearing) |
| alpha-modifier | N | Y | Y | N | Y | N | later |
| color-management-v1 | N | N | Y | Y | Y | N | later — HDR panels are a target fact |
| content-type | N | N | Y | N | N | N | later (a "game" hint could relax pacing) |
| xdg-toplevel-drag, -icon, -tag, xdg-dialog, xdg-system-bell | N | N | Y | Y (most) | N | N | later; -tag and -dialog are cheap and help placement |

The sweep found nothing the seventeen rows miss that M1 needs; it found two things worth a line
in the spec: fractional-scale is *more* important in XR (planes have no native pixel density —
the compositor picks a scale per plane from angular size), and tearing/fifo/commit-timing are
the 2D world's answer to a problem the runtime's reprojection and `xrWaitFrame` pacing solve
differently, so they wait.

## 18. The seam table, and what falls out

| abstraction | 2D standard | Mura doc | translation | owning process (ADR 0012 class) | first bites |
|---|---|---|---|---|---|
| layers | wlr-layer-shell | layer-anchoring-v1; passthrough-hands | spatial extension; environment + foreground new | authority (composition); separate service (perception content); separate clients (shell layers) | M1 / M4 |
| places, anchoring | ext-workspace-v1 | places-model, ADR 0016, zxr-workspace-v1 | standard + spatial extension | authority (model); separate clients (pager) | M1 |
| WM policy | — (river protocol; KWin scripting) | ADR 0012 §2–3, research/30 §5.3 | in-process plugin + config (ruled); river counter-comparable | **Q2** | M1 / M2+ |
| foreign sessions | portal ScreenCast; nested output | foreign-session-integration, ADR 0014, toplevel-export-v1 | mode 3 as is; mode 4 zxr-private | standard seam / private protocol | M1 / M-A |
| proxied clients | waypipe | spikes/waypipe-vm, research/19 | as is (a discipline list) | — | M1 |
| 3D processes | — | zxr-shell-v2, composition §7 | zxr-private (the lineage) | private protocol | M2 |
| notifications | FDO Notifications + layer-shell | research/36 §4 | as is, spatial placement | separate client | post-M1 |
| tray | SNI | registry gap | as is or refused | separate client | **Q3** |
| launcher/dock/overview | foreign-toplevel-list, xdg-activation, desktop-entry | research/30 §5, /36 §3 | as is + anchoring | separate clients; globals authority | M1 (globals) |
| OSD / keyboard / IM | text-input-v3, input-method-v2, virtual-keyboard | research/30 §2.8, /36 §7 | as is; keyboard a layer-shell client | separate clients; privilege filter authority | G1 / M1 |
| idle / lock | ext-idle-notify, idle-inhibit, ext-session-lock | ADR 0007 | lock in-compositor (ruled); idle as is | authority | G1 / G3 |
| capture / consent | portals; ext-image-copy-capture | spatial-sharing, spatialcast-portal | as is; spatial sources extension | separate backend | post-M1 |
| clipboard / DnD | wl_data_device, data-control | research/30 §2.10 | as is; cross-plane DnD open | authority | M1 |
| decoration / activation / sandbox / shortcuts | xdg-decoration, xdg-activation, security-context, portal | registry rows | as is; server-side decoration | authority | M1 |
| a11y | AT-SPI2 | spatial-a11y | as is + duties | authority; reserved protocol | M1 |
| outputs / docked | wlr-output-management | ADR 0015, composition c.5 | one logical output; docked later | authority | M1 / docked rung |
| protocol sweep | §17 | — | M1 set fixed | authority | R0–M1 |

**For the owner (Phase C), beside research/59 Q1:**

- **Q2 — window-management policy seam.** (a) ADR 0012 as ruled: policy in-process (KWin's
  scripting shape) with declarative configuration (niri's) and D7's settings for the knobs;
  external tools write config, never hold a policy socket — reason: in XR, placement is a safety
  and comfort matter (field of view, boundary, focus). (b) river's shape: a separate WM process
  over a `zxr_window_management` protocol, **bounded** by compositor-enforced invariants (frame
  limits, boundary, focus rules it cannot override) — reason (river's): developer ergonomics,
  languages, hot-swap, diversity. (c) both: in-process by default, the bounded protocol as the
  developer surface, later. The owner named user/developer-defined workspace management as a
  requirement, which is why this is asked rather than left as ruled.
  **Ruled 2026-09-26: (c)** — in-process default now, the bounded protocol as the developer
  surface after M1 (ADR 0012 amendment). The owner's framing: under X11 the WM was a separate
  process; Wayland folded it in; "in VR/XR it's not obvious how window/workspace management
  should be done yet, and we should allow users/developers flexibility while still allowing them
  to use our compositor."
- **Q3 — the system tray.** (a) carry SNI as a separate host applet (Plasma, COSMIC — apps expect
  it); (b) refuse it (GNOME — the shell does not want status icons). Nothing XR-specific decides
  it; it decides a registry row. **Ruled 2026-09-26: (a), carried** — the owner's general rule:
  shell elements modular, their own processes, so the spatial desktop environment is
  user-configurable (the keyboard and the tray named alongside the WM).

Everything else converged: the layer model, places over `ext-workspace-v1`, foreign sessions in
two modes, the proxied-client discipline, the 3D-process protocol as the lineage's derivation,
and every shell component as a separate client over a standard seam with placement through
anchoring frames — COSMIC's process split, which research/30 already named the closest precedent,
with the spatial frame as the one thing added. Two small items are recorded in the spec as open
with the owner as decider, not asked now: the foreground layer's name, and cross-plane
drag-and-drop.
