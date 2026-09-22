# Red-team review: perception passthrough and hands

## Verdict

**With changes, but not yet sound enough to implement as written.** The research synthesis is unusually
careful, and the three-rate/non-blocking architecture is a credible basis for experiments. However, the
composition equations do not implement the stated hand-visibility policy: the passthrough environment
already contains the hand before the proposed top layer runs. The Monado-to-compositor buffer and metadata
path is also asserted rather than specified, and co-location does not by itself provide pose-at-exposure or
a common clock. Start only with a hardware feasibility spike and a corrected opaque-only composition
contract; do not start by implementing the four depth backends or Tier-2 matting.

## Blocking issues

1. **The hand is already in `C_scene`, so the top-layer policy cannot hide it.**
   - Perception §“The decomposition” makes passthrough an environment colour+depth contributor, including
     near hands, then §“Hand cutout” applies the hand matte after nearest-depth resolve.
   - Its equation says `hidden: C = C_scene`, but `C_scene` already contains the camera image of the hand.
     `automatic` has the same problem: reducing top-layer alpha reveals the same hand underneath.
   - The stale-matte fallback (“passthrough-hands-absent”) is likewise false unless hands were removed from
     the environment first.
   - This also conflicts with research 15 §6’s Meta precedent: `HandsRemoval` first removes hand depth from
     the environment, then a geometry mask reveals/composites the hand.
   - **Resolution:** define one coherent model before coding:
     (a) remove hand colour and depth from the environment and preserve/reconstruct background, then add
     `αF_hand`; or (b) keep an untouched passthrough colour plane and use a geometry mask to punch virtual
     content away, with explicitly limited policy semantics. Specify hole/background treatment at the hand
     boundary. Do not call the current operation a cutout.

2. **A per-client policy cannot be applied after a single nearest-depth resolve.**
   - Perception §“Hand cutout” proposes a per-spatial-client enum, but its pseudocode resolves all clients
     to only `(C_scene, d_s)` before consulting policy.
   - At that point client identity and all losing samples are gone. Two intersecting clients with different
     policies cannot be handled from colour+depth alone.
   - The problem becomes stricter for composition T2: zxr composition §3 represents multiple ordered
     translucent samples, while a global hand top layer has no defined position among those samples.
   - **Resolution:** for opaque T1, resolve an owner/policy ID alongside colour and depth, and define shell
     precedence for compositor-owned surfaces. Alternatively apply hand policy per client before the final
     cross-client resolve. Explicitly declare T2 interaction unsupported until hand samples participate in
     the deep/ordered representation. Research 15 §9.8 already calls this open; it is a blocker for the
     claimed per-client contract, not merely a future detail.

3. **The hand-depth comparison is inverted under the depth convention the design cites.**
   - Perception §“Hand cutout” says the verified contract is reverse-Z, then computes
     `smoothstep(..., d_h - d_s)` with the comment “hand in front” on the negative side.
   - That sign is correct for positive metric eye distance, where nearer is smaller. It is wrong for
     reverse-Z window depth, where nearer is larger.
   - Passthrough writes `gl_FragDepth`, the backend interface may carry disparity, inverse depth, fixed
     disparity, Hilbert-encoded values, or metric depth, and the hand source is capsule/stereo metric depth.
     “Declared encoding” does not itself make these values directly comparable.
   - **Resolution:** define one canonical comparison quantity, preferably positive linear eye-space metres,
     with one tested conversion from every producer. Define the separate conversion to the shared reverse-Z
     attachment, including near/far, clip range, invalid/far values, and equality tolerance. Add tests for
     “hand in front”, “hand behind”, and both reversed/non-reversed client submissions.

