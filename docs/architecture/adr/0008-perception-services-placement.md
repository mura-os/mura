# ADR 0008: Perception services (passthrough + hand cutout) run Monado-side on the shared frame pipeline

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [perception-passthrough-hands.md](../perception-passthrough-hands.md),
research [13-passthrough](../../research/13-passthrough.md),
[14-mobile-stereo-depth](../../research/14-mobile-stereo-depth.md),
[15-hand-segmentation-matting](../../research/15-hand-segmentation-matting.md),
[16-perception-claims-audit](../../research/16-perception-claims-audit.md). Relates to
[adr/0006-compositor-strategy.md](0006-compositor-strategy.md) (the zxr compositor is an OpenXR
client of Monado) and [zxr-shell-v2-composition.md](../zxr-shell-v2-composition.md).

## Context

Passthrough view-correction and hand cutout both produce per-eye layers the zxr compositor blends
into its sort-last composition (passthrough = environment colour+depth; hand matte = policy-driven
top layer). Both need, at high rate and tight timing:

- **camera frames** (raw, exposure-timestamped),
- **head pose queried at the camera's exposure timestamp** (a 4 ms error visibly floats the image),
- **calibration** (intrinsics, extrinsics, rectification, per-device camera geometry),
- and they must **never block the compositor's display path**.

Two other consumers already need exactly the same camera frames + timestamps + calibration:
**Mercury** (hand joints — also the cutout's prior) and **Basalt/SLAM** (head pose — the very pose
passthrough needs at exposure time). All three live in Monado today, fed by its frameserver
(`xrt_frame` sink graph). The question is where the two new perception services execute.

## Decision

**Monado owns the cameras, the clock domain, and the calibration; the passthrough and hand-cutout
services consume the same `xrt_frame` fan-out that already feeds Mercury and SLAM, and deliver
their outputs to the zxr compositor as dmabuf layers with `wp_linux_drm_syncobj_v1` explicit-sync
— the same transport zxr-shell-v2 already uses for client buffers.**

*(Recast 2026-09-23 to the ADR 0010 corrected framing, resolving the review's in-process/sibling
conflation — perception backlog #5/#7/#8, registry §10.3.)* What is decided here is **ownership**
(Monado owns sensors, clock, calibration; the compositor owns composition) and the **boundary
shape** (finished per-eye layers as dmabuf + explicit-sync; pose-at-exposure served from the
tracker's process). Each service's *execution placement* — in-process `xrt_frame` sink vs.
Monado-adjacent process on a versioned frame+pose relay — is fixed at implementation against
exactly one of those two specified boundaries: if adjacent, the relay carries frames, exposure
timestamps, and pose queries with a stated backpressure rule; if in-process, a stated sink ABI
and a crash-containment story (the GPL cutout net is process-isolated regardless). It is never
left as "sink or adjacent process" ambiguity per component.

Concretely:

- **Camera ownership stays in Monado's frameserver.** Passthrough, hand cutout, Mercury, and SLAM
  all subscribe to the same frame graph, so there is **one set of camera timestamps and one
  calibration** shared across every perception consumer — the central invariant of
  [perception §"shared invariants"](../perception-passthrough-hands.md). This is the decisive reason:
  the alternative (compositor opens the cameras independently) creates two clock/calibration domains
  for data that must be pixel- and time-aligned.
- **The pose-at-exposure query is a Monado-internal call**, not a cross-process round-trip — the
  service that needs `T_{W←camera}(t_expose)` is in the same process as the tracker that produces it.
- **Each service publishes latest-complete**, never awaited by the compositor. Passthrough publishes
  a `DepthFrame` + camera textures; the cutout publishes premultiplied `αF` + `α` + hand-depth. The
  compositor imports these as dmabuf with timeline-semaphore acquire/release and treats them exactly
  like the environment/top layers in [composition §7.4](../zxr-shell-v2-composition.md): warp to
  display time from the current pose, drop/late-warp if stale, never stall.
- **The depth estimator is a pluggable backend inside the passthrough service**
  (classical Vulkan / `VK_QCOM` / Adreno DFS / Hexagon HTP — [perception §depth backend](../perception-passthrough-hands.md)),
  selected per device, none of which changes this placement.
- **Camera access itself is a per-device `spatial.adaptation.camera` concern** (V4L2 vs a vendor
  path); Monado's frameserver already abstracts camera sources, so per-device camera bring-up has a
  natural home.

## Rationale

- **One timestamp/calibration domain.** The single most important perception invariant is that
  colour, depth, matte, and pose are all keyed to the same exposure time. Sharing Monado's frame
  graph makes that structural instead of a synchronization problem across processes/clocks.
- **The pose producer and the pose consumer are co-located.** Passthrough's hardest timing
  requirement (pose at exposure) becomes an in-process query against Basalt, not an IPC.
- **Reuse, not duplication.** Mercury already decodes these frames for joints (the cutout's prior);
  SLAM already consumes them for pose. Adding two more `xrt_frame` sinks is the cheapest integration
  and keeps the GPL/BSL/license boundaries clean (the cutout net, potentially GPL, runs as its own
  process/sink — [perception §licensing](../perception-passthrough-hands.md) — not linked into the
  compositor or Monado core).
- **The compositor stays a pure consumer.** ADR 0006 made the zxr compositor an OpenXR client of
  Monado; keeping perception Monado-side preserves that boundary — the compositor imports finished
  layers and composites, exactly as it does for 2D planes and clients. No ML inference or stereo
  matching ever enters the compositor's frame loop (the deadline rule).

## Consequences

- spatial-os builds the two services as Monado frame-sinks (or Monado-adjacent processes sharing its
  frameserver), delivering dmabuf + explicit-sync layers to the zxr compositor. The compositor gains
  an **environment-layer** input and a **hand-top-layer** input with policy, both non-blocking.
- **Monado's pose-query API must serve arbitrary exposure timestamps at camera rate with high
  accuracy.** If it cannot, that is a blocking dependency to resolve upstream — flagged in the
  perception doc and here.
- The GPL matting model is process-isolated by construction; the compositor (and Monado core, BSL)
  do not become derivative works.
- `spatial.xr.passthrough.*` contract options (enable, latencyMode, depthBackend, handCutout policy)
  configure these services; per-device camera geometry extends `spatial.adaptation.camera`.
- **Not ratified beyond placement:** the *internal* structure of each service, the depth backend per
  device, and the exact frame-graph wiring depend on the BSP-access unknowns the claims audit
  enumerated (camera zero-copy into Vulkan, Adreno DFS/HTP availability, the "12 ms" endpoints) and
  on hardware spikes; those are backlog items, not decided here.

## Alternatives considered

- **Compositor-internal perception** (compositor opens cameras, runs stereo + matting itself):
  rejected — creates a second camera timestamp/calibration domain competing with Mercury/SLAM,
  duplicates frame decode, cross-process pose queries for exposure time, and risks ML/stereo work
  drifting onto the display-critical path. The compositor should import finished layers, not own
  perception.
- **A standalone privileged perception sibling** (own process, opens cameras directly, feeds both
  Monado and the compositor): rejected as the default for the same timestamp/calibration-domain
  reason — it would have to re-share frames with Monado's Mercury/SLAM anyway. Retained as a
  fallback shape only if a specific BSP cannot expose cameras through Monado's frameserver.
- **Per-application (client-side) passthrough/cutout** (à la the Rectus OpenXR API layer): rejected —
  it is the model spatial-os exists to replace; passthrough and hand cutout are system services with
  a privacy boundary (clients never see camera frames or mattes), not per-app effects.
