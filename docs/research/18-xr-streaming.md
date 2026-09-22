# 18 — XR/game streaming engines: WiVRn, ALVR, Sunshine/Moonlight, wolf, Monado comp_multi

**Date:** 2026-09-22. Research pass for spatial-os spatial sharing. This document studies the
*network protocols and streaming pipelines* of the mature open XR/game streamers, as engine
candidates for the sharing modes of
[zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md): mode 3 (per-observer
3D streaming: a shared app renders extra views for remote observers *inside the same atomic
colour+depth frame*) and the general video-transport question for modes 1–2 (flat/stereo video
mirroring). It deliberately does **not** cover Wayland protocol proxying (sibling doc 19), and it
does not repeat the WiVRn *packaging* study ([05-xr-userspace.md](05-xr-userspace.md) §2.2) — this
is the protocol half that study deferred.

Primary sources (local clones; paths relative to `references/`):

- `wivrn/` (GPL-3, C++23, commit 6f9e146 2026-09-18 — active) — Monado-derived server streaming
  per-view video to a headset client with pose feedback. **The priority subject.**
- `alvr/` (MIT, Rust, SteamVR-driver architecture) — in-repo wiki, esp. `wiki/How-ALVR-works.md`.
- `sunshine/` (GPL-3, C++) — Moonlight-protocol game-streaming server.
- `wolf/` (games-on-whales, C++/Rust) — Moonlight server, one headless Wayland compositor per app.
- `monado/src/xrt/compositor/multi/` — the local multi-client compositor (brief).

## 1. Purpose and scope

The zxr-shell-v2 composition model ends at the local GPU: dmabuf colour+depth pools, atomic
per-frame submission, one composed projection layer to Monado. Sharing a 3D app with a *remote*
observer breaks the dmabuf assumption: buffers must become packets. The questions for each engine:

1. What is the wire protocol (channels, reliability, framing, versioning)?
2. How do poses flow *to* the renderer and frames flow *back*, and what closes the timing loop?
3. What is the encoder abstraction; what does per-view/foveated/alpha encoding look like?
4. What happens on packet loss and late frames?
5. What does it *not* carry that mode 3 needs — above all **depth** and **multi-observer**?

Terminology: "server" = the machine rendering (our compositor/apps), "client" = the viewing
headset, matching WiVRn/ALVR usage.

## 2. WiVRn protocol deep dive

WiVRn is architecturally the closest existing system to our problem: its server *is* a Monado
(pinned via `monado-rev` + 11 patches, per 05 §2.2) whose compositor backend, instead of scanning
out to a panel, squashes client layers, foveates, encodes, and ships packets; the headset client is
a native OpenXR app on the vendor runtime that decodes and resubmits. One process boundary in the
middle of an OpenXR runtime — exactly where a per-observer stream would sit in zxr-shell-v2.

### 2.1 Connection, handshake, version lock

- Discovery is mDNS/avahi (`server/avahi_publisher.cpp`); the client connects a **TCP control
  socket** to the configured port (`server/accept_connection.cpp:36` — a single `TCPListener`; the
  server tells Monado `headset_connected`/`headset_disconnected` over an internal IPC socket, i.e.
  it is structurally **one headset client per server instance**).
- The client opens with `from_headset::crypto_handshake{protocol_version, public_key, name}`
  (`client/wivrn_client.cpp:111`). The version is not a number to negotiate: it is a **structural
  hash of the entire packet variant** — `serialization_type_hash<std::variant<from_headset::packets,
  to_headset::packets>>(protocol_revision)` (`common/protocol_version.h`). Any change to any field
  of any packet changes the hash; mismatch ⇒ `crypto_state::incompatible_version` and disconnect
  (`client/wivrn_client.cpp:196`). Brutal but honest: no cross-version compatibility, ever, which
  is why the dashboard installs the exact-commit APK (05 §2.2).
- Pairing: server replies with its key and a state; if `pin_needed`, a 4-message **socialist
  millionaire protocol** exchange (`pin_check_1..4`, `common/smp.cpp`) verifies a 6-digit PIN
  without transmitting it; then both sockets get AES keys derived from the ECDH secret + PIN
  (`common/secrets.cpp`; `TCP::set_aes_key_and_ivs`, `UDP` counter-IV encryption in
  `common/wivrn_sockets.h:96-131`).
- Server then sends `to_headset::handshake{stream_port}`; the client connects a **UDP stream
  socket** to that port and sends `from_headset::handshake` on it, repeated until the server
  answers with a second handshake — a hole-punching/liveness loop (`client/wivrn_client.cpp:202-221`).
  `stream_port = -1` ⇒ TCP-only mode: `send_stream()` transparently falls back to the control
  socket (`server/driver/wivrn_connection.h:94-112`).

