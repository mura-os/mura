# Spatial mapping: anchors, persistence, and world understanding (Tier 4)

**Status:** design, extending [perception-passthrough-hands.md](perception-passthrough-hands.md)
and [zxr-shell-v2-composition.md](zxr-shell-v2-composition.md).
**Date:** 2026-09-22. Synthesizes research docs
[20-slam-stacks-for-xr](../research/20-slam-stacks-for-xr.md),
[21-anchors-persistence-openxr](../research/21-anchors-persistence-openxr.md),
[22-dense-geometry-no-lidar](../research/22-dense-geometry-no-lidar.md), and
[23-relocalization-multisession](../research/23-relocalization-multisession.md). The stack choice
is decided in [ADR 0009](adr/0009-spatial-mapping-architecture.md); service placement follows
[ADR 0008](adr/0008-perception-services-placement.md).

## 1. Goal and non-goals

**Goal:** apps are *physically located* in the user's environment — place a window above the desk,
take the headset off, reboot, put it on tomorrow, and the window is above the desk — on
camera+IMU hardware with **no LiDAR**, with the rendered world never jumping.

Non-goals (v1): LiDAR-grade watertight meshes; shared/cloud anchors (the spatial-sharing track
consumes this design later); object-level scene understanding beyond an 8-label vocabulary;
correcting VIO itself (drift is absorbed by anchors, not fed back — [23 §11 Q2](../research/23-relocalization-multisession.md)).

## 2. The five layers

| Layer | Function | RT class | Implementation |
|---|---|---|---|
| **A. VIO** | 6DoF head pose, gravity, velocity | hard RT (camera rate) | **Basalt via VIT, unmodified, from nixpkgs** (`basalt-monado`, cached) — [20 §2–3](../research/20-slam-stacks-for-xr.md) |
| **B. Mapping** | keyframes, loop closure, map optimization | async (100 ms–s) | **mapping service** (separate process): MargData ingest + NfrMapper-derived factor recovery + robust PGO (Kimera-RPGO-shaped) — [20 §3, §7](../research/20-slam-stacks-for-xr.md) |
| **C. Reloc + persistence** | recognize the room at boot; encrypted store; `T_map_local` | boot ≤1–2 s; background low-rate | **two-tier reloc** (classical always-on + learned at boot) over an RTAB-Map-shaped store — [23](../research/23-relocalization-multisession.md), [21 §5](../research/21-anchors-persistence-openxr.md) |
| **D. Geometry** | planes, TSDF mesh, semantics, boundary | duty-cycled (1–5 Hz keyframes; ≤1 Hz mesh/labels) | gravity-RANSAC planes (v1) → bounded Vulkan TSDF clipmap (v1.5) → 8-label fusion — [22](../research/22-dense-geometry-no-lidar.md) |
| **E. API surface** | anchors/planes/persistence to apps | n/a | **`XR_EXT_spatial_entity` family in Monado** (upstream); compositor consumes services directly — [21 §2, §7](../research/21-anchors-persistence-openxr.md) |

```mermaid
flowchart TB
    subgraph monadoProc [Monado process - BSL/BSD only]
        fanout["xrt_frame fan-out (one clock, one calibration - ADR 0008)"]
        vio["Basalt VIO via VIT (local frame; poses+velocity out; masks in)"]
        oxr["OpenXR state tracker: XR_EXT_spatial_entity/anchor/plane/persistence (new)"]
    end
    subgraph mapProc [mapping+anchor service - separate process]
        ingest["MargData/pose+feature ingest (subscription, not bespoke API)"]
        pgo["factor recovery + robust PGO (map frame)"]
        reloc["two-tier relocalization -> T_map_local"]
        anchors["anchor table: T_map_kf * T_kf_anchor; re-expression via T_map_local"]
        store["encrypted per-map store (SQLite-shaped, descriptors-not-images)"]
    end
    subgraph geoProc [geometry service - duty-cycled]
        planes["gravity-RANSAC planes -> xrt_plane_detector shapes"]
        tsdf["bounded Vulkan TSDF clipmap + incremental mesh"]
        sem["8-label semantics fusion"]
        boundary["boundary probes (IMU-rate distance queries)"]
    end
    fanout --> vio
    fanout --> ingest
    fanout --> planes
    vio -->|"MargData tee (one queue redirect)"| ingest
    ingest --> pgo --> anchors
    reloc --> anchors
    store <--> pgo
    store <--> reloc
    anchors --> oxr
    planes --> oxr
    tsdf --> comp["zxr compositor: occlusion, shadows, boundary overlay"]
    planes --> comp
    anchors --> comp
    boundary --> comp
```

