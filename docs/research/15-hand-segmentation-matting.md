# 13 — Egocentric hand segmentation & matting for the passthrough hand cutout

**Scope:** what zxr-shell-v2 needs in order to composite the user's real hands/upper limbs OVER
virtual content per a visibility policy (the visionOS `.visible`/`.hidden`/`.automatic` model), and
what the studied codebases actually provide. **Date:** 2026-09-22.

**Sources studied locally:** `references/ego2hands/`, `references/egohos/`,
`references/robust-video-matting/`, `references/lightweight-hand-segmentation/`,
`references/monado/` (Mercury). Web sources are cited by URL and flagged where unverified.
Compositor context: [zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md),
[ADR 0006](../architecture/adr/0006-compositor-strategy.md).

---

## 1. The four artifacts, and why they get conflated

"Hand tracking" and "hand cutout" are routinely collapsed into one feature. They are four distinct
artifacts with different producers, different consumers, and different failure modes:

| # | Artifact | What it is | Producer | What it is NOT |
|---|---|---|---|---|
| 1 | **Joints** | 26 3D joint poses per hand (`xrt_hand_joint_set`, `XRT_HAND_JOINT_COUNT 26`, `xrt_defines.h:1455`) | Monado Mercury | a pixel-accurate silhouette |
| 2 | **Segmentation mask** | per-pixel binary/semantic labels (bg / left hand / right hand) in a camera image | a segmentation net (Ego2Hands CSM, EgoHOS, …) | usable directly at soft edges |
| 3 | **Alpha matte** | per-pixel fractional coverage α plus a foreground-colour estimate F | a matting net (RVM-style) or a refinement pass over #2 | a mask; α ∈ [0,1], not {0,1} |
| 4 | **Hand depth** | per-pixel metric depth of the hand surface, same timestamp as #2/#3 | joint-capsule render + stereo refinement | the scene depth map; hands are usually *removed* from those (see Meta, §6) |

The conflation happens because #1 is the only artifact today's Linux XR stack produces, and because
Monado's source contains the words "hand masks" (§2) — which turn out to be bounding boxes. The
compositor needs **#3 and #4, aligned to one capture timestamp**: an alpha matte to cut the hand out
of the passthrough image and composite it over virtual content, and a hand depth image to (a)
reproject the matte from the camera into each eye and (b) drive `.automatic` depth ordering. #1 is a
**prior** that makes producing #2–#4 cheaper and more robust; #2 is an intermediate on the way to #3.

### The matting equation, and why a binary mask is wrong at edges

A camera pixel on a hand boundary is a mixture (image formation / compositing equation):

```
I = α·F + (1−α)·B        # I: camera pixel, F: hand colour, B: real background, α: coverage
```

The compositor wants the hand over *virtual* content V:

```
C = α·F + (1−α)·V
```

Two consequences, both load-bearing:

1. **You cannot reuse the camera pixel `I` as the foreground.** Substituting F≈I gives
   `C = α·I + (1−α)·V = α²F + α(1−α)B + (1−α)V` — the real background B bleeds into the composite as
   a halo exactly where α is fractional (motion blur, defocus, fine finger edges). A matting model
   therefore predicts **both α and F** (or equivalently premultiplied `α·F`). RVM does precisely
   this: its projection head emits 4 channels split into a 3-channel foreground residual plus
   1-channel alpha (`references/robust-video-matting/model/model.py:32,59-65`), and its composite is
   `com = fgr*pha + bgr*(1-pha)` (`references/robust-video-matting/inference.py:135`).
