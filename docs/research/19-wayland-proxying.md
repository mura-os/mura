# 19 — Wayland protocol proxying and virtio surface forwarding ("share-the-app")

**Status:** research input for `docs/architecture/spatial-sharing.md`. **Date:** 2026-09-22.
**Scope:** sharing **mode 4 — share-the-app**: instead of capturing pixels, forward the *Wayland
protocol* so that a remote or VM-isolated application's surfaces arrive as **real `wl_surface`s**
in zxr-shell-v2, and are textured onto compositor-generated depth planes exactly like local 2D
windows ([zxr-shell-v2-composition §7.3](../architecture/zxr-shell-v2-composition.md)).
**Out of scope:** pixel capture / video streaming (mode 2), which is the sibling comparison in §7
and §9(a); the XR protocol itself ([10](10-xr-wayland-protocol-comparison.md)).

**Evidence legend.** `[V]` verified by reading the cited code or primary source in the local clone;
`[R]` reported by a primary source (README/design doc/spec) but not independently executed here;
`[I]` inference or design judgement by this document. Code citations are `repo/path:line` relative
to the reference clone root `references/`.

**Clone provenance.** `waypipe` a1ffdd8 (v0.11.2, 2026-08-25); `wprs` 12b864d (2026-08-31);
`platform2` a2754cad (2026-09-22, only `vm_tools/sommelier` studied); `crosvm` 847fbc5 (2026-09-21);
`wayland-proxy-virtwl` aa515e5 (2026-09-04); `spectrum` 77d2389 (2026-08-06). `[V]`

---

## 1. Purpose and scope: what mode 4 buys, and why it is hard

### 1.1 The proposition

A pixel-capture share (mode 2) gives the XR compositor a *video frame*: flat, pre-composited, at the
source's resolution and framerate, with input that must be synthesised backwards. A protocol proxy
gives it **surfaces**. A remote Firefox becomes a `wl_surface` with an `xdg_toplevel`, real
subsurfaces and popups, a real `wl_buffer` the compositor samples directly as a texture, and
ordinary `wl_pointer`/`wl_keyboard` input from our seat. Everything §7.3 of the composition model
says about local 2D windows then applies unchanged: arbitrary 6DoF pose, compositor-generated
per-pixel plane depth in the shared depth test, resolution driven by *our* scale, no capture
pipeline at all. The same machinery is also an **isolation** architecture: if the "remote" is a
microVM on the same machine, mode 4 becomes Qubes-style per-app compartmentalisation whose windows
remain native, GPU-composited, correctly-depth-tested planes — precisely what Spectrum OS does today
with crosvm (§5).

### 1.2 Why Wayland is not network-transparent

Wayland's wire format is small — object id, opcode, length, inline args. That part proxies
trivially. The problem is that all the *content* travels out-of-band as file descriptors attached
to `SCM_RIGHTS` ancillary data, and a file descriptor is meaningless across a machine or VM
boundary:

| fd class | Carried by | What must happen at a boundary |
|---|---|---|
| shm pool | `wl_shm.create_pool` | replicate the mapping; ship only what changed |
| dmabuf planes | `zwp_linux_buffer_params_v1.add` | re-allocate on the far GPU; ship contents or an encoded stream |
| syncobj timeline | `wp_linux_drm_syncobj_manager_v1.import_timeline` | re-create a timeline semaphore; translate signal points |
| keymap | `wl_keyboard.keymap` | one-shot read-only file copy |
| clipboard / DnD pipes | `wl_data_offer.receive`, `…source.send`, primary-selection, data-control | create a local pipe pair and pump bytes |
| ICC profile | `wp_image_description_creator_icc_v1.set_icc_file` | one-shot read-only file copy |

A proxy therefore cannot be a dumb byte relay. It must parse *enough* of the protocol to know which
messages carry fds, what each fd means, and how the associated object's lifetime and damage work.
All four proxies studied here are, at bottom, four different answers to how much parsing that is.

---

## 2. waypipe — deep dive

Waypipe is the general-purpose, transport-agnostic network proxy, developed at
freedesktop.org (`waypipe/README.md:131-134`). The Rust rewrite is the current implementation;
the original C is retained as `waypipe-c` "for use on older systems", and explicitly *lost*
features in the port — notably reconnection support (`waypipe/README.md:101-108`). `[V]`

### 2.1 Roles, topology, threading

Two processes, mirror images of each other. **`waypipe client`** runs on the **display side**: it
opens a listening socket (default `/tmp/waypipe-client.sock`) and connects to the local compositor
as an ordinary Wayland client (`waypipe/waypipe.scd:31-34`). **`waypipe server`** runs on the
**application side**: it creates a *fake compositor socket*, sets `WAYLAND_DISPLAY` (or
`WAYLAND_SOCKET` with `--oneshot`), execs the application, and connects out to the matching client
socket (`:36-40`, `:293-299`). Note the inversion that trips everyone up — the *server* is next to
the app, the *client* next to the display. `waypipe ssh user@host app` wraps both plus an `ssh -R`
Unix-socket reverse tunnel, requiring OpenSSH ≥ 6.7 (`waypipe/README.md:14-35`,
`waypipe.scd:353-357`). Internally, `Globals.on_display_side` is the single boolean distinguishing
the roles throughout the message handlers (`waypipe/src/tracking.rs:4242`, `:4334`). `[V]`

One process per connection: a main loop (`waypipe/src/mainloop.rs:4656 loop_inner`) plus a worker
pool sized by `--threads T`, defaulting to *half* the hardware threads
(`waypipe/src/main.rs:2141-2143`). Workers run `work_thread` (`mainloop.rs:4370`) draining a
`TaskSet` of `DiffTask`, `DiffDmabufTask`(+`…2`), `FillDmabufTask`(+`…2`), `VideoEncodeTask`,
`VideoDecodeTask`, `DecompTask`, `ApplyTask` (`:408-522`); a separate `vulkan_wait_thread` (`:4572`)
services GPU timeline waits so the main loop never blocks on the GPU. Parallelism granularity is
the **diff chunk**, `DIFF_CHUNKSIZE = 262144` bytes (`:2013`), with `split_damage` (`:2078`)
partitioning damaged intervals into per-thread work lists; diff and compress are fused per
chunk. `[V]`

### 2.2 The channel wire protocol

Waypipe's own protocol on the transport is a sequence of length-tagged messages. `WmsgType`
(`waypipe/src/util.rs:70-111`) enumerates the whole surface:

| Msg | Purpose |
|---|---|
| `Protocol` | a batch of verbatim (possibly rewritten) Wayland messages |
| `InjectRIDs` | bind previously-transferred fds to the next protocol messages |
| `OpenFile` / `ExtendFile` | create/grow a shm replica of a given size |
| `BufferFill` / `BufferDiff` | whole-region fill, or a diff, optionally compressed |
| `OpenIRPipe` / `OpenIWPipe` / `OpenRWPipe` / `PipeTransfer` / `PipeShutdownR` / `PipeShutdownW` | pipe lifecycle and data |
| `OpenDMABUF` | create a dmabuf replica with `dmabuf_slice_data` |
| `OpenDMAVidSrc(V2)` / `OpenDMAVidDst(V2)` / `SendDMAVidPacket` | video-mode buffers and encoded packets |
| `OpenTimeline` / `SignalTimeline` | drm-syncobj timeline replica and signal points |
| `AckNblocks`, `Restart`, `Close`, `Version` | flow control, (C-era) reconnect, teardown, negotiation |

Connection setup negotiates a version and a feature bitfield in one word: `CONN_NO_DMABUF_SUPPORT`,
`CONN_{NO,LZ4,ZSTD}_COMPRESSION`, `CONN_{NO,VP9,H264,AV1}_VIDEO` (`util.rs:53-68`). The current
protocol version is `0x11` with a floor of `0x10` (`util.rs:53-54`). `[V]`

Each fd Waypipe proxies becomes a **`ShadowFd`** with a remote id (`Rid`) and one of four variants:
`File`, `Pipe`, `Dmabuf`, `Timeline` (`mainloop.rs:635-652`). `MAX_OUTGOING_FDS = 8` bounds how many
fds ride one `sendmsg` (`mainloop.rs:173`). `[V]`

### 2.3 fd class: `wl_shm` pools — mirror, diff, compress

