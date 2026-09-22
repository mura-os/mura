# ADR 0009: Spatial mapping — layered (Basalt VIO + separate mapping/anchor service), not single-SLAM

**Status:** accepted as direction; implementation gated on the M0 foundations spike
(see Consequences and [REVIEW-mapping.md](../REVIEW-mapping.md))
**Date:** 2026-09-22
**Context sources:** [20-slam-stacks-for-xr](../../research/20-slam-stacks-for-xr.md) (the VIT seam
and the evidence summary §11), [21-anchors-persistence-openxr](../../research/21-anchors-persistence-openxr.md),
[22-dense-geometry-no-lidar](../../research/22-dense-geometry-no-lidar.md),
[23-relocalization-multisession](../../research/23-relocalization-multisession.md).
Design elaborated in [spatial-mapping.md](../spatial-mapping.md). Placement follows
[ADR 0008](0008-perception-services-placement.md).

## Context

Persistent spatial computing (anchors surviving reboots) needs layers Monado+Basalt do not
provide: mapping/loop-closure, relocalization+persistence, and an anchor service honoring the
**map/local frame contract** — corrections move anchors, never the rendered world. Two candidate
architectures were studied at code level:

- **(a) Layered:** keep Basalt as the hard-RT VIO behind the (unchanged) VIT seam; build a
  separate mapping/anchor service consuming VIO byproducts.
- **(b) Single-SLAM:** put ORB-SLAM3 (VIO+mapping+reloc+multi-map+persistence in one) behind the
  tracker seam — proven runnable by Collabora's `orbslam3-monado` bridge.

## Decision

**Layered.** Specifically ([spatial-mapping.md](../spatial-mapping.md) §2–§4):

1. **Layer A:** Basalt via VIT, unmodified, from nixpkgs (`basalt-monado`, cached). VIT stays a
   thin pose seam; no map API grows into it.
2. **Keyframe egress:** Basalt's existing `out_marg_queue` is the egress *point*; the egress
   *packet* is ours to define — a versioned keyframe packet (epoch, masks, descriptors-or-images,
   marg prior) behind a non-blocking relay, because in-memory `MargData` (Hessian/gradient,
   keyframe poses/states, observations) does not by itself carry the pixels/descriptors loop
   closure needs, and the raw queue push is blocking
   ([20 §3.2](../../research/20-slam-stacks-for-xr.md); REVIEW-mapping M-1/M-5). Bounded work in
   maintained BSD code; candidate VIT minor-version extension upstream.
