# Perception: passthrough view-correction and hand cutout

**Status:** design, extending [zxr-shell-v2-composition.md](zxr-shell-v2-composition.md).
**Date:** 2026-09-22. Synthesizes research docs [13-passthrough](../research/13-passthrough.md),
[14-mobile-stereo-depth](../research/14-mobile-stereo-depth.md),
[15-hand-segmentation-matting](../research/15-hand-segmentation-matting.md), and
[16-perception-claims-audit](../research/16-perception-claims-audit.md). Service placement is decided
in [adr/0008-perception-services-placement.md](adr/0008-perception-services-placement.md).

## The decomposition

"Passthrough with hand cutout and world mapping" is three problems in different architectural
places. This document covers the two that couple to the compositor; world mapping/anchors (SLAM,
planes, persistence) is a separate runtime-layer track and is out of scope here.

| Problem | What it is | Where it lives |
|---|---|---|
| **View correction (passthrough)** | reproject two cameras (displaced from the eyes, captured earlier) to each eye at display time, using a depth proxy | compositor **environment layer** (farthest contributor to the sort-last composition) |
| **Hand cutout** | a camera-aligned **alpha matte + hand depth**, composited over virtual content per policy | compositor **foreground top layer** + a per-client policy enum |

The unifying insight: **both are compositor-owned layers in the existing zxr-shell-v2 sort-last
pipeline** ([composition §2/§7.4](zxr-shell-v2-composition.md)), not new machinery. Passthrough
writes real colour + `gl_FragDepth` as the environment; the nearest-depth resolve then gives correct
per-pixel real/virtual occlusion **for free** — no new protocol, no vendor depth-test extension. The
hand matte is applied after that resolve as a policy-driven top layer. Neither is ever a *client*:
they are compositor-internal, like 2D-plane rasterization, so they are never subject to the client
composition cutoff.

### §1a. The cutout under the composition ruling (2026-09-26): hands above windows, shape open

[ADR 0006 amendment 2](adr/0006-compositor-strategy.md) makes 2D windows runtime quad layers
and zxr's projection layer conditional. **Ruled:** the cutout is a runtime layer submitted
*after every quad*, so hands composite above all windows — the mechanism is OpenXR's painter's
order (`rendering.adoc:1143-1147`) and Monado's per-layer source-alpha blending
(`render_gfx.c:409, 767-776`); its alpha is the matte, its colour the hand's passthrough
pixels. This holds regardless of depth (a window nearer than the hand still shows the hand),
which is the "your hands are always yours" posture visionOS takes at the system level
[external, mechanism only] and what `handCutout.upperLimbVisibility` already models. **Open —
the layer's shape, decider: the owner at the passthrough rung, on measured edge quality and
bandwidth:**

| shape | mechanism | cost per matte update (camera rate, hands in view) | fidelity | notes |
|---|---|---|---|---|
| (i) view-aligned cutout projection layer, submitted last | one RGBA image per eye, α = matte, colour = hand pixels, transparent elsewhere; may run at reduced resolution | a full-view store per eye: ~7 MB at 896×1007, ~28 MB at XR2-class; ÷4 at half resolution | exact by construction (view-aligned, any hand pose) | simplest; the projection-path bandwidth but only at camera rate and only with hands in view |
| (ii) one billboard quad per hand, submitted last | a small swapchain (≈ 384²) per hand at the hand's depth, facing the viewer, covering its projected bounding box; matte reprojected from the camera onto the plane | ≈ 0.6 MB per hand; zero with no hand in view | approximate at the hand's edges: a hand is 10–20 cm deep at 40–60 cm, the billboard is flat, so the runtime's reprojection under head motion between camera and display is slightly off at the silhouette | the efficient shape; measure the silhouette error before adopting |
| (iii) depth-correct ordering | windows the hand intersects are drawn in zxr's projection layer with the cutout, so a nearer window can hide the hand | those windows' render every frame | depth-correct | not what the ruling asks for (hands above, always); recorded because it is the only way to get occlusion *by* a window on Monado, which never depth-tests across layers |

No open-source XR compositor implements any of these (research/62 §3.5); the evidence is the
compositing facts above and Mura's own perception research (13, 15). My read, labelled as
such: (i) at reduced resolution first, (ii) once the matte pipeline exists and the silhouette
error can be measured.

