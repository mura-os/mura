# 20 — SLAM/VIO stacks for XR: code-level study for the Tier 4 mapping architecture

**Date:** 2026-09-22.
**Question:** what do the actual codebases — the VIT interface, basalt-monado, ORB-SLAM3 and its
Monado bridge, ILLIXR, OpenVINS, Kimera — tell us about the choice the pending ADR must make:
**(a) layered** (Basalt VIO behind the VIT seam + a separate mapping/anchor service) versus
**(b) single-SLAM** (ORB-SLAM3 behind the tracker seam doing VIO+mapping+reloc in one)?

This is the layer A/B seam study for the Tier 4 stack (A: VIO · B: mapping/loop-closure ·
C: reloc+persistence · D: geometry · E: OpenXR surface). Scope discipline: sibling docs already
own big pieces and are cited, not duplicated —
[23](23-relocalization-multisession.md) owns ORB-SLAM3's relocalization internals (the BoW
candidate funnel) and RTAB-Map (Bayes reloc, WM/LTM, SQLite schema);
[22](22-dense-geometry-no-lidar.md) owns ov_plane's method and Kimera-Semantics;
[21](21-anchors-persistence-openxr.md) owns the OpenXR anchor surface. Per
[ADR 0008](../architecture/adr/0008-perception-services-placement.md), perception services run
Monado-side on the shared `xrt_frame` fan-out. The standing frame contract: corrections move
anchors via `T_map_local`, never the rendered world.

Primary sources (local clones under `references/`, cited `file:line`): `vit/`, `monado/`,
`basalt-monado/`, `orbslam3/`, `orbslam3-monado/`, `illixr/`, `open-vins/`, `kimera-vio/`,
`kimera-rpgo/`. All claims below are from files read directly unless marked otherwise.

---

## 1. Purpose

The core design problem is the map/local frame split. VIO owns the local frame the compositor
renders in; mapping owns the map frame that persists and gets corrected. The question is whether
the seam between them is (a) a process boundary we build, with Basalt on one side, or (b) erased
entirely by putting one SLAM system (ORB-SLAM3) behind Monado's tracker interface and letting it
correct its own world frame. This doc reads the code on both sides of that choice:

- What the VIT interface actually transports today, and what a mapping service would need that
  it doesn't (§2).
- How much of a mapping layer Basalt already contains beyond VIO (§3).
- Whether ORB-SLAM3-behind-Monado has been done, and what it looked like (§4).
- What ORB-SLAM3's Atlas persistence actually serializes, since path (b) leans on it (§5).
- One alternative integration pattern (ILLIXR's topic bus, §6) and two more codebases read for
  design rather than adoption (OpenVINS, Kimera, §7).
- Licenses and packaging, which turn out to force part of the answer (§8).

---

## 2. The VIT seam

### 2.1 What the interface is

`vit_interface.h` (BSL-1.0, upstream `gitlab.freedesktop.org/monado/utilities/vit`) is a C ABI at
version **2.0.1** (`vit/vit_interface.h:27-29`), dlopen'd by Monado — the implementation lives in
the tracker's `.so`, Monado resolves symbols at runtime (`t_vit_bundle_load`,
`monado/src/xrt/auxiliary/tracking/t_tracker_slam.cpp:1457`), defaulting to `libbasalt.so`
(`t_tracker_slam.cpp:54`). The VIT repo's last commit is 2024-05-07 (`cb3cf67`) — the interface is
stable, not churning. A C++ helper class exists for implementers
(`vit/vit_implementation_helper.hpp`).

**Data in** (consumer → tracker):

- `vit_imu_sample`: timestamp ns, accel, gyro (`vit_interface.h:164-173`).
- `vit_img_sample`: cam index, timestamp, raw pixels (L8/L16/RGB), plus **rectangular masks to
  ignore** (`vit_interface.h:188-203`) — this is how hand occlusion reaches the tracker.
- Calibration, behind extensions: `vit_camera_calibration` with intrinsics, distortion
  (RT4/RT5/RT8/KB4), and a full row-major 4×4 `T_imu_cam` (`vit_interface.h:253-264`);
  `vit_imu_calibration` with bias/noise random-process parameters (`vit_interface.h:292-298`).

**Data out** (tracker → consumer) — this is the load-bearing fact:

