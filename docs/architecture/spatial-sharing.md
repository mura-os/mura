# spatial-os architecture: spatial sharing

**Status:** design note (no ADR yet; ratification follows the first implementation spikes).
**Date:** 2026-09-22. Synthesizes [17-sharing-capture-stack](../research/17-sharing-capture-stack.md),
[18-xr-streaming](../research/18-xr-streaming.md), [19-wayland-proxying](../research/19-wayland-proxying.md),
and extends [zxr-shell-v2-composition.md](zxr-shell-v2-composition.md) / [adr/0006](adr/0006-compositor-strategy.md).

"Screen sharing" in a spatial compositor is not one feature. It decomposes by **capture point in our
pipeline** — and one mode captures nothing at all. This note fixes the taxonomy, the per-mode
mechanism, the security invariants, and the protocol hooks zxr-shell-v2 must reserve.

## 1. The five modes (normative taxonomy)

| # | Mode | Capture point / mechanism | Representation on the wire | Viewer pose freedom | Primary cost |
|---|---|---|---|---|---|
| 1 | **Spectate** | Post-composition eye buffer (or a compositor virtual camera), as an output/virtual-output capture source | Video (PipeWire) | None — sharer's view | 1 video stream; works in OBS/calls today |
| 2 | **Share a 2D window** | The window-local texture, pre-placement, as a foreign-toplevel capture source | Video + placement metadata | **Full** — each receiver rasterizes its own plane depth | 1 stream/app; refreshes only on app damage |
| 3 | **Share a 3D app** | The app's atomic colour+depth submission, pre-composition; **remote observers are additional `zxr_view`s** the shared app renders inside the same atomic frame | Per-observer RGBD groups over the bridge (§4) | Full, via observer-specific rendered views | Scales per observer; depth codec |
| 4 | **Share-the-app** | **No capture.** Wayland protocol proxying (waypipe / virtio cross-domain): the remote or VM app's surfaces arrive as real `wl_surface`s | Wayland protocol + diffed/encoded buffers | Full — it *is* a local window (composition §7.3) | O(damage); memory mirrors both ends |
| 5 | **Workspace join** | Not media: authoritative **placement-graph replication**; each app shared underneath via modes 2/3/4 | Small state sync + N per-app shares | Full | Sum of the above |

Two structural insights carry the design. First, **mode 3 requires no new rendering contract**: zxr's
N-view generalization means an authorized remote observer is just more views in the shared app's
atomic frame — simulation advances once, renders N view-sets, and the atomic-submission rule already
prevents torn observer frames. Second, **mode 4 is both a sharing mode and a security architecture**:
the same proxy that brings a remote laptop's editor into the headset brings a *microVM-isolated*
app's windows in natively (Spectrum OS's model, [19 §5](../research/19-wayland-proxying.md)).

## 2. Modes 1–2: the ecosystem tier (capture, consent, input)

Per [17](../research/17-sharing-capture-stack.md), the adoption path is cheap and standard:

- **Compositor capture surface:** implement `ext-image-copy-capture-v1` +
  `ext-image-capture-source-v1` (output *and* foreign-toplevel sources, hence
  `ext-foreign-toplevel-list-v1`), with dmabuf-modifier fixation and shm fallback. Skip legacy
  `wlr-screencopy` unless a concrete tool demands it.
- **Portal:** reuse `xdg-desktop-portal-wlr` unmodified on day one (it needs only the protocols
  above plus our desktop name in its `UseIn` list) — this buys OBS, browser WebRTC, and
  portal-based tools with zero portal code. A native `xdg-desktop-portal-spatial` backend follows
  the xdpw shape (three D-Bus methods + chooser + PipeWire producer) when we need the in-space
  consent picker and SpatialCast source types.
- **SpatialCast** extends the portal bitmask additively: `XR_VIEW = 8` (spectate),
  `APP_VOLUME = 16` (mode-3 transport), `WORKSPACE = 32` (a session handle, not pixels), with
  vendor-scoped restore data and per-type consent language (§6).