### 2.2 Wire protocol: two typed channels, one serialization

All packets of both directions are defined in one header, `common/wivrn_packets.h`, as plain
structs in `namespace from_headset` / `to_headset`, each direction closed under a `std::variant`
(`from_headset::packets` ~25 alternatives, `to_headset::packets` ~17). A `typed_socket<Socket,
Recv, Send>` template (`common/wivrn_sockets.h:207`) prefixes each packet with the variant index
(1 byte) and uses reflection-style serialization (`common/wivrn_serialization.h`). There is no
protobuf/capnp; the hash-versioning above is what makes this safe.

Channel split in practice (`server/driver/wivrn_connection.h:52-53`):

- **Control (TCP, reliable, ordered):** handshake/pairing, `headset_info_packet` (resolutions,
  refresh rates, codec list, audio formats, tracking capabilities), `settings_changed`,
  `video_stream_description`, `audio_stream_description`, application list/launch (server-side app
  management), `tracking_control`, visibility masks, session state — and, notably, **audio data**
  (`server/audio/audio_pipewire.cpp:394` sends `audio_data` via `send_control`; timestamped raw
  PCM, no audio codec on the wire).
- **Stream (UDP, encrypted, unordered):** `tracking`, `hand_tracking`, body/face packets, `inputs`,
  `feedback`, `timesync_*`, `battery` upstream; `video_stream_data_shard`, `haptics` downstream.
  Both peers `poll()` both sockets in one loop and dispatch through the same visitor
  (`server/driver/wivrn_connection.h:121-171`).

### 2.3 The video stream: shards, view_info, three streams

`to_headset::video_stream_description` (`common/wivrn_packets.h:804-816`) fixes per-eye
`width`/`height`, `frame_rate`, and a **codec per stream item**: `std::array<video_codec,3> codec;
// left, right, alpha`. So a WiVRn "frame" is up to **three independent video streams**: left eye,
right eye, and an optional half-resolution alpha stream for passthrough cutouts, each with its own
encoder instance and its own decoder on the client.

`to_headset::video_stream_data_shard` (`common/wivrn_packets.h:817-859`) is the entire video wire
format:

- `max_payload_size = 1400` bytes (MTU-safe), `stream_item_idx` (0/1/2), `frame_idx` (u64
  monotonic), `shard_idx` (u16 within frame).
- The **first** shard of a frame carries `view_info_t`: `display_time` (in *headset* clock),
  per-eye `pose` + `fov` **the frame was actually rendered with**, per-eye `foveation_parameter`,
  and the alpha flag. This is the atom that makes reprojection possible: colour pixels never travel
  without the camera that produced them.
- The **last** shard carries `timing_info_t{encode_begin, encode_end, send_begin, send_end}` —
  server-side timestamps handed to the client so its `feedback` can reconstruct the full pipeline
  timeline.

There is **no FEC and no retransmission**: a frame with a missing shard is simply never submitted
to the decoder (see §2.7).

### 2.4 Pose flow client→server and prediction

The server does not passively receive "current pose"; it *programs* the client's sampling:

- Server side, each `xrt_device::get_tracked_pose(at_ns)` call from Monado's compositor/apps
  registers the requested prediction horizon in `tracking_control::add_request`
  (`server/driver/tracking_control.cpp:34`), and once per period `resolve()` compiles a **sampling
  pattern**: per device, a range `[min_prediction, max_prediction]` (clamped to
  `max_extrapolation_ns`, stepped at 3 ms for head/grip/aim/gaze) plus a measured
  `motions_to_photons`, sent as `to_headset::tracking_control` (`tracking_control.cpp:58-127`).
- Client side, a dedicated tracking thread walks that pattern each display period: it sorts entries
  by phase (`client/scenes/stream_tracking.cpp:374`), sleeps to the right sub-frame offset, calls
  `xrLocateViews`/`xrLocateSpace` at `t0 + prediction_ns`, and sends `from_headset::tracking`
  packets over UDP — each containing `production_timestamp` (when sampled) *and* `timestamp` (the
  time the pose is *for*), both eye views (pose+fov relative to VIEW space) and all device poses
  with validity flags (`common/wivrn_packets.h:374-439`).
- Server side, every device keeps a time-indexed history: `pose_list` stores samples and serves
  `get_at(at_timestamp_ns)` by interpolating between the two nearest samples or extrapolating
  polynomially beyond the newest (`server/driver/pose_list.cpp:151-241`,
  `server/driver/history.h:75-108`, `polynomial_interpolator.h` — degree-3 position, degree-4
  quaternion). So Monado's normal "give me the pose at predicted display time" call is answered
  from a *pre-predicted sample stream*, not a round trip.

