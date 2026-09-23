# Codec-Avatar Route Audit (Learned Universal Expression Latent)

**Audit date:** 2026-09-22
**Scope:** whether Meta's codec-avatar driving stack (universal 256-d expression latent + headset-image encoder) is recoverable from public artifacts as a v2 upgrade path for the Mura Persona system.
**Method:** read-only study of `references/ava-256` (main + local study branches `study/pr-1`, `study/pr-7`, `study/pr-19`), `references/goliath`, `references/match`, plus paper-level web verification. No builds, no dataset/checkpoint downloads; S3 objects were verified by HTTP `HEAD` only (headers, no payload).
**Verdict definitions:** as in `docs/research/16-perception-claims-audit.md` — VERIFIED / PARTIAL / UNVERIFIED / FABRICATED.

## Executive finding

The learned-driver route splits into two problems with very different status.
**Driving an existing Ava-256 subject** is bounded engineering: the universal decoder checkpoint, per-frame dome expression codes, headset-frame ground-truth codes, and full encoder training code are all publicly recoverable, though scattered across three unmerged PRs whose file formats do not quite agree.
**Enrolling a NEW person into the latent space** is a research project, not bounded engineering: every public path to a code runs through registered meshes + unwrapped textures in Ava topology, produced upstream by closed registration tooling, and no trained headset-encoder or mesh-encoder-for-new-subjects checkpoint is published. MATCH/TEMPEH provides a credible open substitute for the registration step, but nobody has demonstrated the splice.
For Mura this means: ship the semantic control space as v1, and make the Persona control interface a **versioned, opaque-vector-capable contract** so a learned-latent driver can be added without touching the renderer.

# Part 1 — ava-256 `main`: what the code-producing path actually is

## 1.1 The autoencoder consumes registered meshes and textures, not images

**Verdict: VERIFIED**

`Autoencoder.forward` in `references/ava-256` main (`models/autoencoder.py`) takes as encoder inputs `avgtex: [B, 3, 1024, 1024]` ("Texture map averaged from all viewpoints"), `verts: [B, 7306, 3]` ("Mesh vertex positions"), plus neutral-frame counterparts (`neut_avgtex`, `neut_verts`, `target_neut_avgtex`, `target_neut_verts`). There is no image input to the expression path; camera images appear only as rendering targets.
`models/encoders/expression.py` computes the code from `(verts - neut_verts)` rasterized to a UV geometry map and `(avgtex - neut_avgtex)`, emitting a `[64, 4, 4]` feature map; `utils.py` (`get_autoencoder`) wraps it in `vae.VAE_bottleneck(64, 16)`, giving a **16×4×4 = 256-dim expression code**. The decoder consumes it reshaped as `(16, 4, 4)` (see `study/pr-1:models/headset_encoders/ud.py`, `encodings.reshape(b, 16, 4, 4)`).
The `ExpressionEncoder` docstring states the intent explicitly: "We discard this encoder after training, and plug other ways to drive the decoder, eg, from a head-mounted device with outside-in cameras, such as a Quest Pro" (`models/encoders/expression.py`).

Consequence: the released "code-producing path" requires per-frame **registered meshes in the 7306-vertex Ava face topology** (`assets/face_topology.obj`) and per-frame **unwrapped view-averaged textures**. The dataset ships these as `kinematic_tracking/registration_vertices.zip` and `uv_image/color.zip` (`download.py`), but the registration/unwrapping tooling that produced them is not in the repo. For dataset subjects the inputs exist; for a new person they do not.

## 1.2 `download.py` "encoder" asset group on main

**Verdict: VERIFIED**

Main's `download.py` `ASSETS["encoder"]` contains exactly `frame_list.csv` plus five camera zips: `image/cam-cyclop.zip`, `cam-left-eye-atl-temporal.zip`, `cam-left-mouth.zip`, `cam-right-eye-atl-temporal.zip`, `cam-right-mouth.zip`. Main's `README.md` describes this as "5 infrared camera views captured from a Quest Pro". This group was merged into main via upstream PR #3 ("Download encoder data as well", merged 2024-06-16 per the GitHub API). Main does **not** download ground-truth encodings or any checkpoint.

