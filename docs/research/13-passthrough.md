# 11 — Camera passthrough view-correction (video see-through) for zxr-shell-v2

**Status:** research input for the perception architecture. **Date:** 2026-09-22.
**Scope:** how to turn two front-camera images, captured earlier in time from the wrong place, into
per-eye colour **and depth** at display time, as the *environment layer* of the sort-last
composition model in [architecture/zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md).
**Out of scope (sibling doc):** *which* depth backend to use. This doc only fixes the interface
passthrough needs from it (§9.1).

**Evidence legend.** `[V]` verified by reading the cited code or primary source; `[R]` reported by
a primary source (paper/vendor) but not independently verified here; `[I]` inference or design
judgement by this document. Code citations are `path:line` relative to the reference clone root
under `references/`.

---

## 1. The problem, precisely

### 1.1 It is reprojection, not stitching

Two cameras sit on the front shell, displaced from the eyes in all three axes (several cm forward
and outward), and expose 10–50 ms before the photons representing them leave the display. The task
is **novel-view synthesis of a dynamic scene from two images**, where the target views are known
exactly and the viewpoint change is small but not negligible.

It is not panoramic stitching. Stitching produces one image on a shared surface; passthrough must
produce **two different images**, because the stereo disparity between them *is* the depth cue the
visual system will fuse. Passthrough+ makes this explicit: left camera textures left eye, right
textures right eye, so each image is warped over the *shorter* baseline
(camera-to-same-side-eye), and this also "fills up the entire 180° field of view around the user;
which would not be possible using the same image to texture the mesh for both eyes" `[V]`
(Chaurasia et al. 2020 §3.4).

### 1.2 The geometry

For target eye `e` (view matrix `V_e`, projection `P_e`) and source camera `Ci` (intrinsics `K_i`,
pose `T_{W←Ci}` **at its exposure time**), the mapping for a pixel is:

```
1. pick a scene point along the eye ray:   X_e  = Z_e · K_e⁻¹ · u_e          (u_e = eye pixel, homog.)
2. to world:                                X_W  = T_{W←e}(t_display) · X_e
3. to the source camera at capture time:    X_Ci = T_{W←Ci}(t_expose)⁻¹ · X_W
4. project:                                 u_Ci = distort_i( K_i · X_Ci )
5. sample the ORIGINAL (undistorted-in-lookup, not resampled) camera image at u_Ci
```

Two dual formulations exist and both appear in the reference implementations:

- **Backward / gather (target-driven).** Rasterize the eye view, and for each eye pixel with a
  hypothesized depth, run steps 1–5. Needs depth *in the eye view*. This is
  `fullscreen_passthrough_ps.hlsl` — clip→world at `:38-41`, world→camera-frame at `:43`,
  homogeneous→UV at `:53`, fisheye lookup at `:64-71`, colour fetch at `:80` `[V]`.
- **Forward / scatter (source-driven).** Take the depth *in the camera view*, build geometry from
  it, and rasterize/splat that geometry into the eye view. This is Passthrough+'s hemispherical
  mesh `[R]`, viewcorrection's `MeshDepthmapCUDA` (`viewcorrection/src/view_correction/view_correction_display.cu:121-208`)
  `[V]`, and NeuralPassthrough's softmax splatting (`neuralpassthrough/code/utils.py:88-133`) `[V]`.

