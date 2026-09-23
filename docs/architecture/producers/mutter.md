# producers/mutter: the GNOME producer brief for zext-toplevel-export-v1

**Status:** brief (producer-specification workstream). The behavioral contract is
[specs/toplevel-export-producer.md](../../../specs/toplevel-export-producer.md); the wire contract
[`protocols/zext-toplevel-export-v1.xml`](../../../protocols/zext-toplevel-export-v1.xml); the
full file:line evidence and the R1–R24 matrix are in
[research/40 §Mutter](../../research/40-toplevel-export-producers.md). Strategy context:
[ADR 0014 §4](../adr/0014-toplevel-delegation-protocol.md) (this brief supersedes its one-line
GNOME verdict). Code citations below are into `references/mutter` @ 888a7b7 (Mutter 51.0),
`references/gnome-shell`, `references/gnome-remote-desktop`.
**Budget impact** (inv. 9): none on spatial-os device budgets — this brief plans changes to a
foreign codebase; zxr's consumer-side cost is bounded in the conformance spec's own statement.

## 1. Verdict

**Technically, Mutter is a better host than research/32 §6.2 assumed.** Every load-bearing seam
exists cleanly: client dmabuf plane fds are retained for the buffer's lifetime
(`meta-wayland-dma-buf.c:126-140`), release is a per-buffer use count that signals syncobj
release points at zero (`meta-wayland-buffer.c:651-705` — the producer-owned join of conformance
§3.3 is literally "hold one extra use count, merge the consumer's fence"), commit state is atomic
and observable outside the paint path (`pre-state-applied`/`applied` signals,
`meta-wayland-surface.c:898,1094-1097`), per-window EIS coordinate viewports exist
(`meta-stream-window.c:215-282`), and `meta_window_drag_end` is already the side-effect-free
move-end that detach (§7.1) needs (`meta-window-drag.c:385-424` — tiling side-effects live only
in `end_grab_op`). Total: **~5–9 kLOC, of which only ~0.7–1.5 kLOC touches core** — KWin-sized;
Mutter is not structurally harder.

**Institutionally, plan Mutter as the third producer.** GNOME's privileged surface is private
D-Bus + portals by deliberate policy (`org.gnome.Mutter.ScreenCast` is marked private in its own
XML; no foreign-toplevel-list, no ext capture protocols, no security-context in the tree), and
the NEWS record shows external protocols land only standardized-plus-sponsored: drm-syncobj 46.1
(NVIDIA), xdg-toplevel-drag 47 (Igalia), commit-timing/fifo (Valve). Engagement is at milestone
M-C (the upstream proposal), with the smithay reference producer and the KWin MR already alive.

## 2. Why the existing window stream is not the answer

Mutter *can* export window content today (`RecordWindow`), but the path is copy-capture in
exactly the sense research/32 §1 rejects: `capture_into`/`blit_to_framebuffer` re-render the
flattened actor tree into a consumer allocation (`meta-window-actor.c:1436-1584`), streams are
fixed at logical-monitor size ("windows can be resized, whereas streams cannot",
`meta-stream-window.c:195-202`), BGRA-only (`meta-stream-source-window.c:851-865`), on a
hardcoded 60 Hz private cadence (`:400-402`). These measured limitations are the quantified
producer-side pain the upstream pitch leads with — they are what per-toplevel delegation fixes
*for GNOME's own remote desktop*, independent of XR.

## 3. The patch plan

One self-contained protocol module plus five contained core seams (full matrix in research/40):

- **Module** (`meta-wayland-toplevel-export.c` + an ext-foreign-toplevel-list module,
  ~3.5–6 kLOC): export/tree/node objects, buffer+fence relay, pacing source (mirroring the
  per-view timerfd `FrameCallbackSource`, `meta-wayland.c:222-346`), feedback ledger, policy.
  Comparable to several existing single-protocol modules combined (syncobj 645 LOC,
  toplevel-drag 478, transaction 772).
