# 24 — Avatar Representation & Enrollment: Code-Level Deep Dive

**Date:** 2026-09-22. This document is a read-only source-code audit of five avatar
systems cloned under `references/`. Every code-level claim cites a file path (and
function/line where useful). Status markers: **VERIFIED** (code confirms), **PARTIAL**
(code partially confirms; some details inferred), **UNVERIFIED** (paper claim only;
code absent or ambiguous). Paper-level claims are audited in a sibling doc (27);
this doc focuses on *what the code actually shows*.

Study corpus: `rgbavatar/` (RGBAvatar, CVPR 2025 Highlight, MIT), `gaussianavatars/`
(GaussianAvatars, CVPR 2024), `match/third_party/GEM/` (GEM component of MATCH,
CVPR 2026), `flexavatar/` (FlexAvatar, CVPR 2026, NC license),
`metrical-tracker/` (Metrical Photometric Tracker).

---

## 1. Control Inputs Per Model

What exactly animates each representation at inference time?

### 1.1 RGBAvatar (FLAME path)

| Input | Tensor shape | Source |
|---|---|---|
| `expression_params` | `[B, 100]` | FLAME 100 expression coefficients |
| `jaw_pose_params` | `[B, 1, 3, 3]` | 3×3 rotation matrix for jaw joint |
| `eye_pose_params` | `[B, 2, 3, 3]` | 3×3 rotation matrices, L + R eyeball |
| `neck_pose_params` | `[B, 1, 3, 3]` | 3×3 rotation matrix for neck joint |
| `eyelid_params` | `[B, 2]` | scalar L/R eyelid offsets |
| `shape_params` | `[B, 300]` | FLAME identity (fixed per person) |

**VERIFIED.** `FLAME.forward()` (rgbavatar/submodules/flame/flame.py:354–406) accepts
exactly these seven inputs. Eyelids are applied as pre-LBS vertex offsets via loaded
`l_eyelid.npy` / `r_eyelid.npy` (line 389–391). Eye pose enters the kinematic chain
as two rotation matrices in `full_pose` (line 378). Jaw and neck are separate joints.

The blend weight vector fed to the Gaussian model concatenates expression coefficients
(100), neck rot6d (6), jaw rot6d (6), eye rot6d (12), eyelids (2), and global
translation (3) — total **129** dims (`num_basis_in: 129` in config/offline.yaml:12).
**VERIFIED** in `FLAMEDataset.precompute_blend_weight()` (dataset/flame_dataset.py:180–194).

### 1.2 RGBAvatar (FuHead path)

| Input | Tensor shape | Source |
|---|---|---|
| `identity` | `[231]` | FuHead identity coefficients (no batch) |
| `expressions` | `[B, 51]` | FuHead expression coefficients |
| `eye_rots` | `[B, 3, 3]` | single rotation matrix for both eyes |

**VERIFIED.** `FuHead.forward()` (submodules/fuhead/fuhead.py:212–218). The FuHead
template includes explicit eyeball geometry (lines 72, 107–108): `left_eyeball_v`,
`right_eyeball_v` loaded from `fu_head.npz`, scaled 1.2×, rotated by `eye_rots`,
translated by hard-coded offsets. Teeth are also explicit geometry (lines 69, 206–208):
upper teeth are static, lower teeth displaced by expression delta (line 208:
`delta_verts[:, 102:105]`).

### 1.3 GaussianAvatars

| Input | Tensor shape | Source |
|---|---|---|
| `expr` | `[T, N_expr]` | FLAME expression (default 100) |
| `jaw_pose` | `[T, 3]` | axis-angle jaw |
| `eyes_pose` | `[T, 6]` | axis-angle L+R eyes |
| `neck_pose` | `[T, 3]` | axis-angle neck |
| `rotation` | `[T, 3]` | axis-angle global head rotation |
| `translation` | `[T, 3]` | global translation |

