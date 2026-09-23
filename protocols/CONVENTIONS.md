# Protocol authoring conventions

Style guide for every Wayland protocol XML in `protocols/` (the `zxr`/`zext` families).
Every rule below is derived from upstream practice in the pinned clones under
`references/wayland-protocols/`; citations are `file:line` into those clones. Where upstream
is internally inconsistent, the picked side is marked **[picked]** and the alternatives noted.

## 1. Scope and enforcement

- Applies to every XML file in `protocols/`, present and future, regardless of status column
  in `protocols/README.md`.
- Enforced socially at review (a reviewer may block on any rule here) and mechanically by
  `tests/protocols.nix`: `xmllint --noout` plus `wayland-scanner` client-header, server-header,
  and private-code generation. A protocol that does not scan does not merge.
- The target register is wayland-protocols staging quality: these files are written as if a
  wayland-protocols member will review them, because on the `xx_` path one eventually will
  (GOVERNANCE.md:99-101 — formal in-depth review by a member project is an inclusion requirement).

## 2. File anatomy

- **Filename**: kebab-case with major-version suffix, `<name>-v1.xml`
  (wayland-protocols README.md:129-141; e.g. `ext-workspace-v1.xml`, `linux-dmabuf-v1.xml`).
- **Protocol `name` attribute**: snake_case of the filename including the version suffix:
  `<protocol name="ext_workspace_v1">` (ext-workspace-v1.xml:2). **[picked]** Upstream is
  inconsistent — `xdg_shell` (xdg-shell.xml:2) and `presentation_time` (presentation-time.xml:2)
  are unversioned grandfathered names, and `linux_dmabuf_v1` (linux-dmabuf-v1.xml:2) drops the
  `zwp` prefix its interfaces carry. We follow the modern form: protocol name matches the
  interface namespace and carries `_v1` (so `zext_toplevel_export_v1`, `zxr_shell_v2`).
- **Copyright block**: first child of `<protocol>`. Upstream embeds the full MIT license text
  (xdg-shell.xml:4-30); spatial-os files use `SPDX-License-Identifier: MIT` plus attribution
  instead, with **real names** — attribution lines name people or legal entities with years,
  never a bare collective. When a protocol continues prior work, stack lineage copyright lines
  the way upstream does: ext-workspace-v1.xml:3-6 credits Billington 2019 / Bozhinov 2020 /
  Brekenfeld 2022 across three generations, linux-drm-syncobj-v1.xml:4-7 credits
  Chromium/Intel/Collabora/Ser across five years. A `zxr-shell-v2` that continues wxrc's
  zxr-shell-v1 keeps the 2019 Status Research & Development GmbH line.
- **Protocol-level `<description>`**: optional upstream (absent from xdg-shell,
  presentation-time, ext-workspace; present in linux-drm-syncobj-v1.xml:29-56). **[picked]**
  Required here, because our files must carry the experimental disclaimer: state what the
  protocol does, then a phase warning modeled on syncobj's "Warning! The protocol described in
  this file is currently in the testing phase..." (linux-drm-syncobj-v1.xml:52-56). Experimental
  protocols must be clearly tagged (GOVERNANCE.md:108).
- **RFC 2119**: new protocol descriptions must use the lowercase keywords and include the
  RFC 2119 boilerplate paragraph (wayland-protocols README.md:143-156). **[picked]** None of the
  five studied upstream files actually contains the boilerplate (they predate the rule); the
  README says new protocols must, so we do.
- **Indentation**: 2 spaces. **[picked]** xdg-shell mixes tabs into description bodies;
  ext-workspace, dmabuf, and syncobj use spaces consistently. Spaces win.

## 3. Interface conventions

