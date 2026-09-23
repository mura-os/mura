# Perception design backlog: disposition of the review

Triages [REVIEW-perception.md](REVIEW-perception.md) (GPT Sol red-team of the passthrough + hand
design). Verdict there: *with changes; start with a hardware feasibility spike and a corrected
opaque-only composition contract, not the four depth backends or Tier-2 matting.* This is a
research/design task, so the disposition is: fix genuine doc errors now; promote the review's kill
criteria to an explicit **P-1 gate**; record the rest as pre-prototype / pre-release design work.

## Fixed now (in [perception-passthrough-hands.md](perception-passthrough-hands.md))

| # | Review finding | Fix |
|---|---|---|
| 1 | The hand is already in `C_scene`, so a top layer can't hide it | Composition model corrected: the passthrough environment has the **hand region removed** (Meta HandsRemoval model); the hand layer is the **sole owner of hand pixels** and re-composites per policy. `hidden` now genuinely hides. Reinforces "one coupled service". |
| 3 | Reverse-Z sign inversion in the `smoothstep` | All depth comparisons now use **positive eye-space metres** (`d_s - d_h`), with one tested conversion per producer and a separate tested reverse-Z conversion to the shared attachment. |
| 4 | "Same-timestamp atomicity" contradicts the three-rate pipeline | Split into **two contracts**: hand matte strictly atomic; passthrough carries `t_colour` + `t_geometry` + the geometry→colour alignment transform. |
| — | Stale `ADR 0007` references (should be 0008) | Both occurrences fixed. |

## Promoted to a program gate: the P-1 BSP kill-test

The review's central point (#5, #6, #13, and "the one thing most likely to sink this") is that the
highest risk is **not depth quality** but obtaining synchronized, calibrated camera frames from the
target BSP with trustworthy exposure timestamps, mapping those into Monado's pose-history clock, and
moving frames into the perception/compositor GPU path without an unbounded copy or wait. This is
promoted from "P0 image quality" to a **binary P-1 gate on one named device**, ahead of all backend,
matting, and protocol work:

> **P-1 (kill-gate):** on one target headset BSP, demonstrate — synchronized stereo capture;
> documented per-frame exposure timestamps in a known clock; a pose-history query for an arbitrary
> capture timestamp with characterized accuracy/uncertainty; calibration retrieval; zero-copy (or
> explicitly measured copy) camera-dmabuf import into Vulkan; and transfer of the frame into the
> compositor process, all under sustained concurrent load (Mercury + SLAM + app render running).
> If any leg has no viable BSP route, stop the passthrough/hands architecture track for that device.

Only after P-1 passes do passthrough P0–P6 and hand tiers 0–2 proceed. The minimal first proof
(review #15) is deliberately tiny: one camera path, one fixed-proxy passthrough background with the
hand region held open, Mercury capsule mask, one global `visible` policy, revealing a preserved
passthrough hand image in front of one opaque virtual cube — *then* classical stereo for metric
ordering. Defer per-client policy, learned depth, alpha matting, foreground estimation, and vendor
accelerators until that path and P-1 exist.

## Deferred to pre-prototype design (specify before building the services)

- **#5 Camera→compositor buffer path.** Decide once: does Monado export *source-domain* artifacts
  (DepthFrame + camera textures) for a compositor-side warp, or *final per-eye layers*? ADR 0008
  currently alternates — pick one and specify a versioned IPC protocol + bounded buffer pools
  carrying stereo atomicity, format/planes/modifier, colour space, exposure/gain, distortion map,
  timestamps, calibration version, producer device id, and reuse lifetime.
  *SPECIFIED (2026-09, specification workstream): [specs/perception-intake.md](../../specs/perception-intake.md)
  rev 2 — source-domain artifacts (compositor owns display-time warp), dual-rate colour/geometry
  generation record covering every field above; §9 records the field-by-field disposition.*
- **#6 Pose-at-exposure API.** A concrete Monado API returning pose + source-clock mapping +
  interpolation status + uncertainty for a capture timestamp; characterized against hardware
  timestamps; a defined degraded mode (fixed-proxy) when unavailable. This is an **enable gate**.
- **#7 Recast ADR 0008 around invariants** — *wording recast DONE (2026-09-23, gap-closure
  workstream): ADR 0008 §Decision now states ownership + the two admissible boundary shapes in
  the ADR 0010 form (execution placement fixed at implementation against exactly one specified
  boundary), registry §10.3 resolved.* Still open from this item: the strawman framing of the
  rejected alternatives, and legal review of the actual GPL IPC/linkage (carried in M-18-style
  pre-release review).
