# producers/mutter: the GNOME producer brief for zext-toplevel-export-v1

**Status:** brief rev 2 (producer-specification workstream; Mutter-persona red-team findings
absorbed — privilege story made an explicit decision, g-r-d wedge replaced, release-join and
pacing costs named honestly). The behavioral contract is
[specs/toplevel-export-producer.md](../../../specs/toplevel-export-producer.md); the wire contract
[`protocols/zext-toplevel-export-v1.xml`](../../../protocols/zext-toplevel-export-v1.xml); the
full file:line evidence and the R1–R24 matrix are in
[research/40 §2](../../research/40-toplevel-export-producers.md). Strategy context:
[ADR 0014 §4](../adr/0014-toplevel-delegation-protocol.md) (this brief supersedes its one-line
GNOME verdict). Code citations are into `references/mutter` @ 888a7b7 (Mutter 51.0),
`references/gnome-shell`, `references/gnome-remote-desktop`.
**Budget impact** (inv. 9): none on spatial-os device budgets — this brief plans changes to a
foreign codebase; zxr's consumer-side cost is bounded in the conformance spec's own statement.

## 1. Verdict

**Technically, Mutter is a better host than research/32 §6.2 assumed.** Every load-bearing seam
exists: client dmabuf plane fds are retained for the buffer's lifetime
(`meta-wayland-dma-buf.c:126-140`), release is a per-buffer use count that signals accumulated
syncobj release points at zero (`meta-wayland-buffer.c:651-705`), commit state is atomic and
observable outside the paint path (`pre-state-applied`/`applied` signals,
`meta-wayland-surface.c:898,1094-1097`), per-window EIS coordinate viewports exist
(`meta-stream-window.c:215-282`), and the drag machinery cleanly separates end-of-move placement
side-effects (`end_grab_op`, `meta-window-drag.c:1795-1862`) from grab teardown
(`meta_window_drag_end`, `:385-424`). Total: **~5–9 kLOC, of which ~0.7–1.5 kLOC touches core** —
KWin-sized; Mutter is not structurally harder.

Two honest costs the seams do not hide (red-team findings 4/10):

