# 32 — Per-toplevel zero-copy export/delegation: prior art and producer feasibility

**Status:** research complete. **Date:** 2026-09-23. **Question:** can an existing 2D Wayland
compositor delegate each toplevel's *client-owned buffer stream* to an external XR compositor
(`zxr`), while retaining shell authority and accepting input back?

This is KWin's preferred out-of-process shape: provide window content/information and input, then
let another process compose overlays ([research 31 §1](31-kwin-vr.md),
[kwin!8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671)). It is not capture, protocol
proxying, or a nested desktop.

**Evidence notation.** `[V]` means verified in the pinned local source; `[R]` means a primary
specification, commit, or upstream discussion reports it; `[I]` is a design inference. Paths are
relative to `references/`. Pins: [waypipe a1ffdd8d0f44](https://gitlab.freedesktop.org/mstoeckl/waypipe/-/commit/a1ffdd8d0f44),
[wayland-protocols 819004adb3ab](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/commit/819004adb3ab),
[KWin d84a316bb6a0](https://invent.kde.org/plasma/kwin/-/commit/d84a316bb6a0),
[Mutter 888a7b7dac0c](https://gitlab.gnome.org/GNOME/mutter/-/commit/888a7b7dac0c),
[COSMIC 26daf75d9369](https://github.com/pop-os/cosmic-comp/commit/26daf75d9369), libei
`a9bf31da06f0`, wolf `a1edc1bf44cb`, and KWin VR `ccdd46e`.

---
## 1. Problem statement and four client modes
The useful taxonomy is:

1. **Native client.** The app connects directly to zxr. Zxr owns its `wl_surface` tree, buffers,
   frame callbacks, presentation feedback, and seat events.
2. **Protocol-proxied client.** Waypipe relays the app's Wayland connection. Buffers are replicated
   or encoded across the boundary; at zxr they are ordinary local `wl_buffer`s
   ([research 19 §2.4–2.5](19-wayland-proxying.md)).
3. **Nested compositor as one quad.** A complete 2D session is composited by a nested compositor;
   zxr sees one client surface/output. Internal toplevel identity and popup/input structure stop at
   the nested boundary.
4. **Per-toplevel delegated export.** KWin, Mutter, or a wlroots/Smithay compositor remains the
   client's Wayland server and window manager, but sends zxr each selected toplevel's original
   surface-tree buffers, commit metadata, timing, and lifetime. Zxr places the tree as floating
   quads and returns surface-local input. **This is the missing mode.**

`ext-foreign-toplevel-list-v1` supplies a suitable identity handle, deliberately leaving content
and state to extension protocols (`wayland-protocols/staging/ext-foreign-toplevel-list/
ext-foreign-toplevel-list-v1.xml:30-43,122-218`). `xdg-foreign-v2` is not this seam: it exports a
handle so another client can establish a transient-parent relationship, not read content or send
input (`wayland-protocols/unstable/xdg-foreign/xdg-foreign-unstable-v2.xml:27-47,80-97,155-188`).

### Why the standardized capture pair cannot be stretched into mode 4

`ext-image-capture-source-v1` can make a foreign toplevel an opaque source, but
`ext-image-copy-capture-v1` then asks the *consumer* to allocate and attach a destination buffer.
The compositor copies or renders a flattened image into it
(`wayland-protocols/staging/ext-image-copy-capture/ext-image-copy-capture-v1.xml:27-34,85-103,
180-188,200-282`). Even a GPU-to-GPU render into a client dmabuf remains **copy-capture protocol
semantics**, not forwarding the app's own allocation. `[V]`

The mismatch is structural:

- capture is **damage/request driven**: after the first frame, `capture` may wait indefinitely for
  source content to change (`ext-image-copy-capture-v1.xml:273-282`);
- it reports when a copied frame is ready, but gives no contract by which the consumer's next
  presentation deadline drives `wl_surface.frame` at the producer;
- it flattens the toplevel into one image; there is no subsurface/popup topology, input region,
  synchronized subtree commit, popup grab, or per-surface input path;
- its `presentation_time` reports when source content was presented on a source output, not when zxr
  samples that content into an XR frame (`:311-339`);
- `wl_buffer.release` is explicitly unused for the consumer buffer (`:229-239`).

Hyprland's `hyprland-toplevel-export-v1` has the same category despite its name: the consumer calls
`copy(buffer, ignore_damage)` and receives `ready`; the implementation renders/copies into that
buffer ([protocol](https://wayland.app/protocols/hyprland-toplevel-export-v1),
[source](https://github.com/hyprwm/Hyprland/blob/b6633c41/src/protocols/ToplevelExport.cpp)).
The older `wlr-export-dmabuf` does export compositor-owned output frames, but has no defined
cross-frame lifetime/synchronization; Simon Ser explicitly recommends against it for that reason
([discussion](https://lists.freedesktop.org/archives/wayland-devel/2024-May/043640.html)). None
delegates a client's commit stream. `[R]`

---
## 2. Buffer lifetime and synchronization across a compositor boundary
### 2.1 What waypipe actually solves

Waypipe is the closest cross-boundary lifetime implementation, but its data model is replication:

- shm pools get a local mapping plus a full mirror (`waypipe/src/mainloop.rs:533-551,1125-1168`);
  surface/buffer damage is tracked through scale, transform, and viewporter state
  (`waypipe/src/tracking.rs:1921-1980`; [research 19 §2.3](19-wayland-proxying.md));
- app-side dmabufs are imported, while the display side allocates a **new** dmabuf; damaged segments
  are copied to staging, diffed, compressed, and applied to the replica
  (`waypipe/src/mainloop.rs:1171-1262,2899-3020`; `waypipe/src/dmabuf.rs:2342`;
  [research 19 §2.4](19-wayland-proxying.md));
- ordinary `wl_buffer.release` remains an end-to-end Wayland event from the real display compositor
  to the app. Waypipe preserves object identity and forwards the event after the replica has ceased
  use; it does not fabricate an early release. Its shadow fd/object remains independently tracked
  (`waypipe/src/tracking.rs:109-128`; generated release opcode at
  `waypipe/src/wayland_gen.rs:4393-4407`). `[V]`

Explicit synchronization makes the lifetime bridge visible. Waypipe imports the app's syncobj fd as
a Vulkan timeline, creates a peer timeline (`OpenTimeline`), and stores acquire/release points per
commit (`waypipe/src/mainloop.rs:626-633,1265-1294,1841-1881`;
`waypipe/src/tracking.rs:2340-2454`). On the display side, completion of all replica-apply tasks
signals the acquire point before the real compositor samples the replica
(`waypipe/src/tracking.rs:1990-2068`; `waypipe/src/mainloop.rs:2660-2676`). The real compositor's
release point is waited on and sent back as `SignalTimeline`; app-side pruning removes each
outstanding buffer use and signals the corresponding timeline point
(`waypipe/src/mainloop.rs:2637-2657,4871-4884,5239-5254`). `[V]`

Thus waypipe preserves the invariant, “the app may reuse only after the ultimate consumer is done,”
despite having two allocations. The transferable lesson is the release relay, not its pixel-copy
machinery.

### 2.2 Same-GPU delegation: one allocation, three parties

On one DRM device, the producer can duplicate the client's dmabuf plane fds with `SCM_RIGHTS` and
send format, modifier, offsets, pitches, dimensions, transform, source box, color metadata, and
damage. Zxr imports exactly that allocation. No GPU render or copy is required. Import compatibility
must be checked against zxr's device/modifier set; “same machine” is insufficient on hybrid GPUs.
Shm is necessarily a copied fallback (or an explicit unsupported profile), not zero-copy. `[I]`

Three release designs are plausible:

1. **Producer-relayed release — recommended.** The producer keeps its normal buffer reference,
   gives zxr a lease on duplicated fds plus an acquire fence/point, and waits for a zxr GPU-complete
   release ack. Only after *both* local composition and zxr are done does it drop the final
   `GraphicsBufferRef`/Smithay `Buffer` and signal the client's original release point. This
   preserves one authority for client-visible release and contains a misbehaving consumer.
2. **One client syncobj shared tri-party.** Zxr imports the client's timeline and signals a point.
   This does not by itself express the AND of “producer done” and “consumer done”; whichever party
   signals first can release too early. A producer-owned join point still has to aggregate both,
   so sharing the client's release timeline buys complexity, not semantics.
3. **Consumer retains latest-N.** The producer lets zxr keep a bounded rolling set and releases
   older buffers on replacement. This is a useful backpressure policy, but not a synchronization
   primitive: GPU completion still needs an ack/fence, and a stopped consumer otherwise pins client
   buffers forever.

`linux-drm-syncobj-v1` supplies the exact reusable rules. A commit has one acquire point that must
signal before sampling and one release point signaled when compositor use ends; points are
double-buffered with the attached buffer
(`wayland-protocols/staging/linux-drm-syncobj/linux-drm-syncobj-v1.xml:127-151,180-259`).
`wl_buffer.release` becomes undefined while explicit sync is active (`:140-147`). Release points can
complete out of commit order, and signaling point N signals all prior points, so the spec strongly
recommends a separate release timeline per buffer (`:210-235`). Those rules prohibit a single
monotonic “all exported frames” release timeline. `[V]`

KWin already embodies the recommended relay primitive. `GraphicsBufferRef` increments a buffer ref;
the zero-ref transition clears all stored release points and emits `released`
(`kwin/src/core/graphicsbuffer.h:62-68,143-251`;
`kwin/src/core/graphicsbuffer.cpp:37-66,103-107`). Linux-dmabuf buffers translate that to
`wl_buffer.release` (`kwin/src/wayland/linuxdmabufv1clientbuffer.cpp:440-445`), while a committed
syncobj release point is attached to the same buffer reference
(`kwin/src/wayland/surface.cpp:623-652`). An exporter can hold one extra ref until zxr's fence
completes instead of inventing another client lifetime. `[V]`

Disconnect policy is mandatory: stop new exports, treat all outstanding consumer leases as released
only after locally submitted GPU work is fenced or the GPU context is torn down, then drop refs.
Never block the producer event loop waiting for a consumer; cap outstanding buffers/bytes and revoke
a stalled export. `[I]`

---
## 3. Presentation semantics: consumer-driven pacing
Core Wayland pacing is producer-compositor driven. A `wl_surface.frame` callback tells a client it
may begin another update; `wp_presentation` later says whether one commit became visible or was
discarded, in one stable presentation clock
(`wayland-protocols/stable/presentation-time/presentation-time.xml:27-51,73-119,124-139,200-266`).
KWin couples these to a `SurfaceItem` being painted: commit schedules a frame, paint sends
`frameRendered`, attaches presentation feedback, and clears the FIFO barrier
(`kwin/src/scene/surfaceitem_wayland.cpp:117-121,242-270`). `[V]`

Delegation offers two clocks:

- **Producer-output clock.** Keep driving callbacks from the monitor that owns the 2D window. This
  requires almost no producer change, but an occluded/off-output window may stop, 60 Hz desktop
  pacing aliases against a 90/120 Hz HMD, and the reported presentation is not what the XR user saw.
- **Consumer-forwarded clock — required for XR.** Zxr sends cadence updates derived from
  `xrWaitFrame`: predicted display time, period, and a commit cutoff translated to the producer's
  presentation clock. The producer schedules `wl_surface.frame` early enough for the client to
  render, and reports presentation only when zxr confirms that commit was first sampled into a
  submitted XR frame. A late client does not stall XR: zxr reuses the last ready buffer.

`commit-timing-v1` adds a *client-authored not-before timestamp* in the compositor presentation
clock; it does not tell a client when to start rendering
(`wayland-protocols/staging/commit-timing/commit-timing-v1.xml:27-49,90-113`).
`fifo-v1` prevents the next commit from replacing a latched one before one refresh, but may be
ignored for off-screen/occluded surfaces and explicitly requires another throttling mechanism
(`wayland-protocols/staging/fifo/fifo-v1.xml:26-38,89-129`). They help preserve ordered commits
once the delegated clock exists; neither creates that clock. `[V]`

The export protocol therefore needs consumer cadence and per-commit outcomes:

- **presented** = the commit was first sampled into an XR frame, with consumer timestamp/period
  translated into the producer's `wp_presentation` clock;
- **discarded** = superseded or revoked before zxr ever sampled it;
- **reused** is not another presentation event for the same Wayland commit; it is zxr's internal
  late-frame policy.

Despite the feature name, zxr sampling a client dmabuf in an XR composition pass must not set
`wp_presentation_feedback.zero_copy`: that flag means the client buffer reached display hardware as
is, and OpenGL/Vulkan compositing counts as a copy (`presentation-time.xml:155-196`). “Zero-copy”
here means no *intermediate pixel buffer*, not direct scanout. `[V]`

This is the precise inversion from capture: capture waits for producer damage and then fills a
consumer buffer; delegation advertises a consumer deadline, wakes production, and samples whichever
eligible original client buffer is ready at that deadline.

---
## 4. The surface-tree and popup problem
An xdg toplevel is not one texture. It owns synchronized/desynchronized subsurfaces with independent
buffers, positions, stacking, transforms, viewports, input/opaque regions, and commit application.
KWin already models that recursively: `SurfaceInterface` exposes ordered children and position,
applies synchronized child state on parent commit, and computes the tree bounds
(`kwin/src/wayland/surface.cpp:623-782,901-942`); `SurfaceItemWayland` creates one item per
subsurface (`kwin/src/scene/surfaceitem_wayland.cpp:25-72,124-151`). `[V]`

Popups add shell policy. `xdg_positioner` specifies size, an anchor rectangle in parent window
geometry, anchor/gravity/offset, and flip/slide/resize adjustments. What counts as constrained is
compositor policy, conventionally an output work area
(`wayland-protocols/stable/xdg-shell/xdg-shell.xml:124-145,169-178,239-347`). Reactive popups must
be reconstrained when those conditions change (`:370-405`). Popup grabs are authenticated by a
recent seat serial, nest in strict order, keep keyboard focus on the topmost popup, and deliver
owner events across the client's surfaces (`:1244-1333`). `[V]`

In XR the placement boundary belongs to zxr: angular/metric quad bounds, not the producer's monitor
work area. Three options follow:

1. **Forward the whole tree and let zxr run `xdg_positioner`.** Semantically pure, but zxr must
   duplicate shell policy and return configure/reposition sequences to the producer, which still
   owns the client's xdg objects and serial ordering.
2. **Forward the whole tree, but producer resolves positioners against consumer-supplied 2D
   bounds — recommended for v1.** Zxr supplies the parent quad's current plane-local usable
   rectangle; the producer runs its existing xdg-shell implementation and sends normal configures.
   This retains one shell authority while making the placement volume consumer-owned.
3. **Flatten popup pixels into the parent quad.** Reject. Hit testing, popup grabs, independent
   damage/lifetime, outside-click dismissal, and nested popup focus become unrecoverable.

KWin VR proves option 2 in one process. Its 29-line core seam replaces hardcoded `clientArea()` with
a `PopupBoundsResolver`; the VR callback returns the union of parent/transient geometry, adding the
2D work area only when the root is not in VR
(`kwin-vr/src/xdgshellwindow.cpp:1927-1944`;
`kwin-vr/src/workspace.h:158-160,792`;
`kwin-vr/src/plugins/vr/kwinvr.cpp:273-288`; fork commit `c877221`, catalogued in
[research 31 §3](31-kwin-vr.md)). Across processes, the callback's value becomes protocol state.

So the popup verdict is **complete tree forwarding plus a bounds callback**, not “bounds instead of
tree.” Zxr must receive every popup/subsurface as a separately textured and hittable node; the 2D
producer remains responsible for xdg positioner/configure/grab semantics.

---
## 5. Input back-path
### 5.1 Existing mechanisms

libei/EIS is an authorized virtual-seat transport. EIS creates seats/devices with pointer,
absolute-pointer, keyboard, touch, button, scroll, and newer gesture/stylus capabilities; sender
clients emulate events into the compositor
(`libei/src/libeis.h:37-89,159-211,214-236,377-426`). Absolute virtual devices expose rectangular
regions and optional mapping IDs; regions can represent only part of a desktop and use compositor-
private coordinates (`libei/src/libeis.h:159-186`). It is seat/device-level, not a Wayland
per-`wl_surface` event protocol. `[V]`

KWin's RemoteDesktop EIS endpoint advertises output-sized regions whose mapping IDs are output
names, then turns EIS events into ordinary `InputDevice` signals
(`kwin/src/plugins/eis/eisbackend.cpp:100-139,162-204`;
`kwin/src/plugins/eis/eiscontext.cpp:272-350`). KDE's portal backend obtains that fd from KWin and
returns it through `RemoteDesktop.ConnectToEIS`
([xdg-desktop-portal-kde!279](https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/merge_requests/279),
[commit 6db5e848](https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/commit/6db5e8488409c53b9c79ff856d870e79e519d7e1)).
KRdp normally uses the portal/Plasma screencast session; it is not evidence of a KWin-private
per-window EIS API ([KRdp portal commit](https://invent.kde.org/plasma/krdp/-/commit/8d76d9a634d7072fdc1036bb2f2da45b5f8dd917)). `[V/R]`

Mutter is closer than expected. Every screen-cast `MetaStream` gets a unique mapping ID, and a
window stream implements a **standalone EIS viewport** that transforms stream-local coordinates
back to stage coordinates (`mutter/src/backends/meta-stream.c:269-290`;
`mutter/src/backends/meta-stream-window.c:215-281`). RemoteDesktop adds each selected stream as an
EIS viewport (`mutter/src/backends/meta-remote-desktop-session.c:212-290,1278-1344`);
gnome-remote-desktop is an `ei_new_sender` and targets a stream by its mapping ID
(`gnome-remote-desktop/src/grd-session.c:716-755,1390-1416,1614-1681`). This is strong prior art for
the coordinate mapping, but Mutter ultimately injects a global Clutter virtual device and performs
normal compositor hit testing (`mutter/src/backends/meta-eis-client.c:467-504`). `[V]`

The KWin VR fork instead registers a synthetic `InputDevice`, emits absolute motion/buttons/keys,
and overrides hovered-window resolution from its 3D pick result
(`kwin-vr/src/plugins/vr/kwinvrinputdevice.cpp:19-33,96-150,267-279`;
`kwin-vr/src/plugins/vr/kwinvrhoveredwindowresolver.cpp`;
[research 31 §2.5](31-kwin-vr.md)). That forced-target seam is what generic EIS lacks. `[V]`

### 5.2 Serials, activation, grabs, and DnD

Zxr must not synthesize Wayland serials. It sends device transitions against an export/tree node;
the producer routes them through its real seat, generates serials, and delivers ordinary
`wl_pointer`, `wl_keyboard`, and `wl_touch` events. A press establishes the producer's implicit
grab; later motion/release stays with that grab even if zxr's ray moves to another quad. Popup grabs
remain producer state and outside hits become a dismissal decision there.

An `xdg_activation` token is also producer-domain state. Tokens may carry the producer seat's recent
serial and requesting surface, and the producer may issue an ineffective token for focus-stealing
prevention (`wayland-protocols/staging/xdg-activation/xdg-activation-v1.xml:27-67,85-111,
114-199`). A token minted by zxr's own compositor cannot activate a KWin/Mutter client. The export
channel should carry activation *intent*; the producer validates the delegated gesture and mints or
consumes its own token.

DnD is deferred, but not because pointer motion is hard. `wl_data_device` binds a drag to an
implicit-grab serial, transfers offer/source pipes, discovers targets across surface trees, and may
cross toplevels. KWin verifies the pointer/touch/tablet grab serial before starting a drag and then
changes the effective surface target during motion (`kwin/src/input.cpp:2579-2608,2671-2731,
2884-2912`). A per-window input stream carries none of the data-offer or cross-export target
authority. DnD needs a later data-device bridge; v1 must explicitly reject/defer it rather than
silently producing broken drags.

### 5.3 Verdict

**libei is useful transport machinery but not a sufficient contract.** Mutter proves that a
per-stream EIS mapping ID can solve coordinates. It still does not bind each event to an exported
surface-tree node, define focus/grab ownership, connect popup dismissal, or carry activation
intent. The protocol therefore needs a per-export input channel (or an explicitly paired EIS
device plus protocol target/grab channel). Implementations may feed those events into libei/
Clutter/KWin `InputDevice`; the wire semantics cannot be “just open a global EIS seat.”

---
## 6. Producer feasibility by compositor family
### 6.1 KWin — feasible, medium patch, best first mature producer

Concrete in-tree assets:

- `SurfaceInterface::buffer()` exposes the current `GraphicsBuffer`; it includes dmabuf plane fds,
  modifier/offset/pitch/device, refcounting, and release-point retention
  (`kwin/src/core/graphicsbuffer.h:20-40,62-139`;
  `kwin/src/wayland/surface.h:152-175`). This is the actual forwarding seam.
- The scene already mirrors a complete subsurface tree and tracks commit/damage/color/timing state
  (`kwin/src/scene/surfaceitem_wayland.cpp:25-72,117-151,220-270`).
- The fork's `loadGraphicsBufferToQSGTextures()` demonstrates direct import of all dmabuf planes
  through EGLImage to Qt textures; shm alone uploads
  (`kwin-vr/src/plugins/vr/kwingraphicshelpers.cpp:149-220,406-501`;
  `kwin-vr/src/plugins/vr/kwinwaylandsurface.cpp:90-165`). This is the “cheap baseline”: exporting
  the fd and metadata is cheaper than that already-working in-process import.
- `WindowThumbnail` is a fallback, not the desired path: it allocates an RGBA offscreen texture,
  re-renders the whole decorated `WindowItem`, and intentionally incurs one frame of latency
  (`kwin/src/scripting/windowthumbnailitem.cpp:112-157`). It proves selection/damage integration,
  not client-buffer forwarding.
- The internal-window/QPA path already presents `GraphicsBuffer`s plus native fences into KWin
  (`kwin/src/plugins/qpa/eglplatformcontext.cpp:90-171`; `kwin/src/internalwindow.cpp`). This helps
  include KWin-owned menus/overlays where policy permits; they are not ordinary client toplevels.
- EIS supplies pointer/keyboard/touch injection, while the fork supplies the missing targeted-hover
  and popup-bounds shapes (§4–§5).
- The fork's internal-window transient patch was also filed separately as
  [kwin!8500](https://invent.kde.org/plasma/kwin/-/merge_requests/8500), according to
  [research 31 §3](31-kwin-vr.md); it matters for exporting KWin-owned menus as related nodes, not
  for the base client-buffer path.

KWin does **not** currently advertise ext foreign-toplevel list or ext capture in the surveyed 6.7
tree ([support matrix](https://wayland.app/protocols/ext-foreign-toplevel-list-v1),
[capture matrix](https://wayland.app/protocols/ext-image-copy-capture-v1)). A producer must add a
trusted handle/global (preferably ext list plus the draft extension), not expose QObject internals.

**Patch estimate `[I]`: 5–8 kLOC including XML and integration tests, across roughly 12–20 files;
about 0.5–1.0 kLOC should touch existing KWin core.** The rest can live in one protocol/plugin
module: export objects and authorization; tree serialization; fd/sync/release relay; cadence and
feedback; input endpoint; tests. It needs the three clean seams from the fork (targeted hover/input,
popup bounds, off-output frame callbacks), but **none** of its invasive VR move/resize,
window↔output, output leasing, Qt Quick 3D, or OpenXR code. This is an order of magnitude smaller
and much less cross-cutting than the 16.8 kLOC VR plugin plus 20 core commits
([research 31 §3, §8](31-kwin-vr.md)).

The estimate is credible because buffer lifetime, tree modeling, and seat injection already exist;
it is not “a weekend protocol” because consumer-driven presentation and disconnect-safe release
need compositor tests. Maintainer willingness is explicit but conditional: David Edmundson said
KWin can forward buffers “pretty cheaply” and wants application-level input forwarding; Vlad
preferred window info/thumbnails/input with external overlay composition
([research 31 §1](31-kwin-vr.md), primary discussion
[kwin!8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671)).

### 6.2 Mutter/GNOME — technically possible, institutionally late

Mutter's window screencast is a capture path. Damage queues a record; the window actor paints to a
CPU bitmap or a consumer framebuffer, and PipeWire transports the result
(`mutter/src/backends/meta-stream-source-window.c:353-405,459-558,658-719`;
`mutter/src/compositor/meta-window-actor.c:1329-1583`). `MetaWindowActor` texture access is
in-process rendering state, not a cross-process client-dmabuf export ABI
(`mutter/src/meta/meta-window-actor.h`; `mutter/src/compositor/meta-window-actor.c`). `[V]`

The supported application-facing route is the ScreenCast portal: select a WINDOW source, receive a
PipeWire stream, and pair RemoteDesktop with EIS
([portal API](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)).
Mutter's `RecordWindow` D-Bus API is private and still creates a PipeWire stream
([API](https://github.com/jadahl/gnome-remote-desktop/blob/master/src/org.gnome.Mutter.ScreenCast.xml)).
Surveyed Mutter 51 has neither ext foreign-toplevel list nor ext image-copy capture
([list matrix](https://wayland.app/protocols/ext-foreign-toplevel-list-v1),
[capture matrix](https://wayland.app/protocols/ext-image-capture-source-v1)).

A GNOME Shell JS extension cannot implement this safely: Shell is Mutter's sole `MetaPlugin`, and
extensions become code inside that process; they cannot create a stable external dmabuf/lifetime
ABI ([GNOME Shell plugin](https://github.com/GNOME/gnome-shell/blob/master/src/gnome-shell-plugin.c),
[extension architecture](https://gjs.guide/extensions/overview/architecture.html)). The producer
requires Mutter C changes in `MetaWaylandSurface/Buffer`, syncobj release aggregation, shell/tree
model, frame clock, and EIS viewport targeting.

**Verdict `[I]`: do not plan on a private Mutter producer.** GNOME is likely to participate only
after an `ext_` protocol has multi-compositor evidence and an upstream review path. Until then use
portal window capture + PipeWire + EIS as a visibly lower-fidelity fallback: flattened, copied/
rendered, source-paced. A downstream Mutter patch would be comparable to KWin in code size but has
no supported plugin boundary and a much worse upstream/maintenance posture.

### 6.3 wlroots and Smithay — cheapest reference producers

wlroots 0.20 has reusable ext foreign-toplevel and capture-source APIs; its helper explicitly
copies a `wlr_buffer` into the client-provided frame
([foreign-list API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_ext_foreign_toplevel_list_v1.h.html),
[capture API](https://wlroots.pages.freedesktop.org/wlroots/wlr/types/wlr_ext_image_copy_capture_v1.h.html)).
Replacing the final copy handler with “duplicate source dmabuf + hold `wlr_buffer` until consumer
release” is localized, although tree/timing/input remain policy. `[R/I]`

Smithay/COSMIC is the closest checked implementation. Ext handles become toplevel sources
(`cosmic-comp/src/wayland/handlers/image_capture_source.rs:20-51`); its handler has Output,
Workspace, and Toplevel variants and chooses the source buffer's DRM node
(`cosmic-comp/src/wayland/handlers/image_copy_capture/mod.rs:79-160,290-428`). Toplevel capture
walks `CosmicSurface` render elements, renders into the consumer dmabuf, holds source buffers until
GPU sync completes, then succeeds
(`cosmic-comp/src/wayland/handlers/image_copy_capture/render.rs:61-116,540-800`).

Surprisingly, **COSMIC toplevel capture already uses the standard ext source**; private
`zcosmic_workspace_image_capture_source_manager_v1` adds only workspaces
(`cosmic-protocols/unstable/cosmic-image-capture-source-unstable-v1.xml:26-54`). Its source-object
plumbing, device selection, recursive elements, held refs, and async fence are close to this seam,
although the final operation is still a render copy.

A Smithay reference producer can accept normal clients, read `RendererSurfaceState::buffer()` and
tree state, duplicate planes, hold `Buffer` until zxr release, forward cadence, and route input.
Smithay signals syncobj release when the last `Buffer` ref drops
([source](https://smithay.github.io/smithay/src/smithay/backend/renderer/utils/wayland.rs.html),
[syncobj](https://smithay.github.io/smithay/smithay/wayland/drm_syncobj/struct.DrmSyncPoint.html)).
Estimate: 2–4 kLOC for a reference compositor/protocol client pair before production hardening.

### 6.4 Wolf-style per-app headless sessions — no new protocol

Wolf creates a Smithay micro-compositor per stream session/app and exposes its raw framebuffer to
GStreamer (`wolf/docs/modules/dev/pages/how-it-works.adoc:1-14`;
`wolf/src/moonlight-server/sessions/moonlight.cpp:88-180`). Only that compositor socket is mounted
into the container (`wolf/src/moonlight-server/sessions/common.cpp:40-45`). Present each private
compositor's output as one zxr quad; with one app, nesting approximates per-app export. Costs: one
compositor/pool per app; no shared WM/workspace/clipboard/DnD/focus or cross-app popups; synthetic
outputs; gamescope/Xwayland for X11. It suits isolated VM/container/remote apps, not an existing
shared KWin/Mutter/COSMIC session ([research 18 §5](18-xr-streaming.md)).

---
## 7. Governance and upstreaming path
`ext` is catch-all (`xdg` is own-window management, `wp` plumbing). Ext requires two member ACKs,
one open-source client and server, and in-depth non-author review; proposals discuss at least 30
days and non-members may request a sponsor (`wayland-protocols/GOVERNANCE.md:60-75,82-128,149-159`).
Experimental upstream interfaces use `xx_` and are renamed on promotion (`:77-80,103-109,179-184`).
KWin, GTK/Mutter, Smithay/COSMIC, and wlroots/Sway are members
(`wayland-protocols/MEMBERS.md:1-20`). `[V]`

COSMIC workspace is the precedent. In October 2022 its private protocol was a namespaced copy of
the evolving upstream proposal
([cosmic-protocols issue #8](https://github.com/pop-os/cosmic-protocols/issues/8)); upstream
[wayland-protocols!40](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/merge_requests/40)
became `ext-workspace-v1` on **2024-12-20**
([announcement](https://lists.freedesktop.org/archives/wayland-devel/2024-December/043920.html)).
COSMIC adopted it in [cosmic-comp#1213](https://github.com/pop-os/cosmic-comp/pull/1213) on
**2025-02-13**, retaining a small private extension: a multi-year migration. `[R]`

**Recommended posture.**

1. Keep `zspatial-toplevel-export-v1.xml` experimental, privileged, and local. `zspatial` is not upstream;
   prefer `zxr_` privately, `xx_` for upstream experimental, then `ext_`.
2. Build zxr plus Smithay and KWin producers: one client + two servers exceeds the ext minimum and
   tests Rust/Smithay against mature C++.
3. Publish interop tests for modifiers, out-of-order release, disconnect, popup reconstraint, late
   reuse, and grabs; then seek Smithay/COSMIC and KWin sponsorship with early wlroots/Mutter review.

---
## 8. Requirements distillate for the XML draft
The following requirements are the direct input to `protocols/zspatial-toplevel-export-v1.xml`.

1. **R1 — Stable selection.** Bind an `ext_foreign_toplevel_handle_v1` or equally unique mapped-lifetime handle, never title/app-id matching (§1).
2. **R2 — Privileged visibility.** Connection-filter the global; enumeration grants neither export nor input authority (§1, §5).
3. **R3 — Complete tree.** Export root, ordered subsurfaces, popups/transients, and lifecycle as separate nodes; no mandatory flattening (§4).
4. **R4 — Atomic commits.** Identify commits and carry attach/null, damage, scale, transform, viewport, offset, color/representation, alpha, input/opaque regions, and synchronized-tree application (§2, §4).
5. **R5 — Original dmabuf.** Transfer duplicated plane fds plus fourcc, modifier, offsets, pitches, dimensions, and device identity for the client's allocation (§2).
6. **R6 — Explicit fallback.** Typed fallback/unsupported events cover shm, modifier/device mismatch, protected content, and cross-GPU; copy capture is a separate profile (§1, §2).
7. **R7 — Acquire synchronization.** Every buffer use carries an acquire fence or syncobj point zxr waits on (§2).
8. **R8 — Producer-owned release join.** Zxr returns GPU-complete release; only the producer joins local+consumer completion and releases the client (§2).
9. **R9 — Per-buffer release ordering.** Permit out-of-order completion; never share one unsafe monotonic release timeline across reusable buffers (§2).
10. **R10 — Bounded flow control.** Negotiate in-flight buffer/byte caps; revoke stalls without blocking the Wayland loop (§2).
11. **R11 — Consumer cadence.** Carry clock conversion, predicted display time, period, and `xrWaitFrame`-derived cutoff (§3).
12. **R12 — Frame callback ownership.** Exactly one explicit pacing mode drives the tree and switches atomically (§3).
13. **R13 — Presentation outcomes.** Identify first XR sampling for `presented`; report `discarded` if never sampled (§3).
14. **R14 — Late-frame reuse.** Reuse last-ready without delaying XR or re-presenting the old Wayland commit (§3).
15. **R15 — Consumer bounds, producer placement.** Zxr sends plane-local bounds/change serials; producer runs xdg-positioner and configure (§4).
16. **R16 — Surface-local input.** Events name a tree node and carry local coordinates, device class, controls/touch IDs, timestamp, and frame (§5).
17. **R17 — Producer serials and grabs.** Producer generates serials and owns implicit pointer/touch, popup, focus, and cancellation state (§5).
18. **R18 — Activation intent.** Carry gesture-derived intent, never a foreign xdg-activation token; producer mints/validates its token (§5).
19. **R19 — DnD capability.** Advertise unsupported unless data offers and cross-export targeting are implemented (§5).
20. **R20 — Revocation/disconnect.** Unmap, lock, auth loss, death, or GPU reset revokes export, cancels input, and safely releases buffers (§2, §5).
21. **R21 — Privacy/policy.** Producer may redact/deny lock, protected, private, cursor, decoration, or internal surfaces (§1, §6).
22. **R22 — Two zero-copy meanings.** Never promise/set `wp_presentation.zero_copy` merely because no intermediate allocation exists (§3).

*Added by the transition-mechanics review ([31 §2.9](31-kwin-vr.md);
[foreign-session-integration.md §3.7](../architecture/foreign-session-integration.md)):*

23. **R23 — Detach handoff.** The consumer may request delegation of the toplevel *currently in
    interactive move* on the producer (identified via the producer's active move-grab, not a
    handle guess); the producer ends its move without placement side-effects (no
    electric-border/tiling/output snap) and the handoff carries the **cursor-anchor point**
    (surface-local position under the cursor at handoff) so the consumer's spatial grab keeps the
    same content pixel under the ray. Precedent: `xdg-toplevel-drag-v1`'s attach-toplevel-to-drag
    semantics, single-compositor.
24. **R24 — Adopt with placement.** The consumer may end a delegation with a landing hint —
    target output, surface-local/global 2D coordinates (from the consumer's pick UV on the
    session quad), and a resume-move flag; the producer warps its pointer accordingly and, if
    resuming, continues its interactive move ("final position as if `xdg_toplevel.move` ended",
    per xdg-toplevel-drag). Only *delegated* toplevels can be adopted: a consumer-native client
    can never enter the producer's session (its connection belongs to the consumer) — a
    product-semantics asymmetry single-compositor implementations (KWin VR) do not have.

**Bottom line.** KWin/Smithay buffer refs, syncobj, foreign-toplevel identity, KWin VR's popup/input
seams, and Mutter's per-window EIS mapping exist. Missing is one contract joining them under
consumer-driven presentation and producer-retained shell authority.