On `wl_shm.create_pool` (`tracking.rs:1632`), `translate_shm_fd` (`mainloop.rs:1128`) mmaps the
client's pool as an `ExternalMapping` and allocates a **`Mirror`** of the same size — Waypipe's
private copy of what the *other end* currently holds (`mainloop.rs:533-551`, `waypipe/src/mirror.rs`).
`ExternalMapping` is deliberately exposed as `&[AtomicU8]`/SIMD-only because the client may mutate
the pool concurrently and a plain `&[u8]` would be unsound (`waypipe/src/kernel.rs:9-27`). On the
far side the replica is a fresh `memfd_create` (`mainloop.rs:1397`). `[V]`

Damage drives everything. Waypipe tracks `wl_surface.damage` and `damage_buffer`
(`tracking.rs:1921`, `:1952`), applies buffer transform/scale and any `wp_viewport` source/dest
transform (`:310 apply_viewport_transform`, `:396`, `:452`, `:530`), clips to the buffer (`:277`),
and converts rectangles to byte intervals (`:765 get_damage_for_shm`, `waypipe/src/damage.rs:38-60`)
aligned to 64-byte boundaries with near-adjacent intervals merged. Only those intervals are diffed.
The diff kernel (`waypipe/src/kernel.rs`) is **cache-line granular** — it compares 64-byte blocks of
target vs. mirror and emits `(start,end)` runs — with an AVX2 path
(`kernel.rs:183-184 construct_diff_segment_two_avx2`, dispatched at `:604-630`) and a scalar
fallback (`:490-501`); `apply_diff` (`:710`) writes both the receiver's mirror and the real
mapping. `[V]`

Compression applies to fills and diffs: **`none`, `lz4`, `zstd`, default `lz4`**
(`waypipe/waypipe.scd:50-58`), with `-c zstd=7`-style level suffixes, over thin FFI wrappers
(`waypipe/src/compress.rs`, `wrap-lz4/`, `wrap-zstd/`). `waypipe bench` exists to pick the best
setting for a given bandwidth and thread count (`waypipe.scd:41-46`). **Confirmed: both zstd and
lz4, not one or the other.** `[V]`

### 2.4 fd class: `linux-dmabuf` — replicated GPU allocation, or video

Waypipe's dmabuf support is **Vulkan-first** with a GBM fallback (`dmabuf = ["dep:ash"]`,
`gbmfallback = ["dep:waypipe-gbm-wrapper"]`, `waypipe/Cargo.toml:31-37`; `DmabufDevice` is
`Unknown | Unavailable | VulkanSetup | Vulkan | Gbm`, `mainloop.rs:97`). App-side,
`zwp_linux_buffer_params_v1.add` accumulates planes (`tracking.rs:2554`) and `create`/`create_immed`
(`:2718`, `:2590`) call `translate_dmabuf_fd` (`mainloop.rs:1172`), importing the client's dmabuf
into Vulkan (`vulkan_import_dmabuf`, `:1213`) or GBM (`:1231`). The display side **allocates a fresh
dmabuf** with `vulkan_create_dmabuf` (`waypipe/src/dmabuf.rs:2342`, called at `mainloop.rs:1642`,
`:1749`) or `gbm_create_dmabuf` (`waypipe/src/gbm.rs:284`) and hands *that* fd to our compositor.
The two buffers are unrelated memory kept in sync by Waypipe. `[V]`

Synchronisation is the shm diff scheme plus a GPU staging step: damage is computed in the buffer's
*linear* view (`tracking.rs:808 get_damage_for_dmabuf`), `run_diff_dmabuf_task`
(`mainloop.rs:2899`) issues `start_copy_segments_from_dmabuf` copying **only the damaged segments**
into a host-visible Vulkan buffer (`:2927-2936`), and `run_diff_dmabuf_task_2` (`:2953`) diffs that
staging buffer against the mirror and compresses. This matters for §7: the dmabuf path is *not* a
naive full-frame readback, but it *is* a GPU→CPU copy plus a CPU diff plus a compress, per damaged
region, per frame. `[V]`

**Format negotiation is intersected, not passed through.** Waypipe rewrites
`zwp_linux_dmabuf_feedback_v1` — it parses the compositor's format table
(`tracking.rs:2917 …FORMAT_TABLE`, `:1173 parse_format_table`), filters tranches to formats and
modifiers its own Vulkan/GBM device supports (`tracking.rs:854 process_dmabuf_feedback`,
`:891 rebuild_format_table`, `mainloop.rs:1053 dmabuf_dev_supports_format`), and re-emits a new
table over a fresh memfd (`tracking.rs:3082`). A proxied client therefore sees the **intersection**
of our modifiers with Waypipe's device's modifiers, not our advertised set. `[V]`

**Video mode.** `--video V` with `V ∈ {none, h264, vp9, av1} × {sw,swenc,swdec,hw,hwenc,hwdec} ×
bpf=B` (`waypipe/waypipe.scd:134-161`). The codec wiring is explicit
(`waypipe/src/video.rs:694-720`):

| Codec | Hardware | Software |
|---|---|---|
| H.264 | `h264_vulkan` (encode), `h264` (decode) | `libx264` / `h264` |
| VP9 | *(none)* | `libvpx-vp9` / `vp9` |
| AV1 | `av1_vulkan` (encode), `av1` (decode) | `libaom-av1` / `libdav1d` |

**Correction to a common assumption: the Rust implementation uses Vulkan Video, not VAAPI.** The
hwdevice type is `AV_HWDEVICE_TYPE_VULKAN` (`video.rs:16`, `:626`) and the required extensions are
`VK_KHR_video_encode_{h264,av1}` / `VK_KHR_video_decode_{h264,av1}`
(`waypipe/src/dmabuf.rs:863-895`); libva appears only in the legacy C tree
(`waypipe/waypipe-c/video.c`, `waypipe/README.md:118`). RGB↔NV12 conversion uses Waypipe's own
Vulkan compute shaders (`video.rs:22`, `:760-778`; `waypipe/shaders/`), and the default target rate
is `bpf = 1e5` bits per frame (`video.rs:2150`, `:2234`). Documented defect: one video stream is
maintained **per buffer**, so as a surface rotates through its buffer pool the window visibly
flickers between streams with different encoding artifacts (`README.md:217-222`); opaque, 10-bit
and multiplanar formats are unsupported (`waypipe.scd:134-136`). `[V]`

### 2.5 fd class: drm-syncobj timelines (explicit sync)

`wp_linux_drm_syncobj_v1` **is** supported and is the *only* explicit-sync protocol supported:
`zwp_linux_explicit_synchronization_v1` is blacklisted as "outdated, uses fences instead of
timelines" (`tracking.rs:4197`). `import_timeline` (`:2362`) → `translate_timeline`
(`mainloop.rs:1266`) creates a `ShadowFdTimeline` backed by a Vulkan timeline semaphore (`:626-633`),
and `set_acquire_point`/`set_release_point` (`tracking.rs:2410-2418`) are tracked so the far side's
copy-apply completion drives the signal (`mainloop.rs:2661 signal_timeline_acquires`,
`WmsgType::SignalTimeline`). The syncobj *global* is dropped unless the local Vulkan device supports
timeline semaphore import/export (`tracking.rs:4266-4308`); because device setup may not have
happened when the global is first advertised, Waypipe **buffers and replays** the advertisement
after `zwp_linux_dmabuf_v1` forces device selection (`:4269-4274`, `:4324-4362`). Implicit sync goes
through `DMA_BUF_IOCTL_EXPORT_SYNC_FILE` (`dmabuf.rs:580-584`, `:687`). `[V]`

### 2.6 fd class: pipes, keymaps, one-shot files

- **Pipes** (`ShadowFdPipe`, `mainloop.rs:603-624`) back every clipboard/DnD/data-control transfer —
  `wl_data_source.send`, `wl_data_offer.receive` and the `zwp_primary_selection_*`, `gtk_primary_*`,
  `ext_data_control_*`, `zwlr_data_control_*` equivalents all share one handler arm
  (`tracking.rs:3329-3346`). Buffering is a fixed 4 KiB read buffer one way and an unbounded
  `VecDeque` the other, with a source comment noting **there is no backpressure mechanism**
  (`mainloop.rs:605-609`). `[V]`
- **Keymaps and ICC profiles** are read-once fixed-size files: `wl_keyboard.keymap`
  (`tracking.rs:3197-3207` → `translate_or_wait_for_fixed_file`, `:1329`),
  `wp_image_description_info_v1.icc_file` and `…creator_icc_v1.set_icc_file` (`:3208-3260`).
  Read-once files skip mirror allocation entirely (`mainloop.rs:1141-1146`). `[V]`
