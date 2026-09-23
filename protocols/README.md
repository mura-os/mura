# protocols/

Home of spatial-os's Wayland protocol XMLs — the `zxr` / `zext` families. Declared in
[repo-structure.md](../docs/architecture/repo-structure.md).

## Contents

| File | What | Status |
|---|---|---|
| `zext-toplevel-export-v1.xml` | Per-toplevel zero-copy export/delegation between compositors (foreign 2D sessions → floating windows in zxr) | experimental draft — design in [foreign-session-integration.md](../docs/architecture/foreign-session-integration.md), requirements R1–R24 in [research/32](../docs/research/32-toplevel-export-prior-art.md), strategy in [ADR 0014](../docs/architecture/adr/0014-toplevel-delegation-protocol.md); 10 recorded [CONVENTIONS](CONVENTIONS.md) deviations to fix at next revision |
| `zxr-shell-v2.xml` | The 3D-client shell protocol: N views, typed colour+depth slots with explicit sync, atomic frame snapshots, ray/6DoF input | **drafted** (red-team in progress) — design in [zxr-shell-v2-composition.md](../docs/architecture/zxr-shell-v2-composition.md) §7, drafting brief in [research/08 Part 3](../docs/research/08-wxrc.md), ADR 0006 |
| `zxr-workspace-v1.xml` | Place kind/anchor/pose/bounds/preview/entry-presence + continuous transitions beside `ext-workspace-v1` | **drafted** (review in progress) — fields from [places-model.md §6](../docs/architecture/places-model.md) / ADR 0016 |
| `zxr-layer-anchoring-v1.xml` | Head/body/world frames + angular size + exclusive solid angle for layer surfaces | **drafted** (review in progress) — ADR 0012 §4.3 |
| `zext-a11y` (name reserved) | Spatial accessibility semantics for assistive clients (window poses/relations, place membership + currency, gaze/ray context, boundary state, privileged navigation verbs) | design-note stage — surface fixed in [spatial-a11y.md](../docs/architecture/spatial-a11y.md); carrier (zxr protocol vs AccessKit payload) deferred to Newton/AccessKit maturity |

## Namespace and governance posture

Per [research/32 §7](../docs/research/32-toplevel-export-prior-art.md) (wayland-protocols
GOVERNANCE.md verified):

- **`zext_`/`zxr_`** — spatial-os experimental namespaces, local to this tree. Nothing here is an
  upstream protocol, and 2D-protocol words are never silently given new wire meanings
  (ADR 0012 §4 rule).
- On upstream proposal, interfaces are renamed to the upstream experimental **`xx_`** prefix and
  submitted to `wayland-protocols`; promotion to **`ext_`** requires two member ACKs, an
  open-source client + server, and review (members include KWin, GTK/Mutter, Smithay/COSMIC,
  wlroots/Sway).
- The precedent is COSMIC's workspace protocol (private namespaced copy 2022 →
  `ext-workspace-v1` 2024-12-20 → COSMIC adoption 2025-02): a multi-year migration, planned for.

For `zext-toplevel-export-v1`, the upstreaming bar we set ourselves (ADR 0014): the zxr consumer +
a smithay reference producer + a KWin producer MR, with published interop tests (modifier
negotiation, out-of-order release, disconnect, popup reconstraint, late-frame reuse, grabs)
before proposing `xx_toplevel_export`.