- **#8 Transport is not just `wp_linux_drm_syncobj_v1`.** A Monado frame sink is not a Wayland
  surface; define either a real private Wayland client with surface commits, or an equivalent IPC
  carrying acquire/release timeline points, plus the non-blocking "latest signalled snapshot"
  selection and producer-death/timeline-reset behavior.
  *SPECIFIED (2026-09, specification workstream): [specs/perception-intake.md](../../specs/perception-intake.md)
  rev 2 — dedicated SEQPACKET protocol (not a Wayland client), registration-time image tables with
  per-image acquire/release timelines, GPU-safe reclamation gated on release-point completion,
  producer_epoch teardown; §9 records the disposition.*
- **#2 Per-client hand policy after a single resolve.** A global top layer can't honor per-client
  policy from `(C_scene, d_s)` alone (client identity is gone). For opaque T1, resolve an
  owner/policy ID alongside colour+depth, or apply hand policy per client before the final resolve;
  declare T2 (translucent) interaction unsupported until hand samples join the ordered representation.
- **#9 Monado reprojection semantics.** Specify the single projection layer's pose/time and final
  depth semantics. For the first prototype, disable `XR_KHR_composition_layer_depth` / runtime depth
  reprojection and measure rotational late-warp only; treat enabling depth reprojection as a gated
  step, not a no-depth-prototype blocker.
- **#10 Separate Vulkan devices / multi-GPU.** Require matching DRM render-node/device UUID for the
  zero-copy MVP; negotiate exact format/modifier/usage/handle tuples; define copy/blit fallback or
  reject mismatched devices. Record as an ADR 0008 consequence + qualification test.
- **#14 Contract surface.** For the prototype, expose only `enable`, `depthBackend = auto |
  classical | none`, `handCutout.enable`; move vendor backend selectors (`vk-qcom`/`adreno-dfs`/
  `hexagon`) and runtime `upperLimbVisibility` policy out of the device contract into device
  capability data / shell+protocol config. Add the missing **typed camera-array schema** under
  `spatial.adaptation.camera` (role/source, stereo sync group, format/rate, intrinsics/distortion,
  head↔camera extrinsics, calibration URI/version, timestamp clock, exposure/readout model, colour
  metadata, dmabuf caps, matte-source) and cross-field assertions (passthrough ⇒ Monado+zxr; hand
  cutout ⇒ passthrough). *(The current stubs are left as-is with this note; trimming happens when
  the services are built.)*

## Deferred to pre-release (per-device qualification / correctness before shipping)

- **#11 Rolling shutter + exposure/gain/WB data model.** Carry exposure interval, gain, WB/black
  level, sensor clock, readout direction/row time; correct or invalidate colour history across
  exposure changes; gate rolling-shutter devices on a row-pose warp or measured-acceptable evidence.
- **#12 Learned-depth roadmap wording.** Reframe XR-Stereo's "30 FPS on XR2" as a *reported,
  unreproduced* existence result (dataset-only release; TC-Stereo's temporal impl is CUDA-specific,
  no Qualcomm result). Require on-BSP export + full-delegation + sustained-thermal evidence before it
  is a milestone dependency. Keep all 2026 preprints + Hilbert encoding as experiments.
- **Non-blocking set:** minimum confidence contract (fabricated confidence = distinct lower-capability
  mode); measurable `latencyMode` semantics; per-artifact finite staleness bounds; monotonic snapshot
  IDs + restart/recalibration/crash recovery; colour-history anti-drift (the rounding-to-black
  failure); the client-interaction privacy analysis (joints vs mediated hit-test; no casual
  client-visible depth map; hands-present-bit observability); concurrent resource-contention
  qualification; guide-image-as-referenced-plane not copied payload; the Mercury-independent
  full-frame fallback cost/cadence budget.

## Preserved (review "things done well" — do not regress)

Non-blocking display-rate warp with decoupled camera/geometry/display cadence; pose-at-exposure
treated as blocking (not tuning); well-qualified research claims (Adreno DFS / QCOM / HTP / "12 ms"
never made mandatory); classical stereo as the portable fallback; the four-artifact
joints/mask/matte/F/depth distinction with Mercury optional; the client-declares-policy privacy
boundary; three-ages metrics with tail distributions.
