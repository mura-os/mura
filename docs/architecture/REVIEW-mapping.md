# Red-team review: spatial mapping architecture

**Reviewer model:** GPT-5.6 Sol  
**Date:** 2026-09-22  
**Documents reviewed:** `docs/architecture/spatial-mapping.md`;
`docs/architecture/adr/0009-spatial-mapping-architecture.md`;
`docs/research/20-slam-stacks-for-xr.md`;
`docs/research/21-anchors-persistence-openxr.md`;
`docs/research/22-dense-geometry-no-lidar.md`;
`docs/research/23-relocalization-multisession.md`;
`docs/architecture/adr/0008-perception-services-placement.md`;
`docs/architecture/perception-passthrough-hands.md`; and
`lib/contract/default.nix` (`spatial.xr.mapping`). Relevant Basalt, Monado,
ORB-SLAM3, and OpenXR reference code was checked where claims depended on it.

## Findings

### M-1 — blocker — `MargData` is not sufficient input for the proposed mapper
**Claim/design element.** The spec calls `MargData` “exactly the mapping input” and reduces the
Basalt fork to “one queue redirect” (`spatial-mapping.md` §4; ADR 0009, Decision 2).

**Evidence.** `MargData` cereal serialization omits `opt_flow_res`
(`references/basalt-monado/include/basalt/utils/imu_types.h:372-381`); the disk path writes and
reloads images separately (`references/basalt-monado/src/io/marg_data_io.cpp:73-78,126-168`).
NfrMapper copies those images, then detects mapping keypoints/descriptors from pixels
(`references/basalt-monado/src/vi_estimator/nfr_mapper.cpp:136-139,419-449`). Hessians and tracks
alone do not drive its HashBow/loop-closure path.

**Recommended fix.** Define and prototype a versioned keyframe packet containing either calibrated
stereo images plus masks, or precomputed descriptors/BoW plus 2D–3D observations. Gate the ADR on
online loop closure from that payload, including bandwidth, copying, privacy, and drop behavior.

### M-2 — blocker — VIO resets have no epoch contract and can corrupt the graph
**Claim/design element.** M1 assumes a continuous `local` frame while the service subscribes to
poses and MargData without reset/discontinuity messages (`spatial-mapping.md` §§3–5, §11).

**Evidence.** VIT reset is a full wipe with no output event (`20-slam-stacks-for-xr.md` §2.1).
Basalt's `state_reset` is private to `OpticalFlowInput`
(`references/basalt-monado/include/basalt/optical_flow/optical_flow.h:92-99`), and reset clears
poses, keyframes, IMU state, and prior flow
(`references/basalt-monado/src/vi_estimator/sqrt_keypoint_vio.cpp:140-160`). The mapper can connect
two unrelated local gauges as odometry.

**Recommended fix.** Put a monotonic `tracking_epoch` on every pose/frame/keyframe/MargData packet;
publish RESET/LOST/REINITIALIZED; forbid edges across epochs until verified merge; and define
atomic anchor/`T_local_map` handoff. Add forced-reset and service-restart tests to M1.

### M-3 — blocker — the core transform is named in both directions
**Claim/design element.** `T_A_B` maps B into A and rendering uses
`T_local_anchor = T_local_map · T_map_anchor` (`spatial-mapping.md` §3).

**Evidence.** Layer C, the diagram, and boot flow instead publish `T_map_local`
(`spatial-mapping.md` §§2,5), as does research 23 (§§1,5,9). These are inverses. The spec also says
loop closure changes both keyframe poses and the correction without an invariant preventing
double application.

**Recommended fix.** Use `T_local_map` everywhere and specify, at a matched timestamp,
`T_local_map = T_local_kf · inverse(T_map_kf)`, including multi-match estimation and covariance.
Add direction tests using non-identity rotation and translation.

### M-4 — blocker — “the rendered world never jumps” is impossible as written
**Claim/design element.** The goal and correction policy guarantee no rendered-world jump because
VIO is never corrected (`spatial-mapping.md` §§1,3; ADR 0009, Rationale).

**Evidence.** Continuous `T_local_head` prevents a camera/reference-space jump, but changing
`T_local_map` or `T_map_keyframe` changes anchor poses. Immediate application pops content;
interpolation moves it while temporarily making it physically wrong. The cited research explicitly
says no policy provides zero visual motion and perfect registration
(`21-anchors-persistence-openxr.md` §4.2).

**Recommended fix.** Guarantee only that predicted head pose and LOCAL/STAGE receive no map
discontinuity. Specify budgets for anchor step, interpolation velocity, misregistration, and
pause/fade. Make M1 measure head continuity and anchor correction separately.

