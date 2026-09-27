# specs/settings-daemon: `mura-settingsd`, the process behind `org.mura.Settings1`

**Status:** rev 1 (2026-09-25; D7). The process design [settings-schema.md](settings-schema.md)
rev 3 §10 left open, derived from the stores' source in
[research/58](../docs/research/58-settings-stores-from-comparables.md) and ruled shape C
(research/58 §13.1). Normative for `pkgs/mura-settingsd`, `lib/settings`, `modules/os/settings.nix`.
**Design sources:** settings-schema.md rev 3 (the contract); research/58 §3 (single session
writer — dconf), §11 (Rust + zbus, measured), §12.3 (steamos-manager's two-mode binary), §13.
**Grounding:** "XDG" = Base Directory; D-Bus activation and `Type=dbus` are systemd's and
dbus-broker's unmodified; the generation hook is NixOS's `system.userActivationScripts`
(`nixos/modules/system/activation/activation-script.nix:184-200`, the `nixos-activation`
user service that "switch-to-configuration restarts … explicitly on every switch").
**Budget impact** (overview invariant 9): one resident process per graphical session,
D-Bus-activated on first use; measured on the reference shape (research/58 §11): ~1.2 MB binary,
~3.5 MB RSS, four threads (zbus's `async-io` executor; never tokio's multi-thread runtime), no
timers, no polling — the artifact is read once at start and again on the generation hook;
stores are read on first access per schema and kept in memory; a `Set` is one atomic file
rewrite. Nothing on the frame path: consumers read the bus once and then act on `Changed`.

## 1. The problem this spec closes

The contract fixes *what* the store guarantees; this spec fixes *who* provides it, how it starts
and stops, where the validator lives, how a generation switch reaches a running session, and
how the same program later serves the device stratum without a proxy.

## 2. One crate, two modes, one CLI

`pkgs/mura-settingsd` (Rust; `libc`, `serde`/`serde_json`, `zbus` 5 with `async-io` — research/58 §11):

| Invocation | Bus | Runs as | Serves | Exists when |
|---|---|---|---|---|
| `mura-settingsd` | session | the user | `stratum = per-user` keys of every schema | every graphical session (D7) |
| `mura-settingsd --system` | system | root | `stratum = device` keys | the artifact has at least one device key (a target declares one; none today) |
| `mura-settings <get\|set\|reset\|list\|instances\|generation>` | either (`--system` selects) | caller | the `gsettings`/`kwriteconfig` CLI for scripts, tests and the recovery shell; `--direct` reads the artifact and files with no bus (cosmic's rule: values stay recovery-readable) | with the daemon |

The two modes share the artifact loader, the resolver, validation, the store and the bus
interface; they differ in the bus, the storage roots, and the **handler layer**: the session mode
has none (a `Set` is a file write); the system mode dispatches each key to a handler that
validates against the device contract and applies (steamos-manager `-r`; snapd `configcore`).
Handlers are a trait with no implementations in D7. A key asked of the wrong mode is
`ERR_WRONG_BUS`.

## 3. Lifecycle

**Activation.** `org.mura.Settings1` is D-Bus-activatable on the user bus
(`SystemdService=mura-settingsd.service`; the unit is `Type=dbus`, `BusName=org.mura.Settings1`,
`Restart=on-failure`, `PartOf=graphical-session.target`). The first `Get` from any consumer
starts it. dconf's shape ("not activated in the user session until the user modifies a
preference", `dconf/README:11-14`) — with reads too, because Mura consumers have no mmap'd db.

**Residency.** The daemon **stays resident** once started (cosmic-settings-daemon's shape), not
exit-on-idle (dconf's, systemd's mini-services'). Reason: dconf's service can exit because its
readers mmap the db and the bus daemon holds the watch list, so nothing is lost while it is gone;
Mura's daemon is the *notifier* — a consumer that received `Changed` subscriptions expects
`GenerationChanged` from the same source, and an exited daemon cannot observe the switch. At
~3.5 MB RSS the residency is the cheaper of the two (research/58 §11).

**Generation switch.** `system.userActivationScripts.muraSettings` runs
`mura-settings generation-changed` in every logged-in user's manager when
`switch-to-configuration` restarts `nixos-activation.service` (NixOS's hook for exactly this —
its own example rebuilds the KDE service cache). The hook pokes a *running* daemon only: one not
running has no subscribers to tell and reads the new artifact when the bus next activates it
(the hook also runs at every login, and must not defeat activation on first use). The daemon
re-reads the artifact, re-resolves every loaded key, emits `Changed` for each whose
effective value or provenance moved (§4 of the contract), then `GenerationChanged(s generation)`
where `generation` is the artifact's store hash (its identity in the closure). No inotify on
`/etc`: the switch is the only writer of the artifact and NixOS already tells the session.

**Shutdown.** SIGTERM from the manager at session end; nothing to flush — every accepted `Set`
was durable before its reply.

## 4. Storage

Roots (contract §2): `$XDG_CONFIG_HOME/mura/settings/` (preferences),
`$XDG_STATE_HOME/mura/settings/` (state); the system mode uses `/var/lib/mura/settings/{config,state}/`
only for device keys nothing else owns (a handler that *is* the store).

One file per (schema, instance): `<schema>.json` or `<schema>:<instance>.json` (instance
percent-escaped). Content:

```json
{ "schema": "xr.passthrough", "instance": null, "schemaVersion": 1,
  "generation": "<artifact hash that last wrote>",
  "values": { "latencyMode": "high-quality" } }
```

Sparse: only explicit overrides. **Write**: serialize, write `<file>.tmp`, `fsync`, `rename`,
`fsync` the directory (snapd's `AtomicWriteFile`; dconf's `g_file_set_contents` skips the fsync
on a new file — Mura does not, the store is small). **Read**: on first access per schema; a file
that fails to parse is treated as *absent* and logged, never rewritten (cosmic's "corrupt keys
stick"; the user's copy survives). Entries the schema does not know are kept verbatim across
rewrites (an older or newer generation's keys — contract §4 rule 5, §5 additive migrations).

## 5. Resolution and validation

For a key `k` of schema `S` (instance `i`):

1. Not in the artifact (and no template matches) → `ERR_UNKNOWN_KEY`.
2. `locked` → the artifact's `default`, provenance `locked`.
3. `mutability = immutable` → the artifact's `default`, provenance `default`; `Set` →
   `ERR_IMMUTABLE`.
4. Stored value present and valid (type, enum, range) → it, provenance `user` (`device` in system
   mode).
5. Stored value present and invalid → run migrations for `(S, header.schemaVersion)` if below the
   artifact's; if still invalid → the artifact's `default`, provenance `invalid`.
6. Absent → the artifact's `default`, provenance `default`.

`Set(k, v)`: steps 1–3 as above; then type check (`ERR_TYPE`), enum/range (`ERR_RANGE`); write
the override **even if `v` equals the resolved value** (contract §3); reply after the rename;
emit `Changed` only if the effective value *or provenance* changed (a `Set` equal to a stored
`user` value emits nothing; a `Set` equal to the `default` with no override emits — provenance
moved to `user`). `Reset(k)`: remove the override, rewrite, emit `Changed` if the effective value
or provenance moved.

Consumers of security-relevant keys read the artifact themselves (contract §7); the CLI's
`get --direct` is the reference implementation of that discipline and the conformance test's
instrument for item 8.

## 6. Signals and coalescing

Single-threaded: one method call at a time, so "per event-loop turn" is per call. Each
accepted `Set`/`Reset` emits at most one `Changed(key, value, provenance)`. The generation hook
emits one `Changed` per moved key, then one `GenerationChanged`. There is no timer-based
coalescing (Android's 200 ms window exists to batch *disk writes*, which Mura does per call).

## 7. Migrations

`migrations.rs`: a table `(schema, fromVersion) → fn(&mut Store)`. On first access, while a
store's `schemaVersion` is below the artifact's for that schema, apply the step for its version,
bump the header, repeat. Steps are **additive** (contract §5): they may add keys and rewrite the
header; they may not remove or overwrite existing keys. A step is ordinary Rust in the daemon,
reviewed like any code; the Nix side declares only `schemaVersion` and the daemon refuses to
start if the artifact's version for a schema exceeds the highest step it knows plus one (a
schema bump without a migration is a build error the VM test catches).

## 8. The CLI

`mura-settings get <key>` prints `<value>\t<provenance>`; `set <key> <value>` parses the value
per the artifact's type; `reset`, `list [prefix]`, `instances <template>`, `generation`;
`--system` targets the system mode; `--direct` bypasses the bus (read-only). Exit codes: 0, 1
(refused: locked/declarative/range/type), 2 (unknown key/usage), 3 (no bus and no `--direct`).

## 9. Conformance (the VM test, `tests/vm/settings.nix`)

Items 1–9 of settings-schema.md §9, plus: (10) the daemon is not running before the first
`Get` and is after (activation); (11) `mura-settings generation-changed` after a
`specialisation` switch produces `Changed` for a key whose default moved and `GenerationChanged`
with the new artifact hash; (12) the binary carries no interpreter and joins `tests/closure.nix`'s
login-path roots; (13) measured RSS and thread count recorded against §"Budget impact".

## 10. Open items

The system mode's handler set and polkit action granularity (the rung that brings the first
device key); whether `state, session` is opened (the first component that names the need).