4. **“Same-timestamp atomicity” contradicts the intentional three-rate pipeline.**
   - Perception §“The shared production architecture” intentionally permits fresher colour than geometry.
   - Perception §“The shared invariants” then requires colour, depth/matte, confidence, and pose to travel as
     one unit keyed to one camera exposure.
   - Passthrough geometry may be based on an older stereo pair than the displayed camera colour. Pretending
     they share one timestamp either throws away the colour-freshness benefit or mislabels stale geometry.
   - Hand matte is different: research 15 §5 requires `αF`, alpha, hand depth, and source colour to be from
     the same capture.
   - **Resolution:** split the contracts. Passthrough must carry `t_colour`, `t_geometry`, their respective
     poses/calibration versions, and the transform/reprojection used to align geometry to colour. A hand
     artifact remains one strict atomic unit. Never describe the two cases with one timestamp invariant.

5. **ADR 0008 does not close the camera-to-compositor buffer path.**
   - ADR 0008 §“Decision” alternates between “finished dmabuf layers” and publishing a `DepthFrame + camera
     textures`; perception §“Passthrough pipeline” requires the compositor to perform the final per-eye warp.
     Those are materially different interfaces.
   - Monado `xrt_frame` availability does not imply a camera frame is a dmabuf, Vulkan-importable, modifier-
     compatible, or exportable to another process. Research 16 Part 1 explicitly leaves camera zero-copy
     unanswered.
   - No interface carries stereo atomicity, camera format/planes, DRM modifier, colour space, exposure/gain,
     distortion map, timestamps, calibration version, producer device identity, or buffer reuse lifetime.
   - **Resolution:** choose whether Monado exports source-domain artifacts for a compositor warp or exports
     final per-eye layers. Specify a versioned IPC protocol and bounded buffer pools for that choice. Prove
     capture → processing → cross-process import on one target BSP before treating ADR 0008 as implementable.
     A CPU-copy fallback may be acceptable for bring-up, but it must be explicit and measured.

6. **Pose-at-exposure is named as a blocker but has no implementable contract or fallback.**
   - Perception §“Shared invariants” and ADR 0008 §“Consequences” correctly mark arbitrary historical pose
     lookup as blocking.
   - ADR 0008 nevertheless assumes an in-process Basalt query supplies it. Monado may use another tracker;
     camera timestamps may be sensor/ISP/V4L2 clocks; and pose history interpolation, clock conversion, and
     uncertainty are not specified.
   - “Same process” removes an IPC hop but does not prove a common clock or a sufficiently accurate pose
     history. A worker may block without blocking scanout, but stale or absent pose still invalidates view
     correction.
   - **Resolution:** add a concrete API returning pose, source clock mapping, interpolation status, and
     uncertainty for a capture timestamp. Characterize it against hardware timestamps, not publish time.
     If unavailable, disable corrected passthrough (or expose an explicitly degraded fixed-proxy mode);
     do not silently use latest pose. Make this a release/enable gate.

7. **The Monado-side placement decision conflates capture ownership, execution, and process location.**
   - ADR 0008 §“Decision” says services are Monado-side and pose queries are in-process.
   - ADR 0008 §“Consequences” then allows “Monado-adjacent processes sharing its frameserver”; the GPL matte
     is explicitly a separate process. Those variants do not have the asserted in-process pose path.
   - One capture owner is valuable, but finished artifacts still cross a process boundary with metadata,
     clocks, calibration versions, pool lifetime, and synchronization. Placement avoids a second camera
     domain only if all of that provenance survives transport.
   - The rejected alternatives are strawmen: compositor-internal perception need not open cameras
     independently, and a privileged sibling could subscribe to a Monado-owned exported stream rather than
     own the cameras.
   - **Resolution:** recast the ADR around invariants: Monado owns capture and timestamp assignment; services
     may be in-process or siblings behind one specified frame/pose transport; the compositor owns only the
     display-rate warp. Compare those concrete variants on failure isolation, copies, licensing, and API
     burden. Do not claim that process separation by itself settles GPL derivative-work questions; obtain
     legal review for the actual IPC/linkage and distribution.