- **Input injection:** libeis in-compositor. Per shared window, one absolute device whose region is
  the window-local texture space, `mapping_id`-joined to the stream; remote (x, y) feeds the *same*
  window-local input path as XR ray→plane intersection. Emulated input is badged and pausable
  (e.g. while a password prompt is focused).

**PipeWire ground truth constrains mode 3's local publication** ([17 §6](../research/17-sharing-capture-stack.md)):
there is **no single-channel float video format** and **no cross-stream frame atomicity**. Therefore
an `APP_VOLUME` stream is **one stream whose buffers carry colour and depth as separate `spa_data`
blocks** (atomic by construction) with view descriptors (per-view P·V, depth encoding, bounds) in
custom metadata (`SPA_META_START_custom`), and `SPA_META_SyncTimeline` for explicit sync
(gnome-remote-desktop proves that path is deployed practice).

## 3. Mode 4: share-the-app (protocol proxying) — the default for remote 2D apps

Per [19](../research/19-wayland-proxying.md), **waypipe, unmodified from nixpkgs, is the shipped
mechanism** for remote application windows: it alone covers dmabuf (Vulkan-first, damage-segment
GPU copies — not full-frame readback), `wp_linux_drm_syncobj_v1` timelines, presentation-time with
cross-machine clock translation, colour management, full clipboard/DnD, and forward-compatible
pass-through of fd-free protocols. A proxied app is indistinguishable from a local one at the
surface layer — same plane treatment, same depth test, input as real Wayland events, rendered at
*our* scale. Mode 2 is reserved for whole-desktop mirroring and high-motion content past the
damage≪area crossover ([19 §7.2](../research/19-wayland-proxying.md)).

Hard consequences for zxr-shell-v2 (the [19 §8](../research/19-wayland-proxying.md) globals list):

- **Must/should implement:** `wl_compositor` 6, `wl_shm`, `wl_seat` (real keymap fd), `wl_output` 4,
  `xdg_wm_base` 7, `wl_data_device_manager` 3, **`zwp_linux_dmabuf_v1` v4+ with real feedback
  tranches** (waypipe intersects them against its own device), `wp_viewporter`,
  `wp_fractional_scale_manager_v1`, `zxdg_decoration_manager_v1` (force server-side),
  `wp_linux_drm_syncobj_manager_v1`, `wp_presentation` (stable clock_id).
- **`wp_security_context_manager_v1`, implemented early — the highest-leverage item.** waypipe
  attaches a per-app security context to the socket it hands us (`--secctx`); that is the durable
  identity our policy engine keys on. It also closes a real hole: waypipe forwards
  `zwlr_screencopy`/`ext-image-copy-capture` to proxied clients in full, so **capture and
  data-control protocols are gated behind security-context policy** — otherwise a proxied remote app
  can screencapture the XR session.
- **Transports:** any reliable stream. Order of adoption: ssh/TCP (exists), **`--vsock` for
  microVMs** (no virtio-gpu/rutabaga/guest-proxy stack needed — the first VM-isolation
  implementation), **USB via CDC-NCM gadget networking** first (zero new code; the deviceinfo
  `usb_network_function` pattern from [02](../research/02-postmarketos.md)), FunctionFS bulk
  endpoints only against measured NCM latency.
- **VM isolation end-state:** Spectrum's process shape — per-VM unprivileged, bwrap-jailed
  cross-domain backend holding only our compositor socket — recorded as the target architecture;
  virtio-gpu cross-domain deferred until `--vsock` copies measurably bottleneck.
- **Proxied 3D clients are a virtualization feature, not a networking one**
  ([19 §9.3](../research/19-wayland-proxying.md)): the zxr control plane proxies for free
  (fd-free pass-through), but colour+depth cannot cross a network link (full-area damage; lossy
  video destroys the `argmin` identity T1 correctness depends on). In-VM zero-copy via virtgpu
  resource sharing is credible but gated on four missing pieces (guest dmabuf forwarding, guest
  syncobj support, fd-aware proxying of our protocol, a guest GPU context decision) — deferred.

