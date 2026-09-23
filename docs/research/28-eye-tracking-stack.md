# 28 — Eye-tracking / auto-IPD software stack

**Scope:** the software pipeline that turns inward-facing IR eye-camera frames into (a) a gaze
signal exposed through OpenXR and (b) a metric IPD measurement good enough to drive a lens motor
and Monado's `eye_relation` — and where that pipeline should execute (input to ADR 0011).
**Date:** 2026-09-22.

**Sources studied locally:** `references/pye3d/`, `references/pupil/`, `references/eyetrackvr/`,
`references/eyerectoo/`, `references/ritnet/`, `references/ellseg/`, `references/deepvog/`,
`references/monado/`, `references/alvr/`, `references/wivrn/`; papers under
`references/papers/eye-tracking/` cited by their `references/PAPERS.json` names.
Hardware-per-target companion: [29-eye-hardware-ipd-per-target](29-eye-hardware-ipd-per-target.md).
Architecture context: [ADR 0008](../architecture/adr/0008-perception-services-placement.md)
(perception placement), [ADR 0007](../architecture/adr/0007-session-greeter-lock.md) (tiered
tracking, calibration-as-system-state), [05-xr-userspace](05-xr-userspace.md) (Monado structure,
VIT plugin seam).

**Background facts taken as established (not re-derived here):** OpenXR has no IPD API — rendering
IPD is the per-view pose separation returned by `xrLocateViews`; Monado drivers only *read*
mechanical IPD sensors (`vive_device.c:300-319` mainboard reports, `wmr_hmd.c:550-608` control
packets); the view-pose helper turns an `eye_relation` vector into symmetric eye poses
(`u_device.c:541`, documented override point `u_device.h:146-151`); Monado has no camera-based ET
pipeline and no motor interface.

---

## 1. The measurement pipeline, precisely

### 1.1 Two measurement families

**PCCR (pupil center + corneal reflections).** IR LEDs at *known, calibrated positions* produce
glints on the cornea; with calibrated camera intrinsics, the cornea's center of curvature is
recoverable in closed form in metric camera coordinates, and the pupil-center-to-glint vector gives
gaze after a short user calibration (guestrin-eizenman-2006-pccr). This is what shipping vendor
stacks are: doc 25 documents illuminator rings around the eye cameras on every commercial target,
and the PSVR2 firmware pipeline Monado consumes (§3.4) is PCCR-class. PCCR's price is **calibrated
LED geometry per unit** — a per-device-port reverse-engineering cost Mura cannot assume it
can pay on donor hardware where the illuminator positions are undocumented.

**Glint-free 3D model fitting (Swirski lineage).** Uses only the pupil ellipse across many frames.
swirski-dodgson-2013 is the core: each detected pupil ellipse is unprojected through a pinhole
model into a *pair* of candidate 3D circles (the two circular intersections of the cone through
the camera center and the ellipse — Safaee-Rad 1992, cited therein); a sphere tangent to all pupil
circles is fit by intersecting gaze lines, first in 2D image space (which cancels both the
distance–size ambiguity and the two-circle ambiguity, paper eq. 3–6), then in 3D; pupils are
re-projected consistently onto that sphere. No lights, no user calibration; ~2° gaze accuracy on
their synthetic ground truth (paper Table 1). The paper's own conclusion matters for us: accuracy
is dominated by *finding the correct sphere center*, not by pupil-fit quality — and the sphere
center is exactly the quantity auto-IPD needs.

### 1.2 What pye3d actually estimates, and in which frame

pye3d is the industrialized Swirski/Dierkes fit (dierkes-2018-refraction-eye-model is its basis,
per `PAPERS.json`). Reading the code:

- **Model:** a "two-sphere" eye — an eyeball rotation sphere plus a corneal sphere handled through
  refraction correction. Constants: pupil-circle-to-eyeball-center distance
  `_EYE_RADIUS_DEFAULT = 10.392304845413264` mm and `DEFAULT_SPHERE_CENTER = (0, 0, 35)` mm
  (`pye3d/constants.py:3-4`).
- **Observation:** each 2D ellipse is unprojected to the 3D circle pair; from each circle a
  **Dierkes line** is built — origin at `circle_center − R·normal` (a candidate eyeball center),
  direction through the circle center (`pye3d/observation.py:58-64`); the per-observation
  least-squares accumulators for line intersection are precomputed as 2×3 / 2×3×4 matrices
  (`observation.py:46-56`).