- **Gamma tables** via `zwlr_gamma_control_v1.set_gamma` (`tracking.rs:3305`), and **screen
  capture** — `zwlr_screencopy_v1` and `ext-image-copy-capture-v1` are fully tracked (`:3372-4060`),
  so a proxied client can screencapture the *remote* compositor. `[V]`

### 2.7 Protocol knowledge: what it parses, what it blocks, what flows through

Waypipe compiles in **91 interfaces** (`waypipe/src/wayland_gen.rs:6892-6984`,
`INTERFACE_TABLE: &[WaylandData; 91]` at `:7084`), generated by `protogen.py` from the 22 XML files
in `waypipe/protocols/`: core `wayland.xml` (v10), `xdg-shell.xml` (v7), `linux-dmabuf-v1.xml` (v5),
`linux-drm-syncobj-v1`, `presentation-time` (v2), `commit-timing-v1`, `color-management-v1` (v2),
`viewporter`, `xdg-toplevel-icon-v1`, `security-context-v1`, `virtual-keyboard-unstable-v1`,
`input-method-unstable-v2`, `ext-foreign-toplevel-list-v1`, `ext-image-capture-source-v1`,
`ext-image-copy-capture-v1`, `ext-data-control-v1`, the four `wlr-*` protocols
(data-control, screencopy, export-dmabuf, gamma-control), both primary-selection variants, and
`wayland-drm.xml`. `[V]`

Everything else is **forwarded verbatim**. The wire format is partially self-describing, so
unparsed messages pass through; the price is that Waypipe can never add or remove protocol objects,
because it cannot rewrite object ids in messages it does not understand
(`waypipe/README.md:181-198`). Practical consequence: `xdg-decoration`, `fractional-scale-v1`,
`tearing-control`, `pointer-constraints`, `text-input`, `xdg-activation`, `ext-session-lock` and
friends are **not in the table and do not need to be** — they carry no fds and simply work. `[V/I]`

Five globals are **dropped from the registry** (`tracking.rs:4192-4198`): `wl_drm` (superseded by
linux-dmabuf v4), `wp_drm_lease_device_v1`, `zwlr_export_dmabuf_manager_v1`,
`zwp_linux_explicit_synchronization_v1`, and `wp_security_context_manager_v1` — the last because it
"sends socket listen fd over network". Six get **version clamped** to Waypipe's compiled-in maximum
(`:4202-4226`), and `zwp_linux_dmabuf_v1` is additionally dropped when no GPU device is usable
(`:4233-4261`), which is also what `--no-gpu` forces (`waypipe.scd:66-68`). Note that Waypipe
*uses* the security-context protocol even though it refuses to forward it: `--secctx S` makes
`waypipe client` attach a security context with app-id `S` to the socket it hands the compositor
(`waypipe/src/secctx.rs:25-35`) — a load-bearing fact for §8. `[V]`

Two rewriting behaviours worth noting: `--title-prefix P` mutates `xdg_toplevel.set_title`
(`tracking.rs:4061-4075`), and `wp_presentation`/`wp_commit_timer_v1` timestamps are **converted
between the two machines' clocks** (`tracking.rs:4125-4185`, `:1216 timespec_midpoint`,
`:1241 clock_sub`). `[V]`

### 2.8 Transport assumptions

Waypipe needs exactly one **reliable, ordered byte stream**. Confirmed modes `[V]`: a local Unix
socket (`waypipe.scd:203-211`); `ssh -R` Unix-socket forwarding, automated by `waypipe ssh`
(`:186-201`); arbitrary TCP/TLS via `ncat`/`socat` bridging into the Unix socket (`:213-239`, the
documented escape hatch for *any* transport); and **`--vsock`** with `-s [s]CID:PORT`, covering
host↔guest in both directions plus sibling-guest via `VMADDR_FLAG_TO_HOST` (`:163-168`, `:259-289`;
`waypipe/src/main.rs:191-228`, `:489-530`). There is no framing requirement beyond stream
reliability, no multiplexing, no UDP mode, and no built-in encryption or authentication — those are
the transport's job (`waypipe.scd:307-316`).

### 2.9 Known limits (from the source and man page) `[V]`

**No resource limits** — a malicious peer can force arbitrary memory/CPU/GPU use
(`README.md:172-177`). **No protocol filtering** — if the compositor exposes screenshot or
lock-screen protocols, proxied clients get them too (`waypipe.scd:309-316`). **Traffic analysis** —
message size and timing leak typing, scrolling and pointer motion even over ssh (`README.md:163-170`).
**Latency spikes on full-window updates** — mirror/diff is excellent for static UI, poor for
animation and games (`:199-211`). **No reconnection** in the Rust implementation; it exists only in
`waypipe-c` (`:101-108`). **No object-id rewriting**, hence no global deduplication or startup-time
tricks (`:192-197`). **Little-endian only**; big-endian needs an external `wswapendian` shim
(`:227-234`). **Single-instance apps** (Firefox, gnome-terminal, kate) may silently open a tab in
the *local* instance (`waypipe.scd:346-352`). And video mode flickers (§2.4) while Vulkan hardware
video is "somewhat experimental" (`README.md:223-225`).

---

## 3. wprs — xpra semantics for Wayland

Google's `wprs` takes the opposite design decision: **do not forward the protocol, forward
serialized state.** `[V]`

**Architecture and serialization.** `wprsd` (application side) is a *real compositor* built on
Smithay; `wprsc` (display side) is a Smithay-Client-Toolkit *client* recreating corresponding local
objects (`wprs/README.md:139-156`). Wayland objects are Rust types serialized with `rkyv`, and the
protocol is deliberately **idempotent**: instead of the
surface/xdg-surface/toplevel/configure/attach/commit dance, `wprsd` sends one commit message
carrying the surface's *complete* state (`:176-188`;
`wprs/src/serialization/{wayland,xdg_shell,framing}.rs`). `[V]`

**Session resumption — the headline feature.** Because `wprsd` holds all compositor state including
last-committed buffer contents, `wprsc` can disconnect, be killed and reconnect without applications
noticing; a `wprsd` restart still kills everything, like any compositor restart (`:157-167`).
Waypipe-rs cannot do this at all. Frame callbacks are generated *locally* by `wprsd` at a configured
rate rather than round-tripped, and paused when no client is attached (`:189-192`). `[V]`

**Compression** is a bespoke lossless image codec: AoS→SoA channel transpose, per-channel wrapping
DPCM, a YUV-like decorrelation (`y:=g, u:=b-g, v:=r-g`), then zstd (`wprs/README.md:194-218`;
`wprs/src/sharding_compression.rs:36`, `MIN_SIZE_TO_COMPRESS = 4096` at `:44`), sharded across
threads with hand-written SSE2/SSSE3/SSE4.1/AVX/AVX2 and NEON kernels (`wprs/src/simd/`,
`wprs/src/filtering/{x86,neon}.rs`). `[V]`

**Coverage, maturity, security.** Exactly what `wprsd` delegates: compositor, xdg-shell,
xdg-decoration, kde-decoration, shm, seat, data-device, output, primary-selection, viewporter
(`wprs/src/server/smithay_handlers.rs:1243-1252`; construction at `wprs/src/server/mod.rs:148-159`).
**No `linux-dmabuf`, no explicit sync, no presentation-time**; no touch; no XWayland DnD
(`wprs/README.md:126-137`). XWayland is a separate `xwayland-xdg-shell` binary modelled on sommelier
and wayland-proxy-virtwl (`:239-259`). Version `0.1.0` (`wprs/Cargo.toml:3`), with a wire protocol
explicitly *not stable* across builds, dependency versions or rustc versions (`:220-224`). `wprsd`
being a compositor, anything reaching its socket has full surface access and can inject input;
protection is Unix permissions in `$XDG_RUNTIME_DIR` plus the transport (`:261-273`). `[V]`

**Assessment `[I]`:** wprs's resumption semantics are genuinely desirable and unavailable elsewhere,
but the absence of dmabuf, the version-fragile wire format and the narrower coverage make it
unsuitable as spatial-os's primary mechanism. Its *idea* — a stateful, idempotent surface-state
protocol — is the right model if we ever want detachable XR sessions.

---

## 4. Sommelier and crosvm cross-domain — the VM path

### 4.1 What sommelier is, and its channel abstraction

A nested Wayland compositor that "delegates compositing to a 'host' compositor", designed to run
inside a tight jail or VM (`platform2/vm_tools/sommelier/README.md:1-8`). Three deployment shapes:
a **parent** sommelier owning the guest's `$XDG_RUNTIME_DIR` socket and forking a child per
connection; a **peer** sommelier per application (better isolation and multicore use); and a shared
**X11** sommelier hosting Xwayland for all X clients (`sommelier/README.md:11-34`). `[V]`

