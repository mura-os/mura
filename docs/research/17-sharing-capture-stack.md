# 17: The Linux capture / consent / input-injection stack, and what zxr-shell-v2 must implement

**Status:** research for `docs/architecture/spatial-sharing.md`. **Date:** 2026-09-22.
**Scope:** modes 1–2 of the five-mode sharing taxonomy — (1) *spectate* (composed eye buffer as
video) and (2) *share a 2D window* (its view-independent window-local texture, per
[zxr-shell-v2-composition §7.3](../architecture/zxr-shell-v2-composition.md)). These two modes are
simultaneously the **general Linux ecosystem-compatibility tier**: OBS, video calls, remote
desktop, `grim`-style tools. Mode 3 (per-observer colour+depth) appears here only where the
PipeWire ground truth constrains it; mode 4 (Wayland protocol proxying) is sibling doc 19; mode 5
(workspace join) is placement-graph replication, not pixel capture.

All paths are relative to `references/` unless noted. Line numbers are from the local clones.

## 1. The portal model (xdg-desktop-portal + xdg-desktop-portal-wlr)

### 1.1 Frontend/backend split

Apps never talk to the compositor for capture. They call
`org.freedesktop.portal.ScreenCast` on the `xdg-desktop-portal` frontend, which proxies to a
per-desktop **backend** implementing `org.freedesktop.impl.portal.ScreenCast`
(`xdg-desktop-portal/data/org.freedesktop.impl.portal.ScreenCast.xml`). Backend discovery is
config, not code: a `.portal` file names the backend's bus name, interfaces, and desktops
(`xdg-desktop-portal-wlr/wlr.portal`: `DBusName=org.freedesktop.impl.portal.desktop.wlr`,
`Interfaces=…ScreenCast;…Screenshot`, `UseIn=wlroots;sway;…`), plus `portals.conf` per-desktop
override (`xdg-desktop-portal/doc/portals.conf.rst.in`). **spatial-os becomes a first-class
capture citizen by shipping one D-Bus service + one `.portal` file** — no upstream changes.

### 1.2 Session lifecycle

From `xdg-desktop-portal/data/org.freedesktop.portal.ScreenCast.xml:16-60` (interface v6):

1. `CreateSession()` → a `Session` object (closable by either side).
2. `SelectSources(session, {types, multiple, cursor_mode, restore_token, persist_mode})` — once
   per session.
3. `Start(session, parent_window)` — this is where the backend shows the consent dialog — returns
   `streams a(ua{sv})` and optionally a new `restore_token`.
4. `OpenPipeWireRemote(session)` → an fd to be used with `pw_context_connect_fd`; only the
   session's stream nodes are visible on that connection (lines 303-322).

Source types are a bitmask property `AvailableSourceTypes`: `MONITOR=1, WINDOW=2, VIRTUAL=4`
(lines 324-332) — **forward-extensible, which is the hook for SpatialCast (§4)**. Cursor modes
likewise: `HIDDEN=1, EMBEDDED=2, METADATA=4` (metadata = cursor as PipeWire `spa_meta_cursor`,
not burned into frames; lines 333-347).

Each stream tuple carries a PipeWire node ID (deprecated for targeting since v6 — node IDs are
reused after destruction) plus properties: `id`, `position`, `size`, `source_type`, `mapping_id`
(links a stream to a libei input region, lines 265-276), and `pipewire-serial` (monotonic 64-bit
`object.serial`, preferred with `PW_KEY_TARGET_OBJECT`; lines 278-286).

### 1.3 Persistence: restore tokens and the permission store

The **token is frontend-owned; the payload is backend-owned.** An app passes `persist_mode`
(0 = none, 1 = while app runs, 2 = until revoked) and gets back a single-use `restore_token`.
The frontend translates token ↔ backend `restore_data (suv)` = (vendor, version, private variant)
(`data/org.freedesktop.impl.portal.ScreenCast.xml:71-84, 202-222`), storing it in the permission
store table `SCREEN_CAST_PERMISSION_TABLE` via `Set`/`Lookup`/`Delete` on
`org.freedesktop.impl.portal.PermissionStore`
(`xdg-desktop-portal/desktop-portal/screen-cast.c:120-126, 475-505, 774-779`;
`desktop-portal/xdp-session-persistence.c:116-159`). Backends must tolerate foreign/invalid
`restore_data` (user switched desktops). xdpw's entire restore payload is
`("wlroots", 1, {output_name})` (`xdg-desktop-portal-wlr/src/screencast/screencast.c:400-471,
706-715`) — restore data can be minimal.

