# Avatar design backlog: disposition of the review

Triages [REVIEW-avatar.md](REVIEW-avatar.md) (GPT Sol red-team of the Persona avatar design).
Verdict there: *directionally strong; the keystone (the per-person adapter) was a research problem
mislabeled as serialization; three direct contradictions; do not freeze schemas before three
spikes.* Disposition: genuine doc errors fixed now; the adapter spike promoted to a named gate;
the rest recorded as pre-implementation / pre-release design work.

## Fixed now (in [avatar-persona.md](avatar-persona.md) and [adr/0010](adr/0010-avatar-control-space-and-driver.md))

| # | Review finding | Fix |
|---|---|---|
| 1 | UE→FLAME adapter presented as an asset field despite a supervision gap | Design now states the gap explicitly and the two honest v1 options (generic heuristic mapping vs per-user headset-calibration capture); **A-1 adapter spike** added as a schema-freeze gate. |
| 3 | Per-channel validity cannot be recovered from FB2 (1 validity bit + 2 region confidences for 70 weights) | ControlFrame reworked: channel `state` gains `unsupported`/`unobserved`/`invalid`/`derived`; set-level evidence carried via `lineage` (confidence-region / source-channel); per-channel values declared *derived*, not fabricated observations. |
| 4 | One timestamp cannot describe the degraded-mode ladder | ControlFrame now carries per-signal-group `t_observed`/`t_produced` + measured/interpolated/predicted state + `clock_domain_id`; remote use requires the sharing layer's clock mapping. |
| 7 | Re-publication recursive and lossy as described | ADR 0010 §4 rewritten: distinct physical-source vs derived-output roles, no descendant subscription, explicit face-role policy, FB2 view documented as lossy. |
| 10 | Privacy boundary contradicted remote local rendering | Three trust classes defined (trusted local runtime / untrusted local apps / consenting remote peer); remote v1 uses rendered-RGBD observer mode until a Persona-sharing mode exists. |
| 12 | Alpha-splat avatar is not an opaque T1 zxr client | "Nothing new required" claim removed; **opaque-cutout profile** specified (coverage threshold at silhouettes, edge loss quantified in Z-1); true interleaving deferred to T2. |
| 13 | R-1 circular (gates a renderer on a compositor that doesn't exist) | Split into **R-0** (standalone Vulkan splat bench) / **Z-1** (zxr T1 acceptance incl. cutout profile) / **R-1** (integrated sustained run), bounded investment per stage. |
| 6 | "In-process or sibling" repeated ADR 0008's conflation | ADR 0010 §4 now separates ownership (decided: Monado) from execution placement (one topology chosen at implementation against a specified boundary). |

## Promoted to a named gate: the A-1 adapter spike

The review's "one thing most likely to sink this": enrollment produces FLAME tracks with no UE
observations; live use produces UE observations with no FLAME ground truth. Before any schema
freeze: record one coverage-defined calibration performance captured simultaneously by a verified
FB2/UE source and an independently fitted FLAME tracker; fit the smallest allowed adapter;
evaluate held-out jaw/lip/lid/gaze error against thresholds. Outcomes: (pass) per-user
headset-calibration capture becomes a supported enrollment step; (fail) v1 ships the generic
heuristic mapping and says so. Either way the "enroll once on a phone, drive from any headset"
claim is amended to match reality.

## Pre-implementation design work (before building driver/runtime/asset code)

- **#2 semantic-v1 registry.** A machine-readable channel registry: stable IDs, order, units,
  coordinate frames, rotation representation, legal ranges, neutral values, NaN policy,
  unknown-channel rule, version-compatibility rules. "UE-88" names a factoring, not a registry;
  semantic-v1 needs exact registry-version identity even without a learned decoder.
- **#5 head-pose/gaze coordinate contract.** Declared sampling times and reference spaces;
  separate local head motion from facial controls; avatar-root/eye frames; remote placement as a
  sharing-layer transform (raw local origins never leave the host — also a privacy leak vector).
- **#8 asset container binary spec.** Byte encoding, shapes/strides, endianness, compression,
  colour space, opacity/covariance conventions, units/axes, winding, barycentrics, skinning, rest
  transforms, extension handling; a closed adapter *operator set* (else "no-Python runtime" is
  untestable); resource limits + deterministic validation (a peer-supplied asset is a parser/GPU
  attack surface); conformance corpus.
- **#11 Persona-sharing mode.** Asset object, resumable transfer, identity binding, consent
  language, cache/retention/revocation/deletion — a separately reviewed sharing mode;
  rendered-RGBD observer mode is the v1 remote path.
- **#17 enrollment exporter contract.** Enrollment stays off-image, but the OS owns a normative
  exporter schema + validator + quality report (topology, model version, transforms, neutral
  capture, expression coverage, eye/teeth completeness, adapter fit error) and a reference
  conformance set. Tracker outputs are not interchangeable merely because both say "FLAME"
  (research 24 §6).
- **#14 contract refactor.** Separate hardware presence / execution location / transport-runtime
  path / schema / OS accessibility (Quest Pro `fb2-visual` is a WiVRn-path fact, not a hardware
  fact; Steam Frame is the inverse). Split `faceWeights` schema from source. WiVRn relay
  capability belongs in a runtime-path profile.
- **#15/#16 assertions + gate wiring.** Tie each schema to its Monado build feature; sensing
  floor for `avatar.enable`; typed S-1/A-1/R-0/Z-1/R-1 pass/fail results in qualification with
  numeric thresholds and a named device, asserted per support tier. (Current stubs stay minimal
  until then, per the perception review's #14 lesson.)
- **#9 FLAME shipping rights.** Pin exact model/topology version; prove export rights for every
  embedded datum (template topology, eyelid offsets, derived teeth, rig bases — FLAME 2023 Open
  ≠ older `generic_model.pkl`, research 27 §19); provide a cleanly licensed reference asset
  before calling the representation MIT-portable.

## Pre-release / measurement items (non-blocking set)

- Memory-traffic reality: the 20-basis blend reads ~52 MiB/frame (~3.7 GiB/s at 72 Hz) before
  mesh/sort/raster — R-0 measures bandwidth, not just MADs.
- ControlFrame packing numbers (order-of-a-KiB/frame, not "hundreds of bytes").
- Confidence values are not cross-vendor probabilities; forbid treating them as calibrated
  interchangeable scalars.
- Calibration lifecycle: per-donning neutral/rest capture, drift detection, hardware/user keying,
  invalidation on firmware change.
- Observation-vs-rendered-value separation in fallback (invalid channel ≠ neutral rendered pose).
- Procedural blink trigger wording (which *measured* signal; label speech/random timing).
- Baballonia license (non-commercial copyleft): architectural precedent vs shippable code.
- Asset transfer lifecycle (quota, dedup, update, revocation, retention) once the sharing mode
  exists.
- R-1 quality gate additions: gaze/blink/teeth/silhouette/OOD-expression acceptance, not just
  frame timing (eyes+mouth are the uncanny-valley risk, research 24 §5).
- Reference profile decision on explicit eyeball geometry.
- Control-space negotiation (capability exchange, fallback, migration, multi-version).
- `decoder_binding`: artifact type + operator/tensor ABI + precision + provenance, not just a
  content hash.
- Remote data as hostile input: value bounds, NaN/Inf rejection, epoch authentication, parser/GPU
  allocation limits.

## Preserved (review "things done well" — do not regress)

Semantic v1 with a reserved (not claimed) latent route; decoder binding mirroring the real
code↔checkpoint coupling; RGBAvatar selection from audited code rather than paper headlines;
ARKit-52 kept as degraded-emitter only; no-silent-zeros; the honest per-device sensing matrix;
separated runtime cost boundaries; 40–60k/72 Hz called class-feasible-but-unproven; honest
non-goals; no-Python/no-network render-time conformance boundary.
