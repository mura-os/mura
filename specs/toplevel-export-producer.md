# specs/toplevel-export-producer: producer conformance for zext-toplevel-export-v1

**Status:** draft (producer-specification workstream).
**Contract:** [`protocols/zext-toplevel-export-v1.xml`](../protocols/zext-toplevel-export-v1.xml)
(rev 2) is the wire contract; this spec is the *behavioral* contract a producer compositor must
satisfy behind that wire, plus the conformance tests both sides run. Requirement numbers Rn refer
to [research/32 §8](../docs/research/32-toplevel-export-prior-art.md); design rationale in
[foreign-session-integration.md](../docs/architecture/foreign-session-integration.md) and
[ADR 0014](../docs/architecture/adr/0014-toplevel-delegation-protocol.md). Per-compositor
integration briefs: [producers/kwin.md](../docs/architecture/producers/kwin.md),
[producers/mutter.md](../docs/architecture/producers/mutter.md); code evidence in
[research/40](../docs/research/40-toplevel-export-producers.md).
**Grounding:** "XDG" here means the xdg-shell protocol family (sense (b) of the docs-README
disambiguation): every rule below is stated against xdg-shell/wl_surface semantics the producer
already implements for its own clients.
**Budget impact** (inv. 9, consumer side): delegated trees enter zxr's 2D quad tier as ordinary
textures; the added cost is bounded by the negotiated in-flight cap per tree (§3.4) and the
pacing channel is one small message per consumer frame per tree. Nothing new on the producer's
client hot path: export observation rides the commit bookkeeping the producer already does.

## 1. Roles and scope

A **producer** is a Wayland compositor that delegates individual toplevels. A **consumer** is a
privileged Wayland client of the producer that presents them (zxr presents them as floating
planes; the protocol is XR-agnostic and a 2D nested consumer is equally valid). Delegation is
not capture (no portal/consent machinery of the sharing stack governs it — producer binding
policy does), not remote transport, and not a nested session: the producer keeps full shell
authority over its clients throughout.

Producer conformance has one **mandatory core** — tree export with correct buffer lifetime and
revocation (§2, §3, §6) — and four **capability-gated extensions**: input (§5), consumer pacing
(§4), detach/adopt (§7), and dnd (reserved, absent in v1). A producer advertising a capability
must satisfy that section entirely; a producer omitting it must ignore the corresponding
requests per the protocol's capability rule.

## 2. Exposure, identity, and tree export

- **2.1 Privileged global (R2).** The `zext_toplevel_export_manager_v1` global must be filtered
  to authorized connections using the producer's privileged-global mechanism (security-context
  filtering or equivalent). `ext-foreign-toplevel-list` visibility must not imply export
  authority: an unauthorized client seeing handles gains nothing.
- **2.2 Identity is the handle (R1).** Export binds to an `ext_foreign_toplevel_handle_v1`;
  producers must never resolve by title/app-id. A race with toplevel closure resolves as
  `denied(gone)`, never as a protocol error.
- **2.3 Complete tree (R3).** The producer must export the root, every mapped subsurface, and
  every popup/transient of the delegated toplevel as separate nodes with parent and stacking
  relations, announced parent-first. Flattening is not permitted; a consumer must be able to
  reconstruct the producer's z-order within the tree exactly.
- **2.4 Atomic application (R4).** All state observed from client commits — attach, damage,
  position, scale/transform, viewport, window geometry, input/opaque regions — must be forwarded
  and applied at `done` boundaries that correspond to the producer's own atomic application of
  the same state, including synchronized-subsurface batching: the consumer must never be able to
  observe a tree state the producer's own scene never contained.
- **2.5 Redaction (R21).** Producer policy may deny or revoke any surface (lock surfaces,
  protected content, internal/private surfaces) via `denied`; the `policy` reason must not leak
  which rule matched. Cursor and decoration surfaces the producer synthesizes are not part of
  the client tree and must not be exported.

## 3. Buffer delivery and lifetime

- **3.1 Original allocations (R5).** `buffer` events must lease the client's own dmabuf planes
  (duplicated fds of the same allocation) with exact fourcc/modifier/plane layout and honest
  device identity (`main_device`, per-buffer device changes revoke instead of lying).
