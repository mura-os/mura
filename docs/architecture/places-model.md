# Mura architecture: the places model

**Status:** draft design (places workstream). The window/space model the registry names its
largest structural gap (§8 item 1), designed on the evidence of
[research/34](../research/34-workspace-models.md) (workspace models across DEs and spatial OSes)
and the KWin VR mining ([31 §2.9–2.12](../research/31-kwin-vr.md)). Decisions ratified in
[ADR 0016](adr/0016-places-model.md). Grounding rule: layer 1 is grounded in **OpenXR `XrSpace`
semantics** — the spatial analog of the repo's XDG grounding rule; we do not invent frame
vocabulary where OpenXR has it.

## 1. The three-layer model

"Head-anchored vs room-anchored" is the right observation but the wrong primitive: those are two
prominent frames in a larger family (wrist panels, the docked output plane, a joined peer's
mapped frame, a vehicle frame). The model is three layers, each cut on the repo's
mechanism/policy knife ([desktop-environment.md §4](desktop-environment.md)):

```text
Layer 1 — THE FRAME GRAPH (model; authority plane; this document's core)
   typed frames + parenting + membership + reparent verbs
Layer 2 — ATTACHMENT CONSTRAINTS (policy; pluggable in-process modules)
   how content relates to its parent frame: rigid, lazy-follow, billboard, tether
Layer 3 — INTRA-PLACE LAYOUT (policy; per-place engines)
   how sibling members arrange within a place: free 3D, curved band, screen quad, docked flat
```

The floating-vs-tiling analogy lands per layer: tiling-vs-floating is a layer-3 choice;
follow-vs-pinned is a layer-2 choice; room-vs-head is a layer-1 choice. The model stays small;
everything users will want to swap is policy — the niri lesson (topology is policy,
[34 §4](../research/34-workspace-models.md)) applied three times.

## 2. Layer 1: the typed frame graph

Every placeable thing — window, place, panel, OSD — is **parented to a frame**. Frames form a
graph rooted in the runtime's spaces, and each frame carries four typing properties the model
must expose:

| Frame (XrSpace grounding) | Persistence | Stability contract | Motion class | Shareability |
|---|---|---|---|---|
| Map anchor (`XR_EXT_spatial_anchor` family, ADR 0009) | survives reboot (encrypted store) | receives bounded corrections; **corrections move the frame, never the rendered world mid-frame** (ADR 0009) | static-world | mappable to peers (mode 5) |
| LOCAL / STAGE (`XR_REFERENCE_SPACE_TYPE_LOCAL/STAGE`) | session (recenter re-seats it) | stable within session; recenter is an explicit event | static-world | no |
| VIEW / head (`XR_REFERENCE_SPACE_TYPE_VIEW`) | none | perfectly stable by definition (it *is* the pose) | body-locked | no |
| Hand / wrist (action spaces) | none | tracker-dependent; loss ⇒ fallback parent (head) | hand-locked | no |
| Docked output plane (ADR 0015) | while docked | fixed to the connector's presentation | static-presentation | no |
| Shared/peer frame (mode 5, [spatial-sharing.md §5](spatial-sharing.md)) | host-owned | host's map contract, mapped into visitor LOCAL | static-world (remote) | is the sharing mechanism |
| Vehicle (future: travel mode) | none | IMU-vs-visual disagreement is its defining property | vehicle-locked | no |

A **place** is a named node parented to exactly one frame, owning a member set (windows), a
layout policy (layer 3), optional entry policy (§5), and — if pinned — a persisted identity.
A **window** is parented to exactly one place (cardinality: ADR 0016 answer 1), except
overlay-class members (§4.3).

**Reparent verbs** — the anchor-system "transitions" collapse into one model operation plus UX
choreography (the [31 §2.9](../research/31-kwin-vr.md) vocabulary: edge-barrier detach,
cursor-anchor continuity, pick-warp re-entry — all first-class policy here, never residency
surgery per composition §7.3 constraint 5):

- **pin** — head/LOCAL → map anchor ("park this layout here"; creates or joins a pinned place);
- **summon** — map anchor → here (a *presentation* move: the place temporarily presents at a new
  pose; its home anchor is unchanged — dismissal returns it);