## 3. The two-frame architecture (the core contract)

With `T_A_B` transforming coordinates from frame B into A ([21 §4.2](../research/21-anchors-persistence-openxr.md)):

- **`local`**: the smooth, gravity-aligned VIO/render frame. The compositor and head pose live
  here, always. It drifts; that is allowed.
- **`map`**: the optimized, persistent frame. Loop closure and relocalization change map-side
  estimates; that is allowed.
- Anchors are stored **relative to reference keyframes**: `T_map_anchor = T_map_kf · T_kf_anchor`
  (ORB-SLAM3's internal `mTcp` parent-relative pattern, adopted as design —
  [20 §5.3](../research/20-slam-stacks-for-xr.md)).
- Rendering re-expresses continuously: `T_local_anchor(t) = T_local_map(t) · T_map_anchor`.

**Correction policy:** loop-closure/reloc corrections update `T_local_map` and keyframe poses;
the rendered world never jumps. Corrections are applied immediately when small, interpolated when
large, and surface as `PAUSED`/degraded anchor state when very large — the policy is explicit and
bounded, never silent ([21 §4.2](../research/21-anchors-persistence-openxr.md)). Honesty note from
the research: no shipping platform guarantees camera-pose smoothness across corrections (ARCore
documents both camera and anchors may jump); the layered design is what *lets* us guarantee it,
because VIO never receives corrections by construction ([20 §11](../research/20-slam-stacks-for-xr.md)).

**OpenXR fit:** `LOCAL`/`STAGE` stay stable through routine map optimization; anchor corrections
appear as changed anchor component poses in update snapshots; the
`XrEventDataReferenceSpaceChangePending` event is reserved for genuine LOCAL/STAGE redefinition
([21 §4.3](../research/21-anchors-persistence-openxr.md)).

## 4. Service topology and seams

Per [ADR 0009](adr/0009-spatial-mapping-architecture.md) (layered, not single-SLAM):

- **VIT stays a thin pose seam.** No map API grows into it; v2.0.1's pose+timing+features surface
  is enough, and its pose queue is single-consumer by contract
  ([20 §2](../research/20-slam-stacks-for-xr.md), §10).
- **The keyframe egress is Basalt's `out_marg_queue`.** `MargData` already carries the
  marginalization Hessian/gradient, keyframe poses/states, and optical-flow observations; today it
  feeds a disk saver (`MargDataSaver`). The fork surface for layer B is a tee/redirect of that
  queue to the mapping service — bounded, in maintained BSD code, and a candidate VIT
  minor-version extension upstream ([20 §3.2](../research/20-slam-stacks-for-xr.md)).
- **The mapping+anchor service is a separate process** — for the frame contract (corrections
  computed where they belong), for duty-cycling/kill-ability, and because it quarantines any
  GPL-derived code behind IPC while Monado stays BSL/BSD in-process
  ([20 §8](../research/20-slam-stacks-for-xr.md)). Its input is a **subscription** to
  {frames, IMU, poses(+features), MargData} — ILLIXR's topic framing, not a bespoke pairwise API
  ([20 §6](../research/20-slam-stacks-for-xr.md)).
