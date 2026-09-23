# ADR 0016: The places model — typed frame graph, decomposed currency

**Status:** accepted (draft)
**Date:** 2026-09-23
**Context sources:** [34-workspace-models](../../research/34-workspace-models.md) (the evidence
base and its §8.5 questions), [31-kwin-vr](../../research/31-kwin-vr.md) §2.9–2.12 (follow-mode
hysteresis, grab-all, transition choreography), [36-vr-shell-interaction-patterns](../../research/36-vr-shell-interaction-patterns.md)
(placement/recenter convergences), design elaborated in [places-model.md](../places-model.md).
Composes with [ADR 0009](0009-spatial-mapping-architecture.md) (anchors, corrections contract,
reloc), [ADR 0012](0012-de-modularity-spinout-seams.md) (pager seam, policy modules),
[ADR 0014](0014-toplevel-delegation-protocol.md) (restore slots), [ADR 0015](0015-docked-desktop-mode.md)
(docked presentation).

## Context

The registry's largest structural gap (§8 item 1): nothing defined local workspaces/places —
while sharing mode 5 replicates a placement graph, restore needs place identity, the pager needs
a model to consume, and docked mode needs a per-place flat layout. Doc 34 surveyed the desktop
and spatial precedents and left five design questions (§8.5). The conversation preceding this ADR
settled the abstraction: "head-anchored vs room-anchored" is two prominent members of a larger
family, and the right primitive is a **typed reference-frame graph** grounded in OpenXR `XrSpace`
semantics, with attachment behavior and layout pushed into policy.

## Decision

### 1. The three-layer split

**Layer 1 — the frame graph — is the model** (authority plane): typed frames (map anchor / LOCAL
/ head / hand / docked plane / shared-peer / vehicle) carrying persistence, stability-contract,
motion-class, and shareability typing; places parented to frames; windows members of places;
reparent verbs (pin / summon / grab-all / assign-to-frame). **Layer 2 — attachment constraints —
is pluggable policy** (rigid, lazy-follow-with-hysteresis, billboard, tether) under the
constraint-6 velocity clamps. **Layer 3 — intra-place layout — is per-place policy** (free 3D,
curved band, screen quad, docked flat). Specified in [places-model.md](../places-model.md) §1–§3.

### 2. The five §8.5 answers

1. **Membership cardinality: exclusive plus an overlay class.** One place per window; head/hand
   sticky members are overlay-class (excluded from currency competition, never location-current).
   Restore and mode-5 get exclusivity's simplicity; the Activities cardinality post-mortem
   ("session restoration is at odds with the cardinality of windows to activities" — Edmundson,
   [34 §1.2](../../research/34-workspace-models.md)) is decisive against sets.
2. **"Active" is decomposed currency, not a bit.** Per-consumer selectors (spawn / pager /
   notification-routing / restore / sharing / docked) over the ladder *selection > focus >
   location > sticky-last*, with **offer-never-yank** (location events never forcibly rearrange —
   ADR 0009's corrections rule generalized) and the C1–C7 reconciliation table
   ([places-model.md §4](../places-model.md)). The `ext-workspace` `active` bit reports the
   pager selector only. Walks emit enter/exit events with hysteresis plus partial-transition
   offsets (KWin `currentChanging` precedent).
3. **Group axis: groups = reference frames.** ext-workspace groups map to frames (world / head /
   hand / docked / shared), making `assign` the protocol-visible reparent verb; room
   identity/labels ride the zxr workspace extension (upstream groups are identity-less).
4. **Lifecycle: transient by default, pin to persist.** Placement implicitly creates transient
   places (GNOME shape); pinning names, persists (anchored store), and upgrades them to objects
   independent of members — empty pinned places are legal. **Entry policy** (activate
   attachments / launch / summon on place-entered) is a pinned-place property under an explicit
   per-place consent grant, suspended during capture and guest sessions.
5. **No second axis.** Same-anchor place-swap (doc 34 §8.1's B-variant) plus app-driven
   switching-as-policy covers the context cases (Horizon's two pinned sets); an anchored place
   already *is* a context; the Activities post-mortem is heeded.

## Rationale

- **The model is minimal where every surveyed system that grew was later shrunk** (Activities'
  session semantics dropped; KDE's own cardinality argument), and expressive where XR genuinely
  differs (frames, currency, entry policy). Wherever ecosystems diverged, the divergence was
  policy-shaped — so it lands in layers 2/3 and per-consumer selectors, not in model structure.
- **XrSpace grounding** keeps layer 1 standards-shaped (the spatial XDG-grounding analog) and
  keeps the runtime boundary honest: frames the runtime owns (VIEW, LOCAL, anchors) are consumed,
  not reinvented.
- **Groups-as-frames** turns the hardest UX transition (between anchor systems) into the standard
  protocol operation the pager already has, and gives docked mode and mode 5 their group-shaped
  answers ([34 §8.4](../../research/34-workspace-models.md)) for free.
- **Decomposed currency** is forced by a real conflict set (C1–C7), each of which has a boring
  answer once "active" stops being one bit — and the offer-never-yank rule extends a contract
  users already depend on (ADR 0009) from geometry to context.

## Alternatives considered

- **Single active bit** (all surveyed desktops): rejected — C1/C3/C5 have no correct value;
  forcing one reintroduces the yank behavior the mapping contract exists to prevent.
- **Location-always-wins currency** (the "smart room" instinct): rejected — summon (C1) and
  docked (C5) are explicit user acts that must not be overridden by mere presence; and it makes
  follow places (C4) impossible.
- **Two-axis place × activity** (KWin shape): rejected on KDE's own post-mortem + matrix cost;
  the B-variant expresses the residual cases inside one axis.
- **Membership as sets** (KWin desktops shape): rejected — restore and mode-5 cardinality; the
  overlay class covers the legitimate "with me everywhere" need.
- **Frames as zxr-private-only** (skip ext-workspace): rejected — forfeits stock pagers and the
  ADR 0012 §1 standard seam; the extension carries only what upstream cannot.
- **Always-persistent places** (no transient tier): rejected — reintroduces management burden
  the dynamic models eliminated, and M1's "shell pins windows" baseline ships before any
  workspace UX exists.

## Consequences

- [places-model.md](../places-model.md) is the normative design; the registry space-model row
  moves missing → specified; gap-list #1 closes as designed-not-built.
- The pager/overview consumes ext-workspace + the zxr workspace extension fields enumerated in
  places-model §6; the toplevel-membership extension (cosmic-info shape) joins the zxr
  shell-integration family (ADR 0012 §4 item 1 refined).
- Restore (ADR 0014/doc 30 A2), docked (ADR 0015), and mode 5 (spatial-sharing §5) bind to
  `place_id`s and the state-sync-shaped place document per places-model §7.
- Entry policy adds a settings-model surface (per-place grants) and a consent interaction with
  capture badging (doc 17) — noted for the settings and sharing designs.
- New open items tracked in places-model §9 (place volumes, multi-user ownership, vehicle
  frames, the extension XML draft timing).
- Budget impact: none on the frame path; see places-model §8.