- **Naming**: `<namespace>_<thing>_v<major>` — the major-version suffix goes on every interface
  name, not just the protocol (README.md:134-141; `ext_workspace_handle_v1`,
  `wp_linux_drm_syncobj_timeline_v1`). Note the legacy `z` prefix (`zwp_linux_dmabuf_v1`,
  linux-dmabuf-v1.xml:27) marks the retired "unstable" policy (README.md:57-63) — our `zxr_`/
  `zext_` are namespace names in their own right, not that marker, and are never shortened.
- **Version discipline**: the interface `version` attribute is the highest revision the file
  defines (xdg-shell.xml:32 `version="7"`). Additions carry `since="N"` on the request/event/
  entry (xdg-shell.xml:370, :909); deprecations carry `deprecated-since="N"`
  (linux-dmabuf-v1.xml:111, :128). Group each revision under a `<!-- Version N additions -->`
  comment (xdg-shell.xml:368, linux-dmabuf-v1.xml:161). Existing opcodes, enum values, and
  argument lists are **never** renumbered or reordered; backward-incompatible change means a new
  major version file with all `since` attributes stripped and interface versions reset
  (README.md:170-182).
- **Summary style**: lowercase phrase, no trailing period — "create desktop-style surfaces"
  (xdg-shell.xml:33), "list and control workspaces" (ext-workspace-v1.xml:31), "set the acquire
  timeline point" (linux-drm-syncobj-v1.xml:181). No summary is a sentence fragment shorter than
  the element deserves, and none is a URL.
- **Full-description register**: declarative present tense describing the contract, with
  RFC 2119 "must/may/should" for the normative parts ("The client must call wl_surface.commit…",
  xdg-shell.xml:428-429; "Compositors must not send duplicate format + modifier pairs…",
  linux-dmabuf-v1.xml:586-588). No marketing adjectives, no rationale essays, no TODOs, no
  citations of repository-internal documents — design rationale lives in `docs/`, the XML
  states only the wire contract. Cross-references use `interface.member` form
  ("See xdg_wm_base.ping", xdg-shell.xml:98-99).

## 4. Requests, events, arguments, enums

- **Requests are imperative verbs**: `set_*` for state (xdg-shell.xml:157, :689), `get_*` for
  role/extension objects returning a `new_id` (xdg-shell.xml:76, linux-dmabuf-v1.xml:173),
  `create_*` for factories (xdg-shell.xml:67), plus plain verbs (`move`, `resize`, `grab`,
  `commit`, `activate`, `ack_configure`).
- **Events are nouns, state facts, or past participles**: `configure`, `close`, `done`,
  `removed`, `capabilities`, `created`/`failed` (linux-dmabuf-v1.xml:342, :354),
  `presented`/`discarded` (presentation-time.xml:200, :261), `repositioned` (xdg-shell.xml:1398).
- **Destructors**: every client-created object gets `<request name="destroy"
  type="destructor">`, conventionally the first or an early request. Its description states the
  fate of related objects ("Existing objects created by this object are not affected",
  presentation-time.xml:67-69; "wl_buffers... will remain valid", linux-dmabuf-v1.xml:94-96) and
  any ordering constraint with its error ("destroyed before children" → `defunct_surfaces`,
  xdg-shell.xml:61-63). Server-destroyed objects use destructor *events* (`finished`,
  ext-workspace-v1.xml:106; `presented`/`discarded`, presentation-time.xml:200, :261).
  **[picked]** `destroy`, never `release`, for new interfaces.
- **new_id args**: name the argument `id` and always set the `interface` attribute
  (xdg-shell.xml:73, :92); use a descriptive name only when one object flows through another
  (`params_id`, linux-dmabuf-v1.xml:107; `callback`, presentation-time.xml:86). A `new_id`
  in an *event* is how servers announce objects (ext-workspace-v1.xml:61, :74;
  linux-dmabuf-v1.xml:350).
- **fd args**: `type="fd"` with a summary naming the fd kind ("dmabuf fd",
  linux-dmabuf-v1.xml:258; "table file descriptor", :491); the description states lifetime and
  access rules ("must map the file descriptor in read-only private mode", :484).