- **Internals are Kimera-shaped**: odometry factors in → robust pose-graph optimization
  (Kimera-RPGO is BSD and importable) with outlier-rejected loop factors owning the map estimate;
  Basalt's in-tree NfrMapper (factor recovery + HashBow + PGO) is the starting point, driven
  online instead of batch ([20 §3.3, §7, §9](../research/20-slam-stacks-for-xr.md)).
- **The geometry service** consumes the same fan-out plus keyframe-rate depth at the mapping
  operating point, and publishes planes in Monado's existing `xrt_plane_detector` shapes and mesh
  as dmabuf — the same transport as every other compositor layer source
  ([22 §4.2, §3](../research/22-dense-geometry-no-lidar.md), ADR 0008).

## 5. Relocalization and the boot flow

Two-tier reloc ([23 §3.3, §9](../research/23-relocalization-multisession.md)): classical
BoW/inverted-index place recognition with Bayes-filter temporal accumulation always-on in the
mapping service; learned features (SuperPoint-class + LightGlue) invoked **only at boot and
lost-tracking**, sharing the same retrieve→match→PnP-verify skeleton. Acceptance discipline is
ORB-SLAM3's: ≥50-inlier-class geometric gates plus **multi-frame confirmation before any
anchor-visible action**; wrong-reloc is the unacceptable failure mode, and both studied systems
achieve zero wrong accepts via these gates.

Boot never blocks: VIO initializes in `local` immediately (passthrough + head-locked UI); reloc
runs concurrently and publishes `T_map_local` on confirmation — anchored content *appears*,
nothing *moves*. UX states: `UNLOCALIZED` (anchored content hidden — never shown at guessed
poses) → `LOCALIZED_TENTATIVE` (fade in, interaction disabled) → `LOCALIZED` → `DEGRADED`
(per-anchor confidence, "may have moved" affordance). Budget target 1–2 s, decomposed in
[23 §5](../research/23-relocalization-multisession.md); the learned-tier NPU numbers are
explicitly unmeasured (no published Snapdragon timings exist) and the design degrades gracefully
to classical-only + multi-condition maps if a device tier misses the budget.

**Illumination robustness is multi-condition mapping** (the verified Labbé & Michaud 2022
result): every session the user wears the device is passively a mapping pass at a new light
level; merged sessions lift even binary features to high reloc rates at any hour
([23 §4.1](../research/23-relocalization-multisession.md)).

## 6. Data model, store, and privacy

The store is **RTAB-Map-shaped, not ORB-SLAM3-shaped** ([23 §2.3, §10](../research/23-relocalization-multisession.md)):
a versioned transactional SQLite-class database with in-tree schema migrations — graph
(nodes/links with information matrices), descriptors (words/features/global descriptors), and
*optional, default-absent* raw sensor blobs in a separate table — never a monolithic shutdown-time
serialization blob. The full schema sketch is [21 §5.2](../research/21-anchors-persistence-openxr.md);
the reloc payload inventory (descriptors, keypoint geometry, poses, covisibility, vocabulary
identity — **no raw images required**) is [23 §2.2](../research/23-relocalization-multisession.md).

Identity discipline: durable `persist_uuid` ≠ context-local `XrSpatialEntityIdEXT` ≠ process
handles ≠ internal map ids; only the UUID crosses OpenXR sessions
([21 §5.2](../research/21-anchors-persistence-openxr.md)).

**Privacy posture** ([21 §6](../research/21-anchors-persistence-openxr.md)): maps are geometry and
recognizable structure of homes — sensitive by default. On-device only; descriptors-not-images as
the target (construction images bounded and volatile); per-map encryption with device/user-bound
key wrapping (TPM2 + systemd credentials); user-visible list/usage/delete/delete-all; deletion
covers WAL, blobs, backups, stale generations; sharing only via deliberate, separately-encrypted
export with consent UI. Plus the gating bonus: masked dynamic content (people, pets, hands) never
contributes descriptors, so **people are never fingerprinted into the home map**
([23 §7](../research/23-relocalization-multisession.md)).