8. **`wp_linux_drm_syncobj_v1` is not a complete Monado-service transport.**
   - ADR 0008 §“Decision” says perception uses the same transport as zxr clients.
   - `wp_linux_drm_syncobj_v1` is per-`wl_surface` commit. Research 09 §explicit-sync explicitly notes that
     out-of-band zxr buffers must be bound to a surface commit or given equivalent per-buffer semantics.
     A Monado frame sink is not automatically a Wayland client surface.
   - “Latest complete” also needs a non-blocking readiness test. Importing a dmabuf and then waiting on an
     unsignalled acquire point violates the design’s main scheduling rule.
   - **Resolution:** either make the service a real private Wayland client with defined surface commits, or
     define equivalent IPC carrying acquire/release timeline points. Specify how the compositor polls/selects
     only signalled snapshots, how release is returned, and what happens on producer death or timeline reset.

9. **The Monado reprojection issue is broader—and more precise—than “double-warp risk.”**
   - Perception §“Open questions” correctly flags passthrough depth versus Monado reprojection, but the term
     “double-warp” is ambiguous.
   - A camera-to-predicted-eye warp followed by Monado’s predicted-to-late-pose warp is normally two
     different transforms and may be desirable. Applying the same pose delta twice is a metadata bug.
   - The harder problem is the single merged projection layer: virtual content, passthrough, and a
     post-resolve alpha-blended hand may require different reprojection behavior. If the hand pass changes
     colour but leaves `d_s`, Monado reprojects hand pixels at the underlying scene depth. If it writes
     `d_h`, partially blended hand/scene pixels have no single correct depth.
   - **Resolution:** specify the projection-layer pose/time and final depth semantics. For the first
     prototype, disable `XR_KHR_composition_layer_depth` and measure rotational late warp only, or disable
     Monado reprojection if the runtime permits. Add depth-assisted reprojection only after tests prove the
     exact transform chain and edge behavior. Treat this open question as a blocker before enabling runtime
     depth reprojection, not before a no-depth prototype.

10. **Separate Vulkan devices and multi-GPU operation are assumed away.**
    - ADR 0008 assumes dmabuf import is “the same transport” as client buffers.
    - Monado, the perception service, and the zxr compositor are separate processes with separate Vulkan
      logical devices. On desktop they may select different physical GPUs; even on one SoC, external image
      format/modifier and semaphore support must be queried for both producer and consumer.
    - zxr composition §7.5 explicitly leaves multi-GPU out of its MVP, but ADR 0008 states a general
      placement without constraining device selection.
    - **Resolution:** require matching DRM render node/device UUID for the zero-copy MVP; negotiate exact
      format/modifier/usage/handle tuples; define copy/blit fallback or reject mismatched devices. Record
      this as an ADR consequence and qualification test.

11. **Rolling shutter and exposure changes have no data model despite history reuse being load-bearing.**
    - Perception §“Shared invariants” lists rolling shutter and exposure notes as contract inputs but defines
      no fields or algorithm.
    - Research 13 §5.4 warns that auto-exposure/gain changes make reprojected colour history visibly patchy.
      Perception adopts validated history fill without carrying exposure, gain, or white balance.
    - Research 13 §9.2 says rolling shutter makes pose a function of image row. One mid-exposure timestamp is
      therefore insufficient on affected sensors.
    - **Resolution:** carry exposure interval, gain, white balance/black level where available, sensor
      timestamp clock, readout direction, and row readout time. Correct or invalidate colour history across
      exposure changes. Gate rolling-shutter devices on a row-pose warp or measured evidence that the error
      is acceptable.

12. **The learned-depth roadmap overstates what is reproduced on XR2.**
    - Perception §“Depth backend” calls temporal stereo the phase-2 recipe “proven 30 FPS on XR2 by
      XR-Stereo.”
    - Research 14 §2.2 and research 16 Part 2 say XR-Stereo releases only a dataset, not model code. Its fast
      XR2 model removes temporal warping, while TC-Stereo’s released temporal implementation uses
      CUDA-specific scatter/dynamic operations and has no Qualcomm result.
    - The design is appropriately cautious about Adreno DFS, QCOM Vulkan, and HTP availability; preserve
      that caution. The overstatement is the implied reproducible recipe, not the existence of the paper.
    - **Resolution:** say “reported existence result, unreproduced.” Require an on-BSP model export,
      full-delegation report, sustained thermal run, and released/reimplemented algorithm before it becomes
      a milestone dependency. Keep all 2026 preprints and Hilbert encoding as experiments, not interface-
      shaping requirements for the first backend.

