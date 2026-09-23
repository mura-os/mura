# specs/

Normative non-Wayland contracts — the peer of [`protocols/`](../protocols/README.md) for IPC
framings, storage formats, and D-Bus/PipeWire interfaces. Design docs (under
`docs/architecture/`) say *why*; these say *exactly what*. Every spec carries a budget-impact
statement ([overview.md](../docs/architecture/overview.md) invariant 9) and grounds its
vocabulary per the [docs README rules](../docs/README.md) (XDG sense stated; spatial terms in
XrSpace semantics).

| Spec | Contract | Design source | Status |
|---|---|---|---|
| `perception-intake.md` | Perception→compositor layer intake: packet, sync/release, snapshot selection, placement bindings | [ADR 0008](../docs/architecture/adr/0008-perception-services-placement.md) (recast), perception backlog #5/#8 | drafted (review in progress) |
| `session-auth.md` | `spatial-authd` socketpair framing, lock state-machine events, `--greeter` restricted mode | [ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md) | drafted (review in progress) |
| `settings-schema.md` | Schema artifact emitted from NixOS options, strata storage layout, notification bus | composition §7.3 constraint 9, [research/35](../docs/research/35-settings-config-models.md) | drafted (review in progress) |
| `spatialcast-portal.md` | Portal source-type extensions + SPA custom metadata (per-view P·V, depth encoding) | [research/17 §9](../docs/research/17-sharing-capture-stack.md), [spatial-sharing.md](../docs/architecture/spatial-sharing.md) | drafted (review in progress) |
| `place-document.md` | The state-sync place representation (restore record = mode-5 unit = store payload) | [places-model.md §7](../docs/architecture/places-model.md) | gated (after zxr-workspace fields settle) |