- **The release join is arithmetically safe but not free.** Holding one extra
  `inc_use_count` per observed attach is correct even for the same buffer attached in successive
  commits (commit-time inc precedes apply-time dec of the prior hold,
  `meta-wayland-surface.c:1179-1182,922-923`). But Mutter has **no sync-file merge helper**:
  `handle_release_points` imports exactly one fd (cogl's latest sync fd) into every point, and
  silently early-returns when that fd is unavailable. The implementation choice is (i)
  CPU-observe the consumer's release point (poll/eventfd) before `dec_use_count` — zero core
  change, one scheduler round-trip added per release — or (ii) add fence-merge machinery to
  `handle_release_points` — new code in the most invariant-laden file in the tree. The MR series
  must pick and cost one; this brief recommends (i) for the first implementation.
- **`meta_window_drag_end` is "side-effect-free" only after an audit.** It conditionally raises
  the window (raise-on-release mode) and emits `grab-op-end`, which gnome-shell JS consumes —
  detach is a new entry point *plus* handling of those two observable effects, and the function
  is compositor-internal, not contract-stable API.

**Institutionally, plan Mutter as the third producer.** GNOME's privileged surface is private
D-Bus + portals by deliberate policy (`org.gnome.Mutter.ScreenCast` is marked private in its own
XML; no foreign-toplevel-list, no ext capture protocols, no wp_security_context in tree), and the
NEWS record shows externally-carried protocols land standardized-plus-sponsored: drm-syncobj in
46.1 ([!3300](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/3300), driven by NVIDIA's
explicit-sync need), xdg-toplevel-drag in **48.0**
([!4107](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/4107), carried by Igalia for
Chromium tab-dragging), commit-timing/fifo in 48.0
([!3355](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/3355), Valve-adjacent latency
work). (Sponsor attributions are external knowledge of those MRs; NEWS records versions and MR
numbers only.) Engagement is at milestone M-C, with the smithay reference producer and the KWin
MR series already alive.

## 2. Why the existing window stream is not the answer

Mutter *can* export window content today (`RecordWindow`), but the path is copy-capture in
exactly the sense research/32 §1 rejects: `capture_into`/`blit_to_framebuffer` re-render the
flattened actor tree into a consumer allocation (`meta-window-actor.c:1436-1584`), streams are
fixed at logical-monitor size ("windows can be resized, whereas streams cannot",
`meta-stream-window.c:195-202`), BGRA-only (`meta-stream-source-window.c:851-865`), on a
hardcoded 60 Hz private cadence (`:400-402`). These measured limitations are real — but see §6
for who actually feels them (it is not gnome-remote-desktop).

## 3. The patch plan

One self-contained protocol module plus **six** contained core seams (full matrix in
research/40; seam 6 added by the red-team review):

- **Module** (`meta-wayland-toplevel-export.c` + an ext-foreign-toplevel-list module,
  ~3.5–6 kLOC): export/tree/node objects, buffer+fence relay, pacing source, feedback ledger,
  policy. Comparable to several existing single-protocol modules combined (syncobj 645 LOC,
  toplevel-drag 478, transaction 772).
- **Core seams** (~1–1.5 kLOC, the review-expensive part):
  1. **Input forced-target routing** — coordinates per window exist, but delivery is global
     virtual-device + stage pick (`meta-eis-client.c:467-506`); node-addressed delivery into
     `MetaWaylandSeat`/pointer/keyboard/touch is the largest genuinely new piece.
  2. **Frame-callback ownership switch** — the correct template is the per-view timerfd
     `FrameCallbackSource` (`meta-wayland.c:222-346,440-482`) rekeyed to a consumer clock, *not*
     the `is_streaming` exemption (that keeps dispatch on a local stage view's clock; it only
     proves obscured windows can keep callbacks). Callbacks are spliced from per-state lists
     into view-keyed compositor lists at role-apply time, so the atomic owner switch intercepts
     at those splice points; **fifo-v1 barrier clearing is its own line item** — it is
     view-transaction-driven, and a fifo-committing client in a consumer-paced, locally-invisible
     tree hangs unless the export module takes over barrier clearing.
  3. **Popup work-area override** — the constraint rectangle is `work_area_monitor`
     (`constraints.c:485-487,856-864`); substituting consumer bounds is ~100–200 LOC, the same
     seam KWin VR patched.
  4. **Release-join choice** — §1's option (i) or (ii) in `meta-wayland-buffer.c`.
  5. **Detach/adopt entry points** — over the existing drag API with the §1 audit
     (`meta_compositor_get_current_window_drag`, public `meta_window_begin_grab_op`, pointer
     warp; in-tree `xdg-toplevel-drag` is the semantic precedent).
  6. **Transaction-boundary hook** — the `applied` signal is per-surface-state; a synchronized
     subsurface transaction fires N signals with no transaction-complete boundary, so the
     protocol's atomic `done` needs a new hook in `meta-wayland-transaction.c`
     (`meta_wayland_transaction_apply` has no signal today). Expect this to be the
     most-scrutinized diff in the module MR.

Upstream-sized MR series, each independently defensible: (a) ext-foreign-toplevel-list support
(~600 LOC, independently valuable, the easiest landing); (b) release-join choice + dmabuf plane
accessors (small, syncobj-adjacent); (c) popup bounds override hook; (d) transaction-apply
signal + frame-callback pacing hook; then (e) the export module and (f) input targeting once the
protocol has upstream standing.

## 4. Privilege: an explicit decision, not a mechanism name

The red-team review's central finding: **GNOME currently has no category for a privileged
non-GNOME system client, and `ServiceChannel` does not create one.** ServiceChannel's pidfd
check is race-free *identification*, not authorization — today any session-bus process can call
`OpenWaylandServiceConnection` for one of its three hardcoded, GNOME-owned portal-backend types
(`meta-service-channel.c:115-209`), and can even displace the real backend's slot. GNOME
tolerates this because x11-interop caps are low-stakes; an export cap (read any window, inject
input) is full-session-compromise-grade, so the missing authorization layer becomes the whole
review. gnome-remote-desktop is *not* a precedent here: its privilege flows through the private
`org.gnome.Mutter.ScreenCast`/`RemoteDesktop` D-Bus APIs reserved for GNOME's own components.

The brief therefore poses M-C's GNOME privilege question as a decision between the two shapes
that exist:

- **(i) Portal-fronted grant (recommended, consistent with GNOME culture).** A portal interface
  in front of the global: the consumer requests export access through xdg-desktop-portal, the
  user sees a consent dialog with a persistence story (restore-token-like), and the granted
  session carries the dedicated Wayland fd + cap. Cost: a portal interface design + backend
  implementation + the Mutter cap plumbing — substantially more than "~150 LOC", and it must be
  in the M-C proposal, not discovered in review.
- **(ii) Upstream-named service type.** A new `MetaServiceClientType` naming the consumer class,
  which means Mutter upstream ships knowledge of a specific external consumer — no precedent
  (all three existing types are GNOME's own), so this path requires the consumer's identity
  itself to be upstream-blessed (e.g. a freedesktop-hosted reference consumer), and an
  authorization check added to ServiceChannel regardless.

Either way: **new security surface that must be designed and costed in the proposal.** The
conformance spec's §2.1 language is mechanism-neutral deliberately; for GNOME, "producer binding
policy" *is* consent machinery.

## 5. Shell (JS) involvement

None in the data path; policy only — with one shape-mismatch the seam list carries: the
session-lock revocation chain (`gnome-shell/js/ui/main.js:138-147` →
`meta_remote_access_controller_inhibit_remote_access`, `meta-remote-access-controller.c:146-184`)
iterates `MetaDbusSessionManager` sessions, and **exports are Wayland-connection state, not
D-Bus sessions** — wiring them in means either a per-export inhibitable session object or a
parallel inhibit path on the controller (small, core-adjacent, part of the module MR). The
GNOME-applicable form of conformance test 15 is also different: GNOME's lock screen is
shell-internal chrome (no ext-session-lock, no lock *surface* in the export domain), so the
hazard is delegated app windows streaming between the lock transition and asynchronous inhibit
propagation — the test asserts no `done` after the revocation point and a bounded revocation
latency, not lock-surface non-export. No extension-based implementation is possible (extensions
run inside the shell process against unstable internals; the buffer-lifetime ABI must be C) —
confirming research/32 §6.2.

## 6. Engagement conditions (what makes Mutter maintainers say yes)

1. **Standardization first**: `xx_toplevel_export` at wayland-protocols with the ext bar already
   met — smithay reference producer + KWin MR series + the zxr consumer + published interop
   tests (conformance §8). This matches every recent Mutter adoption (§1's record).
2. **Pre-empt the PipeWire counter-proposal.** GNOME's obvious counter is: *"fix RecordWindow —
   dmabuf window streams, resizable streams, negotiated formats, over the existing private
   D-Bus + PipeWire — why is any of this a Wayland protocol?"* That counter genuinely covers
   the capture half (R5/R6 delivery, much of R11–R14 via PipeWire's own pacing/feedback). The
   pitch must lead with what cannot ride PipeWire: the **node-addressed input back-path with
   producer-retained grabs/serials**, the **live popup tree with consumer-bounds reconstraint**
   (a stream has no positioner), and **detach/adopt** (a stream cannot end an interactive move
   or continue one). Concede the capture half explicitly; the protocol's case stands on the
   interactive half.
3. **A demand signal that exists.** The red-team check refuted the g-r-d wedge: g-r-d contains
   zero per-window code (RecordWindow appears only in its vendored D-Bus XML) — treat per-window
   remote desktop as a **hypothesis to validate with the g-r-d maintainers before M-C**, not a
   claim. The demand that is real today: the portal window-share path (video conferencing
   through xdg-desktop-portal-gnome) feels §2's copy/monitor-size/format limits on every share,
   and the XR consumer (this project) is the second, fully-implemented example.
4. **Invariant-preserving framing**: R7–R9 presented as reuse of Mutter's own
   `use_count`/`release_points`/syncobj semantics with the §1 release-join choice stated
   honestly — no redesign of buffer lifetime paths.
5. **Input as protocol, not EIS-global**: Mutter's own code is the argument (global virtual
   devices + stage pick cannot address occluded exported windows, `meta-eis-client.c:467-506`);
   the per-node channel needs working grab/focus evidence from the other two producers before
   Mutter review.
6. **A named sponsor**: §1's adoption record says a motivated external contributor carries the
   series; spatial-os (or a contracted GNOME shop) plays that role at M-C+, offering §3's
   independently-defensible MRs (a)–(d) first.