## 7. Geometry without LiDAR

The representation ladder and policies are [doc 22](../research/22-dense-geometry-no-lidar.md)'s;
normative summary:

- **Planes first (v1):** gravity-constrained RANSAC (1-DoF horizontal height modes; 2-DoF
  vertical (θ,d)) over SLAM landmarks + keyframe depth; CAPE-style growing when dense depth
  exists; published through `xrt_plane_detector.h` shapes (FLOOR/WALL/CEILING/PLATFORM) so the
  standard plane extension works for free. The floor plane is the single highest-value output
  (shadows, placement, boundary).
- **Bounded Vulkan TSDF clipmap (v1.5):** room-scale dense grid (≈1.5 M voxels @5 cm ≈ 12 MB),
  nvblox's projective integration as the compute-shader blueprint, voxblox's dirty-bit
  incremental meshing, mesh delivered as dmabuf. No GPU voxel hashing in v1.
- **Tri-state map state** (observed/inferred/unknown) lives *in the map*, with opposite
  conservatisms per consumer: occlusion — unknown never occludes, inferred occludes feathered;
  boundary — inferred counts occupied, unknown is keep-out.
- **Two operating points, one `DepthFrame` interface:** passthrough (per-frame, stability-biased,
  hole-filling) vs mapping (keyframe-rate, accuracy-biased, hard confidence gate, no filling).
- **Semantics:** 8-label uint8 histogram fusion (Kimera's pattern at 1/10 memory), labels lifted
  to planes/mesh at extraction.
- **Boundary:** floor + play volume + tri-state kept-out volumes; compositor-owned overlay +
  composition-policy breach response (no client cooperation), IMU-rate point probes; threshold
  semantics are part of the contract (the SteamVR +40 cm lesson).

Per-device policy via `spatial.xr.mapping.depthSource`
(`none | flood-ir | active-ir-pattern | tof-sensor | android-backed`): controls the inferred-state
budget and illuminator duty, **not** backend selection (that stays
`spatial.xr.passthrough.depthBackend`). Steam Frame's IR is flood (SNR, not texture — blank walls
still fail at night); Quest 3's dot projector is the no-LiDAR existence proof
([22 §6](../research/22-dense-geometry-no-lidar.md)).

## 8. The OpenXR surface and upstream posture

Implement the ratified `XR_EXT_spatial_entity` + `_anchor` + `_plane_tracking` + `_persistence`
(+`_persistence_operations`) family in Monado — currently absent there, while Android XR ships it
([21 §2, §7](../research/21-anchors-persistence-openxr.md)). Reusable foundations already in
Monado: **`XR_EXT_future` is implemented** (`oxr_future_ext`, async IPC futures), and the old
plane-detection plumbing (`xrt_plane_detector.h`, IPC arrays) is reusable *below* a new canonical
plane tracker that owns association and stable entity IDs — the old request-scoped IDs cannot be
wrapped directly ([21 §7.3](../research/21-anchors-persistence-openxr.md)).

Upstream in dependency order ([21 §7.4](../research/21-anchors-persistence-openxr.md)):
(1) generic entity/snapshot xrt interfaces + oxr handles; (2) the plane adapter proving
discovery/update/snapshot semantics; (3) anchor service interface + map/local poses;
(4) persistence contexts + encrypted-store adapter; (5) persistence operations + policy hooks.
Storage stays behind an implementation-neutral xrt interface; distro-specific encryption/UI stays
service-side.

## 9. Scheduling, power, dynamic content

- **Duty-cycling** (ADR 0008): map on novelty/change triggers; heavy refinement (teacher-grade
  depth, mesh cleanup, semantic re-pass, multi-session merge optimization) only while docked.