- **3.2 Typed fallback (R6).** shm-only clients, cross-device buffers the consumer cannot
  import, and protected content produce `fallback`/`denied` with the typed reason — never a
  silent copy path inside this protocol.
- **3.3 Explicit sync (R7–R9).** Every attach carries an acquire timeline point the consumer
  waits on. Release is the mirrored join: the consumer returns one release point per attach;
  the producer must signal the *client's* release (or reuse the buffer) only after BOTH its own
  local completion AND all outstanding consumer release points for that buffer signal. Consumer
  release points may complete out of order across buffers; the producer must not serialize them
  through one monotonic timeline shared across reusable buffers.
- **3.4 Bounded in-flight (R10).** The producer enforces a negotiated in-flight cap per tree.
  On overrun or consumer stall it must revoke (`denied`) rather than block its client or its own
  loop — the never-block discipline is structural, matching the
  [perception-intake](perception-intake.md) §8 conformance shape: no producer path that serves
  its own clients may wait on consumer progress.
- **3.5 Teardown ordering (R20).** On revocation, consumer destroy, adopt, or consumer
  disconnect: input state is cancelled immediately (§5.6), but client buffers are released only
  once outstanding consumer release points signal **or the consumer connection is gone** (kernel
  fd semantics then guarantee no further GPU reads can be issued against the leases). GPU reset
  on either side revokes with `gpu_reset`.

## 4. Pacing (capability `pacing`)

- **4.1 One owner (R12).** Frame-callback dispatch for the delegated tree has exactly one owner
  at a time: the producer's own scheduler (default) or the consumer's `frame` stream
  (consumer-paced). The switch — `get_pacing` + `set_consumer_clock` in, pacing destroy out —
  must be atomic: no commit may observe both modes, and no client frame callback may be dropped
  or double-fired across the switch.
- **4.2 Cadence (R11).** While consumer-paced, the producer schedules the tree's client frame
  callbacks against the consumer's predicted display times and cutoffs, converted through the
  supplied clock correlation. Clients of the producer need no changes: they see ordinary frame
  callbacks at a different cadence.
- **4.3 Honest feedback (R13, R22).** Exactly one of `presented`/`discarded` per `done`
  commit id while consumer-paced. `presented` reflects first actual sampling by the consumer;
  `zero_copy` reflects the consumer's final presentation path only — the producer must not set
  it because the wire had no intermediate copy.
- **4.4 Late frames (R14).** A commit that misses its cutoff is not re-presented by the
  producer; the consumer reuses its last-ready state and the commit is eventually `presented`
  (first sampled later) or `discarded` (superseded first).

## 5. Input (capability `input`)

- **5.1 Node addressing (R16).** Consumer input names a tree node and carries node
  surface-local coordinates. The producer maps node → its own surface object and injects into
  its normal delivery path so that all its existing filters, grabs, and semantics apply.
- **5.2 Producer authority (R17).** Wire serials toward the client are the producer's own. The
  producer runs implicit grabs, popup grabs and dismissal, and focus by its own rules,
  reporting the results (`focus`, `popup_dismissed`) as information, not as consumer-controlled
  state.
- **5.3 Keymap ownership.** The producer announces the keymap (`keymap` event) and the consumer
  must send key codes valid in it. Modifier state is computed by the producer.
- **5.4 Activation (R18).** `activate` carries intent only; the producer mints/validates its
  own activation token subject to its focus-stealing policy. Foreign activation tokens must be
  rejected at the boundary by construction (the protocol cannot carry them).
- **5.5 No DnD in v1 (R19).** Producers must not advertise `dnd`.
- **5.6 Cancellation.** Input-object destroy, tree teardown, or revocation cancels
  consumer-originated state toward the client: pointer leave, key releases, touch cancel — the
  client must never be left with a stuck button/key/touch from a vanished consumer.

## 6. Revocation matrix (R20)

