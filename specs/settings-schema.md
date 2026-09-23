# specs/settings-schema: the generated schema artifact, strata, and reconciliation

**Status:** draft normative spec (specification workstream, wave 3).
**Design sources:** composition §7.3 **constraint 9** (one source of truth for defaults),
[research/35](../docs/research/35-settings-config-models.md) §7–§8 (the NixOS-interplay evidence
this spec makes normative). Decides the artifact format, strata, ownership semantics, and
reconciliation rules; the settings *daemon* itself remains a registry gap (this is its contract).
**Grounding:** storage paths use the XDG **Base Directory** spec (CDG sense) for per-user state;
schema semantics follow GSettings' capability set (type/range/default/writability) without its
compiled-blob mechanism.
**Budget impact** (inv. 9): schema generation is build-time; runtime reads are startup +
per-key-change notifications (D-Bus rate); zero frame-path cost. Watchers must be
event-driven — no polling (budgets.md §4.4 idle rule).

## 1. The schema artifact (build-time, generated — never hand-written)

Emitted by the NixOS module system from evaluated options (`nixosOptionsDoc`-shaped projection;
doc 35 §7.1) as `system.build.spatialSettingsSchema` → installed at
`/etc/spatial/settings-schema.json`. Constraint 9 rule: **no consumer may compile in an
independent default**; a missing/corrupt artifact is an explicit failure mode, not a fallback to
built-in values.

Per-key record (all fields mandatory unless marked):

```text
id            stable dotted key ("shell.follow.startFovDeg") — never renamed in place (§5)
type          bool | int | double | string | enum | list<...>  (Nix option type projection)
constraints   enum values / numeric range (optional)
default       the evaluated build default (post Nix priority resolution: module < device
              < profile — a single value by generation time; doc 35 §7.2)
stratum       build-fact | per-unit | per-user | session   (§2)
ownership     declarative | runtime   (§3 — the Home-Manager lesson, doc 35 §7.3)
locked        bool (appliance-profile lockdown; locked ⇒ writes rejected, UI shows enforced)
apply         live | reload:<unit> | restart:<unit> | reboot   (activation semantics, §4)
description   from the option declaration (one docs source)
schemaVersion integer, per-namespace (§5)
```

`spatial.*` contract options are *not* automatically keys: the module marks options for
projection (`spatial.settings.export`-style internal flag); build facts (panel geometry, SoC)
are **never** exported as writable keys — they appear, if at all, as `stratum=build-fact,
locked=true` for introspection.

## 2. Strata and storage layout

| Stratum | Store | Written by | Survives |
|---|---|---|---|
| build-fact | the schema artifact itself | nixos-rebuild only | generations |
| per-unit | `/var/lib/spatial/settings/` (system state, ADR 0007's calibration precedent) | settings daemon (polkit-gated for privileged keys) | reboots + rebuilds + users |
| per-user | `$XDG_STATE_HOME/spatial/settings/` | settings daemon on behalf of the session | reboots + rebuilds, per user |
| session | daemon memory only | anyone with the key's write grant | nothing |

Value stores are **sparse key–value files** (cosmic-config-shaped: one file per namespace,
atomic rename writes, versioned header): a key absent from every stratum resolves to the
generated default — *copying defaults into user storage at first login is forbidden* (doc 35
§7.2: it breaks default-advancement on rebuild).

Resolution order per key: `session > per-user > per-unit > generated default` (build-facts skip
the ladder). Locked keys resolve to the generated value regardless of stores.

## 3. Ownership: the two legitimate modes (normative)

Every key is marked at generation time:

- **`declarative`** — Nix owns the value; rebuild/switch *reasserts* it (any runtime write is
  rejected with `ERR_DECLARATIVE`; the UI shows it as system-managed). For appliance-profile
  policy keys.
- **`runtime`** — Nix owns only the *default*; an explicit user/unit value survives rebuilds;
  reset re-reveals the current generation's default.

This mark is what prevents "Nix emits defaults" from silently becoming "Nix overwrites
preferences" (doc 35 §7.3).

## 4. Reconciliation at generation switch (doc 35 §7.5, made normative)

For key `k` with old/new defaults `D_old/D_new` and optional explicit value `U`:

1. `U` absent ⇒ effective value advances `D_old → D_new` silently.
2. `U` present, valid under the new schema ⇒ `U` stays effective (`runtime` keys).
3. Reset(`k`) ⇒ delete `U`, reveal `D_new`.
4. Newly `locked` or `declarative` ⇒ `U` is quarantined (retained on disk under
   `quarantine/`, inert), enforced value applies, one notification emitted.
5. `U` invalid under the new schema (type/range/rename) ⇒ **migrate if a migration is declared
   (§5), else quarantine + default** — never silent coercion, never boot failure.
6. Consumers holding cached values receive `GenerationChanged` and must re-resolve; units with
   `apply=reload/restart` are handled by the activation script (standard NixOS switch flow).

## 5. Versioning and migration

`schemaVersion` is per-namespace, bumped on any incompatible key change (rename/split/merge/type
change). Migrations are declared *in the Nix module* (`from`, `to`, pure value-mapping function
serialized into the artifact as a description of the mapping; executable migration logic ships
in the settings daemon, keyed by `(namespace, fromVersion)`). Unknown future versions on disk ⇒
read-only quarantine (never destructive).

## 6. The notification and access interface

`org.spatialos.Settings1` on the session bus (system-scoped keys proxied with polkit
authorization; the polkit-agent gap is noted):

- `Get(s key) → (v value, s provenance)` — provenance ∈ {default, per-unit, per-user, session,
  locked, declarative}.
- `Set(s key, v value)` — validated against the schema record; errors: `ERR_LOCKED`,
  `ERR_DECLARATIVE`, `ERR_TYPE`, `ERR_RANGE`, `ERR_UNKNOWN_KEY`.
- `Reset(s key)`; `List(s namespacePrefix)`.
- Signal `Changed(s key, v value, s provenance)` — per-key, emitted only on effective-value
  change (a `Set` that equals the current effective value is a no-op).
- Signal `GenerationChanged(u generation)` — after nixos-rebuild activation (§4.6).

Wayland-side consumers (the zxr HMD settings API, ADR 0012 §4.5) do **not** speak this bus from
clients: zxr consumes the daemon (or the files directly, watch-based) and exposes only its own
narrow protocol; shell components use the bus. Entry-policy grants
([places-model.md §5](../docs/architecture/places-model.md)) are ordinary per-user keys under
`places.<place_id>.entry.*` with `runtime` ownership.

## 7. Appliance lockdown

The appliance profile (`spatial.xr.session.autoLogin`) may mark namespaces locked wholesale
(dconf-lockdown analog, generated from the profile module) — the guest/kiosk story. Locks are
schema facts, not daemon configuration, so a compromised daemon cannot unlock them (consumers
verify `locked` against the artifact).

## 8. Conformance checklist

1. Rebuild with changed default, no user value ⇒ new default effective without any store write.
2. Rebuild with changed default, user value present ⇒ user value survives (runtime key).
3. Write to declarative key ⇒ `ERR_DECLARATIVE`; value unchanged; no store write.
4. Kill daemon mid-write ⇒ no torn file (atomic rename); restart resolves identically.
5. Downgrade to older generation with newer-version user files ⇒ quarantine, not corruption.
6. `Changed` storm test: N rapid `Set`s coalesce per key; no unbounded signal amplification.

## 9. Open items

The daemon's process/activation design (registry gap #9 — this spec is its contract); polkit
routing for per-unit keys pending the polkit-agent design; whether `session` stratum is needed
at v1 or deferred; the quarantine UX.
