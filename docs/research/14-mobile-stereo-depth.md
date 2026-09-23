# 12 — Mobile stereo depth: the geometry proxy for passthrough

**Date:** 2026-09-22. Research pass for Mura. Depth is the geometry proxy the passthrough
compositor needs: from two rectified camera images, produce per-view disparity/depth + confidence,
fast and stable, on a Qualcomm XR2 / XR2+ Gen 2 class SoC (Adreno GPU / Hexagon NPU). This
document defines the depth-backend options and the interface the compositor consumes. The
Qualcomm hardware path (Adreno per-frame Depth-From-Stereo, CVP DFS engine) is audited in the
sibling doc **16-perception-claims-audit** — here it appears only as one pluggable backend the
interface must accommodate.

Primary sources (local clones, paths relative to repo root; `file:line` citations against these):

- `references/tc-stereo/` — jiaxiZeng/Temporally-Consistent-Stereo-Matching, ECCV 2024, MIT
  (`references/tc-stereo/LICENSE`). **Full training + inference code.** The key temporal-stereo
  reference with code.
- `references/xr-stereo/` — za-cheng/XR-Stereo, WACV 2024 "Stereo Matching in Time". **Dataset-only
  release** — see §2.2 for exactly what is and is not present.
- `references/openstereo/` — XiandaGuo/OpenStereo model zoo (LightStereo, IGEV-RT, CoEx, FADNet,
  a Fast-FoundationStereo port, …) plus ONNX/TensorRT deployment tooling.
- `references/raft-stereo/` — princeton-vl/RAFT-Stereo, 3DV 2021, MIT. Teacher / quality ceiling.

Web sources are cited inline as URLs and marked **[verified]** / **[unverified]**. Everything in
the model table (§3.4) was existence-checked on 2026-09-22.

---

## 1. Disparity → depth: the math the interface is built on

### 1.1 Triangulation and the Q matrix

For a rectified stereo pair with focal length `f` (pixels), baseline `B` (metres), and disparity
`d` (pixels), depth along the optical axis is

```
Z = f · B / d
```

This is exactly what TC-Stereo implements, with a floor on `d` to avoid the pole at zero:

```7:16:references/tc-stereo/core/utils/geo_utils.py
def disp2depth(disp, baseline, fx):
    '''
    :param disp: N,1,H,W
    :param baseline: N,1
    :param fx: N
    :return: depth: N,1,H,W
    '''
    assert not torch.isnan(disp).any() and not torch.isinf(disp).any()
    assert (disp >= 0).all()
    return baseline.view(-1, 1, 1, 1) * fx.view(-1, 1, 1, 1) / torch.clip(disp, min=0.001)
```