`WaylandChannel` (`sommelier/virtualization/wayland_channel.h:78-165`) is a small virtual interface
— `init`, `supports_dmabuf`, `create_context`, `create_pipe`, `send`, `handle_channel_event`,
`allocate`, `sync`, `handle_pipe`, `max_send_size` — with two implementations: `VirtWaylandChannel`
(the legacy `/dev/wl0` virtio-wl device) and `VirtGpuChannel` (virtio-gpu cross-domain contexts)
(`:167-299`). A null channel is "noop mode, without virtualization": plain socket proxying
(`sommelier/compositor/sommelier-shm.cc:43-51`). `[V]`

**"Contexts" replace virtio-wl.** A cross-domain context is a virtio-gpu *context* created with
`DRM_IOCTL_VIRTGPU_CONTEXT_INIT` (`sommelier/virtualization/virtgpu_channel.cc:260-290`) using
capset `VIRTIO_GPU_CAPSET_CROSS_DOMAIN = 5` (`crosvm/devices/src/virtio/gpu/protocol.rs:422`,
`context_init` at `:346-352`, `crosvm/devices/src/virtio/gpu/virtio_gpu.rs:1306`). Crosvm enables it
with `--gpu=context-types=cross-domain --wayland-sock $XDG_RUNTIME_DIR/wayland-0`, and it requires
guest Linux ≥ 5.16 with `CONFIG_DRM_VIRTIO_GPU` (`crosvm/docs/book/src/devices/wayland.md:11-15`,
`:64`). `[V]`

### 4.2 The cross-domain protocol

Defined in `sommelier/virtualization/virtgpu_cross_domain_protocol.h` (mirrored in rutabaga):

- Commands: `INIT`, `GET_IMAGE_REQUIREMENTS`, `POLL`, `SEND`, `RECEIVE`, `READ`, `WRITE` (`:10-17`).
- Channel types `WAYLAND = 0x0001` and `CAMERA = 0x0002` (`:19-21`); up to
  `CROSS_DOMAIN_MAX_IDENTIFIERS = 28` fds per message (`:24`), deliberately matching virtio-wl's
  `VIRTWL_SEND_MAX_ALLOCS` (`wayland_channel.h:13-17`).
- Identifier types: `VIRTGPU_BLOB` (memory resource), `VIRTGPU_SYNC` (sync resource), `READ_PIPE`,
  `WRITE_PIPE` (`:26-41`).
- Two rings — a `QUERY_RING` for metadata and a `CHANNEL_RING` for the Wayland byte stream
  (`:43-48`) — both guest blobs (`VIRTGPU_BLOB_MEM_GUEST | VIRTGPU_BLOB_FLAG_USE_MAPPABLE`) that
  the guest mmaps (`virtgpu_channel.cc:219-258`). `[V]`