- `vit_pose_data`: timestamp, position, orientation quaternion, **linear velocity** — nothing
  else (`vit_interface.h:208-220`), popped one at a time by a single consumer via
  `vit_tracker_pop_pose` (`vit_interface.h:450-458`, explicitly "consumed by a single consummer").
- Extension `POSE_TIMING`: per-pose pipeline-stage timestamps (`vit_interface.h:225-228`).
- Extension `POSE_FEATURES`: per-camera feature list `{id, u, v, depth}` per pose
  (`vit_interface.h:230-244`).

**Extension mechanism:** a fixed enum of exactly four extensions — add camera calib, add IMU
calib, pose timing, pose features (`vit_interface.h:101-112`) — queried/enabled through boolean
sets (`vit_interface.h:117-122`). There is no string-keyed or open-ended capability negotiation;
growing the interface means editing the header and bumping the version. Minor-version additions
are defined as backwards compatible (`vit_interface.h:27-29`), so adding calls is legal without
breaking Basalt or Monado.

**What does not exist anywhere in the header:** keyframe access, marginalized-factor egress, map
queries, save/load/serialization, loop-closure or map-change events, relocalization events, any
`T_map_local`-shaped output, multi-consumer pose access. `vit_tracker_reset` is a full state wipe
(`vit_interface.h:400-403`), not a reloc primitive.

### 2.2 The consumer side (Monado)

`t_tracker_slam.cpp` (1563 lines, BSL-1.0) is the adapter. Facts that matter for the ADR:

- **Sinks:** the tracker registers per-camera frame sinks, one IMU sink, a ground-truth pose
  sink, and a `hand_masks_sink` (`t_tracker_slam.cpp:273-277`). Hand-tracking rectangles arrive
  asynchronously (`t_slam_hand_mask_sink_push`, `:1204-1212`), are latched, and attached to the
  next image sample as VIT masks (`receive_frame`, `:1308-1331`). This is the ADR 0008 fan-out
  working today: another consumer (hand tracking) already feeds *into* the SLAM path.
- **Calibration:** Monado converts its `t_slam_calibration` into VIT structs and pushes them
  before start (`:1027-1143`) — RT8, WMR (as RT8+rpmax), and KB4 models handled (`:1043-1081`).
- **Pose consumption:** `flush_poses` drains the VIT queue into a `RelationHistory` (`:744-815`);
  angular velocity is *derived* by finite-differencing successive quaternions (`:790`) since VIT
  carries only linear velocity.
- **Prediction:** five modes selected by `SLAM_PREDICTION_TYPE` — NONE, POSE_ONLY, GYRO,
  ACCEL_GYRO, DEAD_RECKONING (default, `:92`) — extrapolating the last SLAM relation with raw IMU
  fifos (`predict_pose`, `:818-898`).
- **Filtering:** three optional smoothing filters — moving average, exponential, one-euro —
  applied after prediction (`:315-340`, `filter_pose` `:900-953`). These are UI-toggled comfort
  filters, not correction absorbers; nothing in this file is designed to hide a multi-centimeter
  loop-closure jump.
- **Config env vars** (`:86-97`): `SLAM_LOG`, `VIT_SYSTEM_LIBRARY_PATH`, `SLAM_CONFIG` (path to
  implementation-specific config), `SLAM_UI`, `SLAM_SUBMIT_FROM_START`,
  `SLAM_OPENVR_GROUNDTRUTH_DEVICE`, `SLAM_PREDICTION_TYPE`, `SLAM_WRITE_CSVS`, `SLAM_CSV_PATH`,
  `SLAM_TIMING_STAT`, `SLAM_FEATURES_STAT`, `SLAM_CAM_COUNT`. These are the knobs a NixOS module
  will surface.

### 2.3 Verdict: can a mapping service consume keyframes through or beside VIT today?

**Through VIT: no.** The interface transports poses (+ timing, + 2D features) in one direction
and samples/calibration in the other. There is no keyframe, factor, covariance, map, event, or
serialization surface at all, and the pose queue is single-consumer by contract
(`vit_interface.h:450-458`).