Full 3D reprojection is the OpenCV `Q` matrix from `cv::stereoRectify`
([docs](https://docs.opencv.org/4.x/d9/d0c/group__calib3d.html)): a homogeneous map
`[x y d 1]ᵀ ↦ [X Y Z W]ᵀ`,

```
Q = ⎡ 1  0   0      -cx           ⎤
    ⎢ 0  1   0      -cy           ⎥        X = (x−cx)/W', Y = (y−cy)/W',
    ⎢ 0  0   0       f            ⎥        Z = f/W',      W' = −d/Tx + (cx−cx')/Tx
    ⎣ 0  0  −1/Tx   (cx−cx')/Tx   ⎦
```

with `Tx = −B`. The compositor should consume `f`, `B`, `cx`, `cy`, `cx'` (i.e. the rectified
intrinsics + baseline) rather than a baked point cloud, so a backend can hand over raw disparity
and the render side can reproject in a shader. TC-Stereo's equivalent decomposition is
`pixel2point` (`references/tc-stereo/core/utils/geo_utils.py:32-42`, `P = Z · K⁻¹·[x y 1]ᵀ`) and
`point2pixel` (`geo_utils.py:45-58`).

### 1.2 Reference-view vs both-view disparity

A single stereo pass yields disparity in **one** reference view (by convention the left camera;
all four local codebases predict left-view disparity — e.g. OpenStereo's data option
`RETURN_RIGHT_DISP: false` in `references/openstereo/cfgs/lightstereo/lightstereo_s_sceneflow.yaml:9`).
Passthrough needs geometry for **both** eyes. Three options:

1. **Run the network twice** (left-ref and mirrored right-ref). Doubles cost; only acceptable if
   the model is very cheap or the two passes share the feature extraction.
2. **Reproject the left-view depth into the right view** (forward-warp, exactly the machinery in
   TC-Stereo's `warp()`, §2.1). Cheap, but produces disocclusion holes at depth edges — precisely
   the pixels where passthrough reprojection artifacts are most visible. Holes must be flagged in
   the validity mask, not inpainted silently.
3. **Left-right consistency as a by-product**: if both views are computed (even at reduced rate),
   the LR check `|D_L − warp(D_R)| < τ` is the standard confidence signal — LAS2 uses exactly this
   (τ=1 px) to filter teacher pseudo-labels ([arXiv 2606.24457](https://arxiv.org/abs/2606.24457),
   Eq. 14). The interface should allow a backend to return either one or two views and say which.

### 1.3 Subpixel precision is not optional

Depth error propagates as `dZ = (Z²/(f·B)) · dd` — quadratic in distance. For plausible headset
numbers (640×480 tracking cameras, `f ≈ 500 px`, `B ≈ 64 mm`, so `f·B ≈ 32 m·px`):

| Z | dZ per 1.0 px disparity error | per 1/8 px | per 1/32 px |
|-----|------|--------|--------|
| 1 m | 3.1 cm | 4 mm | 1 mm |
| 2 m | 12.5 cm | 1.6 cm | 4 mm |
| 4 m | 50 cm | 6.3 cm | 1.6 cm |

Integer or coarsely-quantized disparity therefore produces visible **depth terracing** on walls
and floors at 2–4 m — the dominant content of indoor passthrough — and the terraces *shimmer*
between frames as pixels flip quantization bins, which the rendered eye view amplifies into
crawling geometry. Consequences:

- The interface must carry disparity (or depth) at **≥ 1/8 px effective precision**. `R16F` is
  marginal at far range (10-bit mantissa); `R32F`, `R16` fixed-point disparity in 1/16 px units,
  or the two-channel Hilbert encoding of §3.3 are all acceptable.
- All the learned models here are subpixel by construction — softmax disparity regression
  (LightStereo: `disparity_regression` over `max_disp/4` bins,
  `references/openstereo/stereo/modeling/models/lightstereo/lightstereo.py:55-56`) or continuous
  GRU refinement (RAFT-Stereo/TC-Stereo). Classical SGBM produces 1/16 px fixed point. The
  danger is *losing* the subpixel signal at the quantization/deployment step (§4), not at the
  model level.

---

## 2. The temporal-stereo insight (the most important section for mobile)

A headset runs stereo at 30–90 Hz on scenes that are ~95 % identical frame-to-frame, with a
6-DoF pose delta known from the tracker (SLAM/VIO). Single-frame stereo throws all of that away
and re-solves from scratch each frame. The temporal-stereo family instead **spreads iterative
refinement across frames**: carry the previous disparity and the recurrent network state forward
through the known camera motion, so each new frame only needs 1–2 cheap update iterations instead
of 10–30. This simultaneously cuts per-frame cost by ~3–5× *and* improves temporal stability —
which for passthrough is worth more than mean EPE, because flicker is what users see.

Two references define the approach: TC-Stereo (code, studied below) and XR-Stereo (the XR2
deployment evidence, §2.2).

### 2.1 TC-Stereo: the mechanism, from code

TC-Stereo (`references/tc-stereo/`, [arXiv 2407.11950](https://arxiv.org/pdf/2407.11950), ECCV
2024, MIT) is a RAFT-Stereo-derived recurrent model with an explicit temporal state.

**The recurrent state.** In test mode the model returns exactly the state that must be fed to the
next frame:

```223:229:references/tc-stereo/core/tc_stereo.py
        if test_mode:
            testing_output = {'flow': torch.clip(flow_refine_up, max=0),
                              'flow_q': torch.clip(flow_q, max=0),
                              'net_list': net_list,
                              'fmap1': fmap1.detach(),
                              }
            return testing_output
```

i.e. (a) `flow_q` — quarter-resolution disparity (stored as non-positive x-flow), (b) `net_list`
— the multi-scale ConvGRU hidden states, (c) `fmap1` — the left matching-feature map. The caller
loop threads these through together with poses: `evaluate_stereo.py:77-86` builds
`params = {K, T, previous_T, baseline, last_disp, last_net_list, fmap1}` per frame.

**Forward-projecting historical disparity** (`tc_stereo.py:119-140`): the previous disparity is
converted to depth, lifted to 3D, moved by the relative pose `T_prev→cur`
(`cal_relative_transformation`, `geo_utils.py:148-155`), and **forward-splatted** into the current
view together with the previous feature map:

```193:198:references/tc-stereo/core/utils/geo_utils.py
    forward_flow = current_coords - coords0
    metric = (current_disp - current_disp.mean()).clamp(-50, 50)
    # disp&fmap concat and warp
    feats = torch.cat((current_disp, fmap), dim=1)
    feats, warped_mask = softsplat(feats, forward_flow, metric, 'soft-clipeps', valid_mask.float())
    current_disp, current_fmap = feats[:, :1], feats[:, 1:]
```

Softmax splatting weighted by disparity resolves z-fighting (nearer surfaces win); the returned
`warped_mask` marks pixels that received any evidence. A **temporal photometric-consistency
confidence** is then computed as cosine similarity between the current features and the warped
previous features, and used to gate the warped disparity (`tc_stereo.py:139-140`:
`cost = Σ normalize(fmap1)·normalize(warped_fmap1); cost *= sparse_mask`). On the very first
frame there is no history; initialization falls back to a winner-take-all argmax over the cost
volume with a ratio test (`main_cost − second_cost > 0.3`) that yields a *sparse, confident*
disparity + mask (`references/tc-stereo/core/corr.py:67-79`) — note this is a per-pixel
**confidence estimate for free**, worth exposing in any backend.

**Temporal completion module.** The warped/sparse disparity has holes (disocclusions, low
confidence). `DisparityCompletor` (`references/tc-stereo/core/update.py:308-399`) embeds
(sparse disp, cost, mask), runs a small encoder-decoder over context features, predicts a dense
mono-style disparity `disp_mono` and a blending weight `w`, and outputs
`disp_completed = w·disp_sparse + (1−w)·disp_mono` (`update.py:392`). This dense completed
disparity initializes the RAFT iteration (`tc_stereo.py:146-152, 170-172`) — so the expensive
part of convergence has already been paid for by previous frames.

**Hidden-state warping and fusion.** The GRU hidden states are also carried across frames, but
warped *backward*: `get_backward_grid` (`geo_utils.py:201-236`) maps each current pixel into the
previous frame using the *completed current* disparity, and the previous `net_list` is
bilinear-sampled through that grid (`tc_stereo.py:155-163`, one grid per pyramid level). Warped
and freshly-computed hidden states are merged by `Lightfuse`, a 1×1-conv GRU-style gate
(`update.py:20-36`, applied at `tc_stereo.py:90-94, 166-168`) — the network learns per-pixel how
much to trust history vs the current frame.

**Refinement in disparity space AND disparity-gradient space.** Each iteration
(`tc_stereo.py:175-202`) does two things:

1. *Disparity space*: standard RAFT-style lookup of the 1D correlation pyramid at the current
   estimate (`corr.py:33-52`) → multi-scale ConvGRU → `delta_flow`
   (`BasicMultiUpdateBlock`, `update.py:127-168`).
2. *Disparity-gradient space*: compute `∇d` (`disp2disp_gradient_xy`, `geo_utils.py:115-132`),
   refine it with `DispGradPredictor` (`update.py:171-214`) using local plane candidates built
   from cross-products of neighboring (x, y, d) difference vectors
   (`disp2disp_grad_candidates`, `geo_utils.py:73-101`), then `DispRefine`
   (`update.py:217-305`) **propagates disparity along the refined gradients**: each pixel gets 9
   candidates `d_neighbor + ∇d_neighbor·Δx` (planar extrapolation from its 8-neighborhood,
   `propagate_disparity`, `update.py:259-289`) fused by a learned softmax (`update.py:291-305`).
   Working in gradient space is what preserves **planar surfaces and depth boundaries** — the
   two things quantized/smoothed mobile depth gets wrong — and the paper's motivation is
   explicitly that temporal accumulation otherwise over-smooths edges. Finally
   `HiddenstateUpdater` injects the applied correction `Δd` back into the finest hidden state
   (`update.py:48-68`, called at `tc_stereo.py:199-200`) so the recurrent state stays consistent
   with the refined disparity.

**Mobile relevance and caveats.** The design shows exactly what state a temporal backend carries:
`{disparity, hidden states, feature map, pose}` — roughly 1/4-res × (1 + Σhidden + 256) channels
of float state. But this exact implementation is not deployable as-is: `softsplat` is a custom
CUDA kernel (`references/tc-stereo/core/utils/splatting/softsplat.py:277-285`), and forward
scatter + dynamic `grid_sample` + pyramid lookups are hostile to NPU compilers (§3.2). It is the
*teacher-of-architecture*, not the artifact to ship.

### 2.2 XR-Stereo: the XR2 evidence — and what is actually released

**What the repo contains (verified locally):** `references/xr-stereo/` holds exactly three files —
`README.md`, `LICENSE`, `paper.png` (`ls` on 2026-09-22). It is a **dataset release**: 640×480
photorealistic indoor stereo video (57.4 GB subset, SceneFlow format, CC BY 4.0; the full ~4 TB
by contacting the authors), rendered with real 6-DoF HMD trajectories
(`references/xr-stereo/README.md`). **There is no network implementation, no training code, no
ONNX export path in the repo.** Anything built on XR-Stereo-the-algorithm is a reimplementation
from the paper.

**What the paper establishes** ([WACV 2024 open access
PDF](https://openaccess.thecvf.com/content/WACV2024/papers/Cheng_Stereo_Matching_in_Time_100_FPS_Video_Stereo_Matching_for_WACV_2024_paper.pdf),
verified): a RAFT-Stereo-like iterative cost aggregation **unrolled in time** — instead of N GRU
iterations per frame, run 1–2 per frame and warp the aggregated features/disparity forward through
the known pose, reusing computation across frames. Headline numbers (their Table 2, XR-Stereo
dataset, 640×480):

- **Full model**, 1 GRU iteration/frame: EPE 1.48, 108 fps on an RTX 3090 Ti; 5 iters: EPE 1.42,
  57 fps.
- **Fast model**: EPE 1.67 at **134 fps** desktop, and — the number that matters here — **30 fps
  on a battery-powered HMD with a Qualcomm XR2** ("we convert the fast model via ONNX to a
  device-friendly float-point format running on Qualcomm XR2 chip **without quantization**",
  §implementation). The fast variant differs from the full model in exactly three ways: it
  (a) **removes temporal warping** (assumes small, continuous inter-frame motion), (b) **halves
  the intermediate feature channels** in the encoder, and (c) runs **one recurrent update per
  frame**.
- Temporal warping matters under motion: without it, EPE degrades from 1.70→3.18 at 6× playback
  speed, with it only 1.48→1.68 (their Table 4/Figure 8). So the fast variant's warp removal is a
  deliberate trade that ties depth quality to head-motion speed.
- Pose sensitivity: needs only *relative* inter-frame pose; a modified ORB-SLAM3 on their headset
  stays within 0.3° / 0.5 mm inter-frame error, below the noise level where accuracy degrades
  (their Figure 6). Mura gets this pose from Monado's tracker.
- Cold start: accuracy converges over the first handful of frames as temporal aggregation fills in
  (their Figure 7) — a backend must report warm-up (e.g. via confidence) rather than emit garbage.

Together: **TC-Stereo supplies the studied mechanism with code; XR-Stereo supplies the existence
proof that this class of model runs at 30 fps at 640×480 on the exact silicon Mura targets —
in plain float, no quantization, via ONNX.** That existence proof is the single most
load-bearing external fact in this document.

---

## 3. Efficient architectures survey: what to run on the device

### 3.1 2D cost aggregation — the mobile design point

The classical accuracy recipe (PSMNet lineage) builds a 4D concat volume and regularizes with 3D
convolutions — memory- and compute-prohibitive on mobile, and 3D conv support in NPU toolchains is
poor. The efficient family replaces this with a **3D correlation volume** (`H/4 × W/4 × D/4`)
treated as a 2D tensor whose *channels are disparities*, aggregated with plain 2D convs:

- **LightStereo** (ICRA 2025, [arXiv 2406.19833](https://arxiv.org/abs/2406.19833) **[verified]**;
  code in the local zoo). Pipeline in
  `references/openstereo/stereo/modeling/models/lightstereo/lightstereo.py:44-71`: MobileNetV2
  backbone → 1/4-res correlation volume (`lightstereo.py:51`, `max_disp/4 = 48` channels) →
  inverted-residual 2D aggregation at 1/4, 1/8, 1/16 with left-image attention
  (`aggregation.py:8-50`) → softmax disparity regression (`lightstereo.py:55-56`) → learned 9-tap
  convex upsampling to full res (`lightstereo.py:58-62`). Configs for S/M/L/LX in
  `references/openstereo/cfgs/lightstereo/` (S: `MAX_DISP: 192`, `AGGREGATION_BLOCKS: [1,2,4]`,
  `EXPANSE_RATIO: 4` — `lightstereo_s_sceneflow.yaml:23-28`). Paper: LightStereo-S = 22.7 GFLOPs,
  EPE 0.73 SceneFlow, 17 ms on RTX 3090. Purely feed-forward, static shapes, no warping — the
  most NPU-shaped model in the local zoo.
- **BANet** ([arXiv 2503.03259](https://arxiv.org/abs/2503.03259), ICCV 2025,
  [gangweix/BANet](https://github.com/gangweix/BANet) **[verified — real 2025 work with an actual
  Snapdragon measurement]**). Splits the correlation volume into *detail* and *smooth* parts via a
  scale-aware spatial attention map, aggregates each with 2D convs, fuses — recovering the edge
  sharpness that pure-2D aggregation (MobileStereoNet-2D) loses, without deformable convs or
  iterative warping, which the paper explicitly rejects as not mobile-deployable. **Measured:
  45 ms @ 512×512 on Snapdragon 8 Gen 3, breakdown 16 ms feature extraction + 6.5 ms correlation
  volume + 22.5 ms bilateral aggregation** (paper §4, latency paragraph). This breakdown is the
  single most useful budgeting datum: features and aggregation dominate; correlation is cheap.
- **LAS2 / Lite Any Stereo V2** ([arXiv 2606.24457](https://arxiv.org/abs/2606.24457),
  [TomTomTommi/LiteAnyStereo](https://github.com/TomTomTommi/LiteAnyStereo) **[verified: repo
  exists and contains `export_onnx.py`, `run_onnx.py`, `verify_onnx.py`, `profile_speed.py`
  (GitHub tree listing, 2026-09-22)]**). 2D-only aggregation, FasterNet backbone chosen for
  *measured* latency over MACs; S/M/L feed-forward + H iterative variants; three-stage training —
  synthetic supervision → self-distillation under photometric perturbation → **knowledge
  distillation from FoundationStereo on 0.5 M unlabeled real pairs** with LR-consistency
  pseudo-label filtering (τ=1 px) and error clamping. Zero-shot SOTA among efficient models.
  **Timings are H200 and Jetson Orin NX 8G at 384×1248 — no QNN/Snapdragon numbers** (their
  Table II): LAS2-S 6.6/81 ms, M 8.1/101 ms, L 11.4/166 ms, H 15.1/344 ms; same-table baselines:
  LightStereo-S 7.1/89 ms, BANet-2D 11.6/126 ms, Fast-FoundationStereo 27.3/918 ms.
- **Fast-FoundationStereo** ([NVlabs/Fast-FoundationStereo](https://github.com/NVlabs/Fast-FoundationStereo),
  CVPR 2026, [arXiv 2512.11130](https://arxiv.org/abs/2512.11130) **[verified]**). Distills the
  FoundationStereo hybrid backbone into one student, blockwise-NAS's the cost filtering, prunes
  the iterative refiner; 1.4 M pseudo-labeled in-the-wild pairs; >10× faster than FoundationStereo
  at near-equal zero-shot accuracy; ONNX/TensorRT export. **License caveat: NVIDIA non-commercial
  research license** (stated by derivative repos, e.g.
  [Fast-FoundationStereo-TRT](https://github.com/ruisv/Fast-FoundationStereo-TRT): "distributed
  under the same NVIDIA non-commercial research license"; the OpenStereo-vendored copy carries
  `references/openstereo/stereo/modeling/models/fast_foundationstereo/LICENSE.txt`, © 2026 NVIDIA)
  — **unusable in a shipping Mura image; fine as an offline teacher only if the license's
  research scope covers generating training data — needs legal reading.** Also simply too heavy
  for XR2-class silicon (918 ms on Orin NX, which is faster than an XR2 NPU for fp16).

### 3.2 What makes a model NPU-friendly

Distilled from the QNN/SNPE constraints visible in these papers plus the code studied above:

- **Static shapes end-to-end.** NPU compilers (QNN, SNPE) ahead-of-time compile one resolution.
  Fixed input size, fixed `max_disp`. The interface must not assume runtime-variable resolution.
- **Bounded, small disparity range.** Cost-volume channel count = `max_disp/4`; at 640×480 with a
  64 mm baseline, 96–128 px max disparity (Z_min ≈ 25–33 cm) suffices and halves aggregation cost
  vs the KITTI-default 192.
- **Simple correlation.** A one-shot dot-product correlation volume (LightStereo
  `correlation_volume`, BANet) lowers to matmul/conv primitives. RAFT-style *iterative pyramid
  lookup* (`bilinear_sampler` at data-dependent coordinates each iteration,
  `references/tc-stereo/core/corr.py:33-52`) is a dynamic gather — slow or unsupported on NPUs;
  it is why XR-Stereo's XR2 deployment ran float and why BANet avoids warping entirely.
- **No large dynamic gather/warp/scatter in the hot loop.** TC-Stereo's `softsplat` forward
  scatter is CUDA-only (`softsplat.py:277-285`); backward `grid_sample` warps of hidden state are
  gather-heavy. If temporal warping is wanted on-device, do it on the **Adreno GPU in a fragment
  shader** (a reprojection pass is exactly what compositors do anyway) and keep the NPU graph
  feed-forward.
- **BN/activation choices**: BatchNorm folds into convs; InstanceNorm (used in LightStereo's
  refinement, `lightstereo.py:29-33`, and widely in TC-Stereo) often does *not* fold and can
  break int8 range assumptions — check per-op support before picking a checkpoint.
- **On-chip memory / tiling** (see §4): channel-fat intermediate tensors (the D/4-channel volume
  at 1/4 res) must fit tile buffers or the compiler spills to DDR and the "45 ms" becomes 150 ms.

### 3.3 Quantization pitfalls: depth terracing and the Hilbert-curve fix

Verified: **"Predicting High-precision Depth on Low-precision Devices Using 2D Hilbert Curves"**,
ICML 2025, [arXiv 2405.14024](https://arxiv.org/abs/2405.14024) **[verified — measured on the
Samsung S24+ Hexagon DSP]**. The problem it names is exactly §1.3's terracing, moved from the
model to the deployment stack: an INT8 (W8A8) output tensor can represent only 256 disparity/depth
levels — representing 0–10 m at 1 cm needs 10 bits — so quantized depth shows **false edges on
planar surfaces** regardless of how good PTQ/QAT is. SNPE compounds this by requiring **one
bit-width for all layers** (paper §1, citing the SNPE docs), so "keep the last layer fp" is not
an option on Hexagon.

The fix: train the head to output the **two components (x(q), y(q)) of a 2D Hilbert curve** of
order k instead of scalar depth; both components are low-dynamic-range and quantize well; a
CPU-side LUT (256×256) inverts the curve to recover `q` with `+log₂L ≈ up to 3 extra bits` —
**INT10–11 effective depth from two INT8 channels** — plus an up-to-4.6× reduction in
quantization error (an averaging effect along the curve). Measured (their Table 2, DispNet,
input 384×512 → output 192×256, SNPE 2.24, S24+ / Snapdragon 8 Gen 3 Hexagon):

| Precision | EPE (px) | D1 % | latency | power |
|---|---|---|---|---|
| FP16 | 0.37 | 1.80 | 19.5 ms | 19.5 mW·s |
| W8A16 | 0.63 | 5.22 | 18.7 ms | 12.3 |
| W8A8 (plain) | 0.69 | 5.34 | 10.5 ms | 7.1 |
| **W8A8 + Hilbert** | **0.24** | **1.26** | **12.0 ms** | 8.7 |

i.e. the Hilbert-encoded W8A8 model beats even W8A16 quality at ~2/3 the latency and power —
**~12 ms per frame for a full stereo network on a phone-class Hexagon**. The technique is
head+LUT only (~14 % overhead), composable with *any* backbone in §3.1, and directly relevant to
Mura because passthrough will see every terrace on every wall. Directly informs the
interface: a backend may legitimately return depth as **two 8-bit channels + encoding tag**
rather than one float channel.

### 3.4 Model table

Latencies are as reported by the cited source; hardware differs per row — do not compare across
rows without noting the column.

| Model | Reported latency | Hardware | Resolution | Released? | License |
|---|---|---|---|---|---|
| TC-Stereo (ECCV'24) | no mobile number (desktop GPU work) | CUDA GPU | 640×480-class | **yes, full code + weights** (`references/tc-stereo/`) | MIT **[verified locally]** |
| XR-Stereo fast (WACV'24) | **30 fps on XR2 HMD**, ONNX float; 134 fps RTX 3090 Ti | Qualcomm XR2 / RTX 3090 Ti | 640×480 | **dataset only — no network code** (`references/xr-stereo/`) | dataset CC BY 4.0 **[verified locally]** |
| LightStereo-S (ICRA'25) | 17 ms (paper) / 7.1 ms H200, 89 ms Orin NX (LAS2 re-measure) | RTX 3090 / H200 / Orin | SceneFlow / 384×1248 | yes, in OpenStereo zoo + ONNX/TRT export | Apache-2.0 (OpenStereo) **[verified]** |
| BANet-2D (ICCV'25) | **45 ms @ 512×512** (16 feat / 6.5 corr / 22.5 agg) | **Snapdragon 8 Gen 3** | 512×512 | yes, [github](https://github.com/gangweix/BANet) **[verified]** | repo license not confirmed **[unverified]** |
| LAS2-S/M/L/H (2026) | 6.6–15.1 ms H200; 81–344 ms Orin NX 8G; **no Snapdragon numbers** | H200 / Jetson Orin NX | 384×1248 | yes, [github](https://github.com/TomTomTommi/LiteAnyStereo) incl. `export_onnx.py` **[verified]** | LICENSE present, type unconfirmed **[unverified]** |
| Fast-FoundationStereo (CVPR'26) | 27.3 ms H200 / 918 ms Orin (LAS2 table); ~70 ms RTX 2060 TRT (community) | H200 / Orin / RTX 2060 | 384×1248 | yes, [NVlabs](https://github.com/NVlabs/Fast-FoundationStereo) **[verified]** | **NVIDIA non-commercial** — blocker for shipping |
| Hilbert-curve depth (ICML'25) | **12.0 ms W8A8** (DispNet-based) | **S24+ Hexagon DSP** (8 Gen 3) | 384×512 in / 192×256 out | paper technique; no standalone repo found **[unverified code]** | n/a |
| RAFT-Stereo (3DV'21) | offline; "realtime" config needs `reg_cuda` CUDA kernels (`references/raft-stereo/core/raft_stereo.py:90-98`, README:105) | CUDA GPU | any | yes (`references/raft-stereo/`) | MIT **[verified locally]** |
| SGBM (OpenCV) | ~10–30 ms CPU at VGA (well-known; measure locally) | any CPU | any | yes | Apache-2.0 |

---

## 4. Deployment reality: "small on desktop" ≠ "runs on the NPU"

Three gaps between a paper's GFLOPs table and a working XR2 backend:

1. **Compilation.** The model must lower to the QNN/SNPE op set with static shapes. One
   unsupported op (grid_sample, scatter, InstanceNorm, dynamic reshape) silently falls back to
   CPU/GPU with a per-layer sync, and latency explodes. This is why every verified mobile number
   in §3 comes from a feed-forward 2D-aggregation model (BANet, DispNet-Hilbert) or a float ONNX
   graph with the dynamic parts amputated (XR-Stereo fast: warping removed, one GRU step).
2. **On-chip memory and tiling.** Hexagon executes from tightly-sized TCM/VTCM; the compiler tiles
   activations through it. The `(D/4)×(H/4)×(W/4)` cost volume plus skip connections is the
   watermark tensor: at 640×480/D=128 it's 48×120×160 ≈ 0.9 M values — fine; at 1024×1024/D=192
   it's ~12 M and the schedule starts thrashing DDR. **Latency scales super-linearly past the
   tiling cliff**, so "45 ms @ 512×512" does not extrapolate; every resolution must be re-measured
   on-device. GFLOPs and parameter count predict neither cliff (LAS2 makes this exact argument
   against MACs, [arXiv 2606.24457](https://arxiv.org/abs/2606.24457) §I).
3. **Precision.** fp32 doesn't exist on the NPU path; fp16 (XR-Stereo's choice) costs ~2× W8A8
   latency and power (Hilbert paper Table 2: 19.5 ms vs 10.5 ms, 19.5 vs 7.1 mW·s). Going W8A8
   without §3.3-style output encoding buys terracing. Note SNPE's uniform-bit-width constraint.

**Evaluate by rendered eye-view error, not disparity EPE.** A quantized/temporal model should be
judged by what the compositor produces: reproject the passthrough camera image into the eye view
through the *predicted* depth, compare against reprojection through ground-truth (or teacher)
depth, and measure (a) photometric error concentrated at depth edges, (b) **frame-to-frame jitter
of the rendered view under a static scene with a moving head** — the metric users actually
perceive. Disparity EPE weights a 5 px error on a distant wall the same as on a hand at 40 cm;
rendered-view error weights them by where pixels land in the eye view, penalizes edge fattening
(the classic SGBM artifact) and temporal shimmer (the classic quantization artifact), both nearly
invisible in EPE. TC-Stereo's own evaluation motivation is the temporal version of this point.

---

## 5. The Mura depth-backend interface

What the passthrough compositor consumes, designed so classical, learned, and hardware backends
are interchangeable. Sketch (names illustrative):

```
DepthFrame {
    capture_timestamp_ns        // of the *camera exposure*, not inference completion
    calibration_version         // monotonic; bumps when rectification maps/extrinsics change
    view_mask                   // LEFT | RIGHT | BOTH — which eye(s) this frame covers
    per view:
        disparity               // subpixel: R32F, R16 fixed-point (1/16 px), or 2×R8 Hilbert + LUT id
        encoding                // FLOAT | FIXED_16(frac_bits) | HILBERT8(order)
        confidence              // R8: 0=hole/disoccluded/cold-start, else backend confidence
        validity                // bitmask: measured / temporally-propagated / completed / hole
    intrinsics {f, cx, cy}, baseline, rectified pose per view   // enough to build Q (§1.1)
    backend_id, backend_frame_seq   // provenance + warm-up counting
}
```

Design decisions, each traceable to the research above:

- **Per-view depth + explicit view_mask** — §1.2. A backend may compute left-only and let the
  compositor reproject, but then holes *must* appear in `validity`, not be inpainted silently.
- **Subpixel disparity with declared encoding** — §1.3 + §3.3. The Hilbert two-channel encoding is
  a first-class citizen so a W8A8 Hexagon backend doesn't have to dequantize on CPU; the
  compositor decodes via LUT in the same shader that samples the depth.
- **Confidence/validity always present** — every studied backend produces one natively: TC-Stereo's
  ratio-test mask (`corr.py:67-79`) and completion weight `w` (`update.py:392`), softsplat's
  `warped_mask`, LR-consistency checks (LAS2 Eq. 14), SGBM's uniqueness/speckle filters, and the
  Adreno/CVP DFS engine's own confidence plane (see doc 14). Distinguishing *measured* vs
  *temporally-propagated* vs *hallucinated-by-completion* pixels lets the renderer degrade
  gracefully (e.g. stiffen the mesh where propagated).
- **Capture timestamp, not publish timestamp** — the compositor forward-predicts geometry to
  display time using the tracker pose at `capture_timestamp`; a 45 ms-old depth map is fine *if
  labeled*, poison if it claims to be current.
- **Calibration version** — rectification maps change (thermal, factory recalib, user IPD
  adjustments); depth produced under an old calibration must be droppable atomically.
- **Pose is an input, not part of this interface**: temporal backends additionally *subscribe* to
  the tracker (relative pose per frame, §2.1/§2.2); the interface stays pure-output.
- **Pluggable backends**: (a) **classical** — OpenCV SGBM on CPU/GPU, the day-1 fallback, no NPU
  toolchain dependency, 1/16-px fixed point maps directly onto `FIXED_16(4)`; (b) **learned** —
  a TC-Stereo-class or LightStereo-class network on Hexagon/Adreno via ONNX→QNN; (c) **hardware**
  — the Adreno/CVP Depth-From-Stereo block, which produces disparity+confidence at fixed
  resolution and zero NPU cost (claims audited in sibling doc **16-perception-claims-audit**;
  interface only needs to make room for it, hence `FIXED_16` and per-backend resolution).
- **Teacher–student loop as a project workflow**: RAFT-Stereo (MIT, `references/raft-stereo/`) or
  FoundationStereo-class models run *offline* over sequences captured from the actual headset
  cameras to produce pseudo-labels; the small on-device student is fine-tuned on that domain
  (fisheye-rectified, global-shutter, IR-ish, indoor). This is precisely the LAS2 stage-③ recipe
  (0.5 M pairs, LR-check filter, error clamping) and Fast-FoundationStereo's pipeline (1.4 M
  pairs) — both validate that student-on-pseudo-labels closes most of the domain gap. License
  note: an MIT teacher (RAFT-Stereo) avoids the Fast-FoundationStereo non-commercial question
  entirely, at some quality cost.

---

## 6. What Mura should adopt / reject

**Adopt:**

1. **The interface of §5 now**, before any model work — it decouples compositor development from
   the depth-backend roadmap, and the SGBM backend can be implemented in a day.
2. **SGBM as backend #0** (fallback + harness bring-up + a floor to beat).
3. **LightStereo-S as the first learned backend candidate**: full code + configs + ONNX/TRT export
   in the local zoo (`references/openstereo/deploy/export.py`, §3.1), Apache-2.0, feed-forward
   static-shape 2D aggregation — the shortest path through the QNN toolchain. Retrain at 640×480
   / `MAX_DISP: 128` on SceneFlow + TartanAir + the XR-Stereo dataset (which is licensed CC BY 4.0
   precisely for this), then teacher-distill on headset captures (RAFT-Stereo teacher). Take
   BANet's bilateral-aggregation idea as the upgrade if edges are the observed weakness — its
   45 ms @ 512×512 on 8 Gen 3 bounds what XR2+ Gen 2 (same Adreno/Hexagon family, lower clocks)
   can do: expect ~real-time only at ≤ VGA.
4. **The temporal recipe as phase 2**: add TC-Stereo-style state (previous disparity + hidden
   state, forward-warped by tracker pose) around the LightStereo-class core, with the warp done as
   an Adreno shader pass (§3.2), one refinement step per frame — the XR-Stereo fast/full evidence
   says this is where 30 fps-on-XR2 with good stability actually lives.
5. **Hilbert output encoding** the moment any backend is quantized W8A8 (§3.3) — it is a head +
   LUT change, not an architecture change.
6. **Rendered-eye-view evaluation** (§4, harness below).

**Reject:**

- **Fast-FoundationStereo on-device or in-image** — non-commercial license + far over budget
  (918 ms Orin NX). At most an offline teacher, pending license reading.
- **TC-Stereo's literal implementation on-device** — CUDA softsplat, dynamic lookups (§2.1). Its
  *ideas* (temporal completion, gradient-space refinement, confidence-gated fusion) are the
  adoption; its graph is not.
- **RAFT-Stereo(-realtime) on-device** — `reg_cuda` correlation kernels, iterative lookups; it is
  the teacher / quality ceiling (`references/raft-stereo/`), full stop.
- **Trusting any latency number not measured on Snapdragon** — of everything surveyed, only BANet
  (45 ms @ 512×512), the Hilbert paper (12 ms @ 384×512, different task size), and XR-Stereo
  (30 fps @ 640×480, fp, XR2) are Qualcomm-measured. H200/Orin numbers rank models; they do not
  budget frames.
- **Disparity-screenshot evaluation** — single-frame EPE comparisons say nothing about terracing
  or temporal shimmer (§4).

**Evaluation harness (concrete):** record fixed sequences from the headset cameras (rectified
pairs + Monado poses + timestamps, ~30–60 s each: static room with moving head; moving hands;
low light; textureless wall approach). Every backend runs the *same* sequences through the §5
interface. Measure per backend: (a) end-to-end latency on-device (capture→DepthFrame, p50/p99);
(b) rendered eye-view photometric error vs teacher-depth rendering (RAFT-Stereo offline on the
same sequences); (c) temporal jitter — per-pixel std-dev of rendered luminance over a static-scene
window, and depth-edge flicker rate; (d) hole/confidence statistics. A disparity image is a debug
view, never the score.

---

## 7. Open questions

1. **What does the QNN compiler actually do to LightStereo-S at 640×480?** Ops coverage,
   VTCM tiling behavior, fp16-vs-W8A8 latency on XR2+ Gen 2 specifically — nobody has published
   this; it must be measured (BANet's 8 Gen 3 numbers are the nearest proxy).
2. **NPU/GPU split for the temporal loop**: warp-on-Adreno + refine-on-Hexagon means a
   GPU↔NPU round-trip per frame. Is the interconnect cost (and AHB/fabric contention with the
   compositor's own rendering) below the ~5 ms it saves? Zero-copy paths (dmabuf into QNN)
   need verification on this SoC.
3. **Camera domain gap**: headset tracking cameras are fisheye, global-shutter, often monochrome
   with IR sensitivity; all surveyed training data is pinhole RGB. Does rectification (with its
   resolution loss at the periphery) plus teacher-distillation close the gap, or is
   native-fisheye stereo (epipolar curves, not lines) eventually required?
4. **Pose-error robustness at Mura quality bar**: XR-Stereo tolerates 0.3°/0.5 mm
   inter-frame noise (§2.2) — Monado's tracker on the target device needs characterizing against
   that bound, including during fast rotation where passthrough matters most.
5. **Both-view strategy cost**: shared-backbone two-view inference vs one-view + shader
   reprojection with hole marking — which wins at equal rendered-view quality? (§1.2; needs the
   harness.)
6. **Hilbert encoding in the compositor**: does the 256×256 LUT decode fold into the reprojection
   shader for free on Adreno, and does the encoding interact with linear filtering of the depth
   texture (it shouldn't — decode must precede any interpolation; nearest-sample + decode + own
   filtering)?
7. **Adreno/CVP DFS as a co-backend**: if the hardware block produces usable
   disparity+confidence for near-field (hands) at zero NPU cost, a hybrid — DFS for the near
   field, learned network for the room — may beat either alone. Depends entirely on the audit in
   doc 14.
8. **XR-Stereo dataset licensing chain**: CC BY 4.0 for the 640×480 subset is fine for training;
   confirm attribution requirements propagate correctly into distributed model weights under the
   Mura licensing policy.
