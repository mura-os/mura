# ADR 0010: Persona avatars — semantic control space (versioned) + Monado-side driver

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [avatar-persona.md](../avatar-persona.md), research
[24-avatar-representation-enrollment](../../research/24-avatar-representation-enrollment.md),
[25-avatar-driving-sensing](../../research/25-avatar-driving-sensing.md),
[26-codec-avatar-route](../../research/26-codec-avatar-route.md),
[27-avatar-claims-audit](../../research/27-avatar-claims-audit.md). Relates to
[adr/0006](0006-compositor-strategy.md) (the avatar runtime is a zxr client) and
[adr/0008](0008-perception-services-placement.md) (Monado owns sensors and the sensor clock
domain — extended here from cameras to expression/gaze).

## Context

A Persona system needs a control interface between the *driver* (headset sensing) and the *saved
avatar asset* (personal learned representation). Three candidate control-space families exist:

1. **Semantic controls** — blendshape weights + gaze + jaw + eyelids + head pose. The only format
   every Linux-reachable sensor path emits today: 70 FB2 weights end-to-end through
   WiVRn → Monado → `oxr_face_tracking2_fb.c` (verified, [25 §1](../../research/25-avatar-driving-sensing.md));
   HTC/ANDROID variants have the same plumbing; add-on trackers (Baballonia) and audio models emit
   ARKit-52 subsets.
2. **Personal learned coefficients** — an avatar's own basis weights (RGBAvatar's 20, MATCH-GEM's
   150 PCA components). Ideal *inside* an asset; **not interchangeable across people** —
   independently fitted bases have arbitrary meaning, ordering, and sign.
3. **A shared learned latent** — Meta's 256-d universal expression code. Highest ceiling, but
   [26](../../research/26-codec-avatar-route.md) verified: no trained encoder checkpoint is
   public; a code is only meaningful relative to a specific decoder checkpoint; new-person
   enrollment runs through closed registration tooling (the MATCH/TEMPEH bridge is a credible
   but undemonstrated open attack); and headsets without face cameras can never use the encoder
   family at all.

Separately, the driver has to live somewhere, and the same placement forces from ADR 0008 apply:
one clock domain, no second sensor owner, nothing on the compositor's display path.

## Decision

**1. The v1 driver↔asset wire format is a semantic control space** — the Unified-Expressions
factoring (88 shapes + a separate typed gaze/eyelid/pupil block + jaw + head pose), carried in a
`ControlFrame` whose every channel bears `{validity, confidence, source}` and whose timestamp is
Monado-monotonic sample time. FB2 remains the OpenXR-facing wire schema (what Monado serves);
ARKit-52 is the degraded-mode emitter schema only. Specified in
[avatar-persona.md §control-interface](../avatar-persona.md).

**2. The control space is versioned as a *pair*** — `control_space = { id, kind: semantic-v1 |
latent, dim, decoder_binding? }`. The protocol reserves an opaque float-vector channel bound to a
named decoder artifact hash, plus per-user calibration-blob slots keyed
`(control_space, driver)`. Renderers ignore control spaces they don't declare. This is the entire
hook a learned-latent v2 driver needs; nothing else about the route is v1 work.

**3. Assets carry adapters; the system never unifies control spaces.** A per-person adapter
(inside the asset) maps the semantic wire format to the asset-native input (e.g. the 129-dim
FLAME vector RGBAvatar consumes, [24 §1.1](../../research/24-avatar-representation-enrollment.md)).
Personal coefficients never travel between arbitrary drivers and avatars.

**4. The avatar driver service is Monado-side.** The v1 topology decision (correcting the
in-process/sibling conflation the perception review flagged for ADR 0008 and this ADR repeated):
Monado **owns** the sensor devices and clock; the driver's *execution* placement is decided at
implementation against one specified boundary — if a sibling, a defined source-stream +
virtual-device registration protocol; if in-process, a stated ABI and crash-containment story —
it is not both. The driver consumes `get_face_tracking` weights and gaze poses already rebased
into the server clock, applies calibration and the degraded-mode ladder (visual → add-on mouth
camera → audio-inferred mouth/jaw → procedural, each flagged in `source`), and **re-publishes
synthesized weights as a *derived* Monado face device** under these rules (correcting the
recursion/lossiness the review caught): physical-source and derived-output roles are distinct;
the driver never subscribes to its own descendants; which device serves the static `face` role
is an explicit system policy, not an accident of registration order; and the published FB2 view
is documented as lossy (UE-88 does not fit FB2-70; per-channel source/lineage metadata does not
survive the extension). The avatar *runtime* is an ordinary zxr client and holds no privileged
access.