**Beside VIT: yes, and ADR 0008 already provides the transport for the inputs.** A mapping
service on the shared `xrt_frame` fan-out receives the same frames and IMU as the tracker. What
it *cannot* get from the fan-out is the VIO's internal state: keyframe decisions, marginalized
priors, landmark estimates. Three options exist, in increasing invasiveness:

1. **Run its own frontend** on the fan-out frames (RTAB-Map-style, doc 23) — zero VIT changes,
   duplicated feature extraction cost.
2. **Use the `POSE_FEATURES` extension** — Monado already receives per-pose `{id,u,v,depth}`
   tracks per camera (`vit_interface.h:230-244`); forwarding pose+features to the mapping service
   is a Monado-side change only (tee inside `flush_poses`), no VIT change. That is a poor-man's
   keyframe: enough for BoW-free place recognition experiments and landmark triangulation, but
   without marginalization info or keyframe selection.
3. **Extend VIT** with a keyframe/marg-data egress extension. Legal as a minor-version addition;
   the data already exists on Basalt's side of the wall (§3). This is the "what exactly is
   missing" answer: one enum entry, one `vit_keyframe`-ish struct (keyframe pose, timestamps,
   landmark positions, optionally the marginalization prior), and a second pop queue — a bounded,
   upstreamable patch.

---

## 3. basalt-monado beyond VIO

The Collabora fork (`basalt-monado/`, BSD-3-Clause, last commit 2026-07-24 `df6e970`; the VIT
implementation itself last touched 2026-07-24) builds substantially more than `libbasalt.so`:

- `basalt` SHARED library — the VIT implementation (`CMakeLists.txt:483`).
- Executables (`CMakeLists.txt:497-535`): `basalt_vio`, **`basalt_mapper`**, `basalt_mapper_sim`,
  `basalt_calibrate`, `basalt_calibrate_imu`, `basalt_opt_flow`, `basalt_time_alignment`,
  `basalt_kitti_eval`, RealSense T265 tools. This matches the nixpkgs binary inventory (§8).

### 3.1 The VIT implementation

`src/vit/vit_tracker.cpp` implements all four VIT extensions (`:100-106`). Pipeline: images →
`OpticalFlow` (frontend, own thread) → `SqrtKeypointVioEstimator` (backend) → bounded output
queue of `PoseVelBiasState` capped at 32 (`:181`), popped by Monado. Calibration arrives either
from a unified config file (`--config` CLI-style file parsed at `:214-246`, with `--cam-calib`,
`--config-path`, `--marg-data`, `--num-threads` options) or programmatically via the VIT
calibration extensions (`apply_cam_calibration` `:263-306`, `apply_imu_calibration` `:308-359`).
The 20-stage timing instrumentation (`:62-89`) is what feeds Monado's timing UI. Basalt requires
stereo — `ASSERT(cam_count > 1, ...)` (`:168`).

### 3.2 The marginalization egress — the key discovery

The VIO estimator has a dedicated output queue for marginalization data:
`out_marg_queue` (`include/basalt/vi_estimator/vio_estimator.h:105`). When the VIT config sets
`--marg-data`, the tracker wires it to a `MargDataSaver` (`src/vit/vit_tracker.cpp:403-406`) that
serializes each marginalization step to disk as cereal binary — one `<kf_id>.cereal` per step
(`src/io/marg_data_io.cpp:65-69`), optionally with keyframe images (`:97-101`).

`MargData` is exactly the payload a mapping service wants
(`include/basalt/utils/imu_types.h:328-341`):

```328:341:references/basalt-monado/include/basalt/utils/imu_types.h
struct MargData {
  typedef std::shared_ptr<MargData> Ptr;

  AbsOrderMap aom;
  Eigen::MatrixXd abs_H;
  Eigen::VectorXd abs_b;
  Eigen::aligned_map<int64_t, PoseVelBiasStateWithLin<double>> frame_states;
  Eigen::aligned_map<int64_t, PoseStateWithLin<double>> frame_poses;
  std::set<int64_t> kfs_all;
  std::set<int64_t> kfs_to_marg;
  bool use_imu;

  std::vector<std::shared_ptr<OpticalFlowResult>> opt_flow_res;
};
```

That is: the marginalization Hessian and gradient (`abs_H`, `abs_b`), keyframe poses/states, which
keyframes are being marginalized, and the raw optical-flow observations. **"Keyframes out" is not
a research problem in Basalt; it is a queue that currently points at a disk writer.** Replacing
`MargDataSaver` with an IPC forwarder (or teeing the queue) is the entire fork surface for the
layered path's VIO side.

