# ADR 0014: Toplevel delegation — specify an upstream-shaped protocol, consumer-first

**Status:** accepted (draft); implementation explicitly staged (see Consequences — nothing is
scheduled before the zxr 2D tier exists)
**Date:** 2026-09-23
**Context sources:** [32-toplevel-export-prior-art](../../research/32-toplevel-export-prior-art.md)
(R1–R22 + verdicts), [foreign-session-integration.md](../foreign-session-integration.md) (the
design), [31-kwin-vr](../../research/31-kwin-vr.md) (the named KDE positions),
[19-wayland-proxying](../../research/19-wayland-proxying.md). Composes with
[ADR 0006](0006-compositor-strategy.md), [ADR 0012](0012-de-modularity-spinout-seams.md),
[ADR 0013](0013-kwin-vr-disposition.md).

## Context

ADR 0013 rejected building on KWin VR but identified the integration shape both KWin maintainers
prefer — quoted verbatim in
[foreign-session-integration.md §1](../foreign-session-integration.md): **Vlad Zahorodnii**
("we could provide info about windows, thumbnails, perhaps a more convenient way to deal with
input, and let them compose") and **David Edmundson** ("we can forward windows buffers pretty
cheaply and a path to do application level input forwarding is definitely something we want to
pursue"). No protocol exists for it: capture protocols are damage-driven copies with no pacing
contract, no input path, and no surface tree ([32 §1](../../research/32-toplevel-export-prior-art.md));
protocol proxying (waypipe) forwards apps, not sessions. The missing seam — per-toplevel
zero-copy export with consumer-driven pacing and producer-retained shell authority — would let a
live KWin/Plasma (or any producer) session's windows float individually in Mura.

## Decision

### 1. Specify it now, as an upstream-shaped protocol

The seam is specified in this repository —
[`protocols/zspatial-toplevel-export-v1.xml`](../../../protocols/zspatial-toplevel-export-v1.xml) against
requirements R1–R22 — in the local experimental `zspatial` namespace, renamed `xx_` on
wayland-protocols proposal and `ext_` on promotion (governance rules verified in
[32 §7](../../research/32-toplevel-export-prior-art.md)). It is designed as a *general
inter-compositor seam*, not an XR-private one: nothing in it references XR concepts, so KWin,
COSMIC, or a 2D nested use case can adopt it independently — that is a precondition for
upstreaming and deliberate.

### 2. Ratify the four contested design points (from doc 32's verdicts)

- **Buffer release: producer-owned join (R8).** The consumer returns GPU-complete releases; only
  the producer joins local+consumer completion and signals the client. Tri-party shared syncobj
  rejected (cannot express the AND; whoever signals first releases too early); consumer-retains-N
  retained only as flow-control policy (R10), never as synchronization.
- **Popups: forward the full tree; consumer sends bounds, producer runs the positioner (R3/R15).**
  Flattening rejected (breaks input/dismissal); consumer-run positioners rejected (moves shell
  authority). This is the KWin VR popup-bounds seam made cross-process.
- **Input: a per-export protocol channel; libei is transport, not contract (R16–R18).** A global
  EIS seat cannot bind events to tree nodes, own grabs/focus semantics, or carry activation
  intent; activation crosses as gesture-derived intent and the producer mints its own tokens.
  DnD capability-gated off in v1 (R19).
- **Pacing: consumer-driven with atomic mode ownership (R11–R14).** The producer dispatches
  client frame callbacks against the consumer's forwarded `xrWaitFrame`-derived clock; honest
  presented/discarded feedback; late frames reuse last-ready.

### 3. Staged implementation milestones (recorded, not scheduled)

- **M-A — zxr consumer + smithay reference producer.** After the zxr 2D tier (composition M1)
  exists. Smithay chosen first: cheapest producer, exercises the Rust side we lean toward, and
  COSMIC's zcosmic toplevel-capture handlers are the closest existing code
  ([32 §6.3](../../research/32-toplevel-export-prior-art.md)).
- **M-B — the KWin producer MR series.** The political demonstration: a seam-shaped patch series
  addressed directly to the §Context positions, showing a live Plasma session's windows floating
  in Mura. *Concretized (2026-09-23, producer-spec workstream):* the plan is now
  [producers/kwin.md](../producers/kwin.md) — nine MRs, sequenced so every core seam lands with
  its consumer (two standalone, then the plugin series incl. resubmissions of fork commits the
  author triaged as clean-interface material, plus the **delegated-window-state MR** the
  red-team surfaced as the hardest piece), with review-corrected code-verified sizing from
  [research/40 §1](../../research/40-toplevel-export-producers.md): **core ≈ 450–800 LOC +
  ~500 LOC standalone ext-foreign-toplevel-list; plugin ≈ 4.2–6.2 kLOC** (doc 32's 5–8 k
  envelope held; the release join costs nothing because
  `bufferReleasePoint()`/`SyncObjReleasePoint::addReleaseFence` implement it already, while the
  parked-window state adds the honest core cost the original estimate missed). Behavioral bar:
  [specs/toplevel-export-producer.md](../../../specs/toplevel-export-producer.md) §8. Engage Vlad
  Zahorodnii and David Edmundson with the working consumer in hand; Stanislav Aleksandrov is the
  natural third ally (his fork proves demand and he has asked for exactly such core seams).
- **M-C — wayland-protocols proposal** (`xx_toplevel_export`): after M-A+M-B satisfy the
  two-implementations bar, with the published interop tests (modifiers, out-of-order release,
  disconnect, popup reconstraint, late reuse, grabs) from `protocols/README.md`.

### 4. GNOME verdict

Mutter participation is expected **only post-standardization** ([32 §6.2](../../research/32-toplevel-export-prior-art.md):
window screencast is capture-path; portals-only posture; producer needs Mutter C changes). Until
then, GNOME sessions integrate as nested-session quads or portal capture (degraded), and GNOME
*apps* run natively on zxr anyway. The proposal at M-C should invite early Mutter review
regardless (GTK/Mutter is a wayland-protocols member).

*Superseded in detail (2026-09-23) by [producers/mutter.md](../producers/mutter.md)*: the code
study ([research/40 §2](../../research/40-toplevel-export-producers.md)) found Mutter technically
*better*-seamed than assumed (~5–9 kLOC, ~1 k core-touching; the use_count/release_points model
is the release join; `meta_window_drag_end` is already side-effect-free), while confirming the
institutional sequencing: third producer, post-standardization, with a named sponsor, privilege
framed through `MetaWaylandFilterManager` + a ServiceChannel-style trusted connection, and the
GNOME-relevant wedge being g-r-d per-window remote desktop rather than XR.

### 5. Scope boundaries

This seam is not capture (sharing modes 1–3/5 unchanged; portal/consent machinery does not govern
delegation — producer binding policy does); not remote transport (waypipe/mode 4 remains the
per-app remote path); and complementary to wolf-style per-app headless sessions (the right tool
when there is no foreign session to delegate from).

## Rationale

- **Specify-early costs little and anchors everything**: the XML forces the design decisions
  (release semantics, popup split, input contract) to be made where they're cheap, and gives the
  future KWin conversation a concrete artifact instead of a position statement.
- **Consumer-first sequencing follows the dependency graph**: the consumer needs a compositor to
  live in ([desktop-environment.md §6](../desktop-environment.md)); producers are patches to
  codebases that already exist.
- **The COSMIC playbook is the proven route** for a small team landing an ext protocol
  (cosmic-workspace → ext-workspace-v1, multi-year, implementations-first), and we sit in the
  same smithay ecosystem.
- **Named allies materially change the odds**: this is the rare protocol proposal whose target
  producer's maintainers have already described wanting its function, on the record, with dates.

## Alternatives considered

- **Capture-based embedding** (ext-image-copy-capture toplevel sources as the window feed):
  rejected — damage-driven copies, no pacing contract, no input, no tree; doc 32 §1 shows this
  is structural, not a maturity gap.
- **libei-only input** (no protocol channel): rejected — per-stream mapping solves coordinates
  but not node binding, grabs, focus, activation intent ([32 §5.3](../../research/32-toplevel-export-prior-art.md)).
- **Tri-party shared client syncobj** for release: rejected (§2 above).
- **zxr-private protocol forever**: rejected — forfeits the KWin producer and the upstream
  trajectory that is this seam's whole point; the XR-agnostic design costs nothing.
- **Waypipe-only / wolf-only posture** (never delegate sessions): rejected as the *only* answer —
  both are kept for their niches, but neither gives an existing local desktop session
  per-toplevel presence.
- **Wait for KDE to build it**: rejected — David Edmundson's "want to pursue" is three years shy
  of a spec; the party with the consumer has the motive to write it.

## Consequences

- `protocols/` now carries the draft XML + governance README; repo-structure.md lists the tree.
- [component-registry.md](../component-registry.md) gains the consumer row (authority plane) and
  the reference-producer row (build/deliverable); the dependency graph gains the delegated-session
  edge.
- [ADR 0012](0012-de-modularity-spinout-seams.md) §4's zxr-adjacent protocol surface gains this
  seam as its sixth member (with upstream intent, unlike the five spatial ones).
- [ADR 0013](0013-kwin-vr-disposition.md)'s "revisit if KDE lands Vlad's shape" condition is
  superseded: we are specifying that shape; the revisit trigger becomes M-B's outcome.
- No implementation work is scheduled by this ADR; M-A is gated on composition M1 and enters
  planning only when the 2D tier exists.