## 4. Mode 3: the per-observer RGBD bridge

Per [18](../research/18-xr-streaming.md), no existing engine carries what mode 3 needs, but WiVRn is
the right template and the deltas are precise:

- **Egress/ingress live inside zxr-shell-v2** (a bridge component), not in a fork of WiVRn: WiVRn's
  endpoints are "whole squashed session ↔ one HMD"; ours are "one shared app's observer view-group ↔
  one observer compositor", N-way, tapped where the atomic submissions already exist pre-squash.
  WiVRn remains untouched as the whole-session-to-headset path.
- **Vendor WiVRn's proven modules:** the typed two-channel protocol shape (TCP control + encrypted
  UDP stream, structural-hash version lock), the `video_encoder` abstraction
  (nvenc/vaapi/vulkan-video/x264), the **pacer** (per-frame 13-timestamp feedback, 99.5th-percentile
  present-to-decoded budget, phase-locked virtual vsync), `tracking_control` sampling patterns +
  pose histories, and `clock_offset` (instantiated per observer — clock domains never mix).
- **Build the genuinely new pieces:** (1) **atomic RGBD group framing** — one logical unit binds
  colour+depth for all views of one frame id with view descriptors (P·V per view, depth encoding
  near/far/reversed-Z, bounds) and the rule that receivers consume *complete groups only* (never
  newest-colour with newest-depth; WiVRn's left/right frame-index re-pairing is exactly the hazard);
  (2) a **depth codec** — lossless zstd tiles with per-tile quantization ranges as the honest
  default; lossy depth only ever as an explicitly negotiated error-bounded profile, because depth
  error is cross-app geometry error at silhouettes; (3) **validity masks** (RLE 1-bit coverage) so
  observer depth tests ignore non-samples; (4) **per-observer budgets** (resolution/foveation/
  bitrate negotiated per observer; WiVRn's per-frame foveation params reused per observer).
- **WAN profile:** Sunshine-grade Reed-Solomon FEC + reference-frame invalidation, ALVR's ~300-line
  adaptive `BitrateManager`. Rejected: the Moonlight protocol as base (no pose loop, single-buffer
  frames), squash-before-encode (destroys per-app depth), lossy depth by default.
- **Ingest scheduling** copies Monado `comp_multi`: per-source pacer + progress/scheduled/delivered
  slots; only complete, GPU-signalled groups enter the critical path. A remote group enters the
  observer's compositor as a **first-class colour+depth client** — depth-composited per T1, with
  declared staleness per the T3 rule (reproject/placeholder, never silently depth-test stale eyes) —
  not as a flat quad.
- **Session shape:** wolf's per-app capture groups — a shared app gets a dedicated egress endpoint
  fanning out to N observer pipelines; lifecycle/input isolation live outside the app.

## 5. Mode 5: workspace join

A small **authoritative placement graph**, not media: per shared app — owner, room transform,
bounds, viewers, controller lease, representation mode. One host is authoritative for room
placement; each app's owner is authoritative for its state. Rights are split (**view / control /
move / reshare**, clipboard and file transfer separately authorized); control is a revocable lease
with visible remote pointers for non-controllers. Participants map the shared room into their local
tracking space; private apps are simply absent (or explicit redacted placeholders). Each shared app
uses mode 2/3/4 underneath. Overte's entity-server replication is the studied scale datapoint, not
an adopted design; the MVP is single-host-authoritative.

## 6. Security invariants (additions to [overview.md](overview.md))

1. **Capture is pre-private-composition** for every mode except spectate — a private window can
   never leak into, or occlude content out of, a shared stream. The shared app must remain
   renderable for observers when locally hidden; conversely observer view requests are clipped to
   authorized app bounds and budgeted.
2. **Consent language is per-scope and must not be inherited silently:** a window share says "share
   this window"; an `APP_VOLUME` share must say **"viewers can look at this object from any angle
   you have not hidden"** (observer-controlled viewpoints break 2D occlusion intuitions); an
   `XR_VIEW` spectate needs the gaze warning (viewers see everything you look at, notifications
   included); a workspace join states placement visibility. Active shares are badged in-space.
3. **Proxied connections carry durable identity** via `wp_security_context_manager_v1`; capture,
   data-control, and other sensitive globals are policy-gated per origin. Fail-closed whitelisting
   is the posture for any future VM-boundary proxy we author, even though waypipe's pass-through is
   right for the network case.
4. **Per-observer views are authorized objects**: added only through the sharing service, budgeted
   (max resolution/rate), and revocable; a peer cannot request unbounded views or views of another
   app.
5. **Clock and staleness honesty:** remote content carries its render pose + times in explicit clock
   mappings; validity limits are declared, and stale 3D content degrades to bounding-box/placeholder
   (the composition doc's T3 rule) rather than being silently composed.

## 7. Transport positions

- **PipeWire** where the ecosystem lives: modes 1–2 (portal streams; OBS/calls/wayvnc), and
  `APP_VOLUME` local publication as one-buffer-many-blocks (§2). Not forced onto the network bridge.
- **The mode-3 bridge owns its sockets** (WiVRn-shaped typed channels); PipeWire optionally fronts
  its local end.
- **Mode 4 is transport-agnostic** (reliable stream): ssh/TCP, vsock, USB CDC-NCM → FunctionFS.
- **Mode 5** is a small reliable control protocol (ordered messages for state, latest-wins for
  poses).

## 8. Protocol hooks reserved in zxr-shell-v2

Recorded here so [zxr-shell-v2-composition.md](zxr-shell-v2-composition.md) §8 and the eventual
`zxr-shell-v2.xml` account for them without redesign:

1. **Observer views:** view authorization/budget objects over the existing N-view mechanism —
   a view carries an origin (local HMD / named observer), a budget, and revocation; shared apps may
   decline observer views (capability flag).
2. **Per-app capture groups:** a shared app's egress endpoint (wolf pattern) with per-observer
   fan-out — the compositor-side object the sharing service and SpatialCast `APP_VOLUME` bind to.
3. **Share-scope objects:** one consent surface across the five modes (portal source types
   `XR_VIEW`/`APP_VOLUME`/`WORKSPACE` map onto compositor share sessions).
4. **The proxied-client globals list** ([19 §8](../research/19-wayland-proxying.md)) folded into the
   compositor's baseline global set, with `wp_security_context_manager_v1` and policy-gated capture.
5. **Frame-descriptor reuse:** the mode-3 group descriptor is the network serialization of the same
   atomic frame contract clients already submit — one schema, two encodings (protocol-local and
   wire).

## 9. Open questions carried forward

From [17 §11](../research/17-sharing-capture-stack.md): depth-on-the-wire format for `APP_VOLUME`
(quantized GRAY16 vs metadata-described D32 dmabuf vs upstream SPA format); whether portal `Start`
carries observer count/budget; spectate as real vs virtual output; in-space consent chooser
placement (backend in-process vs Mutter-style private API). From
[18 §9](../research/18-xr-streaming.md): the depth-codec bake-off on real buffers; egress tap
placement (extra pool views vs bridge re-request); observer pose-prediction authority; session
granularity per (app, observer); structural-hash lockstep vs negotiated capabilities; Vulkan-video
maturity on RADV/ANV. From [19 §11](../research/19-wayland-proxying.md): verifying
security-context-gated registry filtering end-to-end; waypipe mirror memory on the target SoC;
dmabuf feedback surviving waypipe's modifier intersection on mobile GPUs; frame-callback pacing for
remote apps at 90 Hz; presentation time of stale planes; the input-latency threshold where a proxied
window stops feeling attached to the hand ray.
