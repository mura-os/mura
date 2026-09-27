# specs/settings-schema: the generated schema artifact, strata, and reconciliation

**Status:** rev 4.1 (2026-09-27) — rev 4 + §6: the compositor is an in-process consumer of the store (research/73 Q7 ruled), not a bus client; rev 4 (2026-09-27) — rev 3 + the per-key axis renamed from `ownership = declarative | runtime` to **`mutability = mutable | immutable`** (§3): the old names implied the wrong thing in both directions — both kinds are declared in Nix, and a `mutable` key is not runtime-only (Nix owns its default, `Reset` returns to it). The words are the comparables' own: NixOS `users.mutableUsers`, KConfig's `[$i]` immutable entries. Field, values, the bus error (`ERR_IMMUTABLE`) and the artifact key change together; per-user stores carry no ownership, so nothing migrates. Rev 3 (2026-09-25) — the rev 2 contract re-derived from the stores' source under
AGENTS rules 7/8 ([research/58](../docs/research/58-settings-stores-from-comparables.md)).
Kept, with comparables: the compiled-schema artifact, sparse XDG stores with a state root,
Set-always-writes/Reset/provenance, relocatable instances, locks as system-layer facts, a single
session writer with per-key signals, declared migrations. Removed, having no comparable: the
polkit-gated `per-unit preference` row and the daemon's privileged "system half", per-record
quarantine with generation remount, apply transactions, the typed migration DSL in the artifact,
the preference `session` stratum. Added: `stratum = device` (ruled shape C, research/58 §13.1),
additive migrations (NixOS's rollback constraint), `invalid` provenance.
**Design sources:** composition §7.3 **constraint 9**, [research/35](../docs/research/35-settings-config-models.md)
§7–§8, [research/58](../docs/research/58-settings-stores-from-comparables.md).
**Grounding:** XDG **Base Directory** (CDG sense): *preferences* under `$XDG_CONFIG_HOME`,
*remembered operational state* under `$XDG_STATE_HOME` (§2). Schema semantics follow GSettings'
(compiled schema, sparse user values, relocatable schemas, locks) without the compiled-blob
mechanism; the device stratum follows snapd's (one API, per-key handlers).
**Budget impact** (inv. 9): build-time generation; one small daemon per session, D-Bus-activated;
reads at startup + per-key notifications; no polling. The process is
[specs/settings-daemon.md](settings-daemon.md).

## 1. The schema artifact

Emitted from evaluated NixOS options as `system.build.muraSettingsSchema` →
`/etc/mura/settings-schema.json` (root-owned, path-pinned, in the system closure). Constraint 9:
**no consumer compiles in an independent default**; a missing or corrupt artifact is an explicit
failure (GSettings aborts on a missing schema for the same reason). The artifact is the compiled
schema in GSettings' sense; `lib/settings` is the compiler.

Per-key record:

```text
id            "<schema>.<key>"            e.g. "xr.passthrough.latencyMode"
schema        the schema (file) the key belongs to
key           the key within the schema
option        the NixOS option path it was generated from
type          bool | int | double | string | enum
values        enum values (type = enum)
range         { min, max } (int/double; optional)
default       the evaluated build default (post Nix priority resolution)
class         preference | state                 (§2 — the storage root)
stratum       build-fact | per-user | device     (§2)
mutability    mutable | immutable                (§3)
locked        bool                               (§7; locked ⇒ writes rejected)
apply         live | restart:<unit> | relogin | reboot   (§6 — a label, not a mechanism)
description   from the option declaration
schemaVersion integer, per schema (§5)
```

Build facts are never writable keys; they appear as `stratum = build-fact, locked = true` only
when a consumer needs to read them through the same bus.

### 1.1 Relocatable (instance) schemas

Dynamic namespaces are GSettings-relocatable: a **schema template** (e.g. `places.entry`, keys
`enabled`, `launch`, `summon`, …) is declared once in Nix and **instantiated at runtime ids**:
`places.entry:<place_id>` (the stable id from [places-model.md](../docs/architecture/places-model.md),
percent-escaped). A `Set` on a non-existent instance of a declared template creates it; a `Set`
on a key of no declared schema or template is `ERR_UNKNOWN_KEY`. Deletion is explicit
(`DeleteInstance`); instances are enumerable (`ListInstances(template)`) so migrations cover
them. **No garbage collection** (GSettings has none): orphaned instances stay until the referent's
component deletes them.

## 2. Classes, strata, and storage layout

| class / stratum | Store | Written by | Survives |
|---|---|---|---|
| build-fact | the artifact | nixos-rebuild | generations |
| preference, per-user | `$XDG_CONFIG_HOME/mura/settings/` | `mura-settingsd` (session) | reboots, rebuilds |
| state, per-user | `$XDG_STATE_HOME/mura/settings/` | `mura-settingsd` (session) | reboots, rebuilds |
| preference / state, device | the owning hardware or daemon; `/var/lib/mura/settings/` only where nothing else owns the value | `mura-settingsd --system` (root, system bus; §2.1) | reboots, rebuilds, users |
| state, session *(reserved)* | `$XDG_RUNTIME_DIR/mura/settings/` | components, never committed (UCI's `-P` semantics) | nothing |

Render scale, follow-mode knobs, passthrough policy, entry grants are **preferences**.
Remembered operational values (last dock layout position, transient tallies) are **state**.
Intent lives in config, memory lives in state. There is **no device-wide preference**: a value
either belongs to one user (per-user) or to the device (device) — dconf's system layer is never
writable at runtime and neither is Mura's build layer; what a session may change device-wide is
a *device* key (§2.1).

Stores are sparse per-(schema, instance) JSON files: header `{schema, instance?, schemaVersion,
generation}` + explicit key-value entries only; atomic rename writes. Absent key ⇒ generated
default (copying defaults into stores is forbidden). Resolution: `per-user > default` for
per-user keys; a device key's value is whatever its owner reports; locked keys resolve to the
generated value.

### 2.1 The device stratum (ruled shape C, research/58 §13.1)

Keys with `stratum = device` are the appliance comparables' system configuration (snapd's
`system.*`, steamos-manager's typed features): device-wide, runtime-changeable, owned by hardware
or a domain daemon, never one user's preference. They are served by **the same crate in system
mode** on the system bus as root, through the same `Get`/`Set`/`Changed` keyed by artifact id,
with a **per-key handler** that validates against the device contract (`mura.hardware.*`) and
applies — to sysfs, a unit, or by calling the owning freedesktop daemon (`timedate1`,
`hostname1`, `locale1`, accountsservice, NetworkManager), exactly as snapd's `system.timezone`
calls `timedatectl`. Authorization is polkit per action, `auth_admin_keep` by default, wheel
members in active local sessions granted where a comparable grants (the `50-mura-timedate.rules`
pattern). Nothing is proxied through the session daemon; a client dispatches on `stratum`. The
system mode exists **only when a target declares a device key** (none does today); the artifact
reserves the stratum. Handlers for keys with an upstream owner are calls, not stores.

## 3. Mutability

Every key is declared in Nix. The axis is whether Nix owns the key's *value* or only its
*default* — NixOS's `users.mutableUsers`, KConfig's `[$i]` immutable entries:

- **`immutable`** (the default for every exported option): Nix owns the value; runtime writes
  rejected with `ERR_IMMUTABLE`; the switch reasserts it. NixOS's own shape (`time.timeZone`
  set; `users.users` declared with `mutableUsers = false`). For policy and security-relevant
  keys, where refusing the write is the point.
- **`mutable`** (explicit opt-in on the option): Nix owns the *default* — set it in the
  configuration and it is the default; the wearer's explicit value survives rebuilds; `Reset`
  reveals the current default. NixOS's `time.timeZone = null` / `mutableUsers = true`; Plasma's
  kconfig on NixOS. Every per-user *preference* is `mutable` (AGENTS.md rule 3: offer choices).

`immutable` is the key's design; **`locked`** (§7) is this image's policy on a `mutable` key — the
same refusal with provenance `locked`, generated from the profile module, reversible by the
administrator without changing the key. Rev 3 called this axis `ownership = declarative | runtime`;
renamed in rev 4 because both kinds are declarative and `runtime` read as runtime-only.

**`Set` always creates the override, even when equal to the resolved default** — equality is not
absence of intent (GSettings writes it; `g_settings_get_user_value` tells the two apart; KConfig
documents the opposite and its cost). `Changed` is emitted when the *effective value or its
provenance* changes; a `Set` that changes neither emits nothing. `Reset` removes the override and
is the only way back to following defaults. Provenance ∈ `default | user | device | locked |
invalid`.

## 4. Reconciliation at generation switch

For key `k` (`D_old/D_new` defaults, optional explicit `U`):

1. `U` absent ⇒ effective value advances silently (dconf; NixOS `dconf update`).
2. `U` present, valid ⇒ survives (`mutable` keys).
3. `Reset` ⇒ reveal `D_new`.
4. Newly `locked`/`immutable` ⇒ `U` is ignored, the enforced value applies, one `Changed`; the
   stored value is **left in the file** (it is the user's only copy; an older generation reads it
   again).
5. `U` invalid under the new schema (type, range, enum) ⇒ run declared migrations (§5); if none
   apply, the key resolves to the default with provenance **`invalid`** and the stored value is
   left in the file. Never silent coercion (KConfig clamps; every other store defaults), never a
   rewrite, never boot failure. A UI shows "ignored: out of range" from the provenance.
6. Consumers receive `GenerationChanged` and re-resolve.

## 5. Versioning and migration

`schemaVersion` per schema, declared in Nix; bumped on incompatible change. Migrations are
**numbered Rust functions in the daemon**, keyed by `(schema, fromVersion)`, run once when a
store's header version is below the schema's, recorded by rewriting the header — kconf_update,
snapd's patches and Android's upgrade steps, none of which ship a migration language (the KF5
DSL was retired). **Migrations are additive and non-destructive:** a rename writes the new key
beside the old, a transform writes beside, nothing is deleted or overwritten — NixOS's rollback
constraint (`mkStateRevisionOption`'s warning that migrated state must be reversed for an older
generation) is met by never making the old keys unreadable. Instance schemas migrate per
instance, enumerated via §1.1. Old keys are removed only by an explicit user `Reset` or by a
later migration declared after every shipped generation can read the new key.

## 6. Applying a change

`apply` is a **label** in the artifact — `live`, `restart:<unit>`, `relogin`, `reboot` — telling
a UI what a key needs (the KCM's "takes effect after restart"). The daemon signals; **the owner
of the effect applies** (the compositor re-resolves on the store's change — it links the daemon's engine and watches the store directory rather than holding a bus connection, ruled 2026-09-27, research/73 Q7, zxr-core rev 3.8 §8; a bus client would reload on `Changed`; a domain daemon reloads itself as
`localed` reloads PID 1; the switch restarts units whose definitions changed; a device key's
handler applies inside the system mode). Nothing tracks pending/applied status and nothing in the
settings daemon restarts other units — no settings store does. Device keys whose effect can hurt
(a TDP, a network change) get snapd's validate-all/apply-all/commit or LuCI's confirm-or-rollback
*inside their handler*, decided at the rung that brings them.

## 7. Lockdown

Locks are schema facts generated from the profile module (NixOS's `programs.dconf.*.locks`
shape). **Consumers of security-relevant keys resolve the effective value from the authenticated
artifact themselves** (path-pinned, root-owned `/etc/mura/settings-schema.json`, in the closure):
a compromised daemon can lie on the bus but cannot alter locked effective values for consumers
that follow this rule; the daemon is convenience, not authority, for locked keys. The UI shows a
locked key as read-only from `locked` and `Get`'s `locked` provenance (`g_settings_is_writable`).

## 8. The bus interface

`org.mura.Settings1` — the session bus for `per-user` keys (`mura-settingsd`), the system bus
for `device` keys (`mura-settingsd --system`); the same interface on both:

- `Get(s key) → (v value, s provenance)`; `Set(s key, v value)`; `Reset(s key)`;
  `List(s prefix) → a(s key, v value, s provenance)`; `ListInstances(s template) → as`;
  `DeleteInstance(s instance)`; `GetGeneration() → u`.
- Errors: `ERR_LOCKED`, `ERR_IMMUTABLE`, `ERR_TYPE`, `ERR_RANGE`, `ERR_UNKNOWN_KEY`,
  `ERR_UNKNOWN_INSTANCE`, `ERR_WRONG_BUS` (a per-user key asked of the system mode or the reverse).
- Signals: `Changed(s key, v value, s provenance)` — every accepted effective-value-or-provenance
  change is durable; *notifications* coalesce per event-loop turn per key, and the final signal of
  a turn carries the final value and provenance (dconf's changeset; Android's 200 ms window).
  `GenerationChanged(u generation)`.

Values are D-Bus variants of the key's type (`b`, `i`, `d`, `s`). Files stay readable without
the daemon (cosmic's rule): the `mura-settings` CLI reads them directly when the bus is absent.

zxr consumes the daemon and exposes only its own narrow HMD protocol (ADR 0012 §4.5); shell
components use the bus; entry-policy grants are `places.entry:<place_id>` instances (§1.1) with
`mutable` and `class = preference`.

## 9. Conformance checklist

1. Rebuild default-change with/without user value (advance vs survive).
2. `Set` equal to default ⇒ override created, provenance `Changed` emitted; `Reset` returns to
   default-following.
3. Immutable write ⇒ `ERR_IMMUTABLE`, no store write.
4. Kill daemon mid-write ⇒ no torn file; identical resolution on restart.
5. Stored value invalid under the running schema ⇒ resolves to the default with provenance
   `invalid`; the file is byte-identical afterwards; a generation whose schema accepts it reads
   it again.
6. Template instance: `Set` on new `places.entry:<id>` creates it; `ListInstances` enumerates;
   `DeleteInstance` removes exactly it.
7. `apply ≠ live` key: `Set` lands durable and `Changed` fires; nothing else happens (the label
   is visible in the artifact).
8. Locked-key consumer resolves from the artifact even when the daemon lies (fault injection).
9. A migration renames a key: the new key is readable and the old key is still in the file.

## 10. Open items

The device stratum's polkit action granularity (one action as snapd, or per domain as systemd;
decider: the rung that brings the first device key). Whether `state, session` is opened, and by
whom (decider: the first component that names the need). Quarantine UX surfaces are withdrawn
with quarantine; the `invalid` provenance is what a UI shows.
