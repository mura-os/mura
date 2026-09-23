# Red-team review: Persona avatar architecture

## Verdict

**Not sound enough to implement as written.**
The high-level split—offline enrollment, portable asset, Monado-adjacent driver, and ordinary zxr
renderer—is directionally strong.
Choosing a semantic v1 instead of pretending the Ava-256 latent route is product-ready is also
correct.
But the keystone does not exist: the design treats a per-person UE-88 → FLAME-129 adapter as an
asset field while specifying neither the paired data, supervision, training owner, calibration
protocol, nor acceptance metric that could produce it.
That is a research problem mislabeled as an implementation detail.

Three direct contradictions compound it.
The ADR promises that untrusted clients never receive assets or controls, while remote local
rendering requires both.
The driver consumes Monado's single face role and republishes another device into that role without
loop avoidance.
An alpha-splat avatar is called an ordinary T1 zxr client even though zxr's baseline supports
nearest opaque samples only.
The Nix contract advertises the feature without representing native versus WiVRn-relayed sensing,
recording either kill-gate result, or asserting any usable sensing floor.

Proceed only with three narrow spikes: prove a supervised UE→FLAME mapping on paired recordings;
define a non-recursive source/derived-device topology in Monado; and establish an opaque/coverage
avatar submission against minimal zxr T1.
Do not freeze the asset or `ControlFrame` schemas before those results.

## Blocking issues

1. **The per-person adapter is an unsolved research problem presented as serialization.**
   - Avatar design §“The control-interface specification” and §“The asset format specification” say the asset carries a learned per-person `UE→FLAME-129` adapter.
   - Phone enrollment in §“The four-stage decomposition” observes images and Metrical Tracker FLAME parameters; it never observes the same performance through the target headset's UE/FB2 path. It produces no paired `(UE, FLAME-129)` supervision.
   - Research 25 §2 shows the nearest verified learned mapping (OFERA) uses paired recordings. Research 24 §7.3 explicitly leaves headset-sensor → FLAME mapping open.
   - The MATCH analogy does not close this gap: research 26 Part 4 says its same-frame bridge has no adapter code and is empirically unverified; it concerns Ava latent↔MATCH controls, not UE→FLAME for a phone-enrolled user.
   - **Resolution:** specify capture, supervision, model/operator family, owner, expression coverage, held-out metric, and threshold. Until a paired-data spike passes, call this a heuristic global mapping or require post-enrollment headset calibration.

2. **`semantic-v1` is not a control-space specification.**
   - Avatar design §“Driver → runtime” gives prose—UE-88 plus gaze, lids, jaw, and head pose—but no normative channel IDs/order, ranges, neutral values, units, handedness, axes, rotation representation, coordinate frames, NaN policy, or unknown-channel rule.
   - ADR 0010 §“Decision” calls this a wire format yet points back to that sketch. `{id, kind, dim}` does not define 88 float semantics.
   - Research 25 §2 says UE is a factoring borrowed from VRCFT, not a frozen Mura registry, and notes differing roll/shrug and eye factoring.
   - “Semantic-v1 needs no binding” is false in the compatibility sense: it needs an exact registry/version identity even if it needs no learned decoder.
   - **Resolution:** publish a machine-readable registry with stable IDs, canonical units/frames, legal ranges, defaults, and version-compatibility rules.

3. **Per-channel validity cannot be recovered from FB2.**
   - `ControlFrame` requires `{validity, confidence, source}` for every channel (avatar design §“Driver → runtime”).
   - Research 25 §1–2 establishes that FB2 has one global validity bit, two regional confidences, and one eye-following validity bit—not 70 independent validity/confidence observations. HTC has per-set activity; ANDROID has three region confidences.
   - Copying one regional scalar to many UE channels does not create channel-level evidence. Splitting or deriving channels during FB2→UE conversion further weakens provenance.
   - The schema cannot distinguish “source schema lacks this channel,” “temporarily unobserved,” “sensor failed,” and “derived from another channel.”
   - **Resolution:** retain set-level validity and confidence-region IDs, source-channel lineage, and optional channel overrides; add explicit `unsupported`, `unobserved`, and `derived` states.