# Part 2 — The study branches (unmerged upstream PRs)

Upstream status via GitHub API (2026-09-22): PR [#1 "UE"](https://github.com/facebookresearch/ava-256/pull/1), PR [#7 "add ability to download UD checkpoint + UE codes"](https://github.com/facebookresearch/ava-256/pull/7), and PR [#19 "Tobias/bg images and expr codes download"](https://github.com/facebookresearch/ava-256/pull/19) are all still **open**, never merged. The local branches are shallow single-commit grafts (no merge base with main), so all statements below come from tree-level diffs (`git diff main study/pr-N`).

## 2.1 PR-1: the universal headset-image encoder

**Verdict: VERIFIED as code; PARTIAL as a runnable artifact**

What it consumes/emits (`study/pr-1:models/headset_encoders/universal.py`, `tests/test_headset_encoder.py`, `data/headset_dataset.py`):

- Input: `[B, 4, 1, 400, 400]` — **four** single-channel 400×400 headset views (`left-eye-atl-temporal`, `right-eye-atl-temporal`, `left-mouth`, `right-mouth`; the cyclop camera is loadable but not in the default view list), each reduced to one channel via `arr[:1]` in the dataloader.
- Conditioning: `[B, 4, num_conds, 1, 400, 400]` neutral-segment frames of the same user (`cond_headset_cam_img`), fused at feature level — a code-level echo of the calibration mechanism in the paper (§5.1).
- Architecture: two `timm` `tf_mobilenetv3_large_100` backbones (eye/face), 1×1-conv conditioning fusion, MLP head → **`[B, 256]` expression code** (`ddp-train-ue.py` instantiates `UniversalEncoder(in_chans=1, out_chans=256, num_views=4)`).

Supervision (`models/headset_encoders/loss.py`, `configs/config_universal_encoder.yaml`): three losses — `img_L1` (frontal render of predicted code vs frontal render of ground-truth code, through a **frozen** universal decoder from a fixed dome camera, `FixedFrontalViewGenerator` in `ud.py`), `expression` (MSE to `gt_latent_code`), and `geo_weighted_L1` (decoded vertices, weighted by `assets/face_weight_geo.npy`). Note the image loss compares render-to-render, not render-to-photo, so the frozen decoder is the sole ground-truth oracle.

Internal dependencies that are NOT public:

- Ground-truth codes are loaded from `rosetta_correspondences.zip` pickles keyed `{sid}-{segment}_{segment_name}_{frame}` under `gt_dir = /uca/leochli/oss/ava256_udmapping_v2/render_expression_regressor` (`data/headset_dataset.py`, config). **VERIFIED mismatch**: the publicly downloadable ground truth (PR-7) is `encoder/encodings_gt.pt` + `encodings_framelist.csv` — different filename, container format, and directory layout. A small adapter must be written; none exists in any branch.
- The frozen decoder is loaded via `UDWrapper(ud_exp_name="/uca/leochli/oss/ava256_universal_decoder")`, which requires `{exp_name}/ae_info.pkl`, `{exp_name}/aeparams.pt`, and per-identity `identity_conditioning/{ident}/id_cond.pkl` (`models/headset_encoders/ud.py`). `ae_info.pkl` is not in any download manifest and `HEAD` probes at plausible S3 paths return 404. Workaround exists on main: `get_autoencoder(dataset, assetpath)` reconstructs the exact architecture from `assets/face_topology.obj` + dataset stats, and `generate_id_cond.py` regenerates `id_cond.pkl` from decoder data + checkpoint — so `ae_info.pkl` is avoidable with modest glue code.

Training-loop details worth recording (`ddp-train-ue.py`, `configs/config_universal_encoder.yaml`):

- Loss weights: `img_L1: 1.0`, `expression: 1.0e-3`, `geo_weighted_L1: 5.0`; Adam with cosine annealing; batch 4/GPU; headset images loaded at `downsample=2.083` (≈192 px effective).
- The identity split is unusual: `enc_ids_train` is the **first 15%** of identities and the remaining 85% are validation — consistent with encoder-generalization evaluation, and a reminder that this branch is an experiment scaffold, not a productionized trainer.
- The subject list comes from `256_ids_merged.csv`, which extends main's `256_ids.csv` with `hcd`/`hct` (headset capture date/time) columns; `data/utils.py` adds a `HeadsetCapture` class whose folder convention is `{sid}_{hcd}--{hct}`, distinct from the decoder's `{mcd}--{mct}--{sid}` — the two capture sessions are paired per subject but are different recordings (no dome/headset temporal sync exists or is needed).
- Headset segments are named like `vrs_file_{SID}-raised_eyebrows-2023-04-05-17-52-51` with per-segment `image.zip` archives of `{frame}-{camera}.png`, ten images per frame index (`data/headset_dataset.py:build_framelist`); the neutral conditioning frame is pulled from the middle of a `neutral`/`EXP_neutral` segment.

Bit-rot: **VERIFIED**. PR-1's snapshot (2024-06-14) predates main's encoder-download group (it *removes* it in the tree diff) and predates the helper-math consolidation merged via upstream PRs #5/#6 (main has `extensions/include/helper_math.h`; PR-1 carries per-directory copies). It also contains outright bugs on the untraveled path: `self.self.latent_code_dim` in `data/headset_dataset.py` (placeholder-GT branch crashes), `Path(latent_code_directory)` raising `TypeError` when the directory is `None` despite the `Optional` signature, and `args.worldsize`/`args.world_size` naming drift in `ddp-train-ue.py`. Merging PR-1 onto main today is a manual port, not a rebase — but the port surface is ~5 new files plus small diffs to `models/autoencoder.py` and `models/decoders/assembler.py` (a `decode_geo` pass-through used by the geometry loss), which is bounded.

## 2.2 PR-7: decoder checkpoint + encoder ground-truth codes

**Verdict: VERIFIED, including live S3 objects**

`study/pr-7:download.py` adds to the encoder group `encodings_framelist.csv` and `encodings_gt.pt` (per capture), and a new per-dataset group `checkpoints: ["decoder/aeparams_1440000.pt"]` with its own URL scheme (`{size}/checkpoints/...`, per-dataset rather than per-capture).

HTTP `HEAD` probes against the S3 bucket named in `download.py` (`BPATH`, capture `20230405--1635--AAN112`, 2026-09-22 — headers only, no payloads):

| S3 object | Result |
|---|---|
| `4TB/checkpoints/decoder/aeparams_1440000.pt` | 200 OK, multipart ETag (`…-32`), Last-Modified 2024-07-17 |
| `4TB/…/decoder/expression_codes/aeparams_1440000.pkl` | 200 OK, Last-Modified 2024-10-02 |
| `4TB/…/encoder/encodings_gt.pt` | 200 OK, Last-Modified 2024-07-17 |
| `4TB/…/encoder/encodings_framelist.csv` | 200 OK, Last-Modified 2024-07-17 |
| `4TB/…/encoder/frame_list.csv`, `encoder/image/cam-cyclop.zip` | 200 OK, Last-Modified 2024-06-16 |
| `4TB/checkpoints/decoder/ae_info.pkl`, `4TB/checkpoints/ae_info.pkl` | **404** |
| `4TB/checkpoints/encoder/encoder.pt` | **404** |

So a **trained universal decoder checkpoint is publicly hosted**, and per-headset-frame ground-truth codes exist for every capture. No trained *encoder* checkpoint is published anywhere (404 at the plausible path; none referenced in any branch or README). Whether `aeparams_1440000.pt` loads cleanly into main's `get_autoencoder` graph is UNVERIFIED without downloading it (the state-dict key layout cannot be inspected by HEAD), though the architecture constructors on main are deterministic given `assets/face_topology.obj` and dataset statistics, so a key mismatch would be diagnosable and fixable.

## 2.3 PR-19: dome expression codes for every decoder frame

**Verdict: VERIFIED, including live S3 objects**

`study/pr-19:download.py` + `DATASHEET.md` add per-capture `decoder/expression_codes/aeparams_1440000.pkl` — "a pickled dictionary containing a mapping `frame_id => expression_code` where `expression_code` is a 256-dimensional code" (4TB release only) — plus `foreground_masks` and `background_image`. `HEAD` confirms the pkl exists for capture `20230405--1635--AAN112`. The filename ties the codes to the same decoder iteration as PR-7's checkpoint, so checkpoint and codes are mutually consistent (**the codes are meaningless without that specific decoder**).

# Part 3 — Goliath

**Verdict: VERIFIED trainable head code; data-gated**

`references/goliath` contains trainable **Relightable Gaussian Codec Avatar** heads: `ca_code/models/rgca.py` implements a *personalized* (single-subject) VAE whose encoder consumes `registration_vertices` + unwrapped color texture (again mesh+texture, not images: `Encoder.forward(geom, color)` flattens `n_verts_in * 3` vertices through a linear layer and convolves the 1024×1024 texture), bottlenecks to `n_embs: 256` (`config/rgca_example.yml`), and decodes 3D Gaussians rendered via `ca_code/utils/render_gsplat` with learnable radiance-transfer relighting (spherical harmonics + spherical Gaussians via `extensions/sgutils`). The repo also carries `mesh_vae_drivable.py` (drivable mesh bodies), `urhand.py`/`hand_mvp.py` (hands) — the RGCA head model is the one relevant here.
It is driven by that per-subject 256-d embedding (plus view/lighting), not by a universal latent and not by headset images; there is no headset-encoder code in the repo at all. The README says head-capture data access "is currently gated" (email request), unlike ava-256's open S3 bucket. Relevance to this audit: it confirms the mesh+texture encoding pattern across the entire Codec Avatar Studio line and offers a trainable Gaussian head decoder (a candidate v2 renderer study target), but it does not shorten the new-person-into-latent-space path.

# Part 4 — MATCH as the registration bridge

**Verdict: PARTIAL — the pieces exist and are frame-aligned; the splice is undemonstrated**

Verified facts from `references/match`:

- MATCH (CVPR 2026) is demonstrated on Ava-256 subjects: `configs/GEM/train_avatar_APP152.yml` sets `subject: APP152`, `cross_subject: PGO261`; `configs/tempeh/save_predictions_GEM.yml` runs on `'["APP152", "PGO261"]'`.
- Its coarse stage is TEMPEH trained on Ava-256 (`configs/tempeh/train_tempeh.yaml`, `--dataset-directory data/ava-256`; vendored TEMPEH under `third_party/TEMPEH`), i.e., **feed-forward multi-view registration into the Ava 7306-vertex topology** — MATCH's own dataloader reads `kinematic_tracking/registration_vertices.zip` as `(7306, 3)` arrays (`match/data/ava256_dataset.py`). A pretrained TEMPEH checkpoint is distributed via the README's Google Drive assets link (existence of the checkpoint itself: UNVERIFIED without downloading).
- MATCH vendors a PR-19-era copy of `download.py` (`scripts/data/download_ava256.py` lists the `expression_codes` availability entry and the encoder group) but **never consumes the expression codes** — no reference to `expression_codes` or `aeparams` anywhere outside the downloader. Its GEM recipe downloads only `foreground_masks camera_calibration frame_list head_pose image` for two subjects (`configs/data/download_ava256_GEM.gin`).
- The GEM avatar driver is a separate personal control space: per-frame Gaussian textures → PCA over registered Gaussians (150 components, `configs/GEM/train_avatar_APP152.yml`: `pca_gauss.py`), FLAME fits (`fit_flame_to_predictions.py`), and a DECA-feature coefficient regressor for monocular driving (`scripts/GEM/predict_face_features.py`, `train_avatar.py`).

The proposed bridge experiment (same-subject dome frames carry BOTH a universal code and MATCH-personal coefficients; fit an adapter u→(z,jaw)): the data plumbing is *visible but not connected*. Both sides key on the same dome `frame_id` from the same `frame_list.csv` (PR-19 codes are `frame_id => code`; MATCH's GEM dataset is built from those same dome frames), so pairing needs no temporal sync — only a join on frame id. What does not exist anywhere: code that loads both and fits the adapter. That is new work, but small (a regression over a few thousand paired frames per subject), and everything it needs is downloadable. UNVERIFIED empirically: whether a low-capacity adapter preserves expression fidelity across the two spaces.

# Part 5 — Paper study

## 5.1 Universal Facial Encoding of Codec Avatars from VR Headsets (Bai et al., SIGGRAPH 2024, [arXiv:2407.13038](https://arxiv.org/abs/2407.13038))

**Verdict: VERIFIED paper; the production system is far beyond the released code**

The paper confirms: identity-independent encoder on consumer-headset cameras; ground-truth codes produced by a style-transfer correspondence pipeline (a generalization of Schwartz et al. 2020, lighting-modulated) against per-subject dome avatars; lightweight runtime calibration from ~6 anchor expressions; and self-supervised pre-training with a cross-view masked-reconstruction objective on **17,000+ subjects of unpaired HMC data** (paired set: 266 subjects; augmented 8-camera *training* headset vs. the consumer *tracking* camera subset). The expression latent is 4×4×16 — exactly Ava-256's 256-d space. None of the SSL pipeline, correspondence tooling, or trained encoders is released; PR-1 is a "really simple" supervised cousin (its own docstring) of this system. The paper also notes the encoder drives phone-scan avatars "in the same expression latent space" — direct precedent for latent-space universality.

## 5.2 SqueezeMe ([arXiv:2412.15171](https://arxiv.org/abs/2412.15171), SIGGRAPH 2025)

**Verdict: VERIFIED, but decoder-only and full-body**

Distills the pose-corrective decoder of Gaussian *full-body* avatars into linear layers: 64-d pose code → correctives; 5 ms quantized on Quest 3's XR2 Gen 2 NPU for the full linear layer, **0.45 ms** with corrective sharing (60k→4k correctives); 3 avatars at 72 FPS via a custom Vulkan splatting pipeline. It is evidence that *decoding* a learned latent is cheap on headset NPUs — it says nothing about the encoder side, and no code release was found.

## 5.3 URAvatar (SIGGRAPH Asia 2024) and FiCA (2026)

**Verdict: VERIFIED papers; NO code released for either**

URAvatar ([arXiv:2410.24223](https://arxiv.org/abs/2410.24223)): universal relightable Gaussian prior trained on hundreds of relightable multi-view scans; personalization by fine-tuning on a phone scan (64 A100s to train the prior). No repository exists; a contemporaneous author response says "not yet".
FiCA ([arXiv:2606.24232](https://arxiv.org/abs/2606.24232)): feed-forward single-image avatar in ~5 s via Sapiens-based UV unwrapping + UV diffusion + a hypernetwork Universal Prior Model, driven "with any facial expression codes". No public code as of 2026-09. The universal-prior line is therefore paper-only for outsiders; the closest open artifacts remain ava-256's decoder and MATCH.

# Part 6 — The central question: getting a NEW person into the latent space

The code-producing path, verified from source: `registered mesh (7306 verts, Ava topology) + unwrapped avg texture → ExpressionEncoder → VAE bottleneck → 256-d code` (Part 1.1). Registration and unwrapping tooling: closed. Candidate routes for a new person:

## Route 1 — MATCH/TEMPEH registration + texture reprojection into the released mesh+tex encoder

**Exists:** TEMPEH inference in Ava topology from calibrated multi-view (`configs/tempeh/save_predictions_*.yml` also emit UV renderings); ava-256's `ExpressionEncoder`/`IdentityEncoder` and (via PR-7) a decoder checkpoint to validate against.
**New work:** re-training or scale-aligning TEMPEH for a non-dome capture rig; producing a view-averaged 1024×1024 texture matching the dataset's `uv_image` statistics; verifying that codes from TEMPEH-registered meshes land in-distribution for the decoder.
**Unverifiable without downloads:** TEMPEH checkpoint quality, decoder checkpoint loadability, texture-domain gap. **Assessment: the most credible route; a multi-week research-engineering effort with real risk in the texture domain gap, requiring a multi-camera enrollment rig.**

## Route 2 — Train an RGB→code encoder on Ava-256 frontal frames + published codes

**Exists:** all supervision data is public — 80-view dome images and per-frame 256-d codes (PR-19) for up to 256 identities; foreground masks and background images (PR-19) to clean the input; and the frozen decoder (PR-7) to reuse PR-1's render-consistency losses unchanged. PR-1's `UniversalEncoderLoss` is largely input-agnostic: swapping the headset dataloader for a dome-frontal dataloader is a contained change.
**New work:** the encoder itself (nothing like it in any studied repo), plus robustness across 256 identities to a *new* face under casual lighting — precisely the generalization problem Bai et al. needed 17K subjects and SSL to solve, albeit for the harder oblique-IR case. Dome lighting is uniform and controlled; casual RGB is not, and Ava-256 contains no lighting variation to train that robustness from.
**Assessment: trainable today as an experiment; generalization to unseen identities from 256 subjects under uncontrolled lighting is the open research risk.** A hybrid (Route 1 for enrollment/identity, a Route 2-style network for runtime driving, PR-1 code as the trainer with the `encodings_gt.pt` adapter) is the plausible v2 shape.

For the headset-sensing case specifically (the actual Mura deployment target), note the sensing mismatch: PR-1 and the ava-256 encoder data assume Quest Pro-style *face-observing* IR cameras. A Mura target headset without eye/face cameras cannot use this encoder family at all, which independently justifies keeping the semantic v1 space (drivable from generic gaze/expression estimators) as the floor.

## Route 3 — None/other

Waiting on URAvatar/FiCA-class releases is not a plan (Part 5.3). Goliath does not provide universality (Part 3).

# Verdict

**Classification: (b) a research project — with a bounded-engineering core inside it.**

- Bounded engineering (weeks): reproduce headset-driven animation of *Ava-256 subjects* — port PR-1 onto main, write the `encodings_gt.pt` adapter, regenerate `id_cond.pkl`, train the encoder against the released decoder. Every required artifact is verified to exist publicly. License note: ava-256 is CC BY-NC 4.0 (`README.md`), so anything derived from the checkpoint or codes is non-commercial.
- Research project (months, uncertain): new-person enrollment (Routes 1/2) and casual-conditions robustness. No public artifact demonstrates either.
- Not blocked: nothing essential is behind Meta-internal walls *for the dataset subjects*, and the MATCH bridge gives the new-person problem a concrete, testable attack.

## Implications for the Mura Persona asset format

The decisive coupling fact: **a 256-d code is only meaningful relative to a specific decoder checkpoint** (PR-19's codes are literally named `aeparams_1440000.pkl` after the decoder iteration). Therefore the control interface must version the *pair* (control space, renderer/decoder), not just the driver. v1 should reserve:

1. **A versioned control-space descriptor** in the avatar asset: `control_space = { id, kind: "semantic-v1" | "latent", dim, decoder_binding }`. Semantic v1 declares blendshapes+gaze+jaw+pose; a latent v2 declares an opaque dim (e.g., 256) bound to a named decoder artifact hash.
2. **An opaque float-vector channel** in the driver→renderer protocol, alongside the semantic channels: `(space_id, seq/timestamp, float[dim])`. Renderers ignore spaces they don't declare; this is the entire hook needed for a learned driver to slot in without renderer changes.
3. **A calibration/enrollment hook**: both PR-1 (neutral conditioning frames) and Bai et al. (anchor expressions) require per-session reference captures. The asset format should allow storing per-user calibration blobs keyed by `(control_space id, driver id)`.
4. **Raw sensor provenance**: keep synchronized multi-view timestamps in any enrollment/recording format, since the cross-view SSL objective (Bai et al. §Part 5.1) and the MATCH frame-join both depend on per-frame alignment, not temporal heuristics.
5. **Adapter slots, not space unification**: the MATCH bridge shows control spaces will be related by learned adapters (u→(z,jaw)); the protocol should permit a translation node between declared spaces rather than assuming one canonical space.

## Recommended v2 experiment sequence (if/when the route is picked up)

Ordered so that each step falsifies the route as early and cheaply as possible; all steps except 5 use only public artifacts.

1. **Checkpoint smoke test** — download `4TB/checkpoints/decoder/aeparams_1440000.pt` plus one capture's `kinematic_tracking`/`uv_image` assets; load through main's `get_autoencoder` + `load_checkpoint`; render a known frame and compare against the dataset image. Falsifies: checkpoint/architecture mismatch.
2. **Code round-trip** — feed the same capture's registered mesh + texture through the released `ExpressionEncoder` and compare the resulting code against PR-19's `expression_codes/aeparams_1440000.pkl` for the same `frame_id`. Falsifies: the assumption that the published codes come from this encoder/checkpoint pair (currently inferred from the shared `1440000` name, not proven).
3. **Encoder training reproduction** — port PR-1 onto main (helper-math layout, `download.py` conflict, the three bugs in Part 2.1), write the `encodings_gt.pt`/`rosetta_correspondences.zip` adapter, regenerate `id_cond.pkl` via `generate_id_cond.py`, train on a small identity subset. Falsifies: recoverability of headset-driven animation for dataset subjects.
4. **MATCH bridge** — run pretrained TEMPEH + MATCH on APP152/PGO261 dome frames, join per-`frame_id` with PR-19 codes, and fit the u→(z,jaw) adapter in both directions. Measures how much expression information survives the space translation — the cheapest empirical answer to whether the two control-space families are compatible.
5. **New-person enrollment probe** — capture one subject with a calibrated multi-camera rig, push through TEMPEH→texture-reprojection→`ExpressionEncoder`, and inspect whether decoded renders (with a borrowed identity conditioning) stay on-manifold. This is the step where Route 1 lives or dies, and the only one needing new hardware.

Steps 1–2 are days; 3–4 are weeks; 5 is open-ended. None of them blocks v1, which is the point of the reserved hooks above.

## Claim table

| Claim | Verdict |
|---|---|
| Main's `Autoencoder.forward` consumes verts+avgtex (registered mesh + unwrapped texture), not IR images | **VERIFIED** (`models/autoencoder.py`, `models/encoders/expression.py`) |
| Main's `download.py` encoder group = framelist + 5 Quest Pro IR camera zips | **VERIFIED** (`download.py`, `README.md`) |
| PR-1 encoder: 4 views, 1-channel 400×400, neutral conditioning, emits 256-d code | **VERIFIED** (`universal.py`, `tests/test_headset_encoder.py`) |
| PR-1 supervision expects `rosetta_correspondences.zip`; public GT is `encodings_gt.pt` — mismatch | **VERIFIED** (`data/headset_dataset.py` vs `study/pr-7:download.py`) |
| PR-1/7/19 are unmerged and PR-1 is bit-rotted vs main | **VERIFIED** (GitHub API; tree diffs) |
| Decoder checkpoint + dome codes + headset GT codes exist on public S3 | **VERIFIED** (HTTP HEAD 200, 2026-09-22) |
| A trained universal *encoder* checkpoint is published | **FABRICATED** (no manifest entry; probe 404) |
| Goliath contains trainable RGCA heads (Gaussians, per-subject 256-d VAE, mesh+tex encoder) | **VERIFIED** (`ca_code/models/rgca.py`) |
| MATCH runs on Ava-256 subjects APP152/PGO261 with TEMPEH coarse stage in Ava topology | **VERIFIED** (`configs/GEM/*`, `configs/tempeh/*`, `match/data/ava256_dataset.py`) |
| Bridge adapter data plumbing (frame-keyed pairing) is downloadable; adapter code exists | **PARTIAL** (pairing verified; no adapter code anywhere) |
| Bai et al. system is recoverable from PR-1 | **UNVERIFIED at best** (17K-subject SSL corpus and correspondence tooling unreleased) |
| SqueezeMe shows 0.45 ms decoder on Quest 3 NPU — decoder-only, full-body, no code | **VERIFIED** ([arXiv:2412.15171](https://arxiv.org/html/2412.15171)) |
| URAvatar / FiCA code releases | **FABRICATED if assumed** — no code for either (project pages, 2026-09) |
