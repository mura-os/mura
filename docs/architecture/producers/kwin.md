# producers/kwin: the KDE producer brief for zspatial-toplevel-export-v1

**Status:** brief (producer-specification workstream). Behavioral contract:
[specs/toplevel-export-producer.md](../../../specs/toplevel-export-producer.md); wire contract:
[`protocols/zspatial-toplevel-export-v1.xml`](../../../protocols/zspatial-toplevel-export-v1.xml); full
file:line evidence and the R1–R24 matrix:
[research/40 §1](../../research/40-toplevel-export-producers.md). This brief *is* ADR 0014
milestone M-B made concrete — the "seam-shaped patch addressed directly to the §Context
positions". Code citations are into `references/kwin` @ d84a316 (master) and `references/kwin-vr`
@ ccdd46e (the [MR !8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671) fork).
**Budget impact** (inv. 9): none on spatial-os device budgets (foreign codebase); zxr's
consumer-side cost is bounded in the conformance spec's statement.

## 1. The two sentences this brief answers

> **David Edmundson**: "We can forward windows buffers pretty cheaply and a path to do
> application level input forwarding is definitely something we want to pursue and can hook up."

> **Vlad Zahorodnii**: "For 3D, we'd rather integrate with something that would take care most of
> things for us, e.g. we could provide info about windows, thumbnails, perhaps a more convenient
> way to deal with input, and let them compose overlays, etc. That would prevent kwin from taking
> on significant weight."

The study verified both are *already mostly true in master*: "forward windows buffers pretty
cheaply" is `GraphicsBufferRef` + `SurfaceInterface::bufferReleasePoint()` +
`SyncObjReleasePoint::addReleaseFence` — the producer-owned release join exists as library code
(research/40 §1.1), and Edmundson has since added keysym/text EIS injection himself (2026,
`eisdevice.cpp`). What is missing is small, enumerable, and mostly consists of seams the fork's
author already triaged as "can be covered by proper interfaces."

## 2. The structural argument (the anti-16 kLOC case)

The VR fork's two patches its author called genuinely hard — forking the interactive-move state
machine (`if (isVr)`, 97 LOC) and blocking window↔output reassignment during moves (41 LOC, four
admitted residual bugs) — are **dissolved, not shrunk**, by delegation: a delegated toplevel
*ends* its 2D interactive move at the handoff boundary (one ~20–40 LOC epilogue-skip finish
variant) and its 2D geometry then simply stops changing, so no output-reassignment suppression is
ever needed. The window never lives inside the 2D machinery while rendered elsewhere — which was
the source of both patches and of Vlad's bitrot objection. Verified totals: **core ≈ 250–450 LOC
across six small patches** (+ ~500 LOC standalone ext-foreign-toplevel-list), **plugin ≈ 3.5–5.5
kLOC** — versus the fork's 16 kLOC single MR, with less core surface than even research/32
estimated because the release-join and transaction machinery are fully reusable.

## 3. The MR series (ten small, each independently justifiable)

MRs 1–5 are core seams with **zero behavior change when unused**; three are near-verbatim
resubmissions of fork commits. MRs 6–10 are plugin-only, iterating behind the experimental
namespace like the screencast plugin does. Sizes exclude tests (≈ +60–100 %).