The two are coupled through depth: **hands are simultaneously the hardest passthrough depth (nearest,
fastest, low-texture — reprojection error scales `≈ f·t·δZ/Z²`, so ~3 px of stereo-inconsistent swim
at 30 cm for a 1 cm error, invisible at 3 m) and the object of the cutout.** So the hand matte does
double duty: alpha for MR compositing *and* a mask giving hands their own geometry/latency policy
inside the passthrough warp. That coupling is why they share one research effort and one service
(ADR 0008); world mapping does not couple this way and is separable.

## The shared production architecture (settled)

Every deployed system converges on one shape ([13 §2](../research/13-passthrough.md): Passthrough+,
Quest Pro/3, Vision Pro, Play For Dream), differing only in the depth estimator:

> **Low-resolution, low-rate, temporally-fused geometry + full-resolution colour sampled at display
> rate through that geometry, with only the final per-eye warp on the display's critical path.**

Three decoupled rates, and the asymmetry that justifies the whole structure:

1. **Camera cadence** (~30 Hz) — the floor on colour freshness.
2. **Geometry cadence** (≤ camera; often lower) — depth solve. Passthrough+ measured photon-to-**geometry** 62 ms and tolerated >100 ms in prototypes.
3. **Display cadence** (72–120 Hz) — the per-eye warp runs here **and only here**.

Passthrough+ measured photon-to-**texture** 49 ms vs photon-to-**geometry** 62 ms and stated colour
freshness matters ~2× more than geometry freshness. Two hard rules fall out, both already required
of zxr-shell-v2 clients and now imposed on perception too:

- **Never block the display path.** A slow depth or matte update must never stall scanout; the warp
  reads the last *complete* snapshot and re-derives from the current head pose.
- **Latency beats cleanliness.** Three independent Play For Dream reviewers preferred its noisier
  14 ms mode over the cleaner 40 ms one. The quality knob lives on the geometry pipeline, never the
  display path; the default favours latency.

## Passthrough pipeline

The concrete implementable reference is Rectus (`references/openxr-steamvr-passthrough`), whose
shaders and reconstruction path [13 §3](../research/13-passthrough.md) documents line-by-line. Its
one load-bearing structural lesson is the **two-pass split**:

```mermaid
flowchart LR
    cams["2 cameras (full-res colour kept as texture; exposure timestamped)"] --> geo
    subgraph geobox [geometry thread - low res, low rate]
        geo["depth backend -> disparity + confidence"] --> temporal["confidence-gated reprojected temporal filter"]
        temporal --> upsample["joint-bilateral upsample + edge snap"]
    end
    upsample --> fwd["forward pass: grid mesh -> eye-space DEPTH buffer"]
    fwd --> bwd["backward fullscreen pass: eye depth -> world -> camera UV -> sample ORIGINAL colour"]
    bwd --> env["environment colour+depth in sort-last composition"]
```

- **Forward pass** rasterizes a grid mesh to turn camera-space disparity into an **eye-space depth
  buffer** (hardware depth test resolves visibility). **Backward fullscreen pass** reads that eye
  depth, reconstructs the world point, projects into the source camera, and samples the **original**
  (never re-rectified) camera colour. This decouples geometry resolution from colour resolution: one
  colour resample, full-res detail, no splat holes, no triangle stretching.
- **Rectify only the matching input; correct UVs on the display path** via a baked per-pixel offset
  map. The colour that reaches the eye is resampled once.
- **Same-side camera → same-side eye** is the baseline (shortest warp per eye, full FOV coverage,
  preserved parallax). Cross-camera gap-fill is a per-device refinement, default off, gated on
  geometric agreement + mutual confidence ([13 §6](../research/13-passthrough.md)).
- A **bounded proxy surface** (fixed-depth cylinder/hemisphere) is only the backstop behind gaps,
  never the primary geometry — a fixed radius mis-registers near content by degrees.

### Anti-wobble (the actual quality battle)