4. **One timestamp cannot describe the degraded-mode ladder.**
   - Avatar design §“Driver → runtime” assigns one Monado-monotonic sample timestamp to face, gaze, lids, head pose, audio inference, and procedural values.
   - Research 25 §1 establishes separate face and gaze devices. Research 25 §4 establishes one-second audio windows producing at 30 FPS. Those samples cannot honestly share one physical observation time.
   - A fresh gaze, interpolated face sample, delayed audio mouth, predicted head pose, and procedural blink under one timestamp conceal exactly the latency the design says must be separated.
   - Monado monotonic time is also host-local and meaningless to a peer; spatial sharing §6 requires explicit remote clock mappings, but avatar design §“The runtime renderer” defines none.
   - **Resolution:** carry frame sequence/epoch plus per-group observation and production times, clock-domain ID, age, and prediction/interpolation state. Define sender→receiver clock mapping and reset behavior.

5. **Head pose and gaze have no coherent sampling or coordinate contract.**
   - Avatar design §“Driver → runtime” says head pose comes “from OpenXR views,” while §“The driver service” makes the driver Monado-side.
   - OpenXR views are located for a requested/predicted display time; face weights have sensor sample times. No sampling time, reference space, or tracking-origin-reset behavior is specified.
   - Research 25 §1 verifies combined gaze pose plus FB2 eye-following weights, but the design gives no precedence, timestamp alignment, or conversion into avatar eye-joint coordinates.
   - Sending world-space head pose can leak tracking-origin/room information; a peer needs a negotiated avatar-root transform, not the sender's raw local origin.
   - **Resolution:** separate local head motion from facial controls, define avatar-root/eye frames, sample each signal at a declared time, and make remote placement a sharing-layer transform.