3. **Layers B+C:** a **separate-process mapping+anchor service** — Kimera-shaped internals
   (factors in, robust PGO via BSD Kimera-RPGO), starting from Basalt's in-tree NfrMapper
   (factor recovery + HashBow + pose graph) driven online; two-tier relocalization
   ([23](../../research/23-relocalization-multisession.md)); RTAB-Map-shaped encrypted store
   ([21 §5](../../research/21-anchors-persistence-openxr.md)); anchors parent-relative to
   keyframes (ORB-SLAM3's `mTcp` pattern as design, not code).
4. **Layer E:** implement the ratified `XR_EXT_spatial_entity`/`_anchor`/`_plane_tracking`/
   `_persistence` family in Monado and upstream it, reusing Monado's existing `XR_EXT_future`
   implementation and plane-detector plumbing below a new canonical plane/entity tracker
   ([21 §7](../../research/21-anchors-persistence-openxr.md)).
5. **ORB-SLAM3 is retained as an evaluation baseline and design reference only** (reloc-funnel
   thresholds, Atlas content inventory, `mTcp`), packaged in-flake for evaluation when needed.

## Rationale (the code evidence is lopsided — [20 §11](../../research/20-slam-stacks-for-xr.md))

- **The head-pose half of the frame contract falls out of (a) by construction.** VIO never
  receives corrections, so head pose and reference spaces cannot jump (anchored content moves only
  under the bounded correction policy — [spatial-mapping.md §3](../spatial-mapping.md)). Under
  (b), `TrackStereo` returns poses in ORB-SLAM3's *corrected*
  world frame: `CorrectLoop`/`MergeLocal`/`ChangeMap` rewrite the frame and the compositor jumps
  by accumulated drift in one frame; Monado's defenses (comfort filters, IMU extrapolation) are
  not correction absorbers. Honoring the contract under (b) means forking the LoopClosing/Atlas
  core of a 4-years-dormant codebase — re-implementing the layered split inside GPL code.
- **Licenses align with the architecture.** VIT trackers are dlopen'd into BSL-1.0 Monado;
  ORB-SLAM3/OpenVINS/ov_plane are GPL-3. Path (a) keeps the in-process tracker BSD and puts
  anything GPL-derived behind the process boundary the design wants anyway. (Process separation
  reduces but does not legally settle derivative-work questions — a license review is still due
  before shipping GPL-derived service code; REVIEW-mapping M-18.)
- **Packaging aligns too:** `basalt-monado` is cached in nixpkgs (zero cost as VIO; the egress
  means a patched overlay until upstreamed); ORB-SLAM3+Pangolin are unpackaged with known build
  friction (bounded, but real).
- **(a)'s gaps are constructive engineering, not research:** online NfrMapper driver, persistence
  format, `T_local_map` estimator, anchor service, MargData IPC schema, the tee patch — each
  de-risked by code that already exists in-tree (marg queue, NfrMapper, RPGO, `mTcp`).
- **(b)'s one real asset (integrated maturity) is undercut** by the fork requirement above, plus:
  the bridge implements the dead pre-VIT ABI, exposes velocity-less poses, has calibration
  injection `#if 0`'d out; Atlas persistence is a monolithic shutdown-time boost blob; and a
  loaded Atlas still boots into a *fresh* map until a merge fires — the boot-reloc UX problem is
  not actually solved by adopting it.

## Consequences

- **The M0 gate** (added per [REVIEW-mapping.md](../REVIEW-mapping.md) M-1/M-6/M-7/M-15): before
  M1 implementation, a foundations spike must (i) prototype the versioned keyframe packet over a
  non-blocking egress, (ii) drive NfrMapper-derived mapping incrementally on replayed data, and
  (iii) replay the same data through an RTAB-Map-core mapping service and an ORB-SLAM3
  split-output baseline. The *layered topology* is decided here; the *mapping-service core*
  (NfrMapper-derived vs RTAB-Map's engine behind the same interface) is explicitly left to M0
  evidence.
- New components to build (phased in [spatial-mapping.md §11](../spatial-mapping.md)): the
  basalt-monado egress patch (+upstream conversation), the mapping+anchor service, the store, the
  boot-reloc flow, the geometry service, and the Monado `XR_EXT_spatial_*` implementation
  (flagship upstream contribution).
- The device contract gains `spatial.xr.mapping.*` (enable, `depthAssist` policy axis,
  persistence/boundary toggles).
- GPL code never enters the Monado process; the mapping service's IPC boundary is the
  architectural license boundary (legal review still due, see Rationale).
- Validation protocol (EuRoC replay, Atlas-style save/reload metrics, NPU reloc timings) is
  specified in [23 §8](../../research/23-relocalization-multisession.md) and deferred to
  implementation phase (the research-phase spikes were cancelled by scope decision; M0 absorbs
  them).

## Alternatives considered

- **Single-SLAM (ORB-SLAM3 behind the seam):** rejected — frame-contract violation by
  construction, GPL-in-process, dormant upstream, wrong-shaped persistence
  ([20 §4, §5, §11](../../research/20-slam-stacks-for-xr.md)).
- **OpenVINS as VIO:** rejected — GPL for no capability gain over Basalt, and no mapping layer
  either ([20 §7](../../research/20-slam-stacks-for-xr.md)).
- **Kimera-VIO as shipped code:** rejected — dormant; adopt its module architecture, import at
  most Kimera-RPGO (BSD) as a library.
- **RTAB-Map as the live tracker:** rejected — map-centric latency profile
  ([23 §2.3, §4](../../research/23-relocalization-multisession.md)). **RTAB-Map's engine as the
  mapping-service core** (behind the same layered interface, with Basalt still the tracker) is
  *not* rejected — it is the strongest alternative to NfrMapper-derived code and is evaluated
  head-to-head in M0 (REVIEW-mapping M-7); its store schema, WM/LTM bounding, and multi-session
  evidence are adopted regardless.
- **Extending VIT into a full map API:** rejected — recreates single-SLAM coupling through the
  back door; VIT stays the thin pose seam ([20 §10](../../research/20-slam-stacks-for-xr.md)).
- **ILLIXR as runtime:** rejected — research vehicle; but its topic-bus framing is adopted for the
  mapping service's subscription-style input ([20 §6](../../research/20-slam-stacks-for-xr.md)).