- **64-bit values**: split into `_hi`/`_lo` uint pairs with the combination spelled out
  ("The 64-bit unsigned value combined from modifier_hi and modifier_lo…",
  linux-dmabuf-v1.xml:241-242; point_hi/point_lo, linux-drm-syncobj-v1.xml:185-186).
- **array args**: the description must define element width, layout, and endianness ("array of
  32-bit unsigned integers in native endianness", xdg-shell.xml:1163-1164; "Each index is a
  16-bit unsigned integer in native endianness", linux-dmabuf-v1.xml:573-575). An array with
  only a `summary="vec3"` is not a specification.
- **Enums**: every symbolic uint argument gets a real `<enum>` and the arg links it with
  `enum="name"` (xdg-shell.xml:209-210, :827). Entries are lowercase snake_case. Bitfields set
  `bitfield="true"` with power-of-two values (xdg-shell.xml:239, ext-workspace-v1.xml:308).
  **[picked]** decimal bit values per xdg-shell/ext-workspace/dmabuf; presentation-time's hex
  (presentation-time.xml:163-188) is the minority.
- **Error enums**: one enum named `error` per interface that can raise protocol errors
  (xdg-shell.xml:41-55, linux-dmabuf-v1.xml:207-227, linux-drm-syncobj-v1.xml:166-178). Entries
  name the violation; every raising site in prose names its entry, using the fixed idioms
  "…will result in a role error" (xdg-shell.xml:86), "raises an invalid_serial error"
  (xdg-shell.xml:573-574), "the invalid_parent protocol error is raised" (xdg-shell.xml:684).
  Distinguish fatal protocol errors from recoverable failure events — dmabuf sends `failed`
  for runtime import failure precisely because it is *not* a client bug
  (linux-dmabuf-v1.xml:323-326). **[picked]** entry values start at 0 with lowercase summaries;
  xdg_surface starting at 1 with capitalized summaries (xdg-shell.xml:461-474) is the outlier.
- **Capabilities**: advertise optional feature sets with a `bitfield` enum plus a `capabilities`
  event using the canonical two-paragraph phrasing — "clients should hide or disable the UI
  elements", "The compositor will ignore requests it doesn't support" — identical in
  xdg-shell.xml:1220-1243 and ext-workspace-v1.xml:147-163. Requests gated on a missing
  capability are ignored, not errors.

## 5. State-model language

- **Double-buffered state** uses the fixed sentence "Values set in this way are double-buffered,
  see wl_surface.commit" (xdg-shell.xml:960, :1000, :521) or "X is double-buffered state, and
  will be applied on the next wl_surface.commit request" (linux-drm-syncobj-v1.xml:188-191).
  The words "pending" and "committed"/"applied" are reserved for exactly this mechanism.
- **Atomic batches** end with a `done` event whose description carries the formula "This allows
  changes to … be seen as atomic, even if they happen via multiple events" — verbatim in both
  ext-workspace-v1.xml:90-104 and linux-dmabuf-v1.xml:464-472. Client-side atomic batches end
  with a `commit` *request* (ext-workspace-v1.xml:77-88). Sub-batches get their own terminator
  (`tranche_done`, linux-dmabuf-v1.xml:527-533).
- **Configure/ack handshakes** for compositor-suggested, client-acknowledged state: a
  `configure` event with a serial, an `ack_configure` request consuming it, and the sentence
  "The configured state should not be applied immediately" (xdg-shell.xml:1147-1148). Role
  events latch state that the final `configure` commits (xdg-shell.xml:600-608). Use this only
  where the client must render before the state takes effect; use plain `done` batching for
  observation-only interfaces.
- **Inert objects**: when the server withdraws the referent, say so with the fixed language of
  ext-workspace's `removed`: "the compositor will immediately consider the object inert. Any
  requests will be ignored except the destroy request", plus the guarantee "there won't be any
  more events referencing this object" (ext-workspace-v1.xml:199-210, :362-373; also
  linux-dmabuf-v1.xml:179-180). The client still sends `destroy` to finalize
  (ext-workspace-v1.xml:223-231).