| Trigger | Producer obligation |
|---|---|
| Toplevel unmapped/closed | `unmapped` per node, `denied(gone)` on the tree |
| Producer session locks | `denied(session_locked)` on every tree (unless policy exempts) |
| Consumer authorization lost | `denied(policy)` |
| Consumer disconnect | teardown per §3.5, input cancel per §5.6 |
| Producer GPU reset | `denied(gpu_reset)`; leases invalid, client re-allocation resyncs |
| Flow-control overrun | `denied(policy)` after the negotiated cap (never blocking) |

All triggers leave the *producer's client* in a state indistinguishable from an ordinary
compositor interaction (a window that was delegated and revoked has simply "not been visible").

## 7. Detach and adopt (capability `detach_drag`)

- **7.1 Detach (R23).** `detach_drag` binds to the seat's active move-grab. The producer ends
  its interactive move with **no placement side-effects** — no electric borders, no tiling
  snap, no output reassignment — and emits `anchor` (the root-surface-local point under the
  cursor at handoff) before the first `done`. The no-move race resolves as
  `denied(no_active_move)`.
- **7.2 Adopt (R24).** `adopt` lands the toplevel at the hint: producer warps its pointer to
  the landing point on the named output and, with `resume_move`, continues its interactive move
  there with final placement semantics as if `xdg_toplevel.move` ended at that point. Only
  delegated toplevels are adoptable; the asymmetry (consumer-native clients can never enter the
  producer) is structural.

## 8. Conformance tests

Interop tests are run producer × consumer; the reference consumer is zxr's delegation intake,
the reference producer the smithay implementation (§9.1). Each test names its normative source.

1. **Privilege filter** (§2.1): an unauthorized connection neither sees the manager global nor
   gains export by guessing names; foreign-toplevel-list enumeration alone grants nothing.
2. **Tree fidelity** (§2.3–2.4): client with two synchronized subsurfaces + a repositioned
   popup; consumer-reconstructed z-order and atomic state match the producer's scene at every
   `done`; no intermediate state is observable.
3. **Modifier/device negotiation** (§3.1–3.2): matching-device dmabuf delivers the client's
   planes (fd/stride/offset identity verified); an shm client yields `fallback(shm_only)`; a
   mismatched device yields the typed reason, no copies.
4. **Out-of-order release** (§3.3): two buffers released in reverse order; client reuse gated
   correctly per buffer; no premature client release observed (validation layer on the client's
   timeline).
5. **Consumer stall / flow control** (§3.4): consumer stops releasing; producer's client
   continues committing unimpeded until the cap, then the tree is revoked; producer loop never
   blocks (watchdog).
6. **Disconnect mid-lease** (§3.5): consumer killed with attaches outstanding; producer
   releases only after its own completion; client sees a normal release, no corruption, no
   leak (fd census before/after).
7. **Pacing switch atomicity** (§4.1): toggling consumer-paced mode under a 60 Hz committing
   client drops and duplicates zero frame callbacks (callback census).
8. **Cadence + late frame** (§4.2–4.4): consumer at 90 Hz with a tight cutoff; producer
   schedules callbacks to land before cutoffs; a deliberately-late commit is not re-presented,
   feedback reports exactly one outcome per commit id.
9. **Popup reconstraint** (§2.3 + protocol `set_bounds`): consumer shrinks bounds so a mapped
   popup no longer fits; producer repositions via its positioner and echoes the bounds serial
   in `configure_bounds`.
10. **Grabs and dismissal** (§5.2): consumer clicks outside an open popup-grab region; the
    producer dismisses (`popup_dismissed` + `unmapped`), serials remain producer-consistent
    (client-side protocol validity checked with a strict toolkit).
11. **Stuck-input cancellation** (§5.6): consumer dies mid-button-down and mid-touch; client
    receives release/cancel; no stuck state after reconnect.
12. **Activation intent** (§5.4): `activate` raises/focuses per producer policy; a synthetic
    foreign token replay is impossible by construction (API review, not runtime).
13. **Detach race** (§7.1): move ended between user gesture and request arrival ⇒
    `denied(no_active_move)`; an actual detach carries a correct `anchor` (pixel-identity
    verified against the pre-detach cursor position).
