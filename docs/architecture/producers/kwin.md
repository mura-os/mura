# producers/kwin: the KDE producer brief for zspatial-toplevel-export-v1

**Status:** brief rev 2 (producer-specification workstream; KWin-persona red-team absorbed —
series resequenced so seams land with their consumers, the delegated-window-state MR added as
the acknowledged hardest piece, the "dissolved patches" claim re-argued honestly). Behavioral
contract: [specs/toplevel-export-producer.md](../../../specs/toplevel-export-producer.md); wire
contract: [`protocols/zspatial-toplevel-export-v1.xml`](../../../protocols/zspatial-toplevel-export-v1.xml)
(rev 3); full file:line evidence and the R1–R24 matrix:
[research/40 §1](../../research/40-toplevel-export-producers.md). This brief *is* ADR 0014
milestone M-B made concrete. Code citations are into `references/kwin` @ d84a316 (master) and
`references/kwin-vr` @ ccdd46e (the
[MR !8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671) fork).
**Budget impact** (inv. 9): none on Mura device budgets (foreign codebase); zxr's
consumer-side cost is bounded in the conformance spec's statement.

## 1. The two sentences this brief answers

> **David Edmundson**: "We can forward windows buffers pretty cheaply and a path to do
> application level input forwarding is definitely something we want to pursue and can hook up."

> **Vlad Zahorodnii**: "For 3D, we'd rather integrate with something that would take care most of
> things for us, e.g. we could provide info about windows, thumbnails, perhaps a more convenient
> way to deal with input, and let them compose overlays, etc. That would prevent kwin from taking
> on significant weight."

The code study verified the first sentence is *already mostly true in master*: "forward windows
buffers pretty cheaply" is `GraphicsBufferRef` + `SurfaceInterface::bufferReleasePoint()` +
`SyncObjReleasePoint::addReleaseFence` — the producer-owned release join exists as library code
(research/40 §1.1) — and Edmundson has since added keysym/text EIS injection himself (2026,
`eisdevice.cpp`). What is missing is enumerable; one piece of it (§2) is genuinely hard and this
brief names it rather than hiding it.

## 2. The structural argument, stated honestly

The VR fork's two patches its author called unmergeable split under delegation:

- **The interactive-move fork genuinely dissolves.** The fork forked the move state machine
  (`if (isVr)`, 97 LOC) because VR windows keep *living* in the 2D move machinery while rendered
  in 3D. Delegation *ends* the move at the handoff boundary — a ~20–40 LOC finish variant that
  skips the placement epilogue (`window.cpp:1078-1097`), invoked once. Verified clean.
- **The window-mode problem is relocated, not dissolved.** A delegated window still exists in
  KWin: without further work it would render at its parked rectangle, take local clicks
  (`findToplevel` finds it), appear in alt-tab/taskbars at a stale position, and be moved/resized
  by output hotplug and `checkWorkspacePosition` — the fork's output-reassignment patch
  (0448fdd) suppressed exactly that *during moves*; delegation needs it *for the delegation
  lifetime*. The conformance spec now makes this a normative producer obligation
  ([§2.8, the parked-window model](../../../specs/toplevel-export-producer.md)): not presented
  locally, no local input, topology-frozen geometry, excluded from session state, switcher
  representation as policy. In KWin terms that is **one new window state, implemented once, in
  core** (MR 4 below) — the honest counterpart of the fork's scattered `isVr` checks, and the
  hardest MR in the series. The difference from the fork is that the state is *static and
  narrow*: a delegated window has no live interaction between 2D machinery and 3D rendering (no
  move fork, no per-frame policy), just an "absent with a recorded placement" mode — closest
  existing analog: how KWin already treats minimized windows (invisible, listed, restorable),
  plus geometry freezing.

Verified totals with the delegated-state MR included: **core ≈ 450–800 LOC across seven
patches**, plugin ≈ **4.2–6.2 kLOC** (MRs 5–9 summed) plus the ~500 LOC standalone
ext-foreign-toplevel-list — still well under the fork's 16 kLOC single MR, and with the honest
claim: *the only behavior changes outside the plugin are bug fixes and the delegated state
itself*.