- **Sphere-center estimate:** projected 2D center from intersecting projected gaze lines
  (`pye3d/eye_model/base.py:107-118`, literally Swirski eq. 6), then 3D center as the least-squares
  nearest-intersection of the disambiguated Dierkes lines, optionally biased toward a 3D prior
  (`base.py:120-183`). **This 3D point is the eyeball center of rotation, in the eye camera's
  pinhole coordinate frame, in millimeters** — the plausibility gates in
  `pye3d/detector_3d.py:589-592` (x ∈ ±15 mm, y ∈ ±10 mm, z ∈ 15–75 mm from the camera) confirm
  frame and units.
- **Refraction correction (the Dierkes contribution):** corneal refraction biases the naive fit;
  pye3d corrects sphere center, gaze vector, pupil radius and circle through *learned polynomial
  regression pipelines* (degree-3 polynomial features → standard scaler → linear regression),
  shipped as msgpack models and applied in `pye3d/refraction.py:17-94`;
  `TwoSphereModel.apply_refraction_correction` and `corrected_sphere_center`
  (`base.py:64-83, 258-279`).
- **Temporal structure:** three concurrent models over different windows — short-term (deque of
  10 high-confidence observations), long-term (spatially binned storage, 10 horizontal bins,
  forgetting rules), ultra-long-term (60 s window) — with update schedules of 1 s / 10 s and a 5 s
  warmup (`detector_3d.py:89-122, 187-229`). The **long-term model owns position** (sphere center,
  the IPD quantity); the short-term model owns instantaneous gaze normal under slippage
  (`detector_3d.py:352-368` comment). A Kalman filter over (φ, θ, radius) bridges low-confidence
  frames and drives a 3D edge search fallback (`detector_3d.py:343-401`, `pye3d/kalman.py`).
  `BinBufferedObservationStorage` enforces *gaze diversity* in the fit by capping observations per
  image-region bin (`pye3d/observation.py:129-218`) — the fit degenerates when all gaze lines are
  parallel, so diversity is structural, not incidental.
- **Freeze switch:** `is_long_term_model_frozen` pauses model updates while continuing gaze
  prediction (`detector_3d.py:146-163`) — directly useful during lens actuation (§5.3).

### 1.3 Why raw pupil-center IPD is wrong, and rotation centers are right