### M-5 — blocker — IPC can backpressure VIO and has no wire contract
**Claim/design element.** Mapping is killable/duty-cycled and isolated from “hard RT” VIO; MargData
delivery is a bounded tee/subscription (`spatial-mapping.md` §§2,4).

**Evidence.** Basalt has one pointer to one bounded `out_marg_queue`
(`references/basalt-monado/include/basalt/vi_estimator/vio_estimator.h:100-106`) and does a blocking
push during marginalization (`sqrt_keypoint_vio.cpp:962-976`). This is neither a tee nor
failure-isolated. No schema defines matrix layout, calibration/epoch/sequence IDs, maximum size,
version negotiation, dropped ranges, ownership, or restart replay.

**Recommended fix.** Add a non-blocking relay/spool outside tracking, bounded memory, and explicit
GAP events. Specify/version the wire format and measure dense-Hessian/image serialization.
Mapper death or slowness must degrade mapping only.

### M-6 — major — “online NfrMapper” is not a small batch-driver conversion
**Claim/design element.** ADR 0009 calls the missing pieces constructive engineering, and the spec
says NfrMapper only needs online driving (`spatial-mapping.md` §4; ADR 0009, Rationale).

**Evidence.** `basalt_mapper` loads the entire corpus, then globally runs detect → match-all →
build/setup → filter → optimize (`references/basalt-monado/src/mapper.cpp:170-189,307-316,590-615`).
There is no incremental keyframe lifecycle, bounded working set, concurrent query/update,
durable IDs, online loop validation, mapper-state marginalization, or restart behavior.

**Recommended fix.** Make this a feasibility spike. Require replay evidence for bounded RAM/latency,
incremental insertion/culling, loop closure during motion, reset epochs, and deterministic restart
before selecting NfrMapper as the production core.

### M-7 — major — ADR 0009 does not compare the strongest alternatives
**Claim/design element.** RTAB-Map is rejected only as a “live tracker,” and ORB-SLAM3 is said to
require reimplementing LoopClosing/Atlas (ADR 0009, Alternatives/Rationale).

**Evidence.** Basalt remains the live tracker, so the relevant option is RTAB-Map as the mapping/
reloc service. Research reports its WM/LTM bounding, SQLite store, Bayes reloc, incremental
vocabulary, and measured keyframe-rate costs (`23-relocalization-multisession.md` §§2.3,3.3,4.1).
The ORB bridge proves corrected output is currently unmediated, not that a narrower odometry-plus-
correction hook is impossible (`20-slam-stacks-for-xr.md` §4). VINS-Fusion-class service backends
are not evaluated at all.

**Recommended fix.** Replay the same Basalt data through NfrMapper-derived, RTAB-Map-core, minimal
custom, and an ORB-SLAM3 split-output baseline. Compare patch size, license, payload, precision/
recall, RAM, latency, persistence, and continuity; mark unstudied candidates “not evaluated.”

### M-8 — major — anchor parents can disappear with no migration semantics
**Claim/design element.** Anchors are parent-relative to keyframes, citing ORB-SLAM3's `mTcp`
pattern (`spatial-mapping.md` §3).

**Evidence.** ORB recomputes `mTcp` when culling a keyframe
(`20-slam-stacks-for-xr.md` §5.3). This design also plans keyframe culling, graph reduction,
staleness expiry, merge, and deletion (`23-relocalization-multisession.md` §§4.2,6), but defines no
anchor migration or failure state when a parent disappears.

**Recommended fix.** Specify parent selection, multi-keyframe support/covariance, atomic reparenting,
copy-on-write migration, and map-deletion behavior. Test culling, merge, stale-region replacement,
and interrupted migration.

### M-9 — major — persistent geometry is outside the map/local correction contract
**Claim/design element.** Geometry publishes planes/mesh directly to Monado/compositor
(`spatial-mapping.md` §§2,4,7 and diagram).

**Evidence.** The diagram gives geometry no map-pose input and no `T_local_map` at handoff. Yet doc
22 requires stable plane IDs, cross-fades, and yesterday's map loading as inferred
(`22-dense-geometry-no-lidar.md` §§2,7,9). Loop closure can therefore move anchors while leaving
occlusion, shadows, boundary, and planes in the old local gauge.

**Recommended fix.** Store geometry in a named map/epoch frame, version snapshots with graph
generation, and transform/cross-fade through the same correction authority. Make anchor, plane,
mesh, and boundary updates atomic.

