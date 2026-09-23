# spatial-os architecture: foreign-session integration (toplevel export/delegation)

**Status:** draft design (foreign-session workstream). Specifies how windows owned by *another*
compositor become first-class floating windows in spatial-os, and the new protocol seam that
requires. Evidence base: [research/32](../research/32-toplevel-export-prior-art.md) (prior art +
the R1–R22 requirements distillate), [research/31](../research/31-kwin-vr.md) (the KWin VR fork),
[research/19](../research/19-wayland-proxying.md) (protocol proxying). Strategy and milestones:
[ADR 0014](adr/0014-toplevel-delegation-protocol.md). Protocol draft:
[`protocols/zspatial-toplevel-export-v1.xml`](../../protocols/zspatial-toplevel-export-v1.xml).

## 1. Provenance: the named positions this seam answers

This design implements, from the XR side, the integration shape KDE's own leadership specified in
[KWin MR !8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671) while declining the
in-process KWin VR plugin. For future upstream presentation, verbatim and dated
(full context in [31 §1](../research/31-kwin-vr.md)):

> **Vlad Zahorodnii** (KWin lead maintainer, 2026-03-25): *"KWin is a (2D) stacking window
> manager. I'm not entirely convinced about making it 3D, the window management bits are written
> with 2D in mind. For 3D, we'd rather integrate with something that would take care most of
> things for us, e.g. we could provide info about windows, thumbnails, perhaps a more convenient
> way to deal with input, and let them compose overlays, etc."*

> **David Edmundson** (KDE Plasma developer, 2026-01-19): *"…what is kwin providing that means
> there's a benefit to having it in the kwin process. We can forward windows buffers pretty
> cheaply and a path to do application level input forwarding is definitely something we want to
> pursue."*

> **Stanislav Aleksandrov** (KWin VR author, 2026-04-20), on the fork's prospects: *"it seems
> that way it just naturally grows into a fork on its own"* — the demand-and-feasibility proof
> that a mature-DE VR mode is wanted, daily-usable, and unmergeable as an in-process plugin.

spatial-os ([ADR 0012](adr/0012-de-modularity-spinout-seams.md)) and the KWin maintainers arrived
at the same boundary from opposite directions: the 2D compositor provides windows + input over a
narrow privileged interface; the XR compositor composes. Nobody has specified that interface.
This document and the draft XML do.

*Producer-side specification (2026-09-23, producer-spec workstream):* the interface now has a
behavioral conformance spec —
[specs/toplevel-export-producer.md](../../specs/toplevel-export-producer.md) — and per-compositor
integration briefs answering these positions with verified patch plans:
[producers/kwin.md](producers/kwin.md) (the 9-MR series; core ≈ 450–800 LOC incl. the
delegated-window state) and [producers/mutter.md](producers/mutter.md) (third-producer
sequencing), on the code evidence of
[research/40](../research/40-toplevel-export-producers.md); both red-teamed in
KWin-maintainer/Mutter-maintainer persona and revised.

## 2. The client-integration taxonomy

Four ways a "foreign" application or session appears in spatial-os space. All four land as
surfaces in zxr's world model (T1 — free-floating, no output binding); they differ in *who owns
the app* and *what crosses the boundary*:

