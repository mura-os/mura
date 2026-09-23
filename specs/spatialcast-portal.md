# specs/spatialcast-portal: spatial source types on the ScreenCast portal

**Status:** draft normative spec (specification workstream, wave 4).
**Design sources:** [research/17 §9](../docs/research/17-sharing-capture-stack.md) (the
SpatialCast sketch this makes normative), [spatial-sharing.md](../docs/architecture/spatial-sharing.md)
§2/§8, consent decisions in spatial-sharing §2 (picker) and §6 (badging); depth encoding mirrors
`zxr_frame_slot_v2.set_depth_range` ([protocols/zxr-shell-v2.xml](../protocols/zxr-shell-v2.xml))
so capture and composition speak one depth language.
**Grounding:** "XDG" here means **xdg-desktop-portal** (the D-Bus service family, sense (c));
interfaces extend `org.freedesktop.impl.portal.ScreenCast` additively — a stock consumer that
never sets the new bits sees today's portal exactly.
**Budget impact** (inv. 9): capture publication rides the existing capture/PipeWire budget
lines; `APP_VOLUME` re-render costs are governed by the observer-view budget objects
(spatial-sharing §8.1); no new frame-path cost.

## 1. Source types (the `AvailableSourceTypes` bitmask extension)

Advertised only by `xdg-desktop-portal-spatial`; values continue the upstream bitmask
(`MONITOR=1, WINDOW=2, VIRTUAL=4`):

| Bit | Name | Stream content |
|---|---|---|
| 8 | `XR_VIEW` | the composed view (spectate): one video stream, mono (chosen eye) or stereo per `view_config` |
| 16 | `APP_VOLUME` | one 3D client's contribution re-rendered for an observer-specific viewpoint: colour+depth |
| 32 | `WORKSPACE` | **not pixels**: selection yields a place-join handle (mode 5); the stream node carries no buffers |

## 2. Stream properties

Per-stream vardict keys (in the `Streams` reply, beside upstream `position`/`size`/`source_type`):

- `view_config` (`(uu u)`: per-view width/height, view count 1|2) — `XR_VIEW` only.
- `presence_mode` (`u`: 0 embedded, 1 metadata-only, 2 hidden) — whether controllers/hands and
  focus highlights are burned into `XR_VIEW` frames or delivered as metadata; the cursor-mode
  analog.
- `depth` (`(ddddu)`: near_m, far_m, min_depth, max_depth, flags bit0=reversed) — `APP_VOLUME`
  only; identical semantics to `zxr_frame_slot_v2.set_depth_range`.
- `place_handle` (`s`: opaque token for the session layer) — `WORKSPACE` only.

## 3. PipeWire publication

- `XR_VIEW`: ordinary `Video/Source` node; dmabuf-with-modifiers preferred, shm fallback;
  `Header`/`VideoDamage`/`VideoTransform` metadata as today; damage is near-total per frame
  (head motion) so consumers should negotiate `maxFramerate` instead.
- `APP_VOLUME`: one stream whose buffers carry **colour and depth as two data blocks of one
  buffer** (cross-stream frame atomicity does not exist — research/17 §6.5); depth encoded per
  the `depth` property (GRAY16 quantized now; a custom-described D32 block later). A custom SPA
  metadata block (`SPA_META_START_custom` range, type name `spatial.cast.view`) carries per-frame:
  observer `view` and `projection` matrices (16×f32 each, column-major), the source `frame id`
  (u64), and validity flags. Consumers ignoring the metadata still decode the colour block.
- `WORKSPACE`: the node exists for lifecycle/consent uniformity but negotiates zero buffers;
  activity is signalled through the session layer using `place_handle`.

## 4. Consent and restore

- **Chooser**: presented by the portal backend as a separate privileged client; while shown, the
  compositor gives its surfaces lock-grade treatment and withdraws input from the requesting app
  (spatial-sharing §2's hardened decision).
- **Consent language is per-type and mandatory** (research/17 §9): `XR_VIEW` carries the gaze
  warning ("viewers see everything you look at, including notifications"); `APP_VOLUME` states
  observer-controlled viewpoint ("viewers can look at this object from any angle you have not
  hidden"); `WORKSPACE` states placement visibility. Implementations must not reuse the
  monitor-share wording for these types.
- **Badging**: an active session of any type renders the in-space share badge (spatial-sharing
  §6 invariant 2); notification suppression applies per capture policy.
- **Restore tokens**: vendor-scoped restore data
  `("spatial-os", 1, {source_type, window_uuid | view_id | place_id})` on the upstream
  single-use-token machinery; `WORKSPACE` restore re-offers, never silently rejoins.

## 5. Input injection scope

`RemoteDesktop`/EIS applies unchanged to `WINDOW` shares (2D, `mapping_id`-joined regions).
`APP_VOLUME` has **no injection path in this version**: a 6DoF pose channel is explicitly out of
scope until the delegation input vocabulary (`zext-toplevel-export` input) stabilizes;
implementations must not map EIS regions onto volume shares.

## 6. Conformance checklist

1. A stock OBS (no spatial bits) against `xdg-desktop-portal-spatial`: sees MONITOR/WINDOW only,
   full functionality.
2. `APP_VOLUME` stream: colour and depth blocks always from the same source frame (single-buffer
   atomicity); metadata frame id matches.
3. Chooser shown: requesting app verifiably receives no input until dismissal.
4. Restore token for a revoked window: chooser re-presented, never silent re-grant.
5. Per-type consent strings present and distinct (i18n keys, not shared).

## 7. Open items

The D32 depth block descriptor (upstream SPA format conversation — research/17 §10 defer);
observer-budget property surfacing to consumers; `WORKSPACE` handle hand-off API (the mode-5
session layer owns it, spec pending `spatial-sharingd` design).