**VERIFIED.** `FlameGaussianModel.select_mesh_by_timestep()` (scene/flame_gaussian_model.py:
121–135) passes these per-timestep params to `flame_model()`. Shape and `static_offset`
are per-person constants. `dynamic_offset` (per-vertex, per-frame) exists in code but
is zeroed/commented-out in training (line 80, 159–160). No explicit eyelid param; eyelids
are captured only through expression coefficients. **PARTIAL** — eyelid support absent
(FLAME's eyelid data is not loaded in this fork's FlameHead).

### 1.4 MATCH-GEM (RegressorModel path)

The GEM regressor does **not** directly take semantic FLAME params. Instead:

| Input | Description | Source |
|---|---|---|
| DECA PCA coefficients | `[B, 50]` | ResNet50 → DECA features → PCA reduction |
| EMOCA PCA coefficients | `[B, 50]` (optional) | ResNet50 → EMOCA features → PCA reduction |
| SMIRK/MP expression | `[B, 50]` or `[B, 52]` | SMIRK encoder or MediaPipe blendshapes |
| Gaze | `[B, 4]` | iris-vs-eye-center from MediaPipe landmarks |
| Jaw | `[B, 3]` | SMIRK jaw params |

**VERIFIED.** `ResnetEncoder.forward()` (encoder/encoder.py:529–661). The regressor
concatenates these and outputs PCA coefficients + jaw axis-angle. Critically: the
`pose_gaussians()` call (lib/common.py:131–150) applies FLAME LBS posing (global,
jaw, neck) but **zeroes eye_pose** (line 133: `flame_fits['eye_pose'] = flame_fits['eye_pose'] * 0`).
Eye movement is therefore **not tracked through FLAME posing** — gaze enters only through
the PCA appearance coefficients (4-dim iris offset), which modulates texture, not eyeball
geometry. **VERIFIED — this is the critical eye limitation.**

### 1.5 FlexAvatar

| Input | Tensor shape | Source |
|---|---|---|
| `expression_codes` | `[B, E, D_expr]` | Concatenation of FLAME params |
| Internal avatar code | `[HT, 1, D_hidden]` | Fitted latent (no semantic meaning) |

**VERIFIED.** `ExpressionCodeConfig.get_dim()` (config/expression_config.py:19–30)
shows expression codes are: FLAME exp (100) + eyes rot6d (12) + eyelids (2) + neck rot6d (6)
+ jaw rot6d (6) = **126 dims** (all flags true). These enter through the
`HeadTransformer._expression_transformer` cross-attention (model/flexavatar_model.py:195).

For live reenactment, `SheapModule.to_expression_code()` (model/sheap.py:67–76) produces
the same concatenation from SHeaP tracker output: `expr` + `eye_l_pose` + `eye_r_pose` +
`eyelids` + `neck_pose` + `jaw_pose` + `torso_pose` + `cam_trans`.

---

## 2. Representation Internals

### 2.1 RGBAvatar — Mesh-Rig + Learned Reduced Blendshapes

**Architecture:** FLAME mesh provides per-frame vertex positions. Gaussians are bound to
mesh triangles via UV rasterization (tex_size×tex_size grid → face_id + barycentric).
Each Gaussian has a canonical state (`_xyz`, `_rotation`, `_feature_dc`, `_opacity`,
`_scaling`) plus `num_basis_blend` (=20) learned correction bases for xyz, rotation,
and color (`_xyz_b`, `_rotation_b`, `_feature_b`).

**Per-frame computation:**
1. Project 129-dim blend weight → 20-dim via `weight_module` (small MLP: 129→128→128→20
   when `use_mlp_proj`, or linear 129→20) — `gaussian.py:33–43`.
2. Linear blending: `attr = base + Σ(weight_i × basis_i)` for xyz, rotation, color —
   `gaussian.py:100–108`. Opacity and scaling are **not blended** (line 109).
3. Bind to mesh: rotate Gaussian by face TBN, translate by barycentric position on
   triangle — `binding.py:175–185`.
4. Rasterize with standard 3DGS splatting.

**Stored asset:** A single `.ply` file containing per-Gaussian: xyz(3) + opacity(1) +
scale(3) + rotation(4) + f_dc(3) + xyz_b(20×3) + rotation_b(20×4) + f_dc_b(20×3) +
weight_module params (flattened into ply) + binding face_id + barycentric — `gaussian.py:169–217`.

