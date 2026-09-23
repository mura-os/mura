# specs/settings-schema: the generated schema artifact, strata, and reconciliation

**Status:** draft rev 2 (specification workstream; rev 1 findings from the GSettings/NixOS-persona
review absorbed — preference/state storage split, relocatable instance schemas, typed migrations,
apply transactions).
**Design sources:** composition §7.3 **constraint 9**,
[research/35](../docs/research/35-settings-config-models.md) §7–§8.
**Grounding:** XDG **Base Directory** spec (CDG sense): *preferences* live under
`$XDG_CONFIG_HOME`, *remembered operational state* under `$XDG_STATE_HOME` — the distinction is
load-bearing (§2). Schema semantics follow GSettings' capability set, including its relocatable
schemas, without the compiled-blob mechanism.
**Budget impact** (inv. 9): build-time generation; runtime reads at startup + per-key D-Bus
notifications; watchers event-driven, no polling.

## 1. The schema artifact

Emitted from evaluated NixOS options (`nixosOptionsDoc`-shaped projection) as
`system.build.muraSettingsSchema` → `/etc/mura/settings-schema.json`. Constraint 9: no
consumer compiles in an independent default; a missing/corrupt artifact is an explicit failure
mode.

Per-key record:

```text
id            dotted key within its schema ("shell.follow.startFovDeg")
type          bool | int | double | string | enum | list<...>
constraints   enum values / numeric range (optional)
default       the evaluated build default (post Nix priority resolution)
class         preference | state          (§2 — chooses the storage root)
stratum       build-fact | per-unit | per-user | session
ownership     declarative | runtime       (§3)
locked        bool (appliance lockdown; locked ⇒ writes rejected)
apply         live | reload:<unit> | restart:<unit> | reboot   (§6)
description   from the option declaration
schemaVersion integer, per schema (§5)
```

Build facts are never writable keys; they appear at most as `stratum=build-fact, locked=true`.

### 1.1 Relocatable (instance) schemas

Dynamic namespaces are first-class, GSettings-relocatable-style: a **schema template** (e.g.
`places.entry`, keys `enabled`, `launch`, `summon`, …) is declared once in Nix and **instantiated
at runtime paths**: `places.entry:<place_id>` where `<place_id>` is the stable id from
[places-model.md](../docs/architecture/places-model.md), percent-escaped. Instance lifecycle:
instances are created by the owning component (the places model, via the daemon) — a `Set` on a
non-existent instance of a declared template creates it; a `Set` on a key of no declared schema
or template is `ERR_UNKNOWN_KEY`. Instance deletion is explicit (`DeleteInstance`); orphaned
instances (referent place gone) are retained until a GC policy owned by the referent's component
removes them, and are enumerable (`ListInstances(template)`) so migrations cover them.

## 2. Classes, strata, and storage layout

| class / stratum | Store | Written by | Survives |
|---|---|---|---|
| build-fact | the artifact | nixos-rebuild | generations |
| preference, per-unit | `/var/lib/mura/settings/config/` | daemon (polkit-gated for privileged keys) | reboots, rebuilds, users |
| preference, per-user | `$XDG_CONFIG_HOME/mura/settings/` | daemon for the session | reboots, rebuilds |
| state, per-unit | `/var/lib/mura/settings/state/` | daemon | reboots, rebuilds |
| state, per-user | `$XDG_STATE_HOME/mura/settings/` | daemon | reboots, rebuilds |
| session | daemon memory | grant holders | nothing |

Render scale, follow-mode knobs, passthrough policy, entry grants are **preferences**
(config-home). Remembered operational values (last dock layout position, transient tallies) are
**state** (state-home). Every exported key declares its class in Nix; the review's rule stands:
intent lives in config, memory lives in state.

Stores are sparse per-(schema, instance) files: versioned header
`{schema, instance?, schemaVersion, generation}` + explicit key-value entries only; atomic
rename writes. Absent key ⇒ generated default (copying defaults into stores is forbidden).
Resolution: `session > per-user > per-unit > default`; locked keys resolve to the generated value.

## 3. Ownership (unchanged from rev 1, sharpened)

- **`declarative`**: Nix owns the value; runtime writes rejected (`ERR_DECLARATIVE`).
- **`runtime`**: Nix owns the default; explicit values survive rebuilds; `Reset` reveals the
  current default.

**`Set` always creates the stratum override, even when equal to the resolved lower-layer value**
— equality is not absence of intent (pinning an equal value protects against future default
changes). `Changed` is emitted when the *effective value or its provenance* changes; a `Set`
that changes neither (same stratum, same value) emits nothing. `Reset` is the only way back to
following defaults.

## 4. Reconciliation at generation switch

For key `k` (`D_old/D_new` defaults, optional explicit `U`):