## 3. The MR series (nine, sequenced so every seam lands with its consumer)

The red-team sequencing correction is adopted: **extension points do not land before their
users.** MRs 1–2 are standalone and self-justifying; MRs 3–9 are submitted as a series once the
zxr consumer is demonstrable (M-A complete), each core seam in the same MR as (or adjacent to)
the plugin code that consumes it. Sizes exclude tests (≈ +60–100 %).

| # | MR | Contents | Size | Covers | Notes |
|---|---|---|---|---|---|
| 1 | `wayland: implement ext-foreign-toplevel-list-v1` | Standard staging protocol on `Window::internalId()`; enumeration only | ~500 | R1 | **The duplication question, answered head-on**: KWin ships `org_kde_plasma_window_management` (restricted, richer: state + actions), and no Plasma taskbar migrates to the info-only ext protocol. The pitch is *not* "taskbars": it is the neutral, ecosystem-standard identity anchor (wlroots + COSMIC ship it; Mutter lacks it too) that privileged protocols key off — the same shape zcosmic uses. Standalone value is third-party tools; primary value is being the export key. No stated KDE position exists in the record; the MR asks the question directly. |
| 2 | `window: offscreen frame-callback fixes` | The fork's 39a0dc5 bug fixes alone (windows outside all outputs; `output()` divergence, `window.cpp:4432-4441`) | ~60 | — | Behavior change by design — it fixes bugs biting screencast/thumbnail users today. Split from any seam so it is reviewable as what it is. |
| 3 | `plugins/toplevel-export: manager, authorization, tree export, shm-fallback signaling` | New plugin owning its global (screencast precedent, `screencastmanager.cpp:33`); dedicated-connection or bind-filter auth (§5); tree serialization off `SurfaceInterface::committed`; typed fallback *signaling* (`fallback(shm_only)` etc. — the protocol has no copy path); deny/redact; revocation matrix | ~1.8–2.5 k plugin | R2 R3 R4 R6 R20 R21 | Vlad's encapsulation: all policy in one plugin. |
| 4 | `workspace: delegated window state` | The §2 core piece, paired with MR 3: a delegated visibility/interaction state — not rendered (visibility reason), excluded from `findToplevel` hit-testing, topology-frozen geometry (no `sendToOutput`/`checkWorkspacePosition` effects while delegated), session-restore records pre-delegation placement, switcher representation policy (minimized-window analog) | ~200–400 core | spec §2.8 | **The hardest MR; named as such.** It is one state implemented once — the anti-`isVr` shape — and it is where the review argument about bitrot is either won or lost. |
| 5 | `toplevel-export: zero-copy dmabuf + explicit-sync relay` | Timeline import, plane-fd dup, buffer/attach events, `GraphicsBufferRef` + release-point holding, `release` → `addReleaseFence`, `flow_control` + revoke-on-overrun | ~0.8–1.2 k plugin | R5 R7 R8 R9 R10 R22 | **David's sentence made concrete, plugin-only** — the strongest MR in the series; every underlying claim code-verified. |
| 6 | `toplevel-export: consumer pacing` | Core half (~70–150): explicit per-window pacing owner (suppresses paint dispatch + offscreen timer when installed; keeps the item unsuspended; defined interaction when the window is also 2D-visible — resolved by MR 4: it never is) + the ~10-LOC `presentationFeedback()` accessor widening (`surface.cpp:509-515`). Plugin half (~0.5–0.8 k): clock correlation, frame deadlines, `mark_presented`/`mark_discarded` → held `PresentationFeedback` | ~0.6–1.0 k | R11–R14 | The genuinely novel semantic, isolated; the sixth core patch (feedback accessor) lives here, with its consumer. |
| 7 | `toplevel-export: input` | Core half (~70): the fork's hovered-window-resolver + position-limiter seams (07306c0/d65d60a shapes, defaults preserved). Plugin half (~0.8–1.2 k): plugin `InputDevice` (EIS template, `eisdevice.cpp`), node→global mapping, keymap serials + keysym channel (reusing the `sendKeySym` machinery), enter/leave/cancel semantics, focus/dismissal events, activation via the pluggable token creator | ~0.9–1.3 k | R16–R19 | David's "application level input forwarding"; Vlad's "more convenient way to deal with input". Seams land with their user. |
| 8 | `toplevel-export: popup bounds` | Core half (~30): the fork's c877221 settable `Workspace::PopupBoundsResolver`, default = the existing `clientArea` expression (`xdgshellwindow.cpp:1891-1898`). Plugin half (~0.1–0.15 k): `set_bounds` plumbing + `configure_bounds` serial echo | ~0.15 k | R15 | The author's own "can be covered by proper interfaces" triage — resubmitted *with* the interface's consumer, answering why it sat unmerged (a seam without a user). |
| 9 | `toplevel-export: detach_drag and adopt` | Core half (~20–40): the third finish mode skipping the `window.cpp:1078-1097` epilogue; anchor accessors (mostly public: `interactiveMoveOffset()`). Plugin half (~0.3–0.5 k): move-grab lookup, anchor event; adopt = pointer warp (`pointer_input.cpp:921-926`) + `move` + `performMousePressCommand(Options::MouseMove)` — the xdg-toplevel-drag filter's components (`input.cpp:2853-2882,2629-2639`), with the resumed move ending on the next producer button-release per the spec's §7.2 contract | ~0.4–0.6 k | R23 R24 | Shared code path with an existing feature; the one new policy (resume-move driving contract) is now specified, not implied. |