- **Enter/leave symmetry**: membership is conveyed by paired `X_enter`/`X_leave` events, never
  by a replace-the-whole-set event (ext-workspace-v1.xml:166-196), and `done` covers the
  cross-object move case (:97-103).
- **No-guarantee requests**: requests that express intent the compositor may refuse say
  "There is no guarantee that…" (ext-workspace-v1.xml:390-393, :401, :409).

## 6. Timing and sync language

Our pacing and sync interfaces (`zext_export_pacing_v1`, future frame-timing work) must match
this register:

- **Clock domains** are established once, by an event, in named-clock terms: "This event tells
  the client in which clock domain the compositor interprets the timestamps… This clock is
  called the presentation clock" (presentation-time.xml:91-98), with the POSIX `clockid_t`
  anchor (:100-102) and stability requirements ("prefer a clock which does not jump and is not
  slewed", :115-117).
- **Timestamps** are `tv_sec_hi`/`tv_sec_lo`/`tv_nsec` uint triples with the validity bound
  "tv_nsec must be in [0, 999999999]" (presentation-time.xml:104-109).
- **Feedback is a strict dichotomy**: one-shot objects deliver exactly one of
  `presented`/`discarded`, both destructor events (presentation-time.xml:127-139, :200, :261),
  with `discarded` deliberately terse (:261-265) and all richness in `presented`.
- **Sync points** are phrased as obligations on a timeline: "Set the timeline point that must be
  signalled before the compositor may sample from the buffer" (linux-drm-syncobj-v1.xml:181-183)
  and "…that must be signalled by the compositor when it has finished its usage"
  (:211-214). Warn about ordering hazards concretely, as :221-234 does for out-of-order release
  signaling.
- **"Undefined" is a term of art**: unspecified behavior is declared, not implied — "the
  behavior is undefined" (xdg-shell.xml:386-387), "The delivery of wl_buffer.release events…
  becomes undefined" (linux-drm-syncobj-v1.xml:140-143), "It is undefined whether…"
  (linux-dmabuf-v1.xml:294-295), even "explicitly undefined" (:309-310). Never leave a gap
  silent.

## 7. What our ancestors did (and these rules forbid)

The lineage protocols were pioneering work under 2014–2022 constraints; this section exists so
v2 does not inherit their era's shortcuts, not to diminish them.

- **wxrc `zxr-shell-unstable-v1.xml`** (2019): protocol name `xr_shell_unstable_v1` does not
  match its `zxr_` interfaces (:2 — violates §2 name rule); protocol description is
  "TODO: Describe overall protocol here" (:27) and TODOs pepper the wire spec (:50, :128,
  :190-196 — §3 register rule); a description ends mid-sentence ("If a view global is removed
  and the client", :46-47); `zxr_shell_v1` has no description at all (:59); `zxr_surface_v1` and
  `zxr_composite_buffer_v1` have no destructors (§4); the composite buffer aggregates per-view
  2D buffers with no format/modifier negotiation and a single catch-all `invalid_buffer` error
  (:135-188 — §4 error-enum discipline, §6 sync absence); and it references a
  `zxr_view_v1.finished` event that is never defined (:168 — dangling cross-reference).
- **zwin** (2022): descriptions are GitHub URLs — `<description summary="http://github.com/
  zwin-project/…"/>` (zwin.xml:30, :37) — outsourcing the normative spec to a mutable webpage
  (§3: the XML *is* the spec); scalars are smuggled through untyped arrays
  (`type="array" summary="off_t"`, zwin.xml:58; "vec3", "uint64" — §4 array/64-bit rules);
  `zwin-gles-v32.xml` couples the wire protocol to one GL generation, baking GLenum constants
  into entry values (`fragment_shader value="0x8830"`, :92-93) and GL error semantics into the
  error enum (:30-32); `zwn_gl_base_technique.error` gives two entries the same value 0
  (:201-204 — scanner-visible bug); `configure`/`ack_configure` ship with no descriptions and
  "double buffered state" as an entire summary (zwin-shell.xml:63-74 — §5 language).
- **motorcar** (2014): no copyright element at all (motorcar.xml:2 — §2); no error enums, no
  versioning discipline, and no destructors outside `six_dof_pointer.release`; an *event* is
  named like a request (`request_size_3d`, :78 — §4 verb/noun split) and its sibling request
  copy-pastes the event's description (:87-88); booleans and enums travel as bare uints without
  `enum=` linkage (:26-27); matrices are float arrays with no endianness and the nonsensical
  unit "specified in meters" for a projection matrix (:128); descriptions carry typos
  ("copositor", "tradtional", :5-6) and single-line run-on paragraphs (:41-44).

## 8. spatial-os addenda

- **Namespace policy** (protocols/README.md "Namespace and governance posture"): `zext_`/`zxr_`
  are local experimental namespaces; nothing in this tree is an upstream protocol, and 2D
  protocol vocabulary is never silently given new wire meanings (ADR 0012 §4 rule). On proposal,
  interfaces are renamed to the upstream experimental `xx_` prefix (GOVERNANCE.md:76-80);
  `ext_` promotion requires two member ACKs plus open-source client and server implementations
  (GOVERNANCE.md:93-96).
- **Lineage attribution**: any protocol that continues prior art (zxr-shell-v2 over wxrc's v1,
  motorcar's concepts, zwin's) carries stacked copyright lines for the prior authors (§2), and
  its protocol description may name the predecessor protocol — but nothing else about the
  predecessor's style is inherited.
- **Budget-impact notes** belong in the protocol's *design document* under `docs/`, not in the
  XML: every design doc for a protocol here must state its frame-loop/latency budget impact.
  The XML stays a pure wire contract (§3 register rule).
- **Deviations in `zext-toplevel-export-v1.xml` as written** (recorded, deliberately not yet
  fixed while the draft iterates):
  1. ~~Attribution is "the spatial-os authors" (:4) — not a real name (§2).~~ *Fixed 2026-09-23:
     all zxr/zext files attribute Jarrad Hope (sole author), stacked over lineage lines where
     prior work is continued.*
  2. No RFC 2119 boilerplate (§2).
  3. Symbolic uints without `<enum>`s throughout: `node.role` "toplevel | subsurface | popup"
     (:154), `denied.reason` (:157-165), `fallback.kind` (:266), `regions.kind` (:254),
     `focus.kind` (:385), the manager `capabilities` bitmask in prose (:70-76), and
     `resume_move` as "0 or 1" (:119) (§4).
  4. No `error` enum on any interface, although prose implies protocol errors ("Fails
     (denied: no_active_move)", :59) (§4).
  5. Descriptions cite R-numbers and repo paths (:9-31, :41, :93) and contain a TODO
     (:180-184) (§3 register).
  6. Missing descriptions/summaries: `plane` (:218-224), `damage` (:234-239), all seven input
     requests (:328-371), and most args file-wide (§3/§4).
  7. Manager `destroy` description states no effect on child objects (:67) (§4 destructor rule).
  8. The atomic-batch boundary is an event named `commit` (:167) where upstream convention is
     `done` (§5).
  9. Seat identified by string `seat_name` (:63) rather than an object reference (§4).
  10. Buffers are uint `buffer_id` handles (:209, :228) rather than protocol objects — a design
      choice a wayland-protocols reviewer would challenge against the params-object pattern
      (linux-dmabuf-v1.xml:100-109).