6. **“In-process or sibling” repeats ADR 0008's placement conflation.**
   - ADR 0010 §“Decision” calls the service “Monado-side (in-process or a sibling behind Monado's transport)” while relying on direct `get_face_tracking`, gaze poses, server clock, and `xrt_device` publication.
   - REVIEW-perception blocking issue 7 already explains why ownership, execution location, and process transport are separate decisions under ADR 0008.
   - A sibling cannot call in-process `xrt_device` pointers or register a device without an IPC/plugin boundary carrying schema, lifecycle, clock, backpressure, permissions, and failure state.
   - **Resolution:** choose one v1 topology. If sibling, specify source-stream and virtual-device registration protocols; if in-process, state ABI, scheduling isolation, and crash-containment consequences.

7. **Re-publication is recursive and lossy as described.**
   - Research 25 §1 says WiVRn wires exactly one tracker to Monado's static `face` role. Avatar design §“The driver service” consumes that role and republishes synthesized weights as another face device.
   - If the derived device replaces the role, the driver can consume itself; if it does not, ordinary OpenXR clients continue seeing the original source. No source handle or recursion guard exists.
   - A mixed internal frame may contain visual eyes, audio mouth, procedural lids, and defaults, while FB2 exposes one requested VISUAL/AUDIO source and no per-channel source/validity. UE's 88 shapes also cannot fit losslessly into FB2's 70.
   - ADR 0010 §“Rationale” therefore cannot promise one coherent face-device surface without documenting source selection and metadata/channel loss.
   - **Resolution:** define distinct physical-source and derived-output roles, prohibit descendant subscription, publish only coherent FB2 views, and document every loss.

8. **The persona asset sketch is not a portable container specification.**
   - Avatar design §“The asset format specification” lists directories, while ADR 0010 §“Consequences” explicitly says exact encoding is not ratified. Calling it a specification is internally inconsistent.
   - Missing data include byte encoding, shapes/strides, endianness, compression, colour space, opacity and covariance conventions, units/axes, face winding, barycentrics, skinning, joint rest transforms, and extension handling.
   - Research 24 §2.1 verifies research `.ply` fields, but those conventions are not imported normatively. `adapters/` does not define an operator set, so “no-Python C++/Vulkan runtime” is untestable.
   - No limits bound Gaussian count, tensors, graph depth, allocations, or decompression; a peer-supplied asset is an unbounded parser/GPU security surface.
   - **Resolution:** write a binary schema, conformance corpus, deterministic validation, resource limits, and a small closed adapter operator set before promising portability.

9. **The design overstates RGBAvatar portability and understates FLAME coupling.**
   - Avatar design §“The asset format specification” calls RGBAvatar “MIT” and its 129 inputs “semantic,” but research 27 §18–19 records separately licensed FLAME assets and materially different FLAME-version licenses.
   - Research 24 §1.1 shows 100 inputs are coordinates in a particular FLAME expression basis, with specific rot6d/matrix conventions. They are model/version coordinates, not universal named semantics.
   - Shipping template topology, eyelid offsets, derived teeth, or rig basis data may carry redistribution obligations; provenance metadata does not grant rights.
   - FLAME 2023 Open cannot be assumed to cover an older `generic_model.pkl` used by audited code (research 27 §19).
   - **Resolution:** pin exact model/topology/version, prove export rights for every embedded datum, and provide a clean licensed reference asset before describing the representation as MIT-portable.

10. **The privacy boundary contradicts remote local rendering.**
   - ADR 0010 §“Consequences” says untrusted clients see rendered avatars but “never the control stream, sensing data, or the asset.”
   - Avatar design §“The runtime renderer” says the receiver holds the asset, receives `ControlFrame`s, and renders locally. The receiver gets precisely what the ADR says clients never get.
   - Avatar design §“The asset format specification” invokes the camera-frame privacy boundary without encryption-at-rest, user-bound keys, ACLs, sandboxing, audit, retention, revocation, or deletion.
   - An ordinary zxr client loading the raw asset is privileged by construction; no trust class prevents extraction.
   - **Resolution:** define trusted local runtime, untrusted app, and remote peer separately. Choose asset transfer, rendered RGBD, or a revocable rendering service; then state an enforceable privacy boundary.

11. **`spatial-sharing.md` does not implement the claimed avatar transport.**
   - Avatar design §“The runtime renderer” says sharing carries controls and a one-time biometric asset under consent.
   - Spatial sharing §1–4 defines video, RGBD observer groups, Wayland proxying, and placement graphs—not an asset object, resumable transfer, control stream, identity binding, consent language, cache policy, or deletion.
   - Spatial sharing §6 treats remote 3D as rendered colour+depth with clock mappings, a different trust and viewpoint model from transferring a reusable face.
   - Manifest hashes provide identity/integrity, not authorization, confidentiality, license compatibility, decoder compatibility, or revocation.
   - **Resolution:** add a separately reviewed Persona-sharing mode or use existing mode-3 RGBD for v1. Reserved hooks are not an implemented biometric transport.

12. **A Gaussian avatar does not fit zxr's opaque T1 contract without an edge/coverage decision.**
   - Research 24 §2.1 verifies per-Gaussian opacity and standard 3DGS alpha splatting.
   - Avatar design §“The runtime renderer” nevertheless calls it an ordinary colour+depth zxr client requiring nothing new.
   - zxr composition §2 guarantees nearest **opaque** visibility; §3 says cross-client transparency needs ordered/deep or stochastic samples and is post-MVP.
   - Hair, silhouettes, lashes, and holes have partial coverage. One depth cannot interleave another app behind those pixels; omitting alpha produces a rectangle or threshold halos. The zxr §7.2 contract names no validity/coverage mask.
   - **Resolution:** specify an opaque-cutout profile with coverage/depth rules and quantify edge loss; validate avatar intersections with 2D/3D content; defer true interleaving to T2.

13. **R-1 is circular and cannot gate “any renderer investment.”**
   - Avatar design §“Kill-gates” requires animated splats with compositor and reprojection active, while §“The runtime renderer” says this avatar is the first native 3D zxr application.
   - zxr composition §7.5 remains a design-stage M1–M4 sequence. R-1 depends on a compositor protocol and renderer that do not exist.
   - A gate cannot precede construction of its test article; a standalone splat benchmark, conversely, cannot prove compositor coexistence.
   - **Resolution:** split R-0 (minimal Vulkan animation+splat), Z-1 (generic zxr T1 acceptance), and R-1 (integrated sustained run), with bounded investment at each stage.

14. **The contract models software paths as immutable device facts.**
   - `lib/contract/default.nix` §`mura.xr.sensing` calls values per-device facts, but Quest Pro `fb2-visual` is specifically a WiVRn client/vendor-runtime path (research 25 §1, §3), not intrinsic Linux hardware exposure.
   - The same hardware used natively, through another streamer, or without permission differs. Steam Frame is the inverse: hardware exists while Linux API exposure is unknown.
   - `faceWeights` conflates schema (`fb2/android/htc`) and source (`visual/audio`); `mouthCamera = "internal"` may describe input to a closed vendor tracker despite no OS camera access.
   - **Resolution:** separate hardware, execution location, transport/runtime, schema, provenance, and OS accessibility. WiVRn relay capability belongs to a runtime-path profile.

15. **The two avatar assertions do not enforce the architecture.**
   - `lib/contract/default.nix` §“Cross-field assertions” checks only that FB2-audio has a mic and avatar enable has a non-`none` runtime plus zxr.
   - Research 25 §5 recommends tying each schema to its Monado build feature; no such assertion exists.
   - The contract permits avatar enable with no gaze, face, lids, mouth camera, or microphone, though avatar design §“Kill-gates” calls Lynx's floor head pose plus mics. It also permits lids with no face weights and internal mouth camera with no visual tracker.
   - Any non-`none` runtime passes although ADR 0010 §“Decision” depends on Monado-side virtual-device support.
   - **Resolution:** add typed capability/path submodules and assertions, or keep the feature out of the public contract until semantics stabilize.

16. **Neither kill-gate is a contract or qualification gate.**
   - Avatar design §“Kill-gates” gives S-1 the right shape, but `N seconds`, “bounded jitter,” and “bounded skew” lack numeric thresholds.
   - R-1 has no named device, resolution, render scale, percentile, duration, thermal/power ceiling, or dropped-frame allowance; it is unclear whether both 40k and 60k must pass or whether 72 Hz means average.
   - ADR 0010 §“Consequences” says both precede investment, while `qualification.acceptanceTests` is only a free-form string list and `avatar.enable` requires no result.
   - **Resolution:** name one BSP/device and binary thresholds, represent typed pass/fail/evidence, and assert required results for the relevant support tier.

17. **The enrollment boundary is not real at the interoperability seam.**
   - Avatar design §“The four-stage decomposition” excludes enrollment internals, while §“Open questions” says the asset adapter absorbs tracker differences.
   - Research 24 §6 shows Metrical Tracker emits a particular FLAME model, camera convention, eyes, lids, and poses. Pixel3DMM/SHeaP output is not interchangeable merely because both say “FLAME.”
   - Runtime and validator must know topology, model version, transforms, neutral capture, expression coverage, eye/teeth completeness, calibration quality, and adapter fit error. Those are enrollment-export requirements.
   - **Resolution:** keep enrollment implementation off-image, but own a normative exporter contract, validator, quality report, and reference conformance set. “Out of OS scope” cannot mean unspecified.

## Non-blocking concerns

- **The compute argument ignores memory traffic.** Avatar design §“Asset format” calls 13M MADs tractable, but research 24 §3 gives ~53 MiB fp32. The 20×10 bases alone are ~52 MiB/frame, roughly 3.7 GiB/s at 72 Hz before mesh, sort, raster, and output. R-0/R-1 must measure bandwidth, not only operations.

- **“Hundreds of bytes/frame” is optimistic.** Avatar design §“Runtime renderer” carries at least 88 values plus metadata, gaze, lids, jaw, head pose, IDs, and times. A naïve encoding exceeds 1 KiB/frame. Still modest, but it needs packing numbers.

- **Confidence values are not cross-vendor probabilities.** Research 25 §2 lists FB2 regional confidence, ANDROID state/calibration, and HTC activity. Avatar design §“Driver service” should prohibit treating them as calibrated interchangeable scalars.

- **Calibration lifecycle is absent.** ADR 0010 §“Decision” says calibration is applied, but no per-donning neutral/rest capture, drift detection, eye alignment, range normalization, hardware/user keying, or firmware invalidation is defined.

- **Fallback conflates observation and rendering.** Avatar design §“No silent zeros” keeps missing channels invalid, while §“Asset format” provides neutral fallback and §“Driver service” holds gaze at rest. Specify invalid observation versus effective rendered value.

- **Procedural blink wording conflicts.** Avatar design §“Driver service” times blinks to saccades, then forbids fake saccades without gaze. State which measured signal triggers them and label speech/random timing procedural.

- **The add-on path has distribution constraints.** Research 27 §18 records Baballonia's custom non-commercial copyleft license. Avatar design §“Driver service” should distinguish architectural precedent from shippable code.

- **Asset transfer needs a lifecycle.** Avatar design §“Runtime renderer” implies one-time transfer of an approximately 53 MiB biometric asset (research 24 §3). Define resumability, quota, deduplication, update, revocation, and retention.

- **R-1 has no quality gate.** Research 24 §5 identifies eyes and mouth as the uncanny-valley risk, while avatar design §“Kill-gates” measures performance only. Add gaze, blink, teeth, silhouette, and OOD-expression acceptance.

- **Optional eyeballs weaken the photoreal target.** Avatar design §“Open questions” leaves eyeball meshes undecided despite research 24 §5. A later reference profile should mandate one eye representation.

- **Control negotiation is missing.** Avatar design §“Driver → runtime” says renderers ignore undeclared spaces but defines no capability exchange, fallback, migration, error, or multi-version behavior.

- **A hash is not a decoder contract.** Avatar design §“Control-interface” uses `decoder_binding` content hash. Also specify artifact type, operator/tensor ABI, precision, canonical encoding, provenance/license, and runtime compatibility.

- **Remote data is hostile input.** Avatar design §“Runtime renderer” allows peer assets/controls but does not bound values, reject NaN/Inf, authenticate epochs, or constrain parser/GPU allocations.

## Things done well

- Semantic v1 with a reserved—not claimed—latent route matches research 26 §“Verdict”.
- Decoder binding follows the real coupling shown by research 26 §2.3 and §“Implications”.
- RGBAvatar selection uses audited eyes, lids, teeth, basis size, and OOD behavior from research 24 §1–5 rather than paper headlines.
- Keeping ARKit-52 as a degraded emitter preserves FB2/UE detail (avatar design §“Driver-internal namespace”; research 25 §2).
- “No silent zeros” is the right principle even though its metadata model needs revision (research 25 §4).
- The device matrix is honest about Steam Frame, Play for Dream, and Lynx (avatar design §“Kill-gates”; research 25 §3).
- Runtime costs remain separated as research 27 §16 requires.
- The 40–60k/72 Hz target is correctly called class-feasible but unproven in the open stack (research 27 §17).
- Relighting, body/hands, universal priors, and latent driving are honestly excluded in avatar design §“Non-goals”.
- No Python, research checkout, or network at render time is a strong future conformance boundary (avatar design §“Asset format”).

## The one thing most likely to sink this

The highest risk is **the missing supervised bridge between what a headset emits and what a
phone-enrolled RGBAvatar consumes**.
Avatar design §“Adapters, not unification” makes that bridge look like a tensor in `adapters/`.
Research 24 §7.3 leaves sensor→FLAME mapping open; research 25 §2 shows the learned precedent uses
paired recordings; research 26 Part 4 says even the better-aligned MATCH/Ava bridge is
undemonstrated.
Enrollment supplies FLAME tracks without UE observations.
Live use supplies UE observations without FLAME ground truth.
No container version, Monado placement, Vulkan optimization, or zxr composition closes that supervision gap.
Record one coverage-defined calibration performance with both verified FB2/UE and independently
fitted FLAME targets, fit the smallest allowed adapter, and set held-out jaw/lip/lid/gaze error
thresholds.
If that cannot preserve expression without person-specific paired capture, “enroll once on a
phone, drive from any headset” is false: v1 must choose a weaker generic mapping or require headset
calibration.
