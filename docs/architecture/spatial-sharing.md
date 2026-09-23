# spatial-os architecture: spatial sharing

**Status:** design note (no ADR yet; ratification follows the first implementation spikes).
**Date:** 2026-09-22. Synthesizes [17-sharing-capture-stack](../research/17-sharing-capture-stack.md),
[18-xr-streaming](../research/18-xr-streaming.md), [19-wayland-proxying](../research/19-wayland-proxying.md),
and extends [zxr-shell-v2-composition.md](zxr-shell-v2-composition.md) / [adr/0006](adr/0006-compositor-strategy.md).

"Screen sharing" in a spatial compositor is not one feature. It decomposes by **capture point in our
pipeline** — and one mode captures nothing at all. This note fixes the taxonomy, the per-mode
mechanism, the security invariants, and the protocol hooks zxr-shell-v2 must reserve. §2.2
additionally fixes the **stills/capture taxonomy** (scope × projection × temporality) that
screenshots and every capture session are points in.

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
  **Consent-picker placement (decided, resolving the research/17 §11.4 fork and registry
  §10.4):** the chooser is *owned by the portal backend* (service plane) and presented as a
  separate privileged client surface on layer-shell with compositor-granted binding — the
  xdpw/COSMIC shape, consistent with ADR 0012's seam model. The Mutter-style private
  compositor-API chooser is rejected: it would move consent *presentation* into the authority
  plane, which ADR 0012 reserves for consent *enforcement* only (which buffers a session may
  reach).   zxr renders the picker's surfaces like any privileged shell client and enforces the
  outcome; the picker's UX design remains open (registry consent-picker row stays partial).
  Hardened by the commercial evidence ([36 §5](../research/36-vr-shell-interaction-patterns.md):
  both Quest and visionOS render permission UI in the compositor trust domain and withdraw app
  input while it shows): while the picker is displayed, zxr treats its surfaces as
  lock-grade (unspoofable placement, no app occlusion) and **withdraws input from the requesting
  app** — the separate-client placement stands, but its surfaces get trusted-surface treatment,
  never ordinary-client treatment.
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

### 2.1 Spectate mechanics: the pre-distortion tap and the FOV crop

Three implementation facts pin down mode 1 (from the mirroring analysis; Monado reference verified
against the pinned clone):

- **The tap is pre-distortion by construction, and it is ours.** zxr-shell-v2 is an OpenXR *client*
  of Monado: we composite into rectilinear eye images and submit one projection layer; **Monado owns
  the lens warp downstream** ([zxr-shell-v2-composition §7.4](zxr-shell-v2-composition.md)). Spectate
  therefore taps **our own composed eye image, pre-`xrReleaseSwapchainImage`** — no inverse
  distortion ever exists in the path, and no runtime cooperation is needed. Monado's
  `comp_mirror_to_debug_gui` (`monado/src/xrt/compositor/main/comp_mirror_to_debug_gui.{c,h}`:
  crop-blit → `vk_image_readback_to_xf_pool` → `u_sink`, with a `push_every_frame_out_of_X`
  throttle) is the reference implementation of exactly this shape; ours feeds the PipeWire
  publication from §2 instead of a debug sink.
- **The "algorithm" is a symmetric-FOV crop + blit.** The composed eye image uses the asymmetric
  off-axis frustum from `xrLocateViews` (tan-angle bounds l/r/u/d). A watchable mono spectator
  frame crops to a **symmetric sub-frustum** (e.g. ±min(|l|,|r|) horizontally, likewise
  vertically), then adjusts to the target aspect (16:9) — a linear mapping in eye-texture UV.
  Default source is the left eye; mono is the default `view_config`, stereo an opt-in.
- **A "nice" third-person spectator camera is an observer view, not new machinery.** The
  standardized prior art is `XR_MSFT_secondary_view_configuration` +
  `XR_MSFT_first_person_observer` (HoloLens mixed-reality capture: apps render an extra
  camera-matched view). Our mode-3 mechanism — **observer views as additional authorized
  `zxr_view`s** — is the generalization: a compositor-owned spectator camera is simply one more
  budgeted view composed by us, through the same hook (§8.1).