1. `U` absent ⇒ effective value advances silently.
2. `U` present, valid ⇒ survives (`runtime` keys).
3. `Reset` ⇒ reveal `D_new`.
4. Newly `locked`/`declarative` ⇒ `U` quarantined (§4.1), enforced value applies, one `Changed`.
5. `U` invalid under the new schema ⇒ run declared migrations (§5); if none apply, quarantine +
   default. Never silent coercion; never boot failure.
6. Consumers receive `GenerationChanged` and re-resolve; `apply` actions run per §6.

### 4.1 Quarantine (per-record, non-destructive)

A quarantine record is `(schema, instance, key, sourceGeneration, reason, serializedValue)`,
stored beside the live file; the live file is atomically rewritten **without only the rejected
keys** — sibling keys stay writable. Operations: `ListQuarantine`, `RestoreQuarantined`
(re-validated against the current schema), `DropQuarantined`. **Downgrade rule:** stores and
quarantine records are generation-tagged; rolling back to a generation whose schema accepts a
quarantined record automatically remounts it — the only copy of a user value is never destroyed
by a version move in either direction.

## 5. Versioning and migration (typed, executable, auditable)

`schemaVersion` per schema; bumped on incompatible change. Migrations are declared in Nix as
**typed operations** serialized into the artifact — `rename(from,to)`, `delete(key)`,
`enum-map(key, {old:new})`, `scale(key, factor, clamp)`, `split(key, {targets})`,
`merge({sources}, key, fn ∈ fixed set)` — with explicit version edges `(from,to)`. The daemon
executes only artifact-declared operations (no out-of-band migration code), selects the unique
shortest edge chain deterministically, and refuses ambiguous graphs at generation *build* time
(a Nix assertion — bad migration graphs never ship). Instance schemas migrate per instance,
enumerated via §1.1. Golden tests (upgrade and downgrade fixtures) are emitted beside the
artifact and run in CI. Old schema + migration artifacts referenced by existing stores are GC
roots until no store references their generation.

## 6. Apply transactions

`Set`/`Reset` on a key with `apply ≠ live` opens an **apply transaction**: the write lands
(durable) with status `pending`; the privileged apply agent (the daemon's system half — the only
actor allowed to reload/restart units at runtime) executes the action and reports
`applied` or `failed{message}` (value stays, effect pending until retry/boot — never silently
rolled back). Status is queryable (`GetApplyStatus`) and signalled (`ApplyChanged`).
Rebuild-time application remains the activation script's (standard NixOS switch).

## 7. Lockdown

Locks are schema facts generated from the profile module. **Consumers of security-relevant keys
resolve the effective value from the authenticated artifact themselves** (path-pinned,
root-owned `/etc/mura/settings-schema.json`, its store hash listed in the system closure):
a compromised daemon can lie on the bus but cannot alter locked effective values for consumers
that follow this rule; the daemon is convenience, not authority, for locked keys.

## 8. The bus interface

`org.mura.Settings1` (session bus; system-scoped writes brokered to the daemon's system half
with polkit):

- `Get(s key) → (v value, s provenance)`; `Set(s key, v value)`; `Reset(s key)`;
  `List(s prefix)`; `ListInstances(s template)`; `DeleteInstance(s instance)`;
  `ListQuarantine()`, `RestoreQuarantined(...)`, `DropQuarantined(...)`; `GetApplyStatus(s key)`.
- Errors: `ERR_LOCKED`, `ERR_DECLARATIVE`, `ERR_TYPE`, `ERR_RANGE`, `ERR_UNKNOWN_KEY`,
  `ERR_UNKNOWN_INSTANCE`.
- Signals: `Changed(s key, v value, s provenance)` — every accepted effective-value-or-provenance
  change is durable; *notifications* may coalesce per event-loop turn per key, and the final
  signal of a turn carries the final value and provenance. `GenerationChanged(u generation)`;
  `ApplyChanged(s key, s status)`.

zxr consumes the daemon (or watches files) and exposes only its own narrow HMD protocol
(ADR 0012 §4.5); shell components use the bus; entry-policy grants are `places.entry:<place_id>`
instances (§1.1) with `runtime` ownership and `class=preference`.

## 9. Conformance checklist

1. Rebuild default-change with/without user value (advance vs survive).
2. `Set` equal to default ⇒ override created, provenance `Changed` emitted; `Reset` returns to
   default-following.
3. Declarative write ⇒ `ERR_DECLARATIVE`, no store write.
4. Kill daemon mid-write ⇒ no torn file; identical resolution on restart.
5. Downgrade with newer-version stores ⇒ quarantine; roll forward again ⇒ automatic remount.
6. Template instance: `Set` on new `places.entry:<id>` creates it; migration enumerates all
   instances; orphan GC only via the owning component.
7. `reload:<unit>` key: write lands durable, `pending` → `applied`/`failed` observable; failure
   never reverts the value.
8. Locked-key consumer resolves from the artifact even when the daemon lies (fault-injection).

## 10. Open items

The daemon's process design (this spec is its contract); polkit action inventory (with the
polkit-agent design); session-stratum need at v1; quarantine UX surfaces.