So the fd problem is solved by **naming**: an fd becomes an identifier + type, and the host side
(rutabaga's cross-domain backend) re-materialises the real host object.

### 4.3 How buffers actually cross, and where the copies are

Two paths, and the difference is the whole point:

1. **Host-allocated blob (shm-equivalent).** `image_query` sends `GET_IMAGE_REQUIREMENTS` and gets
   back `CrossDomainImageRequirements { strides[4], offsets[4], modifier, size, blob_id, map_info,
   memory_idx, physical_device_idx }` (`virtgpu_cross_domain_protocol.h:60-69`,
   `virtgpu_channel.cc:556`). The guest creates a **host blob**
   (`VIRTGPU_BLOB_MEM_HOST3D | USE_MAPPABLE | USE_SHAREABLE`, `virtgpu_channel.cc:648-661`) and maps
   it, so guest CPU writes land directly in host-visible memory the host compositor imports as a
   dmabuf: **zero host-side copy, one guest-side copy.** `[V]`
2. **Guest GPU resource.** `fd_analysis` (`virtgpu_channel.cc:698-738`) runs
   `DRM_IOCTL_VIRTGPU_RESOURCE_INFO` on an incoming dmabuf fd to recover its virtgpu resource id and
   sends that as a `VIRTGPU_BLOB` identifier. A buffer guest Mesa (virgl/venus) allocated is shared
   with the host by reference: **genuinely zero-copy.** `[V]`

The guest-side copy is unavoidable for `wl_shm`, whose pool comes from ordinary guest memory the
host cannot map. Sommelier therefore **never forwards the pool fd**: `sl_shm_create_host_pool` keeps
the client's fd (`sommelier/compositor/sommelier-shm.cc:107-133`), mmaps each buffer (`:53-70`),
allocates a host buffer via `channel->allocate()` with `dmabuf = true`
(`sommelier/compositor/sommelier-compositor.cc:235-243`), and at commit copies **only the damaged
rectangles** (`copy_damaged_rect` at `:543`, from `sl_host_surface_commit` at `:608-716`, with
`pixman_region32` surface- and buffer-damage accumulators at `:46-47`), releasing the client buffer
immediately afterwards (`sommelier/README.md:93-99`). Damage is tracked **per host buffer in a
queue**, so each recycled buffer knows what it is missing relative to the current frame
(`README.md:83-92`); there is deliberately **no backpressure** (`:101-108`). The "virtwl-dmabuf"
rationale is exactly our argument in §9: a **dmabuf** intermediate gives more formats (NV12), skips
host texture upload, and allows hardware overlay scanout (`README.md:67-77`). `[V]`

### 4.4 What sommelier proxies, and its scaling layer

From the registry handler (`sommelier/sommelier.cc:516-802`): `wl_compositor`, `wl_subcompositor`,
`wl_shm`, `wl_shell`, `wl_output`, `wl_seat`, `zwp_relative_pointer_manager_v1`,
`zwp_pointer_constraints_v1`, `wl_data_device_manager`, `xdg_wm_base`, `zaura_shell`,
`wp_viewporter`, `zwp_linux_dmabuf_v1`, `zwp_linux_explicit_synchronization_v1`,
`zcr_keyboard_extension_v1`, `zwp_text_input_manager_v1`, `zcr_text_input_extension_v1`,
`zcr_gaming_input_v2`, `zcr_stylus_v2`, `zxdg_output_manager_v1`, `wp_fractional_scale_manager_v1`,
`zwp_idle_inhibit_manager_v1`. `wl_shm` is advertised to guests at **version 1 only**
(`sommelier-shm.cc:213`, `:233`), with formats filtered/translated from either host `wl_shm` or
host `zwp_linux_dmabuf_v1` depending on channel dmabuf support (`sommelier-shm.cc:138-188`). `[V]`

Three translations are worth stealing conceptually `[V]`:

- **`zaura_shell` → `gtk_shell`** (`sommelier.cc:629-643`): host-specific window management
  re-expressed in a protocol guests already speak.
- **`zcr_stylus_v2` → `tablet-unstable-v2`** (`sommelier.cc:741-758`): a vendor input protocol
  presented to guests as the standard one.
- **`wp_viewporter` is what makes non-integer scaling possible** — binding it sets `ctx->scale` to
  the desired fractional value (`sommelier.cc:644-657`). The whole density story (`--scale`, DPI
  bucketing, per-client optimal `set_buffer_scale`) rides on viewporter plus fractional-scale
  (`sommelier/README.md:130-169`, `sommelier/sommelier-transform.cc`).

**X11 path.** Sommelier runs Xwayland and acts as its window manager in-process
(`sommelier/README.md:19-28`; `sommelier-xdg-shell.cc`, `sommelier-window.cc`, `sommelier/xcb/`):
`sommelier -X --xwayland-path=/usr/bin/Xwayland xeyes`. The crucial property is that the **host
compositor needs no X11 support whatsoever** (`crosvm/docs/book/src/devices/wayland.md:86-90`). `[V]`

### 4.5 crosvm: the host side

**`devices/src/virtio/wl.rs`** (2126 lines) is the original virtio-wl device. Two virtqueues, `in`
and `out`; every proxied fd is a `WlVfd` wrapping either a shared-memory fd (installed into a
hypervisor memory slot and exposed to the guest by PFN) or a Unix socket to the host Wayland server
(`wl.rs:5-29`, `VIRTIO_WL_PFN_SHIFT` at `:258`). The command set is
`VFD_NEW/CLOSE/SEND/RECV/NEW_CTX/NEW_PIPE/HUP/NEW_DMABUF/DMABUF_SYNC/SEND_FOREIGN_ID/NEW_CTX_NAMED`
(`wl.rs:163-177`). Send kinds `LOCAL`, `VIRTGPU`, `VIRTGPU_FENCE`, `VIRTGPU_SIGNALED_FENCE`
(`wl.rs:253-256`) let the guest hand over a *virtgpu resource* instead of memory; the wl device asks
the gpu device over a `resource_bridge` Tube to resolve it into a real host dmabuf
(`wl.rs:138-146`, `:1385-1397`, `:1467-1518`). `[V]`

**The gpu device's cross-domain context is the successor.** In this clone `rutabaga_gfx` is an
*external crate* (`crosvm/Cargo.toml:214`, `= 0.1.80`), so the cross-domain backend itself is not
vendored and its internals are `[R]`, not `[V]` — but the guest-visible contract is fully pinned by
`virtgpu_cross_domain_protocol.h` plus the capset/context plumbing in
`crosvm/devices/src/virtio/gpu/{protocol.rs,virtio_gpu.rs}`. Crucially, crosvm can run the gpu
device **out-of-process** as a vhost-user device
(`crosvm device gpu --fd N --wayland-sock … --params '{"context-types":"cross-domain"}'`). `[V]`

**Security model.** The guest never gets a host fd; it gets virtgpu resource ids and pipe
identifiers scoped to its own context, and crosvm holds the single host Wayland socket. The device
can be split into its own process and jailed, which is exactly what Spectrum does (§5.2). Residual
trust: whatever the *host compositor* exposes on that socket, the guest's proxy can use — the same
caveat as waypipe (`waypipe.scd:309-316`). `[V/I]`

---

## 5. wayland-proxy-virtwl and Spectrum OS

### 5.1 The proxy

talex5's `wayland-proxy-virtwl` is an OCaml relay built on `ocaml-wayland` and Eio, explicitly "similar
to the sommelier proxy from ChromiumOS … easier to modify and less segfaulty", adding primary
selection (`wayland-proxy-virtwl/README.md:1-18`). With `--virtio-gpu` it scans `/dev/dri/` for a
virtio-gpu device and connects through cross-domain instead of a local socket;
**"the proxy previously used the virtwl protocol, but virtio-gpu has now replaced it"**
(`README.md:51-57`). It wants `--gpu=context-types=cross-domain:virgl2` on the host
(`README.md:67`). `[V]`

**Protocol coverage is an explicit whitelist** (`wayland-proxy-virtwl/src/relay.ml:1351-1369`):
`wl_shm`, `wl_compositor`, `wl_subcompositor`, `xdg_wm_base`, `wl_data_device_manager`,
`zxdg_output_manager_v1`, `zwp_primary_selection_device_manager_v1` (plus a `gtk_primary_selection`
compatibility alias, `:1381-1383`), `wl_seat`, `wl_output`,
`org_kde_kwin_server_decoration_manager`, `zxdg_decoration_manager_v1`,
`zwp_relative_pointer_manager_v1`, `zwp_pointer_constraints_v1`, `wp_viewporter`,
`wp_cursor_shape_manager_v1`. Anything else is refused with `"Invalid service name"` (`:1427`), and
versions are clamped to `min(proxy_version, host_version)` (`:1379`). There is **no
`zwp_linux_dmabuf_v1` for clients.** `[V]`

**Buffers.** "Since regular guest memory cannot be shared with the host, the proxy allocates a
shadow buffer from the host and copies the frame data into that. Wayland can also use graphics
memory directly, which should avoid the copy, but this is not yet supported." (`README.md:83-86`).
The copy is a **whole-pool blit on every commit** —
`Cstruct.blit data.client_memory 0 data.host_memory 0 (Cstruct.length data.client_memory)`
(`relay.ml:455`) — with "only copy the buffer regions that have changed" still on the TODO list
(`README.md:262-264`): strictly worse than sommelier's damaged-rect copy. Host-side the shadow
buffer is wrapped as a `wl_buffer` through `zwp_linux_dmabuf_v1` on the *host* connection
(`wayland-proxy-virtwl/virtio_gpu/wayland_dmabuf.ml:72-92`), with a 1×1 probe buffer at startup to
decide whether video memory is usable at all (`probe_drm`, `:39-70`). `[V]`

**Xwayland.** `--x-display=0` listens on `@/tmp/.X11-unix/X0`, spawns Xwayland on demand, and acts
as the X11 window manager — `WM_NAME` → `xdg_toplevel` title, correct xdg roles for
dialogs/menus/tooltips, PRIMARY/CLIPBOARD bridged (`README.md:88-110`). Known failure:
**drag-and-drop does not work, even X→X**, because the hidden X11 window layout cannot be reconciled
with a Wayland layout the compositor never exposes (`:116`). systemd-style socket activation is
supported for both sockets (`:120-127`). `[V]`

### 5.2 How Spectrum wires it

Spectrum OS runs each application in its own VM and composites its windows natively on the host
Weston (`spectrum/host/rootfs/image/etc/s6-rc/weston/run`, `WAYLAND_DISPLAY=/run/wayland/wayland`).
The design statement is unambiguous: **"Every VM has a virtio-gpu device that provides only the
cross-domain context with a Wayland channel (no GPU acceleration). This can be used by the VM to
display Wayland windows on the host."**
(`spectrum/Documentation/doc/using-spectrum/creating-custom-vms.adoc:65-70`). Guest side, an s6
service socket-activates the proxy for both Wayland and X11 —
`wayland-proxy-virtwl --virtio-gpu --x-display=0` with `LISTEN_FDNAMES wayland:x11`
(`spectrum/img/app/image/etc/s6-rc/wayland-proxy-virtwl/run`). `[V]`

Host side is the strongest isolation design in this survey
(`spectrum/host/rootfs/image/etc/s6-linux-init/run-image/service/vm-services/template/data/service/vhost-user-gpu/run`):

- the VMM is **cloud-hypervisor**, patched to accept a vhost-user GPU backend
  (`spectrum/pkgs/cloud-hypervisor/0002-virtio-devices-add-a-GPU-device.patch`), invoked as
  `--gpu socket=…` (`spectrum/release/checks/wayland/default.nix:35`), running as uid `vmm-${VM}`
  and owning only the crosvm control socket (run script `:8-17`);
- the GPU backend is a **separate `crosvm device gpu` process per VM**
  (`--params '{"context-types":"cross-domain"}'`, `:40-43`);
- that process runs as its own uid `gpu-${VM}`, supplementary group 15 (`wayland`), under
  `bwrap --unshare-all --unshare-user --disable-userns`, with **only** the host Wayland socket
  bind-mounted in, `/usr`, `/lib`, `/nix` read-only, tmpfs `/tmp` and `/dev/shm`, and
  `/proc/{fs,irq,kallsyms}` neutered (`:19-38`). `[V]`

Trust chain: guest kernel → virtio-gpu → *unprivileged, jailed, per-VM* cross-domain backend → host
compositor socket. A compromised guest gets at worst the authority of one sandboxed process holding
one Wayland connection. **Properties gained `[I]`:** per-app kernel-level isolation with native,
non-captured windows; no shared filesystem or IPC namespace; the host compositor never runs
untrusted app code; and the guest↔host attack surface is the cross-domain command set (7 commands,
4 identifier types) rather than the full Wayland protocol.

**What breaks `[V]`:** no GPU acceleration in guests at all in Spectrum's configuration
(cross-domain only — talex5's own setup adds `virgl2`, Spectrum does not); no `linux-dmabuf` to
guest clients from `wayland-proxy-virtwl` (§5.1), so even with virgl a guest client cannot hand it a
dmabuf; a full-pool copy per commit scaling with window area rather than damage (`relay.ml:455`); no
X11 drag-and-drop; and a whitelist registry that fails hard rather than degrading when a client
needs an unlisted global.

---

## 6. Protocol coverage matrix

Globals/extensions each proxy *understands* (parses, translates or re-implements) and offers to the
application. "pass" = flows through unparsed and works because it carries no fds.

| Global / extension | waypipe | wprs | sommelier | wayland-proxy-virtwl |
|---|---|---|---|---|
| `wl_compositor`, `wl_surface` | ✔ parse | ✔ reimpl | ✔ proxy | ✔ proxy |
| `wl_subcompositor` / `wl_subsurface` | ✔ | ✔ (client/subsurface.rs) | ✔ | ✔ |
| `wl_shm` (+ pool resize) | ✔ diff | ✔ reimpl | ✔ copy→host blob (v1 only) | ✔ copy→host blob |
| `zwp_linux_dmabuf_v1` | ✔ v5, replicate/encode | ✘ | ✔ (host-alloc + resource ids) | ✘ to clients |
| `wp_linux_drm_syncobj_v1` | ✔ timeline replica | ✘ | ✘ | ✘ |
| `zwp_linux_explicit_synchronization_v1` | dropped (`:4197`) | ✘ | ✔ bound | ✘ |
| `wl_drm`, `wp_drm_lease_device_v1`, `zwlr_export_dmabuf` | dropped (`:4194-4195`) | ✘ | `wl_drm` only | ✘ |
| `xdg_wm_base` (+ toplevel/popup/positioner) | ✔ v7 (title rewrite) | ✔ reimpl | ✔ | ✔ |
| xdg- and KDE-decoration | pass | ✔ reimpl both | ✘ | ✔ both |
| `wl_seat` / pointer / keyboard / touch | ✔ (keymap fd) | ✔ (no touch) | ✔ | ✔ |
| `wl_data_device_manager` (clipboard + DnD) | ✔ pipes | ✔ (DnD "wonky") | ✔ | ✔ |
| primary selection (`zwp_`/`gtk_`) | ✔ pipes | ✔ | ✘ | ✔ both |
| `ext-`/`wlr-data-control` | ✔ pipes | ✘ | ✘ | ✘ |
| `wl_output` | ✔ | ✔ | ✔ (rescaled) | ✔ |
| `zxdg_output_manager_v1` | pass | ✘ | ✔ (direct-scale only) | ✔ |
| `wp_viewporter` | ✔ (damage transform) | ✔ | ✔ (enables fractional scale) | ✔ |
| `wp_fractional_scale_v1` | pass | ✘ | ✔ | ✘ |
| `wp_presentation`, `wp_commit_timing_v1` | ✔ clock translation | ✘ | ✘ | ✘ |
| `wp_color_manager_v1` (+ ICC fd) | ✔ v2 | ✘ | ✘ | ✘ |
| `wp_security_context_manager_v1` | dropped, but *used* client-side | ✘ | ✘ | ✘ |
| `wp_cursor_shape_manager_v1`, `zwp_idle_inhibit` | pass | ✘ | idle-inhibit ✔ | cursor-shape ✔ |
| `zwp_pointer_constraints` / `relative_pointer` | pass | ✘ | ✔ | ✔ |
| text-input / input-method / virtual-keyboard | ✔ (v2 IM, vkbd) | ✘ | ✔ (+ChromeOS ext) | ✘ |
| tablet / stylus / gamepad | pass | ✘ | ✔ (stylus→tablet-v2, `zcr_gaming_input_v2`) | ✘ |
| `zwlr_screencopy` / `ext-image-copy-capture` | ✔ | ✘ | ✘ | ✘ |
| `zwlr_gamma_control`, `ext-foreign-toplevel-list`, `xdg-toplevel-icon-v1` | ✔ | ✘ | ✘ | ✘ |
| ChromeOS `zaura_shell` / `gtk_shell` | pass | ✘ | ✔ (aura→gtk_shell) | ✘ |
| X11 (Xwayland) | ✔ via `--xwls` + xwayland-satellite | ✔ separate binary | ✔ built-in WM | ✔ built-in WM |
| Unknown future protocols | ✔ pass-through | ✘ must implement | ✘ must implement | ✘ rejected |

Sources: `waypipe/src/wayland_gen.rs:6892-6984` + `waypipe/src/tracking.rs:1521-4428` + `waypipe/protocols/`;
`wprs/src/server/smithay_handlers.rs:1243-1252`; `platform2/vm_tools/sommelier/sommelier.cc:516-802`;
`wayland-proxy-virtwl/src/relay.ml:1351-1369`. `[V]`

---

## 7. Cost model and transport notes

### 7.1 Where the copies are

| Mechanism | Per-frame work (app side) | Wire volume | Per-frame work (display side) |
|---|---|---|---|
| waypipe shm | damage→intervals; 64B-block diff vs mirror; lz4/zstd | changed bytes, compressed | decompress; apply diff to mirror + memfd |
| waypipe dmabuf (diff) | GPU copy of damaged segments → host-visible buffer; diff; compress | changed bytes, compressed | decompress; apply; GPU upload into replica dmabuf |
| waypipe dmabuf (`--video`) | GPU RGB→NV12 compute pass; Vulkan-Video or CPU encode | codec bitstream (`bpf` default 1e5 b/frame) | decode; NV12→RGB compute pass |
| wprs | SoA transpose + DPCM + YUV decorrelation + zstd, sharded | compressed full-surface state | decompress; recreate/attach buffers |
| sommelier (cross-domain) | copy damaged rects, guest RAM → **host-visible blob** | *nothing on a wire* (shared memory) | none |
| sommelier (guest GPU resource) | none | resource id only | none |
| wayland-proxy-virtwl | **full-pool blit** → host-visible blob | *nothing on a wire* | none |

The categorical distinction: over a network/USB link the pixels *must* be serialised, so the cost is
`O(damage) × compression` plus an unavoidable RTT; across a VM boundary with virtio-gpu they need
not be serialised at all, so the cost is at most one guest-side memcpy and at best zero.

### 7.2 Rough magnitudes `[I]`

A single 2048×1152 XRGB8888 window is 9.44 MiB; full-frame at 60 Hz is ≈ 4.75 Gbit/s uncompressed —
above USB 3.2 Gen 1's practical throughput and far above any wireless link. But mode 4's whole point
is that UI workloads never send full frames: a blinking cursor, a scrolled region, or a menu opening
is kilobytes to low megabytes of 64-byte-aligned damage, and lz4 typically halves-to-quarters that
on text-like content (exactly what `waypipe bench`'s two synthetic images model, `waypipe.scd:41-46`).
Video and 3D are the opposite — full-frame damage every frame — where `--video` at `bpf = 1e5`
(≈ 6 Mbit/s at 60 Hz) wins by two-plus orders of magnitude, at the cost of being lossy and
per-buffer-flickery.

**Proxying beats streaming** for text editors, terminals, browsers-while-reading, IDEs, chat,
settings panels — anything where damage ≪ area. It also wins on *crispness* (native resolution
chosen by our compositor, no generation loss), *input* (real Wayland events, no coordinate
remapping or synthetic injection), *latency floor* (one RTT, no encoder pipeline depth), and
*semantics* (subsurfaces, popups, clipboard, DnD, per-toplevel placement).

**Streaming beats proxying** for video playback, games, GPU-heavy full-frame content, and anything
where the *composited* result matters more than window structure. There, mode 2 (encode the finished
frame once) is cheaper and simpler than mode 4 with `--video`, which encodes each buffer of each
surface separately.

### 7.3 waypipe over USB

Waypipe needs one reliable ordered byte stream (§2.8), so USB is a transport-shim problem, not a
waypipe problem. Two realistic options `[I]`:

1. **USB gadget network function → IP link → existing waypipe modes.** Configure the headset as a
   USB device with configfs `ncm.usb0` (CDC-NCM), or ECM/RNDIS for Windows-host compatibility, to
   get a point-to-point IP link on which `waypipe ssh` or `socat`/`ncat` bridging
   (`waypipe.scd:213-239`) works unmodified. postmarketOS already models this as typed device data
   (`usb_network_function`, default `ncm.usb0`, alongside `usb_idVendor/idProduct` in `deviceinfo`,
   [02](02-postmarketos.md) §3), so the descriptor-side plumbing is a solved pattern. Throughput:
   ~150–300 Mbit/s through a USB 2.0 HS gadget NCM stack, ~1–2.5 Gbit/s on USB 3.2 Gen 1. NCM's
   *aggregation* is what gets it there and also what adds buffering latency; tune it down for
   interactive use.
2. **FunctionFS bulk endpoints with a custom framing shim.** One bulk IN + one bulk OUT endpoint,
   with a small userspace relay on each side presenting an `AF_UNIX` socket to waypipe. USB bulk is
   reliable and ordered per endpoint, so the shim only bridges two byte streams — no framing
   invention beyond `wMaxPacketSize` and ZLP conventions. Lower overhead than NCM (no
   Ethernet/IP/TCP headers, no congestion control on a lossless link), lower latency, and a whole
   network stack removed from the attack surface — at the cost of maintaining the shim and its
   host-side libusb counterpart.

**Recommendation `[I]`:** start with CDC-NCM — zero new code, and it reuses `waypipe ssh`'s
authentication; reach for FunctionFS only if measured NCM latency is unacceptable. And note that
**`--vsock` already solves the VM case natively**, so it should be preferred over any
IP-over-virtio arrangement for local microVMs.

---

## 8. Compositor requirements for proxied clients

Verified against what the proxies actually bind. `waypipe client` and `wprsc` are ordinary Wayland
clients of zxr-shell-v2; sommelier and wayland-proxy-virtwl are clients of whatever sits at the far
end of the virtio channel — for us, the crosvm gpu backend's connection to zxr-shell-v2.

| Tier | Global | Version | Why, with citation |
|---|---|---|---|
| **must** | `wl_compositor` | 6 | surfaces, buffer scale/transform, damage_buffer (`tracking.rs:1843-1982`) |
| **must** | `wl_subcompositor` | 1 | toolkit subsurfaces; composition §7.3 already requires full surface trees |
| **must** | `wl_shm` | 1 (2 preferred) | the universal fallback; waypipe replicates the pool as a memfd (`mainloop.rs:1397`); sommelier offers guests only v1 (`sommelier-shm.cc:213`) |
| **must** | `wl_seat` | 7+ (10 available) | pointer/keyboard/touch; we must send a real keymap fd (`tracking.rs:3197`). Versions per `waypipe/protocols/wayland.xml`: `wl_compositor` 6, `wl_shm` 2, `wl_seat` 10, `wl_output` 4, `wl_data_device_manager` 3 |
| **must** | `wl_output` | 4 | toolkits need scale/geometry/name before mapping |
| **must** | `xdg_wm_base` | up to 7 | waypipe compiles xdg-shell v7 (`waypipe/protocols/xdg-shell.xml`) |
| **must** | `wl_data_device_manager` | 3 | clipboard **and** DnD; all four proxies route it (`tracking.rs:3344-3345`) |
| **should** | `zwp_linux_dmabuf_v1` | **4, ideally 5** | waypipe's "light setup" path picks the DRM node from `dmabuf_feedback.main_device` only at v ≥ 4 (`tracking.rs:4370`); below that it guesses (`:4384-4388`). Without it, GPU clients drop to shm and every frame is a CPU diff. Emit a real format table and per-surface tranches — waypipe *rewrites* them against its own device (`:854-965`) |
| **should** | `wp_viewporter` | 1 | sommelier's non-integer scaling depends on it (`sommelier.cc:644-657`); waypipe needs it to compute damage correctly (`tracking.rs:310`, `:2240-2335`); and it is how a proxied window is letterboxed onto a plane without reallocation |
| **should** | `zxdg_decoration_manager_v1` | 1 | an XR shell should force `server_side`; wprs and wayland-proxy-virtwl both implement it (`smithay_handlers.rs:1245`, `relay.ml:1363-1364`) |
| **should** | `wp_fractional_scale_manager_v1` | 1 | a plane in XR has no integer scale; fractional scale + viewporter is how a proxied client renders at the resolution *we* want (`sommelier.cc:776-791`) |
| **should** | `zwp_primary_selection_device_manager_v1`, `zxdg_output_manager_v1` | 1 / 3 | both proxied by waypipe and wayland-proxy-virtwl; toolkits expect them |
| **valuable** | `wp_linux_drm_syncobj_manager_v1` | 1 | waypipe supports it and drops it cleanly if unsupported (`tracking.rs:4266-4308`); zxr-shell-v2 wants timeline sync anyway ([composition §5](../architecture/zxr-shell-v2-composition.md)), so advertising it removes implicit-sync guesswork |
| **valuable** | `wp_presentation`, `wp_commit_timing_v1` | 2 / 1 | waypipe translates timestamps between machine clocks (`tracking.rs:4125-4185`); if advertised we must report a stable `clock_id` and never change it (`:4149-4157`) |
| **valuable** | `wp_security_context_manager_v1` | 1 | **highest-leverage item.** waypipe refuses to *forward* it (`tracking.rs:4198`) but `waypipe client --secctx <app-id>` attaches a context to the socket it gives us (`waypipe/src/secctx.rs:25-35`), letting zxr-shell-v2 label every proxied connection and apply per-origin policy |

**Avoid:** `zwp_linux_explicit_synchronization_v1` (deprecated, blacklisted by waypipe at `:4197`);
`wl_drm`, `zwlr_export_dmabuf_manager_v1`, `wp_drm_lease_device_v1` (all dropped, `:4194-4195`);
`wl_shell` (sommelier still binds it, `sommelier.cc:537`, but nothing modern needs it). Capture
protocols (`zwlr_screencopy`, `ext-image-copy-capture`) are forwarded *fully* by waypipe
(`tracking.rs:3372-4060`), which means **a proxied remote app could screencapture our XR session**
unless they are gated behind a security-context policy. See §11.

**Non-requirement, and this is the payoff:** none of `xdg-decoration`, `fractional-scale`,
`pointer-constraints`, `text-input`, `xdg-activation`, `tearing-control` or `cursor-shape` needs
implementing *for waypipe's sake* — it passes unknown fd-free protocols through untouched
(`waypipe/README.md:181-198`). Whatever we implement for local clients works for proxied ones for
free. `[V]`

---

## 9. Intersection analysis for spatial-os

### 9.1 (a) Remote 2D apps as native planes — mode 4 vs mode 2

Mode 4 delivers precisely what §7.3 of the composition model wants: an ordinary `xdg-shell` client
whose texture we own and whose plane depth we generate. A waypipe-proxied remote app is
*indistinguishable* from a local one at the compositor's surface layer — same subsurface tree, same
`wl_buffer` (shm or a locally-allocated replica dmabuf), same seat events, same
`P_e · V_e · T_window` treatment, same participation in the shared depth test. No capture pipeline,
no encode/decode, no synthetic input, arbitrary 6DoF pose, and the window renders at *our* chosen
scale rather than the remote display's. Mode 2 by contrast fixes the source resolution, puts a lossy
encoder in the loop, requires solving input synthesis, and has no subsurface/popup structure at all —
its only structural advantage is bounded bandwidth under full-frame motion. `[I]`

**Conclusion `[I]`:** mode 4 should be the *default* for remote application windows, with mode 2
reserved for whole-desktop mirroring of machines we cannot install on, and for high-motion content
past §7.2's crossover.

Two caveats. waypipe's per-connection process model means one `waypipe client` per remote app or
session — fine, and it is how per-app security contexts arise (§8). And waypipe's mirror memory is
"moderate" but real (`README.md:209-211`): one full copy of every live shm pool and dmabuf, on
**both** ends. With several 2048²-class planes that is tens of MiB per app on the headset, which
matters on a mobile SoC.

### 9.2 (b) Spectrum-style microVM isolation as a spatial-os security option

Spectrum's architecture maps onto spatial-os cleanly, and its per-VM jailed GPU backend (§5.2) is a
better story than anything in the waypipe world because the *proxy itself* is unprivileged and
confined. The shape `[I]`: zxr-shell-v2 listens on a Wayland socket as usual; each untrusted app
gets a microVM with a virtio-gpu device offering the cross-domain context plus a guest proxy; the
cross-domain backend runs as a separate, unprivileged, sandboxed process per VM holding only our
compositor socket (copy Spectrum's `vhost-user-gpu` run script in shape); and zxr-shell-v2 applies
per-connection policy via `wp_security_context_manager_v1`.

Two honest costs. **The guest-side copy** — both available guest proxies copy shm into a
host-allocated blob (sommelier per damaged rect, `sommelier-compositor.cc:543-716`;
wayland-proxy-virtwl the whole pool, `relay.ml:455`), so sommelier's damage-aware version is the
model to adopt. **No GPU in the guest** unless we also enable virgl/venus context types
(`cross-domain:virgl2`), which Spectrum deliberately does not and which enlarges the host attack
surface considerably (virglrenderer parses guest-controlled command streams).

**A cheaper interim `[I]`:** `waypipe --vsock` needs no virtio-gpu, no rutabaga and no guest proxy
beyond waypipe itself, and already supports host↔guest and sibling-guest topologies
(`waypipe.scd:259-289`). It costs a diff+compress+copy per damaged region versus cross-domain's
shared memory, but it is perhaps two orders of magnitude less integration work and brings dmabuf and
explicit-sync support that wayland-proxy-virtwl lacks. This is the right first implementation of VM
isolation for spatial-os.

### 9.3 (c) Could a zxr-shell-v2 **3D** client be proxied?

**Control plane — free.** Per-frame `P_e·V_e`, model transform, bounds, frame id, target display
time and deadline are a few hundred bytes per frame per client. Waypipe forwards unknown fd-free
protocols verbatim (§2.7), so our entire `zxr-shell-v2` control protocol would proxy *without
waypipe knowing it exists*; frame callbacks and input likewise. `[I]`

**Data plane — does not work over a network or USB link.** Two 2048² eyes at RGBA8 + D32 is ≈ 96 MiB
per app per frame, ≈ 8.4 GiB/s at 90 Hz
([composition §8](../architecture/zxr-shell-v2-composition.md)). Every option fails:

- *Diff path.* A 3D client's colour buffer changes almost everywhere every frame, so damage ≈ area:
  a GPU→CPU copy of essentially the whole image, a full CPU diff and a compress per eye per buffer
  per frame (`mainloop.rs:2899-3020`), then the inverse on the far side. This is less a bandwidth
  problem than a *latency and memory-bandwidth* one — several full-image CPU passes inside an 11 ms
  budget. `[I]`
- *Video path.* Cannot carry depth. Waypipe's video mode converts to NV12 (`video.rs:760-778`) and is
  lossy, and lossy depth destroys the `argmin_i t_i(r)` identity that makes sort-last composition
  *correct* ([composition §2](../architecture/zxr-shell-v2-composition.md)): a few LSBs of depth
  error produce cross-app z-fighting, not graceful degradation. Waypipe also refuses opaque, 10-bit
  and multiplanar formats (`waypipe.scd:134-136`), and D32_SFLOAT is not a video format. `[V/I]`
- *Sync.* drm-syncobj timelines do replicate (§2.5), but acquire/release becomes "signalled once the
  replicated copy has been applied" — the client's release wait now contains a network round trip
  plus a decompress and a GPU upload. `[I]`

**Across a VM boundary the answer is "plausible, with work."** If the guest allocates its colour and
depth images as *virtgpu resources* (virgl or venus), then
`fd_analysis` → `DRM_IOCTL_VIRTGPU_RESOURCE_INFO` → send-as-`VIRTGPU_BLOB` shares them with the host
by reference with **zero copy** (`virtgpu_channel.cc:698-738`), and cross-domain already has a sync
identifier type (`CROSS_DOMAIN_ID_TYPE_VIRTGPU_SYNC`, `virtgpu_cross_domain_protocol.h:30-31`) so
timeline semaphores can in principle cross too. What does **not** exist today: (1) no guest proxy
forwards `zwp_linux_dmabuf_v1` to clients in a Spectrum-like configuration; (2) neither guest proxy
speaks `wp_linux_drm_syncobj_v1`; (3) whatever proxy we use must at least *recognise* our
fd-carrying messages — a whitelist proxy rejects unknown globals outright (`relay.ml:1427`), while
waypipe's pass-through would drop the fds; (4) enabling virgl/venus in guests is a significant host
attack-surface decision. `[V/I]`

**Verdict `[I]`:** proxied 3D clients are a *virtualisation* feature, not a *networking* feature.
Over network/USB, proxy 2D windows (mode 4) and stream 3D as composited video (mode 2). Inside a VM
on the same GPU, a zero-copy 3D client is a credible future project gated on dmabuf + drm-syncobj
forwarding in the guest proxy and a guest GPU context type — both large, both deferred.

---

## 10. What spatial-os adopts / rejects / defers

**Adopt.**

1. **waypipe, unmodified from nixpkgs, as the shipped mechanism for remote 2D applications** — the
   only option with dmabuf, explicit sync, presentation-time, colour management, full clipboard and
   DnD, forward-compatible pass-through, and multiple transports.
2. **The §8 globals list as a hard requirement on zxr-shell-v2**, in particular
   `zwp_linux_dmabuf_v1` **v4+ with real feedback tranches**, `wp_viewporter`,
   `wp_fractional_scale_manager_v1` and `zxdg_decoration_manager_v1` — none optional if proxied
   clients are to look right on a plane.
3. **`wp_security_context_manager_v1`, implemented early.** It is how a proxied remote or VM app
   gets a durable identity in our policy engine, and waypipe already knows how to use it
   (`secctx.rs:25-35`). Gate `zwlr_screencopy`, `ext-image-copy-capture` and `ext-data-control`
   behind it.
4. **`waypipe --vsock` as the first VM-isolation transport** — no virtio-gpu, no rutabaga, no guest
   proxy to maintain, works today, sibling-VM capable.
5. **Spectrum's process-isolation *shape*** for any VM story: unprivileged per-VM backend process,
   `bwrap --unshare-all --disable-userns`, only the compositor socket bind-mounted, VMM under a
   different uid.

**Reject.** (a) **wprs** as a shipped component — version-fragile wire protocol, no dmabuf, narrower
coverage, `0.1.0`; its *idempotent surface-state* design is worth remembering for detachable
sessions. (b) **Writing our own general-purpose network proxy** — waypipe is 39 kLOC of Rust with a
fuzz target, a 5.5 kLOC protocol test harness (`waypipe/src/test_proto.rs`) and cross-arch CI;
reimplementing it rivals zxr-shell-v2 in size. (c) **Proxying 3D clients over network or USB**
(§9.3) — stream composited output instead. (d) **`zwp_linux_explicit_synchronization_v1`** in our
compositor: deprecated, blacklisted by waypipe, superseded by drm-syncobj.

**Defer.** (a) **virtio-gpu cross-domain** — needs crosvm/rutabaga in our image, a guest proxy, and
a guest-GPU-context decision; revisit only if `--vsock` copies prove to be the bottleneck.
(b) **USB transport** — ship CDC-NCM first, evaluate FunctionFS only against measured latency
(§7.3). (c) **Session resumption / detachable XR sessions** — waypipe-rs cannot reconnect at all,
wprs can; if we want "unplug the headset, keep the apps", the wprs model (stateful app-side
compositor, stateless display-side client) is the right starting point. (d) **A spatial-os-specific
guest proxy** for 3D clients, contingent on all four prerequisites in §9.3.

---

## 11. Open questions

1. **Does a security-context policy actually constrain waypipe's forwarded capture protocols?** A
   proxied *remote* client can drive `zwlr_screencopy` / `ext-image-copy-capture` against our
   compositor (`tracking.rs:3372-4060`). Verify end to end that a security-context-gated registry
   filter hides these from proxied connections — a security property, not a nicety.
2. **How much mirror memory does a realistic XR session cost?** waypipe keeps a full mirror of every
   shm pool and dmabuf on *both* ends (`mainloop.rs:533-563`). Measure with 4–6 proxied 2048²
   windows on the target SoC before making mode 4 the default.
3. **Will our dmabuf feedback survive waypipe's rewriting?** It intersects our advertised modifiers
   with its own Vulkan device's (`tracking.rs:854-965`); on a mobile GPU the survivors may be
   linear-only, silently making every proxied GPU client pay a tiling conversion. Needs hardware.
4. **Frame pacing.** waypipe generates no frame callbacks of its own, unlike wprs, which fabricates
   them locally to avoid a network RTT per frame (`wprs/README.md:189-192`). At 90 Hz with a remote
   app one RTT away, does the client stall, or does composition §7.3's "reuse the last texture, re-project the
   plane every frame" rule already absorb it?
5. **Clock translation vs. XR time.** waypipe rewrites `wp_presentation` timestamps between machines
   (`tracking.rs:4125-4185`); our frame clock is `xrWaitFrame`'s predicted display time. What is the
   presentation time of a plane whose texture is three frames old?
6. **Input latency budget.** Mode 4's floor is one RTT: ~1–2 ms over USB-NCM, 5–30 ms over Wi-Fi.
   Where does a proxied window stop feeling attached to the hand ray, and does that argue for local
   prediction of hover feedback?
7. **Does `--title-prefix`-style rewriting generalise?** waypipe demonstrates safe in-place rewriting
   for equal-or-shorter payloads (`tracking.rs:4061-4075`). Should we rewrite `xdg_toplevel` app-ids
   to carry provenance for spatial placement, or take that from the security-context app-id instead?
8. **Whitelist or pass-through for a future spatial-os guest proxy?** wayland-proxy-virtwl's
   whitelist (`relay.ml:1427`) fails closed and is auditable; waypipe's pass-through is
   forward-compatible but forwards protocols nobody reviewed. For an isolation boundary fail-closed
   is probably right — which argues *against* reusing waypipe unchanged as the VM-boundary proxy
   even though it is the correct choice for the network case.
