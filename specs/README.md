# specs/

Normative non-Wayland contracts — the peer of [`protocols/`](../protocols/README.md) for IPC
framings, storage formats, and D-Bus/PipeWire interfaces. Design docs (under
`docs/architecture/`) say *why*; these say *exactly what*. Every spec carries a budget-impact
statement ([overview.md](../docs/architecture/overview.md) invariant 9) and grounds its
vocabulary per the [docs README rules](../docs/README.md) (XDG sense stated; spatial terms in
XrSpace semantics).

| Spec | Contract | Design source | Status |
|---|---|---|---|
| `perception-intake.md` | Perception→compositor layer intake: dual-rate generation record, registration/image tables, GPU-safe reclamation, snapshot selection | [ADR 0008](../docs/architecture/adr/0008-perception-services-placement.md) (recast), perception backlog #5/#8 (dispositions in §9) | rev 2 (review absorbed) |
| `session-auth.md` | `mura-authd` lock conversation (nonced, batched), lock transition table, `--greeter` greetd-client mode | [ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md) | rev 2 (review absorbed) |
| `session-bootstrap.md` | **Rev 2 (D4 landed):** the session wrapper greetd execs is **uwsm** (`mura-session` → `uwsm start`), `mura-session.target` as a thin target, the environment contract in four classes (static / session-specific to the compositor unit only / compositor-created after readiness / dependent), seat acquisition inside a user unit, manager-correct lifetimes with `RestartMode=direct` crash restart, teardown-before-return, profile differences, the sway stand-in specifics (`uwsm finalize`); conformance items 1–7 VM-verified | [implementation-path.md §2 B6/B6a, §3c D4](../docs/architecture/implementation-path.md), [ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md), `session-auth.md §5` | draft rev 1 (D4 against sway; revised at G3) |
| `settings-schema.md` | Schema artifact from NixOS options, preference/state storage split, relocatable instance schemas, migrations, apply transactions | composition §7.3 constraint 9, [research/35](../docs/research/35-settings-config-models.md) | rev 2 (review absorbed) |
| `spatialcast-portal.md` | Frontend-patch-carried spatial source types + RGBD PipeWire profile (layout descriptor, per-view metadata) | [research/17 §9](../docs/research/17-sharing-capture-stack.md), [spatial-sharing.md](../docs/architecture/spatial-sharing.md) | rev 2 (review absorbed) |
| `toplevel-export-producer.md` | Producer behavioral conformance for `zspatial-toplevel-export-v1`: privilege, tree fidelity, buffer lifetime (release join), delegated local state (§2.8), pacing, input, revocation, detach/adopt + 24 interop tests | [ADR 0014](../docs/architecture/adr/0014-toplevel-delegation-protocol.md), [research/40](../docs/research/40-toplevel-export-producers.md); per-DE briefs in [producers/](../docs/architecture/producers/kwin.md) | rev 2 (KWin + Mutter persona reviews absorbed) |
| `place-document.md` | The state-sync place representation (restore record = mode-5 unit = store payload) | [places-model.md §7](../docs/architecture/places-model.md) | gated (after zxr-workspace fields settle) |
