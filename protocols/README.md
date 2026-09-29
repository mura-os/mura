# protocols/

Home of Mura's Wayland protocol XMLs — the `zxr` / `zspatial` families. Declared in
[repo-structure.md](../docs/architecture/repo-structure.md).

## Contents

| File | What | Status |
|---|---|---|
| `zspatial-toplevel-export-v1.xml` | Per-toplevel zero-copy export/delegation between compositors (foreign 2D sessions → floating windows in zxr) | **rev 3** (KWin-persona red-team absorbed: restacked event replaces illegal new_id reuse, mark_presented/mark_discarded close the pacing loop, buffers exempt from denied-inert, request_size/set_preferred_scale/close added, input gains leave/enter/keysym/touch_cancel + seat binding, timeline objects replace per-message fds, renamed off `zext_`) — design in [foreign-session-integration.md](../docs/architecture/foreign-session-integration.md), requirements R1–R24 in [research/32](../docs/research/32-toplevel-export-prior-art.md), strategy in [ADR 0014](../docs/architecture/adr/0014-toplevel-delegation-protocol.md); producer conformance in [specs/toplevel-export-producer.md](../specs/toplevel-export-producer.md) with per-compositor briefs ([kwin](../docs/architecture/producers/kwin.md), [mutter](../docs/architecture/producers/mutter.md)) |
| `zxr-shell-v2.xml` | The 3D-client shell protocol: N views, typed colour+depth slots with explicit sync, atomic frame snapshots, ray/6DoF input | **rev 2** (33-finding red-team absorbed) — design in [zxr-shell-v2-composition.md](../docs/architecture/zxr-shell-v2-composition.md) §7, drafting brief in [research/08 Part 3](../docs/research/08-wxrc.md), ADR 0006 |
| `zxr-workspace-v1.xml` | Frame identity for groups + place kind/anchor/pose/bounds/currency/preview + batched transitions beside `ext-workspace-v1` | **rev 2** (review absorbed) — fields from [places-model.md §6](../docs/architecture/places-model.md) / ADR 0016 |
| `zxr-window-management-v1.xml` | The bounded window-management seam (ADR 0012 amendment (c)): one manager client; river's manage/render double-buffered sequences; places and windows announced as objects; per-window place assignment, pose (clamped to compositor `limits`), proposed dimensions, hide/show, flags, maximize/fullscreen, focus backed by an `interaction` serial, exclusive grant; per-place engine + one-shot arrange + emphasis; client requests re-emitted as events | **rev 1, served** (2026-09-27: `place` event replaces `get_place`, `state`/`assign` carry the place object; the compositor's disconnect contract stated) — served by `pkgs/zxr/src/policy/seam.rs` (wayland-scanner codegen), proven by `zxr-test-manager` (spec §12 gate 7); design in [window-workspace-management.md §11](../docs/architecture/window-workspace-management.md), evidence in [research/64 §11](../docs/research/64-window-workspace-management-from-comparables.md); continues river's `river-window-management-v1` (Isaac Freund 2024, MIT) with spatial verbs; `emphasis` and `exclusive` are declared but unadvertised until M2 |
| `zxr-layer-anchoring-v1.xml` | Head/hand/world/docked frames (rev 3: `body` withdrawn — world is seeded in front of the wearer and re-seated on recenter) + angular size/pose + exclusive angular bands for layer surfaces | **rev 2** (redesigned per review) — ADR 0012 §4.3, frames aligned with [places-model.md](../docs/architecture/places-model.md) |
| `zspatial-a11y` (name reserved) | Spatial accessibility semantics for assistive clients (window poses/relations, place membership + currency, gaze/ray context, boundary state, privileged navigation verbs) | design-note stage — surface fixed in [spatial-a11y.md](../docs/architecture/spatial-a11y.md); carrier (zxr protocol vs AccessKit payload) deferred to Newton/AccessKit maturity |

## Namespace and governance posture

Per [research/32 §7](../docs/research/32-toplevel-export-prior-art.md) (wayland-protocols
GOVERNANCE.md verified):

- **`zspatial_`/`zxr_`** — Mura experimental namespaces, local to this tree (the shell
  family was renamed off `zext_` at toplevel-export rev 3: `zext_` read as a claim on the
  wayland-protocols `ext` namespace plus the retired `z` unstable prefix — a red-team finding
  against our own research/32 §7 recommendation). Nothing here is an
  upstream protocol, and 2D-protocol words are never silently given new wire meanings
  (ADR 0012 §4 rule).
- On upstream proposal, interfaces are renamed to the upstream experimental **`xx_`** prefix and
  submitted to `wayland-protocols`; promotion to **`ext_`** requires two member ACKs, an
  open-source client + server, and review (members include KWin, GTK/Mutter, Smithay/COSMIC,
  wlroots/Sway).
- The precedent is COSMIC's workspace protocol (private namespaced copy 2022 →
  `ext-workspace-v1` 2024-12-20 → COSMIC adoption 2025-02): a multi-year migration, planned for.

For `zspatial-toplevel-export-v1`, the upstreaming bar we set ourselves (ADR 0014): the zxr consumer +
a smithay reference producer + a KWin producer MR, with published interop tests (modifier
negotiation, out-of-order release, disconnect, popup reconstraint, late-frame reuse, grabs)
before proposing `xx_toplevel_export`.
