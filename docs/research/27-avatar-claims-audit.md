# Avatar Claims Audit

**Audit date:** 2026-09-22  
**Scope:** external avatar-reconstruction, driving, licensing, and mobile Gaussian-splat rendering claims used by the proposed Mura Persona design.
**Method:** read-only review of primary papers/project pages, current public repository trees, and the pinned local clones listed in `references/MANIFEST.json`; no code, models, or datasets were executed or downloaded.

## Verdict definitions

- **VERIFIED** — the named artifact exists and primary evidence supports the material claim with the stated scope.
- **PARTIAL** — the artifact exists, but the claim mixes stages, omits conditions, overstates release status, or is only partly supported.
- **UNVERIFIED** — no available primary evidence establishes the claim.
- **FABRICATED** — the named artifact does not exist, or an authoritative source directly contradicts the claim.

## Executive finding

The research base is mostly real, but several summaries collapse reconstruction, avatar decoding, animation, and rasterization into one “runtime” number.
That is not safe.
FlexAvatar's 20 FPS, 1.7 GB, and two-minute figures are genuine appendix claims, but apply to one RTX 3090 implementation, not mobile hardware ([paper](https://arxiv.org/html/2512.15599)).
MATCH really predicts one registered static Gaussian texture in 0.5 seconds, yet its complete MATCH-to-GEM avatar still takes 4.6 hours; these are different pipeline boundaries ([paper](https://arxiv.org/html/2603.15811v1)).
SqueezeMe is the strongest direct mobile result: three animated 60k-Gaussian full-body avatars at 72 FPS on a standalone Quest 3, with a custom Vulkan renderer and HTP decoder ([paper](https://arxiv.org/html/2412.15171)).
HRM2Avatar is stronger raw raster evidence on Apple hardware: 533,695 splats at 1920×1824×2 and 90 FPS on Vision Pro M2, and a real Apache-2.0 Metal runtime tree now exists ([runtime](https://github.com/alibaba/Taobao3D/tree/main/HRM2Avatar)).
Neither result is an open, measured Mura path on XR2+ Gen 2.
The defensible verdict is therefore: **40–60k animated splats in stereo at 72 Hz are demonstrated on the closely related Quest 3/XR2 Gen 2 class; the exact open Linux/Vulkan XR2+ implementation and 90 Hz target remain speculative until measured.**

# Part 1 — Paper, repository, and headline-claim audit

## 1. FlexAvatar — Tobias Kirschstein et al.

**Overall verdict: VERIFIED, with important scope qualifications**

The paper is real: **“FlexAvatar: Learning Complete 3D Head Avatars with Partial Supervision,”** arXiv:2512.15599 and CVPR 2026, pages 18193–18203 ([CVPR record](https://openaccess.thecvf.com/content/CVPR2026/html/Kirschstein_FlexAvatar_Learning_Complete_3D_Head_Avatars_with_Partial_Supervision_CVPR_2026_paper.html)).
Do not confuse it with Peng et al.'s different CVPR 2026 paper also named FlexAvatar, arXiv:2512.17717.

Claim-by-claim:

- **VERIFIED — 20 FPS animation plus rendering on RTX 3090.** Appendix B.5 says exactly that ([paper](https://arxiv.org/html/2512.15599)).
- **VERIFIED — 1.7 GB inference VRAM.** The same appendix reports 1.7 GB.
- **VERIFIED — approximately two minutes for avatar creation.** The appendix says “including all processing”; this is not a universal timing for every fitting protocol.
- **PARTIAL — “two-minute avatar creation” as the sole timing.** The paper separately reports about one minute for avatar-code-only fitting, seven minutes for four-view/1,000-step fitting, and ten minutes for 900-frame/2,000-step fitting.
- **VERIFIED — live SHeaP-tracker reenactment added in June 2026.** The local pinned README records “19.06.2026: Add Live Re-enactment” and installs `sheap-3.9`.
- **VERIFIED — released code and checkpoints.** The repository contains inference, fitting, evaluation, GUI, custom-input tracking instructions, and two checkpoint links ([repository](https://github.com/tobias-kirschstein/flexavatar)).
- **PARTIAL — complete reproducibility.** Ava-256 evaluation code is explicitly “not released yet”; the full setup also depends on Pixel3DMM and separately obtained data/models.
- **VERIFIED — CC BY-NC 4.0 root license.** The pinned local `LICENSE` is Creative Commons Attribution-NonCommercial 4.0.

Architecture consequence: FlexAvatar is credible enrollment/quality research, but 20 FPS on RTX 3090 is evidence against treating its decoder as the headset hot path without substantial replacement or distillation.

## 2. MATCH — Malte Prinzler et al.

**Overall verdict: VERIFIED for the method and most numbers; PARTIAL for the compressed pipeline summary**

The paper, arXiv:2603.15811, is **“Feed-forward Gaussian Registration for Head Avatar Creation and Editing,”** CVPR 2026, pages 25270–25280 ([CVPR record](https://openaccess.thecvf.com/content/CVPR2026/html/Prinzler_Feed-forward_Gaussian_Registration_for_Head_Avatar_Creation_and_Editing_CVPR_2026_paper.html)).
The official repository and weights/assets exist ([repository](https://github.com/malteprinzler/match)).

Claim-by-claim:

- **VERIFIED — feed-forward registration.** MATCH predicts dense, semantically corresponding Gaussian-splat textures from calibrated multi-view images.
- **VERIFIED — 0.5 seconds per frame.** This is one static registered MATCH prediction, not a complete animatable avatar.
- **VERIFIED — approximately 4.6 hours for the complete MATCH-based GEM avatar.** Table 4 reports 4.6 h versus GEM's 45.3 h, averaged over 12-camera sequences of about 3,300 frames.
- **PARTIAL — “3,212 frames, A100, 4.63 h.”** The public main paper supports the rounded 4.6 h and approximately 3,300-frame setup; this audit did not find the exact 3,212/A100 formulation in the primary HTML.
- **VERIFIED — 12-view default input.** Training/evaluation use twelve 640×512 head-centered images.
- **VERIFIED — plausible from four views.** The appendix says combined TEMPEH+MATCH produces plausible results starting at four views.
- **VERIFIED — LPIPS 0.230 at four views versus 0.187 at twelve.** These are the combined-input-view ablation values, not the headline benchmark comparison.
- **VERIFIED — in-the-wild COLMAP experiment.** The appendix shows outdoor off-the-shelf-camera captures with camera parameters estimated by COLMAP.
- **VERIFIED — CAP4D-generated-view experiment, narrowly.** For single-image inference, CAP4D supplies additional 2D views that are then input to MATCH; this is not a native one-image MATCH mode.
- **VERIFIED — interpolation.** Dense correspondence enables smooth cross-identity and cross-expression Gaussian-texture interpolation.
- **VERIFIED — no eye tracking in the final avatars.** The limitations state that subject-specific avatars “do not track eye movement.”
- **VERIFIED — training-expression interpolation limit.** The same limitation binds the avatar to interpolations of training expressions.
- **PARTIAL — fully released training stack.** Inference and MATCH-to-GEM workflows exist, but “Training MATCH” remains “Coming soon” in the pinned README.
- **VERIFIED — MIT root license.** The local root `LICENSE` is MIT.
- **VERIFIED — restrictive embedded GEM license.** `third_party/GEM/LICENSE` allows only personal, single-user, non-commercial scientific/educational/artistic use; forbids redistribution/sublicensing and many fields of use.

The MIT label therefore does not relicense the vendored GEM implementation.
For Persona, MATCH is strongest as an enrollment-time registration primitive, not as proof of instant end-to-end avatar creation.

## 3. GAF — Gaussian Avatar Reconstruction via Multi-view Diffusion

**Overall verdict: VERIFIED paper and numbers; VERIFIED that official source is still absent**

GAF is a real CVPR 2025 paper, arXiv:2412.10209, titled **“Gaussian Avatar Reconstruction from Monocular Videos via Multi-view Diffusion”** ([paper](https://arxiv.org/html/2412.10209v2)).

- **VERIFIED — monocular video plus multi-view diffusion.** The diffusion prior fills unseen regions and expressions.
- **VERIFIED — about 12 hours and 32 GB on one A6000.** This is avatar reconstruction after the diffusion model has been trained.
- **VERIFIED — 62 FPS at 802×550.** The reported 0.016 s is post-reconstruction rendering on the authors' system.
- **VERIFIED — official repository still says “Source Code Coming Soon.”** Its current root contains only README/media, not implementation ([repository tree](https://github.com/tangjiapeng/GAF)).

GAF is useful quality evidence, not a reproducible or practical enrollment backend today.

## 4. OFERA

**Overall verdict: VERIFIED venue and inference release; PARTIAL as a reproducible trainable system**

OFERA is a real IEEE VR 2026/TVCG special-issue paper, DOI 10.1109/TVCG.2026.3680590, TVCG 32(5), pages 3820–3830 ([bibliographic record](https://koasas.kaist.ac.kr/handle/10203/343611)).
The title is **“OFERA: Blendshape-Driven 3D Gaussian Control for Occluded Facial Expression to Realistic Avatars in VR.”**

- **VERIFIED — Quest Pro blendshape-driven control.** The headset supplies occluded-face expression/blendshape input; the avatar computation is not demonstrated as standalone Quest execution.
- **VERIFIED — tested inference environment is Windows 11, RTX 4090, CUDA 11.8, PyTorch 2.6, Unity 2021.3, and Quest Link.**
- **VERIFIED — training documentation is “TBD.”**
- **PARTIAL — inference weights.** The README supplies an OFERA Google Drive link plus external FateAvatar and mapping-matrix prerequisites; release completeness and long-term availability are weaker than a self-contained package.
- **VERIFIED — MIT root license.**
- **VERIFIED — dependency caveats.** The local notices identify separate FateAvatar, original Gaussian Splatting, NVIDIA nvdiffrast, Unity viewer, and an unlicensed-local-copy concern for `mediapipe-blendshapes-to-flame`.

OFERA demonstrates the control concept but not an XR2-resident runtime or a reproducible training path.

## 5. FiCA

**Overall verdict: PARTIAL**

FiCA exists as arXiv:2606.24232, dated 2026-06-23, with an official project page ([project](https://kim-youwang.github.io/FiCA), [paper](https://arxiv.org/html/2606.24232v1)).
It creates a drivable Gaussian Codec Avatar from one portrait without person-specific test-time optimization.

- **PARTIAL — “under five seconds.”** Primary wording is “within 5 seconds” or “5 seconds,” not a strict less-than-five bound.
- **VERIFIED — feed-forward single-portrait creation and real-time driving are paper claims.**
- **UNVERIFIED — public code/weights.** No official repository, checkpoint, inference script, or software license was found; the obvious `kim-youwang/FiCA` GitHub path returns 404.

FiCA is a research direction, not an available Persona dependency.

## 6. URAvatar

**Overall verdict: VERIFIED paper; UNVERIFIED public implementation**

URAvatar is **“Universal Relightable Gaussian Codec Avatars,”** SIGGRAPH Asia 2024, DOI 10.1145/3680528.3687653, arXiv:2410.24223 ([project](https://junxuan-li.github.io/urgca-website/), [paper](https://arxiv.org/abs/2410.24223)).

- **VERIFIED — universal relightable Gaussian prior.** It is trained on hundreds of controlled multi-view/light captures.
- **VERIFIED — phone-scan personalization.** The prior is fine-tuned with inverse rendering on a phone capture under unknown illumination.
- **UNVERIFIED — public code or pretrained weights.** The official project page exposes the paper/video, not a cloneable implementation or checkpoint.

The method validates a design pattern, not a deployable enrollment stack.

## 7. Apple HeadsUp

**Overall verdict: VERIFIED paper and project repository; FABRICATED if described as a code release**

Apple published **“Large-Scale High-Quality 3D Gaussian Head Reconstruction from Multi-View Captures”** in May 2026; the project identifies ECCV 2026 and arXiv:2605.04035 ([Apple page](https://machinelearning.apple.com/research/gaussian-head-reconstruction), [paper](https://arxiv.org/html/2605.04035v1)).
The method uses calibrated multi-view images, a transformer encoder, and UV-parameterized foreground/background Gaussian decoders trained on an internal 10k-subject dataset.

- **VERIFIED — scalable feed-forward multi-view Gaussian reconstruction.**
- **VERIFIED — a public GitHub repository exists.**
- **FABRICATED — “HeadsUp code is released.”** The current tree contains the website, README, images/videos, and licenses only; no model implementation, training/inference code, or weights ([tree](https://github.com/apple-aiml-research/ml-headsup)).
- **VERIFIED — repository text uses Apple's permissive sample-code license; published data/media use CC BY-NC-ND 4.0.**

## 8. SqueezeMe

**Overall verdict: VERIFIED system result; UNVERIFIED public code**

SqueezeMe is a real Meta paper, arXiv:2412.15171 and SIGGRAPH 2025, DOI 10.1145/3721238.3730599 ([project](https://forresti.github.io/squeezeme/), [paper](https://arxiv.org/html/2412.15171)).

- **VERIFIED — neural decoder distilled to a linear layer.**
- **VERIFIED — Gaussian corrective sharing.** Approximately 60k/65k Gaussians share 4,096 correctives.
- **VERIFIED — 0.45 ms on Quest 3.** This is the quantized linear corrective decoder on half an HTP core, not rendering and not total frame time.
- **VERIFIED — three avatars at 72 FPS on standalone Quest 3.** This is simultaneous animation and custom Vulkan rendering, and is the most relevant end-to-end mobile/XR evidence in this audit.
- **PARTIAL — “15 avatars.”** The paper extrapolates that the decoder budget could decode 15 in parallel; the demonstrated complete system renders three.
- **VERIFIED — renderer and decoder are distinct bottlenecks.** The paper explicitly optimizes each separately.
- **UNVERIFIED — public code.** No official source repository, runtime package, model, or license was found.

SqueezeMe demonstrates feasibility but does not supply Mura with an implementation.

## 9. HRM2Avatar

**Overall verdict: VERIFIED, including the public runtime tree**

HRM2Avatar is **“High-Fidelity Real-Time Mobile Avatars from Monocular Phone Scans,”** SIGGRAPH Asia 2025, arXiv:2510.13587 ([paper](https://arxiv.org/html/2510.13587)).

- **VERIFIED — 533,695 splats.**
- **VERIFIED — Vision Pro M2 at 1920×1824×2 and 90 FPS.** The reported total is 8.38 ms: 6.44 ms rendering, 0.71 ms sorting, 1.12 ms projection, and 0.11 ms other.
- **VERIFIED — iPhone 15 Pro Max at 2048×945 and 120 FPS.** The reported total is 8.18 ms.
- **VERIFIED — Metal runtime source exists.** `alibaba/Taobao3D/HRM2Avatar` contains CMake, assets, iOS/macOS/visionOS shells, C++/Objective-C++ renderer, animation, neural compensation, and third-party directories ([runtime tree](https://github.com/alibaba/Taobao3D/tree/main/HRM2Avatar)).
- **VERIFIED — Apache-2.0 root license.**
- **VERIFIED — runtime-only/simplified release.** The README calls it a “simplified reimplementation”; enrollment/training/reconstruction code is not released there.
- **PARTIAL — direct XR2 evidence.** It is excellent mobile stereo evidence, but Metal on Apple M2/A17 does not establish Vulkan/Adreno throughput.

## 10. RGBAvatar

**Overall verdict: VERIFIED**

RGBAvatar is a CVPR 2025 Highlight paper, **“Reduced Gaussian Blendshapes for Online Modeling of Head Avatars,”** pages 10747–10757 ([CVPR record](https://openaccess.thecvf.com/content/CVPR2025/html/Li_RGBAvatar_Reduced_Gaussian_Blendshapes_for_Online_Modeling_of_Head_Avatars_CVPR_2025_paper.html)).

- **VERIFIED — 81 s training and 398 FPS at 512×512 on RTX 3090.**
- **VERIFIED — 20 reduced Gaussian blendshape bases.**
- **VERIFIED — reported runtime includes animation calculation and rendering.**
- **VERIFIED — MIT root license in the local clone.**
- **VERIFIED — substantial code is released.** Offline/online training, evaluation, rendering, a CUDA raster extension, NeRSemble scripts, and twelve pretrained-avatar links are present.
- **VERIFIED — real-time DDE tracker integration is not released.** The pinned README says the online FaceWarehouse+DDE version “will [be] release[d] … in the future”; “Real-time Demo” remains TBD.
- **PARTIAL — “online” as turnkey live enrollment.** The released `train_online.py` consumes a stream-like sequence, but the real-time tracking frontend needed for the paper's live system is absent.

The desktop numbers are credible; they do not transfer directly to XR2.

## 11. Ava-256

**Overall verdict: VERIFIED dataset and universal-model source release; PARTIAL if read as a turnkey product**

The current main repository provides 256 paired high-resolution dome and Quest Pro HMC captures ([repository](https://github.com/facebookresearch/ava-256), [Meta page](https://www.meta.com/emerging-tech/codec-avatars/ava256/)).

- **VERIFIED — 256 paired subjects.** Decoder data has 80 RGB camera views; encoder data has five Quest Pro infrared views.
- **VERIFIED — universal encoder and decoder code.** Main contains a joint trainable model/configuration, distributed training entry point, CUDA raymarch/utilities, rendering, tests, and download tooling.
- **VERIFIED — pretrained models/assets are part of the release claim.** Meta's dataset page explicitly lists universal decoder and HMC universal encoder models.
- **PARTIAL — “code for encoders AND decoders” as two clean libraries.** It is a research training codebase with tightly coupled data formats and CUDA extensions, not two portable headset SDK components.
- **VERIFIED — default download is approximately 4 TB, with 8/16/32 TB variants.**
- **VERIFIED — code, data, and model weights are CC BY-NC 4.0.** This is stated in the paper supplement and local root license.

## 12. LAM_Audio2Expression

**Overall verdict: PARTIAL; the quoted claim conflates three different releases**

The official local clone is `aigc3d/LAM_Audio2Expression`, an Apache-2.0 PyTorch/CUDA project that generates ARKit expressions from audio ([repository](https://github.com/aigc3d/LAM_Audio2Expression)).
Its official weights are about 373 MB as an archived streaming checkpoint and are marked Apache-2.0 on Hugging Face ([official weights](https://huggingface.co/3DAIGC/LAM_audio2exp/tree/main)).

- **VERIFIED — official project targets real-time ARKit expression generation.**
- **UNVERIFIED — official CPU benchmark.** The root README documents CUDA 11.8/12.1 setup, not a measured CPU deployment.
- **FABRICATED — official repository ships a 192 MB ONNX model.** No ONNX file or ONNX instructions exist in the pinned official clone.
- **PARTIAL — 192 MB ONNX, fixed one-second input, 52 ARKit outputs at 30 FPS.** These are real properties of the third-party `omote-ai/lam-a2e` conversion, not the official repository ([conversion](https://huggingface.co/omote-ai/lam-a2e)).
- **PARTIAL — “real-time CPU.”** A different third-party derivative, `myned-ai/wav2arkit_cpu`, claims about 45 ms per second of audio and is only 1.8 MB; it is not the 192 MB model ([CPU derivative](https://huggingface.co/myned-ai/wav2arkit_cpu)).
- **FABRICATED — LAM official weights are CC BY-NC.** Current official model metadata says Apache-2.0, matching the code.

Any Persona integration must select and audit one concrete artifact instead of combining these properties.

## 13. Universal Facial Encoding of Codec Avatars

**Overall verdict: VERIFIED paper/system claim; UNVERIFIED public implementation**

Bai et al.'s **“Universal Facial Encoding of Codec Avatars from VR Headsets”** is arXiv:2407.13038 and ACM TOG 43(4), SIGGRAPH 2024 ([paper](https://arxiv.org/html/2407.13038v1)).

- **VERIFIED — real-time encoding from consumer-headset HMC views.**
- **VERIFIED — generalization to unseen users through cross-view self-supervision.**
- **VERIFIED — lightweight user calibration.** Users perform predefined anchor expressions; feature-level calibration adds almost no inference compute/latency.
- **PARTIAL — deployable consumer solution.** The paper demonstrates the method, but this audit found no official code, weights, camera interface, or software license.

# Part 2 — Mobile/XR2-class Gaussian-splat feasibility

## 14. What is actually demonstrated

### SqueezeMe: direct Quest/XR2 Gen 2 evidence

**Verdict: VERIFIED**

Quest 3 uses Snapdragon XR2 Gen 2.
SqueezeMe demonstrates three simultaneously animated 60k-Gaussian avatars at 72 FPS locally on that headset.
The system divides work: a quantized linear decoder runs on HTP, while custom Vulkan handles projection, sorting, and splat rendering on Adreno.
The 0.45 ms figure is decoder-only.
The 72 FPS result is the full animation-plus-render system.
No public source means Mura cannot inspect renderer assumptions, eye-buffer resolution, sustained thermals, compositor load, or exact single-pass stereo implementation.

### HRM2Avatar: high-count mobile stereo evidence

**Verdict: VERIFIED on Apple; PARTIAL for XR2 transfer**

Vision Pro demonstrates 533k animated splats at native 1920×1824 per eye and 90 FPS.
The released runtime uses GPU-driven Metal, hierarchical culling, compressed/rearranged data, and shared/single-pass stereo work.
Its timing breakdown proves that sorting/projection and rasterization are separately material.
Apple M2 bandwidth, tile architecture, Metal APIs, and thermals differ from Adreno, so this is feasibility evidence rather than a Qualcomm benchmark.

### Mobile-GS

**Verdict: VERIFIED paper number; PARTIAL reproducibility**

Mobile-GS reports 116 FPS at 1600×1063 on Snapdragon 8 Gen 3 ([project/repository](https://github.com/xiaobiaodu/Mobile-GS)).
The official repository states that company policy prevents release of the mobile Vulkan implementation; only an initial CUDA version is public.
The reported mobile result is therefore credible paper evidence but not independently reproducible from released code.

### VRSplat

**Verdict: PARTIAL for “mobile VR”**

VRSplat reports 72+ FPS and addresses popping, stereo floaters, and foveated rasterization ([paper](https://arxiv.org/abs/2505.10144)).
Its Quest 3 is tethered to an RTX 4090; it is not standalone Adreno evidence.
It is relevant to VR quality and raster strategy, not to XR2 throughput.

### Standalone viewers and forks

**Verdict: PARTIAL**

An independent Unity Quest 3 fork reports only 16–18 FPS for roughly 300k splats at stereo/native settings ([fork](https://github.com/arghyasur1991/UnityGaussianSplatting)).
`destefy/3DGS-Snapdragon` is a real Android adaptation tested on Snapdragon 8 Gen 2/OnePlus 12R, but publishes no defensible headline FPS ([repository](https://github.com/destefy/3DGS-Snapdragon)).
A 2026 DisplayXR Android branch reports about 45 FPS on an Adreno phone at render scale 0.45 while retaining 25% of splats; that is useful engineering evidence, not a full-resolution headset result ([commit](https://github.com/DisplayXR/displayxr-demo-gaussiansplat/commit/8edaf04e39c3f39af03ea6d28bed4ec12aadf2a7)).
These results show that algorithm, resolution, retained splat count, overlap, sorting, and tile-GPU fit dominate; “Vulkan support” alone predicts little.

## 15. Local renderer clone audit

### `3dgs-cpp`

**Verdict: PARTIAL as an XR2 starting point**

The pinned clone is commit `8fe4b2f` dated 2024-11-13.
It is LGPL-2.1 and implements 3DGS with Vulkan compute pipelines.
Its documented platforms are Windows, Linux, macOS, iOS/iPadOS, and visionOS through MoltenVK.
Android, Qualcomm Spaces, OpenXR, immersive visionOS, improved radix sort, and subgroup batching remain TODO items.
The local clone therefore supplies portable renderer structure, not a demonstrated Adreno/OpenXR backend.

### `vkgs`

**Verdict: PARTIAL as a renderer reference**

The pinned clone is commit `6c9266e` dated 2026-07-17, but its README says it is not actively maintained.
It uses compute for visibility/preprocessing and radix sorting, then a Vulkan graphics pipeline for splat drawing and hardware blending; describing it as wholly compute-rasterized would be wrong.
Published desktop numbers are 350+ FPS at 1600×900 on RTX 4090 and 50+ FPS on Apple M2 Pro.
There is no Android, Adreno, OpenXR, or stereo-headset result.
Its MIT Vulkan radix/sort/render design is useful, but its benchmark does not answer XR2 feasibility.

## 16. Cost boundaries that must remain separate

1. **Enrollment/reconstruction:** seconds to hours, normally off-device; FlexAvatar, MATCH, GAF, RGBAvatar, FiCA, HeadsUp, and HRM2Avatar report incompatible boundaries.
2. **Per-frame control/decoder:** maps pose, expression, gaze, or audio to Gaussian attributes; SqueezeMe's 0.45 ms is here.
3. **Per-frame animation transforms:** LBS, blendshape mixing, skinning, and corrective application; RGBAvatar's desktop runtime includes this.
4. **Projection/culling/sorting:** often a major mobile cost; HRM2Avatar reports 1.83 ms combined projection/sorting.
5. **Splat raster/composite:** depends on eye resolution, visible count, projected area, overdraw, SH degree, and tile strategy.
6. **Full XR frame:** stereo rendering plus compositor, application scene, tracking, timewarp, thermal/power limits, and synchronization.

Quoting any one stage as “avatar latency” is misleading unless the endpoints are named.

## 17. XR2+ Gen 2 verdict

**40–60k animated Gaussians, stereo, 72 Hz: PLAUSIBLE AND DEMONSTRATED IN A COMPARABLE CLOSED SYSTEM.**
SqueezeMe directly demonstrates three 60k avatars at 72 FPS on Quest 3/XR2 Gen 2.
One avatar with 40–60k splats on the faster XR2+ Gen 2 should be a reasonable engineering target.

**The Mura open Linux/Vulkan implementation: NOT YET DEMONSTRATED.**
No audited public renderer combines the required Adreno path, stereo/OpenXR integration, animation decoder, compositor coexistence, and headset measurements.

**90 Hz at target eye resolution: SPECULATIVE ON XR2+.**
HRM2Avatar demonstrates 90 Hz stereo on Vision Pro M2, not Adreno.
Resolution, overdraw, Gaussian footprint, sorting policy, SH degree, and thermal envelope can erase the apparent splat-count margin.

Required proof is an on-headset benchmark with 40k/60k splats, representative head close-ups, both eyes, target render scale, animation updates every frame, compositor/timewarp active, and sustained power/thermal logging.

# Part 3 — Licensing map

Licensing is recorded here for provenance and distro planning; it does not gate research-MVP algorithm selection.

## 18. Pinned local repositories

| Repository | Root code/data license in pinned clone | Material caveat |
|---|---|---|
| `rgbavatar` | MIT | Requires separately licensed FLAME/FaceWarehouse/tracker assets; pretrained-model terms are not separately stated in root README. |
| `gaussianavatars` | CC BY-NC-SA 4.0 | Includes original Gaussian Splatting license; Toyota statement prohibits commercial use without agreement. |
| `match` | MIT | Vendored GEM is restrictive single-user non-commercial research software with no redistribution/sublicensing; other TEMPEH/Sapiens/Pyrender terms also apply. |
| `flexavatar` | CC BY-NC 4.0 | Checkpoints appear under the repository's root terms unless separately stated; Pixel3DMM/BFM/data dependencies retain their own terms. |
| `ava-256` | CC BY-NC 4.0 | Paper supplement expressly applies it to data, code, and model weights. |
| `goliath` | CC BY-NC 4.0 | Dataset access is gated; repository calls itself a pre-release. |
| `baballonia` | Babble Software Distribution License 1.0 | Custom non-commercial copyleft; forbids commercial hardware integration and requires source release for derivatives. |
| `eyetrackvr` | Babble Software Distribution License 1.0 | README says software uses this custom license; documentation is CC BY-SA 4.0. |
| `vrcfacetracking` | Apache-2.0 | Modules and hardware SDK dependencies may add separate terms. |
| `ofera` | MIT for original root code | FateAvatar MIT; Gaussian-Splatting and NVIDIA components have separate terms; one included/referenced converter lacks a local license. |
| `lam-audio2expression` | Apache-2.0 | Official Hugging Face checkpoint metadata is also Apache-2.0, not CC BY-NC. |
| `3dgs-cpp` | LGPL-2.1 | Bundled dependencies include MIT, Apache-2.0, and zlib components. |
| `vkgs` | MIT | Vulkan radix-sort and other submodules retain their own licenses. |

## 19. Other audited artifacts

- **HRM2Avatar:** Taobao3D root and released runtime are Apache-2.0; third-party Metal C++, MNN, JSON, ShaderConductor, and Unity-derived components retain their own licenses.
- **Apple HeadsUp:** repository text/site is under Apple's sample-code-style license; data/media are CC BY-NC-ND 4.0; there is no released model code to reuse.
- **GAF:** no source release and no software license found in the placeholder repository.
- **FiCA:** no public code/weights license found.
- **URAvatar:** no public code/weights license found.
- **SqueezeMe:** no public code/model license found because no implementation release was found.
- **Universal Facial Encoding:** no public implementation license found.
- **OFERA weights:** the repository root is MIT, but external FateAvatar, mapping, and Gaussian dependencies must be traced individually; the README does not clearly grant a distinct license for every downloaded weight.
- **LAM official weights:** Apache-2.0 according to current `3DAIGC/LAM_audio2exp` metadata; the prior “CC BY-NC weights” claim is contradicted.
- **FLAME 2017/2019/2020/2023 models:** custom non-commercial scientific-research/education/art license with redistribution and field-of-use restrictions ([license](https://flame.is.tue.mpg.de/modellicense.html)).
- **FLAME 2023 Open:** separately released under CC BY 4.0, with additional content-use conditions stated by the FLAME site; do not assume an older `generic_model.pkl` is covered.

# Final claim summary

| Item | Verdict | Most consequential correction or confirmation |
|---|---|---|
| FlexAvatar / 2512.15599 | **VERIFIED** | 20 FPS, 1.7 GB, two minutes, June SHeaP update, and CC BY-NC are real; fitting also has 1/7/10-minute modes. |
| MATCH / 2603.15811 | **PARTIAL** | 0.5 s is per static registration; complete animatable avatar is 4.6 h, not 0.5 s. |
| GAF / 2412.10209 | **VERIFIED** | 12 h/32 GB and 62 FPS are real; official source is still absent. |
| OFERA | **PARTIAL** | IEEE VR/TVCG venue and RTX4090+Quest Link inference are real; training remains TBD. |
| FiCA | **PARTIAL** | “Within five seconds” is published; no code or weights found. |
| URAvatar | **PARTIAL** | Paper and phone personalization are real; no public implementation found. |
| Apple HeadsUp | **PARTIAL** | Paper/project repo are real, but the repository is a website/media tree, not code. |
| SqueezeMe | **VERIFIED** | Three 60k avatars at 72 FPS on Quest 3 is genuine; 0.45 ms is decoder-only; no code release. |
| HRM2Avatar | **VERIFIED** | A real Apache-2.0 Metal runtime tree and 533k/90 FPS Vision Pro result exist; reconstruction code is absent. |
| RGBAvatar | **VERIFIED** | 81 s, 398 FPS, and 20 bases are real; the DDE live tracker is not released. |
| Ava-256 | **VERIFIED** | Data, universal training code, models, and HMC/dome assets are released under CC BY-NC 4.0. |
| LAM_Audio2Expression | **PARTIAL** | Official release is Apache PyTorch/CUDA; 192 MB ONNX and CPU claims come from different third-party conversions. |
| Universal Facial Encoding | **PARTIAL** | Real SIGGRAPH/TOG system with lightweight calibration; no public runtime found. |
| `3dgs-cpp` | **PARTIAL** | Vulkan-compute and Apple support are real; Android/Qualcomm/OpenXR remain TODO in the pinned clone. |
| `vkgs` | **PARTIAL** | Fast Vulkan desktop renderer; graphics-pipeline splatting, no Adreno/stereo evidence, not actively maintained. |
| 40–60k stereo at 72 Hz | **VERIFIED class feasibility** | Demonstrated by SqueezeMe on Quest 3/XR2 Gen 2, but only in an unreleased custom stack. |
| Open Mura XR2+ path | **UNVERIFIED** | No public audited stack demonstrates Linux/Vulkan+Adreno+stereo+animation under compositor load. |
| 90 Hz on XR2+ | **UNVERIFIED** | Demonstrated on Apple M2, not on XR2+ Gen 2; requires an on-device prototype. |

# Architecture recommendation

Use a portable Persona asset boundary: canonical Gaussian attributes, explicit rig/control channels, and separable decoder and renderer interfaces.
Treat enrollment as an off-device asynchronous service; MATCH, FlexAvatar, RGBAvatar, FiCA, and similar methods can be swapped without changing the runtime contract.
For the headset path, prototype a SqueezeMe-like compact linear/blendshape decoder plus a purpose-built Adreno renderer rather than porting a large research decoder unchanged.
Budget and measure decoder, animation, projection/sort, raster, stereo, compositor, and thermal costs independently.
Target 40–60k visible animated splats at 72 Hz first.
Do not claim 90 Hz, native eye resolution, or sustained operation until the exact XR2+ Gen 2 BSP passes the on-headset benchmark described above.