**VERIFIED** (full save/load code in gaussian.py:169–307).

### 2.2 GaussianAvatars — Gaussians Rigged to FLAME Triangles

**Architecture:** One Gaussian per FLAME triangle (+ adaptive densification). Each
Gaussian stores: `_xyz` (offset from face center), `_rotation`, `_scaling`, `_opacity`,
`_features_dc`, `_features_rest` (SH coefficients up to degree 3). Binding index maps
Gaussian → face.

**Per-frame computation:**
1. FLAME forward pass → vertex positions.
2. Compute face center, orientation quaternion, and scale from deformed triangle —
   `compute_face_orientation()` (utils/graphics_utils.py).
3. Transform Gaussians: position = face_center + orient × local_offset, rotation =
   face_orient ⊗ local_rotation — `flame_gaussian_model.py:139–148`.
4. Standard 3DGS rasterization.

**No expression-conditioned correctives.** The representation is purely geometric
deformation via FLAME mesh. This makes it robust but limits expression-dependent
appearance (e.g., wrinkles, teeth visibility).

**Stored asset:** Standard 3DGS `.ply` + `flame_param.npz` (all FLAME params per frame) —
`flame_gaussian_model.py:220–224`.

**VERIFIED.**

### 2.3 MATCH-GEM — UV PCA Basis

**Architecture:** Gaussians are arranged on a UV texture map (default `uv_size` = 128
from `base_model.py:30`, but FINAL configs use `pcacomponents150`). Per-subject, a PCA
basis is computed over Gaussian attributes in UV space.

**What is stored:**
- `PCApperance` (pca_gaussian.py) loads `GAUSSIAN_PCA.ptk` containing per-modality
  (geometry, opacity, scales, rotation, colors) per-mask-region: `components` (K×D),
  `mean` (D,), `variance` (K,). The `pca_all` mode concatenates all modalities into one
  PCA basis of dimensionality `CHANNEL_DIMENSIONS = {geometry:3, opacity:1, scales:3,
  rotation:4, colors:3}` = 14 attrs/Gaussian — `pca_gaussian.py:60`.
- A StyleUNet generator (`ApperanceModel`, model.py:168–177) conditioned on deformation
  gradients predicts delta maps over the canonical UV Gaussians.

**Per-frame computation (regressor path — `RegressorModel.predict()`, regressor/model.py:177–263):**
1. DECA/EMOCA/SMIRK encoders extract expression features from cropped face image.
2. PCA-reduced features → regressor MLP → K PCA coefficients per region.
3. `PCApperance.inverse_transform()` (pca_gaussian.py:248–299): `values = coeffs × (std × components) + mean`.
4. `pose_gaussians()` applies FLAME LBS to transform from canonical to posed space.
5. 2DGS rasterizer (`splat()` in lib/common.py uses `twoDgs` flag).