| # | MR | Contents | Size | Covers | Answers |
|---|---|---|---|---|---|
| 1 | `wayland: implement ext-foreign-toplevel-list-v1` | Standard staging protocol on `Window::internalId()`; enumeration only, no export authority | ~500 | R1 | Vlad's "provide info about windows" — standalone value (taskbars, tools) regardless of export |
| 2 | `xdgshell: customizable popup placement bounds` | Fork c877221 resubmitted alone: settable `Workspace::PopupBoundsResolver`, default = the existing `clientArea` expression (`xdgshellwindow.cpp:1891-1898`) | ~30 | R15 core | The author's own "can be covered by proper interfaces" triage, accepted shape |
| 3 | `input: customizable hovered-window resolution + pointer position limiting` | Fork 07306c0 + d65d60a shapes: two settable callbacks, old behavior as defaults (`input.cpp:3697-3708`, `pointer_input.cpp:842,883`) | ~70 | R16/R17 core | David's input-forwarding path, as seams not modes |
| 4 | `window: offscreen frame-callback fixes + explicit pacing-owner seam` | Fork 39a0dc5 bug fixes (off-output windows, `window.cpp:4432-4441`) + an installable per-window pacing owner that suppresses paint/timer dispatch | ~120–200 | R11/R12 core | Fixes real bugs biting screencast today; no export mention needed to justify |
| 5 | `window: finish interactive move without placement side-effects` | Third finish mode skipping the `window.cpp:1078-1097` epilogue; anchor accessors (mostly public: `interactiveMoveOffset()`) | ~40 | R23 core | Encapsulated one-shot transition; also useful to xdg-toplevel-drag edge cases |
| 6 | `plugins/toplevel-export: manager, authorization, tree + shm export` | New plugin owning its global (screencast precedent, `screencastmanager.cpp:33`); dedicated-connection or bind-filter auth; tree serialization off `SurfaceInterface::committed`; deny/redact; revocation | ~1.8–2.5 k | R2 R3 R4 R6 R20 R21 | Vlad's encapsulation: all policy in one plugin |
| 7 | `toplevel-export: zero-copy dmabuf + explicit-sync relay` | Plane-fd dup, buffer/attach events, `GraphicsBufferRef` + release-point holding, `release` → `addReleaseFence`, in-flight caps, revoke-on-stall | ~0.8–1.2 k | R5 R7 R8 R9 R10 R22 | **David's sentence made concrete, plugin-only** |
| 8 | `toplevel-export: consumer pacing endpoint` | Clock correlation, frame deadlines → the MR 4 owner, presented/discarded via held `PresentationFeedback` | ~0.5–0.8 k | R11–R14 | The genuinely novel semantic, isolated for review on its own merits |
| 9 | `toplevel-export: input endpoint` | Plugin `InputDevice` (EIS template, `eisdevice.cpp`), node→global mapping, MR 3 resolvers, focus/dismissal events, activation via the pluggable token creator | ~0.8–1.2 k | R16–R19 | David's "application level input forwarding"; Vlad's "more convenient way to deal with input" |
| 10 | `toplevel-export: detach_drag and adopt` | Move-grab lookup, anchor event, MR 5 finish; adopt = the xdg-toplevel-drag filter recipe (`input.cpp:2853-2882`): warp + move + `performMousePressCommand(MouseMove)` | ~0.3–0.5 k | R23 R24 | Shared code path with an existing feature, no new WM policy |

## 4. Privilege model

Two in-tree options, strongest first: (a) the **dedicated-connection pattern** —
`Display::createClient(fd)` over a socketpair with pointer-identity rights, exactly how Xwayland,
the input method, and the screen locker are privileged (`wayland_server.cpp:195-208,653-657`);
the consumer receives a pre-authorized fd via D-Bus/portal handshake. (b) `restrictedInterfaces`
+ `allowInterface` (`wayland_server.cpp:123-165`) with a connection-identity check. Either way
the global itself lives in the plugin and can reject binds — no core change strictly required.
Both satisfy conformance §2.1 verbatim.

## 5. Engagement plan (M-B execution)

- **Sequence**: land MRs 1–5 first — small, no-behavior-change, three of them resurrecting fork
  commits their author already argued for. Then the plugin MRs 6–10 arrive with the zxr consumer
  *demonstrable*: a live Plasma session's windows floating in spatial-os, each of the conformance
  §8 tests green against the smithay reference producer first (M-A).
- **Counterparts**: Vlad Zahorodnii (lead; encapsulation/bitrot/output-model concerns — answered
  by §2 and the series shape), David Edmundson (buffer forwarding + input forwarding on record;
  now personally invested in EIS injection), Stanislav Aleksandrov (the fork proves demand; MRs
  2–4 resurrect his triaged seams with him as natural co-author/reviewer — "I am ready to make
  new MRs with these changes" is on the record).
- **Timing signals from master**: popup-following window screencast (2025) and syncobj in
  screencast PipeWire buffers show upstream is already comfortable with the ingredients; the
  five fork seams remaining unmerged shows nobody has carried them since — the series does.
- **What we never ask KWin for**: 3D awareness, output-model changes, or leasing regular outputs
  (the fork's rejected items). The producer is 2D-complete; everything spatial stays in the
  consumer, which is the division Vlad asked for.

## 6. Relationship to spatial-os milestones

Gating unchanged (ADR 0014): M-A (zxr consumer + smithay reference producer, after composition
M1) precedes any KWin MR. This brief exists so M-B starts from a verified plan instead of an
estimate; nothing here schedules work. The conformance test list both sides run is
[specs/toplevel-export-producer.md §8](../../../specs/toplevel-export-producer.md).
