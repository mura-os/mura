# Spatial-mapping design backlog: disposition of REVIEW-mapping

**Date:** 2026-09-22. Disposition of the 20 findings in
[REVIEW-mapping.md](REVIEW-mapping.md) against [spatial-mapping.md](spatial-mapping.md) and
[ADR 0009](adr/0009-spatial-mapping-architecture.md), following the pattern of
[perception-design-backlog.md](perception-design-backlog.md).

> **Order authority:** [implementation-path.md §5](implementation-path.md) is the sole deferral
> register; this document is its satellite — it records *what the M0/M1/M2 gates must prove*,
> not when work happens.

## Fixed in the documents now

| Finding | Fix applied |
|---|---|
| **M-1** (MargData insufficient for loop closure) | Spec §4 + ADR Decision 2 rewritten: `out_marg_queue` is the egress *point*; the versioned keyframe packet (epoch, masks, descriptors-or-images) is the deliverable, prototyped at the new M0 gate. |
| **M-2** (no reset-epoch contract) | Spec §3 gains the normative epoch contract: monotonic `tracking_epoch` on every cross-boundary packet, RESET/REINITIALIZED events, no odometry edges across epochs, atomic handoff on epoch joins. M0/M1 gain forced-reset tests. |
| **M-3** (transform named in both directions) | Standardized on `T_local_map` everywhere in the spec (diagram, layer table, boot flow), with the estimation identity `T_local_map = T_local_kf · inverse(T_map_kf)` and the double-application invariant stated. |
| **M-4** (no-jump guarantee impossible as written) | Guarantee re-scoped in §1 and §3: head pose + LOCAL/STAGE never receive map discontinuities; anchored content moves under a bounded step/velocity/PAUSED policy with budgets fixed by M1 measurement. ADR rationale re-worded ("head-pose half"). |
| **M-5** (blocking push, no wire contract) | Spec §4: non-blocking relay/spool with bounded memory + GAP events required; versioned wire format is part of the M0 keyframe packet. Details remain open question §12.4. |
| **M-6** (online NfrMapper not a small conversion) | M0 foundations spike added to §11: incremental drive on replayed data with bounded RAM/latency is a gate, not an assumption. |
| **M-7** (strongest alternatives not compared) | ADR Alternatives amended: RTAB-Map-engine-as-mapping-core explicitly *not rejected*, evaluated head-to-head in M0 alongside an ORB-SLAM3 split-output baseline. |
| **M-9** (geometry outside the correction contract) | Spec §7: geometry snapshots stamped with map-graph generation, re-expressed through the same `T_local_map` authority; anchor/plane/mesh/boundary corrections atomic. |
| **M-10** (UX states vs OpenXR states) | Spec §8: explicit mapping — tentative = omitted/PAUSED, PAUSED = component data invalid, confidence internal, STOPPED = unrecoverable epoch loss; full table is an M4 deliverable. |
| **M-11** (categorical privacy guarantee false) | Spec §6 re-worded: gating is an objective with residual risk; masks must travel in the keyframe packet and be enforced at mapping-side descriptor extraction (NfrMapper gap named); descriptor-region tests required. |
| **M-12** (store corruption/recovery absent) | Spec §6: doc 21's durability rules made normative (checksums, copy-on-write generations, fsync, tombstones); M2 gate includes power-cut/torn-write/wrong-key tests. |
| **M-15** (milestone ordering hides APIs/validation) | M0 added; M1 gains the internal shell↔anchor xrt API; M2 split into durability + reloc-UX gates with false-switch rate. |
| **M-17** (`depthSource = none` contradictory) | Renamed to `mura.xr.mapping.depthAssist` in contract + spec; `none` = passive stereo, documented as such in both places. |
| **M-18** (packaging/RT/license overstatements) | Spec §2/§10 + ADR: "latency-critical soft RT", "unmodified VIO algorithm, patched integration package", license review noted as due — process separation is architectural, not a legal theorem. |
| **M-19** (wrong-reloc evidence overstated) | Spec §5: zero-wrong-accept attributed to the RTAB-Map study specifically; 50-inlier treated as precedent to recalibrate. |
| **M-20** (open questions miss the breakers) | §12 restructured into blocking (M0/M1/M2) vs non-blocking, importing the review's list. |

## Deferred to M0 (the foundations spike — implementation phase)

- **M-1/M-5/M-6/M-7 empirics:** keyframe-packet bandwidth/copy/privacy measurements; incremental
  NfrMapper RAM/latency; the three-way mapping-core comparison (NfrMapper-derived vs RTAB-Map-core
  vs ORB-SLAM3 split-output baseline) on identical replayed Basalt data.
- **M-3 empirics:** `T_local_map` estimator covariance behavior under multi-match and concurrent
  keyframe updates; direction tests with non-identity rotation+translation.
- **M-2 empirics:** forced-reset, service-restart, and epoch-join tests.

## Deferred to design-before-milestone (tracked in spec §12 blocking list)

- **M-8** anchor reparenting/migration semantics (before M1 store work touches culling).
- **M-13** room-transition state machine + false-switch metric (before M2).
- **M-14** app/user authorization model: principals, ACLs, IPC credentials, rebuild identity
  migration (before M2's `LOCAL_ANCHORS_EXT` exposure).
- **M-16** fan-out resource policy: per-consumer queue bounds, retention, zero-copy ownership,
  all-consumers stress test (before M3 adds the geometry consumer; extends ADR 0008).

## Explicitly accepted risks

- The correction-policy budgets (M-4) are set by measurement, not chosen a priori — the spec
  commits to the *shape* of the policy only.
- GPL process-boundary posture (M-18) proceeds architecturally now; formal license review is a
  pre-release task, consistent with the base architecture's handling of donor-blob licensing.