| Mode | What crosses | Granularity | Pacing | Input | Status |
|---|---|---|---|---|---|
| **Native client** | Wayland protocol (app ↔ zxr directly) | per toplevel | zxr-driven frame callbacks | full, native | the default; composition doc §7.3 |
| **Protocol-proxied client** (waypipe/wprs; VM via virtio) | Wayland protocol over a transport | per toplevel | zxr-driven (proxy passes callbacks through) | full, native | designed — sharing mode 4 ([spatial-sharing.md §3](spatial-sharing.md)) |
| **Nested foreign compositor as one quad** | one output-sized surface (the nested session's whole display) | per session | nested compositor's own clock | seat-level into the nested session | works today with zero new protocol; the "virtual screen" compat artifact |
| **Per-toplevel delegated session** (this seam) | client buffers + input + pacing, per window, via the producer compositor | per toplevel | **consumer-driven** (paced to zxr's `xrWaitFrame` cadence) | per-export protocol channel | **specified here; not yet implemented** |

The fourth mode is what upgrades "your Plasma session, as a picture on a slab" into "your Plasma
session's windows, floating individually in the room" — while KWin (or any producer) keeps its
window-management authority over its own clients, and zxr keeps display, input-routing, and
composition authority over the space. It is **not capture**: [32 §1](../research/32-toplevel-export-prior-art.md)
establishes why `ext-image-copy-capture` semantics (damage-driven, copy-flavoured, no pacing
contract, no input, no surface tree) cannot be stretched into this role.

Dependency-graph placement ([desktop-environment.md §6](desktop-environment.md)): the delegated
session enters as a consumer under the seam layer — foreign producer → `zspatial-toplevel-export` →
zxr's window model — parallel to, not through, the capture seam.

## 3. The seam, by problem area

Each subsection states the chosen design; normative details are the R-numbers in
[32 §8](../research/32-toplevel-export-prior-art.md), which the XML draft implements.

### 3.1 Identity and privilege (R1, R2, R21)

Exports are created against `ext_foreign_toplevel_handle_v1` identities — never title/app-id
matching. The manager global is connection-filtered (ADR 0012 §5 binding policy on the producer's
side); enumeration grants neither export nor input authority. The producer may deny or redact:
lock surfaces, protected content, its own internal/decoration surfaces.

### 3.2 Buffers: zero-copy with producer-owned release (R4–R10, R22)

The producer duplicates the client's dmabuf plane fds to the consumer with full format/modifier/
device identity; the consumer imports the *same allocation* — no render, no copy. Fallbacks
(shm, modifier/device mismatch, protected, cross-GPU) are explicit typed events, not silent
copies. Synchronization follows `linux-drm-syncobj-v1` semantics: every use carries an acquire
point; the consumer returns a GPU-complete release; **only the producer joins** "local composition
done" ∧ "consumer done" and then signals the client's release — one authority for client-visible
buffer lifetime, misbehaving consumers contained (the tri-party-syncobj and consumer-retains-N
alternatives are rejected as *synchronization*, the latter retained as flow-control policy —
[32 §2.2](../research/32-toplevel-export-prior-art.md)). Release ordering is per-buffer (never one
monotonic timeline). In-flight caps are negotiated; a stalled consumer is revoked, never waited on.

### 3.3 Pacing: consumer-driven presentation (R11–R14)

The inversion that distinguishes this from every capture path: in delegated mode, frame callbacks
for the exported tree are driven by the **consumer's** cadence. zxr forwards its
`xrWaitFrame`-derived clock (predicted display time, period, commit cutoff, clock correlation);
the producer dispatches the client's frame callbacks against it, so a delegated Blender window
paces to the headset, not to a monitor that may not exist. Exactly one pacing mode owns a tree at
a time (producer-clock ↔ consumer-clock switches are atomic). Presentation feedback reports first
XR sampling as `presented`, never-sampled as `discarded`; late frames reuse the last-ready buffer
without re-presenting the old commit (`zero_copy` flag semantics stay honest per R22).

### 3.4 The surface tree: forward everything, split the popup problem (R3, R15)

The full tree crosses: root toplevel, ordered subsurfaces, popups/transients — separate nodes,
atomically committed, never force-flattened (input and dismissal need real nodes). Popup
placement splits exactly on the KWin VR seam, now cross-process: the **consumer owns the
placement volume** (it sends plane-local bounds + change serials), the **producer runs the
xdg-positioner** against those bounds and issues configures — producer keeps shell authority,
consumer keeps spatial authority.

### 3.5 Input: a per-export protocol channel, libei demoted to transport (R16–R19)

Verdict from [32 §5.3](../research/32-toplevel-export-prior-art.md): a global EIS seat cannot be
the contract — it does not bind events to tree nodes, define focus/grab ownership, or carry
activation intent. The seam therefore carries surface-local input per node (pointer/keyboard/
touch with device class, IDs, timestamps, frames); the **producer** generates serials and owns
implicit grabs, popup dismissal, and focus state for its clients; activation crosses as
*gesture-derived intent* (the producer mints and validates its own xdg-activation tokens — a
foreign token is never trusted). Implementations may route these events into libei/KWin
`InputDevice`/Clutter internally. DnD is capability-gated and initially unsupported: the
cross-export data-offer problem is real, and its decider is the delegation M-A implementation
round (ADR 0014), where the consumer's data-device topology becomes concrete.

### 3.6 Lifecycle (R20)

Unmap, session lock (producer side), auth loss, either process dying, or GPU reset revokes the
export: input is cancelled, outstanding buffers are released only after fenced GPU work
completes, and the consumer's scene node despawns. The producer never blocks its event loop on
the consumer.

### 3.7 Mid-drag transitions: session quad ↔ space (R23, R24)

The signature interaction KWin VR proved ([31 §2.9](../research/31-kwin-vr.md) — edge-barrier
detach at a configurable margin, cursor-anchor continuity, re-entry by pick-UV pointer warp),
recast across the process boundary:

- **Drag out.** The user drags a window *inside* a session quad (zxr forwards input over §3.5;
  the producer runs its ordinary 2D interactive move). zxr owns the ray and the quad geometry,
  so zxr detects the barrier condition — move-grab active and the ray beyond the quad edge by a
  margin — and issues the **detach handoff (R23)**: the producer exports that toplevel mid-move
  (ending its own move with no placement side-effects) and the handoff carries the cursor-anchor
  point, so zxr's spatial grab keeps the same content pixel under the ray. From that moment the
  window is an ordinary delegated toplevel floating in space — i.e. a member of a place in the
  frame graph ([places-model.md](places-model.md); delegated members are restore *slots* per its
  §7).
- **Drag back.** zxr's ray hits a session quad while carrying a delegated window: zxr ends the
  delegation with a **landing placement (R24)** — target output plus 2D coordinates derived from
  the pick's UV on the quad, resume-move flag set; the producer warps its pointer there and
  continues its interactive move (KWin's implementation is nearly verbatim what the fork already
  does single-process: `sendClientToScreen` + pointer position). The window is back under the
  producer's WM authority.
- **The asymmetry to design for**: only *delegated* toplevels round-trip. A zxr-native client
  dragged onto a session quad cannot enter that session — no protocol can transplant a live
  Wayland connection between compositors. Native windows dropped on a quad are placed in front
  of it (with a visual cue), never into it. Single-compositor implementations (KWin VR) don't
  have this asymmetry; ours is structural and the UX must communicate it.

`xdg-toplevel-drag-v1` (staging) is the semantic precedent for both halves *within* each
compositor — attach-toplevel-to-drag, dock/undock via drop targets, "final position as if move
ended" — and producers that already implement it have most of the machinery R23/R24 ask for.

## 4. What this seam is not

- **Not capture/sharing.** Modes 1–3 and 5 of [spatial-sharing.md](spatial-sharing.md) are
  unchanged; the portal/consent machinery does not govern delegation (the producer's binding
  policy does). A delegated window can *additionally* be captured through the normal capture
  seam like any surface.
- **Not remote transport.** Same-machine, same-or-compatible-GPU by design; remote/VM apps keep
  using protocol proxying (mode 4), which already achieves per-toplevel granularity by forwarding
  the protocol itself.
- **Not a replacement for wolf-style per-app sessions** ([18 §5](../research/18-xr-streaming.md)):
  spawning each app under its own headless compositor remains the right tool when no shared
  foreign session exists to delegate *from*.

## 5. Deliverables and sequencing

The protocol draft lives at [`protocols/zspatial-toplevel-export-v1.xml`](../../protocols/zspatial-toplevel-export-v1.xml)
(experimental `zspatial` namespace; renamed `xx_`/`ext_` on upstream proposal per
[`protocols/README.md`](../../protocols/README.md)). Implementation order, decided in
[ADR 0014](adr/0014-toplevel-delegation-protocol.md): zxr consumer (after the 2D tier exists) →
smithay reference producer → the KWin producer MR (the demonstration addressed to the §1
positions) → wayland-protocols proposal with interop tests. GNOME/Mutter is expected to
participate only post-standardization; until then a Plasma-session-as-quad or portal-capture
fallback covers GNOME sessions.
