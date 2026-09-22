# 23 — Relocalization & multi-session persistence: recognizing the room at boot

**Date:** 2026-09-22.
**Question:** how does a headset that mapped a room yesterday recognize it at boot today — within
~1–2 s, across lighting changes and small scene changes — and hand a trustworthy `T_map_local` to
the anchor service so that apps reappear exactly where the user left them?

This is layer C of the Tier 4 stack. Layer A (VIO) stays Monado+Basalt in the local frame; a
separate mapping/anchor service owns layers B+C (the layered-vs-single-SLAM tradeoff is doc 20's
subject, in progress in parallel; the persistent-store design is doc 21's, also parallel — this doc
covers what relocalization *needs from* that store, not its full design). The frame contract is
already established: relocalization outputs `T_map_local`; anchors re-express through it; the
rendered world, which lives in the local frame, never jumps.

Sources: `references/orbslam3/`, `references/rtabmap/`, `references/hloc/`,
`references/lightglue/`, `references/monado/`, `references/basalt-monado/`,
`references/openxr-docs/`, plus web-verified papers. Claims are marked **verified** (primary
source in hand) or **inferred** (our extrapolation) throughout — doc
[16](16-perception-claims-audit.md) is the standing reminder of why.

---

## 1. Purpose

Persistence has three failure modes, in increasing order of badness: reloc is *slow* (user stares
at an empty room for 10 s), reloc *fails* (apps never come back; user re-places everything), reloc
is *wrong* (apps appear embedded in a wall — worse than not appearing at all). The design below
treats wrong-reloc as the unacceptable one: every accept path requires geometric verification, and
confidence is a first-class output.

The two studied classical systems answer complementary questions. ORB-SLAM3 answers *what must be
serialized and how single-shot relocalization works mechanically*. RTAB-Map answers *how to bound
memory/time as maps grow across sessions, how to persist to a real database, and how to survive
illumination change* — its 2022 multi-session paper is the single most relevant published
experiment for our use case (an apartment, mapped repeatedly at different light levels, then
relocalized against). hloc/LightGlue answer *what a learned reloc path buys and costs* when it only
has to run at boot.

## 2. Classical reloc internals

### 2.1 ORB-SLAM3: the reloc pipeline, mechanically

Relocalization is a three-stage funnel (all **verified** from source):

**Stage 1 — candidate retrieval** (`DetectRelocalizationCandidates`,
`references/orbslam3/src/KeyFrameDatabase.cc:733`). The query frame's ORB descriptors are
quantized into a bag-of-words vector against a *fixed, pre-trained vocabulary*; an inverted file
(word → keyframes list) votes for keyframes sharing words. Keyframes with fewer than 0.8× the
maximum shared-word count are dropped (`KeyFrameDatabase.cc:769`), survivors are scored by
vocabulary similarity, then scores are *accumulated over each candidate's top-10 covisible
keyframes* (`KeyFrameDatabase.cc:799`) — place recognition is done on covisibility groups, not
single images — and groups above 0.75× the best accumulated score are returned
(`KeyFrameDatabase.cc:824`), filtered to the queried map (`KeyFrameDatabase.cc:833-835`).

**Stage 2 — descriptor matching** (`Tracking::Relocalization()`,
`references/orbslam3/src/Tracking.cc:3609`). Each candidate is matched by BoW-guided ORB matching;
candidates with <15 matches are discarded (`Tracking.cc:3649`). Survivors get an MLPnP RANSAC
solver (min 6 points, 300 max iterations — `Tracking.cc:3657`).

**Stage 3 — pose verification.** RANSAC pose → motion-only bundle adjustment → if inliers <50,
two rounds of projection-guided match widening (search window 10 then 3 px,
`Tracking.cc:3724-3752`). Accept only at ≥50 inliers (`Tracking.cc:3757`). This ≥50-inlier
discipline is the wrong-reloc firewall and we should keep the shape of it.

**Lost-tracking policy** (**verified**): losing tracking in a map with >10 keyframes enters
`RECENTLY_LOST` (`Tracking.cc:1966-1971`); visual reloc is retried for 3 s
(`Tracking.cc:2006`), IMU dead-reckoning coasts up to `time_recently_lost` = 5 s
(`Tracking.cc:48`, `Tracking.cc:1993-1998`); after that, `LOST` → a *new map* is spawned in the
Atlas rather than blocking (`Tracking.cc:2014-2031`, `CreateMapInAtlas` at `Tracking.cc:2662`).
Mapping continues in the new map; re-attachment to the old one is deferred to the place-recognition
thread. This "never block on reloc, merge later" posture is exactly right for a headset boot.

