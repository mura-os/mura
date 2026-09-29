# specs/spatialcast-portal: spatial source types on the ScreenCast portal

**Status:** draft rev 2 (specification workstream; rev 1 findings from the portal/PipeWire-persona
review absorbed — frontend path made real, WORKSPACE moved out of ScreenCast, RGBD negotiation
specified, SPA metadata properly typed).
**Design sources:** [research/17 §9](../docs/research/17-sharing-capture-stack.md),
[spatial-sharing.md](../docs/architecture/spatial-sharing.md) §2/§6/§8; depth mapping identical to
[perception-intake §2](perception-intake.md)'s `depth_range` record (the one canonical depth
vocabulary; formerly also `zxr_frame_slot_v2.set_depth_range`, retired).
**Grounding:** "XDG" = **xdg-desktop-portal** (sense (c)). This spec extends the portal
*additively and honestly*: the pinned frontend rejects unknown `SelectSources.types` bits, so §1
defines the carrier as a Mura **frontend patch** (our normal patch-series model) with
upstreaming intent — never silent bit-squatting.
**Budget impact** (inv. 9): capture rides existing capture/PipeWire lines; `APP_VOLUME`
re-renders are governed by observer-view budget objects (spatial-sharing §8.1).

## 1. The carrier: a patched frontend, upstreaming intended

The stock `xdg-desktop-portal` frontend validates `SelectSources.types` against `MONITOR|WINDOW|
VIRTUAL` and rejects everything else, so backend-only extension is impossible. Mura
therefore carries a **frontend patch** (in `patches/xdg-desktop-portal/`, per the repo's
patch-series model) that: (a) advertises `version >= 6` plus a vendor property
`org.freedesktop.portal.ScreenCast.SpatialSourceTypes (u)`, and (b) accepts the spatial bits in
`SelectSources.types` only when the backend advertises them. The bit values below are **not
claimed from the upstream numbering space**: they are valid only in combination with the vendor
property (a frontend without the patch never accepts them), and the upstream proposal that
replaces this patch renumbers freely. Conformance is always tested **through** the patched
frontend, never backend-direct.

| Bit (vendor-scoped) | Name | Result |
|---|---|---|
| 0x10000 | `XR_VIEW` | one PipeWire video stream (composed view; mono or stereo) |
| 0x20000 | `APP_VOLUME` | one PipeWire stream in the RGBD profile (§3) |

**`WORKSPACE` is not a ScreenCast source type** (rev 1 error): joining a place is a session-layer
operation with no media stream. It is provided by `spatial-sharingd` as
`org.mura.Sharing1.JoinWorkspace(place_id, options) → handle`, presented through the same
chooser and consent UX (§4) but never through `Start`'s stream list, and never as a bufferless
PipeWire node.

## 2. Selection options and stream properties (vendor-prefixed, typed)

`SelectSources` vendor options (all optional, validated by the backend):

- `spatial_presence_mode` (`u`: 0 embedded, 1 metadata-only, 2 hidden) — requested treatment of
  controllers/hands/focus highlights in `XR_VIEW` frames. Values are this spec's own enum (not
  the cursor-mode constants; the analogy is conceptual only).
- `spatial_passthrough` (`b`, **default false**) — whether room-camera passthrough pixels may be
  included in `XR_VIEW`. Default-excluded per spatial-sharing §6; requesting `true` changes the
  consent text (§4). A backend unable to honor a requested combination rejects the selection.

Per-stream vendor properties in the `Streams` reply:

- `spatial_view_config` (`(uuu)`: per-view width, height, view count 1|2) — `XR_VIEW`.
- `spatial_presence_mode` (`u`) — the *effective* mode.
- `spatial_passthrough` (`b`) — effective inclusion.
- `spatial_depth` (`(ddddb)`: near_m, far_m, min_stored, max_stored, reversed) — `APP_VOLUME`;
  the canonical reciprocal-distance mapping shared with perception-intake's `depth_range`.