- **Dynamic-object gating at three stages** ([23 §7](../research/23-relocalization-multisession.md)):
  VIO feature detection (the existing `masks_sink`→`vit_mask_t` box path, honored in Basalt's
  keypoint detector), map updates (drop masked features; skip mostly-masked keyframes; record
  mask coverage), and reloc queries. Mask sources generalize from hand boxes to Tier 3
  person/pet segmentation.
- **Latency-critical exception:** boundary distance probes run at IMU rate on CPU — queries, never
  reconstruction ([22 §8](../research/22-dense-geometry-no-lidar.md)).

## 10. Packaging and evaluation status

- `basalt-monado` is packaged in nixpkgs (0-unstable-2025-09-25, cache-served; ships `basalt_vio`,
  `basalt_mapper`, calibration tools) — layer A has zero packaging cost.
- ORB-SLAM3 is not in nixpkgs and hard-requires Pangolin (also unpackaged; `-Werror` friction on
  new GCC) plus vendored Thirdparty — a real but bounded cost, relevant only as an **evaluation
  baseline**, not production ([20 §8–9](../research/20-slam-stacks-for-xr.md)).
- The originally-planned EuRoC replay and Atlas save/reload validation spikes were **cancelled per
  scope** (research/architecture task); they remain the first *implementation-phase* validation
  steps, with the measurement protocol specified in
  [23 §8](../research/23-relocalization-multisession.md).

## 11. Phasing

| Phase | Deliverable | Gate |
|---|---|---|
| **M1: session anchors** | mapping service ingesting MargData tee; anchors parent-relative to keyframes; `T_local_map` estimator; shell pins windows within one session (no store) | window stays put over a 30-min session incl. loop closures; rendered world never jumps |
| **M2: persistence** | encrypted store; classical reloc; boot flow + UX states; `LOCAL_ANCHORS_EXT` scope | reboot → relocalized ≤2 s in a mapped room; zero wrong-reloc on the eval protocol |
| **M3: geometry** | gravity-RANSAC planes; floor-plane shadows in compositor; boundary v1 | plane snap + shadow demo; boundary breach → passthrough without client cooperation |
| **M4: standard surface** | `XR_EXT_spatial_entity`/`_anchor`/`_plane_tracking`/`_persistence` in Monado; upstream MRs | conformance-shaped tests pass; first upstream review round |
| **M5: dense** | Vulkan TSDF clipmap + mesh occlusion; semantics; learned reloc tier | measured Adreno budgets; multi-condition reloc matrix |

## 12. Open questions (consolidated)

Carried from the research docs, deduplicated — the blocking ones first:

1. **NPU reloc reality** (23 §11): SuperPoint/LightGlue latency/warm-up on XR2-class HTP — no
   published numbers; first implementation-phase measurement.
2. **Basalt export surface** (20 §2.3, 22 §11): MargData tee vs `POSE_FEATURES` forwarding vs a
   VIT minor-version keyframe extension — needs an upstream conversation with the basalt-monado
   maintainer.
3. **Adreno compute budgets** (22 §11): TSDF integration + marching cubes cost and bandwidth
   contention with the compositor.
4. **Correction-policy tuning** (21 §10): immediate vs interpolated `T_local_map` by magnitude;
   what triggers `PAUSED`.
5. **Vocabulary strategy** (23 §11): fixed pretrained vs incremental dictionary; interacts with
   store size and cross-device sharing.
6. **Screens/TVs** (23 §11): geometrically stable, appearance-unstable — mask content or rely on
   multi-condition robustness; no studied system addresses it.
7. **Depth-sensor reachability** on Galaxy XR/PFDM (22 §11): `tof-sensor` vs `android-backed`.
8. **Repeated-looking rooms** (21 §10): disambiguation without images and without guessing.
9. **Key recovery** after TPM/motherboard replacement; deletion SLA across backups (21 §10).
10. **Per-pixel masks through VIT** (23 §11): whether Tier 3 pixel masks justify an interface
    extension or box-fitting suffices.