13. **The sequence references the right risk first, but does not make it a binary program gate.**
    - Research 13 §7.4 P0 correctly starts with camera exposure timestamps and pose-at-exposure.
    - Perception §“Metrics and sequencing” merely says P0–P6 and hand tiers are gated behind BSP unknowns;
      ADR 0008 is already accepted and the contract already advertises unavailable backends.
    - The likely failure is earlier than P0’s image-quality acceptance: camera access may be unavailable,
      timestamps may be ISP-delayed or in an unmapped clock, and frames may not cross into Vulkan/another
      process.
    - **Resolution:** add a P-1 kill-gate on one named device: synchronized stereo capture, documented
      exposure timestamps, pose-history query, calibration retrieval, GPU import, and compositor-process
      transfer under sustained load. Stop the architecture track if any has no viable BSP route. Only then
      run P0–P6.

14. **The advertised contract surface is neither minimal nor sufficient.**
    - Perception §“Contract surface” and `lib/contract/default.nix` expose concrete `vk-qcom`,
      `adreno-dfs`, and `hexagon` selectors before capabilities exist. `latencyMode` has no testable service
      semantics. `upperLimbVisibility` is a runtime shell/client policy, not a hardware device fact.
    - Conversely, perception says camera geometry extends `spatial.adaptation.camera`, but that option is
      currently only `{ backend = ...; }`; device-contract §`spatial.adaptation.*` specifies no camera
      geometry schema at all.
    - There are no assertions tying passthrough to Monado + zxr, ensuring hand cutout has passthrough, or
      rejecting unsupported backend/device combinations.
    - **Resolution:** for the prototype, expose only `enable`, `depthBackend = auto|classical|none`, and
      `handCutout.enable`; keep vendor backend overrides in developer/device capability data. Put runtime
      visibility policy in shell configuration/protocol. Add a typed camera array schema: role/source,
      stereo sync group, stream format/rate, intrinsics/distortion, head/camera extrinsics, calibration
      URI/version, timestamp clock, exposure/readout model, colour metadata, dmabuf capabilities, and matte
      source. Add cross-field assertions and qualification gates.

15. **The first implementation is obscured by premature backend and artifact scope.**
    - The four-artifact distinction in research 15 §1 is analytically useful, but the compositor wire
      contract only needs the artifacts it consumes. Likewise, a stable depth snapshot is useful, but four
      named backends and Hilbert-native transport are not needed to prove hand occlusion.
    - Perception Tier 0 says it produces hard alpha and hand depth with “no camera pixels,” yet the top-layer
      equation requires premultiplied `αF`. It does not say where `F` comes from.
    - **Resolution:** the minimal proof is: one camera path; one fixed/proxy passthrough background; Mercury
      capsules; one global forced-visible policy; and a compositor pass that uses the capsule mask to reveal
      a separately preserved passthrough image in front of one opaque virtual cube. Then add classical
      stereo for metric ordering. Defer per-client policy, learned depth, alpha matting, foreground
      estimation, and vendor accelerators until that path and P-1 work.

## Non-blocking concerns

- Perception §“Depth backend” requires confidence from every backend but permits fabricating it. Define a
  minimum confidence contract and treat fabricated confidence as a distinct, lower-capability mode.
- `latencyMode` needs measurable semantics per backend—target geometry cadence, history bound, resolution,
  and permitted power—not two marketing labels.
- Specify finite staleness bounds separately for colour, geometry, matte, and joints. “Drop if stale” is not
  actionable, and a stale hand matte has a much shorter safe lifetime than room geometry.