### M-10 — major — UX states do not map cleanly to OpenXR
**Claim/design element.** Large corrections become `PAUSED`/degraded; boot adds
`LOCALIZED_TENTATIVE` and `DEGRADED` with confidence (`spatial-mapping.md` §§3,5).

**Evidence.** OpenXR provides TRACKING, PAUSED, and terminal STOPPED; while PAUSED, component
buffers are not updated (`references/openxr-docs/specification/sources/chapters/extensions/ext/
ext_spatial_entity.adoc:1178-1213`). EXT anchors have no confidence or DEGRADED component. The
spec does not say whether tentative entities are omitted, PAUSED, or TRACKING.

**Recommended fix.** Add a state-mapping table. Keep tentative entities omitted/PAUSED until
confirmation, use PAUSED only when component data is invalid, keep confidence internal unless a
standard component carries it, and define STOPPED after unrecoverable epoch loss.

### M-11 — major — the categorical privacy guarantee is false
**Claim/design element.** Masked people/pets/hands “never” contribute descriptors, so people are
never fingerprinted (`spatial-mapping.md` §§6,9).

**Evidence.** Basalt's frontend honors boxes, but NfrMapper re-detects mapping features with
`detectKeypointsMapping`, which has no mask argument
(`references/basalt-monado/src/vi_estimator/nfr_mapper.cpp:419-441`;
`references/basalt-monado/src/utils/keypoints.cpp:118-137`). Masks exist in `OpticalFlowInput`
(`optical_flow.h:92-99`), but NfrMapper ignores them. Fallible segmentation cannot justify “never.”

**Recommended fix.** Include masks and provenance in keyframe IPC; apply them to local/global
descriptor extraction and retained diagnostics; add descriptor-region tests; and state a measured
privacy objective plus residual risk instead of an absolute guarantee.

### M-12 — major — corruption and recovery are not part of the store design
**Claim/design element.** The store is versioned, transactional, migrated, and encrypted
(`spatial-mapping.md` §6).

**Evidence.** Research 21 requires checksummed blobs, copy-on-write migration, atomic generation
switch, `fsync` before success, and tombstones (§§5.2–5.4). The spec omits integrity verification,
quarantine/rollback, partial transactions, disk-full behavior, backup generations, and crash tests.
M2 tests only happy-path reboot/reloc.

**Recommended fix.** Make those durability rules normative, recover only to a verified generation,
and add power-cut, torn-blob, WAL loss, disk-full, wrong-key, and migration-rollback gates to M2.

### M-13 — major — boot relocation has no room-transition state machine
**Claim/design element.** Boot searches candidate maps and localizes in 1–2 seconds; ambiguity hides
content (`spatial-mapping.md` §5; research 21 §5.3; research 23 §§5–6).

**Evidence.** No behavior is defined for booting in a doorway, walking between rooms during
confirmation, simultaneous plausible rooms, or switching after fade-in. Multi-frame confirmation
does not define hysteresis or an atomic active-map transition.

**Recommended fix.** Define concurrent candidates, motion-aware confirmation, ambiguity timeout,
switch hysteresis, per-map visibility, and atomic generation changes. Add doorway/walking/repeated-
room tests and report false-switch rate, not only time-to-first-localized.

### M-14 — major — app/user authorization is missing
**Claim/design element.** Privacy covers encryption and user UI; M2 exposes `LOCAL_ANCHORS_EXT`
(`spatial-mapping.md` §§6,11).

**Evidence.** That scope is same-device, user, **and app** (`21-anchors-persistence-openxr.md`
§2.7), and its proposed schema has `creator_app_id`/`AppBinding` (§5.2). The architecture omits
stable app identity, ACLs, cross-app discovery rules, IPC credentials, raw-frame authorization,
and NixOS-rebuild identity migration.

**Recommended fix.** Define user/app principals, peer authentication, per-scope ACLs, service
sandboxing, and rebuild migration. Gate M2 on cross-app denial and multi-user isolation.

### M-15 — major — milestone ordering hides required APIs and validation
**Claim/design element.** M1 pins shell windows, M3 publishes planes, and M4 finally implements the
standard entity surface (`spatial-mapping.md` §11).

**Evidence.** No private shell↔anchor API is defined for M1. M3 says old plane shapes make the
standard extension work “for free,” while §8 correctly requires a new stable entity tracker.
ADR 0009 accepts the design after cancelling the spikes that validate its riskiest seams (§10).

**Recommended fix.** Add M0: versioned keyframe egress, replayable online mapper, transform/reset
contract, and RTAB-Map/ORB baselines. Define a minimal internal xrt entity API before M1 and
canonical plane identity before M3; split crash-safe storage from reloc UX in M2.