- **Core seams** (the review-expensive ~1 kLOC): (1) input forced-target routing — coordinates
  per window exist, but delivery is global virtual-device + stage pick
  (`meta-eis-client.c:467-506`), so node-addressed delivery into
  `MetaWaylandSeat`/pointer/keyboard/touch is the largest genuinely new piece; (2)
  frame-callback ownership switch, shaped like the existing `is_streaming` view-primacy
  exemption (`meta-surface-actor-wayland.c:90-153`), plus fifo/commit-timing retargeting; (3)
  popup work-area override — the constraint rectangle is `work_area_monitor`
  (`constraints.c:485-487,856-864`); substituting consumer bounds is ~100–200 LOC, the same seam
  KWin VR patched; (4) release-join extension in `handle_release_points` (today it merges only
  cogl's latest sync fd); (5) detach/adopt entry points over the existing drag API
  (`meta_compositor_get_current_window_drag`, public `meta_window_begin_grab_op`, pointer warp —
  all present; `xdg-toplevel-drag` in-tree is the semantic precedent).

Upstream-sized MR series, each independently defensible: (a) ext-foreign-toplevel-list support
(~600 LOC, independently valuable, the easiest landing); (b) buffer release-join + dmabuf plane
accessors (small, syncobj-adjacent); (c) popup bounds override hook; (d) frame-callback pacing
hook; then (e) the export module and (f) input targeting once the protocol has upstream standing.

## 4. Privilege framing (decisive for GNOME)

R2 must be *presented* in Mutter's own model, and it maps exactly: a new capability bit checked
by a `MetaWaylandFilterManager` per-global filter (`meta-wayland-filter-manager.c:34-101`),
granted over a `ServiceChannel`-style trusted connection (pidfd-verified, dedicated Wayland fd,
caps on the client — `meta-service-channel.c:115-209`; the x11-interop cap is the worked
example). ~150 LOC. The consumer is framed as **a portal-grade system service, like
gnome-remote-desktop** — never "any client with a new global". The conformance spec's §2.1
wording ("the producer's privileged-global mechanism") was written so this satisfies it verbatim.

## 5. Shell (JS) involvement

None in the data path. gnome-shell participates only at the policy layer it already owns:
session-lock revocation should register exports as inhibitable alongside remote-access sessions
(the shell's lock path already terminates those via `MetaRemoteAccessController`,
`gnome-shell/js/ui/main.js:138-147` → `meta-remote-access-controller.c:146-184`), which
implements conformance §6's session-lock row through GNOME's native chain rather than
ext-session-lock (which Mutter deliberately does not implement). No extension-based
implementation is possible (extensions live inside the shell process against unstable internals;
the buffer-lifetime ABI must be C) — confirming research/32 §6.2.

## 6. Engagement conditions (what makes Mutter maintainers say yes)

1. **Standardization first**: `xx_toplevel_export` at wayland-protocols with the ext bar already
   met — smithay reference producer + KWin MR + the zxr consumer + published interop tests
   (conformance §8). This matches every recent Mutter adoption and the COSMIC-workspace
   timeline.
2. **A GNOME-relevant consumer in the pitch**: per-window remote desktop through g-r-d with
   real (client-paced, zero-copy, resize-correct, non-BGRA-locked) window streams — §2's
   measured limitations, fixed. XR is the second example, not the first.
3. **Invariant-preserving framing**: R7–R9 presented as reuse of Mutter's own
   `use_count`/`release_points`/syncobj semantics — no redesign of buffer lifetime paths.
4. **Input as protocol, not EIS-global**: Mutter's own code is the argument (global virtual
   devices + stage pick cannot address occluded exported windows); the per-node channel needs
   working grab/focus evidence from the other two producers before Mutter review.
5. **A named sponsor**: the adoption record (NVIDIA, Igalia, Valve) says a motivated external
   contributor carries the MR series; spatial-os (or a contracted GNOME shop) plays that role at
   M-C+, with the series of §3's independently-defensible MRs (a)–(d) offered first.
