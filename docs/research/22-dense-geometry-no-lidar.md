# 22 — Dense geometry without LiDAR: planes, fused meshes, semantics

**Date:** 2026-09-22. Research pass for Mura. This document covers the SLOW/static world
geometry track: gravity-aligned planes, TSDF-fused meshes, and coarse semantics, built from
camera+IMU only, on Adreno/Hexagon-class mobile XR hardware. Frame-rate *dynamic* occlusion
(passthrough depth, hands) is Tier 1–3's job and is out of scope here
([perception-passthrough-hands](../architecture/perception-passthrough-hands.md)); this is the
geometry that persists across seconds-to-sessions and feeds occlusion, shadows, placement,
collision, and the boundary system. Per [ADR 0008](../architecture/adr/0008-perception-services-placement.md),
the geometry service runs Monado-side on the shared `xrt_frame` fan-out and is **duty-cycled**:
map when the scene is unfamiliar or changed, optionally run heavy refinement while docked.

Primary sources (local clones under `references/`, cited as `file:line`): `voxblox/`,
`vdbfusion/`, `supereight2/`, `nvblox/`, `ov-plane/`, `kimera-semantics/`, `monado/`.
Web claims carry links and **[verified]** / **[unverified]** marks. Established context built on,
not re-derived: the sensor matrix ([07-device-landscape](07-device-landscape.md)), the stereo
depth backends and `DepthFrame` interface ([14-mobile-stereo-depth](14-mobile-stereo-depth.md)),
the T1 depth-tested composition ([zxr-shell-v2-composition](../architecture/zxr-shell-v2-composition.md)),
and the already-committed design direction: gravity-aligned plane priors as v1, multi-view
accumulation over head motion, and a tri-state observed/inferred/unknown map with conservative
occlusion at low confidence.

---

## 1. Purpose

