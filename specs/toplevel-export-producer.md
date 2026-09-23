# specs/toplevel-export-producer: producer conformance for zspatial-toplevel-export-v1

**Status:** draft rev 2 (producer-specification workstream; KWin-persona and Mutter-persona
red-team findings absorbed — delegated local-state model added, SSD/Xwayland/multi-seat
obligations added, pacing and flow-control obligations restated in testable terms).
**Contract:** [`protocols/zspatial-toplevel-export-v1.xml`](../protocols/zspatial-toplevel-export-v1.xml)
(rev 3) is the wire contract; this spec is the *behavioral* contract a producer compositor must
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
textures; the added cost is bounded by the advertised in-flight cap per tree (§3.4) and the
pacing channel is a few small messages per consumer frame per tree. Nothing new on the producer's
client hot path: export observation rides the commit bookkeeping the producer already does.

## 1. Roles and scope

A **producer** is a Wayland compositor that delegates individual toplevels. A **consumer** is a
privileged Wayland client of the producer that presents them (zxr presents them as floating
planes; the protocol is XR-agnostic and a 2D nested consumer is equally valid). Delegation is
not capture (the sharing stack's portal machinery does not govern it), not remote transport, and
not a nested session: the producer keeps full shell authority over its clients throughout.
**Consent and authorization are the producer's binding policy** — which, per ecosystem, may
itself be portal-mediated (GNOME), a trusted dedicated connection (KWin), or a filtered global
(smithay/wlroots); this spec deliberately admits all of them (§2.1).

Producer conformance has one **mandatory core** — tree export with correct buffer lifetime,
delegated local state, and revocation (§2, §3, §6) — and four **capability-gated extensions**:
input (§5), consumer pacing (§4), detach/adopt (§7), and dnd (reserved, absent in v1). A
producer advertising a capability must satisfy that section entirely; the protocol's uniform
failure rule applies (a tree whose backing capability is absent or lost receives
`denied(unsupported)`).

## 2. Exposure, identity, and tree export

- **2.1 Privileged global (R2).** The `zspatial_toplevel_export_manager_v1` global must be
  restricted to authorized connections using the producer's privileged-client mechanism
  (dedicated pre-authorized connection, security-context filtering, portal-mediated grant, or
  equivalent). `ext-foreign-toplevel-list` visibility must not imply export authority.
- **2.2 Identity is the handle (R1).** Export binds to an `ext_foreign_toplevel_handle_v1`;
  producers must never resolve by title/app-id. A race with toplevel closure resolves as
  `denied(gone)`, never as a protocol error.
- **2.3 Complete tree (R3).** The producer must export the root, every mapped subsurface, and
  every popup/transient of the delegated toplevel as separate nodes with parent and stacking
  relations, announced parent-first and bottom-to-top within a parent; restacking uses the
  `restacked` event. A consumer must be able to reconstruct the producer's z-order within the
  tree exactly.
- **2.4 Atomic application (R4).** All state observed from client commits — attach, damage,
  position, scale/transform, viewport, window geometry, input/opaque regions, stacking — must be
  forwarded and applied at `done` boundaries that correspond to atomic application points of the
  client's own commit sequence, including synchronized-subsurface batching: **the consumer must
  never be able to observe a tree state that no atomic application of the client's commits ever
  produced.** (Stated against the commit sequence, not the producer's render scene — a delegated
  tree is typically not in the producer's scene at all, §2.8.) Where the producer's atomic unit
  is a transaction spanning surfaces, `done` corresponds to transaction application.
- **2.5 Redaction (R21).** Producer policy may deny or revoke any surface (lock surfaces,
  protected content, internal/private surfaces) via `denied`; the `policy` reason must not leak
  which rule matched. Overrun revocation uses the dedicated `overrun` reason — the consumer
  caused it and needs to know retry-with-tighter-discipline is correct recovery.
- **2.6 Decorations.** Every node carries `decoration_mode`. `client` (CSD — the GNOME norm):
  decorations are part of the exported buffer, and window geometry tells the consumer where the
  window is inside it. `server_excluded` (SSD — the KDE norm): the producer's decoration is NOT
  exported; the producer must set window geometry accordingly and honor
  `zspatial_exported_tree_v1.close` and `request_size` so the consumer can provide its own
  affordances. Producers must never rasterize their decoration into the exported content.
- **2.7 Non-xdg toplevels (Xwayland).** Version-1 producers must not export X11/Xwayland
  toplevels: `denied(unsupported)`. (Their popups are independent override-redirect windows
  placed in absolute screen coordinates with no positioner for §set_bounds to act on, and their
  geometry semantics differ; a defined degraded mapping is future work, not silence.)
- **2.8 Delegated local state (the parked-window model).** While a toplevel is delegated, the
  producer must, normatively:
  1. **cease presenting it locally** — it is not rendered in the producer's own outputs;
  2. **deliver no local input to it** — the producer's own pointer/touch hit-testing must not
     resolve to delegated surfaces (consumer input via §5 is the only input path);
  3. **freeze topology-derived geometry** — local output hotplug, work-area changes, panel
     changes, and placement policies must not move or resize a delegated toplevel; while
     delegated, `request_size` is the only geometry intent the producer derives configure sizes
     from (client-initiated and rule-forced resizes remain legal and are simply forwarded);
  4. **exclude delegation from session state** — a session save while delegated records the
     window's last non-delegated placement; delegation itself is never persisted;
  5. represent it in switchers/taskbars per producer policy (recommended: listed, marked as
     delegated; activating it is an `adopt`-equivalent recall or a no-op, per policy) — this is
     the one deliberately policy-shaped row.
  These five rules are where the single-compositor "VR mode" forks bled (double rendering,
  double input, output-reassignment fights); they are obligations here precisely so every
  producer answers them the same way.

## 3. Buffer delivery and lifetime

- **3.1 Original allocations (R5).** `buffer` events must lease the client's own dmabuf planes
  (duplicated fds of the same allocation) with exact fourcc/modifier/plane layout and honest
  device identity (`main_device`, per-buffer device changes revoke instead of lying).
- **3.2 Typed fallback (R6).** shm-only clients, cross-device buffers the consumer cannot
  import, protected content, and surfaces carrying committed colour state that v1 cannot express
  (`color_state` — producers advertising nothing must not deliver silently-misinterpreted HDR
  pixels) produce `fallback`/`denied` with the typed reason — never a silent copy path inside
  this protocol.
- **3.3 Explicit sync (R7–R9).** Timelines are imported once (`import_timeline`) and referenced
  by point thereafter. Every attach carries an acquire point the consumer waits on. Release is
  the mirrored join: the consumer returns one release point per attach; the producer must signal
  the *client's* release (or reuse the buffer) only after BOTH its own local completion AND all
  outstanding consumer release points for that buffer signal. Consumer release points may
  complete out of order across buffers; the producer must not serialize them through one
  monotonic timeline shared across reusable buffers.
- **3.4 Bounded in-flight (R10).** The producer advertises its in-flight cap per tree
  (`flow_control`, before the first `done`). The cap is a defense against pathological consumers
  — real clients self-throttle on their swapchains and frame callbacks long before any sane cap.
  On overrun the producer must revoke (`denied(overrun)`) rather than block its client or its
  own loop: no producer path that serves its own clients may wait on consumer progress (the
  never-block discipline, matching [perception-intake](perception-intake.md) §8).
- **3.5 Teardown ordering (R20).** On revocation, consumer destroy, adopt, or consumer
  disconnect: input state is cancelled immediately (§5.6), but client buffers are released only
  once outstanding consumer release points signal **or the consumer connection is gone**. Buffer
  objects are exempt from denied-inertness precisely so those releases can still arrive after a
  revocation. GPU reset on either side revokes with `gpu_reset`.

## 4. Pacing (capability `pacing`)

- **4.1 One owner (R12).** Frame-callback dispatch for the delegated tree has exactly one owner
  at a time: the producer's own scheduler (default) or the consumer's `frame` stream. The switch
  must be atomic in this testable sense: **every outstanding `wl_surface.frame` callback fires
  exactly once, eventually**, and no commit's callbacks are dispatched under both regimes.
  Because a delegated tree is locally invisible (§2.8), the producer-paced default for it is the
  producer's hidden-window cadence (its offscreen/suspension rules apply); producers must
  document that cadence, and consumers should hold a pacing object whenever they present the
  tree live.
- **4.2 Cadence (R11).** While consumer-paced, the producer schedules the tree's client frame
  callbacks against the consumer's predicted display times and cutoffs, converted through the
  supplied clock correlation (resynced periodically per the protocol's drift note). Clients of
  the producer need no changes: they see ordinary frame callbacks at a different cadence.
  Producers implementing fifo-v1/commit-timing must keep their forward-progress guarantees for
  consumer-paced trees (barrier clearing retargets to the consumer cadence).
- **4.3 Honest feedback (R13, R22).** The consumer reports exactly one of
  `mark_presented`/`mark_discarded` per `done` commit id while consumer-paced; the producer
  translates the reports into its own presentation-time feedback toward the client.
  `zero_copy` reflects the consumer's final presentation path only.
- **4.4 Late frames (R14).** A commit that misses its cutoff is not re-presented by the
  producer; the consumer reuses its last-ready state and eventually reports the commit
  `mark_presented` (first sampled later) or `mark_discarded` (superseded first).

## 5. Input (capability `input`)

- **5.1 Node addressing and seat binding (R16).** The input object is bound to one producer
  seat at creation. Consumer input names a tree node and carries node surface-local coordinates;
  the producer maps node → its own surface object and injects into that seat's normal delivery
  path so that all its existing filters, grabs, and semantics apply. **The shared-seat model is
  explicit**: this protocol assumes one human using both presentations; consumer input contends
  for seat focus like any local interaction (there is exactly one keyboard focus, and delivering
  consumer keys moves it).
- **5.2 Producer authority (R17).** Wire serials toward the client are the producer's own. The
  producer runs implicit grabs, popup grabs and dismissal, and focus by its own rules, reporting
  results (`focus`, `popup_dismissed`) as information, not as consumer-controlled state.
  `keyboard_enter`/`keyboard_leave` carry the consumer's focus *intent*; `pointer_leave` and
  `touch_cancel` end consumer-originated hover/touch state (subject to live implicit grabs).
- **5.3 Keymap ownership.** The producer announces the serial-carrying keymap; `keyboard_key`
  codes must be valid in the keymap whose serial they cite (stale-serial keys are dropped, never
  reinterpreted). Consumers whose users type on other layouts use the `keyboard_keysym` channel
  and the producer maps keysyms with its own machinery. Modifier state is computed by the
  producer. Timestamps on all input are advisory; the producer restamps into its own clock
  domain on delivery, so client-observed inter-event timing is always producer-consistent.
- **5.4 Activation (R18).** `activate` carries intent only; the producer mints/validates its own
  activation token subject to its focus-stealing policy. Foreign activation tokens are
  unrepresentable by construction.
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
| Capability lost dynamically | `denied(unsupported)` on dependent trees |
| Consumer disconnect | teardown per §3.5, input cancel per §5.6 |
| Producer GPU reset | `denied(gpu_reset)`; leases invalid, client re-allocation resyncs |
| Flow-control overrun | `denied(overrun)` past the advertised cap (never blocking) |

All triggers leave the *producer's client* in a state indistinguishable from an ordinary
compositor interaction, and reclaim local presentation per §2.8's inverse (the window returns to
the producer's scene at its recorded placement).

## 7. Detach and adopt (capability `detach_drag`)

- **7.1 Detach (R23).** `detach_drag` binds to the seat's active move-grab. The producer ends
  its interactive move with **no placement side-effects** — no electric borders, no tiling
  snap, no output reassignment, no raise beyond what its policy already did — and emits `anchor`
  (the root-surface-local point under the cursor at handoff) before the first `done`. The
  no-move race resolves as `denied(no_active_move)`.
- **7.2 Adopt (R24).** `adopt` lands the toplevel at the hint: the producer places it at the
  landing point on the named output (or as if no hint were given, when the output is null or
  gone) and, with `resume_move`, warps its pointer there and starts an interactive move that
  follows the producer's own pointer and ends on its next button-release or cancel — exactly an
  `xdg_toplevel.move`-initiated move. Adopt on a revoked tree behaves as destroy. Only delegated
  toplevels are adoptable; the asymmetry (consumer-native clients can never enter the producer)
  is structural.

## 8. Conformance tests

Interop tests are run producer × consumer; the reference consumer is zxr's delegation intake,
the reference producer the smithay implementation (§9.1). Each test names its normative source.

1. **Privilege filter** (§2.1): an unauthorized connection neither sees the manager global nor
   gains export by guessing names; foreign-toplevel-list enumeration alone grants nothing.
2. **Tree fidelity** (§2.3–2.4): client with two synchronized subsurfaces + a repositioned
   popup; the consumer-reconstructed z-order and state at every `done` equal an atomic
   application point of the client's commit sequence (existence-checked against a recorded
   commit log; the test verifies sampled states match, not that no other interleaving exists).
3. **Restacking** (§2.3): a client reorders subsurfaces; the consumer's order matches after the
   next `done`, delivered via `restacked`, with no node re-announcement.
4. **Modifier/device negotiation** (§3.1–3.2): matching-device dmabuf delivers the client's
   planes (fd/stride/offset identity verified); an shm client yields `fallback(shm_only)`; a
   mismatched device yields the typed reason, no copies.
5. **Out-of-order release** (§3.3): two buffers released in reverse order; client reuse gated
   correctly per buffer; no premature client release observed (validation on the client's
   release timeline).
6. **Flow control** (§3.4): a *synthetic* consumer that holds attaches without releasing (real
   consumers never reach the cap) exceeds `flow_control.max_in_flight` ⇒ `denied(overrun)`;
   the producer's client and loop never stall (watchdog).
7. **Disconnect mid-lease** (§3.5): consumer killed with attaches outstanding; producer releases
   only after its own completion; client sees a normal release, no corruption, no leak (fd
   census before/after).
8. **Revocation release path** (§3.5): tree revoked (`denied(policy)`) with attaches in flight;
   the consumer's subsequent `release` requests are honored (buffers exempt from inertness) and
   the client's buffers unwind without waiting for consumer disconnect.
9. **Pacing switch** (§4.1): toggling consumer-paced mode under a 60 Hz committing client —
   every outstanding frame callback fires exactly once (callback census over the switch), none
   under both regimes.
10. **Cadence + late frame** (§4.2–4.4): consumer at 90 Hz with a tight cutoff; producer
    schedules callbacks to land before cutoffs; a deliberately-late commit is not re-presented;
    consumer reports exactly one outcome per commit id and the client's presentation feedback
    matches.
11. **Popup reconstraint** (§2.3 + `set_bounds`): consumer shrinks bounds so a mapped popup no
    longer fits; producer repositions via its positioner and echoes the bounds serial in
    `configure_bounds`.
12. **Resize + scale** (§2.8.3): `request_size` produces a configure; a local output hotplug
    and panel change produce none; `set_preferred_scale` reaches the client via the producer's
    scale machinery.
13. **Grabs and dismissal** (§5.2): consumer clicks outside an open popup-grab region; the
    producer dismisses (`popup_dismissed` + `unmapped`), serials remain producer-consistent
    (client-side protocol validity checked with a strict toolkit).
14. **Leave/cancel semantics** (§5.2): `pointer_leave` clears hover (tooltip never fires);
    `touch_cancel` delivers wl_touch.cancel; `keyboard_leave` releases keys this object pressed.
15. **Keymap change mid-stream** (§5.3): producer keymap changes between key presses; stale-
    serial keys are dropped, fresh-serial keys deliver; a `keyboard_keysym` round-trips a symbol
    absent from the producer's base layout.
16. **Stuck-input cancellation** (§5.6): consumer dies mid-button-down and mid-touch; client
    receives release/cancel; no stuck state after reconnect.
17. **Activation intent** (§5.4): `activate` raises/focuses per producer policy; a synthetic
    foreign token replay is impossible by construction (API review, not runtime).
18. **Input timestamps without pacing** (§5.3): input delivered with no pacing object; the
    client observes producer-clock-consistent inter-event timing (double-click and kinetic
    velocity behave normally).
19. **Delegated local state** (§2.8): while delegated — the window is absent from the
    producer's outputs (screenshot diff), local clicks at its parked rectangle hit what is
    beneath it, and a session save/restore records the pre-delegation placement.
20. **Xwayland denial** (§2.7): exporting an X11 toplevel yields `denied(unsupported)`.
21. **Detach race** (§7.1): move ended between user gesture and request arrival ⇒
    `denied(no_active_move)`; an actual detach carries a correct `anchor` (pixel-identity
    verified against the pre-detach cursor position).
22. **Adopt resume-move + dead output** (§7.2): adopt with `resume_move` continues the move at
    the landing point and ends on the producer's next button-release (final placement equals a
    native move ended there); adopt naming a removed output places as if unhinted.
23. **Capability absence** (protocol failure rule): `detach_drag`/`get_pacing`/`get_input`
    without the capability ⇒ `denied(unsupported)` on the tree, never a silent-forever object.
24. **Lock revocation** (§6): producer session locks mid-delegation; every tree receives
    `denied(session_locked)` with no `done` after the revocation point and bounded revocation
    latency; where the producer's lock surfaces live in the export domain, none is ever
    exported. (On GNOME the lock is shell-internal chrome — the ordering-and-latency half is
    the applicable form.)

## 9. Ecosystem annex

*Grounded in [research/40](../docs/research/40-toplevel-export-producers.md); the full
per-compositor patch plans live in the producer briefs.*

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
  green-field pieces (tree mirroring, pacing, input dispatch, detach/adopt, and now the §2.8
  delegated-state policy) — calibrated against real modules of the same shape (smithay's
  toplevel list 542 LOC, cosmic's zcosmic info server 724 LOC, its capture handler ≈1.4 kLOC).

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

Consumer pacing (§4), node-tree wire-mirroring (§2.3–2.4 as protocol), the input back-path
(§5), detach/adopt (§7), and the delegated local-state discipline (§2.8) are green-field in
every surveyed producer. The release join and the privilege model, by contrast, are restatements
of existing idioms — the spec deliberately demands nothing novel there.

## 10. Open items

DnD (R19) design for v2; colour-state forwarding (the reserved `color` capability — blocked on
the colour-pipeline design, registry colour row); a defined Xwayland degraded mapping (§2.7
lifts the v1 denial); tablet/pen input classes; the M-C upstream proposal text.