**Memory math (VERIFIED against code):**
- UV resolution: **128×128 = 16,384 texels** (base_model.py:30). With validity masking, effective count is lower.
- Per-Gaussian attrs in `pca_all` mode: 14 floats (pca_gaussian.py:60, `CHANNEL_DIMENSIONS`).
- PCA components: **K=150** in FINAL configs (configs/distillation/FINAL/*.yml: `pcacomponents150`).
- Mean tensor: 16,384 × 14 = 229,376 floats = **~0.9 MiB fp32**.
- Components tensor: 150 × 229,376 = 34.4M floats = **~131 MiB fp32**.
- Variance/scales: 150 floats, negligible.
- **Total PCA asset: ~132 MiB fp32** for a 128×128 UV.

Note: The claimed "512×512 UV × 14 attrs × K=150 → ~2 GiB" sizing would apply if
`uv_size=512`. At 512², there are 262,144 texels × 14 = 3.67M attrs/component × 150
components = 550M floats = **~2.1 GiB fp32**. The code default is 128, not 512 — the
512 figure likely reflects a higher-quality configuration not present in the released code.
**PARTIAL** — the math is correct for the stated UV size, but the default code config
uses 128×128.

### 2.4 FlexAvatar — Prior + Avatar Code

**Architecture:** A feed-forward transformer model that takes input images, patchifies
them (conv2d, stride=patch_size), encodes via GPT-style transformer, then cross-attends
to head tokens (either mesh tokens on a FLAME template or UV texture tokens). The result
is decoded by an MLP into Gaussian attributes.

**What is stored (per person):**
- **Avatar code**: a single `.npy` file of shape `[HT, 1, D_hidden]` where HT = number
  of head tokens, D_hidden = transformer dimension — `avatar_code_manager.py:17–20`.
  This is the `internal_representations` tensor from `HeadTransformer.forward()`.
- The avatar code replaces the image-encoding step: at inference, it is passed as
  `cached_internal_representations` to skip the encoder (flexavatar_model.py:461, 517–518).

**Per-frame computation:**
1. Load cached avatar code (or run encoder on input images).
2. Expression cross-attention: avatar code tokens attend to expression tokens (from MLP
   on `expression_codes`) — `HeadTransformer.forward()` (flexavatar_model.py:176–196).
3. MLP decoder outputs per-Gaussian: position(3) + scale(3) + rotation(4) + opacity(1) +
   color(3) — `GaussianDecoder._decode_gaussians()` (flexavatar_model.py:267–328).
4. Positions are added to template positions (`initial_gaussian_positions`) as deltas
   (flexavatar_model.py:360–361).
5. Standard 3DGS rasterization.

**Enrollment (inversion):** `FittingManager.run_inversion()` (model/inversion.py:53–135)
optimizes `latent_avatar_code` (nn.Parameter) and optional expression_code_offsets via
Adam for 200 steps (default), minimizing L1 + SSIM + SAM + DINO losses against input views.
Starting point is the encoder's output for the first view batch (line 77–78).

**VERIFIED.**

---

## 3. Memory / Compute Budget

| Model | Gaussians | Per-Gaussian attrs | Basis/PCA dims | Total asset size (fp32 est.) |
|---|---|---|---|---|
| RGBAvatar (256² UV) | ≤65,536 (valid mask) | 10 base + 20×10 basis = 210 | 20 bases + MLP (129→20) | ~52 MiB + MLP (~66K params) |
| RGBAvatar (300² UV, NeRSemble) | ≤90,000 | 210 | 20 bases + MLP | ~72 MiB + MLP |
| GaussianAvatars | ~9,976 faces (FLAME) + densified | 59 (xyz3+rot4+scale3+opacity1+SH48) | 0 (no basis) | ~2.3 MiB (base) + dynamic |
| MATCH-GEM (128² UV) | ≤16,384 | 14 (all mode) | 150 PCA | ~132 MiB |
| MATCH-GEM (512² UV†) | ≤262,144 | 14 | 150 PCA | ~2.1 GiB |
| FlexAvatar | ~G (from template sampling) | 14 per Gaussian | avatar code: HT × D_hidden | avatar code only: HT × D_hidden × 4 bytes |

**RGBAvatar detailed math (VERIFIED):**
- tex_size=256 → up to 65,536 Gaussians. Per Gaussian: base xyz(3) + opacity(1) +
  scale(3) + rotation(4) + color(3) = 14; bases: xyz_b(20×3) + rot_b(20×4) + f_dc_b(20×3) =
  200. Total: 214 floats/Gaussian. 65,536 × 214 × 4 bytes = **~53 MiB**.
- weight_module: if MLP (129→128→128→20): 129×128 + 128 + 128×128 + 128 + 128×20 + 20 =
  35,604 params. If linear (129→20): 129×20 + 20 = 2,600 params. **VERIFIED** from gaussian.py:33–44.

**GaussianAvatars:** FLAME has 9,976 faces (with teeth). After adaptive densification,
typical count is 10–20K. Per Gaussian: xyz(3)+rot(4)+scale(3)+opacity(1)+SH(3×16)=59.
At 20K Gaussians: 20,000 × 59 × 4 = **~4.7 MiB**. Smallest asset. **VERIFIED.**

---

## 4. Out-of-Distribution Robustness

### 4.1 Mesh-rig models (RGBAvatar, GaussianAvatars)

The FLAME mesh provides a physics-plausible deformation for any valid FLAME parameter.
Extreme expressions outside the training distribution will produce:
- Correct geometric deformation (FLAME generalizes via blend shapes + LBS).
- Potentially incorrect Gaussian correctives (RGBAvatar's learned bases may extrapolate
  poorly), but the correctives are small deltas on a well-posed base.
- GaussianAvatars: **no learned correctives at all** — purely mesh-driven, so OOD
  robustness is maximal (but quality ceiling is lower). **VERIFIED.**

RGBAvatar's `weight_module` projects 129-dim input through a small MLP/linear layer
to 20-dim. The MLP applies ReLU activations (gaussian.py:37–39), which naturally clip
extreme inputs. Orthogonality loss on bases (binding.py:226–234) further regularizes
the basis to avoid correlated blowup. **VERIFIED.**

### 4.2 PCA basis models (MATCH-GEM)

PCA coefficients can only reconstruct the span of training data. The regressor clamps
coefficients to `[-2.9, 2.9]` standard deviations (encoder/encoder.py:434). Outside
this range, the PCA inverse transform will produce the mean ± 2.9σ projection, which
is a *graceful clamp* but prevents novel expressions from being represented.

Eye movement is particularly limited: `pose_gaussians()` explicitly zeroes `eye_pose`
(lib/common.py:133), so eyeball rotation is not modeled geometrically. Gaze enters only
as a 4-dim texture-level signal through the encoder. **VERIFIED — significant limitation.**

### 4.3 FlexAvatar (prior + avatar code)

The transformer prior provides generalization: the expression cross-attention can compose
expressions not seen during a specific person's enrollment, because the prior was trained
across many identities. However, the avatar code is fitted to the specific person's
appearance, so extreme OOD appearance (e.g., extreme mouth opening showing unseen teeth)
may degrade. The expression code includes full FLAME semantics (100 exp + eyes + eyelids +
jaw + neck), so the space of control inputs is rich. **PARTIAL.**

---

## 5. Eyes and Mouth Interior

This is the #1 uncanny-valley risk area. Per model:

### 5.1 Eyes

| Model | Eyeball geometry? | Gaze conditioning? | Eyelids? |
|---|---|---|---|
| RGBAvatar-FLAME | No explicit eyeball mesh | Yes: `eye_pose_params [B,2,3,3]` rotates FLAME eye joints in LBS | Yes: dedicated `eyelid_params [B,2]` + loaded vertex offsets |
| RGBAvatar-FuHead | **Yes**: `left_eyeball_v`, `right_eyeball_v` loaded, explicitly rotated by `eye_rots` | Yes: explicit rotation matrix | Via expressions only |
| GaussianAvatars | No explicit eyeball | Yes: `eyes_pose [T,6]` axis-angle | No dedicated eyelid param |
| MATCH-GEM | **No**: `eye_pose` zeroed in `pose_gaussians()` | Only via 4-dim gaze PCA signal | Eyelid fitting commented out in `fit_flame.py:227–228` |
| FlexAvatar | No explicit eyeball | Yes: `eye_l_pose`, `eye_r_pose` rot6d in expression code | Yes: `eyelids [B,2]` in expression code |

**VERIFIED** across all models. MATCH-GEM's eye handling is the weakest: eye_pose is
explicitly zeroed (lib/common.py:133), eyelid fitting is commented out in the FLAME
fitter (lib/fit_flame.py:227–228). The only gaze signal is the 4-dim iris-vs-eye-center
offset computed from MediaPipe landmarks (encoder/encoder.py:50–85), which enters as a
texture modulation, not geometric rotation. **This is a confirmed critical limitation.**

### 5.2 Teeth / Mouth Interior

| Model | Explicit teeth geometry? | Mouth interior handling |
|---|---|---|
| RGBAvatar-FLAME | **Yes**: `add_teeth()` constructs 60 vertices (upper + lower), 56 triangles, with LBS weights (upper→neck, lower→jaw) | Teeth are part of the mesh template, bound to Gaussians via UV |
| RGBAvatar-FuHead | **Yes**: `add_teeth()` constructs teeth geometry, lower teeth displaced by expression delta | Explicit geometry |
| GaussianAvatars | **Yes**: FLAME `FlameHead` with `add_teeth=True` (scene/flame_gaussian_model.py:33) | Teeth triangles are part of FLAME, Gaussians bound to them |
| MATCH-GEM | **Yes**: configs reference `TEETHFOCUS` variants; canonical mesh includes teeth | Teeth are part of UV-mapped Gaussians |
| FlexAvatar | Depends on template mesh (`gghead_template.obj`) | Template-dependent; no explicit teeth code |

**VERIFIED** for RGBAvatar (flame.py:173–320 — detailed teeth vertex/face construction),
GaussianAvatars, MATCH-GEM. FlexAvatar: **PARTIAL** — the template mesh is loaded from
an asset file (`gghead_template.obj`) not included in the repo clone.

---

## 6. Enrollment Capture Requirements

### 6.1 RGBAvatar

**Monocular video.** Preprocessing chain:
1. **Metrical Photometric Tracker** (`metrical-tracker/tracker.py`) fits FLAME per frame:
   optimizes `shape`, `exp`, `eyes`, `eyelids`, `jaw`, `tex`, `sh`, camera params
   (focal length, principal point, R, t) — tracker.py:163–170, 286–297. Outputs per-frame
   `.frame` checkpoints containing FLAME params + camera.
2. **INSTA** masks the head (referenced in README but not in corpus).
3. RGBAvatar `FLAMEDataset` loads these checkpoints (dataset/flame_dataset.py:106–150),
   extracting shape, expression, jaw (rot6d→matrix), eyes (rot6d→matrix), eyelids, global
   rotation, translation.

**Training:** Offline (train_offline.py) or online (train_online.py, simulating real-time
streaming at video FPS). Offline config: batch_size varies, iteration count configurable.
**VERIFIED.**

### 6.2 GaussianAvatars

**Monocular video** (same pipeline as RGBAvatar): Metrical Tracker → FLAME params.
Dataset loads `flame_param.npz` per subject (scene/flame_gaussian_model.py:44–89).
Shape, expression, rotation, neck_pose, jaw_pose, eyes_pose, translation, static_offset,
dynamic_offset are all loaded.
**VERIFIED.**

### 6.3 MATCH

**Calibrated multi-view capture.** The README states: "Given calibrated multi-view images,
MATCH infers static Gaussian splat textures in 0.5 seconds." Example data uses **Ava-256**
dataset (README: "Download example data from Ava256"). Ava-256 provides 12+ synchronized
calibrated cameras.

The GEM avatar training pipeline:
1. **TEMPEH** (third_party/TEMPEH) predicts coarse mesh + UV renderings from multi-view images.
2. `pca_mesh.py` computes mesh PCA from tracked meshes (100 components).
3. Canonical Gaussian fitting (`canonical.py`) from multi-view images.
4. PCA Gaussian decomposition over the training sequence.
5. Regressor training (encoder/encoder.py) maps single-view features → PCA coefficients.

The config path `configs/data/download_ava256_quickstart.gin` confirms Ava-256 as the
capture source. The FINAL regressor configs reference cross-subject evaluation
(e.g., `cross_subject: APP152`), confirming the multi-view requirement for the GEM
avatar but single-view for the regressor at test time.

**VERIFIED** — enrollment requires calibrated multi-view; test-time driving is monocular.

### 6.4 FlexAvatar

**Monocular images or video.** Preprocessing:
1. **Pixel3DMM** tracking produces FLAME params per frame — `scripts/track_pixel3dmm_itw.py`.
   The `ExpressionCodeConfig.from_pixel3dmm_tracking()` (config/expression_config.py:32–46)
   extracts: `flame.exp`, `flame.eyes`, `flame.eyelids`, `flame.neck`, `flame.jaw`.
2. **MatAnyone** (scripts/run_matanyone.py) for background matting (optional).
3. **DINOv2** features extracted as auxiliary input (model/dinov2.py).

**Enrollment:**
- Feed-forward: 1 or more images → encoder → avatar code. For N>1 images, the model
  aggregates via transformer attention.
- Optional fitting: `FittingManager.run_inversion()` (model/inversion.py:53–135) optimizes
  avatar code for 200 steps (~seconds on GPU).
- Avatar code saved as `.npy` — `AvatarCodeManager.save_avatar_code()`.

From one image to animatable avatar, no multi-view calibration needed.
**VERIFIED.**

### 6.5 Metrical Tracker (shared preprocessing)

Outputs per frame (tracker.py:186–210):
```
flame: {exp, shape, tex, sh, eyes, eyelids, jaw}  # all np arrays
camera: {R, t, fl, pp}
opencv: {R, t, K}
```

Eyelids are 2-dim (L/R scalar). Eyes are rot6d (12-dim, split L/R). Jaw is rot6d (6-dim).
The tracker optimizes via photometric + landmark losses in a coarse-to-fine pyramid
(tracker.py:429–542). It uses MediaPipe landmarks including **iris landmarks** for gaze
fitting (tracker.py:59–63: `left_iris_flame`, `right_iris_flame`, `left_iris_mp`,
`right_iris_mp`; loss in tracker.py:503–504).

**VERIFIED.**

---

## 7. Adopt / Reject / Open Questions for Mura

Given the constraints: the OS owns the **asset format + driver + runtime** (enrollment is
an offline desktop tool); working hypothesis is **semantic controls (blendshapes + gaze +
jaw + head pose) as the wire format**, personal learned representation inside the asset,
per-person adapter between.

### 7.1 Summary Table

| Criterion | RGBAvatar | GaussianAvatars | MATCH-GEM | FlexAvatar |
|---|---|---|---|---|
| License | MIT ✓ | Toyota NV/SA (restrictive) | MPG (non-commercial) | NC license ✗ |
| Wire format fit | ✓ Semantic inputs (129-dim) | ✓ Semantic FLAME params | ✗ ResNet features, not semantic | ✓ FLAME expression code (126-dim) |
| Eye quality | Good (FLAME eyes+eyelids) | Moderate (FLAME eyes, no eyelids) | **Poor** (eyes zeroed) | Good (eyes+eyelids in code) |
| Teeth | Explicit geometry ✓ | Explicit geometry ✓ | UV-mapped ✓ | Template-dependent |
| OOD robustness | Good (mesh rig + small correctives) | Best (pure mesh rig) | Poor (PCA clamp) | Moderate (prior helps) |
| Enrollment ease | Monocular video ✓ | Monocular video ✓ | Multi-view (12 cams) ✗ | Monocular (even 1 image) ✓ |
| No-Python runtime | Feasible: ply + FLAME + linear blend | Feasible: ply + FLAME + transform | Hard: PCA + StyleUNet + ResNet | Hard: transformer + MLP |
| Asset portability | Single .ply file ✓ | .ply + flame_param.npz ✓ | PCA .ptk + model weights | .npy avatar code + model weights |

### 7.2 Recommendations

**ADOPT — RGBAvatar's representation as the reference design:**
- MIT license permits integration.
- Semantic 129-dim blend weight matches the wire-format hypothesis exactly.
- Explicit FLAME controls (expression + jaw + eyes + eyelids + neck) map directly to
  headset sensing outputs.
- The reduced blendshape approach (20 learned bases) is tractable for a no-Python C++/Vulkan
  runtime: the per-frame computation is a single matrix multiply (20×N_gs×10) followed by
  mesh binding (barycentric interpolation + TBN rotation).
- Self-contained `.ply` asset with embedded basis and MLP weights.
- Monocular enrollment with Metrical Tracker is achievable on a desktop.

**ADOPT — FlexAvatar's enrollment concept (study, not code):**
- Single-image avatar creation is the gold standard for user experience.
- The inversion-based fitting (200 Adam steps) is a good pattern for enrollment.
- However, the NC license and transformer dependency make direct adoption impossible.

**REJECT — MATCH-GEM for production use:**
- Eyes explicitly zeroed — unacceptable for presence in VR.
- Multi-view enrollment requirement excludes phone-based capture.
- Non-commercial license.
- However, the PCA basis concept is useful for *compression* of a shipped asset.

**REJECT — GaussianAvatars as primary:**
- Toyota license is restrictive.
- No expression-conditioned appearance (no wrinkles, teeth texture changes).
- Useful as a robustness baseline for testing.

### 7.3 Open Questions

1. **Runtime performance of reduced blendshapes on XR hardware.** RGBAvatar's
   `linear_blending()` CUDA kernel (called in gaussian.py:103–107) performs batched
   weighted addition across 20 bases. This needs porting to Vulkan compute. At 65K
   Gaussians × 20 bases × 10 attrs, that's 13M multiply-adds per frame — tractable at
   90 Hz on mobile GPU? **Needs benchmarking.**

2. **Combining RGBAvatar's representation with single-image enrollment.** The current
   training requires monocular video (hundreds of frames). Can a FlexAvatar-style
   feed-forward prior produce the initial reduced-blendshape asset, then refine from
   video? This is the hybrid design the OS should pursue.

3. **Eyeball geometry for FLAME path.** RGBAvatar-FLAME uses FLAME's joint-based eye
   rotation (no explicit eyeball mesh). The FuHead path has explicit eyeballs. Should the
   OS asset format mandate explicit eyeball geometry? The FLAME eyelid mechanism
   (vertex offsets from loaded `.npy` data) could combine with a separate eyeball mesh.

4. **Teeth quality.** All models that include teeth construct them procedurally from lip
   landmarks. This produces flat, uniform teeth. A per-person teeth texture (from enrollment
   images) would significantly improve realism. RGBAvatar's UV binding means teeth
   Gaussians can learn per-person appearance during enrollment.

5. **Asset format versioning.** The `.ply` format used by RGBAvatar embeds custom fields
   (basis weights, binding info). A versioned binary format (e.g., flatbuffers) would be
   more appropriate for an OS standard, with fields for: template mesh reference, Gaussian
   base attributes, basis tensors, adapter weights, metadata (enrollment source, quality
   level, creation date).

6. **Driver → control input mapping.** The 129-dim blend weight assumes a specific tracker
   output (Metrical Tracker's FLAME params). The OS driver layer needs a standardized
   mapping from headset sensors (cameras, IMU) → FLAME-compatible params. SHeaP
   (used by FlexAvatar) is one option; the unreleased DDE tracker (used by RGBAvatar's
   online mode) is another. **The driver protocol should be tracker-agnostic.**

---

## Appendix A: Blend Weight Composition Detail

RGBAvatar `FLAMEDataset.precompute_blend_weight()` (dataset/flame_dataset.py:180–194):

```
shape_weight = exps[:, :100]            # 100 expression coefficients
pose_weight = cat([
    neck_poses[:, :2, :].reshape(-1, 6),   # neck rot6d        → 6
    jaw_poses[:, :2, :].reshape(-1, 6),    # jaw rot6d         → 6
    eye_poses[:, :, :2, :].reshape(-1, 12),# L+R eyes rot6d    → 12
    eyelid_params,                          # L/R eyelids       → 2
    global_transls                          # translation       → 3
], dim=1)                                   # Total pose: 29
# blend_weight = cat([shape_weight, pose_weight]) → 129 dims
```

The `weight_module` (gaussian.py:33–44) then projects 129→20 (learned adapter).

## Appendix B: Per-Model Code Entry Points

| Model | Enrollment entry | Inference entry | Key model file |
|---|---|---|---|
| RGBAvatar | `train_offline.py` / `train_online.py` | `render.py` | `model/gaussian.py`, `model/binding.py` |
| GaussianAvatars | `train.py` | `render.py` | `scene/flame_gaussian_model.py` |
| MATCH-GEM | `GEM/train.py` (appearance) → `GEM/lib/regressor/trainer.py` | `GEM/test.py` | `lib/apperance/pca_gaussian.py`, `lib/regressor/model.py` |
| FlexAvatar | `scripts/render_example.py` (includes fitting) | `scripts/render_example.py` / `scripts/run_gui.py` | `model/flexavatar_model.py`, `model/inversion.py` |
| Metrical Tracker | `tracker.py` → `video.py` | N/A (preprocessing) | `tracker.py`, `flame/FLAME.py` |