14. **Adopt resume-move** (§7.2): adopt with `resume_move` on output B continues the move at
    the landing point; final placement equals a native move ended there (position diff = 0).
15. **Lock revocation** (§6): producer session locks mid-delegation; every tree revoked with
    `session_locked` before any lock-surface content could be exported (ordering assertion).

## 9. Ecosystem annex

*Grounded in [research/40](../docs/research/40-toplevel-export-producers.md); this section
summarizes per-ecosystem feasibility — the full per-compositor patch plans live in the
producer briefs.*

### 9.1 smithay / COSMIC (the reference producer, ADR 0014 M-A)

The reference producer is a smithay compositor module; the study confirms the core obligations
map onto existing idioms:

- **§2.1/§2.2 (privilege + identity) are free**: smithay ships a complete
  ext-foreign-toplevel-list server with `from_resource` handle recovery, and every privileged
  smithay global takes a `can_view` filter closure. COSMIC's two security-context predicates
  gate all its privileged globals the same way, and its `zcosmic_toplevel_info` v2+ is the exact
  "privileged manager keyed off the neutral ext handle" shape `export_toplevel` uses.
- **§3.3/§3.5 (release join + teardown) are idiomatic**: smithay wraps every committed client
  buffer in a refcounted `Buffer` whose last-clone drop sends `wl_buffer.release` *and* signals
  the client's syncobj release point. The producer-owned join is "hold a clone until local and
  consumer completion, then drop" — cosmic-comp's image-copy-capture already holds exactly such
  clones until its render fence lands. One sharp edge: acquire points are consumed
  (`pub(crate)`) at commit, so the reference producer clones them from the syncobj cached state
  in its commit handler (or carries a one-line accessor patch).
- **§2.4 tree state and §4 pacing hooks**: cached per-surface damage/scale/viewport/region
  state, `PopupManager`, commit blockers, and tree-wide frame-callback dispatch with a
  caller-controlled clock all exist; the wire mirroring, commit-id ledger, and cadence
  scheduling are the hand-written parts.
- **§5 input**: smithay seat handles take arbitrary focus-target types and own
  serials/grabs/focus internally — node-targeted synthetic delivery is a routing layer.
- **Scope**: ~2–4 kLOC hand-written inside an existing smithay compositor, dominated by the
  green-field pieces (tree mirroring, pacing, input dispatch, detach/adopt) — calibrated
  against real modules of the same shape (smithay's toplevel list 542 LOC, cosmic's zcosmic
  info server 724 LOC, its capture handler ≈1.4 kLOC).

### 9.2 wlroots

The producer shape mirrors how ext capture composes today: the neutral handle from
`wlr_ext_foreign_toplevel_list_v1`, a privileged manager keyed off it (the ext
toplevel image-capture-source manager's deny-by-default request/accept split is the pattern to
imitate for `export_toplevel`), with the copy helper replaced by plane forwarding.
`wlr_buffer_lock` refcounting lets a producer hold the client's dmabuf arbitrarily past commit
(`wl_buffer.release` is driven by the lock count), with two recorded caveats: a held lock
disables the in-place shm texture-update fast path (the client is forced into multi-buffering),
and plane fds must be duplicated eagerly at export because the source is nulled if the client
destroys its `wl_buffer`. Privilege gating is `wl_display_set_global_filter` +
security-context lookup (display-wide, so the filter dispatches per-global);
`wlr_xdg_positioner_rules_unconstrain_box` takes an arbitrary constraint box, satisfying §2.3's
bounds substitution with no new geometry code.

### 9.3 What no producer has (the protocol's actual contribution)

Consumer pacing (§4), node-tree wire mirroring (§2.3–2.4 as protocol), the input back-path
(§5), and detach/adopt (§7) are green-field in every surveyed producer. The R8 release join and
the privilege model, by contrast, are restatements of existing idioms — the spec deliberately
demands nothing novel there.

## 10. Open items

DnD (R19) design for v2; colour-management state forwarding (blocked on the colour-pipeline
design, registry colour row); tablet/pen input classes; the M-C upstream proposal text.