## Rationale

- **Availability beats ceiling for v1.** Semantic controls are served by verified, upstream code
  today on real devices (Quest Pro, Galaxy XR); the latent route's missing pieces are research
  ([26 §verdict](../../research/26-codec-avatar-route.md)). The reserved hooks make the upgrade
  additive instead of a rewrite.
- **The PCA-non-interchangeability argument** kills the tempting "just stream the avatar's own
  coefficients" design for any multi-avatar/multi-driver system; it survives only inside the
  asset boundary.
- **Clock/placement symmetry with ADR 0008:** expression weights are sensor data with the same
  timestamp-domain requirement as camera frames; WiVRn's rebase
  (`offset.from_headset(face->time)`) is the working model. A driver that opened its own sensor
  path would recreate the two-clock-domain problem ADR 0008 exists to prevent.
- **Re-publication keeps the ecosystem coherent:** one face-device surface (the OpenXR
  extension) regardless of which rung of the ladder produced the weights, instead of a private
  avatar-only signal path.

## Consequences

- Device contract gains `mura.xr.sensing.*` (declared facts: gaze, eyelid, face-weight
  source/schema, mouth camera, mic channels) populated from the verified expression/gaze matrix in
  [25 §3](../../research/25-avatar-driving-sensing.md) and, for microphone channels, the
  application-facing native-source qualification in
  [43 §1.2/§10](../../research/43-microphone-native-linux-capture-audit.md), and a minimal
  `mura.xr.avatar.enable`. Physical microphone count and WiVRn relay width never populate
  `micChannels`.
  Per-device **S-1 sensing** and **R-1 render** kill-gates precede any model/renderer investment
  ([avatar-persona.md §kill-gates](../avatar-persona.md)).
- The audio rung needs a small upstream-shaped piece: a Monado `xrt_device` registering
  `XRT_INPUT_FB_FACE_TRACKING2_AUDIO` (the state tracker already routes it; no device registers
  it today — verified). That device exists only after the target reaches research/43's R4 native
  capture-source gate; the subsequent model/timestamp checks remain S-1.
- The persona asset is biometric data with **three distinct trust classes** (the review caught
  the earlier wording contradicting remote rendering): the trusted local runtime holds the
  user's own asset; **untrusted local apps** see only composited output — never assets,
  controls, or sensing; a **remote peer's trusted runtime** may receive the asset only under
  explicit consent via a dedicated Persona-sharing mode (transfer/retention/revocation to be
  specified; until then, remote uses the rendered-RGBD observer mode).
- **Not ratified here:** the enrollment tool's internals (tracker choice, representation
  training), the exact asset container encoding, and any latent-route work beyond the reserved
  hooks — all backlog, most gated on S-1/R-1.

## Alternatives considered

- **Personal-coefficient wire format** (stream RGBAvatar basis weights / GEM PCA coefficients):
  rejected — coefficients are asset-local; cross-people/driver interchange is meaningless
  (research 24/26). Retained *inside* the asset.
- **Shared learned latent as the v1 interface:** rejected — unrecoverable today for new persons
  without closed tooling; device-dependent (needs face cameras); couples every renderer to a
  decoder checkpoint. Reserved as the versioned v2 space.
- **ARKit-52 as the internal namespace:** rejected — loses FB2/UE tongue and per-corner lip
  detail; it is the degraded-mode schema that audio emitters produce, not a superset
  ([25 §2](../../research/25-avatar-driving-sensing.md)).
- **OSC (`/avatar/parameters/*`) transport:** rejected — the community's app-level hack;
  Mura has Monado's device layer and a typed protocol.
- **Compositor-side driver:** rejected for the ADR 0008 reasons (second clock domain, display
  path risk); the compositor consumes the avatar as a zxr client's colour+depth, nothing more.