Clock domains are explicitly separated: all headset timestamps are converted through
`clock_offset` (headset_time = server_time + b), maintained by a continuous
`timesync_query`/`timesync_response` ping over UDP with an offset estimator
(`server/driver/clock_offset.h:34-75`, initial sample interval 10 ms). This is the precedent for
our "clock-domain separation" requirement: nothing on the wire is ever in the *other* side's clock
without conversion at the edge.

### 2.5 Frame pacing: a phase-locked loop on real feedback

Two pacers cooperate:

**App pacer** (`server/driver/app_pacer.cpp`, a `u_pacing_app` given to Monado): predicts per-app
wake-up so that `cpu_time + gpu_time + compositor_time + margin` lands exactly on the next
compositor display time; app cpu/gpu costs are EWMA-learned from mark points (`mark_gpu_done`,
lerp 0.1, `app_pacer.cpp:212-221`). Display times are snapped to the compositor's phase
(`predict`, `app_pacer.cpp:126-161`).

**Stream pacer** (`server/compositor/pacer.cpp`) replaces the display in Monado's frame loop and is
the part worth copying wholesale. For each frame `predict()` computes
(`pacer.cpp:96-126`):

```
predicted_client_render = last_ns + frame_duration        # next tick of a virtual client vsync
desired_present  = predicted_client_render - safe_present_to_decoded
wake_up          = desired_present - mean_wake_up_to_present + margin
predicted_display = predicted_client_render + mean_render_to_display
```

where the three learned quantities come from the client's `from_headset::feedback` packet — a
**13-timestamp trace per frame per stream** (`common/wivrn_packets.h:612-630`: encode begin/end,
send begin/end, first/last packet received, sent to/received from decoder, blitted, displayed,
`times_displayed`):

- `safe_present_to_decoded` = **99.5th percentile** of (decoded − present) over a 5000-frame
  window + configurable client margin, computed on a worker thread (`pacer.cpp:49-70`). Pacing to a
  high quantile, not the mean, is the tail-latency insight.
- The virtual client vsync is **phase-locked** to reality: each feedback nudges `last_ns` by 1/10 of
  the phase error between predicted and actual client blit time (`pacer.cpp:155-162`) — a software
  PLL on the remote compositor's clock.
- `mean_render_to_display` EWMA from `displayed − blitted` (`pacer.cpp:164-165`).

Result: the server renders *as late as possible* such that the frame finishes decoding just before
the client's compositor wants it — motion-to-photon is minimized without a fixed latency budget.

### 2.6 Encoder abstraction

`video_encoder` (`server/encoder/video_encoder.h`) is a small, clean interface: constructed from
`encoder_settings{width, height, codec, fps, encoder_name, bitrate, options, device}`; subclasses
implement `present_image(vk::Image y_cbcr, sem, slot, frame_index)` (record GPU work) and
`encode(slot, frame_index) -> data` (produce a bitstream span). Implementations: **nvenc**
(`video_encoder_nvenc.cpp`), **vaapi via ffmpeg** (`ffmpeg/video_encoder_va.cpp`), **Vulkan video**
(`video_encoder_vulkan_h264/h265.cpp`), **x264** software, and a **raw** debug encoder. Double
buffering via 2 slots with atomic idle/busy/skip states — if the encoder is still busy when the
next frame presents, the frame is *skipped for that stream*, never queued (`video_encoder.h:89-95`).
A shared sender thread shards the bitstream into ≤1400-byte `video_stream_data_shard`s and pushes
them out (`SendData`, with `prefer_control` to route certain data over TCP).

Bitrate is a single global number (client-requested via `settings_changed.bitrate_bps`) split
across the three encoders by pixel-count weight, with the alpha stream weighted at 5%
(`encoder_settings.cpp:45-73`). There is **no congestion control**: the client (a human in the GUI
or the auto default) picks the bitrate; `idr_handler` (see §2.7) is the only network-adaptive
mechanism. Dimensions are 64-aligned and clamped per encoder capability
(`encoder_settings.cpp:293-304`).

Upstream of the encoders sits the WiVRn compositor (`server/compositor/compositor.h`, a Monado
`comp_base` backend): per frame it (1) **squashes** all client layers — projection,
projection+**depth**, quad, cylinder, equirect2 — into one stereo target via a compute pipeline
(`layer_squasher.cpp:453-560`; depth layers bind a second sampler, `layer_squasher.cpp:683-687`,
used to reproject that layer's colour to the newest pose — the depth is *consumed here* and never
leaves the machine); (2) applies **foveation**: a piecewise per-axis scaling driven by eye gaze /
fixed center, encoded as run-length `foveation_parameter{x[], y[]}` arrays sent in every frame's
`view_info` (`common/wivrn_packets.h:777-791`, `server/compositor/foveation.cpp`); (3) hands the
foveated y_cbcr images to the three encoders.