The known fork (already open question [17 §11.6](../research/17-sharing-capture-stack.md)): the
crop-blit tee cannot *strip* content (notifications, other users' private windows). A
policy-filtered spectate is a second composition pass over a policy-selected subset of the scene —
architecturally the same as a compositor-owned observer view. v0 ships the tee; the re-compose
variant rides the observer-view mechanism when policy demands it.

### 2.2 Stills, scope, and projection: the capture taxonomy (normative)

The five modes of §1 classify *sharing relationships*. Underneath them, every act of capture —
one-shot screenshot or ongoing stream — is a point in a three-axis space. This section fixes that
space, because "screenshot" is not one feature either, and the desktop tools' assumptions
(rectangular screens, screen-space regions) break in specific, enumerable ways. Field evidence:
in the KWin VR thread ([31 §1](../research/31-kwin-vr.md)), a user rejected Breezy Desktop
*solely* because Spectacle screenshots didn't work, and adopted the KWin fork because they did —
capture compatibility is an adoption deal-breaker for the desktop-replacement audience.

**Axis 1 — Scope** (*what content*):

| Scope | Definition |
|---|---|
| `window` | One toplevel's content (its surface tree: subsurfaces, popups per policy) |
| `window-set` | Several toplevels chosen ad hoc (superset case: a "virtual screen" is a persistent, named window-set) |
| `plane-region` | A sub-rectangle of one plane's texture (window or virtual screen) |
| `full-scene` | Everything the user perceives: environment layer, all windows, chrome, hands, cursor/ray |
| `world-volume` | A bounded region of space and the apps inside it, 2D and 3D |

**Axis 2 — Projection** (*how it is imaged*):

| Projection | Definition | View-dependence |
|---|---|---|
| `texture-space` | The window-local texture, as-submitted (composition §7.3) | None — pixel-perfect, no head pose |
| `flat-composition` | A compositor-composed rectangle arranging a window-set as if a monitor existed | None |
| `head-view` | Rectilinear render from the user's head pose, pre-distortion (§2.1 tap); `mono` (symmetric-FOV crop) or `stereo-pair` (both eyes) | Full |
| `observer-view` | Rectilinear render from an arbitrary authorized camera pose (§8.1 machinery) | Observer's, not user's |
| `post-distortion` | The barrel-distorted panel image. **Debug/device-qualification artifact only** — never a user-facing capture | Full + lens |
| `+depth` modifier | Any of `head-view`/`observer-view`/`world-volume` carrying depth per the `APP_VOLUME` framing (§2, §4) | (as base) |

**Axis 3 — Temporality**: `still` or `stream`. Orthogonal to both axes. Consent scales with it:
a stream carries the in-space active-share badge for its whole life (invariant §6.2); a still needs
a shutter-moment consent (the portal's interactive flow) and no persistent badge. Mechanically the
two are one path: an `ext-image-copy-capture` session that captures one frame or many, and on the
portal side `org.freedesktop.impl.portal.Screenshot` vs `ScreenCast` — both served by xdpw on day
one ([17 §1.1](../research/17-sharing-capture-stack.md)).

**The validity matrix.** Not all cells exist; the invalid ones are load-bearing design facts:

| Scope \ Projection | texture-space | flat-composition | head-view | observer-view | +depth |
|---|---|---|---|---|---|
| window | **preferred** (the M1 window screenshot) | degenerate (set of one) | invalid† | mode 2 alt | mode 3 (3D client) |
| window-set | — | **preferred** (ad-hoc or virtual screen) | invalid† | valid | deferred |
| plane-region | **preferred** (region UX below) | valid | invalid† | — | — |
| full-scene | undefined (no single buffer) | undefined | **preferred** (spectate/screenshot of "what I see") | valid (3rd-person spectator, §2.1) | valid |
| world-volume | undefined | undefined | (is just full-scene cropped) | valid | **preferred** (spatial snapshot; single-frame `APP_VOLUME` group) |

† *invalid-by-design*: capturing a window or region through the head-view projection is strictly
worse than its view-independent cell (head pose baked in, perspective distortion, resolution loss)
— the compositor keeps view-independent sources for exactly this reason, and tools must be routed
to them. This is why the window screenshot is the *easy* case here and an afterthought on other
XR desktops.

**Realization rules** (how cells map onto machinery already specified):

- Scope maps onto the portal source-type bitmask: `MONITOR` ≙ window-set via
  flat-composition (a persistent virtual screen, **or an ephemeral one created for an ad-hoc
  window-set capture** — existing consumers see an ordinary monitor source and need no new code),
  `WINDOW` ≙ window/texture-space, `XR_VIEW` ≙ full-scene/head-view, `APP_VOLUME` ≙ +depth cells,
  `WORKSPACE` ≙ not pixels (§2). Projection variants are stream/session properties
  (`view_config` mono/stereo, depth encoding), not new source types.
- **Region capture is ray-swept on a picked plane, never screen-space.** A Spectacle/slurp-style
  rectangle dragged across the *stereo head-view* selects nothing coherent (the two eyes disagree,
  and depth makes a screen rectangle a frustum). The UX is: ray-pick a plane (window or virtual
  screen) → sweep a rectangle **in that plane's texture space** → capture the sub-rectangle from
  the view-independent source. A true "region of the world" is not a region — it is
  `world-volume` scope. Volume-selection UX is deferred with the spatial-snapshot tier.
- **Privacy attaches to cells, not tools.** Passthrough camera pixels can appear *only* in
  `head-view`/`observer-view`/`world-volume` projections — never in texture-space or
  flat-composition cells, which are safe by construction. **Normative default: stills and streams
  exclude the passthrough layer unless the consent dialog explicitly includes it** ("include your
  room's camera view?") — the ADR 0008 privacy boundary extended to capture; mechanism is the
  passthrough-redaction hook (ADR 0012 §4 item 4). Gaze-target and notification leakage exist only
  in `full-scene` scope (the §6.2 gaze warning); hands/cursor embedding follows doc 17 §9's
  `presence_mode`. Eye-camera imagery appears in no cell, ever (ADR 0011).

**The capture tool.** One shell-plane client — the same in-space consent picker/share chooser §2
already requires — owns the whole axis space: pick scope (ray-pick a window; multi-select a
window-set; sweep a plane region; "what I see"; a volume later), pick temporality
(screenshot / start share), pick destination (gallery file, clipboard, stream to portal consumer).
The appliance capture gesture (hardware chord / controller long-press → `full-scene` still →
gallery, Quest-style) is a preset into the same path: global-shortcut interception is authority
plane; the capture itself is an ordinary portal-mediated session; the passthrough-exclusion
default applies. This tool is a registry component
([component-registry.md](component-registry.md) §5), missing today.

**Compatibility verdicts.** Portal-speaking tools (Flatpak apps, browsers, GNOME-style
screenshooters) work on day one via xdpw's `Screenshot` + `ScreenCast` against `MONITOR`/`WINDOW`
sources. **Spectacle as shipped will not run**: on Plasma Wayland its primary path is KWin's
private `org.kde.KWin.ScreenShot2` D-Bus interface (which is also exactly why it *does* work under
the KWin VR fork and broke under Breezy — [31 §1](../research/31-kwin-vr.md)); we do not implement
KDE-private capture APIs, and Spectacle is treated as compatibility evidence, not a target.
Legacy `wlr-screencopy` tools (`grim`) stay per [17 §8.5](../research/17-sharing-capture-stack.md):
add only on demonstrated need. `post-distortion` capture, if ever exposed, lives behind a debug
flag on the device profile, not in the tool.

**Scanout realization.** Mirroring to a *physically attached* display
([ADR 0015](adr/0015-docked-desktop-mode.md)'s mirror tier) is the scanout realization of these
same cells — `full-scene × head-view (mono)` or `window-set × flat-composition` presented on the
external DRM connector directly, not encoded through a capture stream. The taxonomy governs the
semantics either way: the passthrough-exclusion default and the active-share badge duty apply
when presenting to a room, and the docked output itself participates in capture as an ordinary
`MONITOR` source.

## 3. Mode 4: share-the-app (protocol proxying) — the default for remote 2D apps

Mode 4 covers *remote/VM apps*. The adjacent local case — a foreign 2D compositor's **session**
(e.g. a live KWin/Plasma) appearing as per-toplevel floating windows — is **not** a sharing mode
and not capture: it is the delegation seam specified in
[foreign-session-integration.md](foreign-session-integration.md) (client-integration taxonomy
there; protocol `zspatial-toplevel-export-v1`, [ADR 0014](adr/0014-toplevel-delegation-protocol.md)).
Nothing in this document's consent/portal machinery governs delegation.

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

*(The join unit and replication schema are now owned by the places model: a joined workspace is a
**place**, and what replicates is its membership + transforms — [places-model.md §7](places-model.md),
ADR 0016; the shared frame's currency exclusions are rule C7 there.)*

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
   app. **The sharing service is now named (registry §10.5 resolved): `spatial-sharingd`**, a
   separate service-plane session daemon (D-Bus; socket-activated) owning share lifecycle, mode-5
   session authority, and consent state; it authorizes observer views through a privileged
   compositor API while the *data path* (the mode-3 RGBD bridge, capture publication) stays
   in-compositor per §4 — authorization and transport deliberately live on opposite sides of the
   authority boundary. Its API design is still open (registry row stays partial).
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

From §2.2 (stills taxonomy): the world-volume *selection* UX (deferred with the spatial-snapshot
tier); the on-disk format for `+depth` stills (a gallery-viewable RGBD container does not
meaningfully exist); lifecycle of ephemeral flat-composition outputs for ad-hoc window-set
captures (creation/teardown vs portal session lifetime, and keeping them invisible to
`wlr-output-management` consumers); whether the appliance capture chord needs a
consent-free owner-only carve-out or always runs the shutter consent.