**Map merging** (**verified**): the loop-closing thread runs place recognition on every new
keyframe (`LoopClosing::NewDetectCommonRegions`, `references/orbslam3/src/LoopClosing.cc:324`).
A candidate common region must be geometrically re-confirmed in **3 consecutive keyframes**
(`LoopClosing.cc:396`) before `MergeLocal` (`LoopClosing.cc:1215`) welds the maps and the Atlas
switches active map (`LoopClosing.cc:1550`). Temporal consistency before topology-changing
operations is a pattern to adopt verbatim.

### 2.2 What ORB-SLAM3 must serialize for reloc to survive a reboot

The Atlas save/load path (**verified**) is `System::SaveAtlas` / `LoadAtlas`
(`references/orbslam3/src/System.cc:1403`, `System.cc:1445`) — a boost binary archive of the whole
Atlas. What actually goes in it defines the minimum viable reloc store:

- **Per keyframe** (`references/orbslam3/include/KeyFrame.h:57-193`): keypoints + undistorted
  keypoints, the full ORB descriptor matrix (`KeyFrame.h:128`), **precomputed BoW and feature
  vectors** (`KeyFrame.h:130-131`), pose, camera calibration, the covisibility graph as
  id→weight maps (`KeyFrame.h:154`), spanning tree + loop/merge edges (`KeyFrame.h:156-160`),
  and IMU bias/preintegration (`KeyFrame.h:183-192`).
- **Per map point** (`references/orbslam3/include/MapPoint.h:92`): 3D position + representative
  descriptor + observation ids.
- **Atlas** (`references/orbslam3/include/Atlas.h:53-69`): the map list, camera models, and the
  static ID counters (so new sessions don't collide with old ids).
- **KeyFrameDatabase** (`references/orbslam3/include/KeyFrameDatabase.h:52-55`): only inverted-file
  keyframe *ids*. On load, `Map::PostLoad` re-links all pointers and **re-adds every keyframe to
  the database, rebuilding the inverted index from the stored BoW vectors**
  (`references/orbslam3/src/Map.cc:427`, re-add at `Map.cc:469`).
- **Vocabulary**: not stored in the archive — the archive records the vocabulary file's MD5
  checksum, and load *refuses* if the currently-configured vocabulary doesn't match
  (`System.cc:1490-1497`). The vocabulary is part of the map format version, effectively.

Two consequences. First, **no raw images are needed for classical reloc** — descriptors,
keypoint geometry, poses, covisibility, and the vocabulary suffice, which is the privacy posture
doc 21 wants. Second, the ORB-SLAM3 *container* (one monolithic boost archive, all-or-nothing
load, no schema migration, pointer-graph rebuild at load) is a serialization of runtime state, not
a store design — take the *inventory*, reject the *format* (§10).

### 2.3 RTAB-Map: bounded-time reloc and a real persistence schema

RTAB-Map's loop-closure/reloc detector is appearance-based with temporal filtering
(**verified**): per frame, BoW likelihood against working-memory locations → likelihood
normalization → a **Bayesian filter over locations** (`BayesFilter::computePosterior`,
`references/rtabmap/corelib/src/BayesFilter.cpp:150`; used at
`references/rtabmap/corelib/src/Rtabmap.cpp:2140`) → highest hypothesis accepted only above
`Rtabmap/LoopThr` (default 0.11,
`references/rtabmap/corelib/include/rtabmap/core/Parameters.h:209`), *and* only after epipolar
geometric verification (`Rtabmap.cpp:2203`) and a hypothesis-ratio test (`Rtabmap.cpp:2207`).
The Bayes filter means a location's probability builds over consecutive frames — spurious
single-frame similarity doesn't fire. That is the right always-on background verifier; for
single-shot boot reloc it must be replaced by stricter per-frame geometric acceptance.

**WM/LTM memory management** (**verified**): RTAB-Map bounds per-frame update time by keeping only
a working memory (WM) of locations in RAM and transferring the rest to long-term memory (LTM = the
SQLite database). When update time exceeds `Rtabmap/TimeThr` or WM size exceeds
`Rtabmap/MemoryThr` (`Parameters.h:192-193`), `Memory::forget` transfers the least-recently-useful
nodes out (`Rtabmap.cpp:4587-4601`, `references/rtabmap/corelib/src/Memory.cpp:2441`); when a
hypothesis lands near an LTM region, its neighborhood is *retrieved* back into WM
(`Memory::reactivateSignatures`, `Memory.cpp:7241`, invoked at `Rtabmap.cpp:2663`). Reloc cost is
O(working set), not O(lifetime map) — the only studied architecture that stays real-time as a home
accumulates months of sessions.

**The SQLite schema** (**verified**,
`references/rtabmap/corelib/src/resources/DatabaseSchema.sql.in`) is the concrete store precedent
for doc 21:

- `Node(id, map_id, weight, stamp, pose, label, gps, env_sensors, …)` (lines 16-29) — note
  `map_id` (multi-session native) and `env_sensors` (side-channel hints have a schema slot).
- `Data(id, image, depth, …)` (lines 31-52) — raw sensor blobs are a *separate table* from the
  graph, and `Mem/BinDataKept` (`Parameters.h:217`) can disable storing them entirely: the
  descriptors-only privacy mode already exists as a supported configuration.
- `Link(from_id, to_id, type, information_matrix, transform)` (lines 54-63) — typed edges
  (neighbor / global closure / landmark…) with full 6×6 information matrices.
- `Word(id, descriptor)` + `Feature(node_id, word_id, pos_x, pos_y, …, depth_x/y/z, descriptor)`
  (lines 66-89) — the visual vocabulary and per-node keypoints, i.e. exactly the reloc payload.
- `GlobalDescriptor(node_id, type, data)` (lines 91-97) — a slot for NetVLAD-class retrieval
  vectors per node.
- `Admin(version, …, opt_last_localization, dictionary_index)` (lines 119-139) — schema version,
  the last-localized pose (warm-start hint), and a serialized dictionary index.
- Versioned migrations exist in-tree
  (`references/rtabmap/corelib/src/resources/backward_compatibility/DatabaseSchema_0_16_0.sql` …
  `_0_22_0.sql`) — map-format migration is a solved, precedented problem.

Unlike ORB-SLAM3, RTAB-Map's BoW vocabulary is **incremental** (built online,
`Parameters.h:256`), serialized with the map (`dictionary_index`), so there is no pretrained-
vocabulary version-lock — at the cost of a vocabulary that grows with the map (Table 5 of the 2022
paper: 181k–331k words after six sessions).