- Require monotonic snapshot IDs and define camera restart, calibration-version change, pool replacement,
  and service-crash recovery. No old-calibration frame may survive a version transition.
- Perception §“Hole-fill priority” adopts colour history but does not specify how recirculated colour avoids
  drift; research 13 §5.4 records an actual rounding-to-black failure mode.
- The privacy boundary is directionally right, but clients still need interaction input. Keep raw frames,
  mattes, and dense hand depth private; specify whether ordinary OpenXR joints suffice or add mediated
  hit-test/contact queries with explicit leakage analysis. Do not casually add a client-visible depth map.
- A “hands present” bit, policy-dependent visual result, and timing can themselves be observable. Define
  permissions and behavior for untrusted clients, screenshots, remote desktop, and diagnostics.
- Resource contention is missing. Camera ISP, Mercury, SLAM, Vulkan stereo, HTP matting, zxr rendering, and
  Monado reprojection share memory bandwidth and thermal budget. Qualification must test them concurrently,
  not as isolated backend benchmarks.
- The full anti-wobble catalogue is evidence-based but too large for the first pass. Add mechanisms only
  against measured artifacts; otherwise the prototype will be impossible to attribute or tune.
- The full-resolution luma guide in every `DepthFrame` may duplicate a camera plane and increase transport.
  Make it a referenced plane/pool slot, not necessarily copied snapshot payload.
- The hand service’s independence from Mercury is correct, but “full-frame fallback” needs a cost and
  cadence budget; otherwise the exact overlap failure it addresses can trigger overload.
- Upper-limb, sleeves, held objects, interlocked hands, and matte-source camera are already honestly open in
  perception §“Open questions” and research 15 §9. They are release blockers per device, not defects in the
  prototype plan.
- The typo in perception §“The decomposition” citing ADR 0007 for the shared service should point to ADR
  0008; stale decision references are dangerous in an architecture this timing-sensitive.

## Things done well

- The design correctly makes the display-rate warp non-blocking and separates camera, geometry, and display
  cadence. This is the strongest invariant and should be preserved.
- It treats pose-at-exposure as a blocking dependency rather than a tuning detail, and cites the 4 ms
  sensitivity without pretending it has already been achieved.
- Research claims are generally well qualified. In particular, Adreno DFS, QCOM image-processing
  extensions, HTP access, and “12 ms” passthrough are not made mandatory.
- Classical stereo is retained as the portable algorithmic fallback; same-side camera mapping, original-
  colour sampling, confidence, edge handling, and rendered-view evaluation are all well motivated.
- The distinction among joints, segmentation, matte, foreground colour, and hand depth prevents a common
  category error. Keeping Mercury optional for the image service is also correct.
- The privacy boundary—clients request policy rather than receiving camera imagery—is worth preserving.
- The metrics separate colour age, geometry age, and late-pose response and ask for tail distributions.
  That is materially better than one undifferentiated “passthrough latency” number.
- The open-questions sections are candid. The needed change is to promote a few of them—camera transport,
  pose clocking, base-hand removal, and runtime reprojection—from backlog questions to explicit gates.

## The one thing most likely to sink this

The highest risk is **not depth quality; it is obtaining synchronized, calibrated camera frames from the
target BSP with trustworthy exposure timestamps, mapping those timestamps into Monado’s pose-history clock,
and moving the frames into the perception/compositor GPU path without an unbounded copy or wait**. Perception
§“Shared invariants,” research 13 P0, ADR 0008 §“Consequences,” and research 16’s BSP questions all recognize
pieces of this, so the risk is not hidden. But the design does not yet confront it as one end-to-end,
device-specific kill test: ADR 0008 assumes the frameserver and historical pose API close the path, while the
contract has no camera geometry/timing schema and no transport proof. Make the P-1 spike above the first
deliverable and require it to pass before investing in backend abstraction, matting models, or protocol
policy.