### M-16 — major — shared frame fan-out lacks resource and contention policy
**Claim/design element.** Mapping, geometry, passthrough, hand cutout, Mercury, and Basalt consume
one `xrt_frame` fan-out (`spatial-mapping.md` §§2,4; ADR 0008).

**Evidence.** One clock domain does not solve buffer lifetime, format copies, cache/GPU contention,
slow-consumer detachment, raw-frame retention, or synchronization among frames, pose/features, and
MargData. ADR 0008's latest-complete drop policy cannot blindly apply to indispensable factors.

**Recommended fix.** Specify immutable sequence/calibration IDs, bounded per-consumer queues,
retention limits, zero-copy ownership, topic-specific gap rules, and scheduler budgets. Stress all
consumers together before calling the topology settled.

### M-17 — major — `depthSource = none` has contradictory meanings
**Claim/design element.** The spec/contract use `spatial.xr.mapping.depthSource`
(`spatial-mapping.md` §7; `lib/contract/default.nix:217-246`).

**Evidence.** Research 22 says `none` means no depth capability, landmarks-only planes, no TSDF
(§6.3). The Nix contract says it means RGB passive stereo (`default.nix:224-233`). Research names
the option `spatial.perception.geometry.depthSource`, while implementation uses
`spatial.xr.mapping`.

**Recommended fix.** Use one path and meaning. Prefer `depthAssist`, where `none` means passive
visible stereo, plus a separate geometry capability switch. Add a behavior truth table and Nix
assertions making `persistence`/`boundary` depend on `mapping.enable`.

### M-18 — minor — packaging, RT classification, and license claims are overstated
**Claim/design element.** Basalt is “unmodified,” “zero packaging cost,” and “hard RT”; IPC
“quarantines any GPL-derived code” (`spatial-mapping.md` §§2,4,10; ADR 0009).

**Evidence.** Live egress requires changing the current disk-saver wiring
(`references/basalt-monado/src/vit/vit_tracker.cpp:370-406`), so a patched package/overlay exists
until upstream lands. Basalt uses bounded queues and drops old output poses, with no WCET proof
(`vit_tracker.cpp:541-550`), so it is soft real-time. Research 20 §8 calls GPL linking contested;
IPC does not erase obligations or automatically settle derivative-work questions.

**Recommended fix.** Say “unmodified VIO algorithm, patched integration package” and budget the
overlay/ABI/cache work; classify VIO as latency-critical soft RT; inventory GPL relationships and
obtain legal review rather than treating IPC as a legal theorem.

### M-19 — minor — wrong-reloc evidence and threshold claims are overstated
**Claim/design element.** Both studied systems allegedly achieve zero wrong accepts, using a
“≥50-inlier-class” gate (`spatial-mapping.md` §5).

**Evidence.** ORB-SLAM3 source verifies its threshold, not zero errors on the target protocol
(`23-relocalization-multisession.md` §2.1). The reported zero-wrong-accept observation is specific
to the RTAB-Map study (§4.1); §8 calls approximately zero a target. Raw inlier count also varies
with extractor density.

**Recommended fix.** Attribute the result to that dataset, treat 50 as precedent, and calibrate on
inlier ratio/coverage, reprojection error, covariance, temporal consistency, and held-out headset
data.

### M-20 — minor — the open questions omit the items most likely to break M1/M2
**Claim/design element.** Section 12 calls its list consolidated and puts blockers first
(`spatial-mapping.md` §12).

**Evidence.** Missing are reset epochs; the image/descriptor payload; IPC backpressure/versioning;
incremental NfrMapper feasibility; the transform estimator/covariance; anchor reparenting; store
corruption; authorization; room-switch hysteresis; geometry correction atomicity; and full fan-out
contention. These precede NPU and dense-TSDF tuning.

**Recommended fix.** Promote these to blocking questions with M0/M1/M2 tests, and defer optional
dense/learned work until keyframe, epoch, transform, and durability contracts are proven.

## Verdict
The high-level split—smooth Basalt local odometry plus a separately corrected persistent map—is
sound enough to prototype, but ADR 0009 is not sound enough to implement as accepted. First prove
a non-blocking, versioned keyframe egress carrying the actual mapping inputs and masks; define reset
epochs and one transform direction; replace the no-jump promise with measurable correction bounds;
and compare online mapping against RTAB-Map and ORB-SLAM3 baselines. Persistence, standard APIs,
and dense geometry must not proceed past prototypes until those foundations and crash-safe store
semantics pass explicit gates.