The passthrough pipeline answers "what is in front of the camera *right now*"; this track answers
"what is the *room*". The two differ in every axis that matters: rate (keyframes vs display rate),
latency tolerance (seconds vs milliseconds), accuracy bias (centimeters that stay put vs pixels
that don't swim), and persistence (a map that survives looking away — and ideally a reboot).
Without LiDAR, every input is derived: stereo/multi-view depth at keyframe rate, sparse SLAM
landmarks, IMU gravity, and (per device) IR-assisted variants. The design problem is therefore
not "which reconstruction library" but "which *representation ladder* degrades gracefully as the
depth source gets worse" — and the ladder this document lands on is **planes first, bounded TSDF
mesh second, semantics as labels on both**.

## 2. Consumers and requirements

All consumers live in or beside the zxr compositor's sort-last composition
([composition §2/§7.4](../architecture/zxr-shell-v2-composition.md)): static geometry enters the
**environment layer** exactly like passthrough does — colour is irrelevant here, it contributes
*depth* (and shadow-receiving surfaces) to the T1 nearest-depth resolve.

| Consumer | What it consumes | Quality bar |
|---|---|---|
| **Static occlusion** | per-eye depth from planes+mesh rasterized into the environment layer | wrong-side errors < ~2 cm at 1–2 m at plane/desk edges; **no popping** when the map updates (cross-fade); conservative at low confidence (§7) |
| **Shadow catching** | floor plane first, then table/platform planes, then mesh | floor height error < ~2 cm (a floating/sunken shadow reads instantly as fake); shadows are disproportionately responsible for "physically present" so the *floor plane* is the single highest-value output of this entire track |
| **Placement/snapping** | plane polygons + semantic labels (wall/table/floor) | plane orientation < ~1°, extent good to ~5 cm; stable plane IDs across a session (a window snapped to a wall must not drift when the plane refits) |
| **Collision** | coarse mesh or ESDF-style distance queries | 5–10 cm voxel resolution suffices; recall matters more than precision (missing an obstacle is worse than padding one) |
| **Boundary/Guardian** | floor height + kept-out volumes + distance-to-hazard | the safety consumer: false-negative-intolerant; unknown space is keep-out (§8); breach response must not depend on client cooperation |

Two structural notes. First, the latency asymmetry that lets this be slow: all five consumers
tolerate geometry that is *seconds* old, because the per-eye warp re-derives from the current head
pose every frame — the same three-rate argument as passthrough
([perception §shared architecture](../architecture/perception-passthrough-hands.md)). Second, the
quality bars are *stability*-dominated, not accuracy-dominated: a plane 1 cm off but rock-solid
beats one 2 mm off that refits visibly. Every mechanism below is chosen with that bias.

## 3. TSDF/fusion libraries: code-level comparison

All four repos implement the same core idea — a truncated signed distance field updated by
weighted running average, meshed by marching cubes — and differ in memory model, integration
strategy, and platform. The per-voxel state is essentially universal:

```12:16:references/voxblox/voxblox/include/voxblox/core/voxel.h
struct TsdfVoxel {
  float distance = 0.0f;
  float weight = 0.0f;
  Color color;
};
```

| | **voxblox** | **vdbfusion** | **supereight2** | **nvblox** |
|---|---|---|---|---|
| Platform | CPU (std::thread) | CPU (OpenVDB) | CPU (TBB optional) | **CUDA GPU** |
| Memory model | hash map of 16³-voxel blocks (`Layer<VoxelType>::BlockHashMap`, `layer.h:29-31`) | OpenVDB sparse tree: `tsdf_` + `weights_` FloatGrids (`VDBVolume.h:74-75`) | octree, single- or **multi-resolution** blocks (`setup_util.hpp:40-47`) | GPU block hash on stdgpu (`gpu_layer_view.h:34-49`), layers grouped in a `LayerCake`, unified memory |
| Integration input | pointcloud + pose `T_G_C` (`tsdf_integrator.h:100-103`) — depth images must be back-projected first | pointcloud + origin (`VDBVolume.h:43-45`) | depth image + pose via sensor models (pinhole/Ouster) | **depth image + pose, projective**: blocks-in-view are projected into the depth image instead of raycasting (`projective_tsdf_integrator.h:29-33`, `integrateFrame` `:49-53`; view via frustum or raycast, `view_calculator.h:44-46`) |
| Integration strategies | `simple`/`merged`/`fast` (`tsdf_integrator.h:30-35`): merged bundles rays per start voxel; fast adds approximate-hash early ray termination, "up to an order of magnitude faster" (`tsdf_integrator.h:274-285`) | one DDA ray per point, truncated to `[d−τ, d+τ]` unless space-carving (`VDBVolume.cpp:125-132`); weighted-average update (`VDBVolume.cpp:80-90`) | per-block adaptive scale; explicit free-space integration scale (`data_field.hpp:61`) | one kernel per block batch; weight functions pluggable; TSDF/occupancy **decay** integrators for staleness (`tsdf_decay_integrator.h`) |
| Rates | ~real-time at 5–20 cm voxels on desktop CPU; fast integrator for VGA-rate | offline/near-real-time (KITTI-scale batch) | real-time CPU claims via multires ([Funk et al. 2020](https://arxiv.org/abs/2010.07929)) | **measured**: AGX Orin @ 5 cm: TSDF 0.8 ms, mesh 2.3 ms, ESDF 1.7 ms per frame ([Isaac ROS benchmark](https://nvidia-isaac-ros.github.io/repositories_and_packages/isaac_ros_nvblox/index.html) **[verified]**); paper claims 177× (fusion) / 31× (ESDF) over CPU ([arXiv 2311.00626](https://arxiv.org/abs/2311.00626)) |
| Unknown/free model | weight = 0 ⇒ unknown; ESDF adds `observed` and `hallucinated` flags (`voxel.h:19-28`) — §7 | weight only; no observed/free distinction | occupancy field: log-odds sign + explicit `observed` bool (`data_field.hpp:25-38`) — the cleanest tri-state precedent, §7 | TSDF weight + `EsdfVoxel.observed` (`voxels.h:55-74`) + a dedicated **FreespaceVoxel** layer (dynablox-style high-confidence free space, `voxels.h:38-52`) |
| Incremental meshing | yes: per-block update bits `{kMap, kMesh, kEsdf}` (`block.h:17`), `generateMesh(only_mesh_updated_blocks, clear_updated_flag)` (`mesh_integrator.h:133-139`) | **no** — batch `ExtractTriangleMesh` only (`VDBVolume.h:69-70`) | dual marching cubes across octree scales (`marching_cube.hpp:160-180`) | yes, plus **bandwidth-limited mesh streaming** with radius/height block exclusion (`layer_streamer.h:32-40, 63`) — directly the shape a compositor consumer wants |
| Serialization | protobuf `Layer.pb`/`Block.pb` (`layer.h:10-11`; `LoadLayer` `layer_io.h:47-56`) | standard OpenVDB `.vdb` files | octree/mesh IO (`include/se/map/io/`) | layer/mesh serializers (`serialization/`), GPU-side |
| ROS coupling | none in core; `voxblox_ros` separate | none in core; ROS wrapper separate repo | none | none in core; Isaac ROS wrapper separate |
| License | BSD (`voxblox/LICENSE`) **[verified locally]** | MIT (`vdbfusion/LICENSE`) **[verified locally]** | BSD-3-Clause (`supereight2/README.md:2`) **[verified locally]** | Apache-2.0 (`nvblox/LICENSE.md`) **[verified locally]** |

What to take from each: **nvblox** contributes the *architecture* (projective integration — a
perfect fit for compute shaders since it is a gather over a depth texture, not a scatter; block
hash; decay for staleness; mesh streaming; the measured proof that TSDF+mesh+ESDF is a
~5 ms/frame problem on an embedded GPU). **voxblox** contributes the *incremental meshing
discipline* (per-block dirty bits driving remesh of only touched blocks) and a serialization
precedent. **supereight2** contributes the *tri-state field semantics* (§7) and multiresolution
ideas. **vdbfusion** demonstrates how small the core TSDF math is (~200 lines around
`VDBVolume.cpp:62-150`) — the algorithm is not the hard part.

### The Vulkan-compute-TSDF-on-Adreno gap

nvblox's speed comes from CUDA + stdgpu + unified memory — none of which exists on Adreno. What
exists elsewhere, per web search (2026-09-22): [KinectFusion-Vulkan](https://github.com/YJJfish/KinectFusion-Vulkan)
(a genuine Vulkan-compute KinectFusion, but a **fixed dense volume, no voxel hashing**, research
code) **[verified repo exists]**; [FusionCompute](https://github.com/Kosmonaut3d/FusionCompute)
(OpenGL compute, thesis code); [KinectFusionGPU](https://github.com/netbeifeng/KinectFusionGPU)
(OpenCL, incomplete); GLSL fragment-shader toys. **No production-grade GPU-agnostic sparse TSDF
library exists** — this is a real gap, not a shopping failure.

The gap is smaller than it looks, for one load-bearing reason: **room-scale bounds kill the need
for GPU hashing.** An 8×8×3 m volume at 5 cm is 160×160×60 ≈ 1.5 M voxels; at 8 bytes/voxel
(distance f16 or f32 + weight+state packed) that is ~6–12 MB — a single dense 3D image that fits
comfortably in Adreno-accessible memory, updated in place. At 10 cm it is ~1.5 MB. A
scrolling/recentering clipmap (recenter when the user walks off the volume, drop-or-persist the
scrolled-out slab) covers larger homes without ever paying for a hash table. The port is then:

1. **Integration pass** (compute): nvblox's projective update — for each voxel in the camera
   frustum slab, project into the keyframe `DepthFrame`, compute truncated SDF, gated by the
   depth confidence channel (doc 14 §5), weighted-average update. Bounded volume ⇒ static
   dispatch, no dynamic allocation, NPU-style static shapes by construction.
2. **Meshing pass**: marching cubes over dirty 16³ bricks (voxblox's dirty-bit pattern, GPU
   prefix-sum compaction for vertex output), at ≤ 1 Hz, into a dmabuf vertex buffer the
   compositor imports like any other layer source.
3. **Distance/boundary pass**: either a coarse ESDF propagation (nvblox's is the reference) or —
   cheaper and probably sufficient for §8 — direct distance-to-nearest-occupied queries against
   the dense grid for the handful of boundary probe points (head + controllers).

Budget sanity: the integration pass touches ≤ the frustum subset of 1.5 M voxels at keyframe rate
(1–5 Hz duty-cycled), which is orders of magnitude below the Adreno budget that already absorbs
per-frame passthrough warping; the honest unknown is bandwidth contention with the compositor,
flagged in §11. Conclusion: **a bounded dense-grid Vulkan TSDF is a feasible, moderate
engineering task (not research)**; adopt nvblox as the design donor and skip GPU hashing in v1.

## 4. Planes

### 4.1 Why planes are v1

Planes are the highest value-per-FLOP representation this stack can produce: the floor plane
alone unlocks shadows, placement, and boundary floor height; walls unlock window snapping and
most of the occlusion that matters in a room. They are also exactly where no-LiDAR depth is
weakest-but-priored: the blank-wall stereo failure (doc 14's harness tests "textureless wall
approach" as a named failure scene, [14 §6](14-mobile-stereo-depth.md)) aligns
with the plane prior — where stereo returns nothing is disproportionately where a gravity-aligned
plane hypothesis is *right*, so the plane model both fills the hole and marks it honestly as
inferred (§7). This mirrors shipping behavior: ARCore detects planes as "clusters of feature
points that appear to lie on common horizontal or vertical surfaces" and its own docs warn that
"flat surfaces without texture, such as a white wall, may not be detected properly"
([ARCore fundamentals](https://developers.google.cn/ar/develop/fundamentals) **[verified]**), with
plane-finding modes exactly `HORIZONTAL`/`VERTICAL`/both
([Config.PlaneFindingMode](https://developers.google.com/ar/reference/java/com/google/ar/core/Config.PlaneFindingMode)).
ARKit likewise grows/updates/merges plane anchors over time rather than emitting them once.

### 4.2 v1 design: gravity-aligned RANSAC

Inputs, all already produced on the Monado side: (a) sparse 3D landmarks from the SLAM tracker
(Basalt), whose world frame is gravity-aligned by VIO construction (IMU accelerometer observes
gravity); (b) keyframe-rate depth from the mapping operating point (§6), back-projected and
subsampled; (c) the tracked gravity direction itself.

The gravity prior converts general 3-DoF plane RANSAC into two cheap constrained problems:

- **Horizontal planes** (floor/ceiling/tables): normal fixed to ±g ⇒ **1-DoF** (height). This is
  1D mode-finding on point heights — a histogram, not RANSAC. Floor = lowest well-supported
  upward-facing mode; ceiling = highest downward-facing; platforms = intermediate modes.
- **Vertical planes** (walls): normal ⊥ g ⇒ **2-DoF** (azimuth θ, offset d). RANSAC over
  (θ, d) with point-to-plane inlier tests; minimal sample is 2 points instead of 3, and the
  inlier test is 2D, so it stays robust at the low landmark counts a blank-ish wall yields.

Growth/merge/extent: accumulate inliers over keyframes (multi-view accumulation over head motion
is the committed design), maintain plane extent as a polygon in plane-local coordinates
(inlier hull with concavity, e.g. alpha-shape), merge coplanar detections by normal/offset
gates — the same grow-and-merge lifecycle ARCore/ARKit expose as trackables. When dense keyframe
depth is available, a CAPE-style pass is the upgrade for extraction quality at negligible cost:
grid cells fit local planes, region-grow on a normal histogram, refine boundaries — 640×480 at
~300 Hz on one CPU core ([Proença & Gao, IROS 2018](https://arxiv.org/abs/1803.02380)
**[verified]**), i.e. effectively free at our 1–5 Hz keyframe rate.

**Label target.** Monado already carries the semantic vocabulary as its mirror of
`XR_EXT_plane_detection`: orientations `HORIZONTAL_UPWARD/DOWNWARD`, `VERTICAL`, `ARBITRARY`
(`references/monado/src/xrt/include/xrt/xrt_plane_detector.h:55-61`) and semantic types
`UNDEFINED/CEILING/FLOOR/WALL/PLATFORM` (`xrt_plane_detector.h:68-75`), with polygon output in
the flattened `xrt_plane_detections_ext` layout (`xrt_plane_detector.h:139-197`) and capability
bits for semantics per type (`xrt_plane_detector.h:29-38`). The v1 heuristics (height +
orientation + support) populate FLOOR/WALL/CEILING/PLATFORM directly; §5 upgrades labels beyond
this enum. Publishing through this struct makes the geometry service's plane output consumable
by any OpenXR client via the standard extension for free, in addition to the compositor.

**Learned planes rejected for v1**: PlaneRCNN-class methods (Mask-R-CNN backbone + refinement,
[Liu et al., CVPR 2019](https://openaccess.thecvf.com/content_CVPR_2019/html/Liu_PlaneRCNN_3D_Plane_Detection_and_Reconstruction_From_a_Single_Image_CVPR_2019_paper.html)
**[verified]**) are far over the NPU budget doc 14 established for the *depth* network alone, and
single-image plane hallucination is precisely what the verified-vs-inferred discipline exists to
avoid. Revisit only as an offline/docked refinement teacher.

### 4.3 v2 path: what ov_plane teaches (method, not code)

`references/ov-plane/` (RPNG, **GPL-3.0** — `ov-plane/LICENSE`, method reference only; also
hard-coupled to OpenVINS while Mura tracks Basalt) is a monocular MSCKF VIO that makes
planes *first-class state*. The extractable method, from code:

- **Detection from sparsity**: for tracked features with triangulated 3D positions, run Delaunay
  triangulation (CDT) in the image, compute a normal per triangle from the 3D vertices, average
  and merge normals into plane hypotheses (`ov_plane/src/track_plane/TrackPlane.h:163-170`).
- **Fitting**: linear LSQ `p·n + d = 0` per candidate set (`track_plane/PlaneFitting.h:53-64`),
  RANSAC on point-to-plane distance for inliers (`PlaneFitting.h:77-84`), then joint
  feature+plane refinement with the **closest-point (CP) parametrization** `cp = -d·n`
  (`PlaneFitting.h:87-101`).
- **Tight coupling**: long-lived planes live in the filter state; SLAM features carry a
  feature→plane map so point-on-plane constraints keep regularizing both
  (`ov_plane/src/state/State.h:116`), enabling point-to-plane loop closures.

The v2 lesson is that plane-aided VIO is bidirectional: planes don't just come *out* of tracking,
they stabilize it, and plane IDs become as persistent as the odometry itself. For Mura this
is a future Basalt-side integration (or a Monado plane-tracker module), gated on v1 evidence that
plane *stability* (not detection) is the binding constraint.

## 5. Semantics fusion

`references/kimera-semantics/` (MIT-SPARK, BSD — `kimera-semantics/LICENSE.BSD`) is the canonical
2D-segmentation-fused-to-3D pattern, riding directly on voxblox:

- A parallel voxel layer stores a **per-voxel label probability vector**: 21 labels, log-odds,
  initialized uniform (`kimera_semantics/include/kimera_semantics/semantic_voxel.h:14-27`,
  `common.h:26-29`).
- 2D segmentation arrives as a *colorized pointcloud* (label ↔ color via a CSV map,
  `color.h:44-48`); during TSDF integration each observed voxel's label vector gets a Bayesian
  update — multiply measured label by `p_match` (default 0.9), others by `1−p_match`, normalize
  (`semantic_integrator_base.h:140-146`, `:76-77`) — then the MLE label and its color are cached
  (`src/semantic_integrator_base.cpp:161-169`).
- It reuses voxblox's `merged`/`fast` integrators wholesale (`semantic_tsdf_integrator_fast.h`);
  the ROS server is a thin subclass of voxblox's (`semantic_tsdf_server.h:47`).

Adaptation for Mura, keeping the pattern and shrinking the cost:

- **Minimal label set for the shell** (8 fits in 3 bits; one uint8 histogram slot each):
  `floor, wall, ceiling, table/platform, seat, door, window, other`. Rationale per consumer:
  floor/wall/ceiling/platform are the `xrt_plane_detector` enum and drive shadows/snapping;
  *seat* is the one furniture class placement cares about beyond tables; *door* and *window* are
  safety- and lighting-relevant (a door is a likely human entry path — boundary should not treat
  it as a permanent wall; a window is glass — stereo returns garbage there and the label explains
  low confidence). Everything else is `other`: the shell has no behavior that distinguishes it.
- **Pipeline**: a low-rate (≈ 0.5–1 Hz, duty-cycled with the mapper) NPU segmentation net on the
  RGB (Lynx) or mono (Frame) stream — sharing the ONNX→QNN/ExecuTorch toolchain doc 14 already
  requires — projected into the voxel update exactly as Kimera does, but with an 8×uint8
  count/log-odds histogram per voxel instead of 21 floats (21×4 B = 84 B/voxel would multiply the
  whole map by ~10×; 8 B keeps semantics cheaper than the TSDF itself).
- **Label lift**: at extraction time, planes and mesh faces take the majority label of their
  supporting voxels — labels live on the map, not on the network output, so a bad single frame
  cannot flip a wall to a door.

## 6. Depth sources per device and the two operating points

### 6.1 The two operating points on one interface

Both operating points speak `DepthFrame` ([14 §5](14-mobile-stereo-depth.md)); they differ only
in configuration — which is the point, since it keeps the geometry service backend-agnostic:

| | **Passthrough point** (Tier 1–3) | **Mapping point** (this doc) |
|---|---|---|
| Rate | per camera frame (~30 Hz) | keyframe: 1–5 Hz, motion/novelty-triggered |
| Bias | temporal stability, latency | accuracy; latency irrelevant |
| Holes | must fill (bounded backstop) | **must not fill** — integrate only confident pixels; the TSDF *is* the accumulator |
| Confidence use | gates temporal filter | hard gate on integration (a wrong surface poisons the map for many frames; weight-average recovery is slow) |
| Model | LightStereo-class, W8A8, VGA | same net at higher res/more iterations, or the RAFT-Stereo-class teacher when docked |

Multi-view accumulation over head motion is not an extra mechanism — it *is* TSDF weighted
averaging (`voxblox` `updateTsdfVoxel`, `vdbfusion` `VDBVolume.cpp:80-90`): parallax across
keyframes converts one bad baseline into many synthetic ones. Monocular priors (mono-depth
completion) may propose surfaces only into the **inferred** state (§7), never observed — same
rule as plane extension.

### 6.2 Per device

| Device | Sensors ([07](07-device-landscape.md)) | Mapping consequence |
|---|---|---|
| **Lynx R1** | 2 mono global-shutter tracking cams + 2 RGB, no depth, no IR illum. documented | pure passive stereo + motion stereo; the full blank-wall problem; plane inference does the heavy lifting; RGB available for segmentation |
| **Steam Frame** | 4 mono cams + IR illuminators, no depth | IR is **flood, not pattern**: every source describes illuminators/emitters "so tracking works in the dark" ([IGN hands-on](https://www.ign.com/articles/steam-frame-preview-hands-on-with-valves-state-of-the-art-vr-headset), [Wikipedia](https://en.wikipedia.org/wiki/Steam_Frame), [XVRwiki](https://xvrwiki.org/wiki/Steam_Frame) — all **[verified as published descriptions]**); no source describes a structured/dot projector, so treat "flood" as the strong default and pattern as ruled out pending teardown **[flood-vs-pattern: inferred, not device-verified]**. Flood IR fixes **SNR in darkness, not texture**: a blank wall under uniform IR is still a blank wall to stereo. Same operating point as Lynx, plus low-light capability. Valve lists depth sensors among possible expansion-port accessories ([Wikipedia](https://en.wikipedia.org/wiki/Steam_Frame)) — a future `tof-sensor` config, not a plan. |
| **Quest 3** | IR **dot projector** + 4 tracking cams | the no-LiDAR existence proof: the pattern *is* texture, so stereo works on blank walls — active-pattern stereo collapses the blank-wall failure mode entirely (aspirational target; no Linux boot path today) |
| **Galaxy XR / Play For Dream** | dedicated depth sensors | Linux-driver reachability unverified ([07](07-device-landscape.md)); if reachable only through vendor Android services, that is the `android-backed` config below |

### 6.3 The `depthSource` device-contract axis

Proposed typed option, extending the `mura.*` contract
([device-contract §mura.adaptation](../architecture/device-contract.md)):

```
mura.perception.geometry.depthSource =
  none              # no depth capability at all: planes-from-landmarks only, no TSDF
  flood-ir          # passive stereo + flood illumination (Steam Frame): stereo backend,
                    #   night-capable; blank-wall inference REQUIRED; illuminator duty policy
  active-ir-pattern # projector texture (Quest 3): stereo backend with pattern-aware matching;
                    #   blank-wall inference optional; projector duty/eye-safety policy
  tof-sensor        # native depth hardware (Galaxy XR/P4D if drivers land): direct DepthFrame
                    #   producer, confidence from sensor; stereo demoted to fallback
  android-backed    # depth only via a vendor/Android service (libhybris/waydroid bridge):
                    #   treated as tof-sensor with unknown latency/calibration provenance —
                    #   calibration_version discipline mandatory
```

Per-value geometry-service implications: `none`/`flood-ir` raise the inferred-state budget
(plane extension on by default, §7) and lower integration confidence thresholds must NOT be the
response — the response is *fewer observed voxels*, more inferred planes. `active-ir-pattern` and
`tof-sensor` shrink the inferred budget and enable mesh-first occlusion in v2. The enum is about
*geometry policy*, deliberately not about which stereo network runs — that stays
`mura.xr.passthrough.depthBackend` ([perception §contract](../architecture/perception-passthrough-hands.md)).

## 7. The tri-state confidence model

How the studied systems represent not-knowing, verified from code:

- **voxblox**: TSDF weight 0 = unknown, implicitly; the ESDF makes it explicit with `observed`,
  plus — the exact precedent for our "inferred" state — a **`hallucinated`** flag for voxels
  "copied from the TSDF (false) or created from a pose or some other source (true)"
  (`voxblox/include/voxblox/core/voxel.h:19-28`); unknown ESDF defaults to a fixed far distance
  (`esdf_integrator.h:42-49`).
- **supereight2**: the cleanest three-state field — occupancy log-odds sign gives free (< 0) vs
  occupied (> 0), an explicit `observed` bool gives unknown, bounded by
  `log_odd_min/max = ±5.015` (`supereight2/include/se/map/data_field.hpp:25-38, 56-57`).
- **nvblox**: TSDF weight + `EsdfVoxel.observed` (`voxels.h:55-74`), plus a separate
  `FreespaceVoxel` layer that *promotes* free space to "high-confidence freespace" only after
  sustained evidence over time (dynablox recipe: last-occupied timestamp + consecutive-occupancy
  duration, `voxels.h:38-52`) — free is a claim that needs evidence, not the absence of hits.

Mura composes these into the committed tri-state (with the occupied/free split inside
"observed"), per voxel and per plane region:

| State | Source | Occlusion | Shadows | Boundary |
|---|---|---|---|---|
| **observed** (occupied/free) | integrated confident depth; weight above threshold | full depth write | full | free = walkable, occupied = obstacle |
| **inferred** | plane extension into stereo holes; mono-prior completion; the map behind stale decay | depth write with **feathered compositing** (below) | floor/platform only if the *plane* is observed-supported (shadow on hallucinated floor misleads) | treated as occupied (conservative) |
| **unknown** | never observed | **no occlusion — treat as far** | none | **keep-out** |

**Occlusion rendering at low confidence.** The failure modes are asymmetric: occluding with wrong
geometry cuts holes in virtual content (highly visible, reads as broken); failing to occlude
merely lets virtual content float over real (reads as "AR being AR"). Hence conservative
occlusion: unknown never occludes, and inferred surfaces occlude with a softened edge — dilate
the *virtual-wins* side by ~1 voxel footprint and `smoothstep` the depth test across the
confidence boundary, the same mechanism family as the hand cutout's `.automatic` fade
([perception §hand cutout](../architecture/perception-passthrough-hands.md)). The known artifact
to avoid is the **halo**: feathering in world space around an object leaves a bright rim of
un-occluded background; feather in *screen space at the state boundary*, and clamp feather width
so observed-state edges stay hard (doc 13's edge-snapping rule applies unchanged to static
geometry).

**Boundary reading of the same map** is inverted: there, *failing* to flag a hazard is the bad
error, so inferred counts as occupied and unknown is keep-out (§8). One map, two conservatisms,
opposite signs — which is exactly why the state must live in the map rather than being a
per-consumer threshold on raw weight.

## 8. Boundary/Guardian system

Minimum viable boundary, from precedents:

- **Floor height** — from the v1 floor plane, re-validated per session (Quest re-derives floor
  each time a boundary is auto-created: [Meta boundary help](https://www.meta.com/help/quest/463504908043519/)
  **[verified]**).
- **A play volume** — either a user-drawn polygon (roomscale) or a default stationary cylinder;
  Quest's stationary default is 1×1 m centered on the user (same source). Drawn boundaries are
  authored *against passthrough*, which Mura gets for free from the environment layer.
- **Kept-out volumes** — tri-state derived: observed-occupied and unknown space adjacent to the
  play volume. This is where this design exceeds the precedents: Quest/SteamVR guard a
  user-drawn line; a tri-state map also guards *the space the user never scanned*.
- **Breach UX** — distance-graded: mesh/grid overlay fading in as head or controllers approach
  (Quest overlays a translucent grid and fades in passthrough near the edge:
  [Meta Guardian docs](https://developers.meta.com/horizon/documentation/native/android/mobile-guardian/)
  **[verified]**), escalating to forced full passthrough on breach. SteamVR's Chaperone imports
  Guardian geometry but **grows it by 40 cm** to compensate for different fade-in sensitivities
  ([Valve dev statement](https://steamcommunity.com/app/250820/discussions/3/1636410430564547211/)
  **[verified]**) — the portable lesson: threshold semantics (distance at which UX triggers) are
  part of the boundary *contract*, not an implementation detail, or downstream layers will pad
  geometry to fake them.

**Compositor hook.** The boundary is a compositor-owned overlay + composition-policy state,
structurally identical to the lock scene ([composition §8](../architecture/zxr-shell-v2-composition.md)):
the perception side publishes boundary geometry and per-probe distances (head, controllers) at
tracker rate; the compositor renders the grid overlay into the T1 composition and, on breach,
switches composition mode to passthrough-dominant — *no client cooperation involved*, exactly as
lock cannot depend on clients. Distance evaluation is a handful of point queries against the
dense grid (or its 2D floor-plane slice, nvblox's `esdf_slicer.h` pattern), so it can run at IMU
rate on CPU — the one part of this whole document that is latency-critical, and it is trivially
cheap because it queries, never reconstructs.

## 9. Adopt

1. **Planes first (v1)**: gravity-aligned constrained RANSAC (1-DoF horizontal / 2-DoF vertical)
   over SLAM landmarks + keyframe depth; CAPE-style cell growing when dense depth exists;
   published in Monado's `xrt_plane_detector.h` shapes so `XR_EXT_plane_detection` works for
   clients and the compositor alike (§4.2).
2. **Bounded dense-grid Vulkan TSDF (v1.5)**: room-scale clipmap, 5–10 cm voxels, nvblox's
   projective integration as the compute-shader blueprint, voxblox's per-block dirty-bit
   incremental meshing, mesh delivered as dmabuf like every other layer source (§3).
3. **Tri-state as map state, not consumer threshold**: supereight2's observed+sign field
   semantics, voxblox's `hallucinated` flag precedent for *inferred*, nvblox's evidence-based
   freespace promotion; conservative occlusion (unknown = far, inferred = feathered) and inverted
   boundary conservatism (unknown = keep-out) from §7.
4. **Kimera's label-fusion pattern at 1/10 the memory**: 8-label uint8 histograms updated by
   low-rate NPU segmentation, labels lifted to planes/mesh at extraction (§5).
5. **Two operating points on the one `DepthFrame` interface**: mapping = keyframe-rate,
   accuracy-biased, hard confidence gate, no hole-filling (§6.1).
6. **`mura.perception.geometry.depthSource`** enum (`none | flood-ir | active-ir-pattern |
   tof-sensor | android-backed`) in the device contract, controlling geometry *policy* (inferred
   budget, illuminator duty), not backend selection (§6.3).
7. **Boundary as compositor composition-policy + perception-side distance probes**, with
   explicit threshold semantics in the contract (§8).
8. **Duty-cycling as the power story** (per ADR 0008): map on novelty/change detection; heavy
   refinement (teacher-grade depth, mesh cleanup, semantic re-pass) only while docked.
9. **Persistence**: serialize planes + TSDF bricks + labels per room, versioned by calibration
   (voxblox's protobuf layer IO is the shape precedent), so a session starts with yesterday's
   map in *inferred* state until re-observed — cheap warm start with honest confidence.

## 10. Reject

- **nvblox as code** — CUDA, stdgpu, unified memory are non-portable to Adreno. Design donor
  only (§3). Same for any plan that starts by "porting" rather than reimplementing the ~small
  core against Vulkan.
- **vdbfusion for on-device incremental use** — batch-only meshing (`VDBVolume.h:69-70`), no
  observed/free tracking, OpenVDB dependency weight; it is the offline/reference implementation.
- **ov_plane literally** — GPL-3.0 + OpenVINS coupling vs the Basalt-based stack; adopt the
  method (Delaunay-normal detection, CP parametrization, point-on-plane tight coupling) as the
  v2 reference (§4.3).
- **PlaneRCNN-class learned plane detection in v1** — over budget on the NPU that must first pay
  for stereo, and hallucination-prone exactly where the tri-state discipline forbids it (§4.2).
- **Frame-rate dense fusion** — the compositor's dynamic occlusion is Tier 1–3 passthrough
  depth; duplicating it in the map burns the power budget for zero user-visible gain. The
  geometry service never enters the display path.
- **Kimera's 21×float per-voxel label vectors** — ~84 B/voxel dwarfs the TSDF itself; the
  8-label uint8 histogram carries everything the shell consumes (§5).
- **Treating flood IR as a depth feature** — Steam Frame's illuminators change SNR, not texture;
  planning mapping quality around them repeats the blank-wall failure at night (§6.2).
- **GPU voxel hashing in v1** — room-scale bounds make the dense clipmap strictly simpler and
  probably faster on a tiler GPU (§3); hashing is a scale feature Mura doesn't need yet.

## 11. Open questions

1. **Adreno compute-pass measurements**: actual cost of the projective integration and marching
   cubes passes on XR2-class Adreno at 5 cm/10 cm, and bandwidth contention with the compositor
   during simultaneous passthrough warping — the §3 feasibility claim needs numbers.
2. **Basalt export surface**: cleanest way to get landmarks + gravity + keyframe poses out of
   Basalt inside Monado for the plane extractor (existing map access vs a patch — upstream
   conversation needed). Blocking for v1 planes.
3. **Novelty/change detection for duty-cycling**: what triggers "scene unfamiliar/changed" —
   tracker relocalization events, per-frame depth-vs-map disagreement rate, or both? Wrong
   triggers either burn power or let the map rot (stale-couch-moved problem; nvblox's decay
   integrators are one answer, `tsdf_decay_integrator.h`).
4. **Mesh handoff format**: dmabuf vertex/index buffers vs a compact brick format the compositor
   expands; and whether shadow catching wants the mesh at all or only plane proxies (planes are
   cheaper to soft-shadow and cannot have marching-cubes wobble).
5. **Segmentation model + domain**: which 8-label net at ≤ 1 Hz on the NPU, and does mono/IR
   input (Frame) vs RGB (Lynx) need two checkpoints — ties into doc 15's greyscale-native
   findings for hands.
6. **Quest 3 projector under Linux**: is the dot projector independently controllable (duty,
   sync with tracking-camera exposure), and does its pattern interfere with Mercury/SLAM on the
   same cameras? Unverifiable until a Linux boot path exists.
7. **Galaxy XR / Play For Dream depth reachability**: whether `tof-sensor` or only
   `android-backed` is realistic — depends on the vendor-kernel/driver findings of
   [07](07-device-landscape.md) §Galaxy XR/P4D, currently unverified.
8. **ESDF or not**: does the boundary + collision consumer set justify a true ESDF pass
   (nvblox-style), or do point-probes against the dense TSDF grid + the floor slice cover
   everything until path-planning-class consumers exist?
9. **Persistence vs privacy**: a stored room mesh is a floor plan of a home; where map
   serialization lands (encrypted at rest? user-erasable? never leaves device?) belongs in the
   same privacy boundary discussion as camera frames — flag for the device/build contract.
10. **Relocalization coupling**: reloading yesterday's map as *inferred* requires knowing we are
    in yesterday's room — anchor/relocalization design (out of scope here, the world-mapping
    track) gates how useful persistence actually is.