`org.freedesktop.portal.RemoteDesktop` (v2, `data/org.freedesktop.portal.RemoteDesktop.xml`)
mirrors the lifecycle with `SelectDevices` (`KEYBOARD=1, POINTER=2, TOUCHSCREEN=4`) and composes
with ScreenCast: the same session object is passed to `ScreenCast.SelectSources` +
`OpenPipeWireRemote`, and to `Clipboard.RequestClipboard`. Input goes either through legacy D-Bus
`Notify*` methods (relative/absolute pointer, keycode/keysym, touch — absolute coordinates are in
a named stream's logical space) or, preferred, `ConnectToEIS` → an fd for a libei sender context;
after EIS is connected the `Notify*` methods error (lines 40-53, 470-503). Combined
remote-desktop+screencast persistence is handled *only* by the RemoteDesktop portal (lines 56-64).

### 1.4 What a backend actually implements (xdpw as the concrete floor)

`xdg-desktop-portal-wlr` is ~4.6k lines total. The ScreenCast impl is one sd-bus vtable with
three methods (`src/screencast/screencast.c:731-749`):

- **CreateSession** — bookkeeping only (`:246-328`).
- **SelectSources** — parses `types`/`cursor_mode`/`persist_mode`/`restore_data`; picks a target
  either from restore data or interactively (`setup_target`, `:132-231`). "Interactive" is
  literally exec'ing a chooser: `slurp` (click an output), or dmenu-style `wofi`/`rofi` lists of
  outputs and foreign toplevels (`src/screencast/chooser.c:224-227`). The consent UI is
  backend-owned and completely replaceable — for spatial-os it becomes an in-space picker.
- **Start** — initializes the Wayland capture session, creates the PipeWire stream, spins the PW
  loop until a node id exists, and replies with node id + `source_type` + `mapping_id`
  (output name) + `pipewire-serial` + `restore_data` (`:535-729`).

The PipeWire producer (`src/screencast/pipewire_screencast.c`) is the canonical wiring:

- Stream = `pw_stream_new(core, …, {PW_KEY_MEDIA_CLASS: "Video/Source"})`, connected
  `PW_DIRECTION_OUTPUT` with `PW_STREAM_FLAG_ALLOC_BUFFERS | PW_STREAM_FLAG_DRIVER`:
  ALLOC_BUFFERS means the producer supplies the memory — the backend fills each `spa_data` with
  its own dmabuf/memfd fds in the `add_buffer` hook — and DRIVER means the capture side clocks
  the graph (`:690-730`).
- Format offer: for every DRM fourcc from the capture protocol's constraints, one dmabuf
  `EnumFormat` with a modifier choice flagged `MANDATORY | DONT_FIXATE`, plus shm fallbacks
  (`build_formats`, `:158-194`). On `param_changed`, if a non-fixated modifier list comes back it
  **test-allocates a GBM bo with the intersected modifiers** and re-announces the fixated one
  (`:433-500`) — the documented PipeWire fixation dance (§6.3). Fallback ladder: modifier-aware →
  linear/implicit → `avoid_dmabufs = true` → shm.
- Buffers: `SPA_PARAM_Buffers` with `blocks` = plane count from
  `gbm_device_get_format_modifier_plane_count` (`:502-521`); in `add_buffer` the backend fills
  per-plane `fd/stride/offset/size` from its own buffers (`:546-600`).
- Metadata negotiated: `SPA_META_Header` (pts from capture timestamp, `seq`, CORRUPTED flag when
  a capture failed), `SPA_META_VideoTransform` (output rotation passthrough — capture does *not*
  rotate pixels), `SPA_META_VideoDamage` (up to 16 `spa_meta_region`; overflow regions get merged
  into the last slot, `:524-539, 246-303`).

**Minimum viable portal backend for a compositor** = D-Bus service + `.portal` file + a source
chooser + a Wayland-side capture path + this PipeWire producer pattern; RemoteDesktop
additionally requires an EIS server implementation (§5).

## 2. gnome-remote-desktop: the reference decomposition

g-r-d is **not** a portal client or backend; it talks to Mutter's *private* D-Bus APIs
`org.gnome.Mutter.ScreenCast` / `org.gnome.Mutter.RemoteDesktop`
(`gnome-remote-desktop/src/grd-session.c:401-455`, XML at `src/org.gnome.Mutter.ScreenCast.xml`),
the same APIs GNOME's portal backend uses. The compositor exposes `RecordMonitor`,
`RecordWindow`, `RecordArea`, `RecordVirtual` (with `cursor-mode` and `is-platform` options,
XML `:93-182`); consent/session policy lives above. Architecture lesson #1: **the compositor's
capture surface is a small record-source API; consent UI, portal glue, and transports all stack
on top of it without touching compositor code.**

The three planes are fully separated:

- **Pixels: PipeWire consumer.** `src/grd-rdp-pipewire-stream.c:538-584` negotiates
  `SPA_DATA_DmaBuf`, `SPA_META_Header`, `SPA_META_Cursor` (metadata cursor mode — cursor arrives
  as `spa_meta_cursor` + embedded `spa_meta_bitmap`, `:680-715`), and notably
  `SPA_META_SyncTimeline` — **explicit-sync dmabuf consumption over PipeWire is deployed
  practice**, not theory.
- **Input: libei sender.** `ConnectToEIS` on the Mutter session → `ei_setup_backend_fd`
  (`src/grd-session.c:1634-1677`). The event loop binds seat capabilities, tracks per-capability
  devices (keyboard, relative pointer, absolute pointer, touch; `:1430-1520`), and forwards RDP/VNC
  input as `ei_device_*` calls bracketed by `ei_device_frame` (`:471-472, 650-700`). Absolute
  devices carry **regions**; g-r-d matches each stream to a region via the shared `mapping_id`
  (`:1395-1413`) — this is how multi-monitor absolute input finds the right screen.
- **Transport: RDP (FreeRDP) and VNC (libvncserver `rfbScreen`,** `src/grd-session-vnc.c:52`**)**,
  each consuming the same session/stream objects.

Performance machinery worth copying: a pluggable damage detector (`GrdRdpDamageDetector`) with a
64×64-tile memcmp implementation (`src/grd-rdp-damage-detector-memcmp.c:27-28`) and a CUDA one
(`grd-cuda-damage-utils.cu`) — i.e. g-r-d *re-derives* damage rather than trusting upstream
metadata; hardware H.264 encode sessions via VA-API (`src/grd-encode-session-vaapi.c:150-151`),
NVENC (`grd-hwaccel-nvidia.c`), Vulkan (`grd-hwaccel-vulkan.c`), software fallback
(`grd-encode-session-ca-sw.c`); RDP graphics pipeline with AVC420/444 (`grd-rdp-dvc-graphics-pipeline.c`).
Architecture lesson #2: **capture, input injection, and transport meet only at a session object;
each is independently replaceable.** spatial-os should keep the same seams so wayvnc, g-r-d-like
daemons, or WebRTC stacks can all sit on the same compositor surface.

## 3. wayvnc/neatvnc: the minimal baseline and the capture-protocol reality

wayvnc abstracts capture behind a tiny vtable (`wayvnc/include/screencopy-interface.h:34-53`,
caps: CURSOR, TRANSFORM) with two Wayland implementations selected at runtime
(`src/screencopy-interface.c:25-38`):

- **wlr-screencopy-unstable-v1** (`src/screencopy.c`): per-frame request/response — ask the
  compositor to copy an output into a client buffer, events `buffer`/`linux_dmabuf`/
  `buffer_done`/`ready`/`damage`. wlroots-specific, no window sources, no cursor stream, damage
  only via `copy_with_damage`.
- **ext-image-copy-capture-v1** (`src/ext-image-copy-capture.c:58-93`), the ratified
  wayland-protocols staging successor
  (`wayland-protocols/staging/ext-image-copy-capture/ext-image-copy-capture-v1.xml`) paired with
  `ext-image-capture-source-v1`: a **session** object per source with buffer-constraint events
  (shm formats, dmabuf device + format/modifier table — wayvnc scores and picks,
  `format_array`), then per-frame `create_frame` → `attach_buffer` + `damage_buffer` → `capture`,
  with `transform`, `damage`, and `presentation_time` events before `ready` (XML `:202-312`).
  Sources come from separate managers: output sources
  (`ext_output_image_capture_source_manager_v1`) and **per-window** sources
  (`ext_foreign_toplevel_image_capture_source_manager_v1`). Cursor capture is a first-class
  session type (`ext_image_copy_capture_cursor_session_v1`, XML `:372+`).

xdpw's preference order confirms the direction: use ext-image-copy-capture when the compositor
has it, else fall back to wlr-screencopy (`xdg-desktop-portal-wlr/src/screencast/wlr_screencast.c:29-36`).

Damage flows end-to-end: frame damage accumulates per pooled buffer
(`wayvnc/src/ext-image-copy-capture.c:226-247, 587-594`; pool damage-all in
`include/buffer.h:100-125`), and neatvnc *refines* it by XXH3-hashing 32-px tiles before encoding
(`neatvnc/src/damage-refinery.c:28-46`). Encoding pipeline: raw/tight/zrle software encoders plus
Open H.264 RFB extension backed by ffmpeg (VA-API) or V4L2 M2M stateful encoders
(`neatvnc/src/enc/h264/{open-h264,ffmpeg-impl,v4l2m2m-impl}.c`).

**Decision input:** ext-image-copy-capture-v1 + ext-image-capture-source-v1 is what a new
compositor should implement — it alone provides sessions, window sources, cursor sessions, damage,
and presentation timestamps; wlr-screencopy is legacy surface area for `grim`-era tools.

## 4. obs-vkcapture: the capture-adapter pattern for uncooperative apps

For apps that render but cooperate with nothing, obs-vkcapture shows the full adapter:

- **Injection.** A Vulkan *implicit layer* `VK_LAYER_OBS_vkcapture`, activated purely by
  environment (`enable_environment: OBS_VKCAPTURE=1`, `obs-vkcapture/src/obs_vkcapture.json.in:13-18`)
  — no app modification, the loader inserts it. GL apps instead get an `LD_PRELOAD` shim hooking
  `eglSwapBuffers`/`glXSwapBuffers*` (`src/glinject.c:1044-1133`).
- **Export path.** The layer hooks `vkQueuePresentKHR` (`src/vklayer.c:1249-1261`). On capture
  start it creates one extra "export image" with
  `VkExternalMemoryImageCreateInfo{DMA_BUF}` + `VkImageDrmFormatModifierListCreateInfoEXT`
  (modifier list intersected with what the consumer tolerates, `:616-725`), allocates with
  `VkExportMemoryAllocateInfo`, and extracts fds with `vkGetMemoryFdKHR` + per-plane
  `vkGetImageSubresourceLayout` (`:752-826, 863`). Every present then blits/copies the backbuffer
  into that image (`CmdBlitImage`/`CmdCopyImage`, `:1123, 1147`). Single re-used export image:
  tearing is accepted by design.
- **Handoff.** An abstract Unix socket `/com/obsproject/vkcapture` (`src/capture.c:79-115`).
  Client → OBS: process name, then a `capture_texture_data` struct (size, DRM fourcc, per-plane
  stride/offset, modifier, flip, colour space) with up to 4 dmabuf fds via `SCM_RIGHTS`
  (`:168-211`). OBS → client: a control struct (capturing on/off, `no_modifiers`, `linear`,
  `map_host`, target `device_uuid`) that can force re-allocation mid-session (`:123-166`).

Implication for XR: the same pattern applies to an *uncooperative OpenXR app* by layering the
OpenXR loader (hook `xrEndFrame`, copy the submitted swapchain image, export dmabuf) — an
"OpenXR vkcapture". zxr-shell-v2 does not need it for its own clients (the compositor already
holds every client's colour+depth buffers), and composition doc §4 already scopes GPU
interception out of the clean contract. Keep the pattern on the shelf for capturing apps that run
directly on Monado beside our shell.

## 5. libei: the input-injection model

Three parts (`libei/README.md`): **ei** (client), **eis** (server — lives in the compositor),
**oeffis** (helper that does the RemoteDesktop-portal D-Bus dance). Custom binary protocol over a
Unix socket; contexts are *sender* (emulate input) or *receiver* (input capture/forwarding).

Object model (`libei/proto/protocol.xml`): handshake → connection → **seats** advertising
capability sets; the client binds the capabilities it wants (`ei_seat`, `:424`); the server then
adds **devices**, each a bundle of capability interfaces — `ei_pointer` (relative),
`ei_pointer_absolute`, `ei_scroll`, `ei_button`, `ei_keyboard` (with keymap), `ei_touchscreen`,
gestures, `ei_text`, `ei_stylus` (`:958-2121`). Absolute devices carry **regions** — rectangles
in a compositor-private logical space with a scale factor; events are only valid inside regions
(`:773-814`). Since v2 a region can carry a `mapping_id` (`:935-943`) that matches the portal
stream property of the same name — the stream↔input-surface join. Emulation is bracketed by
`start_emulating`/`stop_emulating` with sequence numbers (`:610-633`), and event batches are
timestamped `frame`s.

The design gives the compositor **separation** (emulated input is a distinct channel — can be
badged in UI), **distinction** (per-client device sets), and **control** (pause/discard at will —
e.g. while locked) (`README.md:56-72`). Portal gating: consent happens at
`RemoteDesktop.Start`; `ConnectToEIS` then hands the app a socket to *our* EIS implementation,
where we decide which devices/regions exist.

**Remote-controller semantics for a shared 2D window:** the compositor's EIS server exposes, per
share, one absolute pointer/keyboard device whose single region is the shared window's
window-local texture space, `mapping_id` equal to the stream's. Remote (x, y) then feeds the same
"window-local `wl_pointer` events" path that local ray→plane intersection uses
(composition doc §7.3) — remote input and XR-ray input converge on one code path, and the share
can be paused or masked (e.g. while a password prompt is focused) by pausing the EIS device.

## 6. PipeWire ground truth (verifying the prior research)

### 6.1 Buffer model

`spa_buffer` = `n_datas` × `spa_data` (memory blocks) + `n_metas` × `spa_meta`
(`pipewire/spa/include/spa/buffer/buffer.h:94-100`). Block types: `MemPtr`, `MemFd`, `DmaBuf`,
`MemId`, **`SyncObj`** (drm syncobj fds as extra blocks; `:34-49`). Each block has fd, maxsize,
and a per-cycle `spa_chunk{offset,size,stride,flags}` with `CORRUPTED`/`EMPTY` flags (`:52-92`).
A single buffer can therefore carry *many* fds: N dmabuf planes + 2 syncobj fds is already
standard (§6.3).

### 6.2 Metadata

`spa/include/spa/buffer/meta.h:28-47`: `Header` (flags, `pts` ns, `dts_offset`, 64-bit `seq`),
`VideoCrop`, `VideoDamage` (region array), `Bitmap`, `Cursor` (position+hotspot+inline bitmap),
`Control`, `Busy`, `VideoTransform`, `SyncTimeline` (`acquire_point`/`release_point` on the
syncobj timelines, `:182-205`). Crucially there is an explicit **custom range**
(`SPA_META_START_custom = 0x200`) — a SpatialCast per-frame pose/projection metadata is an
in-model extension, not a hack.

### 6.3 Formats — the honest depth answer

`spa/include/spa/param/video/raw.h`: `GRAY8`, **`GRAY16_BE/LE` exist** (`:63-65`). Float formats:
**only `RGBA_F16` and `RGBA_F32`** (`:117-118`; `DSP_F32` is an alias of `RGBA_F32`, `:130`).
**There is no single-channel float format** — no `R32F`/`GRAY_F32`/depth format. So a depth
stream over PipeWire must either (a) quantize to GRAY16, (b) carry `D32_SFLOAT` bits inside a
fourcc the format table *can* express (e.g. treat as `GRAY16`-doubled or pack into `RGBA_F32` at
¼ width — both smells), or (c) ship depth as a dmabuf whose *real* format is described in custom
metadata while the SPA format lies minimally. Any prior claim that PipeWire "natively streams
`R32_SFLOAT` depth" is **not supported by the format enum**. For mode-3 sharing the honest plan
is (c) or a proposed upstream format addition.

### 6.4 Dmabuf + explicit sync + multi-GPU

`doc/dox/internals/dma-buf.dox` is definitive and matches xdpw §1.4 exactly: per format, offer a
dmabuf `EnumFormat` (modifier choice `MANDATORY | DONT_FIXATE`, include `DRM_FORMAT_MOD_INVALID`
for implicit) plus an shm fallback; the **producer** fixates by test allocation and re-announces;
`SPA_PARAM_BUFFERS_blocks` = plane count; consumers must ignore `maxsize`/`size` for dmabuf.
Explicit sync: negotiate `SPA_META_SyncTimeline` via a mandatory `SPA_PARAM_BUFFERS_metaType`,
with 2 extra `SyncObj` blocks (acquire/release timelines) — the doc walks the full
producer/consumer `drmSyncobjTimelineWait/Signal` flow. Multi-GPU: device-ID negotiation via
`SPA_PARAM_Capability` + mandatory `SPA_FORMAT_VIDEO_deviceId` (a `dev_t`). This aligns
one-to-one with the zxr-shell-v2 client-transport machinery (composition doc §5) — the capture
stack and the client contract use the same dmabuf+syncobj vocabulary.

### 6.5 Multi-stream frame atomicity — the honest answer

What exists: `node.group` / `node.sync-group` force nodes to be **scheduled with the same
driver** (same graph cycle) (`src/pipewire/keys.h:154-162`), and a producer can *be* the driver
(`PW_STREAM_FLAG_DRIVER`, as xdpw uses). All buffers can carry the same clock's `pts` and a
producer-controlled `seq`.

What does **not** exist: any cross-stream transaction. Buffers travel per-stream through
independent queues; a consumer dequeues each stream separately, and one stream can drop or starve
(out-of-buffers, `pw-stream` retry timers as in xdpw `:623-654`) while its sibling delivers.
**Associated streams are correlatable (same clock, matching `pts`/`seq`) but not frame-atomic;
per-stream best-effort is the reality.** A consumer needing colour+depth per view must implement
a small re-association buffer keyed on `seq` and define a drop policy.

The PipeWire-native *atomic* alternative: put all planes of one logical frame into **one buffer**
as multiple blocks (colour plane(s) + depth plane(s) + syncobjs), since `n_datas` is free-form
and plane meaning is producer-defined. That is atomic by construction, at the cost of a custom
format/metadata convention (§6.3(c)) — one more reason SpatialCast mode-3 streams should prefer
one-buffer-many-blocks over many-streams.

Also relevant: portal-managed PipeWire clients are permission-scoped inside the daemon
(`doc/dox/internals/portal.dox`) — the sandboxed app only ever sees the granted nodes.

## 7. WayVR's wlx-capture (the Rust proof)

`wayvr/wlx-capture` is a working Rust consumer of exactly this stack: a small trait
(`init(dmabuf_formats, callback)`, `request_new_frame`, pause/resume —
`wlx-capture/src/lib.rs:24-37`) over four backends: the **portal+PipeWire** path (hand-rolled
zbus `ScreenCast` proxy incl. `restore_token` handling, `src/pipewire/dbus_screencast.rs`,
`src/pipewire/mod.rs:40-45, 515`; stream consumption with `MANDATORY | DONT_FIXATE` modifier
props, `src/pipewire/capture.rs:576`, accepting `DmaBuf`/`MemFd`/`MemPtr`), **wlr-screencopy**
via smithay-client-toolkit (`src/wlr_screencopy.rs:19`), wlr-export-dmabuf, and XSHM. Frames are
a clean enum: `Dmabuf{fourcc, modifier, planes}` / `MemFd` / `MemPtr` + mouse metadata
(`src/frame.rs:15-21, 67-135`). Takeaway for our smithay-leaning compositor: the *consumer* side
in Rust is proven (pipewire-rs + zbus + sctk); the *producer* side (our compositor publishing
`Video/Source` with `ALLOC_BUFFERS`) has xdpw as its C reference and pipewire-rs bindings cover
the needed API surface.

## 8. The capture-protocol decision for zxr-shell-v2

Day-one ecosystem compatibility, in order of leverage:

1. **Implement `ext-image-copy-capture-v1` + `ext-image-capture-source-v1`** (output sources and
   foreign-toplevel sources, hence also `ext-foreign-toplevel-list-v1`) plus
   `zwp_linux_dmabuf_v1` and `xdg-output`. This is the ratified, session-based protocol with
   window sources, cursor sessions, damage, and presentation time (§3) — and it is *exactly* what
   both xdpw and wayvnc prefer.
   - **Mode 2 (share a 2D window)** maps to a foreign-toplevel source whose content is the
     window-local texture from composition §7.3 — view-independent by construction, no XR
     machinery visible to the consumer.
   - **Mode 1 (spectate)** maps to an output source: expose the composed eye view (or the desktop
     mirror window) as an output/virtual output whose content is the post-composition buffer.
2. **Reuse `xdg-desktop-portal-wlr` unmodified as the initial portal backend** — it needs only
   the protocols above (its `wlr.portal` `UseIn` list just needs our desktop name). This buys
   OBS, Chromium/Firefox WebRTC, and portal-based tools with zero portal code written by us.
   wayvnc runs directly against the ext protocols for VNC.
3. **Native `xdg-desktop-portal-spatial` backend as the replacement step**, following the xdpw
   shape (three D-Bus methods + chooser + PW producer), when we need an in-space consent picker
   and SpatialCast source types (§9). Publication pattern: `Video/Source`,
   `ALLOC_BUFFERS | DRIVER`, dmabuf-with-modifiers + shm fallback, `Header` + `VideoDamage` +
   `VideoTransform` (+ `Cursor` for metadata cursor mode, + `SyncTimeline` — g-r-d already
   consumes it).
4. **RemoteDesktop backend = an EIS server (libeis) in the compositor**, one absolute device +
   region per shared surface, `mapping_id` joining stream to region, remote events feeding the
   same window-local input path as XR-ray intersection (§5).
5. **Skip `wlr-screencopy-v1` initially**; add it only if a concrete legacy tool matters
   (`grim` and friends are migrating; xdpw falls back for us in the interim — but note xdpw's
   fallback requires the compositor to have *one* of the two, which we satisfy with ext).

Damage: generate real damage regions from our scene graph for 2D-window sources (we know exactly
what changed); for spectate sources damage is near-total every frame (head motion), so consumers
should negotiate framerate instead (`maxFramerate` as in xdpw `:75-82`).

## 9. The SpatialCast sketch: spatial source types on the portal model

The portal model extends without forking: `AvailableSourceTypes` is a bitmask, `SelectSources`
takes a `types` mask, each stream is typed by a `source_type` property, and restore data is
vendor-scoped. A "SpatialCast" is then:

- **New source types** (advertised only by our backend):
  - `XR_VIEW = 8` — the composed stereo (or chosen-eye) buffer; mode 1. Stream properties gain
    `view_config` (mono/stereo, per-view size). This is an ordinary video stream; existing
    consumers that ignore the type bit can still render it if they select it via the chooser.
  - `APP_VOLUME = 16` — one 3D client's contribution: colour+depth for an *observer-specific*
    view (mode 3's transport). Published as **one stream whose buffers carry colour and depth as
    separate blocks** (atomicity, §6.5), with a custom SPA metadata (`SPA_META_START_custom`
    range) carrying per-frame `P·V` per view, depth encoding (near/far, reversed-Z — mirroring
    `XrCompositionLayerDepthInfoKHR` per composition doc §2), and bounds. Depth format honesty
    per §6.3: GRAY16 quantized now, custom-described `D32` dmabuf behind metadata later.
  - `WORKSPACE = 32` — *not a pixel stream*: selection returns a handle/token for
    placement-graph replication (mode 5, sibling docs); it lives in the same consent dialog but
    hands off to the session layer. Reserving the bit keeps one consent surface for all five
    modes.
- **Restore data**: `("spatial-os", 1, {source_type, window_uuid | view_id | workspace_id})` —
  same single-use-token machinery, zero frontend changes (§1.3).
- **Cursor-mode analogue**: for `XR_VIEW`, a `presence_mode` option (embed controllers/hands and
  focus highlights vs. metadata-only vs. hidden), mirroring how cursor modes gate what leaks
  into the stream.
- **Consent language** (the part that must *not* be inherited silently): a monitor/window dialog
  says "share what's on this screen/window". A 3D volume share must state
  **observer-controlled viewpoint**: "viewers can look at this object from any angle you have not
  hidden" — because `APP_VOLUME` re-renders per observer view, the sharer's occlusion intuitions
  from 2D don't apply. A workspace join must state placement visibility ("others see where your
  windows are arranged"). `XR_VIEW` spectate needs a gaze warning ("viewers see everything you
  look at, including notifications"), the XR analogue of full-monitor share — and the same
  "badge while active" duty compositors have for screen share, rendered in-space.
- **Input for spatial shares**: `RemoteDesktop.ConnectToEIS` unchanged for 2D windows; for
  `APP_VOLUME`, an EIS *region* is meaningless (input is a 6DoF ray/pose, not a rectangle) — a
  future `ei_` capability or a zxr-protocol-side channel; flagged as an open question.

Everything above is additive on interface v6 semantics; a consumer that never sets the new bits
sees today's portal exactly.

## 10. What spatial-os adopts / rejects / defers

**Adopt:**
- The portal frontend/backend split; ship a `.portal` + backend rather than bespoke IPC (§1.1).
- `ext-image-copy-capture-v1` family as the compositor capture surface (§8.1).
- xdpw as day-one backend, then a native backend in its image (§8.2-8.3).
- PipeWire publication with dmabuf-modifier fixation, shm fallback, Header/Damage/Transform/
  Cursor/SyncTimeline metadata (§1.4, §6.4).
- libeis in-compositor for injection; per-share absolute devices with `mapping_id`-joined
  regions; emulated-input badging and pause-on-lock (§5).
- g-r-d's plane separation (capture/input/transport) and damage-aware + hardware-encode pipeline
  as the model for any spatial-os streaming daemon (§2).
- Permission-store-backed restore tokens with vendor-scoped restore data (§1.3).

**Reject:**
- Claiming native float-depth PipeWire streams (no such format; §6.3).
- Relying on cross-stream frame atomicity (doesn't exist; use one-buffer-many-blocks; §6.5).
- Burning cursor/presence into spectate frames as the only mode (metadata mode exists; §1.2).
- A bespoke capture protocol where ext-image-copy-capture suffices.

**Defer:**
- OpenXR-loader capture layer for uncooperative XR apps (pattern proven by obs-vkcapture; §4).
- `WORKSPACE` source type mechanics (belongs to modes 4-5 docs).
- Upstreaming a single-channel float SPA video format.
- 6DoF input injection semantics for shared 3D volumes (§9).
- wlr-screencopy legacy support (only on demonstrated need; §8.5).

## 11. Open questions

1. **Depth on the wire for `APP_VOLUME`:** GRAY16 quantization vs. custom-metadata-described
   `D32` dmabuf blocks vs. upstream format addition — and does any real consumer besides our own
   viewer exist soon enough to care about upstream purity?
2. **Who re-renders for observers?** `APP_VOLUME` implies per-observer extra views from the
   *client* (composition doc's extra-views mechanism); does the portal `Start` result need to
   carry the observer count / view budget so clients can decline?
3. **Spectate source identity:** is the composed eye view a real `wl_output` (visible to all
   output-based tooling: good and bad) or a virtual output minted per session (`VIRTUAL=4`
   already models this)?
4. **Consent UI in-space:** the chooser is exec'd by the backend (xdpw pattern) — but our chooser
   must render *inside* the compositor. Does the backend live in-process with the compositor
   (g-r-d/Mutter split suggests: keep the record API private and the backend separate), or do we
   accept a private D-Bus API like Mutter's?
5. **`pipewire-serial` plumbing:** interface v6 targeting via `PW_KEY_TARGET_OBJECT` — verify
   pipewire-rs exposes `object.serial` early enough for our backend's `Start` reply (xdpw reads
   it from stream properties on state change, `pipewire_screencast.c:355-371`).
6. **Presence modes for spectate:** what exactly is strippable from the composed frame
   (notifications? other users' avatars?) without a second composition pass — may force a
   "spectate = re-compose with policy" design instead of "spectate = tee the eye buffer".
