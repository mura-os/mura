# ADR 0017: First-run provisioning — the fixed-owner account model, PIN enrollment, and OOBE placement

**Status:** accepted
**Date:** 2026-09-23
**Context sources:** [first-run-onboarding.md](../first-run-onboarding.md) (the design this
decides), [research/12](../../research/12-lock-screens-and-appliance-login.md) §6 (PIN options,
Quest precedents), [research/11](../../research/11-display-managers-greeters.md) (greetd),
[ADR 0007](0007-session-greeter-lock.md) (whose PIN-enrollment open question this closes),
[specs/settings-schema.md](../../../specs/settings-schema.md) (storage strata),
[implementation-path.md](../implementation-path.md) (the F-track).
**Budget impact** (overview invariant 9): one-shot boot/setup work plus one small root daemon
active only during enrollment conversations; nothing on the frame path.

## Context

A shipping image must get from "flashed" to "authenticatable" without imperative system
mutation, on an A/B-updated, image-based NixOS where the store is world-readable and per-unit
state is sacred (invariant 4). ADR 0007 assumed the owner account and credential exist; nothing
created them. The boot-to-desktop coverage review (implementation-path rev 2) made the gap
blocking.

## Decision

1. **Fixed declared owner account — scoped to the appliance profile** (amended by
   [ADR 0018](0018-multi-user-accounts.md)). On the appliance profile: `users.mutableUsers =
   false`; the `owner` account is declared in the module system and exists in every image;
   "setting up a user" writes per-unit state (PIN hash, calibration, preferences) — the account
   database is never mutated at runtime. The Steam Deck precedent (fixed `deck` user; OOBE never
   creates an account) is that profile's model. On the **multi-user profile**, ADR 0018 makes
   the account database mutable through exactly one authority (provisiond → userborn's persisted
   files); multi-account and guest are designed there, not here.
2. **PIN = doc 12 option (b), ratified.** A dedicated `pam_spatial_pin` module verifies an
   argon2-hashed PIN stored in the `enrollment/` state class — the device-unlock credential is
   layered above the account password (Android/visionOS layering), and the account password
   stays strong. **PAM wiring:** `pam_spatial_pin` is wired into
   `security.pam.services.spatial-lock` only; greetd's login stack stays standard
   account-password, with owner-password-is-PIN (doc 12 option (a)) recorded as the appliance
   bridge until `pam_spatial_pin` ships. Both stacks are declared through NixOS modules, never
   hand-edited. This closes ADR 0007's open question.
3. **OOBE placement: dispatcher-gated `zxr --oobe`, split from its authority.** greetd's
   `default_session` (multi-user) / `initial_session` (appliance) runs a dispatcher that selects
   `zxr --oobe` while provisioning is incomplete, `zxr --greeter` otherwise (greetd cannot
   select sessions from runtime state; a NixOS option cannot change post-evaluation). The
   dispatcher *executes as greetd's session user* and therefore reads the **non-secret
   `/run/spatial/provisioned` flag** a boot-time root unit publishes — never the root-0700
   marker itself; continuation is re-dispatch on multi-user and launch-wait-exec on the
   appliance (first-run-onboarding §4.1 records both semantics). The OOBE is an **unprivileged UI**; every privileged write goes through
   **`spatial-provisiond`** (root, private socket, spatial-authd shape), which owns PIN-hash
   writes, device keys, and the root-owned **transactionally committed marker**. Appliance MVP:
   autologin + the same wizard as first session content.
4. **The marker on `/persist` is the first-run authority, not `ConditionFirstBoot`** (a fresh
   A/B root slot resembles first boot); `ConditionFirstBoot` is reserved for slot-local work.
   machine-id persists across updates and **rotates on factory reset**.
5. **Secrets are never Nix option values.** PIN hashes, Wi-Fi credentials, and device keys
   exist only as provisiond-written runtime state under protected persistent storage — the
   store is world-readable.
6. **Factory reset is the class-wise inverse** (first-run-onboarding §6): wipe `enrollment/` +
   `/home`, rotate machine-id, preserve `factory/` + `identity/`. Forgotten-PIN exhausts into
   reset (Quest precedent) — never a root shell.

## Alternatives considered

- **Imperative user creation at OOBE** (`mutableUsers = true`): rejected — mutates state the
  image model wants immutable, complicates A/B and reset, and buys nothing the fixed-account
  model lacks.
- **systemd-homed** (portable encrypted per-user homes): rejected for v1 — LUKS-per-home and
  record-signing complexity on a fanless single-owner appliance, for portability no target
  device uses. Revisit with the multi-user profile if real multi-account demand appears.
- **OOBE with direct write access** (no provisiond): rejected — the wizard UI is the largest
  pre-auth attack surface; a UI that can mint credentials or reopen enrollment by touching the
  marker collapses the privilege boundary the greeter design (session-auth §1) established.
- **`ConditionFirstBoot` as the first-run signal**: rejected — wrong across A/B slot
  replacement in both directions.

## Consequences

- The F-track (F1/F2) enters [implementation-path.md](../implementation-path.md) §2; multi-user
  G2 requires enrollment (or the VM fixture); the appliance MVP requires neither.
- New components for the registry: the dispatcher wrapper, `spatial-provisiond`, the OOBE mode,
  F1 provisioning units; `pam_spatial_pin` moves to specified.
- The contract grows `spatial.xr.session.provisioning.*`
  ([lib/contract](../../../lib/contract/default.nix)).
- ADR 0007's open-questions list drops PIN storage/enrollment (pointer added).
- The recovery environment (where factory reset runs) is a named open item
  (first-run-onboarding §9), joined to each family's recovery story.
