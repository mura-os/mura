# Perception Claims Audit

**Audit date:** 2026-09-22  
**Scope:** Qualcomm depth hardware, Linux inference paths, and the cited 2024–2026 perception/NVS literature.  
**Standard:** a claim is not treated as an architecture dependency merely because a paper, product brief, or extension exists.

## Verdict definitions

- **VERIFIED** — the cited artifact exists and the material claim is supported by a primary or authoritative source.
- **PARTIAL** — the artifact exists, but the claim overstates availability, scope, release status, title, or applicability.
- **UNVERIFIED** — the available evidence does not establish the claim.
- **FABRICATED** — the named artifact does not exist or an authoritative source directly contradicts it.

## Executive finding

The safest XR2+ Gen 2 Linux architecture is still a portable classical GPU stereo backend plus an optional learned backend.
Qualcomm really does describe an Adreno depth-from-stereo implementation running “well under 1 millisecond” in many cases, but the article exposes no public API, SDK, sample, or XR2-specific support statement ([Qualcomm DFS blog](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
The Qualcomm Vulkan block-match extensions are real, but no public source found in this audit proves that the XR2+ Gen 2 BSP's Adreno 7xx-class driver advertises them ([Vulkan `image_processing2`](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing2.html), [VVL device evidence](https://github.com/KhronosGroup/Vulkan-ValidationLayers/issues/11339)).
Linux HTP inference is also real and documented, including OpenEmbedded targets and serialized QNN contexts, but practical use on a headset depends on OEM/BSP-delivered QNN libraries, matching firmware, RPC services, permissions, and licensing ([Qualcomm Linux HTP tutorial](https://docs.qualcomm.com/doc/80-63442-10/topic/qnn_tutorial_linux_host_linux_target_htp.html)).

# Part 1 — Qualcomm hardware-depth path

## 1. Adreno GPU Depth-from-Stereo / Adreno Motion Engine

**Verdict: PARTIAL**

### What Qualcomm actually says

Qualcomm published **“Low Latency Depth From Stereo With Qualcomm Adreno GPU”** on 2024-09-06 ([primary article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
The article says the solution is “built into the latest Adreno Motion Engine,” is GPU-based, and targets high-performance, low-latency, low-power depth generation ([Qualcomm article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
It says the algorithm executes “in many use cases in well under 1 millisecond”; it does **not** claim a universal worst-case bound, specify resolution, disparity range, quality level, or test SoC ([Qualcomm article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
It also says GPU placement lets DFS run back-to-back with rendering, making the handoff latency “nearly zero”; that wording concerns pipeline adjacency, not total camera-to-display latency ([Qualcomm article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
The use case described is camera-to-eye reprojection: depth allows camera images to be reprojected toward the eye viewpoints to reduce geometric distortion during head motion ([Qualcomm article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).

### Public API or OEM/internal component?

The public article contains no API names, headers, downloadable SDK, sample code, Vulkan extension mapping, or integration instructions ([Qualcomm article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
Qualcomm's public Adreno developer guide lists general Vulkan and Adreno development material but does not identify a DFS API package ([Adreno developer guide](https://docs.qualcomm.com/bundle/publicresource/topics/80-78185-2/gpu.html)).
The defensible conclusion is therefore: **the implementation exists, but public developer access is unverified**.
It should be treated as an OEM/BSP capability until Qualcomm or the headset vendor supplies an interface contract.

### Does it apply to XR2 Gen 2 or XR2+ Gen 2?

The article says “latest Adreno Motion Engine” but names no Snapdragon SKU ([Qualcomm article](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).
Qualcomm's XR2+ Gen 2 brief confirms an Adreno GPU, depth estimation, a dedicated computer-vision block, and 12-camera concurrency, but it does not identify Adreno DFS or promise the sub-millisecond implementation to applications ([XR2+ Gen 2 brief](https://docs.qualcomm.com/doc/87-73622-1/87-73622-1_REV_A_Snapdragon_XR2__Gen_2_Platform_Product_Brief.pdf)).
Qualcomm does not publish an exact Adreno model number in that brief; third-party analysis identifies XR2 Gen 2 as SXR2230P with an Adreno 740-family GPU, and XR2+ as a higher-clocked family member ([TechInsights SXR2230P analysis](https://www.techinsights.com/blog/qualcomm-snapdragon-xr2-gen-2-ai-integration-digital-floorplan-analysis), [UploadVR XR2+ comparison](https://www.uploadvr.com/qualcomm-snapdragon-xr2-plus-gen-2/)).
That makes compatibility plausible, not established.

### Architecture consequence

Do not make Adreno DFS a required backend.
Define a capability-gated vendor backend that can be enabled only after BSP inspection finds an actual library, service, ioctl, Vulkan path, or vendor interface.
Require measured output format, calibration assumptions, disparity/depth range, confidence semantics, resolution, latency, power, synchronization, and redistribution rights before promoting it beyond “optional.”

## 2. `VK_QCOM_image_processing2` and `VK_QCOM_image_processing3`

### `VK_QCOM_image_processing2`

**Verdict: VERIFIED for extension existence and operations; UNVERIFIED on XR2+ Gen 2**

`VK_QCOM_image_processing2` is an official Vulkan device extension, revision 1, dated 2023-03-10 ([Khronos specification](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing2.html)).
It depends on `VK_QCOM_image_processing` and enables the `TextureBlockMatch2QCOM` SPIR-V capability ([Khronos specification](https://github.khronos.org/Vulkan-Site/refpages/latest/refpages/source/VK_QCOM_image_processing2.html)).
It adds four single-component image operations ([Khronos appendix](https://github.com/KhronosGroup/Vulkan-Docs/blob/main/appendices/VK_QCOM_image_processing2.adoc)):

- windowed sum of absolute differences: `OpImageBlockMatchWindowSADQCOM`;
- windowed sum of squared differences: `OpImageBlockMatchWindowSSDQCOM`;
- four-offset gathered SAD: `OpImageBlockMatchGatherSADQCOM`;
- four-offset gathered SSD: `OpImageBlockMatchGatherSSDQCOM`.

The window operations repeat block comparisons over a 2-D search window and return the selected minimum or maximum error ([Khronos specification](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing2.html)).
The gather operations calculate four offsets and return the four metrics in XYZW ([SPIR-V extension](https://github.khronos.org/SPIRV-Registry/extensions/QCOM/SPV_QCOM_image_processing2.html)).
Applications configure window extent and comparison mode through `VkSamplerBlockMatchWindowCreateInfoQCOM` and query `maxBlockMatchWindow` through `VkPhysicalDeviceImageProcessing2PropertiesQCOM` ([sampler structure](https://github.khronos.org/Vulkan-Site/refpages/latest/refpages/source/VkSamplerBlockMatchWindowCreateInfoQCOM.html), [properties structure](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceImageProcessing2PropertiesQCOM.html)).
Support must be queried through both extension enumeration and `VkPhysicalDeviceImageProcessing2FeaturesQCOM::textureBlockMatch2` ([feature structure](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceImageProcessing2FeaturesQCOM.html)).

These operations directly accelerate stereo correspondence building blocks: local SAD/SSD cost construction, window search, and batched neighboring comparisons.
They do not provide rectification, cost-volume regularization, semi-global matching, occlusion handling, confidence estimation, temporal filtering, or depth conversion; those remain application work.

### `VK_QCOM_image_processing3`

**Verdict: VERIFIED for extension existence and operations; UNVERIFIED on XR2+ Gen 2**

`VK_QCOM_image_processing3` entered the Vulkan specification on 2026-05-08 ([Khronos change](https://github.com/KhronosGroup/Vulkan-Docs/commit/33f0e685493860e6d7a99c3b669be4c1b63d0fed), [extension page](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing3.html)).
Its main addition is `OpImageGatherQCOM`, with horizontal, vertical, diagonal/cardinal, and four-in-a-row gather modes intended for filters, sharpening, upscaling, and vectorized loads ([Khronos proposal](https://github.khronos.org/Vulkan-Site/features/latest/features/proposals/VK_QCOM_image_processing3.html)).
It also improves the earlier block-match family with more format exposure and wrap-mode support, including `VK_FORMAT_FEATURE_2_BLOCK_MATCHING_SXD_BIT_QCOM` and optional clamp-to-edge support ([Khronos proposal](https://github.khronos.org/Vulkan-Site/features/latest/features/proposals/VK_QCOM_image_processing3.html)).
Applications query `imageGatherLinear`, `imageGatherExtendedModes`, and `blockMatchExtendedClampToEdge` separately ([feature structure](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceImageProcessing3FeaturesQCOM.html)).
It can help stereo pre/post filters and memory access, but it is not itself a complete correspondence engine.

### Which Adreno and driver versions?

No public Qualcomm support matrix found by this audit maps either extension to XR2 Gen 2, XR2+ Gen 2, SXR2230P, an Adreno driver build, or an OpenEmbedded BSP release.
A 2026 Vulkan Validation Layers issue reports testing `VK_QCOM_image_processing`/`2` on SM8750 and SM8850 Adreno 8xx devices and explicitly gives “Adreno a7x” as an example without `VK_QCOM_IMAGE_PROCESSING` support ([VVL issue](https://github.com/KhronosGroup/Vulkan-ValidationLayers/issues/11339)).
That evidence is not a formal product matrix, but it is strong reason **not** to assume XR2+'s 7xx-class driver exposes the extensions.
`image_processing3` was standardized only in May 2026, and its registry entry does not name a shipping driver ([Khronos extension page](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing3.html)).

### Required probe

On the real BSP, record:

1. `vkEnumerateDeviceExtensionProperties` output;
2. `VkPhysicalDeviceImageProcessingFeaturesQCOM`;
3. `VkPhysicalDeviceImageProcessing2FeaturesQCOM::textureBlockMatch2`;
4. `VkPhysicalDeviceImageProcessing3FeaturesQCOM`;
5. supported block-match format bits and maximum block/window sizes;
6. a correctness and throughput microbenchmark using the actual camera formats.

Until those probes pass, the QCOM Vulkan path is an optional optimization, not the baseline.

## 3. Hexagon/HTP inference on Linux

**Verdict: VERIFIED for supported Qualcomm embedded-Linux workflows; PARTIAL for an arbitrary XR2+ headset BSP**

### ExecuTorch path

ExecuTorch documents a Qualcomm build option named `--enable_linux_embedded` ([ExecuTorch Qualcomm backend](https://docs.pytorch.org/executorch/stable/backends-qualcomm.html)).
Its supported-SoC list includes SXR1230P as “Linux Embedded” and also lists SXR2230P/SXR2330P, although only the SXR1230P example is explicitly shown with an OpenEmbedded toolchain ([ExecuTorch 1.2 documentation](https://docs.pytorch.org/executorch/1.2/backends-qualcomm.html)).
The documented example builds `build-oe-linux` and runs a DeepLab model for SXR1230P with `aarch64-oe-linux-gcc-9.3` ([ExecuTorch backend documentation](https://docs.pytorch.org/executorch/stable/backends-qualcomm.html)).
ExecuTorch exports a Qualcomm-lowered `.pte`; its backend code aligns and embeds the QNN context in that program rather than requiring applications to manage a separate model graph at runtime ([ExecuTorch export utility](https://github.com/pytorch/executorch/blob/main/backends/qualcomm/export_utils.py)).
The runtime still needs compatible QNN libraries and firmware, and ExecuTorch warns that compiling a context with one QNN version and loading it with another can fail ([ExecuTorch troubleshooting](https://docs.pytorch.org/executorch/stable/backends-qualcomm.html)).

### Native QNN/QAIRT path

Qualcomm documents Linux host to Linux target model builds for OpenEmbedded GCC 8.2, 9.3, and 11.2, plus AArch64 Ubuntu targets ([QAIRT target tutorial](https://docs.qualcomm.com/doc/80-63442-10/topic/qnn_tutorial_linux_host_linux_target.html)).
For HTP, Qualcomm says the target requires a quantized model and a serialized context ([Linux HTP tutorial](https://docs.qualcomm.com/doc/80-63442-10/topic/qnn_tutorial_linux_host_linux_target_htp.html)).
The documented flow creates the context with `qnn-context-binary-generator`, transfers `libQnnHtp.so`, the matching CPU-side `libQnnHtpV##Stub.so`, DSP-side `libQnnHtpV##Skel.so`, support libraries, and the serialized binary, then runs `qnn-net-run --retrieve_context` ([Linux HTP tutorial](https://docs.qualcomm.com/doc/80-63442-10/topic/qnn_tutorial_linux_host_linux_target_htp.html)).
Qualcomm's Linux deployment guide explicitly lists `aarch64-oe-linux-gcc11.2` artifacts including `libQnnHtp.so`, `libQnnHtpPrepare.so`, Stub, Skel, `libQnnSystem.so`, and `ADSP_LIBRARY_PATH` setup ([deployment guide](https://docs.qualcomm.com/doc/80-70015-15B/topic/qnn-run-model.html)).
`libQnnHtpPrepare.so` is needed when graph validation/composition/finalization occurs on target, but a valid pre-serialized context can avoid that preparation path ([backend reference](https://docs.qualcomm.com/bundle/publicresource/topics/80-63442-10/backend.html)).

### Realistic XR2+ access story

The public documentation proves that Qualcomm supports on-device HTP execution on selected embedded-Linux products.
It does **not** prove that a particular XR2+ Gen 2 headset exposes the matching QNN runtime, unsigned/signed Skel accepted by firmware, FastRPC devices, memory heaps, DSP domains, power controls, or redistribution rights to a custom OS image.
The practical route is an OEM/developer BSP containing a mutually matched kernel, DSP firmware, QAIRT/QNN target libraries, RFSA/firmware files, and toolchain.
Copying generic `.so` files onto an unrelated Linux image is not sufficient because Stub/Skel, firmware, SoC architecture, and generated context must agree ([Qualcomm backend reference](https://docs.qualcomm.com/bundle/publicresource/topics/80-63442-10/backend.html), [ExecuTorch version warning](https://docs.pytorch.org/executorch/stable/backends-qualcomm.html)).

Therefore learned HTP stereo is **architecturally credible but BSP-gated**.
Before committing to it, compile a small quantized stereo-shaped graph, confirm all operators remain on HTP, measure host/HTP copies, and verify sustained thermal performance.

## 4. XR2+ Gen 2 “12 ms video see-through”

**Verdict: VERIFIED as a Qualcomm platform claim, not as an independent end-to-end guarantee**

Qualcomm's XR2+ Gen 2 product brief states that the upgraded ISP and full-color video see-through deliver “ultra-fast 12ms” latency ([official product brief](https://docs.qualcomm.com/doc/87-73622-1/87-73622-1_REV_A_Snapdragon_XR2__Gen_2_Platform_Product_Brief.pdf)).
The same brief lists two image front ends capable of 12 MP at 90 FPS for video see-through and 12 concurrent cameras ([official product brief](https://docs.qualcomm.com/doc/87-73622-1/87-73622-1_REV_A_Snapdragon_XR2__Gen_2_Platform_Product_Brief.pdf)).
Qualcomm does not define the measurement endpoints, camera exposure assumptions, display scan position, reprojection mode, resolution, or image-processing configuration in that brief.
The brief also says results vary by OEM implementation and other factors ([official product brief](https://docs.qualcomm.com/doc/87-73622-1/87-73622-1_REV_A_Snapdragon_XR2__Gen_2_Platform_Product_Brief.pdf)).
Accordingly, “12 ms” is evidence of platform capability/targeting, not a guaranteed photon-to-photon latency for spatial-os.

## Part 1 conclusion — realistic XR2+ Gen 2 Linux backends

### Backend A — portable classical GPU stereo

**Available in principle and the correct baseline.**
Implement rectification, census/gradient features, SAD/SSD or another cost, aggregation, winner selection, subpixel refinement, confidence, and temporal filtering in ordinary Vulkan compute.
This path depends only on standard Vulkan compute and actual camera access, not a private Qualcomm component.
Its feasibility still requires on-device profiling, but its API availability can be established directly from the BSP.

### Backend B — `VK_QCOM`-accelerated classical stereo

**Potentially valuable, currently unverified on XR2+.**
Use `VK_QCOM_image_processing2` block-match operations when runtime probes expose them ([Khronos extension](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing2.html)).
Use `image_processing3` gather/format improvements only when separately advertised ([Khronos extension](https://docs.vulkan.org/refpages/latest/refpages/source/VK_QCOM_image_processing3.html)).
Keep identical fallback stages in ordinary compute because public evidence suggests Adreno 7xx drivers may omit the prerequisite extension ([VVL issue](https://github.com/KhronosGroup/Vulkan-ValidationLayers/issues/11339)).

### Backend C — Qualcomm Adreno DFS

**Real component, availability unverified.**
Treat it as a vendor plugin negotiated with Qualcomm/OEM.
Do not infer access from the blog or from the presence of an Adreno GPU ([Qualcomm DFS blog](https://www.qualcomm.com/developer/blog/2024/09/qualcomm-gpu-depth-from-stereo)).

### Backend D — Hexagon HTP learned stereo

**Documented on embedded Linux, but integration is BSP- and model-gated.**
Support either native QNN serialized contexts or ExecuTorch `.pte` deployment ([Qualcomm Linux HTP tutorial](https://docs.qualcomm.com/doc/80-63442-10/topic/qnn_tutorial_linux_host_linux_target_htp.html), [ExecuTorch backend](https://docs.pytorch.org/executorch/stable/backends-qualcomm.html)).
Plan for INT8 quantization, static-ish shapes, operator partition inspection, memory-transfer accounting, and strict QNN/firmware version matching.

### BSP questions that must be answered on hardware

- Exact SoC ID, Adreno driver build, Vulkan extension list, and QCOM feature/property bits.
- Whether camera buffers can be imported zero-copy into Vulkan and shared with display/compositor paths.
- Whether any Adreno Motion Engine/DFS library or service is present, documented, and redistributable.
- QNN/QAIRT version, HTP architecture number, Stub/Skel files, FastRPC nodes, DSP firmware, and signing policy.
- Whether SXR2230P is supported as Linux Embedded by the delivered ExecuTorch/QNN combination, not merely listed as a supported SoC.
- Camera calibration, synchronization, rolling-shutter timing, exposure latency, ISP stages, and the actual endpoints of the “12 ms” mode.
- Sustained depth throughput and power while compositor, reprojection, hand tracking, and application rendering run concurrently.

# Part 2 — 2026 preprint and paper audit

## Verdict table

| Item | Verdict | What is actually verified | Relevance |
|---|---|---|---|
| BANet, arXiv:2503.03259 | **VERIFIED** | ICCV 2025 paper; BANet-2D reports 45 ms at 512×512 on Snapdragon 8 Gen 3 ([paper](https://arxiv.org/html/2503.03259v2), [ICCV version](https://openaccess.thecvf.com/content/ICCV2025/papers/Xu_BANet_Bilateral_Aggregation_Network_for_Mobile_Stereo_Matching_ICCV_2025_paper.pdf)). | Direct mobile stereo candidate. |
| Lite Any Stereo V2 / LAS2, arXiv:2606.24457 | **VERIFIED** | Real 2026 preprint and MIT repository; Orin NX 8G timings include 81/101/166 ms for S/M/L, with benchmark conditions in the paper ([paper](https://arxiv.org/html/2606.24457), [repo](https://github.com/TomTomTommi/LiteAnyStereo)). | Strong zero-shot depth candidate, but not XR2-real-time evidence. |
| Fast-FoundationStereo, arXiv:2512.11130 | **VERIFIED** | CVPR 2026 paper and NVIDIA code; RTX 3090 TensorRT timings are 14.0–23.4 ms at 640×480 depending on model/iterations ([CVPR](https://openaccess.thecvf.com/content/CVPR2026/html/Wen_Fast-FoundationStereo_Real-Time_Zero-Shot_Stereo_Matching_CVPR_2026_paper.html), [repo](https://github.com/NVlabs/Fast-FoundationStereo)). | Accuracy/reference candidate; likely heavy for XR2. |
| 2-D Hilbert-curve depth, arXiv:2405.14024 | **VERIFIED** | ICML 2025 paper; W8A8 stereo depth on Snapdragon 8 Gen 3/Hexagon, up to three effective bits recovered and up to 4.6× quantization-error reduction ([arXiv](https://arxiv.org/abs/2405.14024), [ICML record](https://dblp.dagstuhl.de/rec/conf/icml/UssYSKSSYJJ25.html)). | Useful quantization technique, not a standalone depth model. |
| Stereo Matching in Time / XR-Stereo, arXiv:2309.04183 | **PARTIAL** | WACV 2024 paper reports 134 FPS desktop and 30 FPS on Qualcomm XR2 using ONNX floating point ([paper](https://doi.org/10.48550/arxiv.2309.04183)); public repo clearly releases the dataset, not model implementation ([repo](https://github.com/za-cheng/XR-Stereo)). | Highly relevant evidence; reproducibility of the network is limited. |
| TC-Stereo, arXiv:2407.11950 | **VERIFIED** | ECCV 2024 paper and MIT code implement temporal disparity completion plus dual-space refinement ([Springer](https://link.springer.com/chapter/10.1007/978-3-031-72751-1_20), [repo](https://github.com/jiaxiZeng/Temporally-Consistent-Stereo-Matching)). | Directly relevant temporal depth reference; already cloned. |
| NeuralPassthrough, arXiv:2207.02186 | **VERIFIED** | SIGGRAPH 2022 paper and archived MIT Python/model/data release exist; repo warns it predates the optimized C++ runtime ([paper](https://doi.org/10.48550/arxiv.2207.02186), [repo](https://github.com/facebookresearch/NeuralPassthrough)). | Direct passthrough reference; already cloned. |
| Passthrough+ (2020) | **VERIFIED** | ACM paper and PDF exist; Quest/Snapdragon 835 system reports 72 Hz, about 200 mW, and 49 ms photon-to-texture / 62 ms photon-to-geometry ([PDF](https://alexandruichim.com/pdf/Chaurasia_Passthrough_CGIT20.pdf), [DOI](https://doi.org/10.1145/3384540)). | Direct systems precedent, but implementation is not released. |
| REON-NVS, ECCV 2026 | **VERIFIED** | “Real-Time Online Novel-View Synthesis from Sparse-View Videos” appears in author and ECCV records ([author record](https://scho.postech.ac.kr/research), [ECCV accepted papers](https://eccv.ecva.net/Conferences/2026/AcceptedPapers)). | NVS research, not a drop-in passthrough depth backend. |
| StereoSplat+, arXiv:2607.08808 | **VERIFIED** | Feed-forward stereo 3DGS plus diffusion-assisted progressive inference from a stereo pair ([arXiv](https://arxiv.org/abs/2607.08808)). No official code was found. | Tangential and too reconstruction-heavy for the primary passthrough loop. |
| StreamSplat, arXiv:2608.01659 | **VERIFIED** | Streaming feed-forward 3DGS with a voxel-aligned causal cache; code is promised upon acceptance, not currently linked ([arXiv](https://arxiv.org/abs/2608.01659)). | Tangential persistent-scene research. |
| Mobile-GS, arXiv:2603.11531 | **VERIFIED** | ICLR 2026 mobile 3DGS renderer; reports 116 FPS at 1600×1063 on Snapdragon 8 Gen 3, but the repo says mobile Vulkan code cannot be released ([OpenReview](https://openreview.net/forum?id=vRegY0pgvQ), [project](https://xiaobiaodu.github.io/mobile-gs-project/), [repo](https://github.com/xiaobiaodu/mobile-gs)). | Rendering reference, not stereo depth; key mobile code unavailable. |
| Flux-GS, arXiv:2606.30017 | **VERIFIED** | ECCV 2026 “Monte Carlo Energy Aggregation for Mobile 3D Gaussian Splatting”; Apache-2.0 code and WebGL renderer exist ([paper](https://arxiv.org/abs/2606.30017), [repo](https://github.com/xiaobiaodu/Flux-GS)). | Useful future mobile scene rendering reference, not the depth backend. |
| LiteMatch, arXiv:2606.31636 | **PARTIAL** | ECCV 2026 lightweight zero-shot stereo paper exists and reports 3.36M–9.58M variants ([Springer](https://link.springer.com/chapter/10.1007/978-3-032-37447-9_18), [arXiv](https://arxiv.org/abs/2606.31636)); no cloneable licensed repository was found. | Promising depth design; wait for code/license. |
| LagerNVS, arXiv:2603.20176 | **VERIFIED** | CVPR 2026 feed-forward NVS; reports 31.4 PSNR on RealEstate10K and real-time 512×512 rendering on high-end GPU hardware ([CVPR](https://openaccess.thecvf.com/content/CVPR2026/html/Szymanowicz_LagerNVS_Latent_Geometry_for_Fully_Neural_Real-time_Novel_View_Synthesis_CVPR_2026_paper.html), [repo](https://github.com/facebookresearch/lagernvs)). | Tangential and far heavier than an XR2 depth stage. |
| LiveStre4m, arXiv:2604.06740 | **VERIFIED** | CVPR 2026 workshop paper; unposed multi-view NVS reports 0.07 s/frame at 1024×768 and releases code with a non-standard/unclear license ([CVPRW](https://openaccess.thecvf.com/content/CVPR2026W/3DMV/html/Quesado_LiveStre4m_Feed-Forward_Live_Streaming_of_Novel_Views_from_Unposed_Multi-View_CVPRW_2026_paper.html), [repo](https://github.com/pedro-quesado/LiveStre4m)). | Tangential dynamic NVS, not headset passthrough depth. |
| Camsicle / arXiv:2603.15796 | **VERIFIED** | “Perceptual Requirements for Low-Latency Head-Mounted Displays” introduces a 2 ms catadioptric VST HMD; 57 participants preferred 2/14.3 ms over 23/29 ms in ball catching ([arXiv](https://arxiv.org/abs/2603.15796)). | Highly relevant latency requirement evidence, not an implementation backend. |
| VoroTracing, arXiv:2608.17682 | **VERIFIED** | Zenseact differentiable Voronoi ray tracer reports 623 FPS on RTX 5090 and releases source ([paper](https://arxiv.org/html/2608.17682), [repo](https://github.com/zenseact/VoroTracing)). | Interesting renderer, but desktop CUDA NVS is not an XR2 depth path. |
| On-device stereo matching using NPU acceleration, DOI 10.1117/12.3102515 | **VERIFIED** | IWAIT 2026 paper runs NPU-only stereo on Snapdragon 8 Elite; PTQ/layout optimization reduces NPU latency about 33% at 960×540, with tiling for higher resolution ([institutional record](https://pure.skku.edu/en/publications/on-device-stereo-matching-using-npu-acceleration-for-real-time-de/), [proceedings contents](https://www.proceedings.com/content/085/085295webtoc.pdf)). | Strong NPU feasibility evidence, but not XR2 Linux code. |
| “The Perceptual Cost of Passthrough,” arXiv:2601.02805 | **PARTIAL** | The ID is real, but the title is **“The perceptual gap between video see-through displays and natural human vision”** ([arXiv](https://arxiv.org/abs/2601.02805)). The Unity benchmark repo exists ([repo](https://github.com/Chaosikaros/VST-Visual-Perception-Benchmark)). | Relevant acceptance/quality benchmark; the reported title was wrong. |

## Short per-item notes

### BANet

The 45 ms figure is specifically BANet-2D at 512×512 on Snapdragon 8 Gen 3, broken down as 16 ms feature extraction, 6.5 ms correlation, and 22.5 ms aggregation ([paper](https://arxiv.org/html/2503.03259v2)).
It is not a claim of 45 ms on XR2, Linux, Vulkan, or HTP.
The official repository is MIT-licensed ([BANet repo](https://github.com/gangweix/BANet)).

### Lite Any Stereo V2

LAS2 is real and the official MIT repository contains LAS1 and LAS2 S/M/L/H inference code and checkpoints ([repo](https://github.com/TomTomTommi/LiteAnyStereo)).
Reported Orin timings are not XR2 timings and use NVIDIA CUDA; LAS2-M's 101 ms should not be called headset real-time without a Qualcomm port ([paper](https://arxiv.org/html/2606.24457)).
The paper's strongest defensible headline is efficient zero-shot generalization, not demonstrated XR2 deployment.

### Fast-FoundationStereo

Fast-FoundationStereo is genuinely CVPR 2026 and over 10× faster than FoundationStereo in the authors' comparison ([project](https://nvlabs.github.io/Fast-FoundationStereo/)).
Its code uses NVIDIA's source-code license with a research-only, non-commercial use limitation ([license](https://github.com/NVlabs/Fast-FoundationStereo/blob/master/LICENSE.txt)).
The released TensorRT path is NVIDIA-specific, so it is an accuracy/reference clone rather than evidence for Qualcomm deployment.

### Hilbert-curve output representation

The final ICML version evaluates SNPE 2.24 on a Samsung S24+ with Snapdragon 8 Gen 3/Hexagon and reports W8A8 depth quality comparable to or better than W8A16 in tested cases ([paper](https://arxiv.org/pdf/2405.14024v2)).
The technique modifies output representation and adds a small lookup-table reconstruction; it does not replace the stereo network.
No official implementation repository was found.

### XR-Stereo

The 30 FPS XR2 result is in the paper and uses the fast model converted through ONNX to a device-friendly floating-point format without quantization ([paper](https://doi.org/10.48550/arxiv.2309.04183)).
The public repository releases a 57.4 GB, 640×480 dataset under CC BY 4.0; the roughly 4 TB full dataset requires author contact ([repo](https://github.com/za-cheng/XR-Stereo)).
Calling the repository a release of the model code is unsupported.

### TC-Stereo

TC-Stereo is a real temporal stereo model and the MIT code is released ([repo](https://github.com/jiaxiZeng/Temporally-Consistent-Stereo-Matching)).
It is relevant as an algorithmic reference for temporal state, but the cited sources provide no Qualcomm deployment result.

### NeuralPassthrough and Passthrough+

NeuralPassthrough is a learned end-to-end desktop-connected system, and its public Python implementation is explicitly prior to the customized real-time C++ optimization ([repo](https://github.com/facebookresearch/NeuralPassthrough)).
Passthrough+ is the stronger mobile systems precedent: it used video-encoder motion vectors, coarse geometry, densification, warping, and temporal upsampling under a very small Snapdragon 835 budget ([paper](https://doi.org/10.1145/3384540)).
Its reported 49/62 ms latencies are valuable baselines, not modern targets ([PDF](https://alexandruichim.com/pdf/Chaurasia_Passthrough_CGIT20.pdf)).

### 2026 NVS cluster

REON-NVS, StereoSplat+, both StreamSplat papers, Mobile-GS, Flux-GS, LagerNVS, LiveStre4m, and VoroTracing are genuine works, not hallucinated names.
They solve scene representation or novel-view rendering problems with substantially different latency, training, state, and hardware assumptions from per-frame camera-to-eye depth.
They should inform future persistent-scene or renderer experiments, but they do not justify replacing a low-latency stereo/reprojection path.
The two StreamSplat names must not be conflated: arXiv:2608.01659 is calibrated streaming feed-forward 3DGS, while the ICLR 2026 work is arXiv:2506.08862 on uncalibrated dynamic streams ([2608 paper](https://arxiv.org/abs/2608.01659), [ICLR paper](https://arxiv.org/abs/2506.08862v2)).

### LiteMatch

LiteMatch's paper and ECCV publication are real ([ECCV record](https://eccv.ecva.net/virtual/2026/poster/4533)).
The project page does not currently expose a verifiable clone URL or software license ([project page](https://mdraqibkhan.github.io/Litematch/)).
Track it, but do not put it in an automated clone manifest yet.

### Camsicle and perception benchmark

Camsicle provides direct evidence that reducing VST latency below common 12–60 ms systems remains perceptually useful; 12 ms should not be treated as “already imperceptible” ([Camsicle paper](https://arxiv.org/abs/2603.15796)).
The arXiv:2601.02805 study compares Vision Pro, Quest 3, and Quest Pro with naked-eye acuity, contrast, and color tasks under normal and low light, finding a remaining perceptual gap ([paper](https://arxiv.org/html/2601.02805)).
These are requirements/validation sources, not depth algorithms.

# Part 3 — Tier 2 clone recommendations

## Clone now

### 1. Lite Any Stereo

- **Repository:** [TomTomTommi/LiteAnyStereo](https://github.com/TomTomTommi/LiteAnyStereo)
- **License:** MIT ([repository metadata and license](https://github.com/TomTomTommi/LiteAnyStereo))
- **Why:** the most relevant new efficient zero-shot stereo family, with LAS2 S/M/L/H checkpoints and profiling scripts.
- **Expectation:** use for quality/portability experiments; do not assume Orin CUDA timings transfer to XR2 HTP or Vulkan.

### 2. BANet

- **Repository:** [gangweix/BANet](https://github.com/gangweix/BANet)
- **License:** MIT ([repository](https://github.com/gangweix/BANet))
- **Why:** verified mobile-oriented 2-D cost aggregation and the strongest cited Snapdragon timing.
- **Expectation:** inspect exportability and operator coverage; the published run is Snapdragon 8 Gen 3, not XR2+ Linux.

## Clone conditionally

### 3. Fast-FoundationStereo

- **Repository:** [NVlabs/Fast-FoundationStereo](https://github.com/NVlabs/Fast-FoundationStereo)
- **License:** NVIDIA source-code license, research-only/non-commercial ([license](https://github.com/NVlabs/Fast-FoundationStereo/blob/master/LICENSE.txt))
- **Decision:** clone only into a clearly marked research/reference area if the project accepts that restriction.
- **Why:** useful upper-quality and zero-shot reference; not the likely production XR2 backend.

### 4. OpenStereo_DoItOnce

- **Repository:** [joej970/OpenStereo_DoItOnce](https://github.com/joej970/OpenStereo_DoItOnce)
- **License/status:** README says academic use only and prohibits commercial use; no standard permissive license is established ([repository](https://github.com/joej970/OpenStereo_DoItOnce)).
- **Decision:** clone only for isolated evaluation, never as assumed shippable code.
- **Why:** its single-pass paired feature extraction claims 10–39% acceleration across selected OpenStereo models, which is worth benchmarking on the intended compiler/runtime ([repository](https://github.com/joej970/OpenStereo_DoItOnce)).

## Track, but do not clone yet

### LiteMatch

The paper is relevant and compact, but no verifiable software repository/license was found ([project](https://mdraqibkhan.github.io/Litematch/)).
Add a watch item and clone only after a repository and explicit license appear.

### Hilbert-curve depth representation

The technique is useful for an eventual INT8 HTP model, but no official paper implementation was found ([paper](https://arxiv.org/abs/2405.14024)).
Implement only after selecting a base stereo model and confirming that output precision is a measured bottleneck.

### IWAIT NPU stereo

The paper is strong evidence for Qualcomm NPU-only stereo, but no public code repository was identified ([publication record](https://pure.skku.edu/en/publications/on-device-stereo-matching-using-npu-acceleration-for-real-time-de/)).
Use it to guide tiling, PTQ, and layout experiments rather than as a clone target.

## Drop from the depth clone list

- **REON-NVS:** real, but sparse-video online NVS rather than a depth backend ([ECCV evidence](https://scho.postech.ac.kr/research)).
- **StereoSplat+:** real, but diffusion-assisted 3DGS reconstruction with no official code found ([paper](https://arxiv.org/abs/2607.08808)).
- **StreamSplat 2608.01659:** real, but code is only promised upon acceptance and the method is persistent 3DGS ([paper](https://arxiv.org/abs/2608.01659)).
- **StreamSplat 2506.08862:** real ICLR 2026 work, but dynamic uncalibrated 3D reconstruction is outside the depth hot path ([paper](https://arxiv.org/abs/2506.08862v2)).
- **Mobile-GS:** real, but mobile Vulkan code is withheld and the task is rendering an already built Gaussian scene ([repo](https://github.com/xiaobiaodu/mobile-gs)).
- **Flux-GS:** real and Apache-2.0, but clone only if spatial-os starts a mobile Gaussian renderer workstream ([repo](https://github.com/xiaobiaodu/Flux-GS)).
- **LagerNVS:** real, but high-end feed-forward NVS is not an XR2 stereo backend ([CVPR paper](https://openaccess.thecvf.com/content/CVPR2026/html/Szymanowicz_LagerNVS_Latent_Geometry_for_Fully_Neural_Real-time_Novel_View_Synthesis_CVPR_2026_paper.html)).
- **LiveStre4m:** real, but 14 FPS-class multi-view NVS and an unclear code license make it unsuitable here ([paper](https://arxiv.org/abs/2604.06740), [repo](https://github.com/pedro-quesado/LiveStre4m)).
- **VoroTracing:** real and fast on RTX 5090, but a desktop differentiable NVS renderer is not evidence for XR2 feasibility ([paper](https://arxiv.org/html/2608.17682)).
- **VST Visual Perception Benchmark:** useful as a test-design reference, but not a depth repository; verify its software license before vendoring ([repo](https://github.com/Chaosikaros/VST-Visual-Perception-Benchmark)).

## Final architecture recommendation

Build the depth interface around runtime-discovered capabilities:

1. **Required:** ordinary Vulkan-compute classical stereo.
2. **Optional:** QCOM block-match/gather acceleration when the device advertises and passes conformance probes.
3. **Optional vendor:** Adreno DFS only after obtaining an actual BSP API and redistribution terms.
4. **Optional learned:** QNN/ExecuTorch HTP backend only after a BSP smoke test proves model compilation, full/acceptable delegation, zero-copy or bounded-copy I/O, and sustained latency.
5. **Research references:** LAS2 and BANet first; Fast-FoundationStereo and DoItOnce only under their restrictive licensing conditions.

The architecture should not claim sub-millisecond depth, 12 ms photon-to-photon passthrough, QCOM block-match availability, or Linux HTP access on the target headset until each is measured on the actual XR2+ Gen 2 BSP.