## 3. Learned reloc at boot-time budgets

### 3.1 hloc: the pipeline shape and the index it builds

hloc (**verified** from `references/hloc/README.md` and module layout) is coarse-to-fine
localization: (1) global retrieval descriptors (NetVLAD et al.) + local features (SuperPoint
et al.) extracted for all database images (`references/hloc/hloc/extract_features.py`); (2) a
COLMAP SfM model triangulated from matched database pairs; (3) per query: retrieval of top-K
database images (`hloc/pairs_from_retrieval.py`), learned matching (SuperGlue/LightGlue,
`hloc/match_features.py`), and PnP against the 3D model (`hloc/localize_sfm.py`). The persistent
index = HDF5 feature/descriptor files + the SfM model — again, descriptors and geometry, not
images. Structurally this is the *same* funnel as ORB-SLAM3 reloc (retrieve → match → PnP) with
every stage's component swapped for a learned one; the two paths can share one skeleton.

Robustness deltas are large and published (**verified**, `references/hloc/README.md:100-113`):
Aachen Day-**Night** localization (0.25m/0.5m/5m thresholds): SuperPoint+SuperGlue
86.7/93.9/100 vs SuperPoint+NN-matching 75.5/86.7/92.9 — the learned *matcher* alone adds ~11
points at the strictest threshold at night. InLoc (indoor, large viewpoint/appearance change):
46.5/65.7/78.3 vs 39.9/55.6/67.2. The [long-term visual localization
benchmark](https://www.visuallocalization.net/benchmark) is the standing leaderboard for these
conditions.

### 3.2 LightGlue: cost of the learned matcher

**Verified** from `references/lightglue/README.md:126-139`: on an RTX 3080, SuperPoint+LightGlue
runs at ~150 FPS @ 1024 keypoints and ~50 FPS @ 4096 (4-10× faster than SuperGlue); on a desktop
CPU (i7-10700K), ~20 FPS @ 512 keypoints (≈50 ms/pair). Adaptive depth/width makes easy pairs
cheaper. An ONNX/TensorRT/OpenVINO export path exists
([LightGlue-ONNX](https://github.com/fabio-sim/LightGlue-ONNX), linked from README), with
[measured 2-4× gains over compiled PyTorch on an RTX
4080](https://fabio-sim.github.io/blog/accelerating-lightglue-inference-onnx-runtime-tensorrt/).

**What is NOT verified — flagged deliberately:** this research found **no published end-to-end
SuperPoint/LightGlue timings on Snapdragon/XR-class NPUs or Jetson-class SoCs**. A [Jetson Orin NX
TensorRT port exists](https://github.com/qdLMF/LightGlue-with-FlashAttentionV2-TensorRT) but
publishes no numbers; Qualcomm AI Hub does not list SuperPoint/LightGlue as profiled models. Every
mobile-NPU latency figure in planning documents must therefore come from our own Phase D2
measurements, on the actual QNN/HTP path whose availability caveats doc
[16](16-perception-claims-audit.md) already audited.

### 3.3 The two-tier design: is "learned at boot only" feasible?

The design question: classical ORB/BoW always-on, learned features invoked *only* at boot and
lost-tracking, with a seconds-scale budget on NPU/GPU.

The strongest *verified* datapoint that the budget is comfortable comes from the RTAB-Map 2022
paper (§4): on a 2019 laptop (i7-9750H + GTX 1650), full per-frame reloc against a six-session
apartment map costs **SuperPoint 85 ms detection + 11 ms loop-closure detection + 24 ms
transformation estimation ≈ 120 ms** (SuperGlue matching adds ~40 ms) — and BoW retrieval cost
grew only ~4 ms going from a single-session to a six-session map. Even the *desktop-CPU* LightGlue
number (~50 ms/pair @512 kpts) permits matching ~10 retrieval candidates in ~0.5 s.

**Inferred** (arithmetic, not measurement): one query frame needs 1× feature extraction +
1× global descriptor + K× matching + PnP. With K=5–10 and XR2-class NPU/GPU somewhere within
2–10× of the above reference hardware, boot reloc lands at roughly 0.3–1.5 s of compute — inside
the 1–2 s target, *if* model load/warm-up is amortized (QNN serialized contexts, per doc 16) and
*if* the NPU path exists on the device tier. Both ifs are D2 spike measurements, not assumptions.
The fallback if learned features miss the budget on a given device: classical-only reloc with
multi-condition maps, which §4 shows is genuinely viable — the two-tier design degrades gracefully
to tier one.

Power is the reason learned features are boot-only (**inferred** but uncontroversial): always-on
ORB/BoW place recognition costs ~10 ms/keyframe-class CPU work (RTAB-Map Table 5), while always-on
NPU inference at camera rate competes with hand tracking, segmentation, and reprojection budgets
(Tier 3). Boot/lost-tracking invocation makes the learned path's *duty cycle* approximately zero.

## 4. Robustness evidence + mitigations

### 4.1 The multi-session result

[Labbé & Michaud 2022, "Multi-Session Visual SLAM for Illumination-Invariant Re-Localization in
Indoor Environments," Frontiers in Robotics and AI
9:801886](https://www.frontiersin.org/journals/robotics-and-ai/articles/10.3389/frobt.2022.801886/full)
(**verified** — full text reviewed; [reproduction scripts in-tree at
`archive/2022-IlluminationInvariant`](https://github.com/introlab/rtabmap/tree/master/archive/2022-IlluminationInvariant)):

- Setup: a real apartment, **six mapping + six localization sessions at ~30-min intervals through
  sunset** (16:46→19:42) on a Google Tango phone — natural light fading, lamps switching on,
  auto-exposure swings. Features compared: SURF, SIFT, BRIEF, BRISK, KAZE, DAISY, SuperPoint
  (±SuperGlue), inside full RTAB-Map SLAM.
- Single-session maps: reloc works well only near the mapping session's own light level (the
  diagonal); day↔night cross-relocalization is worst; binary features (BRIEF/BRISK) degrade most;
  **SuperPoint is the most illumination-robust**.
- **Merging all six sessions into one multi-session map lifts every feature to high reloc rates at
  every hour — even BRIEF.** The merged map answers "which lighting condition am I in" implicitly
  through retrieval; no explicit condition selection is needed.
- SuperPoint(+SuperGlue) achieves near-equal performance with *fewer* sessions — learned features
  buy fewer required mapping passes, classical features + more sessions reach the same endpoint.
- Graph reduction (merge redundant nodes on accepted loop closures) removes 65–84% of nodes with
  only slightly worse accuracy; with SuperGlue the six-session reduced map ends up *smaller than
  most single-session maps* (198 nodes, 84% reduction).
- Accepted relocalizations had **2–6 cm pose-jump error**; no wrong relocalizations were accepted
  in any run (thresholds + inlier gates held).
- Costs as in §3.3; multi-session RAM is up to 6× single-session without reduction.

This converts our illumination strategy from speculation to cited practice: **multi-condition
mapping is the primary robustness mechanism; learned features reduce how many conditions must be
captured.** For a headset this is nearly free — every session the user wears the device in the
room *is* a mapping pass at a new light condition; the store accretes conditions passively.

### 4.2 Scene change and staleness

Furniture moves; the 2022 paper's discussion section (verified) sketches the policies we should
formalize: (a) continuous map updating grows RAM even with reduction — bound it with WM/LTM;
(b) record **per-node last-relocalized timestamps** and expire nodes that haven't been matched for
weeks (the renovated-room case); (c) feature-persistence modeling
([Rosen et al. 2016](https://doi.org/10.1109/ICRA.2016.7487237)) is the principled version of (b).
**Inferred** for our design: staleness detection at reloc time — if pose verification succeeds but
inlier count against a region's old keyframes trends down session-over-session while VIO-local
reconstruction disagrees, mark the region stale and prefer re-mapping it; and anchor confidence
should be per-anchor (nearest-keyframes inlier support), not global, so one moved bookshelf
degrades one anchor's confidence, not the room's.

## 5. Boot-to-localized flow

The cold-start sequence, with the 1–2 s budget decomposed. Rendering never waits: VIO initializes
in the local frame immediately (headset boots into passthrough + head-locked UI); reloc runs
concurrently and, on success, publishes `T_map_local` — anchored content *appears*, nothing
*moves*.

| Stage | What happens | Budget (target) | Status |
|---|---|---|---|
| 0. Map manifest load | open store, read map headers + hints (last-known map, Wi-Fi/BT observations, `opt_last_localization`-style warm start) | <50 ms | inferred |
| 1. Candidate map selection | rank maps by hints; load retrieval index of top map(s) (mmap-friendly; SQLite precedent) | <200 ms | inferred |
| 2. Query capture | wait for first well-exposed tracking frames (auto-exposure settle) | 100–300 ms | inferred (hardware-dependent) |
| 3. Coarse place recognition | global descriptor (or BoW) → top-K keyframes | 10–50 ms | inferred; BoW ~11 ms verified on laptop CPU (§3.3) |
| 4. Feature match | learned extract (once) + match K candidates | 150–700 ms | partially verified (§3.2–3.3); NPU numbers = D2 |
| 5. Pose verify | PnP RANSAC + refine; ≥50-inlier-class gate; confirm on a 2nd frame ~100 ms later | <100 ms | verified-precedent (ORB-SLAM3 gates) |
| 6. Handoff | publish `T_map_local` + covariance/confidence to anchor service | ~0 | design |

Total ≈ 0.5–1.4 s **inferred**; stage 4 dominates and is the D2 measurement. Two design rules from
the classical systems: the *two-frame confirmation* in stage 5 is the single-shot analogue of
RTAB-Map's Bayes filter and ORB-SLAM3's 3-consecutive-keyframe merge rule — one frame's PnP
success is never enough to move persistent content; and failure at any stage falls through to
"keep trying in background at low rate" (ORB-SLAM3's new-map-now-merge-later posture), never to a
blocking retry loop.

**Fallback UX states** (design, informed by the failure-mode ranking in §1):

- `UNLOCALIZED` — anchored content hidden; a lightweight head-locked affordance ("looking for your
  space…") if the user opens the anchors UI. Do **not** show anchored content head-locked or at
  guessed poses; wrong placement teaches the user the system is untrustworthy.
- `LOCALIZED_TENTATIVE` (single-frame verify passed, awaiting confirmation / low inliers) —
  content may fade in at mapped poses but interaction affordances stay disabled; if confirmation
  fails, fade out (content that never enabled interaction can vanish without data loss).
- `LOCALIZED` — full confidence, anchors live, background re-verification active.
- `DEGRADED` (background verifier's inlier support dropped, e.g. scene changed) — per-anchor
  confidence drops; anchors in unsupported regions get a subtle "may have moved" affordance
  rather than silently floating.

**Background continuous re-verification** (design, RTAB-Map-precedented): after boot, the mapping
service keeps scoring current keyframes against the persistent map at low rate (Bayes-filter
accumulation, `Rtabmap.cpp:2140-2220` shape). This catches wrong-reloc (posterior collapses →
retract to `UNLOCALIZED` *for anchors*, never yanking the rendered local frame), refines
`T_map_local` smoothly, and feeds per-region freshness stats for §4.2 staleness.

The API surface this ultimately serves is now standardized: `XR_EXT_spatial_persistence` gives
apps UUID-identified entities across sessions with runtime-enumerable storage scopes and an
explicit permission model
(`references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_persistence.adoc`,
overview + `xrEnumerateSpatialPersistenceScopesEXT`), with `ext_spatial_anchor.adoc` and
`ext_spatial_persistence_operations.adoc` alongside. Monado today implements none of it — spatial
anchor/persistence symbols appear only in its vendored OpenXR headers
(`references/monado/src/external/openxr_includes/openxr/openxr.h`), no state-tracker
implementation (**verified** by absence, grep across `monado/src`). Basalt likewise has no
relocalization or map persistence (**verified** by absence: no reloc/serialization code in
`references/basalt-monado/src`) — confirming doc 20's premise that layer C cannot be extracted
from the existing VIO and must live in the mapping service.

## 6. Multi-map organization

- **Per-room vs whole-home:** whole-home as the logical unit, physically organized as per-session
  sub-maps in one store (RTAB-Map's `Node.map_id` precedent). Don't hard-partition by room: the
  2022 paper's key observation is that *retrieval selects the right sub-map implicitly* — the
  best-matching keyframes are by construction from the right room and right lighting. Rooms fall
  out of covisibility clustering; they don't need to be a storage boundary.
- **Map selection at boot:** order candidate maps by (1) last-localized-in (Admin
  `opt_last_localization` precedent), (2) radio environment — the schema slot exists
  (`Node.gps`/`env_sensors`, `DatabaseSchema.sql.in:25-26`); Wi-Fi BSSID/BT beacon sets are the
  indoor analogue (**inferred**; no studied system implements Wi-Fi-hinted map selection, and
  radio scans need OS permissions — open question). Selection only orders the search; retrieval
  still decides.
- **Merging:** ORB-SLAM3's pattern verbatim — sessions start as separate sub-maps; when place
  recognition finds a 3×-confirmed common region, weld with a proper joint optimization
  (`LoopClosing.cc:324`, `LoopClosing.cc:1215`). Multi-condition maps (§4.1) are exactly merged
  sub-maps kept as parallel appearance layers over shared geometry.
- **Size growth:** three verified mechanisms compose — ORB-SLAM3-style redundant-keyframe culling
  at mapping time (≥90% of observed points seen in ≥3 other keyframes → cull,
  `references/orbslam3/src/LocalMapping.cc:902-918`); RTAB-Map graph reduction at merge time
  (65–84% node removal, §4.1, `Mem/ReduceGraph` at `Parameters.h:230`); staleness expiry over
  weeks (§4.2). WM/LTM keeps runtime cost flat regardless of store size.
- **Versioning/migration:** adopt RTAB-Map's practice: schema version in the store, in-tree SQL
  migrations per version bump. Descriptor-model identity (vocabulary checksum, ORB-SLAM3
  `System.cc:1490-1497`; SuperPoint weight hash, equivalently) is part of the format version — a
  descriptor model change invalidates descriptors but *not* geometry: keyframe poses, covisibility
  and anchor attachments survive; descriptors re-extract only if raw keyframe images were retained,
  which privacy mode forbids — so a model bump without images means a background re-mapping
  session, a real cost to schedule deliberately (**inferred**).

## 7. Dynamic-object gating

The existing hook (**verified**): Monado's SLAM tracker accepts hand bounding boxes via
`xrt_hand_masks_sink` (`references/monado/src/xrt/auxiliary/tracking/t_tracker_slam.cpp:277`,
push at `t_tracker_slam.cpp:1204-1212`), attaches the latest masks to each camera frame as
axis-aligned rects (`t_tracker_slam.cpp:1308-1331`), through the VIT interface's `vit_mask_t`
(`references/basalt-monado/thirdparty/vit/vit_interface.h:178-181`). Basalt honors them at
*feature detection*: corners inside masks are skipped
(`references/basalt-monado/src/utils/keypoints.cpp:187`). Doc
[15](15-hand-segmentation-matting.md) §2 already established these are bounding boxes, not
segmentation, and that Tier 3's segmentation service is the upgrade path.

Generalization for layer C (design):

1. **VIO feature gating** (exists): extend mask sources from hand boxes to person/pet/large-moving-
   object boxes from the Tier 3 segmentation service, at whatever rate it runs — the VIT interface
   needs no change for boxes; per-pixel masks would (open question, doc 20's territory).
2. **Map-update gating** (new, ours): the mapping service must apply the *same* masks when
   selecting features for keyframes bound to the persistent store, plus stricter policies —
   drop features on mask classes {person, pet, hands, screens-with-changing-content}; skip
   keyframe insertion entirely when masked area exceeds a threshold (a frame that is mostly
   person is a bad place fingerprint); and record per-keyframe mask coverage so verification can
   discount regions that were occluded at mapping time (**inferred** policies; the RTAB-Map paper's
   static-apartment assumption is exactly what these defend).
3. **Reloc-time gating**: query-frame features under dynamic masks are excluded before retrieval
   and matching — a person standing in front of the bookshelf at boot should cost candidates, not
   produce corrupted descriptors. Cheap, since segmentation already runs for passthrough (Tier 3).
4. **Privacy interaction**: masked regions never contribute descriptors to the persistent store,
   which conveniently means *people are never fingerprinted into the home map* — a stronger
   privacy statement than descriptor-only storage alone (design; coordinate with doc 21).

## 8. Metrics + what spike D2 must record

Metrics (definitions verified from the cited literature's practice):

- **Reloc success rate**: % of query frames correctly relocalized (RTAB-Map 2022's metric), with
  *correct* defined by ground truth or by downstream inlier support; report per lighting-condition
  pair (map condition × query condition matrix, as the paper's Figure 4).
- **Time-to-localized**: wall clock from first camera frame to confirmed `T_map_local`, reported
  as success@{1 s, 2 s, 5 s} plus the full latency decomposition of §5's table.
- **Pose error**: translation/rotation error of `T_map_local` vs ground truth; on EuRoC/TUM-VI
  style data with GT, absolute; on our own recordings, reloc-jump magnitude (the 2–6 cm figure of
  §4.1 is the bar) and anchor-reprojection error in pixels — the user-visible quantity.
- **Wrong-reloc rate**: acceptances later retracted by the background verifier; target ≈0 at the
  cost of success rate (both classical systems achieve 0 on their datasets via inlier gates).
- **Store growth**: bytes and node count per session, before/after culling+reduction.

Spike D2 protocol (concrete, runnable):

1. **Datasets**: EuRoC MH + TUM-VI room sequences for GT pose error (split sequences: map on first
   half, reloc on second); the [RTAB-Map multi-session apartment
   dataset](https://github.com/introlab/rtabmap/tree/master/archive/2022-IlluminationInvariant)
   for illumination replication; plus our own headset recordings of one room across ≥4 lighting
   states and a furniture-move perturbation.
2. **Systems**: (a) RTAB-Map localization-only mode (`Mem/IncrementalMemory=false`,
   `Parameters.h:227`) as classical baseline; (b) hloc-style SuperPoint+LightGlue reloc over the
   same keyframes; (c) the two-tier combination.
3. **Record**: everything in the metrics list, per stage of §5's table, on target-class hardware —
   including the currently-unverifiable NPU numbers (SuperPoint + LightGlue via QNN/HTP and GPU
   fallback), model load and first-inference warm-up time, and peak RAM for a six-session map.
4. **Deliverable**: the measured §5 budget table replacing every "inferred" tag, and a
   go/no-go on learned-at-boot per device tier.

## 9. Adopt

1. **Two-tier relocalization**: always-on classical place recognition (BoW/inverted-index +
   Bayes-filter temporal accumulation, RTAB-Map shape) in the mapping service; learned
   SuperPoint-class features + LightGlue matching invoked only at boot and lost-tracking. Both
   tiers share the retrieve→match→PnP-verify skeleton (§2.1, §3.1).
2. **The ORB-SLAM3 verification discipline**: ≥50-inlier-class geometric acceptance, projection-
   widening rounds, multi-frame confirmation before any topology/anchor-visible action
   (`Tracking.cc:3609-3777`, `LoopClosing.cc:396`).
3. **Never block, never jump**: VIO boots in local frame immediately; reloc publishes
   `T_map_local` when confirmed; lost-tracking spawns sub-map now, merges later
   (`Tracking.cc:2014-2031`).
4. **RTAB-Map's store shape** for doc 21: SQLite-class store, graph/descriptor tables separate
   from (optional, default-absent) raw sensor blobs (`Mem/BinDataKept=false` posture), typed links
   with information matrices, `GlobalDescriptor` slot, `Admin` version + last-localization,
   in-tree migrations (§2.3).
5. **Multi-condition mapping as the primary illumination mitigation**: passively accrete sessions
   across lighting states, merge via confirmed common regions, keep parallel appearance layers
   over shared geometry ([Labbé & Michaud 2022](https://www.frontiersin.org/journals/robotics-and-ai/articles/10.3389/frobt.2022.801886/full)).
6. **Bounded memory**: WM/LTM retrieval-based paging (`Memory.cpp:7241`), keyframe culling
   (`LocalMapping.cc:902`), graph reduction, per-node last-relocalized staleness expiry.
7. **Mask gating at all three stages** — VIO detection (existing `masks_sink` hook), map updates,
   and reloc queries — with person/pet content never entering the persistent store (§7).
8. **`XR_EXT_spatial_persistence`** as the app-facing contract to implement toward
   (UUID-identified entities, enumerable scopes, permissioned).

## 10. Reject

- **Monolithic runtime-state serialization as the store format** (ORB-SLAM3 `.osa` boost
  archives): all-or-nothing load, no incremental access, no migration story, pointer-graph rebuild
  cost at every boot. Keep its *content inventory* (§2.2), reject the container.
- **Always-on learned features**: duty-cycle cost with no reloc benefit over classical + temporal
  filtering once localized; learned tier is boot/lost-only (§3.3).
- **End-to-end pose regression** (PoseNet-class): the 2022 paper's discussion and the long-term
  localization literature both place it below structured retrieve-match-verify indoors; no
  geometric verification means no wrong-reloc firewall.
- **Raw images in the persistent store by default**: not needed by any studied reloc path
  (descriptors + geometry suffice, §2.2, §3.1); the exception (re-extracting descriptors after a
  model upgrade) must be an explicit opt-in with its own retention policy, not a default (doc 21).
- **Hard per-room map partitioning with explicit selection UI**: retrieval already selects
  sub-maps implicitly and more robustly (§6).
- **Blocking boot on relocalization**: the UX states of §5 exist precisely so boot never waits.
- **Unverified NPU benchmark numbers in planning**: every mobile timing in this doc is either
  cited hardware-specific (laptop/desktop) or tagged inferred; D2 measures before anything is
  promised (§3.2).

## 11. Open questions

1. **NPU reality** (D2): SuperPoint/LightGlue latency, warm-up, and memory on XR2-class HTP via
   QNN — no published numbers exist (§3.2); doc 16's BSP caveats apply to the runtime itself.
2. **Pose-prior injection into Basalt** (doc 20): the VIT interface carries images/IMU/masks in
   and poses out; there is no channel for "here is `T_map_local`-derived drift correction" — does
   layer C ever need to correct layer A, or is the strict-layering answer (VIO drifts freely,
   anchors absorb it via `T_map_local` updates) sufficient at room scale? Room-scale Basalt drift
   magnitude over an hours-long session is unmeasured by us.
3. **Per-pixel masks through VIT**: `vit_mask_t` is axis-aligned boxes; whether Tier 3 pixel masks
   justify an interface extension or box-fitting suffices for feature gating (§7; doc 15).
4. **Wi-Fi/BT hints**: OS permission model, scan latency at boot, and whether they beat
   "try last-used map first" in practice (§6) — plausibly not worth it for single-home devices.
5. **Vocabulary strategy for the classical tier**: fixed pretrained ORB vocabulary
   (version-locked, `System.cc:1490-1497`) vs RTAB-Map-style incremental dictionary (grows with
   map, no lock) — interacts with store size and cross-device map sharing.
6. **Screens and displays**: TVs/monitors are geometrically stable but appearance-unstable;
   mask their *content* like dynamic objects, or rely on multi-condition robustness? No studied
   system addresses this; homes are full of them.
7. **On-device merge cost**: multi-session merge optimization (ORB-SLAM3 `MergeLocal`-class) on
   headset compute — background/charging-time job, or cheap enough at session end?
8. **Descriptor-model upgrades without raw images**: scheduling background re-mapping sessions
   when the learned feature model changes (§6) — cadence, user consent, and battery.
9. **Confidence semantics for anchors**: mapping per-region inlier support onto the per-anchor
   confidence the anchor service exposes (and whether `XR_EXT_spatial_persistence`'s component
   model can carry it) — joint design with doc 21's store and the anchor service API.