### 2.7 Client: reassembly, reprojection, loss handling

- **Reassembly:** `shard_accumulator` keeps exactly two in-flight frames (`current`, `next`). Late
  shards for older frames are dropped; jumping ≥2 frames ahead abandons `current` (sending its
  feedback so the server sees the loss) and resets (`client/decoder/shard_accumulator.cpp:112-160`).
  Contiguous shard runs are fed to the decoder *before* the frame completes (streaming decode,
  `try_submit_frame`, `:168-190`); a frame missing any shard is **never displayed** — no FEC, no
  NACK-retransmit.
- **Loss recovery** is codec-level: the server's `default_idr_handler` watches feedback; any lost
  frame triggers an IDR request and frames are skipped until the IDR is acknowledged
  (`server/encoder/idr_handler.h:39-58`).
- **Frame selection:** per vsync, `common_frame(predictedDisplayTime)` finds a frame index present
  in *all* active decoders whose `view_info.display_time` is nearest the target; only if no common
  index exists does it fall back to mismatched per-stream latest frames with a warning
  (`client/scenes/stream.cpp:648-730`) — i.e. even with independent left/right streams the client
  *re-establishes atomicity by frame index* before display. Left/right pairing is best-effort
  re-synchronized, which is exactly the class of hazard our atomic-submission rule ("never pair the
  newest colour with the newest depth") exists to kill; WiVRn needs this dance only because it
  split one logical frame into independent streams.
- **Reprojection (timewarp):** the client does *not* run its own warp shader. It defoveates into an
  OpenXR swapchain (`stream_defoveator.cpp`) and submits a projection layer whose per-view
  `pose`/`fov` are **the server's render pose from `view_info`** (`stream.cpp:1095-1117`); the
  headset vendor's runtime compositor then timewarps server-pose→current-pose as it would for any
  late local app. Late/duplicate frames: the last frame is re-submitted (runtime re-warps it,
  `times_displayed` counts up, `stream.cpp:979-982`); if nothing arrives for 1 s the scene drops to
  a `stalled` state and returns to the lobby (`stream.cpp:915-917`). This is WiVRn's honest version
  of our T3 rule: reuse-under-motion is delegated to a component that declares it (the runtime's
  ATW), never silently depth-tested.
- **Audio:** raw timestamped PCM over TCP both ways (speaker + mic), descriptions negotiated in the
  handshake; pipewire on the server (`server/audio/audio_pipewire.cpp`). No compression;
  loss-handling is inherited from TCP; latency is managed by buffer sizing on the client.

### 2.8 What WiVRn does *not* do (the mode-3 gap list)

Enumerated precisely, since this is the closest system to "per-observer RGBD stream":

1. **No depth on the wire.** Depth exists in the pipeline (apps may submit
   `XRT_LAYER_PROJECTION_DEPTH`; the squasher samples it) but it is consumed server-side for
   layer reprojection. The three wire streams are colour, colour, alpha
   (`video_stream_description::codec`); there is no depth stream item, no depth codec, no depth
   metadata (near/far/encoding) in `view_info_t`.
2. **One observer.** One TCP listener, one connection object, one clock offset, one pacer, one
   foveation state, one settings/bitrate. Nothing is per-client. (Multiple *apps* on the server are
   fine — that's Monado comp_multi + the layer squasher, §6 — but they collapse to one squashed
   view for one headset.)
3. **The observer is the head.** The streamed views are the headset's own eyes; poses flow from the
   viewer to the renderer. A spatial-os remote observer is the same loop, but the renderer is a
   *shared app* rendering an additional view — WiVRn has no notion of a view that isn't the local
   HMD.
4. **No per-app streams.** Everything is squashed pre-encode; a remote observer could never occlude
   its own local content against ours because inter-app depth is resolved before encoding.
5. **Frame atomicity is reconstructed, not guaranteed** (frame-index matching across independent
   decoders, §2.7).
6. **No congestion control / FEC** — acceptable on the LAN it targets; a WAN observer would need
   more (see §4).

## 3. ALVR latency design

ALVR (SteamVR driver + Rust core) is protocol-wise similar — TCP control socket, UDP-or-TCP stream
socket, packets sharded to MTU with per-stream sequence numbers, no FEC (dropped shards ⇒ frame
dropped ⇒ IDR request; `alvr/sockets/src/stream_socket/mod.rs`, `wiki/How-ALVR-works.md`
"Video transcoding"). Video frames carry `VideoPacketHeader{timestamp, global_view_params: [ViewParams;2],
foveation_center_shifts, is_idr}` (`alvr/packets/src/lib.rs:242`) — same
pose-travels-with-pixels principle as WiVRn's `view_info`. Differences that matter:

- **Prediction is client-computed from measured pipeline latency.** The client maintains
  `average_total_pipeline_latency` (tracking-sent → frame-displayed, a `SlidingWindowAverage`) and
  polls its runtime that far into the future, clamped by `max_prediction`
  (`alvr/client_core/src/lib.rs:244-251`, `server_core/src/statistics.rs:177-284`). One scalar
  horizon, versus WiVRn's server-compiled per-device sampling *pattern* + server-side
  interpolation/extrapolation history. WiVRn's is finer-grained; ALVR's is simpler and
  self-tuning end-to-end.
- **"Phase sync" (in-repo design, `wiki/How-ALVR-works.md` §Upcoming):** their generalization —
  every recurring event (frame submission, tracking submission, poll timing) gets a queue plus a
  statistical model targeting a deadline *with variance awareness*, explicitly tunable
  mean-vs-variance. WiVRn's 99.5th-percentile decode pacing is a concrete instance of exactly this;
  ALVR names the pattern and plans it in three places (client submit, SteamVR tracking latch,
  server poll). The articulation worth stealing: **each queue in the pipeline is a separately
  phase-controlled clock domain**, not one global latency number.
- **Adaptive bitrate exists** (unlike WiVRn): `BitrateManager` records per-frame network latency,
  encoder latency, packet sizes; in `NetworkAdaptive` mode the bitrate follows measured throughput
  with headroom multipliers, and a decoder-latency overstep counter learns a max-bytes-per-frame
  bound to stop decoder runaway (`alvr/server_core/src/bitrate.rs`). This is the missing piece of
  WiVRn's transport, at ~300 lines.
- **Sliced encoding** (planned, same wiki section): split frames into slices encoded/sent/decoded
  in parallel to overlap pipeline stages. Relevant to our large per-observer frames.
- Architecture caveat: ALVR must *reverse-engineer* frame↔pose association out of SteamVR
  (libunwind stack inspection on Linux, `wiki/How-ALVR-works.md` §SteamVR driver) because OpenVR
  hides the compositor. WiVRn, owning the Monado compositor, gets this for free — a strong argument
  for the "extend the compositor we own" shape in §7.

## 4. Sunshine/Moonlight transport

Sunshine implements NVIDIA's GameStream protocol as an open server; Moonlight clients are
ubiquitous (including a Quest port). It is the best-engineered open *colour* transport:

- **Session bring-up:** HTTPS/HTTP pairing (`nvhttp.cpp`), then an **RTSP** exchange negotiates the
  session (`rtsp.cpp`, port base+21), then three runtime channels (`stream.h:19-21`):
  **control** = ENet (reliable-UDP, AES-GCM encrypted messages) on base+10, **video** = raw UDP RTP
  on base+9, **audio** = UDP RTP on base+11 (`stream.cpp:1961-2009`).
- **FEC everywhere:** video frames are split into ≤`blocksize` shards and expanded with
  **Reed-Solomon parity** per block — `fec_percentage` (default config, clamped 1-255%) with a
  minimum parity-shard floor (`stream.cpp:791-913`); audio likewise carries RS parity shards
  (`audio_fec_packet_t`, `stream.cpp:311-533`). The client reconstructs lost shards without
  round-trips — the right call for isochronous media on WAN/wifi.
- **Recovery beyond FEC** is receiver-driven via control messages: `IDX_REQUEST_IDR_FRAME` and —
  smarter — `IDX_INVALIDATE_REF_FRAMES` (`stream.cpp:42-48,1193-1209`), which tells the encoder
  "frames N..M are lost, don't reference them" so it can re-reference an older *acknowledged* frame
  instead of paying for a full IDR (reference frame invalidation; needs encoder support).
- **Bitrate/pacing:** the *client* chooses bitrate at session start; Sunshine configures the encoder
  with `rc_max_rate = bit_rate`, VBV buffer sized to a single frame (`video.cpp:2214-2242`) so no
  frame greatly overshoots its slot — burst control by encoder configuration rather than transport
  feedback. Frame pacing is capture-driven (frames sent as produced at the negotiated fps); display
  smoothness is the client's job (Moonlight offers pacing modes). No pose loop exists at all —
  latency is minimized but never *predicted against*; motion-to-photon is out of scope.
- **Encoders:** the widest matrix of the group — NVENC (native), VAAPI/QSV/AMF/VideoToolbox/x264/
  x265/SVT-AV1 via FFmpeg, H.264/HEVC/AV1, HDR10 — with runtime capability probing (`video.cpp`).
- **Input path:** Moonlight sends keyboard/mouse/gamepad over the control channel; Sunshine injects
  via uinput/evdev on Linux (`input.cpp`, `platform/linux/`).

As a benchmark: Sunshine's transport (RTP + RS-FEC + ref-invalidation + client-owned bitrate) is
what a production mode-1/2 colour stream looks like; WiVRn/ALVR's pose-feedback loop is what it
lacks.

## 5. wolf: per-app headless compositors

wolf reimplements the Moonlight *server* with a different execution model
(`docs/modules/dev/pages/how-it-works.adoc`):

- **One micro Wayland compositor per session/app**: `gst-wayland-display` (Smithay, Rust) is
  instantiated per stream session (`src/moonlight-server/sessions/moonlight.cpp:88-100` "Create
  wayland compositor" on each new `StreamSession`); it has no scanout — its output *is* a GStreamer
  source handing raw framebuffers to the encoding pipeline. No XWayland; apps needing X11 run
  gamescope *inside* the session.
- **Isolation:** each app runs in its own container (`runners/docker.cpp`), sees only its own
  compositor socket (mounted into the container, `sessions/common.cpp:40-44`), its own PulseAudio
  sink, and its own **virtual input devices** created by inputtino (uinput/uhid), with **fake-udev**
  injecting synthetic udev events into the container so hotplugged virtual gamepads appear
  (`src/fake-udev/`, `docs/modules/dev/pages/fake-udev.adoc`). The app cannot see the host session
  or sibling apps at all.
- **Transport** is the Moonlight protocol again — custom GStreamer plugins produce
  Moonlight-flavoured RTP with FEC (`src/moonlight-server/gst-plugin/`,
  `docs/modules/protocols/pages/*.adoc`), RTSP/ENet as in §4.
- **Multi-observer of one app already works** in their "lobbies" design: one compositor's raw
  framebuffer fans out through `interpipe` to *N per-client pipelines*, each with its own
  scaling + codec + bitrate (`docs/modules/dev/pages/lobbies.adoc` diagram; `sessions/lobbies.cpp`
  switches input devices between wayland displays). That is per-observer *encode* budgets over a
  shared *render* — the 2D analog of our mode-3 requirement, minus per-observer viewpoints.

For spatial-os this is a pattern, not an engine: **per-app capture groups**. A "sharable app" can
be given its own private compositor endpoint whose output feeds N observer pipelines; session
lifecycle, input isolation, and fan-out live outside the app. It also anticipates sibling doc 19's
proxying question: wolf's answer to "how does a remote session see one app" is "give the app a
dedicated compositor," not "proxy the protocol."

## 6. Monado comp_multi (brief)

`src/xrt/compositor/multi/` is the *local* version of the N-renderers-one-display problem, and the
contracts it chose are instructive (`comp_multi_private.h`):

- Per client (up to 64), a `multi_compositor` holds **three whole-frame slots** — `progress`
  (client still submitting), `scheduled` (complete, for a future display time), `delivered` (ready
  for the render loop) — each an atomic `multi_layer_slot` containing *all* layers of one frame
  plus their sync data (`comp_multi_private.h:81-192`). Layers advance slot-by-slot only as
  complete sets: **atomic frame grouping, per client**, the same invariant as our §7.2 client
  contract.
- Each client gets its own `u_pacing_app` (predict/wake/deliver — the interface WiVRn's `app_pacer`
  implements), so heterogeneous app rates never block the display loop; the system render loop
  simply takes whatever is `delivered` at composite time and re-submits stale slots otherwise
  (`comp_multi_system.c:274-294` — deliver-any, retire-if-stale).
- The system compositor concatenates all visible clients' delivered layers by z-order into one
  layer array and commits it downstream (`comp_multi_system.c:314-358,594`); GPU sync is resolved
  before a slot becomes `delivered` (wait thread per client), so nothing unsignaled ever enters the
  critical path — our §7.4 scheduling rule, already proven here.

zxr-shell-v2 deliberately replaces the *composition semantics* (painter's-algorithm layer stack →
depth-resolved merge) but should keep this *scheduling shape*: per-source pacer + triple slot +
compose-only-delivered. A remote observer bridge is then "a `multi_compositor` whose delivered slot
is fed by a network receiver instead of an IPC client."

## 7. Gap analysis: per-observer RGBD groups vs what exists

What mode 3 needs on the wire, against the survey:

| Requirement | WiVRn | ALVR | Sunshine/wolf | comp_multi (local) |
|---|---|---|---|---|
| Pose feedback loop, predicted display time | **yes** (§2.4-2.5) | yes (§3) | no | n/a (local) |
| Pose/metadata travels with pixels | yes (`view_info`) | yes (`VideoPacketHeader`) | no | yes (layer data) |
| Depth channel + depth encoding metadata | **no** | no | no | local dmabuf only |
| Atomic colour+depth+descriptor group | reconstructed by frame idx | single stream | n/a | **yes** (slots) |
| Multi-observer, per-observer budgets | no | no | wolf lobbies (2D) | 64 clients (local) |
| Validity masks / partial-frame semantics | no (whole frame or nothing) | no | no | no |
| Clock-domain separation | **yes** (`clock_offset`) | yes (statistics keyed on timestamps) | no | single clock |
| WAN-grade loss handling (FEC, ref-invalidation) | no | no | **yes** | n/a |
| Congestion/bitrate adaptation | no | **yes** (`BitrateManager`) | client-set + VBV | n/a |

The genuinely novel work (no existing engine has it) is exactly the intersection with our §2/§7.2
composition contract:

1. **Atomic RGBD group framing.** One logical unit = {colour left/right, depth left/right, view
   descriptors (P·V per view, depth encoding near/far/reversed-Z, bounds), frame id, display time}.
   WiVRn's `view_info`-on-first-shard is the template, but extended to bind *multiple buffers* to
   one frame id with the rule that an observer's compositor may only consume complete groups —
   comp_multi's slot semantics, serialized. Never let receivers pair newest-colour with
   newest-depth.
2. **A depth codec.** Colour codecs are solved (reuse); depth is not. Options to evaluate:
   (a) lossless tiles (e.g. zstd-compressed quantized tiles — depth compresses well, and T1
   correctness argues for lossless or error-bounded); (b) abusing a video codec on packed depth
   (HEVC main-12 on quantized inverse depth — lossy blocking artifacts become *geometry* errors at
   silhouettes, the known failure mode); (c) mesh/quadtree approximations (out of contract). The
   honest default is (a) with per-tile quantization ranges, at ~2-4× the colour bitrate cost,
   cropped by bounds and validity.
3. **Validity masks.** A per-view coverage mask (app rendered only inside its bounds/cropped
   region) so the observer's depth test ignores non-samples; none of the surveyed wire formats has
   this because they always ship full rectangles. Cheap: 1-bit mask, RLE, inside the group.
4. **Per-observer view budgets.** The shared app renders one extra view set per observer within its
   atomic frame (§ per-observer 3D sharing); the *transport* must let each observer negotiate
   resolution/foveation/bitrate independently (wolf's per-client pipelines show the fan-out shape;
   WiVRn's foveation params are already per-frame wire data, reusable per observer).
5. **Clock-domain separation** per observer: WiVRn's `clock_offset` + `tracking_control` pattern,
   instantiated N times.

**Reuse vs build:**

- **Reuse WiVRn's shapes wholesale** (and, where licensing/engineering permits, its code — the
  server is GPL-3 like wxrc; our compositor links Monado already): `typed_socket` + variant + hash
  versioning; control/stream split with TCP fallback; `video_encoder` abstraction (nvenc/vaapi/
  vulkan-video/x264 behind 2 virtuals); the *pacer* (§2.5 phase-locked loop with quantile decode
  budget); `tracking_control` sampling patterns + `pose_list` histories; `clock_offset`.
- **Reuse Moonlight-style transport hardening selectively:** RS-FEC per block and
  reference-frame invalidation are the two features to add for non-LAN observers; ALVR's
  `BitrateManager` is the adaptation logic to port. We do **not** adopt the Moonlight protocol
  itself (no pose loop, no multi-buffer frames — retrofitting costs more than it saves).
- **Build new:** the RGBD group framing, depth codec, validity masks, per-observer session/budget
  management, and the observer-side depth-composited ingest (the remote frame enters the observer's
  compositor as a first-class colour+depth client per §2 of the composition note — *not* as a
  flat quad).
- **Extend WiVRn itself vs a bridge:** extending WiVRn's client/server directly is tempting
  (encoder + pacer live there) but wrong-shaped: WiVRn's endpoints are "whole squashed session ↔
  one HMD." Our unit is "one shared app's observer view-group ↔ one observer compositor," N-way,
  originating *inside* zxr-shell-v2 where the atomic submissions already exist pre-squash. The
  right cut: a **bridge component inside zxr-shell-v2** (egress: taps a shared app's per-observer
  view sets; ingress: presents a remote group as a local client) that *vendors WiVRn's
  encoder/pacer/clock modules* rather than forking its session model. WiVRn remains untouched as
  the whole-desktop-to-headset path (mode 0/2 for our own HMD, already packaged per 05 §2.2).

## 8. What spatial-os adopts / rejects / defers

**Adopt**

- WiVRn's two-channel typed-packet protocol shape, structural-hash version lock, and
  metadata-travels-with-pixels rule (§2.2-2.3) for the observer bridge.
- WiVRn's timing loop verbatim: rich per-frame feedback timestamps, quantile-based
  present-to-decoded budget, phase-locked virtual vsync, server-programmed tracking patterns with
  interpolation histories, explicit clock-offset conversion (§2.4-2.5).
- WiVRn's `video_encoder` abstraction as the colour-encoder layer; per-frame foveation parameters
  as the per-observer scaling mechanism (§2.6).
- Observer-side reprojection *by the observer's own compositor* against the received render pose —
  the remote group participates in T1 composition with declared staleness; like WiVRn's client we
  delegate warp to the component that owns the display, never silently reuse stale depth (§2.7, T3
  rule).
- comp_multi's per-source pacer + slot lifecycle as the ingest scheduling model (§6).
- wolf's per-app capture-group pattern (dedicated compositor endpoint per shared app; per-observer
  fan-out pipelines) as the session/isolation architecture (§5).
- Sunshine-grade transport hardening (RS-FEC, reference-frame invalidation) and ALVR's
  bitrate adaptation, as the WAN profile of the bridge (§3-4).

**Reject**

- The Moonlight protocol as the base transport (no pose loop, single-buffer frames).
- ALVR's SteamVR-style interposition (we own the compositor; no frame↔pose archaeology).
- Squash-before-encode for mode 3 (destroys per-app depth; WiVRn's model is correct for mode 0/2
  only).
- Lossy video coding of depth as the default (silhouette geometry errors violate T1 honesty);
  revisit only with error bounds as an explicit negotiated profile.
- Independent per-buffer streams with receiver-side re-pairing (WiVRn's left/right frame-index
  dance): group framing is mandatory from day one.

**Defer**

- The concrete depth codec bake-off (lossless tiles vs 12-bit inverse-depth video vs hybrids) —
  needs measurements on real app depth buffers; blocks nothing in the protocol design if the group
  framing carries a negotiated depth-encoding id.
- Multi-observer congestion coordination (N observers × RGBD on one uplink; §8 bandwidth math of
  the composition note says a single uncompressed 2048² RGBD eye-pair is ~96 MiB — per-observer
  budgets are not optional, but the arbitration policy can come later).
- T2 (ordered-sample) payloads over the bridge; the group framing must merely not preclude an
  extra buffer class.
- Audio for shared apps (WiVRn's PCM-over-TCP is fine locally; per-app audio capture belongs to
  the wolf-style session layer).

## 9. Open questions

1. **Depth bitrate reality check:** what do real depth buffers (raster, ray-march, splat) cost
   under (a) zstd lossless tiles, (b) HEVC-12 inverse depth, at 2048²@90 vs foveated/cropped sizes?
   Is error-bounded lossy acceptable for T1 at silhouettes?
2. **Where exactly does the egress tap sit** — does the shared app render observer views into
   dedicated pool slots (extra views in the §7.2 contract, compositor-scheduled per observer
   pacing), or does the bridge re-request frames at observer phase? (The former keeps atomicity
   for free; the latter decouples rates. Likely: extra views, with per-observer `fps_divider`.)
3. **Observer pose authority:** observer sends its predicted display pose per WiVRn's pattern
   mechanism — but who owns prediction when the shared app also needs the pose *earlier* than the
   colour pipeline (app render start vs encode start)? Needs a two-stage horizon like ALVR's
   pipeline-latency split.
4. **One bridge session per (app, observer) or per observer?** Per-observer multiplexing saves
   handshakes and clock offsets, but per-app groups keep capture isolation (wolf) and per-app
   teardown clean.
5. **Version-lock policy:** WiVRn's structural hash is safe but forces lockstep upgrades of all
   participating machines; is that acceptable for LAN-first spatial sharing (probably yes, per
   05 §2.2's exact-commit precedent), or do we need a negotiated-capability layer from the start?
6. **Vulkan-video maturity** for the encoder tier on our Nix/NixOS driver matrix (WiVRn ships it
   behind `WIVRN_USE_VULKAN_ENCODE`; is it production-grade on RADV/ANV yet?).
7. **How mode 1/2 (flat/stereo mirror) shares infrastructure:** same bridge with a colour-only
   group and no pose loop (degenerates to Sunshine-class streaming), or is shipping actual
   Sunshine/wolf for flat mirroring less work than owning a second profile?