Also required, outside KWin: the consumer-authorization handshake (how the pre-authorized fd
reaches the consumer — a small KDE D-Bus service or portal interface) is **new public KDE API**
that needs its own design and review; it is called out here so nobody discovers it inside MR 3.

## 4. Privilege model

Two in-tree options, strongest first: (a) the **dedicated-connection pattern** —
`Display::createClient(fd)` over a socketpair with pointer-identity rights, exactly how Xwayland,
the input method, and the screen locker are privileged (`wayland_server.cpp:195-208,653-657`);
the consumer receives a pre-authorized fd via the §3 handshake service. (b)
`restrictedInterfaces` + `allowInterface` (`wayland_server.cpp:123-165`) with a
connection-identity check. Either way the global itself lives in the plugin and can reject binds.
Both satisfy conformance §2.1 verbatim.

## 5. Engagement plan (M-B execution)

- **Sequence**: MR 1 and MR 2 first — standalone, self-justifying. The series MRs 3–9 arrive
  *together with the working consumer*: a live Plasma session's windows floating in Mura,
  the conformance §8 tests green against the smithay reference producer (M-A) first. No seam is
  ever in-tree without its user — the fork's five seams sat unmerged precisely because they were
  argued as interfaces without a consumer; the series does not repeat that.
- **Counterparts**: Vlad Zahorodnii (lead; encapsulation/bitrot/output-model concerns — answered
  by §2's honest split and MR 4's one-state shape), David Edmundson (buffer forwarding + input
  forwarding on record; now personally invested in EIS injection), Stanislav Aleksandrov (the
  fork proves demand; MRs 7–9 resurrect his triaged seams with him as natural co-author —
  "I am ready to make new MRs with these changes" is on the record).
- **Timing signals from master**: popup-following window screencast (2025) and syncobj in
  screencast PipeWire buffers show upstream is already comfortable with the ingredients.
- **What we never ask KWin for**: 3D awareness, output-model changes, or leasing regular outputs
  (the fork's rejected items). The producer stays 2D-complete; everything spatial lives in the
  consumer — with the one honest core cost (MR 4's delegated state) named up front instead of
  discovered in review.

## 6. Relationship to Mura milestones

Gating unchanged (ADR 0014): M-A (zxr consumer + smithay reference producer, after composition
M1) precedes any KWin MR beyond 1–2. This brief exists so M-B starts from a verified plan
instead of an estimate; nothing here schedules work. The conformance test list both sides run is
[specs/toplevel-export-producer.md §8](../../../specs/toplevel-export-producer.md).