Rectus uses *both*, in sequence, and that is the single most important structural lesson in the
whole corpus: a **forward pass converts camera-space disparity into an eye-space depth buffer**
(`passthrough_renderer_dx11.cpp:1812-1958`, comment at `:1812`: "Reprojects one or two disparity
maps into HMD projection space depth maps"), then a **backward fullscreen pass consumes that eye
depth buffer and samples the original colour** (`:2495-2646`, shader `fullscreen_passthrough_ps.hlsl`)
`[V]`. Forward gives correct visibility resolution (a real depth test picks the nearest surface);
backward gives full-resolution colour with no splat holes and no triangle stretching in colour
space. Splitting them decouples geometry resolution from colour resolution — which is exactly what
§2 says every production system does.

### 1.3 Why a fixed proxy surface fails

If you assume a constant radius `R` instead of the true depth `Z`, the angular error of a feature
is `δθ ≈ t · (1/Z − 1/R)` for camera-to-eye offset `t`. For `t = 3 cm`, `R = 2 m` and a hand at
`Z = 30 cm`: `δθ ≈ 0.03 · (3.33 − 0.5) = 85 mrad ≈ 4.9°`. At `f = 900 px` that is **~76 px** of
misregistration between the eye's expected ray and the sampled ray — and, worse, a *different*
error in each eye, so the stereo pair does not fuse. Chaurasia et al. state the consequence
bluntly: "Warping the images from the cameras to the user's eye positions using static geometry
like a fixed plane or hemisphere without reconstructing 3D geometry is known to lead to instant
motion sickness" `[V]` (§1).

Rectus keeps a fixed-proxy mode (`Projection_RoomView2D` / the cylinder mesh,
`mesh.cpp:13-45`, used at `passthrough_renderer_dx11.cpp:1533`, `:2636-2638`) but only as a
fallback and as the **background filler behind reconstruction gaps** (`RenderBackgroundForView`,
`:2651-2710`, drawn with a depth bias) `[V]`. That is the right role for a proxy surface: a
guaranteed-finite backstop, never the primary geometry.

### 1.4 Why near-field depth error dominates

Differentiating the disparity relation `u = f·t/Z`:

```
|δu| ≈ f · t · δZ / Z²          (pixels)
|δθ| ≈     t · δZ / Z²          (radians, resolution-independent)
```

The `1/Z²` is the whole story. Worked examples, `t = 3 cm` (camera↔eye offset), `f = 900 px`:

| Scene depth `Z` | Depth error `δZ` | `δθ` | `δu` @ f=900 px |
|---|---|---|---|
| 30 cm | 1 cm | 3.3 mrad = 0.19° = 11 arcmin | **3.0 px** |
| 30 cm | 5 cm (measured Passthrough+ error at this range) | 16.7 mrad = 0.95° | 15 px |
| 1 m | 1 cm | 0.3 mrad = 0.017° | 0.27 px |
| 3 m | 1 cm | 0.033 mrad | 0.03 px |

So a 1 cm depth error is invisible at 3 m and a ~3 px stereo-inconsistent shift at 30 cm — right in
the hand-interaction band. Passthrough+ measured 5–10 cm absolute depth error for most scene
content with *higher* error below 1 m (their stereo is biased to small disparities), and named this
their most important limitation: "the mesh can be inaccurate for nearby objects and does not align
with object boundaries, as the depth estimation around 30 cm becomes inaccurate … This can cause
object boundaries to occasionally wobble and limit stereoscopic viewing comfort for very close-by
objects" `[V]` (§6.1, §7).

Two corollaries the perception architecture must internalize:

1. **Depth accuracy requirements are non-uniform and should be spent near-field.** Uniform-quality
   depth is the wrong optimization target. Rectus exposes exactly this control surface:
   `StereoMaxDisparity = 96` at `StereoDownscaleFactor = 3`
   (`shared/config_manager.h:642`, `:616`) sets the *near* limit of what can be matched at all `[V]`.
2. **Temporal instability of depth is worse than static bias.** A static 2 cm bias at 40 cm reads
   as "the desk is slightly closer than it is". A 2 cm *jitter* at 40 cm reads as boiling geometry.
   Every system in the corpus spends more code on temporal stabilization than on accuracy.

### 1.5 The second irreducible problem: stereo-inconsistent texture

With left-camera→left-eye texturing, a single mesh fragment can receive different colour in each
eye. Chaurasia et al.: "occasionally, each eye views a different texture on the same fragment of 3D
geometry, which the visual system cannot merge. We observed that this is noticeable only for
objects around 30 cm" `[V]` (§3.4). This is *not* a depth-accuracy bug; it is intrinsic to
view-dependent texturing over a wrong proxy. It is why cross-camera blending (§6) has to be
depth-gated rather than always-on.

---

## 2. The shared production architecture

Every deployed system in the corpus — Passthrough+ on Quest, Quest Pro, Quest 3, Apple Vision Pro,
Play For Dream, and the research designs (NeuralPassthrough, viewcorrection) — is the **same
architecture**, with different depth backends bolted into the same socket:

> **Low-resolution, low-rate, temporally-fused geometry + full-resolution colour sampled at display
> rate through that geometry, with the final per-eye warp on the display's critical path and
> nothing else on it.**

Evidence, per system:

| System | Geometry | Geometry rate | Colour/warp rate | Source |
|---|---|---|---|---|
| Passthrough+ (Quest) | 70×70 hemispherical inverse-depth grid, 300–1200 stereo points, Laplace-completed | 30 Hz (camera) | 72 Hz (display), vertex-shader warp | `[V]` Chaurasia §3.2, §3.4, §4, §5 |
| Quest Pro | ≤10 000 points/frame to 5 m → 3D mesh, "combining this output across a few frames" | camera rate | display rate, "individually warped to the headset's left and right eye views using our Asynchronous TimeWarp" | `[V]` [Meta blog](https://www.meta.com/blog/mixed-reality-definition-passthrough-scene-understanding-spatial-anchors/) |
| Quest 3 | ML depth; "Most of the 3D geometry improvements in Passthrough come from AI" — architecture undisclosed | undisclosed | display rate via ATW | `[V]` [Meta blog](https://www.meta.com/blog/ai-powered-technologies-quest-3-pro-ray-ban-meta-smart-glasses/) |
| Apple Vision Pro | warp *meshes* (vertex grids); separate passthrough-warp and virtual-warp blocks feeding a merging compositor; POV correction | undisclosed | R1 "streams new images to the displays within 12 milliseconds" | `[V]` [US20250123490A1](https://patents.google.com/patent/US20250123490A1/en); [Apple newsroom](https://www.apple.com/newsroom/2023/06/introducing-apple-vision-pro/) |
| NeuralPassthrough | per-input-view RAFT-Stereo depth | proposed 30 Hz | proposed 72 Hz (splat+filter+fusion = 7.3 ms of the 32 ms total) | `[V]` Xiao et al. §3.3 |
| Rectus | SGBM disparity at 1/3 res on a CPU thread | camera rate, skippable | every app frame; mesh + fullscreen pass | `[V]` `depth_reconstruction.cpp:322-950` |
| viewcorrection | meshed inpainted source depth map | input rate | target-view render rate | `[V]` `view_correction_display.cc:1769` |

### 2.1 The three decoupled rates

1. **Camera cadence** — when photons were collected. Quest: 30 Hz `[V]`. NeuralPassthrough
   prototype: 30 Hz at 1280×720, 90° FOV `[V]`. This is set by the sensor and is the *floor* on
   colour freshness. You cannot beat it; you can only avoid adding to it.
2. **Geometry cadence** — when depth was last solved. Always ≤ camera cadence, often lower
   (Rectus has an explicit `StereoFrameSkip`, `config_manager.h:615`, enforced at
   `depth_reconstruction.cpp:415`) `[V]`.
3. **Display cadence** — when the eye buffers must exist. 72–120 Hz. **The per-eye warp must run
   here and only here.**

Passthrough+ quantifies the resulting latencies: **photon-to-texture 49 ms, photon-to-geometry
62 ms** `[V]` (§5). And critically: "photon-to-geometry latency requirements seemed less critical;
some of our early prototypes had this latency above 100 ms without too much additional discomfort",
whereas photon-to-texture should be "as close as possible to 33 ms … a latency higher than around
100 ms can cause noticeable judder" `[V]`. **Colour freshness matters ~2× more than geometry
freshness.** This single asymmetry justifies the whole architecture.

### 2.2 The non-negotiable scheduling rule

Passthrough+: "An important consideration is to never block the render thread for a long (>4 ms) or
non-deterministic period. Therefore, we only perform the non-blocking fast operations on this
thread … to fit well within the 14 ms between two successive display refresh events" `[V]` (§4).
Their compute thread is double-buffered; the render thread *polls* and reuses the previous depth
map if nothing new is ready `[V]`.

Rectus implements the same separation but gets the boundary subtly wrong in one place: its async
disparity-filter submit ends in a blocking `vkWaitForFences(..., 100 ms)`
(`async_renderer.cpp:1377-1385`) — acceptable only because that call happens on the
`DepthReconstruction` worker thread (`depth_reconstruction.cpp:752, 857`), not the render thread
`[V]`. This is precisely the pattern the zxr-shell-v2 scheduling rule already demands of 3D clients
("never enqueue an unsignaled client dependency onto the headset's critical path",
[zxr-shell-v2-composition.md §7.4](../architecture/zxr-shell-v2-composition.md)); passthrough must
obey it too `[I]`.

### 2.3 Pose accuracy at exposure time is a first-class requirement

Passthrough+: "We observed that an error of even **4 ms** in capture timestamps was enough to cause
visibly floating and lagging rendering. The SLAM system must be capable of delivering device pose
at such a high frame rate and also with high accuracy" `[V]` (§4).

Rectus does this correctly: the camera frame carries `ulFrameExposureTime`
(`camera_manager_openvr.cpp:661`), the HMD pose is *queried for that timestamp*
(`:666`, `GetHMDPoseForTime`), and camera view-to-world is `headToTracking(t_expose) × cameraToHMD`
(`:673-678`) `[V]`. Its render models are likewise posed at an exposure-relative time
(`passthrough_system.cpp:1124-1128`) `[V]`. Its own readme names the failure mode for the path that
*cannot* do this: "USB webcam frames can not be accurately timed. This will cause hitching in the
image, especially if the frame rate jitters" `[V]` (`readme.md`, Limitations).

### 2.4 Latency beats cleanliness (perceptual, not theoretical)

Play For Dream MR ships two passthrough modes: ~14 ms low-latency and a cleaner ~40 ms
high-quality. Two independent reviewers preferred the **noisier, lower-latency** one `[R]`:

- "the low-latency mode leaves a bit of visible noise … This disappears in the high-quality mode,
  but latency worsens noticeably to 40 ms … the low-latency, visual-experience-first mode felt the
  most natural, and honestly the noise didn't bother me much."
  ([review](https://note.com/fleabaneh/n/n07220438ab1f))
- "I found the high quality passthrough mode actually looked a lot worse … (I feel that the HQ mode
  tries way too hard to over-correct some dark areas, to the point it creates a weird geometric
  noise)."
  ([review](https://ramipastrami.engineering/play-for-dream-mr-review-potentially-the-best-4k-standalone-vr-headset-yet/))

A third hands-on independently reports the 14 ms figure and the "grain"
([The Ghost Howls](https://skarredghost.com/2025/09/11/play-for-dream-hands-on-review/)) `[R]`.
Corroborated from inside Meta: "mathematically optimum points don't necessarily mean perceptual
optimums" — Ricardo Silveira Cabral `[V]`
([Meta blog](https://www.meta.com/blog/ai-powered-technologies-quest-3-pro-ray-ban-meta-smart-glasses/)).

**Consequence:** the quality knob belongs on the *geometry* pipeline, never on the display path,
and the default must favour latency `[I]`.

---

## 3. Rectus deep-dive: `openxr-steamvr-passthrough`

The concrete implementable reference. MIT-licensed, Windows/SteamVR/D3D11-oriented, with a Vulkan
path for the async compute filters. Below, every stage with file:line.

### 3.0 Pipeline at a glance

```
CPU worker thread (depth_reconstruction.cpp:322 RunThread)
  acquire CPU camera frame (+ exposure timestamp + camera poses at exposure)     :357, :422-424
  decode raw format (RGBX/RGB24/YUYV/NV12/BAYER16/MJPEG)                         :427-557
  cv::remap through precomputed rectification maps                               :587-588
  cv::resize to 1/DownscaleFactor                                                :592-593
  pad left edge by numDisparities into "extended" frames                         :597-598
  StereoSGBM::compute (left; optionally right for both-eye disparity)            :603-626
  ximgproc WLS disparity filter + confidence map                                 :644-691
  pack L|R disparity side-by-side (right negated), pack confidence, upload       :760-831
  upload B&W rectified guide image (for joint bilateral)                          :834-855
  async GPU compute (async_renderer.cpp:1317-1358): fill-holes ×7 → joint bilateral
  publish DepthFrame (4-deep queue) with DisparityToDepth, view→world, timestamps :694-859

Per-app-frame, on the app's device/queue (passthrough_renderer_dx11.cpp:1299 RenderPassthroughFrame)
  per eye:
    RenderDepthPrepassView   :1812   grid mesh + passthrough_stereo_vs → eye-space DEPTH + validity
      (optionally twice: primary camera, then cross camera into a second depth target)
    RenderSetupView          :1694   per-view constants incl. prev-HMD-frame + colour-history matrices
    [RenderAlphaPrepassView / masked / alpha-test prepasses]      :2203 / :1963 / :2336
    RenderPassthroughView    :2495   fullscreen triangle; reads eye depth → world → camera UV → COLOUR
    RenderBackgroundForView  :2651   depth-biased cylinder behind any remaining gaps
```

### 3.1 Calibration & rectification (`depth_reconstruction.cpp:79-208`) `[V]`

- Intrinsics per eye from the camera manager (`:88-107`); 4-coefficient distortion (`:109-120`).
- Extrinsics as a single left→right rigid transform, basis-changed into OpenCV convention
  (`:123-133`).
- `cv::fisheye::stereoRectify` + `fisheye::initUndistortRectifyMap` for fisheye models
  (`:139-152`); `cv::stereoRectify` + `initUndistortRectifyMap` otherwise (`:156-174`), both with
  `CALIB_ZERO_DISPARITY`.
- The reprojection matrix `Q` is transposed and kept as `m_disparityToDepth` (`:183-184`) — this is
  the one matrix that turns `(u, v, disparity)` into a metric camera-space point.
- The rectified rotations `R1`, `R2` are kept so the disparity frame can be mapped back to world:
  `DisparityViewToWorld = ViewToWorld(t_expose) × R^T` (`:707-719`).
- `CreateDistortionMap` (`:211-316`) bakes a **per-pixel UV offset texture** (rectified position →
  distorted position, normalized) covering both halves of the stereo frame. It also rewrites `P1`
  into a clip-space projection with a `-1` w-row and near plane (`:225-234`).

**This is the key trick for colour quality.** Rectification is never applied to the colour image
that reaches the eye. Only the low-res *matching* input is remapped and downscaled; the display
path samples the **original distorted camera texture** and applies the offset map as a final UV
correction (`fullscreen_passthrough_ps.hlsl:64-71`, `passthrough_ps.hlsl:51-59`) `[V]`. One
resampling, not two. Directly transferable.

### 3.2 Stereo matching + WLS (`depth_reconstruction.cpp:600-691`) `[V]`

- `cv::StereoSGBM::create(minDisparity, numDisparities, blockSize, P1·bs², P2·bs², dispMaxDiff,
  preFilterCap, uniquenessRatio, speckleWindowSize, speckleRange, mode)` at `:603-608`; computed on
  frames padded left by `numDisparities` so the search never runs off the image (`:597-598`).
- Optional second matcher for the right view (`:620-626`) — either a genuine reversed SGBM, or
  `ximgproc::createRightMatcher` when only needed for WLS (`:650-652`).
- `ximgproc::createDisparityWLSFilter` with `lambda = 8000`, `sigmaColor = 0.5`,
  `depthDiscontinuityRadius = ceil(0.5 · blockSize)` (`:657-664`, defaults
  `config_manager.h:652-655`). **`getConfidenceMap()` (`:677-686`) is the load-bearing output** —
  every downstream anti-wobble stage is gated on it.
- Fixed-point packing: disparity as `CV_16S`, and `DisparityToDepth.m[11] *= 2048.0f` because
  `65536/2/16 = 2048` gives 4 fractional bits in int16 (`:726-733`). Valid range truncated by
  `±4/2048` to survive the fractions (`:744-745`).
- Confidence rescaled `32768/255` into int16 (`:792`); zeroed when WLS is off (`:827-828`).

**Platform-neutral.** OpenCV + `ximgproc` build fine on Linux; this stage ports unchanged. But it
is CPU-heavy and its cost scales with `numDisparities × resolution` — which is why it runs on its
own thread at 1/3 resolution by default.

### 3.3 GPU disparity post-process (Vulkan compute) `[V]`

Dispatched from `async_renderer.cpp:1317-1358`, on a dedicated Vulkan device, writing a texture
shared into D3D11 (`passthrough_renderer_dx11.cpp:2896-2959`).

**`vulkan_fill_holes.comp.glsl`** — iterative nearest-valid propagation, run
`StereoFillHolesIterations` times (default **7**, `config_manager.h:613`), then once more in
"last pass" mode.

- Encoding trick: the confidence image doubles as scratch. An invalid pixel that finds a valid
  4-neighbour stores that neighbour's disparity as a **negative confidence** (`:100-115`), and
  subsequent iterations propagate the *largest* (i.e. closest-to-zero, i.e. nearest-surface)
  negative confidence outward (`:134-149`) `[V]`.
- Right-edge guard: disparities near the right edge are progressively rejected because the
  extended-frame search cannot support them (`:70-82`).
- The final pass (`:38-68`) resolves the encoding into a clean `(disparity, confidence)` pair,
  marking hole-filled pixels with `confidence = -1` so later stages can tell "filled" from
  "measured".

**`vulkan_joint_bilateral.comp.glsl`** — confidence- and discontinuity-aware **joint bilateral
upsample**, guided by the B&W rectified camera image. Off by default
(`config_manager.h:657`) but the highest-value shader in the repo.

- Specialization constants `g_minDisparity`, `g_maxDisparity`, `g_bUseInputConfidence`,
  `g_bilateralDispCutoff`, `g_bilateralDistance` (`:11-15`) — the kernel is recompiled, not
  branched.
- Precomputed separable-ish weight LUTs in a UBO: `lumaWeights[48]` indexed by
  `|Δluma|·255` clamped, and `spaceWeights[10][10]` storing **one quadrant** of the symmetric
  spatial kernel (`:21-25`, comment at `:20`) `[V]`.
- `ReadDisparity` / `ReadDisparityConfidence` (`:34-71`) decode the hole-fill encoding and — the
  important part — coerce *invalid* pixels to `g_minDisparity`, i.e. **treat unknown as far**, so
  they never pull a foreground surface backwards.
- `CalculatePixel` (`:74-112`) is the core tap:
  - reject samples that are invalid, or that came from hole-filling when the centre is genuinely
    measured (`:80`) — measured data always beats extrapolated data;
  - **discontinuity rejection** (`:86`): if `|centreDisp − sampleDisp| > g_bilateralDispCutoff`,
    the tap is dropped entirely, so the filter never averages across a depth edge;
  - **background detection** (`:92-95`): if the rejected neighbour is *nearer* than the centre by
    more than the cutoff, the centre is on the background side of an edge → `bCenterIsBackground`;
  - weight = `spaceWeights[|dy|][|dx|] · lumaWeights[|Δluma|]` (`:102-107`).
- **Nearest-neighbour edge snapping on upscale** (`:153-166`): when the output grid is finer than
  the disparity grid, the two nearest neighbours in the sub-pixel direction are probed; if both are
  valid and both differ from the centre by more than the cutoff, the centre is **snapped to
  `min(neighborH, neighborV)`** and its confidence zeroed. This is a crisp fix for the classic
  bilinear-upsample smear across a silhouette.
- Circular support: `radius = g_bilateralDistance`, inner loop bounded by
  `sqrt(r² − y²)`, all four quadrants sampled per `(x,y)` (`:179-196`).
- Output confidence (`:200`):
  `confidence = (valid && !background) ? min(totalWeight/numSamples, pixelConfidence) : 0`
  — i.e. confidence is *also* a measure of how many taps survived rejection. Pixels near
  discontinuities automatically get low confidence, which is what feeds the temporal filter's
  invalidation logic (§4.1).
- Output is `rg16_snorm`: `(disparity, confidence)` in one texture (`:30`, `:202`).

### 3.4 Forward pass: disparity → eye-space depth (`passthrough_stereo_vs.hlsl`) `[V]`

Rasterized over a grid mesh whose vertex count is tied to the depth-map resolution
(`GenerateDepthMesh`, `passthrough_renderer_dx11.cpp:1183-1207`, `:1430`; mesh built by
`mesh.cpp:49-95` `MeshCreateGrid` or `:98-151` `MeshCreateHexGrid`). `MeshCreateGrid` places
vertices at *pixel centres* with an extra clamped ring at the border, and stores a border marker in
`z` (`mesh.cpp:68-82`).

Per vertex:

1. Grid UV → disparity-texture UV via `g_disparityUVBounds` (`:32`), which selects the left or right
   half of the packed disparity image; sample `(disparity, confidence)` (`:35`).
2. Compute `minDisparity` / `defaultDisparity` from `g_projectionDistance` (`:37-42`): invalid
   vertices are pushed to a plausible 2 m default with `confidence = -10000` (`:54-59`).
3. Border-proximity rejection: vertices within `maxFilterWidth` of the image edge get
   `projectionConfidence = 0` to prevent filters sampling off-image (`:60-65`).
4. **Discontinuity block** (`:70-123`), when `g_bFindDiscontinuities`:
   - 8 taps at `g_cutoutFilterWidth` spacing (`:74-82`);
   - a **clamped Sobel** where every tap is `min(disparity, tap)` (`:84-88`, comment at `:83`:
     "Clamp the max disparity tested to the sampled pixel disparity in order to not filter
     foreground pixels") — so the edge response fires on the *background* side of a silhouette only;
   - a **one-sided, camera-dependent** Sobel `filterCamX` (`:92-95`, comment at `:91`: "Filter only
     the occluded side for camera selection. Assumes left and right cameras") — for the left camera
     only the left-facing gradient is penalized, because that is the side that camera cannot see
     behind;
   - two different confidences are emitted from the same taps (`:99-102`):
     `cameraBlendConfidence` **optimistic** (only occlusions penalized → used for camera
     selection/blending), `projectionConfidence` **pessimistic** (`min` over both →
     "Output pessimistic values for depth temporal filter to force invalidation on movement");
   - **depth-fold / contour snapping** (`:104-121`): if `maxDisp − minDisp` exceeds
     `g_depthFoldTreshold`, a screen-space offset of at most ±1 disparity pixel is computed from the
     un-clamped Sobel, *signed by whether the vertex is foreground or background* (`:115-119`), and
     lerped in by `contourFactor`. Applied to clip position at `:167`. This pulls silhouette
     vertices onto the actual image edge.
5. Optional gaussian disparity blur, **gated on low confidence** (`:126-147`) — smooth only where
   uncertain.
6. `DisparityToWorldCoords` (`:18-26`): `g_disparityToDepth · (u, v, disparity, 1)`, dehomogenize,
   clamp `|z|` to `g_projectionDistance`, then `g_depthFrameViewToWorld{Left|Right}`.
7. Floor clamp (`:152-164`): points below `g_floorHeightOffset` are ray-plane-intersected onto that
   height — a cheap prior that kills the "floor bends up" artefact.
8. Outputs: HMD clip position (+ the contour offset), plus `cameraReprojectedPos`,
   `prevCameraFrameScreenPos`, `prevHMDFrameScreenPos` (`:175-179`) — three reprojections computed
   once per vertex for downstream passes.

The pixel shader for this pass is trivial (`depth_write_ps.hlsl:9-14`): it writes confidences into
a 4-channel validity target, with **which channels land selected by the blend state** rather than
by the shader — `blendFactor = {1,0,1,0}` for the primary camera
(`passthrough_renderer_dx11.cpp:1854-1855`) and `{0,1,0,1}` for the cross camera (`:1947-1951`)
`[V]`. Depth comes from the ordinary depth test with `LessWrite`/`GreaterWrite` per reversed-Z
(`:1856`). Two separate depth targets are used, `passthroughDepthStencil[0]` (primary) and `[1]`
(cross), cleared and rendered independently (`:1850-1851`, `:1943-1945`).

### 3.5 Temporal depth filter (`depth_write_temporal_ps.hlsl`) `[V]`

Selected in place of `depth_write_ps` when `StereoUseDisparityTemporalFiltering` is on, with the
*previous swapchain index's* depth + validity bound as SRVs
(`passthrough_renderer_dx11.cpp:1864-1877`, `:1933-1941`).

- **Confidence gate** (`:58`): the entire history path runs only when
  `projectionConfidence < 0.5 || cameraBlendConfidence < 0.5`. High-confidence pixels take the
  fresh value unconditionally. This is the opposite of a conventional TAA (which blends
  everywhere) and is what stops the filter from adding latency to good data.
- **Reprojected history fetch**: `prevHMDFrameScreenPos` (computed in the VS from the *world* point,
  so it is a true 3D reprojection, not a 2D motion vector) → UV (`:60`), rejected within a 3 %
  border (`:62-68`), fetched with a 4-tap bicubic B-spline (`:73-74`, `util.hlsl:375-408`).
- **History is re-sharpened before use** (`:77`, `sobel_discontinuity_adjust` at `:14-47`) —
  comment at `:76`: "Create sharp edges so that the discontinuity adjust in the main pass works
  properly". Repeated bicubic resampling of a history buffer softens depth edges; re-snapping each
  frame prevents cumulative silhouette drift.
- **Acceptance test** (`:84`): history UVs valid **and** `prevProjectionConfidence >=
  currentProjectionConfidence` **and** `prevDepth > 0` **and** the world-space depth difference
  `|Δz|·(far−near)` is within `g_depthTemporalFilterDistance` (default 0.5 m,
  `config_manager.h:620`). History is only trusted when it is *better* than the present and
  *geometrically consistent* with it.
- **Blend weight** (`:86`): `lerpFactor = remap(conf, 0.5→0, 0→g_depthTemporalFilterFactor)`
  clamped — the worse the current confidence, the more history. Default strength 0.9
  (`config_manager.h:619`).
- Depth, projection confidence, and blend validity are all lerped together (`:87-89`) so confidence
  decays along with the data it describes.
- Channel selection by `g_doCutout` (`:79-80`) lets the same shader serve the primary and cross
  camera passes.

### 3.6 Backward pass: eye depth → colour (`fullscreen_passthrough_ps.hlsl`) `[V]`

One fullscreen triangle (`passthrough_renderer_dx11.cpp:2573-2574`, `:2621` `Draw(3,0)`), shader
variant chosen by `{cutout, temporal}` (`:2571-2618`).

1. Sample eye depth and the validity target (`:24-25`).
2. **Sobel discontinuity adjust** (`:33-36` → `fullscreen_util.hlsl:20-81`): 8 taps, Sobel
   magnitude vs `g_depthContourTreshold`; if exceeded, optionally gaussian-smooth the depth over
   `g_depthContourFilterWidth` (`:56-71`, comment at `:60`: "Filter with an output pixel-centered
   gaussian blur to get a smooth contour over the low res depth map pixels"), decide
   foreground-vs-background by whether `max−smoothed > smoothed−min` (`:73`), then **lerp the pixel
   all the way to `min` or `max` depth** by `saturate(strength·10·magnitude)` (`:76-79`). Net
   effect: the low-resolution depth edge is converted into a hard, smooth-contoured step at output
   resolution, and the pixel is committed to one side of it.
3. Reconstruct the world point from `(screenPos.xy, depth)` via `g_HMDProjectionToWorld` (`:38-41`).
4. Project into the source camera frame with the left or right camera matrix by
   `g_cameraViewIndex` (`:43`).
5. Optional metric depth cutoff: distance from the world eye position, clipped against
   `g_depthCutoffRange` (`:45-50`).
6. Homogeneous → UV (`:53`); optional `clip()` against `w` and the 0..1 range (`:55-59`).
7. Remap into the eye's half of the packed frame via `g_uvBounds`, clamp, then **add the fisheye
   correction offset** (`:64-71`).
8. **Sample the original camera colour** (`:80`), optional 5-tap unsharp (`:82-91`), optional
   CIELAB-D65 brightness/contrast/saturation (`:93-104`, comment at `:97`: "Using CIELAB D65 to
   match the `EXT_FB_passthrough` adjustments").
9. Output colour **and** `SV_Depth` (`:138-139`) — so passthrough participates in the depth test
   against the application's content.

### 3.7 Consuming application depth via `XR_KHR_composition_layer_depth` `[V]`

Rectus is an API layer, so it *reads* the app's depth rather than producing it — but the mechanics
are exactly what zxr-shell-v2 needs in reverse.

- Detection for telemetry: walk the `next` chain of `layer->views[0]` looking for
  `XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR` (`passthrough_system.cpp:483-506`).
- Per-eye consumption (`:961-992`): read `nearZ`/`farZ`, detect **reversed depth** when
  `farZ < nearZ` and swap, so the passthrough projection matches the app's convention exactly.
- Projection fixups (`:997-1009`): `XrMatrix4x4f_CreateProjectionFov` is patched for infinite Z
  (`m[10] = 0; m[14] = nearZ`) and, in the general case, `m[10]`/`m[14]` are rebuilt from
  `nearZ, farZ, minDepth, maxDepth` — i.e. the full `XrCompositionLayerDepthInfoKHR` semantic
  including the sub-range.
- The view matrix is built from the **application-provided layer view pose**
  (`:1029-1063`, comment at `:1039`: "The application provided HMD pose used to make sure
  reprojection works correctly") — not from a fresh tracker query. Using a *different* pose than
  the depth buffer was rendered with is a guaranteed misregistration.

This is a direct validation of the zxr-shell-v2 protocol requirement that depth carry *meaning*
(near/far/reversed-Z/normalization), not just format
([§2, requirement 2](../architecture/zxr-shell-v2-composition.md)) `[V]`.

### 3.8 Prepasses, blending and the app-alpha dance (`[V]`, platform-specific)

`RenderAlphaPrepassView` (`:2203-2335`, shader `alpha_prepass_ps.hlsl`) clips on
`projectionConfidence` (`:9-11`) and writes `1 − g_opacity` into alpha (`:20-22`);
`RenderMaskedPrepassView` (`:1963`) does chroma keying; `RenderAlphaTestPrepassView` (`:2336`) does
threshold alpha. The `bRenderAlphaPrepass` predicate (`:1599-1605`) and the blend-state selection
(`:2550-2565`) encode a long truth table over `{BlendMode, bEnableDepthBlending, bInvertLayerAlpha,
ProjectionMode}`.

**None of this transfers.** It exists to negotiate with an opaque SteamVR compositor and with
applications that own the eye buffers. zxr-shell-v2 *is* the compositor and owns the buffers, so
passthrough is simply the first (farthest) contributor to the depth-composed scene (§7).

### 3.9 Transferability verdict

| Component | Verdict |
|---|---|
| `vulkan_joint_bilateral.comp.glsl` | **Adopt nearly verbatim.** Already GLSL/Vulkan, already spec-constant-configured. |
| `vulkan_fill_holes.comp.glsl` | **Adopt**, but reconsider the negative-confidence encoding now that we are not squeezing into `r16_snorm` pairs. |
| Sobel discontinuity snap (`fullscreen_util.hlsl:20-81`) | **Port** (HLSL→GLSL is mechanical). Highest quality-per-instruction in the repo. |
| Forward disparity→eye-depth VS (`passthrough_stereo_vs.hlsl`) | **Port the math**, including clamped/one-sided Sobel, dual confidences, depth-fold offset, floor clamp. |
| Confidence-gated reprojected temporal depth filter (`depth_write_temporal_ps.hlsl`) | **Port the algorithm.** Re-sharpen-history is non-obvious and important. |
| Backward colour pass (`fullscreen_passthrough_ps.hlsl`) | **Port**, dropping the D3D register plumbing and the opacity/premultiply branches. |
| Rectify-only-the-matching-input + UV-offset map on the display path | **Adopt as policy.** One resample. |
| SGBM + WLS (`depth_reconstruction.cpp`) | **Portable but a placeholder.** Sibling doc's problem. Keep the *interface*: disparity + confidence + `Q` + view→world + exposure timestamp. |
| D3D11 renderer, DXGI shared handles, D3D11↔Vulkan interop (`passthrough_renderer_dx11*.cpp`) | **Discard.** Pure platform glue; we are Vulkan-native. |
| OpenVR camera access, SteamVR dashboard, API-layer dispatch, alpha/masked prepasses | **Discard.** |
| Blocking `vkWaitForFences` after async submit (`async_renderer.cpp:1377`) | **Reject the pattern**; use timeline semaphores and let the display path read the last-complete result. |

---

## 4. Anti-wobble catalogue

"Wobble" is the dominant complaint and it is a *temporal* artefact of *spatial* depth error. Five
distinct techniques, each tied to code.

### 4.1 Confidence-gated, reprojected temporal filtering

Blend the current frame's depth with the previous frame's depth **reprojected through 3D**, with a
weight driven by confidence, and reject on geometric inconsistency.

- Rectus: `depth_write_temporal_ps.hlsl:58` (gate), `:60-74` (reproject + bicubic fetch),
  `:84` (accept only if history confidence ≥ current *and* `|Δdepth| ≤ 0.5 m`), `:86-89` (weighted
  blend of depth *and* confidence) `[V]`.
- Passthrough+ does the same at the solver level: the previous frame's depth map is projected into
  the current headset pose and injected as *additional constraints* in the Laplace solve
  (`x_t ← LaplaceSolver[S_res(t) ∪ w·x_{t−1}]`), explicitly IIR-inspired, with the previous frame's
  output prioritized **30×** over current stereo points — "this is important for strong temporal
  stabilization. As this prioritization is decreased, temporal stabilization weakens" `[V]` (§3.3).
  (The PDF extraction garbles the exact per-point weights; the 30× ratio statement is unambiguous.)
- Passthrough+ *also* filters the sparse points before the solve: a point must be observed in ≥2
  frames to enter the result set, weights increment on re-observation and decrement on absence, and
  the cap is **6 frames (0.2 s)** for objects nearer than 1 m and **16 frames (0.5 s)** beyond —
  "This ensures that for fast movements of nearby scene elements such as hands, stale geometry that
  is older than 6 frames … does not negatively affect the rendering quality" `[V]` (§3.3).
  Ablation: median depth error unchanged, **90th-percentile error drops significantly** `[V]`
  (§6.1). *Depth-dependent history length is the cheapest near-field win in the corpus.*
- Quest Pro: "combining this output across a few frames to generate a dense 3D and
  temporally-stable representation" `[V]`.

### 4.2 Edge-aware / joint-bilateral upsampling

Never bilinearly upsample a depth map. Upsample it guided by the full-resolution luma, with taps
across depth edges rejected outright.

- Rectus: `vulkan_joint_bilateral.comp.glsl:74-112` (per-tap rejection at `:86`, luma weight at
  `:102`), circular support at `:179-196`, output confidence from surviving tap mass at `:200` `[V]`.
- viewcorrection's inpainting is the same principle applied to holes rather than upsampling:
  `base_weight = 1/(1 + 50·gradient_magnitude)`
  (`viewcorrection/src/view_correction/cuda_convolution_inpainting.cu:250`), so diffusion slows
  across image edges `[V]`.
- Passthrough+ explicitly *declined* edge weights and explains why: at 70×70, "the size of a grid
  cell in the hemisphere usually spans across multiple object boundaries, and adding edge weights
  therefore did not add any value. If more CPU/GPU resources were available, incorporating edge
  weights could produce a higher fidelity of the depth map at object boundaries" `[V]` (§3.2).
  **Read this as a resolution threshold, not as a refutation** — at any geometry resolution where a
  cell is comparable to an object boundary, edge awareness pays `[I]`.

### 4.3 Depth-discontinuity snapping (the silhouette must be hard)

A soft depth edge produces a smeared, *swimming* silhouette; a hard edge produces a crisp one, even
if slightly misplaced. Four independent implementations:

- **Output-resolution snap:** `fullscreen_util.hlsl:20-81` — Sobel magnitude gate, optional gaussian
  pre-smooth, side decision, full lerp to `min` or `max` depth `[V]`.
- **Sub-pixel snap during upsample:** `vulkan_joint_bilateral.comp.glsl:153-166` — snap to
  `min(neighborH, neighborV)` when both nearest neighbours disagree with the centre `[V]`.
- **Vertex-position snap:** `passthrough_stereo_vs.hlsl:104-121` — move the *vertex* in clip space by
  up to ±1 disparity pixel toward the true edge, signed by foreground/background `[V]`.
- **RGB-D sharpening:** NeuralPassthrough detects depth edges with Sobel + morphological dilation
  and sets edge pixels' RGB-D to their nearest non-edge neighbour, "to reduce flying pixels"
  (`neuralpassthrough/code/utils.py:194-210`, called at `code/test_prototype.py:140-141`) `[V]`.
  Costs 0.3 ms of their 32 ms budget `[V]`.

### 4.4 Never rasterize a triangle across a depth discontinuity

A triangle spanning a silhouette becomes a rubber sheet stretching from the hand to the wall behind
it — the most recognizable passthrough artefact there is.

- viewcorrection is the cleanest implementation: `MeshDepthmapCUDAKernel`
  (`viewcorrection/src/view_correction/view_correction_display.cu:121-208`) `[V]`. With
  `kJumpThreshold = 0.070 m` (`:130`), it (a) flags foreground-boundary vertices in a colour
  attribute (`:155-160`) and (b) when the max pairwise depth difference over a quad exceeds the
  threshold, **emits a degenerate index pattern instead of the quad** (`:166-190`) — no geometry is
  created there at all. The TODO at `:130` is the right critique: "This should depend on the depth
  instead of being constant" — per §1.4 the threshold should scale with `Z²` `[I]`.
- Rectus instead keeps the grid topology but pushes vertices onto one side of the edge and zeroes
  their confidence, letting the fullscreen Sobel snap and the cross-camera/hole-fill logic resolve
  the gap `[V]`. Its border handling is related: `MeshCreateGrid` stores a border weight in vertex
  `z` (`mesh.cpp:68-82`) and the VS rejects vertices within `maxFilterWidth` of the image edge
  (`passthrough_stereo_vs.hlsl:60-65`) `[V]`.

### 4.5 Bound the proxy and the priors

Cheap, unglamorous, effective:

- Clamp reconstructed `|z|` to a max projection distance (`passthrough_stereo_vs.hlsl:22`) `[V]`.
- Treat unknown depth as **far**, never near (`vulkan_joint_bilateral.comp.glsl:45-49`) `[V]`.
- Floor-plane clamp below a configured height (`passthrough_stereo_vs.hlsl:152-164`) `[V]`.
- Fixed Dirichlet border depth in the densification solve — Passthrough+ initializes the
  hemisphere border to 2.0 m `[V]` (§3.2).
- A depth-biased backstop surface behind everything (`RenderBackgroundForView`,
  `passthrough_renderer_dx11.cpp:2651-2710`) `[V]`.
- Solve on **inverse depth**, not depth — Passthrough+ §3.2 `[V]`; NeuralPassthrough's splat
  importance weight is a function of inverse depth (`utils.py:79-86`, `:128`) `[V]`. Inverse depth
  makes error roughly uniform in disparity space, which is where the perceptual error lives (§1.4).

---

## 5. Disocclusion and hole-fill

### 5.1 Three kinds of hole, three different correct answers

| Kind | Cause | Correct response |
|---|---|---|
| **Sampling / resampling hole** | Forward splatting at a scatter rate below the target rate; grid coarser than output | Densify: splat every pixel, or use a backward pass, or low-pass the splat |
| **Partial disocclusion** | Surface hidden from *one* camera, visible to the other | **Take the other camera's pixel.** Real data. |
| **Full disocclusion** | Hidden from *both* cameras | No data exists. Fill plausibly and *stably*; never hallucinate detail. |

Conflating these is the classic mistake — running an inpainter over a sampling hole wastes time and
blurs real data; running the other-camera lookup over a full disocclusion returns garbage.
NeuralPassthrough makes the distinction explicit and formal: masks `m_l`, `m_r` mark per-view
splat holes, partial disocclusion is removed by `c_l ← (1−m_l)·c_l + m_l·c_r` (Eq. 3), and
`m = m_l ⊙ m_r` marks full disocclusion (Eq. 5) `[V]`. In code:
`run_partial_disocclusion_filter` then `run_full_disocclusion_filter`
(`neuralpassthrough/code/utils.py:135-192`, called at `code/test_prototype.py:183-189`) `[V]`.

### 5.2 Priority order

Synthesizing all three references `[I]`, with each step's provenance:

```
1. primary camera, high confidence          →  use it
2. cross camera                             →  Rectus RenderDepthPrepassView second pass
                                               (passthrough_renderer_dx11.cpp:1921-1955);
                                               NeuralPassthrough Eq. 3
3. validated reprojected history            →  viewcorrection ForwardReprojectToInvalidPixels
                                               (view_correction_display.cc:1036-1061)
4. cheap diffusion inpaint (depth first, then colour)
                                            →  viewcorrection InpaintDepthMapWithConvolutionCUDA
                                               (view_correction_display.cc:1094-1105) then
                                               InpaintImageWithConvolutionCUDA (:1130-1140)
5. bounded backstop surface                 →  Rectus RenderBackgroundForView (:2651-2710)
```

Step 3 is the one most systems skip and shouldn't. It is real photographic data from a valid
viewpoint, merely stale — strictly better than synthesized pixels for a *static* background, which
is what most disocclusions reveal.

### 5.3 viewcorrection's kernels in detail `[V]`

**Cheap weighted-convolution diffusion** (`cuda_convolution_inpainting.cu`) — the recommended path
(`flags.cc`: `vc_inpainting_method` defaults to `"convolution"`).

- Jacobi-style iteration with a fixed 3×3 stencil: corners `0.073235`, edges `0.176765`, centre `0`
  (`:122-176`). It is a normalized 8-neighbour average, i.e. a discrete Laplacian solve —
  structurally the same equation Passthrough+ solves with Jacobi over-relaxation `[V]`.
- Only hole pixels update (`kIsPixelToInpaint = depth ≤ 0`, `:94`); valid pixels are Dirichlet data.
- Uninitialized neighbours are excluded from *both* numerator and denominator
  (`pixel_weight = (…&& temp_depth > 0) * w`, `:122-176`), so the front advances rather than
  averaging in zeros. The commented-out "version without explicit handling of uninitialized values"
  at `:178-187` shows exactly what that costs.
- **Weighted variant** (`:219-353`): `base_weight = 1/(1 + 50·gradient_magnitude·√2/255)` (`:250`)
  makes diffusion respect image edges. Enabled by default (`flags.cc`,
  `vc_use_weights_for_inpainting = true`).
- `kIterationsPerKernelCall = 4` (`:39`) with a 32×32 block and a 4-pixel apron, so four Jacobi
  sweeps happen entirely in shared memory per launch (`:104-212`).
- **Block-sparse dispatch**: an init kernel counts active (hole) pixels per block via
  `cub::BlockReduce` (`:74-81`), the host compacts the active blocks' coordinates (`:384-407`), and
  subsequent launches use a 1-D grid over *active blocks only* (`:416-417`). Convergence is
  re-checked at most every 25 iterations (`:414`) and converged blocks drop out (`:455-469`).
  Iteration cap = `max(width, height)` (`view_correction_display.cc:1097`), tolerance `1e-3` for
  depth and `1e-2` for colour (`:1098`, `:1134`).
- An RGB variant with the identical structure handles colour
  (`cuda_convolution_inpainting_rgb.cu:92-218` unweighted, `:221-377` weighted with
  `__launch_bounds__(32*32, 1)`).

**Huber-TV primal-dual** (`cuda_tv_inpainting_functions.cu`) — the quality comparison point,
selectable via `--vc_inpainting_method TV`. Chambolle–Pock on a Huber-regularized TV energy:
`kHuberEpsilon = 0.01` (`:40`); dual step with `huberFactor = 1/(1 + σ·0.5·ε)` and projection onto
the `g`-unit ball whose radius carries the edge weight `1 + gradient_magnitude·√2/255`
(`:270-334`, `:302`); primal step with **diagonal preconditioning** `τ = 1/rowSum` from the actual
Neumann-boundary row count (`:202-266`); proximal-point extrapolation `γ = 0.1` on both (`:250`,
`:325`); duals packed as `int16_t` via `kDualIntToFloat = 2/32767` (`:42`); same
4-iterations-per-launch fused kernel and block-activity machinery (`:336-476`), plus an adaptive
variant (`:477`). Fixed at 800 iterations in the orchestrator
(`view_correction_display.cc:1108`, `:1144`) — no early-out.

**The comparison is the finding.** TV preserves sharp discontinuities across a hole (an edge
crossing a gap can continue); weighted convolution diffuses and will blur an edge across a wide
hole. But TV needs primal-dual machinery, two dual images, hundreds of iterations and no early
termination, whereas the convolution path converges in tens of sweeps over sparse active blocks.
The repo defaults to convolution — **for passthrough, where holes are thin and the budget is a few
ms, cheap diffusion is the correct engineering choice and the TV solver is the reference for "what
we gave up"** `[I]`.

### 5.4 Temporal initialization of newly exposed regions `[V]`

`view_correction_display.cc:1036-1061`, gated on `vc_ensure_target_frame_temporal_consistency`
(default true) and `have_previous_rendering_`:

- The previous frame's **inpainted** depth+colour output is forward-reprojected into the current
  target view, writing **only where the current target is still invalid**
  (`ForwardReprojectToInvalidPixelsCUDA`, kernel at `view_correction_display.cu:674-740`).
- The z-test is `dest_depths(iy, ix) <= 0` (`:717`) — write only into holes, never over fresh data.
- It scatters only every second pixel per axis (`:691`), trading coverage for 4× fewer scatter
  conflicts (honest comment at `:718-720` that the unsynchronized writes should be locked).
- Colour is written as a 5-tap cross blur, not a point copy (`:729-735`), with the note at
  `:727-729` that "Proper rounding … is important, otherwise the colors slowly go to black" — a real
  drift hazard in any recirculating history buffer.
- Then depth is inpainted (`:1094-1105`) and colour is inpainted (`:1130-1140`), so history fills
  what it can and the inpainter handles only the genuine remainder.
- **Load-bearing caveat, stated in the source** (`:1035-1037`): "It is a good idea to fix the
  exposure time if using this (or know the differences and adapt the colors accordingly)." Reusing
  history across an auto-exposure change produces visible patches. Either lock exposure, or carry
  exposure/gain/white-balance per history buffer and correct on read `[V]`.
- An analogous but *unused* precaution: `DeleteAlmostOccludedPixelsCUDA`
  (`view_correction_display.cu:623-664`, radius 2 px, threshold `2 × 0.07 m`) deletes background
  pixels near foreground ones "to prevent foreground objects from being projected onto the
  background if occlusion boundaries are imprecise" (`view_correction_display.cc:1745-1758`) —
  compiled out via `kDeleteAlmostOccludedPixels = false` (`:1745`). A conservative alternative to
  §4.3's snapping; worth re-evaluating `[I]`.

### 5.5 NeuralPassthrough's full-disocclusion filter is a useful shape `[V]`

Rather than inpainting, it applies "depth-assisted, anisotropic lowpass filtering" with an explicit
prior: "the disoccluded regions are more often missing information from background objects rather
than from foreground occluders; as a result, our method fills in the disoccluded pixels using only
the smoothed colors of relatively *far* objects within the local neighborhood" — a 29×29 zero-mean
Gaussian, σ = 7 px, restricted to far-depth samples (`filter_color_disocclusion_cupy`,
`neuralpassthrough/code/utils.py:144-192`), costing 0.9 ms `[V]`. They explicitly rejected
context-aware inpainting because "if applied to our dynamic passthrough problem, this approach will
introduce temporal flickering into the output videos", and masked full-disocclusion regions out of
the *training loss* to prevent the network learning to inpaint `[V]`. **Stability over
plausibility.**

---

## 6. Left-camera→left-eye vs cross-camera fusion

### 6.1 Baseline: same-side camera textures same-side eye

This is the baseline everywhere, for three reasons stated by Chaurasia et al. §3.4 `[V]`:
shortest warp baseline per eye; preserved view-dependent illumination and parallax; full 180° FOV
coverage that a single image could not give both eyes. Rectus encodes it as a simple UV-bounds
selector: `GetFrameUVBounds` returns `(0,0,0.5,1)` for the left eye and `(0.5,0,1,1)` for the right
in a horizontally packed stereo frame (`passthrough_renderer.h:22-61`), selected per pass by
`g_cameraViewIndex` / `g_uvBounds` (`fullscreen_passthrough_ps.hlsl:43`, `:64-71`) `[V]`.

**Adopt this as the default.** It is cheap, it is what production ships, and it is the only option
that covers the full FOV.

### 6.2 Refinement: cross-camera gap filling

Rectus's "cutout" mode (`StereoCutoutEnabled`, default **off**, `config_manager.h:626`):

- `RenderDepthPrepassView` runs the grid mesh **twice** — once for the primary camera into
  `passthroughDepthStencil[0]`, once for the cross camera into `[1]`, with
  `cameraViewIndex`/`disparityUVBounds` swapped and `cameraBlendWeight =
  StereoCutoutSecondaryCameraWeight` (default 0.5) (`passthrough_renderer_dx11.cpp:1921-1955`) `[V]`.
- The blend state selects which channel pair of the shared 4-channel validity target each pass
  writes (`:1854-1855` vs `:1947-1951`) `[V]`.
- `fullscreen_passthrough_composite_temporal_ps.hlsl` then fuses:
  - read both depths and `cameraValidation.xy` (projection confidences) / `.zw` (blend validities)
    (`:31-36`);
  - `selectMainCamera = blendValidity.x >= blendValidity.y`; `blendCameras` only when *both*
    exceed 0.1; `cameraBlend` from their difference (`:38-41`);
  - Sobel-snap each depth map independently, and only if that camera contributes (`:47-57`);
  - reconstruct *two* world points and *two* camera UVs (`:59-68`);
  - sample both cameras with a 5-tap unsharp, tracking per-camera neighbourhood `min`/`max`
    (`:115-169`), and **clamp the cross colour into the primary's neighbourhood AABB**
    (`:172`) — the standard TAA history-rectification trick applied *spatially* across cameras, so
    the seam cannot introduce out-of-family colour;
  - **sub-pixel proximity arbitration** (`:174-184`): prefer whichever camera's sample lands nearer
    a source pixel centre — i.e. prefer the *less interpolated* camera;
  - **depth-agreement gate** (`:186-189`):
    `depthFactor = saturate(1 − |depth − crossDepth|·1000)`, and
    `combineFactor = g_cutoutCombineFactor · depthFactor · conf.x · conf.y`. Blending is only
    allowed where both cameras agree geometrically *and* both are confident;
  - final blend `:190-199`, and depth/confidence lerped along with colour so the composited depth
    stays consistent with the composited colour.
- The temporal colour history filter shares the pass (`:202-266`): reprojected via
  `prevCameraFrame_WorldToHMDProjection` (per-camera-frame, not per-display-frame), fetched with
  point/bilinear/bicubic/Catmull-Rom/Lanczos2 by `g_temporalFilteringSampling` (`:212-232`),
  AABB-clipped to the neighbourhood with configurable leeway (`:248-251`), and **hard-disabled**
  when confidence < 0.1 or either depth map was discontinuity-filtered (`:243-246`). History is
  written only on the first render of a camera frame (`:263-266`).

### 6.3 Verdict

- **Baseline (ship first):** same-side camera → same-side eye, no cross-camera sampling. `[I]`
- **Refinement (ship later, default off):** cross-camera fill, and only under Rectus's three gates
  simultaneously — geometric agreement, mutual confidence, and the occlusion-side one-sided Sobel
  from `passthrough_stereo_vs.hlsl:92-95`. Rectus's own default (`StereoCutoutEnabled = false`) is
  a hint that ungated cross-camera blending does more harm than good. `[I]`
- Note the FOV motivation is hardware-shaped: Rectus's readme calls out "wide aspect camera systems,
  such as on the Valve Index" `[V]`. On a headset whose camera FOV barely covers the display FOV,
  cross-camera fill buys much less. Make it a per-device capability, not a global mode `[I]`.
- NeuralPassthrough's version is unconditional-but-learned: it splats *both* input views into *each*
  target view and lets a U-Net fuse them (`code/test_prototype.py:158-189`, `code/net.py:99-107`)
  `[V]`. Design reference; not deployable in our budget.

---

## 7. What Mura should adopt, reject, and how it maps onto zxr-shell-v2

### 7.1 Passthrough is the environment colour+depth source

The composition model already says the compositor performs cross-client nearest-depth composition
itself and submits **one** projection layer to Monado
([§7.4](../architecture/zxr-shell-v2-composition.md)). Passthrough slots in as *another
contributor* to the same depth-composed scene — the farthest, always-present one:

```
xrWaitFrame → target display time t_d
xrBeginFrame; xrLocateViews(t_d)
  distribute the frame snapshot (P_e·V_e, bounds) to all 3D clients
  ── PASSTHROUGH ENVIRONMENT PASS (compositor-internal, per eye) ──
    latest complete geometry snapshot (disparity/depth + confidence + t_expose + poses)
    forward pass:  grid mesh → eye-space depth + per-camera validity      [§3.4]
    backward pass: eye depth → world → camera UV → ORIGINAL camera colour [§3.6]
    writes colour AND gl_FragDepth into the shared colour+depth targets
  ── then 2D planes, compositor objects, and each ready 3D client ──
    ordinary depth test resolves everything against everything
xrEndFrame → ONE stereo projection layer
```

Consequences that fall out for free `[I]`:

- **A virtual object correctly occludes and is occluded by the real world**, per-pixel, with no new
  protocol and no vendor depth-test extension — because passthrough writes real depth into the same
  buffer the T1 min-depth identity already governs
  ([§2](../architecture/zxr-shell-v2-composition.md)). It obeys the same depth-*meaning* contract as
  clients (near/far/reversed-Z), which Rectus demonstrates end-to-end (§3.7).
- Passthrough is **never a client**: not subject to the composition cutoff, cannot be late, cannot
  stall. It is compositor-internal, like 2D plane rasterization.
- Passthrough's data is **reusable across display frames in a way 3D clients' is not.** The T3
  objection (a colour+depth image lacks occluded geometry,
  [§3](../architecture/zxr-shell-v2-composition.md)) applies to reusing a *composed eye image*; here
  we reuse *camera-space* data and re-derive the warp from the current pose every frame. Same
  distinction that makes 2D windows re-projectable
  ([§7.3](../architecture/zxr-shell-v2-composition.md)).
- **Bandwidth:** the composition doc §8 already flags ~96 MiB/app/frame for 2×2048² RGBA8+D32.
  Camera textures and the geometry buffer are small, but the per-eye environment depth target is
  another full-resolution D32. Sizing it *below* eye resolution and letting §4.3's Sobel snap
  reconstruct the edges is the whole point of the low-res-geometry architecture.

### 7.2 Adopt

1. **The three-rate architecture** (§2.1) as a hard structural rule: camera thread, geometry thread,
   display path; double-buffered geometry handoff; display path reads the last *complete* snapshot
   and never waits.
2. **Rectify only the matching input; correct UVs on the display path** via a baked offset map
   (§3.1). One resample of the colour that reaches the eye.
3. **Forward geometry pass → eye depth, backward fullscreen pass → colour** (§1.2, §3.4, §3.6).
   Decouples geometry resolution from colour resolution and gives correct visibility via the
   hardware depth test.
4. **Confidence as a first-class per-pixel channel**, produced by the depth backend, refined by the
   upsampler, consumed by the temporal filter and the cross-camera blend
   (§3.2, §3.3, §3.5, §6.2).
5. **The full anti-wobble set** (§4): confidence-gated reprojected temporal depth filter with
   geometric rejection *and depth-dependent history length*; joint-bilateral upsample; Sobel edge
   snapping at output resolution; no triangles across discontinuities; inverse-depth solving;
   bounded priors and a backstop surface.
6. **Same-side camera → same-side eye** as the baseline (§6.1).
7. **The hole-fill priority order** (§5.2), with the cheap weighted-convolution diffusion fill
   (§5.3) and validated-history initialization (§5.4).
8. **Pose-at-exposure discipline** (§2.3): every camera frame carries its exposure timestamp; the
   pose is *queried for that timestamp*; the geometry snapshot carries both. Monado's device pose
   query must support this, and if it cannot, that is a blocking dependency, not a detail.
9. **Latency-first defaults** (§2.4), with quality knobs confined to the geometry pipeline.

### 7.3 Reject

1. **All D3D11/DXGI/OpenVR/SteamVR glue** and the API-layer dispatch framework — we are the
   compositor and we are Vulkan-native.
2. **The alpha/masked/alpha-test prepass truth table** (§3.8) — an artefact of not owning the eye
   buffers.
3. **Blocking fence waits anywhere near the display path** (`async_renderer.cpp:1377`); use timeline
   semaphores, consistent with the transport design already chosen
   ([§5](../architecture/zxr-shell-v2-composition.md)).
4. **Neural fusion networks on the display path.** NeuralPassthrough needed 2× Titan V for 32 ms at
   2×1280×720 `[V]`; that is not a standalone-headset result and was never claimed to be.
5. **Full-disocclusion inpainting that hallucinates detail.** Both NeuralPassthrough (explicitly,
   via the masked loss) and viewcorrection (by defaulting to diffusion) reject it for temporal
   reasons `[V]`.
6. **Huber-TV inpainting on the frame budget** (§5.3) — keep it as an offline quality reference.
7. **Panoramic stitching of the two camera feeds into a shared surface** (§1.1). Wrong problem.
8. **Ungated cross-camera blending** (§6.3).

### 7.4 Sequencing

| Milestone | Content | Acceptance |
|---|---|---|
| P0 | Camera capture with exposure timestamps + pose-at-exposure; raw undistorted mono view, no reprojection | Timestamp↔pose error measurably < 4 ms (the Passthrough+ threshold) |
| P1 | Fixed-proxy warp (backward pass only, constant depth) as the plumbing skeleton | Correct per-eye geometry for far content; near content *visibly* wrong (this is the baseline to beat) |
| P2 | Any depth source → forward pass → eye depth → backward colour pass; same-side texturing | A hand at 40 cm registers in both eyes; no rubber-sheet silhouettes |
| P3 | Anti-wobble set (§4) | Static scene, static head: no boiling. Static scene, moving head: no swimming silhouettes |
| P4 | Hole-fill priority chain (§5.2) | Fast lateral head translation past a near object leaves no persistent black wedge |
| P5 | Environment layer wired into sort-last composition (§7.1) | A virtual cube is correctly occluded by a real hand passing in front of it, per-pixel |
| P6 | Cross-camera fill, default off, per-device capability (§6.3) | Measurable FOV-edge gap reduction on a wide-camera device; no new seams elsewhere |

---

## 8. Metrics

### 8.1 The three ages, measured separately

Passthrough+'s naming is the right vocabulary `[V]`:

| Metric | Definition | Reference value | Target |
|---|---|---|---|
| **Capture-to-display age** (photon-to-texture) | mid-exposure → photons leave the display for the pixel that sampled that colour | 49 ms (Passthrough+); 14 ms (Play For Dream low-latency); ~12 ms display-path only (Vision Pro R1) | as close to one camera period as possible; hard ceiling well under 100 ms (judder threshold, Passthrough+ §5) |
| **Geometry age** (photon-to-geometry) | mid-exposure of the frames that produced the current depth → display | 62 ms (Passthrough+); >100 ms tolerated in their early prototypes | 2–3× the capture age is acceptable |
| **Late-pose response** | most recent pose used by the per-eye warp → display | — | one display frame; must not inherit either age above |

The third is the one that must be small and is entirely under our control. Reporting a single
"passthrough latency" number hides which of the three is broken `[I]`.

### 8.2 Instrumentation

- Timestamp every buffer at every hop (exposure mid-point, decode done, geometry solved, geometry
  published, warp submitted, scanout). Rectus has the primitives
  (`shared/perfutil.h`, `camera_manager_openvr.cpp:568` frame-latency measurement; viewcorrection
  uses per-stage `cudaEvent_t`, `view_correction_display.cc:1089`, `:1118`) `[V]`.
- Log the **distribution**, not the mean. Passthrough+ aggregated over 10 000 hours of uncontrolled
  usage and reported 90th-percentile depth error specifically because that is where the artefacts
  live `[V]` (§6.1).
- Track **geometry staleness at display time** as a histogram, and count frames where the display
  path reused an older snapshot than the previous frame did (a non-monotonic snapshot is a visible
  jump).
- Track the **hole budget**: fraction of eye pixels resolved by each step of §5.2, per frame.
  Rectus's debug overlays are a good model: confidence (`fullscreen_passthrough_ps.hlsl:111-125`),
  camera selection, temporal blend, temporal clipping, discontinuity filtering
  (`fullscreen_passthrough_composite_temporal_ps.hlsl:287-356`) `[V]`.
- Replicate Passthrough+'s **image coverage** metric: divide the camera image into 20×20 blocks and
  count blocks with at least one valid depth sample, plotted against an allowed depth-error
  threshold `[V]` (§6.1). Coverage that is flat in the error threshold means the depth backend is
  not biased toward any accuracy band, which is what lets downstream heuristics be tuned
  independently.

### 8.3 Test scenes

Each targets a specific failure mode `[I]`, with the failure mode's provenance:

1. **Hands at 20–40 cm, still, then moving fast.** The canonical case. Static → boiling silhouettes
   (§4.1); moving → stale geometry and smeared edges. Passthrough+ caps near-field history at 6
   frames / 0.2 s precisely for this `[V]`.
2. **Reach-and-grasp (haptic trust).** Reach for a cup at 40 cm with only passthrough. Passthrough+
   names this as goal (3) and as the justification for high geometry refresh rate `[V]`.
3. **Printed text at 30 cm, 60 cm, and 2 m.** Diagnoses the resample chain (§3.1) more than depth.
   Reviewers use exactly this: Play For Dream lost a side-by-side text-readability comparison to
   Quest 3 `[R]` ([The Ghost Howls](https://skarredghost.com/2025/09/11/play-for-dream-hands-on-review/)).
4. **Thin structures** — cable, chair leg, plant stem, monitor edge. Diagnoses whether
   discontinuity handling *deletes* thin geometry. viewcorrection's MPI comparison singles out "the
   table leg" as the structure a competing method failed to reconstruct `[V]`.
5. **Low light / high dynamic range** — a lamp, a bright window, dark corners. Diagnoses the
   matching front-end and the exposure interaction with history reuse (§5.4). Play For Dream's
   high-quality mode reportedly "tries way too hard to over-correct some dark areas, to the point it
   creates a weird geometric noise" `[R]`.
6. **Fast head translation (not rotation) parallel to a near object.** The pure disocclusion test:
   maximal newly-exposed area, and the case where a fixed proxy is most obviously wrong (§1.3).
7. **Textureless surfaces** — white wall, blank desk. Passthrough+: correspondences "get discarded
   when ending up on textureless image regions" `[V]` (§7). Tests the densification prior, not the
   matcher.
8. **Mixed real/virtual occlusion.** A virtual object interpenetrating a real one — the actual point
   of §7.1, and the one no upstream reference tests because none of them is a general compositor.
9. **Depth-source outage.** Freeze or drop the geometry snapshot for 500 ms while moving.
   Verifies §2.2: the display path must degrade to stale-geometry warping, never to a stall.
   Rectus has a debug toggle for exactly this (`DebugStereoReconstructionFreeze`,
   `depth_reconstruction.cpp:361`) `[V]`.

---

## 9. Open questions

### 9.1 The interface passthrough needs from the depth backend (the one thing to fix now)

Per **view** (one per camera, not per eye), per snapshot:

```
DepthSnapshot {
  // geometry
  depth_or_disparity   : image, own resolution (typically 1/2–1/4 of camera res)
  encoding             : { disparity + Q-matrix } | { metric depth } | { inverse depth }
                         + near/far/normalization, i.e. the XrCompositionLayerDepthInfoKHR semantic
  confidence           : image, same resolution, [0,1]; plus a distinguished value for
                         "extrapolated / hole-filled" vs "measured"  (cf. Rectus's negative-
                         confidence encoding, vulkan_fill_holes.comp.glsl:47-59)

  // time
  exposure_timestamp   : mid-exposure, in the same clock as pose queries      (§2.3)
  snapshot_id          : monotonic; the display path must never go backwards

  // space
  view_to_world        : pose at exposure_timestamp, per source view          (§3.1)
  intrinsics           : + distortion model, + the rectification rotation if the depth is
                         expressed in a rectified frame
  guide_image          : full-res luma of the *rectified* frame, for joint-bilateral upsampling
                         (Rectus uploads exactly this, depth_reconstruction.cpp:834-855)
}
```

Non-negotiables `[I]`: **(a)** confidence is mandatory, not optional — every anti-wobble technique
in §4 is gated on it, and a backend that cannot produce it forces the compositor to fabricate one
(as Rectus does when WLS is off, `depth_reconstruction.cpp:827-828`, with correspondingly worse
results); **(b)** the timestamp must be mid-exposure in the pose clock; **(c)** a snapshot is atomic
across `{depth, confidence, pose, timestamp}` — the same all-or-nothing rule the client contract
already imposes ([§7.2](../architecture/zxr-shell-v2-composition.md)); **(d)** publication is
non-blocking and the display path is always free to reuse the previous snapshot.

### 9.2 Genuinely open

1. **Geometry resolution and topology.** Passthrough+'s 70×70 hemisphere was chosen to fit a GPU
   rasterization budget on a Snapdragon 835 in 2020 `[V]`; at that resolution edge-aware weighting
   was measurably worthless `[V]`. Where is the crossover on our targets, and is the right proxy a
   head-centred hemisphere (wide FOV, survives rotation) or a per-camera grid (natural for
   disparity, needs two)? Rectus uses per-camera grids; Passthrough+ uses a shared hemisphere.
2. **Is a full-resolution per-eye environment depth target affordable?** If not, what resolution,
   and does §4.3's Sobel snapping recover enough edge quality at half or quarter resolution? This
   is a measurable experiment, not a judgement call.
3. **Depth-dependent thresholds.** viewcorrection's `kJumpThreshold` TODO is right (`:130`): the
   discontinuity threshold, the temporal rejection distance, and the history length should all scale
   with `Z` or `Z²` per §1.4. Nobody in the corpus implements this. Cheap to try.
4. **Passthrough vs. Monado's own reprojection.** The compositor may attach
   `XR_KHR_composition_layer_depth` to its single projection layer to aid Monado's reprojection
   ([§7.4](../architecture/zxr-shell-v2-composition.md)). If passthrough writes real environment
   depth into that buffer, Monado's late reprojection will move the *real world* too. Is that
   desirable (it reduces effective latency, like Quest Pro's ATW-warped mesh `[V]`) or harmful
   (double-warping, and the real world moving relative to the room)? Needs an experiment.
5. **Where does the per-eye warp actually belong?** Vision Pro's answer is a dedicated chip on the
   display path ("R1 streams new images to the displays within 12 milliseconds" `[V]`) and a
   patented split of passthrough-warp and virtual-warp feeding a merging compositor
   (US20250123490A1: `warp mesh generator`; separate passthrough/virtual warp subcomponents; media
   merging compositor `[V]`). We have no such silicon. Does the warp go in the compositor's frame
   (as designed in §7.1), or behind Monado's reprojection as a late stage? Related to (4).
6. **Auto-exposure across history reuse.** viewcorrection's advice is "fix the exposure time, or
   know the differences and adapt the colors accordingly" `[V]`. Carrying exposure/gain/WB metadata
   per camera frame and correcting on history read is the principled fix; nobody in the corpus does
   it. Interacts with the Play For Dream observation that dark-area over-correction is itself an
   artefact source `[R]`.
7. **Per-device camera geometry as a contract input.** Camera↔eye offset `t`, camera baseline,
   camera FOV vs display FOV, and rolling-shutter parameters all change the tuning and decide
   whether cross-camera fill is worth anything (§6.3). This belongs in the typed `mura.*` device
   contract ([device-contract.md](../architecture/device-contract.md)), not in compositor constants.
8. **Rolling shutter.** Nothing in the corpus models it. A rolling-shutter camera on a rotating head
   makes `T_{W←Ci}(t_expose)` a function of image row. Probably ignorable; needs to be *measured*
   rather than assumed.
9. **Pupil-tracked passthrough.** NeuralPassthrough cites Krajancich et al. 2020 on the value of
   tracking the moving pupil rather than warping to a nominal eye position, and notes that all
   current systems including Passthrough+ warp to the nominal position "while accepting any
   artifacts resulting from the computational limits" `[V]`. We have eye tracking on some targets.
   Out of scope for v1; worth a note that the warp target should be a *parameter*, not a constant.

---

## Sources

**Local code** (under `references/`, pinned by `references/MANIFEST.json`):

- `openxr-steamvr-passthrough/` — Rectus, MIT. Read at `603c4c8`. Shaders in
  `XR_APILAYER_NOVENDOR_steamvr_passthrough/shaders/`; reconstruction in `depth_reconstruction.cpp`;
  renderer in `passthrough_renderer_dx11.cpp`; async compute in `async_renderer.cpp`; OpenXR
  interaction in `passthrough_system.cpp`, `layer.cpp`; defaults in `shared/config_manager.h`.
- `viewcorrection/` — puzzlepaint / ETH CVG, BSD-3. Schöps, Oswald, Speciale, Yang, Pollefeys,
  "Real-Time View Correction for Mobile Devices", TVCG special issue on ISMAR 2017
  ([pdf](http://cvg.ethz.ch/research/view-correction/paper/Schoepst2017ISMAR.pdf)).
  Kernels in `src/view_correction/`.
- `neuralpassthrough/` — facebookresearch, MIT, archived. Xiao, Nouri, Hegland, Garcia Garcia,
  Lanman, "NeuralPassthrough: Learned Real-Time View Synthesis for VR", SIGGRAPH 2022
  ([doi](https://doi.org/10.1145/3528233.3530701)). Python reference implementation only ("prior to
  customized real-time inference optimization in C++").

**Papers and disclosures:**

- Chaurasia, Nieuwoudt, Ichim, Szeliski, Sorkine-Hornung, "Passthrough+: Real-time Stereoscopic View
  Synthesis for Mobile Mixed Reality", PACMCGIT 3(1) Article 7, May 2020.
  [doi:10.1145/3384540](https://doi.org/10.1145/3384540) ·
  [PDF](https://alexandruichim.com/pdf/Chaurasia_Passthrough_CGIT20.pdf) ·
  [Meta AI listing](https://ai.meta.com/research/publications/passthrough-real-time-stereoscopic-view-synthesis-for-mobile-mixed-reality/)
- Meta, "Introducing Meta Reality: A Look at the Technologies Necessary to Convincingly Blend the
  Virtual and Physical Worlds" —
  [Quest Pro passthrough architecture](https://www.meta.com/blog/mixed-reality-definition-passthrough-scene-understanding-spatial-anchors/)
  (≤10 000 points/frame to 5 m; multi-frame fused mesh; per-eye predictive warp via ATW).
- Meta, "The Magic Under the Hood: How AI Is Powering Meta's Technologies Today + in the Future" —
  [Quest 3 ML depth](https://www.meta.com/blog/ai-powered-technologies-quest-3-pro-ray-ban-meta-smart-glasses/)
  ("Most of the 3D geometry improvements in Passthrough come from AI"; "mathematically optimum
  points don't necessarily mean perceptual optimums").
- Meta, [v66 release notes](https://www.meta.com/blog/meta-quest-v66-software-update-reduced-passthrough-distortion-background-audio/)
  — reduced passthrough distortion and warping around moving hands/objects.
- Apple, [US20250123490A1](https://patents.google.com/patent/US20250123490A1/en), "Head-Mounted
  Device with Double Vision Compensation and Vergence Comfort Improvement", filed 2024-06-11,
  published 2025-04-17. **Note:** the publication is real and does contain the warp-mesh /
  separate-passthrough-and-virtual-warp / merging-compositor / POV-correction machinery cited in §2
  — but its *subject* is prism/double-vision compensation, not POV correction per se. The dedicated
  POV-warp disclosures are a different family (e.g. US11989854, US12236563 "Point-of-view image warp
  systems and methods"; US12106444 "Hierarchical grid interpolation systems and methods"), which add
  hierarchically subdivided warp grids and invalid-region skipping to save fetch bandwidth.
- Apple, [Introducing Apple Vision Pro](https://www.apple.com/newsroom/2023/06/introducing-apple-vision-pro/)
  — "R1 streams new images to the displays within 12 milliseconds".
- Play For Dream MR reviews: [note.com deep-dive](https://note.com/fleabaneh/n/n07220438ab1f)
  (14 ms low-latency vs ~40 ms high-quality; prefers low-latency despite noise) ·
  [The Ghost Howls](https://skarredghost.com/2025/09/11/play-for-dream-hands-on-review/)
  (14 ms; visible grain; lost text-readability comparison to Quest 3) ·
  [ramipastrami.engineering](https://ramipastrami.engineering/play-for-dream-mr-review-potentially-the-best-4k-standalone-vr-headset-yet/)
  (independently prefers low-latency mode; describes HQ mode's dark-area over-correction as
  "geometric noise").

**Project context:** [architecture/zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md)
(sort-last composition model, client contract, scheduling rule, one-projection-layer output),
[research/10-xr-wayland-protocol-comparison.md](10-xr-wayland-protocol-comparison.md)
(view/projection/model split, dmabuf + `wp_linux_drm_syncobj_v1` transport),
[architecture/adr/0006-compositor-strategy.md](../architecture/adr/0006-compositor-strategy.md).