All vendor keys carry the `spatial_` prefix; signatures are canonical D-Bus strings.

## 3. The RGBD PipeWire profile (`APP_VOLUME`)

- **Consumers opt in**: the profile is negotiated; a consumer that does not negotiate it never
  receives an RGBD stream (the chooser offers `XR_VIEW` as the generic fallback).
- **One `spa_buffer` carries two logical images** (colour, depth), each possibly multi-plane:
  a negotiated `SPA_PARAM` descriptor (`mura.cast.layout`, versioned pod) maps each logical
  image to an ordered range of `spa_data` blocks with fourcc, modifier, per-plane offsets/strides
  and dimensions. Explicit-sync `SPA_DATA_SyncObj` blocks, where negotiated, are reserved after
  the image blocks and identified by the descriptor. Colour-block-only decoding by a consumer
  that negotiated the profile but ignores depth is legal.
- **Metadata**: `Spa:Pointer:Meta:SpatialCastView` — a versioned C struct (declared size,
  little-endian, 8-byte aligned) negotiated via `SPA_PARAM_Meta`, carrying `version`,
  `frame_id (u64)`, `view_count`, and per view: `view_matrix[16]`, `projection_matrix[16]`
  (binary32, column-major), image-index pair, bounds (half-extents, binary32), validity flags.
  N views, matching `spatial_view_config`; never a single collapsed view.
- **`SPA_META_SyncTimeline` and dmabuf device/modifier fixation are mandatory** for dmabuf
  streams (the doc-17 §6.4 rules apply unchanged).
- Depth block format: GRAY16 quantized baseline; D32 via the descriptor when the upstream SPA
  conversation lands (open item).
- `XR_VIEW` streams remain ordinary `Video/Source` nodes (dmabuf preferred, shm fallback,
  `Header`/`VideoDamage`/`VideoTransform`; damage near-total ⇒ consumers negotiate
  `maxFramerate`).

## 4. Consent and restore

- **Chooser**: portal-backend-owned, separate privileged client; lock-grade surface treatment +
  input withdrawal from the requesting app while shown (spatial-sharing §2).
- **Per-type mandatory consent language** (distinct i18n keys): `XR_VIEW` gaze warning (+ the
  passthrough sentence when `spatial_passthrough=true`); `APP_VOLUME` observer-controlled
  viewpoint; workspace join (via Sharing1) placement visibility.
- **Badging**: active sessions render the in-space badge (spatial-sharing §6 invariant 2);
  notification suppression per capture policy.
- **Restore tokens**: vendor-scoped restore data
  `("mura", 1, {"kind": s, "window_uuid"|"view_id"|"place_id": s})` (exact GVariant types;
  all members `s`); workspace-join restore re-offers, never silently rejoins.

## 5. Input injection scope

`RemoteDesktop`/EIS unchanged for `WINDOW` (2D, `mapping_id` regions). `APP_VOLUME` has **no
injection path**: EIS regions must not be mapped onto volume shares; 6DoF input waits for the
delegation input vocabulary.

## 6. Conformance checklist

1. Stock OBS through the **patched frontend**, no spatial bits requested: MONITOR/WINDOW
   unchanged, full functionality; spatial bits requested without backend advertisement: clean
   rejection.
2. RGBD: colour and depth blocks always same-source-frame (single-buffer atomicity); descriptor
   maps blocks correctly for a multi-plane colour format; a profile-negotiating,
   depth-ignoring consumer decodes colour.
3. Chooser: requesting app receives no input while shown.
4. `spatial_passthrough` default-false: an `XR_VIEW` stream started without the option shows no
   passthrough pixels even when the user's own view does.
5. Restore of a revoked source: chooser re-presented.
6. Per-type consent strings present, distinct, and passthrough-conditional.

## 7. Open items

The upstream proposal (frontend property + types — the M-C-style milestone); the D32 depth SPA
format; observer-budget surfacing; `Sharing1.JoinWorkspace` argument schema (with the
`spatial-sharingd` design).