The pupil sits ≈10.4 mm in front of the rotation center (pye3d's `_EYE_RADIUS_DEFAULT`). When the
eyes converge on a near target, each pupil translates nasally along the eye surface. Arithmetic
(inference, not from a source): at 65 mm IPD fixating at 30 cm, each eye rotates inward
≈ 6.2°, displacing each pupil ≈ 10.4·sin 6.2° ≈ 1.1 mm — a **≈2.2 mm apparent IPD error that is a
function of what the user happens to look at**. A pupil-distance measurement is therefore only
meaningful under *controlled fixation* (a distant target, so optical axes are parallel — this is
the vendor "IPD wizard" flow). The **eyeball rotation center does not move with gaze**: the
distance between the two rotation centers is vergence-invariant, and for parallel (distance)
fixation it equals the optician's IPD. That is precisely what pye3d's long-term `sphere_center`
estimates, continuously, without any fixation UX. (Residual caveat, marked as inference: the
rendering-relevant point is strictly the entrance pupil, ~1 mm dynamics around the rotation-center
prediction; every shipping headset ignores this and renders from a fixed per-eye point.)

### 1.4 Calibration requirements

pye3d's camera model is minimal — focal length in pixels plus resolution (`pye3d/camera.py:1-6`);
it assumes an **undistorted pinhole image**, so real lenses need intrinsics + undistortion upstream.
The metric quality of the sphere center is only as good as the focal length: EyeTrackVR ships
`focal_length: int = 30` px as a config default (`eyetrackvr/EyeTrackApp/config.py:110-114`) —
fine for *normalized* gaze, useless for metric IPD. Getting IPD out requires, per eye:

1. **Intrinsics** of the eye camera (focal length, principal point, distortion) — per-unit factory
   or service calibration.
2. **Extrinsics** `T_device←eyecam` — the two sphere centers live in two different camera frames;
   IPD is `‖ T_device←camL·c_L − T_device←camR·c_R ‖` (and its x-component is what feeds
   `eye_relation.x`). Pupil-labs solves the equivalent transform *online* by calibration bundle
   adjustment — `Model3D._fit` outputs `eye_camera_to_world_matrix` per eye
   (`pupil/pupil_src/shared_modules/gaze_mapping/gazer_3d/gazer_headset.py:104-148`), and the HMD
   variant `calibrate_hmd` takes **known `eye_translations`** as input instead
   (`gazer_3d/gazer_hmd.py:55-107`, with hardcoded ±33.35 mm fallbacks at lines 196-198). On a
   built-in headset the extrinsics are rigid and should be **per-unit calibration in system state**
   (`/var/lib/mura/`, ADR 0007 "calibration is system state, never `$HOME`") — with the twist
   that on IPD-motorized devices the eye cameras typically ride the lens tubes, so extrinsics are
   a *function of the IPD encoder position* (inference; per-target reality in doc 25).

---

## 2. Pupil-detection lineage and compute budgets

The 3D model fit consumes a 2D pupil ellipse + confidence per frame. The front-end lineage:

| Method | Class | Input size | Cost (as measured/reported) | Source |
|---|---|---|---|---|
| ExCuSe | classical (histogram/edge) | 320×240 | μ=2.51 ms, 1 core i5-4590 | fuhl-2015-excuse; timing from santini-2018-pure §4.4 |
| ElSe | classical, 2-stage (ellipse selection + blob fallback) | 346×260 | μ=6.59 ms, same CPU | fuhl-2016-else; timing ibid. |
| **PuRe** | classical, edge-segment selection/combination + **confidence** | 320×240 | μ=5.56 ms σ=0.6, same CPU, unparallelized | santini-2018-pure §4.4 |
| PuReST | PuRe + tracking-by-detection | 320×240 | faster than PuRe in the common tracked case | santini-2018-purest (cite-only) |
| RITnet | CNN segmentation (DenseNet+U-Net) | 640×400 | 248,900 params / 0.98 MB; 301 Hz on GTX 1080 Ti | chaudhary-2019-ritnet; `ritnet/densenet.py`, `ritnet/README.md` |
| EllSeg (DenseElNet) | CNN, segments *full elliptical structures* + regresses ellipses | 320×240 | GPU-class; encoder-decoder with ellipse head | kothari-2021-ellseg; `ellseg/evaluate_ellseg.py:59,241` |
| DeepVOG | CNN segmentation (U-Net-like, Keras) | 320×240×3 | GPU-class | `deepvog/deepvog/model/DeepVOG_model.py:70` |
| NVGaze | CNN, direct gaze regression | down to 127×127 | sub-millisecond nets on desktop GPU; 2.06° cross-subject | kim-2019-nvgaze abstract |

Notes from the code:

- **PuRe** (reference C++ in `eyerectoo/EyeRecToo/src/pupil-detection/PuRe.{h,cpp}`): Canny with
  edge thinning/filtering, curvature-based segment selection, conditional segment combination,
  candidate scoring as `0.33·aspectRatio + 0.33·anchorDistribution + 0.34·outlineContrast`
  (`PuRe.h:124-129`); anthropomorphic priors in *millimeters* scaled by expected canthi distance
  (`PuRe.h:187-190`) — i.e. it internally downscales to a working size, so cost is
  resolution-insensitive. Its calibrated **confidence measure** (recommended threshold 0.66,
  santini-2018-pure §5) is exactly what pye3d's observation storage wants as input gating.
  PuReST lives beside it (`src/pupil-tracking/PuReST.cpp`). License caveat: the EyeRecToo PuRe
  sources are **non-commercial research license** (`PuRe.h:1-37`) — a clean-room or upstream
  Pupil-labs `pupil-detectors` implementation (LGPL, `pupil/.../detector_2d_plugin.py:22` imports
  `pupil_detectors.Detector2D`) is the shippable route.
- **CNN class:** RITnet is the efficiency sweet spot of the segmentation lineage — a quarter-million
  parameters is NPU-trivial; the cost driver is the 640×400 input (OpenEDS-shaped:
  garbin-2019-openeds — 152 participants, 200 Hz eye cameras, 12,759 annotated frames at 640×400).
  EllSeg's contribution is robustness, not speed: segmenting the *occluded* elliptical whole makes
  ellipse fits survive eyelid/lash occlusion (kothari-2021-ellseg abstract). NVGaze shows the other
  extreme — tiny direct-regression nets at 127×127 — but direct gaze regression skips the explicit
  eyeball-center estimate that IPD needs, so it is a gaze-only option here.

**Budget conclusion (qualitative — nothing was run).** Per ADR 0008, ET shares the SoC with
Basalt/SLAM (CPU), Mercury (ONNX/NPU), the compositor, and apps. PuRe-class detection at 320×240
costs ~5–6 ms on a 2014 desktop core; two eyes at 60–120 Hz on two XR2-class efficiency cores is
plausible headroom-wise, and PuReST's tracked path cuts the common case further. pye3d's model
*updates* are amortized (1 s / 10 s schedules); its per-frame cost is an ellipse unprojection plus
small linear algebra — CPU-negligible. A RITnet-class front end quantized to int8 on the Hexagon
HTP is plausible *if* the NPU isn't saturated by Mercury and the depth backend — that contention is
exactly the ADR 0008 "BSP unknowns" list, so: **classical front end as baseline, CNN front end as a
per-device upgrade behind the same ellipse+confidence interface** (both feed pye3d identically;
this two-tier shape is also EyeTrackVR's architecture, §3.1).

---

## 3. VR implementations studied

### 3.1 EyeTrackVR — the open-hardware precedent

The closest existing "ET inside an HMD on open software" system. Architecture, end to end
(`eyetrackvr/EyeTrackApp/`):

- **Camera ingest** (`camera.py`): three source classes — a **custom serial protocol** for
  ESP32-CAM boards over USB (`0xFF 0xA0` / `0xFF 0xA1` framed JPEG packets, `camera.py:43-50`),
  HTTP MJPEG for WiFi cameras, and cv2/UVC for local devices. One capture thread per eye.
- **Per-eye processing thread** (`eye_processor.py`): ROI crop+rotate → an **algorithm cascade**
  rebuilt every frame from settings (`_rebuild_algorithm_slots`, `eye_processor.py:789-862`) and
  walked with same-frame fallback on failure (`ALGOSELECT`, lines 763-787). The slots: AHSF/HSF
  (Haar-like surround feature detectors, `AHSF.py`, `haar_surround_feature.py`), HSRAC (HSF crop +
  RANSAC), DADDY (ONNX heatmap landmark net, input 192 px / heatmap 48, `daddy.py:42-46`), LEAP
  (ONNX landmark+lid net, `leap.py`), and RANSAC3D — which is thresholded-contour ellipse fitting
  feeding **pye3d's `Detector3D`** (`ransac.py:22,336-338`; detector instantiated per ROI at
  `eye_processor.py:881-897`).
- **Filtering/calibration**: one-euro filter on the output point (`eye_processor.py:241-250`),
  per-user min/max recentering (`osc_calibrate_filter.py`), newer robust-calibration session
  (blink/pursuit phases, `utils/robust_calibration.py`).
- **Output**: OSC to VRChat avatar parameters (`osc/VRChatOSCSender.py:260-302`,
  `/avatar/parameters/v2/EyeX` etc.), a VRCFaceTracking module bridge (`osc/VRCFTModuleMessenger.py`),
  and an OpenVR overlay/manifest registration (`OVR/OpenVRService.py`). **Normalized gaze only, no
  metric output**: with a 30 px default focal length the pye3d sphere center is not metric, and
  nothing consumes it as IPD.

**What transfers to built-in eye cameras:** the cascade-with-confidence-fallback shape; pye3d as
the 3D core (validated by an enthusiast community on cheap 5–60 USD cameras); the lesson that
detector diversity beats one tuned algorithm across skin/eye/camera variation. **What does not:**
serial/MJPEG ingest (built-in cameras are V4L2/BSP paths in Monado's frameserver per ADR 0008), the
OSC surface (an app-level side channel that bypasses the OpenXR input system — the opposite of a
system service), and per-user GUI-driven ROI/threshold tuning (must become calibration state).

### 3.2 Pupil-labs platform

The add-on lineage (clip-in eye cameras for Vive/HoloLens etc.): C++ `Detector2D` behind a plugin
(`pupil/pupil_src/shared_modules/pupil_detector_plugins/detector_2d_plugin.py`), pye3d behind
another (`pye3d_plugin.py`), and gaze mapping as pluggable "gazers" — the 3D one bundle-adjusts
eye-camera-to-scene-camera extrinsics from a calibration choreography and then maps both eyes'
sphere centers + gaze normals into a common frame (`gazer_3d/gazer_headset.py`; per-eye
`eye_center_3d` in world coordinates at lines 150-169). `GazerHMD3D` is the HMD specialization fed
with **known eye translations** (`gazer_3d/gazer_hmd.py`) — the shape Mura inherits, except
our "scene camera" is the device frame and the extrinsics come from unit calibration rather than
choreography.

### 3.3 ALVR and WiVRn — vendor-ET forwarding

Both prove the OpenXR plumbing end-to-end but compute nothing themselves:

- **ALVR** (headset client, Rust): gates on `XR_EXT_eye_gaze_interaction` support, requests
  `com.oculus.permission.EYE_TRACKING` / Pico equivalent, creates one action bound to
  `/user/eyes_ext/input/gaze_ext/pose` (`alvr/client_openxr/src/interaction.rs:394-418`), and
  additionally uses `XR_FB_eye_tracking_social` for *per-eye* poses
  (`extra_extensions/eye_tracking_social.rs:36-68`); both are located against the **view space**
  for heading-independence (`interaction.rs:965-993`) and streamed to the PC for social/face use.
- **WiVRn**: client locates the eye-gaze space per tracking tick with a Pico quirk workaround
  (`wivrn/client/scenes/stream_tracking.cpp:197-262, 453-459`); the server exposes a dedicated
  `xrt_device` named `XRT_DEVICE_EYE_GAZE_INTERACTION`, device type `XRT_DEVICE_TYPE_EYE_TRACKER`,
  with the single input `XRT_INPUT_GENERIC_EYE_GAZE_POSE` served from a timestamped relation
  history (`wivrn/server/driver/wivrn_eye_tracker.cpp:38-92`). This is the **reference shape for
  "a Monado-side ET source presents as a device"**.

### 3.4 Monado's existing ET surface

More complete than expected:

- **Full `XR_EXT_eye_gaze_interaction` plumbing**: input enum `XRT_INPUT_GENERIC_EYE_GAZE_POSE`
  (`monado/src/xrt/include/xrt/xrt_defines.h:964-965`), interaction profile
  `/interaction_profiles/ext/eye_gaze_interaction` with `/user/eyes_ext/input/gaze_ext/pose`
  (`auxiliary/bindings/bindings.json:2424-2438`), an "eyes" device role auto-assigned from device
  type or input presence (`auxiliary/util/u_device.c:392-501`), and oxr-side session/space support
  (`state_trackers/oxr/oxr_session.c:435-437`, `oxr_space.c`).
- **One in-tree producer: the PSVR2 driver** — gaze computed *by headset firmware* (PCCR-class),
  delivered as USB bulk packets with per-eye + combined gaze, blink, pupil diameter
  (`drivers/psvr2/psvr2_eye.c:20-166`); one-euro-filtered (lines 87-91, 123-127), pushed into an
  `m_relation_history` (line 165) and served at arbitrary `at_timestamp_ns` as a head-relative pose
  via a relation chain (`psvr2.c:174-199`); a stored per-user **eye calibration blob is uploaded to
  the device** at start (`psvr2_eye.c:316-336`); gaze+blink are also re-synthesized into three
  face-tracking extension formats (`psvr2_eye.c:420-576`). So Monado's runtime surface for gaze is
  proven; what does not exist is any *camera-based* ET tracker (nothing eye-related in
  `auxiliary/tracking/`) or any motor/actuation interface.

---

## 4. Monado integration options (input to ADR 0011)

The pipeline to place: eye-camera frame pair → per-eye detector (PuRe-class or CNN) → per-eye
pye3d-style model → (gaze pose, two eyeball centers, blink/openness) → OpenXR gaze input + IPD
consumer. Three placements:

### (a) Monado-side service on the eye-camera frame group (ADR 0008 extension)

Eye cameras become a second frameserver group ("eye" vs "world"), session-scoped per ADR 0007;
the ET service is another `xrt_frame` sink; outputs are (i) gaze relations pushed into an
`m_relation_history` behind an eyes-role device exactly like PSVR2/WiVRn, (ii) filtered eyeball
centers → `eye_relation` override and a motor-proposal signal, (iii) later blink/face inputs.

**Honest fit against ADR 0008's reasoning.** ADR 0008's decisive arguments were (1) one
timestamp/calibration domain across consumers of the *same* frames, and (2) pose-at-exposure as an
in-process query. For eye tracking, **(2) does not apply**: eye cameras are rigid to the device,
gaze is produced in the device/view frame, and no world pose at exposure time is needed. And (1)
applies only weakly: the eye-camera group shares no pixels with Mercury/SLAM — only the *clock*
must be common (gaze samples must be predictable/interpolable against the same monotonic domain the
compositor and input system use, as `psvr2.c:188` does with its hw→monotonic offset). So the ADR
0008 arguments do not *compel* co-location the way they did for passthrough. What does argue for
Monado-side placement is different and still strong: **the consumer is Monado itself** — the gaze
pose must answer `get_tracked_pose(at_timestamp_ns)` inside the input pipeline, and the IPD result
feeds `get_view_poses`/`eye_relation`, both Monado-internal; camera bring-up for a session-scoped
BSP camera group already has its home in the frameserver (`mura.adaptation.camera`); and the
privacy boundary (clients get gaze *pose*, never eye images) falls out of the existing IPC design
for free.

### (b) VIT-style dlopen plugin

Model an "ET system library" on the SLAM seam (`auxiliary/tracking/t_tracker_slam.cpp:54-87`,
`VIT_SYSTEM_LIBRARY_PATH`, per 05 §3): Monado owns cameras and the device; a small C ABI takes
frames and returns gaze + eyeball centers, letting implementations (classical, CNN, vendor blob) be
swapped without rebuilding Monado. Attractive for the same reason VIT is (Mura will want
per-device backends), but it means inventing and maintaining a *second* private plugin ABI on top
of an internal seam whose own stability was already flagged as a co-pin risk (05 §11.4) — and
unlike VIT there is no upstream constituency for it yet. This is a *refinement* of (a) — the
service exists either way; the plugin boundary is an internal detail that can be added when a
second backend actually materializes.

### (c) External process feeding a Monado driver

A standalone ET daemon opens the eye cameras (V4L2), runs the pipeline, and feeds a thin Monado
driver over local IPC — WiVRn's `wivrn_eye_tracker` shape with a UNIX socket instead of WiFi.
Pros: language freedom (pye3d is Python/NumPy + small C++ modules — this is the **prototyping
path**), crash and license isolation, no Monado rebuild per iteration. Cons: a second camera
ownership domain outside the frameserver (against the ADR 0008 invariant, though — stated honestly —
that invariant matters less for a frame group nothing else consumes), cross-process timestamp
translation, and one more privileged camera-holding process to sandbox (eye images are biometric —
iris — so the privacy bar is the *highest* of all camera consumers, ADR 0007's tiered-tracking
rationale).

**Recommendation (for ADR 0011):** target architecture **(a)**, with the detector+model behind an
internal interface so **(b)**'s pluggability can be added later; use **(c)** explicitly and only as
the bring-up/prototyping vehicle (Python pye3d + PuRe port against recorded/live V4L2, feeding a
socket driver), to be retired once the C++ port lands as a frame sink. Session scoping per ADR
0007: the ET service starts with the authenticated session (cameras are post-auth), the greeter
never sees eye frames; per-unit intrinsics/extrinsics live in system state; the per-user long-term
model and measured IPD are per-user session state.

---

## 5. Auto-IPD specifics

### 5.1 Two measurement flows

- **Fixation-controlled ("IPD wizard").** Show a distant target (vergence ≈ 0), collect a few
  seconds of high-confidence samples, measure. With rotation centers this merely accelerates
  convergence and removes the near-fixation bias risk; with pupil-center or PCCR measurements it is
  *required* for correctness (§1.3). Right UX for first-boot / new-user enrollment (ties into ADR
  0007's per-user-IPD-post-login handoff).
- **Continuous rotation-center estimation.** pye3d's long-term model converges during natural use
  — 5 s warmup, 1 s update cadence, spatial-bin diversity gating (§1.2) — and is fixation-invariant
  by construction. Right mechanism for drift monitoring, re-donning detection, and "IPD changed —
  adjust?" prompts without ceremony.

Both output the same quantity: `c_L`, `c_R` in device frame via per-eye extrinsics; measured IPD is
the x-component separation (full 3D separation also yields per-eye vertical/axial offsets, which
`eye_relation` as an `xrt_vec3` can carry — `u_device.h:151-156`).

### 5.2 Filtering and hysteresis before touching motors

Eyeball centers are quasi-static per user; the estimator is noisy per update (SVD residuals exist —
`base.py:185-204` computes RMS residual as a quality signal; `model_confidence` gates outliers,
`detector_3d.py:560-600`). Before actuation (inference — design guidance, no source implements
this): median-filter the measured IPD over a window of minutes of high-confidence samples; require
sustained deviation above a dead-band (~0.5 mm class, below both motor step and optical
sensitivity) before proposing a move; rate-limit proposals; never chase the estimator. Actuation is
also an *event*, not a servo loop: propose → (policy: confirm in-UI or auto during idle) → move →
re-settle.

### 5.3 The actuator interface shape

No precedent exists in the studied stacks (Monado drivers only *read* IPD sensors — background
facts). Required shape (inference): a bounded, slow, encoder-fed positioner —
`set_target_ipd_mm(v)` clamped to hardware range, `get_encoder_mm()`, `get_limits()`, move-complete
signal, hardware-enforced travel limits. It belongs in the per-device Monado driver (it is a device
control like `wmr_hmd`'s control packets), surfaced through the `mura.adaptation` per-device
contract, never generic. During motion: **freeze the eye models** (`is_long_term_model_frozen` —
the cameras may be moving with the lens tubes, so observations mid-travel are calibration-invalid),
and re-resolve extrinsics at the new encoder position before unfreezing.

### 5.4 Distortion re-centering after lens motion

Moving the lenses moves each lens's optical axis relative to its (fixed) panel: the per-eye
distortion center, FOV asymmetry, and chromatic terms are functions of encoder position. Monado's
distortion pipeline is per-device mesh/function state computed at startup — so per-unit calibration
must be stored as a *function of IPD position* (interpolatable table over encoder range —
inference; doc 25 has the per-target mechanics), and the compositor's distortion state plus
`hmd->views` geometry must be refreshed after each move. This is a Monado-internal consequence that
placement (a) makes an in-process update rather than an IPC dance.

### 5.5 Feeding measured IPD into rendering

The override point already exists and is documented: drivers pass an `eye_relation` vector into
`u_device_get_view_poses` / `u_device_get_view_pose`, and `u_device.h:146-151` says verbatim to
copy the relation and "put your known IPD in `real_eye_relation->x`" when known better than the
system default. So the consumption path is: ET service publishes filtered IPD (+ per-eye offsets) →
device driver substitutes it for the mechanical-sensor/default value in its `get_view_poses` →
`xrLocateViews` separation follows. No OpenXR surface changes needed; per-user measured IPD is
applied post-login exactly as ADR 0007 already specifies for per-user IPD preferences.

---

## 6. Adopt / reject / open questions

### Adopt

1. **pye3d's model as the algorithmic core** — glint-free rotation-center estimation with
   refraction correction and multi-timescale robustness; it is the only studied implementation
   whose primary output *is* the vergence-invariant IPD quantity. Production use means a C/C++
   port of `observation.py` + `eye_model/base.py` + the msgpack refraction pipelines (small,
   NumPy-expressible math) as a Monado-side component.
2. **PuRe/PuReST-class classical detector as the baseline front end** (clean-room or LGPL
   `pupil-detectors` lineage, not EyeRecToo's non-commercial sources), CPU-budgeted, with its
   confidence measure gating pye3d observation storage; **RITnet/EllSeg-class quantized CNN as a
   per-device NPU upgrade** behind the same ellipse+confidence interface.
3. **Monado's existing ET surface**: `XR_EXT_eye_gaze_interaction` bindings, eyes device role,
   `m_relation_history` + relation-chain pattern from PSVR2/WiVRn; expose gaze only through this,
   never a side channel.
4. **Placement (a)**: ET as a Monado-side consumer of a session-scoped eye-camera frame group —
   adopted *with the stated caveat* that ADR 0008's pose-at-exposure argument does not apply; the
   binding reasons are Monado-as-consumer, frameserver camera bring-up, shared clock, and the
   privacy boundary.
5. **Calibration as system state, keyed by encoder position** (intrinsics, eye-cam extrinsics,
   distortion) per ADR 0007; per-user long-term model + measured IPD as session state.

### Reject

- **EyeTrackVR's OSC/VRCFT integration surface** as anything but a compatibility bridge — it
  bypasses the runtime input system. (Its algorithm cascade and its ESP32 serial camera ingest
  remain valuable references — the latter only for DIY retrofit targets.)
- **PCCR as the required design** — depends on calibrated illuminator geometry unavailable on donor
  hardware; keep as a per-device *enhancement* where doc 25 establishes LED geometry.
- **DeepVOG's eye fitter** as a second 3D core — `deepvog/eyefitter.py` is the same Swirski
  construction without refraction correction or temporal robustness; pye3d strictly supersedes it.
- **NVGaze-style direct gaze regression** as the primary path — no explicit eyeball center, so no
  IPD; possible later as a low-power gaze-only mode.
- **ET in the compositor or per-app** — same reasoning as ADR 0008's rejected alternatives
  (deadline rule; camera privacy boundary).
- **Closed-loop motor servoing off the raw estimator** — event-shaped, hysteresis-gated actuation
  only (§5.2).

### Open questions (for ADR 0011 and hardware spikes)

1. **Eye-camera BSP access and exposure timestamps** per target (doc 25's qualification matrix):
   V4L2 vs vendor path, timestamp quality against the monotonic clock, achievable rate (60–120 Hz
   assumed here; OpenEDS hardware runs 200 Hz).
2. **Extrinsics-vs-encoder calibration**: is a two-point (min/max IPD) interpolation adequate, or
   does each unit need a swept calibration? Who produces it (factory data vs on-device routine)?
3. **Kappa / visual-axis offset**: pye3d yields the *optical* axis; per-user visual-axis offset
   (~5°) needs a one-point calibration for gaze-interaction accuracy (swirski-dodgson-2013
   acknowledges this limit). Where does that UX live (enrollment flow beside the IPD wizard)?
4. **NPU contention**: Mercury + depth backend + ET CNN on one HTP — the ADR 0008 BSP-unknowns
   list extends to ET; classical-vs-CNN front end may end up a per-device decision.
5. **Gaze data privacy policy**: gaze is exposed per-client via an OpenXR extension the client must
   enable — is that gating sufficient, or does Mura want an explicit per-app permission
   (Android-style `EYE_TRACKING` permission, as ALVR must request on Quest/Pico)?
6. **Upstreaming**: does upstream Monado want a camera-based ET tracker + an ET plugin ABI
   (option b), or does this remain a Mura patch series per the WiVRn pinning pattern
   (05 §9.2)?
7. **Foveated-rendering latency budget**: gaze-to-photon for foveation is stricter than for
   interaction; whether the PuRe+pye3d path at 120 Hz meets it on target hardware is a
   measurement, not a design, question. *Update (2026-09-23):* a working reference now exists —
   the `monado-galaxyxr` fork implements gaze-driven two-level `VK_KHR_fragment_shading_rate`
   foveation on Galaxy XR (12° full-rate disc; 68% vs 77% GPU busy measured), using the OEM QNN
   eye tracker ([31-kwin-vr §5](31-kwin-vr.md), [ADR 0013 §4](../architecture/adr/0013-kwin-vr-disposition.md));
   where foveation *policy* lives in Mura remains undecided.

### Recommended baseline (marked as ADR 0011 input, not a decision)

Per authenticated session, on the eye-camera frame group inside Monado: **per-eye PuRe-class
detector (CPU) → ported pye3d two-sphere model with refraction correction → (i) gaze via an
eyes-role `xrt_device` serving `XRT_INPUT_GENERIC_EYE_GAZE_POSE` from a relation history
(`XR_EXT_eye_gaze_interaction` to clients), (ii) rotation-center IPD, median-filtered with
dead-band hysteresis, into the driver's `eye_relation` and an actuation-proposal event consumed by
the per-device motor control, with model freeze + extrinsics/distortion refresh around each lens
move.** CNN front end (RITnet-class, quantized) as a per-device substitution where the NPU budget
allows; Python/pye3d external-process rig (option c) as the prototyping harness only.