"Wobble" is a temporal artifact of spatial depth error; the corpus spends more code here than on
accuracy ([13 §4](../research/13-passthrough.md)). Mura adopts the full set: confidence-gated
**reprojected** temporal depth filtering with geometric rejection **and depth-dependent history
length** (near hands: short memory ≤0.2 s; far walls: long); joint-bilateral upsampling guided by
full-res luma with taps rejected across depth edges; Sobel depth-discontinuity **snapping** so
silhouettes are hard (a soft edge swims); **never rasterize a triangle across a depth
discontinuity**; solve on **inverse depth**; bounded priors (treat unknown as *far*, floor clamp,
backstop surface).

### Hole-fill priority

Three hole kinds, three answers ([13 §5](../research/13-passthrough.md)): (1) other camera first
(real data from a valid viewpoint), (2) validated reprojected history (stale but real, good for
static backgrounds), (3) cheap weighted-convolution diffusion inpaint (viewcorrection's kernel; the
Huber-TV solver is the offline quality reference, too expensive for-frame), (4) bounded backstop.
Never hallucinate detail — both NeuralPassthrough and viewcorrection reject inpainting-with-detail
for temporal-flicker reasons.

## Depth backend (pluggable)

Depth is *which estimator*, behind a stable interface the compositor consumes. The interface
(`DepthFrame`, [14 §5](../research/14-mobile-stereo-depth.md); == the passthrough doc's
`DepthSnapshot`) is the contract:

- per-view **disparity/depth with a declared encoding** (`FLOAT` | `FIXED_16(frac)` |
  `HILBERT8(order)` — the two-channel Hilbert encoding is first-class so a W8A8 Hexagon backend
  needn't dequantize on CPU);
- **confidence + validity always present** (measured / temporally-propagated / completed / hole) —
  every anti-wobble stage is gated on it; a backend that can't produce it forces a fabricated one;
- **capture (mid-exposure) timestamp**, in the pose clock, not publish time;
- **calibration version** (droppable atomically on IPD/thermal recalibration);
- intrinsics + baseline + rectified pose per view (enough to build `Q`); a full-res rectified luma
  **guide image** for the joint-bilateral upsample.

Four interchangeable backends, all **gated on BSP inspection** per the claims audit
([16 Part 1](../research/16-perception-claims-audit.md)):

| Backend | Status | Role |
|---|---|---|
| **A. Classical GPU stereo** (Vulkan compute: census/SAD → aggregate → subpixel → confidence; or OpenCV SGBM day-1) | available, depends only on standard Vulkan + camera access | **the baseline and fallback** |
| **B. `VK_QCOM_image_processing2/3` block-match** | potentially valuable, unverified on XR2+ (Adreno 7xx drivers may omit it) | accelerate correspondence *with identical compute fallback* |
| **C. Adreno Depth-From-Stereo** (CVP DFS engine, "<1 ms") | real component, access **unverified** — vendor/OEM-negotiated plugin, do not infer from an Adreno GPU | drop-in hardware backend if exposed |
| **D. Hexagon HTP learned stereo** (TC-Stereo-/LightStereo-class via ONNX→QNN or ExecuTorch `.pte`) | documented on embedded Linux, BSP+model-gated (INT8, static shapes, memory) | quality backend where NPU headroom exists |

Adopt **temporal stereo** (TC-Stereo's carry-disparity+GRU-state-forward, proven 30 FPS on XR2 by
XR-Stereo — verified dataset-only release, paper-backed) as the phase-2 recipe. **Teacher–student**
is the shipping-weights workflow: an MIT teacher (RAFT-Stereo) run offline over headset-camera
captures pseudo-labels a small on-device student — which also sidesteps Fast-FoundationStereo's
non-commercial license. Evaluate every backend by **rendered eye-view error and temporal jitter on
fixed captured sequences**, never a disparity screenshot.

## Hand cutout

Four distinct artifacts, routinely conflated ([15 §1](../research/15-hand-segmentation-matting.md)):
**joints** (Mercury, 26/hand — a *prior*, not a cutout), **segmentation mask** (binary/semantic),
**alpha matte** (`I = αF + (1−α)B`; the compositor needs α *and* a foreground estimate F, because
reusing the camera pixel as F bleeds the real background into a halo at soft edges), **hand depth**
(for reprojection + `.automatic` ordering). The compositor consumes **matte (premultiplied αF, α) +
hand depth, aligned to one capture timestamp**.

Critical correction, cited from source: Monado's `xrt_hand_masks_sample` is **bounding boxes** (one
`xrt_rect_f32` per hand per view, consumed only by SLAM to *ignore* features on hands) — finding
"hand masks" in Monado does not mean a cutout exists.