- **grab-all / follow-toggle** — current members → head frame (kwin-vr's grab-all precedent);
- **assign-to-frame** — the generic protocol-visible form (§6: ext-workspace `assign` between
  groups-as-frames).

## 3. Layer 2: attachment constraints

The subtle finding from the mining: KWin VR's follow mode is *not* rigid head-parenting — it is
position-pinned with orientation *hysteresis* (engage 40°, release 4°, eased —
[31 §2.10](../research/31-kwin-vr.md)). "Follow" is a **constraint controller between frames**.
The family: rigid lock, lazy-follow (deadband + easing), billboard-toward-user, distance tether,
gravity alignment. Constraints are pluggable policy modules executing under the effects module's
authority-tier comfort caps — the **velocity clamp over exponential easing** of composition §7.3
constraint 6, whose evidence is precisely that the uncapped implementations are the ones with
documented user pain ([36 §9](../research/36-vr-shell-interaction-patterns.md)).

## 4. Currency: decomposed, never one bit

### 4.1 Why

A place becomes "current" three ways — explicit selection (pager, summon), location (reloc
recognizes the room and your body is in the place's volume), or focus (you are interacting with
its members). Desktop models only have the first, so one `active` bit suffices; ours conflict.

### 4.2 The rules

- **Per-consumer selectors** over the precedence ladder *explicit selection > focus-derived >
  location-derived > sticky-last*: spawn-target (focus-first), pager-highlight
  (selection-first), notification-routing (location-first, presentation-policy scoped),
  restore write-target (membership facts, not currency), sharing scope (selection only),
  docked-presentation source (policy selection, ADR 0015).
- **Offer-never-yank**: location events (place entered/exited, reloc state changes) may *offer*
  currency and trigger entry policy; they never forcibly rearrange presentation — ADR 0009's
  corrections rule generalized to context.
- **Overlay-class exclusion**: head/hand-frame sticky members and follow places never compete
  for location currency (else nothing else could ever win); they gain currency only via focus or
  selection.
- **Walks are continuous**: transitions emit partial-transition offsets (KWin `currentChanging`
  precedent, [34 §1.1](../research/34-workspace-models.md)) so pagers can animate them, plus
  discrete enter/exit events with hysteresis (the reloc `LOCALIZED_TENTATIVE` shape).

### 4.3 The conflict table (normative reconciliations)

| # | Conflict | Reconciliation |
|---|---|---|
| C1 | Summoned place while located in another place's volume | Selection wins and persists until the next location *event* (room change) or dismissal; pager shows summoned=active, located=present (two protocol states). |
| C2 | Mid-walk / hallway (no containing place) | Sticky-last currency; spawn falls back to the head frame; pager may show the transition offset. |
| C3 | Two places visible at once (open door); gaze in the non-located one | Decompose: window focus may live in a non-current place (desktop focus-follows-mouse precedent); spawn follows focus context; notifications follow located-place policy. |
| C4 | Follow place is always "here" | Overlay class: excluded from location competition; focus/selection only. |
| C5 | Docked + doffed (ADR 0015) | Currency = docked presentation's place (policy selection); don resumes location-derived currency *as an offer*. |
| C6 | Boot before relocalization | Sticky-last/head staging; on `LOCALIZED`, anchored places populate in place; restore never blocks on reloc; reloc never yanks. |
| C7 | Joined remote place present in the room (mode 5) | Shareability-typed frames barred from location-derived currency for routing-class consumers; presentation currency by selection only (a peer's place must never capture your notifications). |

## 5. Lifecycle and entry policy

- **Transient by default** (GNOME shape, [34 §2](../research/34-workspace-models.md)): placing a
  window somewhere implicitly creates a transient place; empty transient places evaporate. The
  degenerate baseline — visionOS-style "place of one window" — is what mapping M1 ships first.
- **Pin to persist** (COSMIC shape, [34 §3](../research/34-workspace-models.md)): pinning names
  the place, writes it into the anchored store (ADR 0009), and upgrades it to an object that
  exists independently of its members — **an empty pinned place is legal** (the named kitchen
  layout with no windows), which is what restore restores into and entry policy hangs off.
- **Entry policy is a pinned-place property**: on place-entered (a location event), activate
  declared attachments and launch/summon declared members — the kitchen recipe app, the
  couch remote-in-hand (a hand-frame overlay member). **Consent rule**: entry actions are an
  explicit per-place user grant (auto-launching on camera-recognition of a room is never a
  default); grants surface in the settings model and are suspended while any capture/spectate
  session is active (doc 17's badging duty) and in multi-user/guest sessions.

## 6. Protocol mapping (the pager/overview seam)

Per doc 30 §5.1 and [34 §8.2](../research/34-workspace-models.md):

- **`ext-workspace-v1` base**: place → workspace handle; pinned places emit stable `id`
  (= the restore record's `place_id`); transient ones emit none; `name` = user's place name;
  coordinates at most a 1-D sort key (never metres — doc 30's rule).
- **Groups = reference frames** (ADR 0016 answer 3): world/map, head, hand, docked, shared —
  making `assign` the protocol-visible reparent verb ("make this place portable" = assign to
  head group). Room identity/labels ride the zxr extension (upstream groups are identity-less).
- **The zxr workspace extension carries** (COSMIC workspace-v2 as shape precedent): place kind
  (transient/pinned/overlay), anchor id + resolution state (reloc-pending/localized), metric
  transform/bounds, compositor-rendered preview source (capture stack, ADR 0012 §4.1),
  pin/rename operations, entry-policy presence (boolean — contents are settings-model data,
  never protocol).
- **Toplevel membership** rides a cosmic-info-shaped extension on foreign-toplevel handles
  (without it the overview cannot draw windows-in-places).
- Partial-transition offsets (§4.2) as an event on the active-place transition.

## 7. Restore, docked, and mode-5 bindings

- **Restore** ([34 §8.3](../research/34-workspace-models.md)): the restore manager binds
  `xdg-session-management` session ids to `place_id`s; membership is recordable at
  `add_toplevel` and applied within the initial configure; unresolved-anchor state = C6;
  delegated/proxied members are *slots* re-filled only when their producer re-exports
  (R23/R24, [foreign-session-integration.md §3.7](foreign-session-integration.md)).
- **Docked** (ADR 0015): the flat-layout slot is **per-place presentation** — each place may
  define its docked arrangement; the docked output is a group (frame) so pagers can `assign`
  places onto the monitor.
- **Mode 5** ([spatial-sharing.md §5](spatial-sharing.md)): the join unit is a place; what
  replicates is exactly the model — membership + transforms (+ redaction as membership
  filtering). The model is deliberately **state-sync-shaped** (registry §9's spin-out note
  confirmed): a place's state is a replicable document, which is also what makes the restore
  record and the mode-5 wire format the same schema family.

## 8. Budget impact (per [budgets.md](budgets.md) standing rule)

Frame-graph maintenance is O(active frames + places) bookkeeping executed off the frame path;
per-frame cost on the display path is limited to transform composition for visible members
(already counted in the zxr composition line). Attachment constraints run in the capped effects
tier (already budgeted). Entry-policy evaluation is event-driven (reloc/volume events at
mapping-service rate, not frame rate). Reloc/mapping costs are the perception plane's existing
lines. Net new frame-path cost: none. Net new async cost: negligible (event handlers).

## 9. Open items

Every item names its decider (docs README rule: designs specify or ask; order lives in
implementation-path).

- The place-volume definition (how a place's containment region is authored/derived — bounds
  from layout vs explicit volume vs room mesh) — decided by the geometry service's room
  segmentation (spatial-mapping §7) plus a UX pass in the shell-plane design round.
- ~~Multi-user place ownership~~ **RESOLVED** by [multi-user.md](multi-user.md) §places
  (per-account place sets partition by login; shared-place semantics recorded there).
- Vehicle frame semantics (travel mode): a condition-shaped rule — the vehicle frame type is
  added only when a target device ships motion-vs-visual disagreement handling; until then the
  frame taxonomy deliberately excludes it.
- The zxr workspace extension is drafted (`protocols/zxr-workspace-v1.xml`, rev 2); its
  remaining field questions are tracked in the protocol file itself.