### 3.3 The mapper: proof the recovery math exists, but offline

`basalt_mapper` (`src/mapper.cpp`) is an **offline Pangolin GUI tool**: it loads the saved
`MargData` from disk (`MargDataLoader`, `src/io/marg_data_io.cpp:134,162`) and runs `NfrMapper`
(`src/vi_estimator/nfr_mapper.cpp`), which:

- recovers **non-linear factors** (roll-pitch and relative-pose) from the marginalization
  Hessians (the "nonlinear factor recovery" method — this is the mapper's namesake),
- detects keypoints per keyframe (`detect_keypoints`, `nfr_mapper.cpp:419-439`),
- builds a lightweight BoW — `HashBow` (`include/basalt/hash_bow/hash_bow.h`, instantiated with
  `config.mapper_bow_num_bits` at `nfr_mapper.cpp:57`), computes/queries BoW vectors for loop
  candidates (`:446-448`, `match_all` `:505-545`),
- matches stereo and loop pairs (`match_stereo` `:466`, hamming threshold via
  `config.mapper_max_hamming_distance` `:488`),
- and optimizes the resulting pose graph / BA problem.

So Basalt contains, in-tree: keyframe egress, factor recovery, BoW place recognition, and
pose-graph optimization — everything layer B needs *except* running online and *except* any
save/load of the resulting map (the mapper visualizes and evaluates; it does not persist a
relocalizable map, and nothing feeds corrections back into the running VIO). There is no
relocalization path into the VIO at all — which is fine, because our frame contract forbids
exactly that: corrections belong to the map frame, not the rendered local frame.

**Distance from "keyframes out": one queue redirect. Distance from "online mapping service": the
NfrMapper logic needs to be driven incrementally instead of batch-loaded, plus a persistence
format (doc 23's territory).**

---

## 4. The orbslam3-monado bridge

`orbslam3-monado/Examples/Monado/slam_tracker.cpp` (447 lines; the bridge file itself is
BSL-1.0-headered, `:1-2`, inside a GPL-3 repo) is Collabora's proof-of-concept that ORB-SLAM3 can
sit behind Monado's tracker seam. Facts:

- **It implements the old ABI, not VIT.** It fulfills `slam_tracker.hpp`
  `HEADER_VERSION_MAJOR = 6` (`Thirdparty/monado/slam_tracker.hpp:32`) — the C++ interface the
  VIT README says VIT *supersedes* (`vit/README.md`). Current Monado no longer loads this;
  reviving the bridge means porting it to VIT (mechanical: the surface is nearly isomorphic).
- **It exposes poses only, plus optional timing.** `supported_features` contains exactly
  `F_ENABLE_POSE_EXT_TIMING` (`slam_tracker.cpp:119`). Calibration injection is a literal
  `#if 0` block: "This is how this should look if we supported other features, but we don't"
  (`:259-270`) — calibration comes solely from the ORB-SLAM3 YAML (`SLAM.vocabulary`,
  `SLAM.type`, `SLAM.rectify` keys, `:133-139`), with optional in-bridge stereo rectification
  (`:159-184`). No feature info, no reloc events, no map events.
- **The pose it pushes has no velocity** (`pose{ts, px..rw}`, `:320`) — the old ABI's pose struct
  carries position+orientation only, so Monado-side prediction would run degraded versus Basalt's
  `vit_pose_data` velocity.
- **Threading:** frames and IMU are queued (moodycamel), a consumer thread calls
  `TrackStereo`/`TrackMonocular` synchronously (`:335`, `:359`) and pushes
  `T_w_b = (T_c_w)⁻¹ · T_c_b⁻¹` (`try_push_pose`, `:312-323`).
- **Maintenance:** the clone is shallow (1 commit visible); the tip is `d1038b3` 2025-07-20 and
  touches only config files. This is an evaluation artifact, not a maintained product. Adopting
  it means owning it.

**Viability verdict:** yes, it proves ORB-SLAM3-behind-Monado runs — Collabora used it to
benchmark against Basalt (the timing titles include a `monado_dequeued` stage, `:91-94`). The
disqualifying evidence it also provides is the smoothness risk: `TrackStereo` returns the pose in
ORB-SLAM3's *corrected world frame*. When `LoopClosing::CorrectLoop`
(`orbslam3/src/LoopClosing.cc:969`) or `MergeLocal` (`:1215`) rewrites keyframe poses — or a map
merge switches the active map via `Atlas::ChangeMap` (`:1550`) — the very next tracked pose is
expressed in the corrected frame, i.e. **the compositor's world jumps by the accumulated drift,
in one frame**. Nothing in the bridge mediates this, and Monado's defenses (§2.2) are comfort
filters and IMU extrapolation, neither designed for topological corrections. A single-SLAM path
would need to intercept corrections inside ORB-SLAM3 (fork territory: split its output into
"odometry frame" and "correction transform") to honor our map/local contract — at which point
you have re-implemented the layered split inside a GPL codebase.

---

## 5. ORB-SLAM3 Atlas/serialization internals

Upstream `orbslam3/` (GPL-3, last commit `4452a3c` 2022-02-10 — **dormant for 4+ years**).
Persistence is settings-driven: `System.SaveAtlasToFile` / `System.LoadAtlasFromFile` YAML keys
(`src/Settings.cc:475-476`, `src/System.cc:81-97`), and the EuRoC stereo-inertial example
constructs `System` with `viewer=false` (`Examples/Stereo-Inertial/stereo_inertial_euroc.cc:132`)
— headless operation is a constructor flag, no fork needed.

### 5.1 Mechanics

- **Save** (`System::SaveAtlas`, `src/System.cc:1403-1443`): triggered only at `Shutdown`
  (`:548-551`). Calls `Atlas::PreSave`, then writes `<name>.osa` as a **boost binary archive**
  (`boost::archive::binary_oarchive`, `:1436`; a text mode exists) containing the vocabulary
  filename, an **MD5 checksum of the vocabulary**, and the whole Atlas object graph (`:1438-1440`).
- **Load** (`System::LoadAtlas`, `:1445-1509`): at construction (`:156-180`). Refuses to load if
  the vocabulary checksum differs (`:1490-1497`). After load: re-attach KeyFrameDatabase and
  vocabulary, `Atlas::PostLoad`, then **`CreateNewMap()`** (`:172`) — a new session always begins
  in a fresh map; getting back into the loaded maps is relocalization + map merge (the BoW funnel
  and its thresholds are doc 23 §ORB-SLAM3's subject).

### 5.2 What is serialized

- **Atlas** (`include/Atlas.h:54-70`): the map vector (`mvpBackupMaps`), camera models
  (Pinhole/KannalaBrandt8 registered polymorphically), and the static ID counters of
  Map/Frame/KeyFrame/MapPoint/GeometricCamera — global state, one blob.
- **Map** (`include/Map.h:46-67`): KF/MP backup vectors, origin KF ids, IMU-initialization flags.
  `Map::PostLoad` (`src/Map.cc:427-470`) rebuilds pointer sets from serialized ids and re-adds
  every keyframe to the KeyFrameDatabase (`:469`) — the inverted BoW index is *rebuilt*, not
  stored.
- **KeyFrame** (`include/KeyFrame.h:57-190`): full undistorted keypoints, the entire ORB
  **descriptor matrix** (`:128`), **both DBoW2 vectors** `mBowVec`/`mFeatVec` (`:130-131`), pose
  `mTcw` (`:148`), the covisibility graph as an id→weight map (`:154`), spanning-tree parent and
  children ids, loop/merge edge ids (`:159`), and the full inertial state — bias, preintegration,
  calib, prev/next KF ids, velocity (`:182-190`). Dozens of commented-out fields show the
  authors' choice to exclude tracking/BA temporaries.
- **MapPoint** (`include/MapPoint.h:49-92`): world position (`:86`), observation id-maps
  (`:90-91`), best descriptor (`:92`).

**Size character:** each keyframe carries ~1–2k keypoints × (keypoint struct + 32-byte
descriptor) plus BoW vectors and grids — order 100s of KB per keyframe in binary form; map points
add 3D position + descriptor + observation maps. It is a **monolithic O(map) blob written once at
shutdown**: no incremental save, no journaling, a crash mid-write loses everything. Contrast
RTAB-Map's per-node SQLite rows with incremental commits (doc 23 §RTAB-Map).

### 5.3 The reference-relative pose pattern

KeyFrame serializes `mTcp` — "pose relative to parent" (`include/KeyFrame.h:133`), computed as
`mTcp = mTcw * mpParent->GetPoseInverse()` when a keyframe is culled
(`src/KeyFrame.cc:671`, in `SetBadFlag`), so children re-express themselves relative to a
surviving ancestor in the spanning tree. This is precisely the anchor pattern our map/local split
needs at service level: **an anchor is a pose relative to a reference keyframe; corrections move
keyframes; anchors re-express through their parent, and the rendered world never sees the
correction.** ORB-SLAM3 uses it only for graph maintenance internally — evidence that the pattern
is sound, and that ORB-SLAM3 itself gives external consumers no access to it.

### 5.4 Multi-map entry points

`Atlas::CreateNewMap` (`src/Atlas.cc:58-77`) is invoked on tracking loss (new disconnected map);
`Atlas::ChangeMap` (`:79-90`) switches the active map during merges; the merges themselves are
`LoopClosing::MergeLocal` (`src/LoopClosing.cc:1215`, visual) and `MergeLocal2` (`:1783`,
inertial), with in-map corrections in `CorrectLoop` (`:969`). All of these rewrite poses that a
tracker consumer has already reported — the §4 jump problem, seen from the inside.

---

## 6. Integration patterns: ILLIXR's topic bus

ILLIXR (`illixr/`, University of Illinois permissive license; very active — last commit
2026-09-21) is the "what if the runtime were a pub/sub bus" counter-design to Monado's VIT:

- Everything is a **plugin** connected through **switchboard**, a typed topic bus with
  `get_reader`/`get_writer`/`put` and callback scheduling (`include/illixr/switchboard.hpp:95-125`
  usage docs), plus a `phonebook` service registry.
- SLAM is just a plugin publishing to pose topics: `plugins/openvins/` and `plugins/orb_slam3/`
  both exist as thin CMake wrappers fetching the external system. A separate `gtsam_integrator`
  plugin consumes slow VIO poses + IMU and serves fast poses; renderers query a
  `pose_prediction` service (`include/illixr/data_format/pose_prediction.hpp:32-38`).
  `offload_vio/` plugins even split VIO across the network at topic boundaries.
- The lesson for Mura: **the multi-consumer seam VIT lacks is a topic**. In ILLIXR a
  mapping service would just subscribe to the camera, IMU, and slow-pose topics — no interface
  change. We don't adopt ILLIXR (research runtime, single-process plugin model, no isolation, no
  OpenXR conformance story comparable to Monado), but the layered design should treat ADR 0008's
  `xrt_frame` fan-out + a pose/keyframe IPC topic as our equivalent of switchboard, i.e. design
  the mapping-service input as a subscription, not a bespoke pairwise API.

## 7. OpenVINS and Kimera in brief

**OpenVINS** (`open-vins/`, **GPL-3** (`LICENSE`), active — last commit 2025-11-30): a clean
MSCKF: `VioManager` (`ov_msckf/src/core/`), state clones + propagation
(`src/state/State.h`, `Propagator.h`), and EKF updates split into `UpdaterMSCKF` (features
marginalized per-update), `UpdaterSLAM` (persistent landmarks), `UpdaterZeroVelocity`
(`src/update/`). ROS lives in a separate `ros/` directory; the core is ROS-free (ILLIXR wraps it
as a plugin). It contains **no mapping, no loop closure, no persistence** — as a candidate it is
"Basalt's role, but GPL and filter-based," so it would inherit every missing piece of the layered
path while adding a license problem inside the Monado process. Method-vs-code: the ov_plane
plane-landmark method built on it is covered in doc 22; nothing else is worth extracting as code.

**Kimera** (`kimera-vio/` BSD-2, last commit 2025-02-10; `kimera-rpgo/` BSD-2, 2024-11-25 —
both effectively dormant ~1–2 years): the value is the **blueprint, not the binaries**. The
pipeline is modules-with-queues (`src/pipeline/Pipeline.cpp`, `PipelineModule.cpp`;
Mono/Stereo/RgbdImuPipeline variants); the backend is a GTSAM fixed-lag smoother
(`src/backend/VioBackend.cpp`); and crucially, loop closure is a *separate module wrapping a
separate library*: `LoopClosureDetector` feeds odometry and loop factors into
`KimeraRPGO::RobustSolver` (`src/loopclosure/LoopClosureDetector.cpp:20,173-182`), a robust pose
graph with GNC/PCM outlier rejection. That is the layered architecture drawn *inside one
process*: VIO emits odometry factors; a distinct robust-PGO component owns the map estimate.
Mura should adopt this as the mapping service's internal shape (odometry factors in, robust
PGO, outlier-rejected loop factors) across a process boundary instead of a queue.

---

## 8. Licenses, service boundaries, packaging

Checked against actual `LICENSE` files in each clone:

| Component | License | File checked |
|---|---|---|
| Monado | BSL-1.0 | `monado/LICENSE` |
| VIT interface | BSL-1.0 | `vit/LICENSE` |
| basalt-monado | BSD-3-Clause | `basalt-monado/LICENSE` |
| ORB-SLAM3 | **GPL-3** | `orbslam3/LICENSE` |
| orbslam3-monado bridge | BSL-1.0 file in GPL-3 repo → combined work GPL-3 | `orbslam3-monado/LICENSE`, `Examples/Monado/slam_tracker.cpp:1-2` |
| OpenVINS | **GPL-3** | `open-vins/LICENSE` |
| ov_plane | **GPL-3** | `ov-plane/LICENSE` |
| Kimera-VIO / RPGO | BSD-2-Clause | `kimera-vio/LICENSE.BSD`, `kimera-rpgo/LICENSE.BSD` |
| RTAB-Map | BSD | `rtabmap/LICENSE` |
| ILLIXR | Illinois/NCSA-style permissive | `illixr/LICENSE` |

**Service-boundary implication.** VIT trackers are dlopen'd *into the Monado process*. Loading a
GPL-3 `.so` into BSL-1.0 Monado and shipping the combination is, at minimum, contested territory
— dynamic linking does not launder GPL. So the single-SLAM path (b) puts GPL-3 code in-process
with the compositor. The layered path (a) keeps the in-process tracker BSD (Basalt) and pushes
anything GPL-derived (ORB-SLAM3 reloc ideas, ov_plane) behind a **separate-process IPC boundary**
— which the map/local split wants architecturally anyway. The licenses and the architecture point
the same direction; this is rare and worth exploiting.

**Packaging facts** (established by the parent session, restated verbatim): nixpkgs ships
`basalt-monado` (0-unstable-2025-09-25, cached, binaries include basalt_vio AND basalt_mapper +
calibration tools) — the Basalt path has zero packaging cost. ORB-SLAM3 is NOT in nixpkgs; it
hard-requires Pangolin (also not in nixpkgs; builds -Werror with vendored tinyobj warnings on new
GCC) plus vendored Thirdparty (DBoW2/g2o/Sophus) — real but bounded packaging cost. (Note from
§5: the Pangolin viewer is construction-flag-optional at *runtime*, but the build still links it;
a headless-build patch would be part of that packaging cost.)

---

## 9. What Mura should adopt

1. **Basalt via VIT as layer A, unmodified, from nixpkgs** — the seam is stable (v2.0.1), the
   implementation is complete (all four extensions), maintained (2026-07), BSD, and cached.
2. **The `out_marg_queue` as the keyframe egress point** (`vio_estimator.h:105`,
   `vit_tracker.cpp:403-406`): tee `MargData` to the mapping service instead of (or beside) the
   disk saver. Smallest possible fork of basalt-monado; candidate for upstreaming as a VIT
   minor-version extension (§2.3 option 3).
3. **NfrMapper's recovery pipeline as the mapping service's starting point** — factor recovery
   from marginalization Hessians + HashBow + pose graph already exist in-tree (BSD) and only need
   to be driven online (§3.3).
4. **Kimera's module shape** for the mapping service internals: odometry factors in, robust PGO
   (RPGO is BSD and importable as a library) with outlier rejection owning the map estimate (§7).
5. **ORB-SLAM3's `mTcp` parent-relative pattern** as the anchor re-expression design
   (§5.3) — pattern, not code.
6. **ILLIXR's topic framing** for the service interface: mapping input = subscription to
   {frames, IMU, poses(+features), marg-data}, not a bespoke API (§6).
7. **ORB-SLAM3 + Pangolin packaged in the Mura flake anyway** — not for production, but as
   the evaluation baseline and reloc-behavior reference (doc 23 depends on studying it live);
   bounded cost, GPL is irrelevant for local evaluation.

## 10. What to reject

- **ORB-SLAM3 as the production tracker behind VIT** — corrected-world pose jumps violate the
  frame contract and fixing that means forking a 4-years-dormant GPL codebase (§4, §5.4);
  GPL-in-Monado-process besides (§8).
- **The orbslam3-monado bridge as a dependency** — dead ABI (pre-VIT), shallow-maintained,
  poses-without-velocity; keep it only as reference reading (§4).
- **Atlas `.osa` as a persistence format** — monolithic shutdown-time boost blob, vocabulary
  checksum coupling, no incremental save (§5.2); doc 23's store design supersedes it.
- **OpenVINS as the VIO** — GPL for no capability gain over Basalt; no mapping layer either (§7).
- **Kimera-VIO as shipped code** — dormant; adopt the architecture, import at most RPGO (§7).
- **ILLIXR as runtime** — research vehicle; Monado is the product runtime (§6).
- **Extending VIT into a full map API** (keyframes and events and serialization and reloc) —
  keep VIT as the thin pose seam it is; map/anchor/persistence traffic belongs to the mapping
  service's own IPC surface, or we recreate single-SLAM coupling through the back door.

## 11. Layered vs single-SLAM: the evidence summary for the ADR

**Evidence for (a) layered — Basalt VIO + separate mapping/anchor service:**

- The frame contract falls out for free: VIO never receives corrections, so the rendered world
  cannot jump; `T_map_local` lives where corrections are computed (§2.3, §3.3).
- The keyframe egress is one queue redirect in BSD code (`MargData`, §3.2), and factor recovery +
  BoW + PGO already exist in the same tree (§3.3).
- Licenses align: BSD/BSL in-process, GPL quarantined behind IPC (§8).
- Packaging: zero cost today, cached in nixpkgs (§8).
- Process isolation matches ADR 0008's placement and lets mapping be duty-cycled/killed without
  touching tracking.

**What (a) is missing** (the honest gap list): an online driver for NfrMapper (it is batch/GUI
today); a persistence format and reloc funnel (docs 21/23 designs, unbuilt); the
`T_map_local` estimator itself; the anchor re-expression service; the marg-data IPC schema; and
the VIT/Monado tee patch. None of it is research-hard — it is engineering that the code studied
here de-risks — but it is all net-new code we own.

**Evidence for (b) single-SLAM — ORB-SLAM3 behind the tracker seam:**

- VIO+mapping+loop-closure+reloc+multi-map+persistence in one battle-tested system; nothing to
  integrate (§4, §5).
- Proven runnable behind Monado's seam by the Collabora bridge (§4); headless via constructor
  flag (§5); persistence via two YAML keys (§5).
- Genuinely less code to write *if* pose jumps and licensing were acceptable.

**What (b) is missing:** any way to honor the map/local split — corrections land in the pose
stream by construction, and separating "odometry frame" from "correction" means forking the
LoopClosing/Atlas core of a dormant GPL project (§4, §5.4); a VIT port of the bridge; calibration
injection (bridge `#if 0`, §4); velocity output; nixpkgs packaging of ORB-SLAM3+Pangolin; and a
defensible GPL-in-Monado-process position (§8). Persistence exists but is the wrong shape
(monolithic shutdown blob, §5.2), and reloc after load still starts in a fresh map until a merge
fires (§5.1) — the boot-reloc UX problem (doc 23) is not actually solved by adopting it.

**Net:** the code evidence is lopsided. Path (b)'s one real asset — integrated maturity — is
undercut by the fact that honoring Mura's core frame contract requires forking exactly the
part of ORB-SLAM3 that makes it integrated. Path (a)'s gaps are all constructive (build the
service), sit on maintained BSD code with a zero-cost package, and every piece studied here
(marg queue, NfrMapper, RPGO, mTcp pattern) shortens them. The ADR should choose **layered**,
with ORB-SLAM3 retained as an evaluation baseline and design reference.