Mercury's joints feed the cutout as priors (ROI crop → run the net on ~256² crops not full views;
21 projected 2D joints as a trimap/skeleton seed; capsule radii + the 22-bin depth head as a depth
prior; motion/handedness gating to reset recurrent state) — **but the cutout must be a parallel
camera-frame service that runs without Mercury**, because Mercury deliberately drops tracking exactly
when hands overlap (the visually most important moment).

Prototype path, each tier ships something:

- **Tier 0 — policy plumbing + geometry mask.** Add the upper-limb policy enum to the protocol and
  the compositor top-layer pass; render Mercury capsules per eye for a hard α and hand depth. No ML,
  no camera pixels. Validates the whole contract; remains the permanent fallback. (This is Meta's
  `HandsRemoval` geometry-mask approach.)
- **Tier 1 — Mercury-seeded greyscale segmentation.** A reimplemented Ego2Hands-CSM-class small net
  (1–2 ch grey+Canny, 3 classes + energy head — Ego2Hands is greyscale-native, matching monochrome
  tracking cameras) on Mercury ROI crops; binary mask → guided-filter feather as provisional α.
- **Tier 2 — recurrent matte.** RVM-style ConvGRU decoder + deep-guided-filter, predicting α **and**
  F-residual, with a Mercury-prior input channel; reset recurrent state on Mercury appearance
  events. Removes the halo and temporal flicker.
- **Depth refine.** Stereo patch refinement inside the matte ROI, seeded by capsule depth; keep a
  `smoothstep` fade for `.automatic` (a hard z-test on noisy hand depth flickers at contact).

**Composition policy** (the visionOS contract, verified: premultiplied alpha, reverse-Z depth
submission, `.visible`/`.hidden`/`.automatic`): a **per-spatial-client policy attribute** in the zxr
protocol, shell-owned default `automatic`.