2. **A binary mask is the α∈{0,1} degenerate case.** Acceptable for a first prototype (with edge
   feathering as fake α), visibly wrong on moving hands: hard 1-px staircase edges against virtual
   content, flickering fringes under motion blur. This is why "segmentation" (artifact #2) and
   "matting" (artifact #3) are different problems with different literatures.

The natural wire format toward the compositor is **premultiplied-alpha RGBA (αF, α) + one depth
image + the capture timestamp** — the same contract Apple imposes on Metal clients (clear colour
`(0,0,0,0)`, premultiplied alpha, reverse-Z depth; §6).

---

## 2. The Mercury prior: what joints give you, what they don't

Mercury (`references/monado/src/xrt/tracking/hand/mercury/`) is Monado's optical hand tracker:
stereo greyscale frames in, two 26-joint `xrt_hand_joint_set`s out. Reading `hg_sync.cpp`
end-to-end, the pipeline per frame pair is:

1. **Input:** two L8 views wrapped as `CV_8UC1` (`hg_sync.cpp:736-737`) — Mercury is *already*
   monochrome-native, which matters for §3.
2. **Detection (when a hand isn't tracked):** `grayscale_detection_160x160.onnx`
   (`hg_model.cpp:299`) on a letterboxed downscale; per-hand existence confidence, gated at 0.92
   summed over views (`hg_sync.cpp:422-427`) with a per-view floor of 0.3
   (`hg_sync.cpp:33`).
3. **Keypoints:** per hand per view, `grayscale_keypoint_jan18.onnx` (`hg_model.cpp:438`) on an ROI
   crop; outputs `heatmap_xy`, `heatmap_depth`, `scalar_extras`, `curls` (`hg_model.cpp:761`). The
   depth head is a 22-bin distribution per joint, decoded by centre-of-mass into *relative* depth
   spanning ±1.5 of the ROI scale (`hg_model.cpp:129-182`) — a genuine per-joint depth signal, but
   coarse and relative.
4. **Kinematic fit:** Levenberg–Marquardt optimizer fuses both views' 2D keypoints (+ a little of
   the depth head — `amt_use_depth` defaults to 0.01, `hg_sync.cpp:1201-1204`) into a 26-joint
   hand model; joint radii are then applied via `u_hand_joints_apply_joint_width`
   (`hg_sync.cpp:958`) — so **Mercury already carries per-joint capsule radii**, usable for a
   geometry-mask fallback and a depth prior (§5).
5. **Reprojection / ROI prediction:** the fitted 26 joints are reduced to 21 (wrist + 5×4,
   `hg_sync.cpp:489-502`), linearly extrapolated from the last two frames
   (`hg_sync.cpp:541-551`, lerp factor 0.4, `hg_sync.cpp:1196-1199`), and reprojected into each
   camera through the calibrated distortion model (`back_project`, `hg_sync.cpp:164-262`) to
   produce next frame's per-view square ROI (`hg_sync.cpp:226-245`).

### The critical correction: Monado's "hand masks" are bounding boxes

Finding `xrt_hand_masks_sample` / `masks_sink` in the Monado tree does **not** mean a per-pixel
cutout exists. The struct is explicit:

```154:168:references/monado/src/xrt/include/xrt/xrt_tracking.h
/*!
 * Masks (bounding boxes) of different hands from current views
 */
struct xrt_hand_masks_sample
{
	struct xrt_hand_masks_sample_camera
	{
		bool enabled; //!< Whether any hand mask for this camera is being reported
		struct xrt_hand_masks_sample_hand
		{
			bool enabled;             //!< Whether a mask for this hand is being reported
			struct xrt_rect_f32 rect; //!< The mask itself in pixel coordinates
		} hands[2];
	} views[XRT_TRACKING_MAX_CAMS];
};
```

One `xrt_rect_f32` per hand per view. Mercury fills it inside `predict_new_regions_of_interest`
(`hg_sync.cpp:510,559-585`): the predicted ROI, inflated by 1.25 for uncertainty
(`hg_sync.cpp:571-574`), pushed to the optional sink (`hg_sync.cpp:583-585`, wired from
`create_info.masks_sink` at `hg_sync.cpp:1122`). The *only* consumer in-tree is the SLAM tracker,
which forwards the rects as `vit_mask_t` so the VIO frontend **ignores visual features on hands**
(`references/monado/src/xrt/auxiliary/tracking/t_tracker_slam.cpp:1204-1212,1308-1331`). It is a
feature-rejection hint for SLAM, in the same *shape* as a cutout request but three artifacts short
of one.

### What the joints DO give the cutout pipeline (the prior, itemized)

- **ROI**: `back_project`'s per-view square (`hg_sync.cpp:244-245`) crops the segmentation input —
  the single biggest real-time win (run the net on ~2×256² crops instead of 2 full views).
- **Skeleton seed**: 21 projected 2D joints per view (`keypoints_global`, `hg_sync.cpp:205-216`)
  are guaranteed-foreground pixels — a trimap seed for matting and the direct replacement for
  MediaPipe in the landmark-guided method (§3.3).
- **Per-hand depth prior**: fitted 3D joints + capsule radii → an analytic depth interval per pixel
  neighbourhood; plus the keypoint net's own 22-bin depth head (§5).
- **Motion gating & handedness**: per-hand presence, left/right identity, frame-over-frame motion
  (`history_hands`), and confidence — resets the matting recurrent state on appearance changes and
  labels the two classes for free.
- **Failure modes to design around**: Mercury deliberately stops tracking when hand boxes overlap
  (`stop_everything_if_hands_are_overlapping`, `hg_sync.cpp:601-627`), suppresses output for the
  first N frames (`hg_sync.cpp:1019-1024`), and drops too-far hands (`hg_sync.cpp:629-634`). Exactly
  when hands interact — the visually most important cutout moment — the prior *goes away*.

### The integration boundary: a parallel camera-processing service

Because the prior is intermittent and Mercury's process cadence is its own, the segmentation/matting
stage must be a **parallel consumer of the same camera frames, able to run without Mercury**, with
pose assistance strictly optional:

```
frameserver ──┬── Mercury (joints)            ──► OpenXR hand tracking, joints prior
              ├── SLAM (uses bbox masks_sink) ──► head pose
              └── hand-cutout service         ──► α + F + hand-depth + capture timestamp ──► zxr-shell-v2 top layer
                      ▲ optional: joints/ROI/confidence from Mercury (same timestamps)
```

Monado's frame-sink fan-out (`xrt_frame_sink` graphs, same mechanism the debug sinks use,
`hg_sync.cpp:1279-1280`) supports this without touching Mercury. The wrong designs, for the record:
inside Mercury (dies when tracking dies, wrong cadence, wrong output type), or inside the compositor
render loop (an ML inference on the headset's critical path violates the deadline rule of
[zxr-shell-v2 §7.4](../architecture/zxr-shell-v2-composition.md)). The service publishes per camera
frame; the compositor treats it like any other client submission: latest-complete wins, warped to
display time — never awaited.

---

## 3. Segmentation: the studied repos

### 3.1 Ego2Hands / Convolutional Segmentation Machine — the monochrome baseline

**Repo:** `references/ego2hands/` (AlextheEngineer), paper arXiv
[2011.07252](https://arxiv.org/abs/2011.07252). The standout for spatial-os because it is the one
purpose-built, real-time, *two-hand* egocentric segmentation model that is **trained on greyscale**.

**Architecture** (`models/CSM/CSM.py`): a compact ResNet-bottleneck encoder (7×7 s2 conv → avgpool
→ four `Bottleneck` stages, expansion 4; `CSM.py:104-111`) feeding a **two-stage cascaded decoder**
("Convolutional Segmentation Machine", after Convolutional Pose Machines): stage 1 emits seg (+
"energy") at 1/4 resolution (`CSM.py:114-126,200-205`), which is concatenated back into the features
(`CSM.py:214-218`) so stage 2 refines at 1/2 resolution (`CSM.py:222-228`). Test-time can run
truncated (`n_stages` 1 or 2, `model_train_test.py:337-353`) to trade accuracy for speed.
3 classes: background / left / right (`configs/config_ego2hands_csm.yml:18`). The optional
per-hand **"energy" channel** (sigmoid heatmap, `CSM.py:126,146`) doubles as a detection/bbox output
(`utils`' `get_bounding_box_from_energy`, used at `data_loaders/Ego2Hands.py:375-378`).

**Greyscale handling** (`data_loaders/Ego2Hands.py`): training images are composited — right-hand
captures with alpha (flipped to synthesize left hands) over random backgrounds with brightness/blur
augmentation (`Ego2Hands.py:219-313`) — then collapsed to grey via `cv2.cvtColor(..., COLOR_RGB2GRAY)`
(`Ego2Hands.py:334`), optionally stacked with a **Canny edge map** (25/100 thresholds,
`Ego2Hands.py:337-341`) as a second input channel (`CSM.py:100`: 1 or 2 input channels; no 3-channel
RGB path at all). Working resolution 288×512 (`Ego2Hands.py:211`). Input normalization mean 128 /
scale 256 (`Ego2Hands.py:345`). Author's README states models were trained on greyscale, with the
edge+energy variant the best performer (`README.md:25,70,149`).

**Pretrained weights:** *not in the repo* — a Box link in `README.md:38` (plus 8 scene-adapted
variants). Availability is at the mercy of a personal Box account; re-training requires the ~90 GB
dataset (also Box-hosted).

**Notable design detail worth copying:** cheap **scene/domain adaptation** — freeze the composited
foregrounds, swap the background pool for captures of the actual environment, fine-tune 10k iters
(`README.md:109-124,141-163`). Translated to spatial-os: record hand-free passthrough sequences per
headset (or per home), composite hands over them, and adapt the model to the device's optics.

**License — flagged:** `README.md:171-173`: "This dataset can only be used for
scientific/non-commercial purposes." Moreover the repo has **no LICENSE file at all** — the code
defaults to all-rights-reserved. Ego2Hands is therefore *benchmark and blueprint*, not shippable
code or shippable weights (§7).

**Real-time viability:** the paper targets real-time (author reports interactive-rate gesture demo);
the architecture (~2-stage compact CSM at 288×512) is comfortably NPU/mobile-GPU class,
in the same budget family as Mercury's own 160×160 + per-hand crop ONNX models. `--speed_test` mode
exists (`model_train_test.py:33,333-361`) but no numbers are committed to the repo.

### 3.2 EgoHOS — data and teacher, not runtime

**Repo:** `references/egohos/` (owenzlz, ECCV 2022, arXiv
[2208.03826](https://arxiv.org/pdf/2208.03826.pdf)). Fine-grained egocentric hand-object
segmentation: classes 0–8 = background, **left hand, right hand**, and 1st/2nd-order interacting
objects per hand (`README.md:71-81`). Built on `mmsegmentation`; the default two-hand model is
**Swin-L + UPerNet** (`README.md:102`), run via `mmsegmentation/pred_twohands.sh` with config
`seg_twohands_ccda` and checkpoint `best_mIoU_iter_56000.pth` (`pred_twohands.sh:2-6`); `ccda` is
their context-aware compositional data augmentation. Checkpoints are Google-Drive-hosted
(`download_checkpoints.sh` uses `gdown`).

**Role for spatial-os:** Swin-L is a server-class backbone — not a candidate for on-headset
real-time. Its value is (a) **11k+ labelled egocentric RGB frames** with separate L/R hand classes
(sourced from EPIC-KITCHENS, Ego4D, THU-READ and their own escape-room footage per the paper), and
(b) an **offline teacher**: run it over RGB passthrough recordings from our target devices to
pseudo-label training data for a small greyscale student net. The interacting-object classes also
matter later for the "does a held object join the cutout?" question (§9).

**License:** code MIT (`LICENSE`). The *dataset* inherits upstream terms — EPIC-KITCHENS is
CC BY-NC 4.0 and Ego4D requires a license agreement (**unverified from the local clone**; flag
before training anything shippable on it). Checkpoints trained on that data inherit the ambiguity.

### 3.3 lightweight-hand-segmentation — the Mercury-seam demonstrator

**Repo:** `references/lightweight-hand-segmentation/` (itap-robotica-medica, MIT). Paper:
Sánchez-Brizuela et al., "Lightweight Real Time Hand Segmentation Leveraging MediaPipe Landmark
Detection", *Virtual Reality* 27:3125–3132 (2023),
[doi:10.1007/s10055-023-00858-0](https://doi.org/10.1007/s10055-023-00858-0). One file,
`hand_segmentation.py`, no learning in the segmentation stage:

1. **Landmarks:** `extract_hands()` (`hand_segmentation.py:11-40`) runs MediaPipe Hands and draws
   the 21-landmark skeleton onto a black canvas.
2. **Search region:** the skeleton is dilated (3×3, then 21×21 ×2 iterations,
   `hand_segmentation.py:96-99,142-144`) into a guaranteed-hand core plus a search region.
3. **Adaptive skin model:** image → **CIELab** (`hand_segmentation.py:151`); from 150 randomly
   sampled skeleton pixels, take 25th–75th percentile ranges of the a*/b*-ish channels (their
   sat/val slots) (`hand_segmentation.py:158-175`); `cv2.inRange` per channel, AND them, OR the
   skeleton back in (`hand_segmentation.py:174-181`).
4. **Morphology:** close → open (13×13) → dilate (7×7), then AND with the search region
   (`hand_segmentation.py:187-190`).

**Qualifications, all three from their own code/paper:** (a) **colour-dependent** — the entire
stage-3 model is chroma percentiles; on monochrome/IR frames there is no chroma and intensity
percentiles collapse against skin-toned or bright backgrounds; (b) **forearm excluded** — the mask
cannot extend past the dilated landmark region, so this is a *hand* cutout, not an *upper limb*
cutout; (c) the reported **IoU 0.869 on Ego2Hands is computed only inside the landmark search
region** (`ground_truth & mask_s2`, `hand_segmentation.py:210-211`) — misses outside the region
(and MediaPipe detection failures) are not penalized. Timings in-code are per-stage
(`hand_segmentation.py:203-207`); the paper claims 90 fps CPU-only.

**Why it still matters:** the *structure* is exactly the Mercury seam. `extract_hands()` is the only
MediaPipe-dependent stage, and Mercury's `back_project` already produces the same thing better: 21
2D joints per calibrated camera view with tracked handedness (`hg_sync.cpp:205-216`). Replace stage
1 with Mercury projections and stages 2–4 become a ~1 ms/frame classical fallback path — with the
caveat that stage 3 must be re-invented for monochrome (intensity + edge + distance-transform
heuristics, or dropped in favour of the CSM net).

### 3.4 Inputs compared — and the greyscale ≠ IR flag

| Model | Input | Classes | Real-time on-device? | Weights available |
|---|---|---|---|---|
| Ego2Hands CSM | **1–2ch: grey (+Canny edge)**, 288×512 | bg/L/R (+energy) | yes (designed for it) | Box link only, NC dataset |
| EgoHOS Swin-L | 3ch RGB | 9 (L/R hands + objects) | no (teacher) | GDrive, dataset terms unclear |
| lightweight-hand | 3ch colour (CIELab thresholds) | binary | yes (90 fps CPU claimed) | n/a (no learned weights in seg stage) |
| RVM (§4) | 3ch RGB video | α + F (person) | yes (HD 104 fps on 1080 Ti) | GPL-3.0, human-matting |
| Mercury's own nets | 1ch grey (160² det, ROI crops) | joints, not pixels | yes (shipping) | Monado models repo |

**Flag:** "trained on greyscale" means *visible-spectrum luminance* (Ego2Hands greys down RGB
captures, `Ego2Hands.py:334`). Headset tracking cameras on likely spatial-os targets (Quest-class,
Steam Frame-class) are monochrome with **near-IR sensitivity and often active IR illumination**
(assumption — verify per device in the device contract). Skin albedo, sclera/vein contrast, and
background reflectance all differ at ~850 nm from visible luminance; Canny-edge auxiliary input
helps (edges are more spectrum-stable) but a fine-tune pass on real device footage is mandatory
either way. Visible-greyscale-trained ≠ IR-ready; treat it as a favourable initialization, not a
solved domain.

---

## 4. Matting: RVM as the architecture to adapt

**Repo:** `references/robust-video-matting/` (PeterL1n, WACV 2022, arXiv
[2108.11515](https://arxiv.org/abs/2108.11515), GPL-3.0). The reference design for *temporal* alpha
matting without auxiliary inputs (no trimap, no pre-captured background):

- **Encoder:** MobileNetV3-Large (or ResNet50) + LR-ASPP (`model/model.py:23-30`).
- **Recurrent decoder:** four upsampling stages, each with a **ConvGRU on half its channels**
  (`model/decoder.py:57-111,152-190`) — the recurrent state is how it stays temporally coherent and
  resolves ambiguity from motion; the split (`a,b = x.split(...)`) keeps cost down.
- **Output head:** `project_mat` → 4 channels = **3ch foreground residual + 1ch alpha**
  (`model/model.py:32,59-65`); `fgr = residual + src` — i.e. it predicts F as a correction to the
  input pixel, exactly the F needed by §1's equation. There is also an auxiliary
  `segmentation_pass` head used during training (`model/model.py:47,66-68`) — a ready-made hook for
  distilling a segmentation prior into the same trunk.
- **Deep Guided Filter:** inference runs the trunk on a downsample (auto ratio targets 512 px,
  `inference.py:153-157`) and a learned guided filter restores full-res edges
  (`model/model.py:60-61`, `model/deep_guided_filter.py`) — the cheap high-res-edges trick a
  headset implementation needs.
- **Recurrent-state plumbing:** inference threads `rec = [r1..r4]` through every call
  (`inference.py:120-127`) — on-device this is 4 small persistent GPU textures; reset them when
  Mercury reports hand appearance/disappearance.
- **Exports:** official TorchScript, **ONNX** (opset 12), CoreML, TFLite (README, release links) —
  the ONNX path matches Monado's existing onnxruntime usage. Speed: 4K 76 fps / HD 104 fps on a
  GTX 1080 Ti (README), i.e. plausible-but-unproven on XR SoCs at ROI-crop resolutions.

**Adapt, not drop-in — three reasons:** (1) trained for **human/portrait matting** (VideoMatte240K
+ image-matte + segmentation datasets per `documentation/training.md`), where "person attached to
the bottom edge" is the object prior — egocentric hands entering from screen-bottom *corners* with
extreme perspective are out-of-distribution and **not validated anywhere**; (2) input is 3-channel
RGB — the first conv must be retrained for 1–2-channel grey(+edge or +prior) input; (3) it has no
notion of left/right or of a joints prior. The adaptation that preserves its value: keep the
recurrent-ConvGRU decoder + DGF skeleton, shrink the encoder, add input channels for the Mercury
prior (rendered skeleton/capsule-distance map), train on Ego2Hands-style composited greyscale data
+ EgoHOS-teacher pseudo-labels, distill temporal coherence from RVM-on-RGB-recordings where
available. **License flag:** GPL-3.0 code *and* weights — see §7.

---

## 5. Hand depth and occlusion ordering

The matte alone answers "which pixels are hand"; the compositor also needs "how far is the hand" —
for reprojection and for `.automatic`.

**Depth sources, cheapest first:**

1. **Capsule/mesh render from Mercury joints.** The fitted 26 joints + per-joint radii
   (`u_hand_joints_apply_joint_width`, `hg_sync.cpp:958`) define a capsule skin; rasterizing it in
   the camera at the capture timestamp yields a dense, smooth, slightly-wrong depth image (mesh ≠
   real silhouette, no clothing/forearm). This alone is enough for `.automatic` at typical
   hand–content separations, and it is the Meta HandsRemoval approach transplanted (§6).
2. **Mercury's learned depth head** — 22-bin relative depth per joint (`hg_model.cpp:129-182`)
   already fused (lightly, `amt_use_depth` = 0.01) by the LM optimizer: use the *fitted joints*, not
   the raw bins.
3. **Stereo refinement inside the matte.** Both views produce a matte from the same synchronized
   stereo pair Mercury consumes; block/plane-sweep matching restricted to matte pixels, seeded by
   the capsule depth (search range = capsule ± a few cm), upgrades accuracy at finger boundaries
   where it matters. (This is a narrow special case of the passthrough-depth problem studied in
   `references/openstereo/`, `references/raft-stereo/`, `references/tc-stereo/` — the hand ROI is
   tiny compared to full-frame stereo, so classical methods suffice here.)

**Soft ordering for `.automatic`.** Per pixel, with hand depth `d_h` and composed-scene depth `d_s`
(the compositor already owns a resolved depth buffer after its nearest-depth pass,
[zxr-shell-v2 §7.4](../architecture/zxr-shell-v2-composition.md)):

```
occ = smoothstep(-ε, +ε, d_h − d_s)      # 0: hand in front … 1: hand behind
α_final = α_matte · (1 − occ · fade)     # fade < 1 keeps a ghost, matching Apple's "fade out
                                         # as its depth increases", not a hard z-test
```

A hard `z-test` on a noisy capsule depth flickers at hand–object contact (the exact moment users
look at their hands); the fade is the honest rendering of depth uncertainty. GrabAR (§6) exists
precisely because raw depth at hand–object contact is unreliable — it learns the occlusion mask
directly and skips depth; its lesson here: keep ε generous and consider learned ordering later for
grabbed-object scenarios.

**The same-timestamp / warp-identically invariant.** The matte, the foreground colour, and the hand
depth are three channels of *one* capture: they must carry the same `timestamp_ns` (Mercury anchors
everything to `left_frame->timestamp`, `hg_sync.cpp:1056-1059`) and must be reprojected into each
eye at display time **by the same warp** (camera→eye extrinsics + hand depth + head-pose delta). A
per-pixel-perfect matte applied to a *different* frame's pixels — or warped with scene depth instead
of hand depth — produces a cutout of empty air next to the visible hand. This is the hand-layer
instance of the composition doc's atomic-submission rule ([zxr-shell-v2
§7.2](../architecture/zxr-shell-v2-composition.md)): α, F, depth, timestamp travel as one unit, and
a late matte is *dropped or late-warped as a unit*, never mixed-and-matched across frames.
Corollary: the *passthrough* image the matte cuts from must be the same camera frame the matte was
computed on — if passthrough rendering runs its own newer frame, the hand layer must still use its
own paired frame for the hand pixels.

---

## 6. Composition policy: the visionOS contract mapped onto zxr-shell-v2

**Apple (verified):** WWDC24 "Render Metal with passthrough in visionOS"
([developer.apple.com/videos/play/wwdc2024/10092/](https://developer.apple.com/videos/play/wwdc2024/10092/))
and the `upperLimbVisibility` scene modifier (introduced with CompositorServices, WWDC23 10089:
[developer.apple.com/videos/play/wwdc2023/10089/](https://developer.apple.com/videos/play/wwdc2023/10089/)).
The app in mixed immersion renders colour with **premultiplied alpha** (clear `(0,0,0,0)`) and depth
in **reverse-Z**; it then declares one of three modes:

- `.visible` — limbs always composited on top, regardless of content depth;
- `.hidden` — limbs always occluded by rendered content;
- `.automatic` — the system compares *the app's submitted depth texture* against its own limb depth
  and shows / fades / hides per pixel ("fully visible … or partially hidden, if it's behind or
  within the object").

The load-bearing observation for spatial-os: **Apple documents only the composition contract —
depth semantics, alpha semantics, a 3-value policy enum — and never the segmentation network.** The
cutout is a system service; apps interact with a policy. That is precisely the right protocol
boundary for zxr-shell-v2.

**Meta (verified):** Unity-DepthAPI `HandsRemoval` sample
([github.com/oculus-samples/Unity-DepthAPI](https://github.com/oculus-samples/Unity-DepthAPI),
[hands-removal docs](https://developers.meta.com/horizon/documentation/unity/unity-depthapi-hands-removal/)).
`EnvironmentDepthManager.RemoveHands = true` removes hands from the environment **depth map**
(replacing them with approximate background depth), and the sample renders **tracked `OVRHand`
meshes as a high-resolution occlusion mask** that clips virtual content. This is the
**geometry-mask** approach: no camera-derived matte at all — the tracked hand *mesh* stands in for
the silhouette. Tradeoff, explicitly: sharp and cheap (mesh raster at render resolution, no ML on
camera pixels, no camera-to-eye reprojection problem because the mesh is rendered directly in the
eye view) versus **mesh mismatch** — the mesh is the tracker's belief, not the real hand: no
sleeves/watches/rings, wrong finger poses exactly when tracking degrades, no held objects, hard
mesh-edge boundaries where the real hand has soft ones. Note it *punches a hole* for passthrough to
show through rather than compositing a hand image over content — viable only because Quest
composites over live passthrough anyway.

**Occlusion-ordering references:** GrabAR (Tang, Hu, Fu, Cohen-Or, **UIST 2020**,
[doi:10.1145/3379337.3415835](https://doi.org/10.1145/3379337.3415835), arXiv
[1912.10637](https://arxiv.org/abs/1912.10637)) — learns the hand-vs-virtual-object occlusion mask
directly from paired hand image + rendered object, bypassing depth entirely; right idea for
close-contact grabbing, wrong shape for a general compositor (needs the virtual object image as
input, per-object). Wu et al., hand-object occlusion handling with depth correction, **IEEE VRW
2023** — cited in the task brief as a depth-correction occlusion reference; **unverified: not
independently confirmed (title/DOI) during this research pass** — treat as a pointer, not a source.

**Mapping into zxr-shell-v2.** The composition doc defines a sort-last pipeline: 2D planes and 3D
client submissions resolve by nearest-depth into one colour+depth, submitted to Monado as a single
stereo projection layer ([zxr-shell-v2 §7.4](../architecture/zxr-shell-v2-composition.md)). The hand
cutout is a **compositor-owned top layer applied after the nearest-depth resolve**:

```
resolve clients → (C_scene, d_s)                      # existing sort-last pass
for each eye:
    warp (αF_hand, α, d_h) from camera(t_c) to eye(t_display)   # one warp, all channels
    per pixel, by policy:
        visible:   C = αF_hand + (1−α)·C_scene
        hidden:    C = C_scene
        automatic: occ = smoothstep(−ε, +ε, d_h − d_s)
                   α′ = α·(1−occ·fade);  C = α′F_hand + (1−α′)·C_scene
submit ONE projection layer to Monado (unchanged)
```

Policy scope: the visionOS enum is per-immersive-app; zxr-shell-v2 is a multi-client desktop, so the
policy is a **per-spatial-client (or per-volume) attribute in the zxr protocol** with a shell-owned
default of `automatic` — a fullscreen "movie volume" may request `hidden`; UI panels near the body
want `automatic`; the shell's own system UI can force `visible` during setup. Rule inherited from
Apple: clients never see the matte or the camera frames (privacy boundary — hand images are
biometric-adjacent); they only declare policy, exactly as they declare bounds. Scheduling: the hand
layer obeys the same deadline rule as clients — if no fresh matte exists at composition cutoff,
late-warp the previous one by head-pose delta (hands are body-locked; a stale unwarped matte is
worse than none) and drop it entirely past a staleness bound, degrading to `visible`-without-cutout
(passthrough hands simply absent behind content) rather than stalling the frame. Note the depth
conventions must line up here: the composition doc mandates an explicit depth-encoding contract
(reverse-Z policy etc., [zxr-shell-v2 §2](../architecture/zxr-shell-v2-composition.md)); `d_h` must
be produced in — or converted to — that same encoding before the compare, or `.automatic` inverts.

---

## 7. Licensing table

| Asset | License | Shipping implication for spatial-os |
|---|---|---|
| Ego2Hands **code** | **none** (no LICENSE file in repo) | default all-rights-reserved: may not redistribute or derive shipped code from it; the *architecture idea* (2-stage CSM, grey+edge input, energy head) is freely reimplementable |
| Ego2Hands **dataset + pretrained weights** | "scientific/non-commercial purposes only" (`README.md:171-173`) | usable for internal research/benchmarking; **do not train shipped weights on it**; a FOSS distro is redistributable downstream incl. commercially, so NC-tainted weights are a no |
| EgoHOS code | MIT (`LICENSE`) | fine |
| EgoHOS dataset/checkpoints | MIT repo, but images sourced from EPIC-KITCHENS/Ego4D etc. — upstream terms not re-verified (EPIC-KITCHENS is CC BY-NC-4.0; **flag, verify before training shipped weights**) | safe as offline *teacher* whose pseudo-labels label **our own captured frames** (weights-from-our-data question remains a judgement call; the conservative path is teacher-for-eval only — decide with counsel) |
| RVM code + weights | **GPL-3.0** (`LICENSE`; README: re-released GPL-3.0 Sep 2021) | GPL is shippable in a FOSS distro; run it as a **separate service process** so the compositor (and Monado, BSL-1.0) don't become derivative works; unusable in any future proprietary/dual-licensed component; retrained-from-scratch reimplementation of the architecture avoids GPL for the weights |
| lightweight-hand-segmentation | MIT (`LICENSE`) | fine; its MediaPipe dependency (Apache-2.0) disappears when Mercury replaces `extract_hands()` |
| Monado / Mercury | BSL-1.0 | fine (already core to the stack) |
| Mercury ONNX models | distributed by Monado project (license not re-checked here — **flag**) | verify before redistribution in images |

Practical consequence: **every path to shippable weights runs through data we generate ourselves**
(device captures + composited synthetic hands + teacher pseudo-labels on our own footage), with
Ego2Hands reserved as an *evaluation* benchmark and the NC/GPL assets kept out of the image or
isolated by process boundary respectively.

---

## 8. What spatial-os should adopt / reject, and the prototype path

**Adopt:**
- The **four-artifact framing** as protocol vocabulary: joints (exists), mask, matte (α+F), hand
  depth — each with a timestamp; compositor consumes only matte+depth.
- The **parallel-service integration boundary** (§2): cutout service beside Mercury on the same
  frame fan-out, Mercury priors optional, compositor never blocks on it.
- **visionOS's contract shape** (§6): 3-value per-client policy in zxr-shell-v2; premultiplied
  alpha; depth in the protocol's declared encoding; segmentation implementation invisible to
  clients.
- **Meta's geometry-mask as tier 0**: Mercury capsule mesh rendered per eye = day-one `.automatic`
  with zero ML on camera pixels — and it remains the permanent fallback when the matte is stale or
  the service is absent.
- **Ego2Hands's recipe** (not its code/weights): greyscale+edge input, compositing-based training
  data generation, energy/detection auxiliary head, cheap background-swap domain adaptation.
- **RVM's decoder pattern**: ConvGRU recurrence per scale + deep-guided-filter upsampling,
  F-as-residual + α output head.
- **EgoHOS as teacher** over our own recorded RGB passthrough footage (license caveat §7).

**Reject:**
- Treating Monado's `masks_sink` as a cutout (bounding boxes; SLAM feature rejection — §2).
- MediaPipe anywhere in the pipeline (Mercury supersedes it, on-device, calibrated, stereo).
- Colour-space skin thresholds (CIELab percentiles) on monochrome tracking cameras — no chroma
  exists; the lightweight-hand *structure* survives, its stage 3 does not.
- Geometry-mask as the *end state* (mesh mismatch: sleeves, rings, tracker error, hard edges).
- RVM weights as a drop-in (portrait prior, RGB input, unvalidated on egocentric hands, GPL).
- Running inference inside the compositor's frame loop (deadline rule).

**Recommended prototype path** (each tier ships something):

1. **Tier 0 — policy plumbing + geometry mask.** Add the upper-limb policy to the zxr protocol and
   the compositor top-layer pass (§6 pseudocode). Render Mercury capsules per eye for α (hard) and
   d_h. No cameras touched. Validates the whole contract end-to-end.
2. **Tier 1 — Mercury-seeded greyscale segmentation.** CSM-style small net (reimplemented, 1–2ch
   grey+edge, 3 classes + energy head), run on Mercury's ROI crops (fallback: full-frame at reduced
   res when Mercury has no lock, restoring independence from the prior). Training data: composited
   synthetic greys over device-captured backgrounds + EgoHOS-teacher pseudo-labels on our footage.
   Output: binary mask → guided-filter feathering as provisional α; F := camera pixel (accepting
   the §1 halo for now). Evaluate against Ego2Hands eval set (internal only).
3. **Tier 2 — recurrent matte.** Swap the head for an RVM-style ConvGRU decoder + DGF predicting
   α **and** F-residual; add a prior input channel (rendered capsule-distance or skeleton map);
   reset recurrent state on Mercury appearance events. This removes the halo and the temporal
   flicker.
4. **Depth refine.** Stereo patch refinement inside the matte, seeded by capsule depth (§5); keep
   the smoothstep fade for `.automatic`.

**Domain-adaptation caveats (all tiers ≥1):**
- **Fisheye:** headset tracking lenses are wide-FOV; Ego2Hands/EgoHOS/RVM are rectilinear. Either
  train with the device's distortion model as augmentation, or run on undistorted ROI crops the way
  Mercury normalizes its keypoint inputs (`hg_model.cpp` crop/affine machinery,
  `hg_sync.cpp:132-150` circular-boundary handling shows the fisheye reality).
- **IR spectrum:** visible-greyscale ≠ IR (§3.4) — skin/background contrast shifts, active
  illumination creates strong near-field falloff (hands close to the face are *bright*); fine-tune
  on real device frames is mandatory, and per-device (illuminator power, sensor IR-cut differ).
- **Exposure:** tracking cameras run short, sometimes fixed or SLAM-driven exposures; frames are
  dark/noisy indoors and the auto-exposure steps discontinuously — brightness augmentation à la
  Ego2Hands (`Ego2Hands.py:169-188` brightness regimes) plus exposure-metadata as a net input are
  both cheap insurance. If the matte source is instead the *colour passthrough* camera (device
  contract question, §9), the spectrum problem disappears but a mono-tracking-camera fallback is
  still needed for devices without RGB passthrough.

---

## 9. Open questions

1. **Which camera is the matte source per device?** Tracking mono (matches Mercury timestamps &
   calibration; IR domain; wide FOV) vs colour passthrough camera (matches what the user *sees*;
   different extrinsics/latency; not present on all targets). The §5 invariant says: whichever
   camera provides the passthrough hand *pixels* must provide the matte. If passthrough is rendered
   from the colour camera but the matte comes from mono tracking cams, the matte must be reprojected
   via hand depth into the colour camera — added error at exactly the edges we care about. Needs a
   per-device decision in the device contract.
2. **Compute budget & runtime.** Mercury already spends ONNX inference per frame; what NPU/GPU
   headroom remains on XR2-class SoCs for a second net at camera rate? Options: shared trunk with
   Mercury (invasive), half-rate matting + full-rate warp (likely), Vulkan-compute inference vs
   onnxruntime EP per device.
3. **Latency & warp quality.** Is late-warping a 10–20 ms-old matte by head-pose delta + capsule
   motion good enough during fast hand motion, or does `.automatic` need a predicted matte (dilate
   along Mercury's velocity, cf. `predict_new_regions_of_interest`'s extrapolation)?
4. **Upper-limb scope.** Joints stop at the wrist; visionOS cuts out the whole forearm. Ego2Hands
   annotates arms; EgoHOS labels hands only (its objects are separate classes). Do we segment
   "hand+forearm+sleeve" (needs training-label policy) and does the capsule fallback get an
   elbow-less forearm frustum from the wrist pose?
5. **Held objects.** A grabbed real object visually belongs to the hand layer (visionOS notably does
   *not* cut out held objects; Meta's HandsRemoval can't). EgoHOS's interacting-object classes are
   the data hook; GrabAR argues contact-region ordering needs learning, not depth. Defer, but keep a
   class slot for it.
6. **Two-hands-interlocked.** Mercury drops tracking on overlap (`hg_sync.cpp:601-627`) — exactly
   when the cutout matters most. The service must degrade to prior-free full-frame segmentation
   there; how much quality is lost, and should the recurrent matte's memory carry it?
7. **Privacy boundary.** Camera frames and mattes stay inside the service/compositor; is the policy
   enum + (maybe) a hands-present bit the *only* thing clients may observe? Also: does a client-
   visible hand *depth* (for content reactions) leak silhouette information?
8. **Protocol shape.** One global top layer owned by the shell vs per-client policy attribute with
   shell arbitration ([zxr-shell-v2 §8](../architecture/zxr-shell-v2-composition.md) open-questions
   list should absorb this); interaction with future T2 transparency profiles (a translucent virtual
   pane in front of a hand needs α-aware ordering both ways).
9. **Unverified citations to close out:** Wu et al. IEEE VRW 2023 (depth-corrected hand-object
   occlusion) — locate DOI; Mercury ONNX model licensing; EPIC-KITCHENS/Ego4D terms as they apply to
   EgoHOS checkpoints; per-device IR illumination characteristics (currently assumption-grade).