**The correctness subtlety (raised by the review, corrected here):** the passthrough environment
layer already contains the real hand pixels, so a hand *top* layer alone cannot hide them. The hand
must be the **sole owner of hand-region pixels** — the passthrough environment has the hand region
**removed** (hand colour and depth excluded; the background behind held open / reconstructed, exactly
what Meta's `HandsRemoval` does, [15 §6](../research/15-hand-segmentation-matting.md)) and the hand
layer re-composites `αF_hand` per policy. Then `hidden` yields virtual content with no hand;
`visible`/`automatic` add the hand back. This is another reason passthrough and the hand cutout are
**one coupled service** (removing the hand from the environment needs the matte).

All depth comparisons use one **canonical quantity — positive linear eye-space metres** (nearer =
smaller), with one tested conversion from *each* producer (capsule/stereo metric `d_h`, and the
environment's own depth `d_s`), never raw reverse-Z or disparity; the reverse-Z / near-far conversion
to the shared depth attachment is a separate, tested step ([composition §2](zxr-shell-v2-composition.md)
depth-meaning contract).

```
resolve clients, hand region EXCLUDED from the passthrough environment -> (C_scene, d_s)
  # d_s, d_h both in positive eye-space metres
per eye: warp (αF_hand, α, d_h) camera(t_capture) -> eye(t_display)   # matte+F+depth: ONE atomic unit, one warp
  visible:   C = αF_hand + (1-α)·C_scene
  hidden:    C = C_scene                                    # hand absent from C_scene by construction
  automatic: occ = smoothstep(-ε, +ε, d_s - d_h)            # d_s-d_h>0  <=>  hand nearer than scene -> occ->1
             α' = α·(1 - fade·(1 - occ))                    # occ=1: full hand; occ=0: faded ghost (Apple's "fade as it goes behind")
             C = α'·F_hand + (1-α')·C_scene
```

Clients declare policy only — never see the matte or camera frames (a privacy boundary; hand images
are biometric-adjacent), exactly as they declare bounds. The hand layer obeys the client deadline
rule: no fresh matte at cutoff → late-warp the previous one by head-pose delta; past a staleness
bound, **the environment can no longer have the hand removed (removal needs the matte), so the hand
degrades to plain uncut passthrough at scene depth** (effectively `visible`-without-policy), never a
stall.

**Licensing** ([15 §7](../research/15-hand-segmentation-matting.md)): every path to shippable weights
runs through **data we generate ourselves** (device captures + composited synthetic hands +
teacher pseudo-labels on our own footage). Ego2Hands (NC dataset, no code license) is
evaluation-only; RVM (GPL-3.0) informs the architecture and, if used as-is, runs as a separate
process; EgoHOS (MIT code, upstream-dataset caveats) is a teacher. All recorded in the device/build
contract where a shipping decision would hit them.

## The shared invariants (both layers)

1. **Timestamp discipline — two contracts, not one** (corrected per review). The **hand matte is
   strictly atomic**: `αF`, α, hand depth, and the source colour it cuts from share one
   camera-exposure timestamp — a perfect matte on the wrong frame is a wrong cutout. **Passthrough
   is deliberately *not* atomic across colour and geometry** — the three-rate architecture serves
   fresher colour than geometry — so it carries **both `t_colour` and `t_geometry`** with their
   respective poses/calibration versions and the transform that aligns geometry to colour. Never
   describe the two cases with a single "same-timestamp" rule.
2. **Pose-at-exposure.** Every camera frame carries its mid-exposure timestamp; the pose is *queried
   for that timestamp* (Passthrough+ saw visible float from a 4 ms timestamp error). If Monado's
   pose query can't serve arbitrary timestamps at the needed rate/accuracy, that is a **blocking
   dependency**, not a detail.
3. **Non-blocking publication.** Both services publish latest-complete; the display path never awaits.
4. **Calibration-versioned; per-device geometry is a contract input.** Camera↔eye offset, baseline,
   camera-vs-display FOV, rolling-shutter, IR illumination, and which camera is the matte source all
   belong in the typed `mura.*` device contract, not in compositor constants.

## Contract surface (additions)

Minimal typed options under `mura.xr.passthrough.*`, consistent with this design (stubs now;
semantics tracked in the design backlog):

- `enable` (bool), `latencyMode` (enum `low-latency` | `high-quality`, default `low-latency`),
- `depthBackend` (enum `classical` | `vk-qcom` | `adreno-dfs` | `hexagon` | `none`, default
  `classical` — the BSP-independent baseline),
- `handCutout.enable` (bool) and `handCutout.upperLimbVisibility` (enum `visible` | `hidden` |
  `automatic`, default `automatic`) — the per-client-overridable shell default.

Per-device camera geometry (offset, baseline, FOV, matte-source camera, IR/exposure notes) extends
`mura.adaptation.camera` / the device contract.

## Metrics and sequencing

Measure the **three ages separately** ([13 §8](../research/13-passthrough.md)): capture-to-display
(colour freshness), geometry age, late-pose response — a single "latency" number hides which is
broken. Report distributions (p90/p99), not means. Test scenes target specific failures: hands at
20–40 cm (still + fast), reach-and-grasp, printed text at 30/60/200 cm, thin structures, low light,
fast lateral head translation, textureless walls, mixed real/virtual occlusion, and a depth-source
outage (must degrade to stale-geometry warp, never stall).

Passthrough milestones P0–P6 ([13 §7.4](../research/13-passthrough.md)) and hand tiers 0–2 above
sequence independently but share the perception service and the camera pipeline. Both are gated
behind the display-path feasibility work and the BSP-access unknowns the claims audit enumerated.

## Open questions (carried to ADR 0008 and the backlog)

Where the perception services live (Monado-side vs compositor-internal vs sibling) and the dmabuf +
explicit-sync delivery interface — decided in [ADR 0008](adr/0008-perception-services-placement.md).
Plus: geometry resolution/topology (per-camera grid vs head hemisphere); whether a full-res per-eye
environment depth target is affordable; depth-dependent thresholds (`Z²` scaling nobody implements);
passthrough-depth vs Monado reprojection interaction (double-warp risk); the matte-source camera per
device; upper-limb scope (forearm/sleeve); held objects; two-hands-interlocked degradation; and the
BSP questions from [16 Part 1](../research/16-perception-claims-audit.md) (camera zero-copy, Adreno
DFS/HTP availability, the "12 ms mode" endpoints).
