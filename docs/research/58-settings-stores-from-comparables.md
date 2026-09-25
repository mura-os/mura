# 58 — Settings stores from comparables: what shipping desktops do, why, and what transfers

**Research date:** 2026-09-25. **Question:** [specs/settings-schema.md](../../specs/settings-schema.md)
rev 2 fixes a *contract* (artifact, strata, ownership, reconciliation, quarantine, migrations,
apply transactions, lockdown, bus) whose evidence base, [research/35](35-settings-config-models.md),
was written before AGENTS rules 6–8 and cited the stores by URL. Before the daemon (D7) is
designed, each mechanism the contract asserts is re-derived from the stores' *source*: what
problem each comparable solved, what it chose, **why** (its comments and design docs), which
assumptions it rests on, whether they transfer to a battery-powered headset with a
NixOS-generated schema, and what adopting it trades off. Where no comparable has a mechanism,
that is recorded as a signal (rule 7) and the item goes to the owner (rule 8).
**Method:** six stores newly pinned in `references/` (`dconf`, `glib`, `kconfig`, `libcosmic`,
`cosmic-settings-daemon`, `gsettings-desktop-schemas`; MANIFEST.json), read beside the already
pinned system-scoped daemons (`systemd` hostnamed/timedated/localed, `accountsservice`,
`networkmanager`), the embedded layered-prefs precedent (`platform2/power_manager`) and the
locked nixpkgs (`nixos/modules/programs/dconf.nix`, `switch-to-configuration-ng`); then — after
the owner's challenge that "no comparable among desktop stores" is not "no comparable" — the
appliance and device OSes that *do* ship a privileged device-wide configuration service: snapd's
`snap set system` (`references/snapd`), OpenWrt UCI + procd + LuCI (`uci`, `procd`, `luci`),
SteamOS's `steamos-manager` (Rust; `references/steamos-manager`, v26.4.1), ChromeOS device
settings (`platform2/login_manager`), systemd-homed's user records, and Android's
SettingsProvider ([external], engineering evidence only, rule 2). Two budget measurements were
made on this host (§11). **Budget impact** (overview invariant 9): this
document schedules nothing; the daemon it informs is judged in §11 and in the spec it produces.

## 0. The comparables and what each *is*

| Comparable | Problem it solved | Shape | Licence |
|---|---|---|---|
| **dconf + GSettings** (GNOME, Phosh, postmarketOS GNOME/Phosh) | preferences read thousands of times per write, by many processes, with site defaults and lockdown | compiled schema (defaults) + one binary user db mmap'd by readers + a write-only session service | LGPL-2.1+ |
| **KConfig + KConfigXT + kconf_update** (Plasma, Plasma Mobile) | many cooperating Qt apps sharing INI files without a broker | in-process library, cascading INI, dirty-entry merge on write, opt-in D-Bus notify, login-time migration scripts | LGPL-2.0+ |
| **cosmic-config + cosmic-settings-daemon** (COSMIC; Rust) | typed Rust config with live reload and no compiled-blob step | file-per-key RON under XDG, versioned directories, every process writes, a session daemon concentrates inotify into D-Bus signals | MPL-2.0 / GPL-3.0 |
| **systemd hostnamed / timedated / localed; accountsservice** | letting an unprivileged desktop session change a few *system* facts | one small D-Bus service per domain owning specific `/etc` (or `/var/lib`) files, own polkit actions, bus-activated, exit-on-idle | LGPL-2.1+ / GPL-3.0 |
| **NixOS `programs.dconf`** | declaring a desktop store's system layer from the module system | keyfiles → `dconf compile` at *build* time; profiles and locks under `/etc/dconf`; no runtime writer | MIT |
| **ChromeOS `power_manager` prefs** | a vendor daemon's tunables on a fixed device, overridable for debugging | file-per-pref, layered RO sources under `/usr/share` + one RW override dir under `/var/lib`, watched | BSD-3 |

None of them is "a Nix-module-to-runtime-registry pipeline" (research/35 §9 said so; it holds).
What they converge on, and where they diverge, is below, mechanism by mechanism in the spec's
order.

## 1. Schema artifact with no compiled-in defaults (spec §1)

**dconf/GSettings.** Defaults live in the *schema*, compiled once by `glib-compile-schemas` into
`gschemas.compiled`; the application carries none (`references/glib/gio/gsettings.c:63-65`).
Distributors adjust defaults without patching XML through `.gschema.override` keyfiles, because
"patching the XML source for the schema is inconvenient and error-prone"
(`glib/gio/gsettings.c:219-237`; `glib/docs/reference/gio/glib-compile-schemas.rst:41-51`).
**KConfig.** The opposite: KConfigXT compiles defaults *into the app* from a `.kcfg`, with the
stated goal "Have the default value for config entries defined in 1 place" — before it, the read
path, the settings dialog and "Use defaults" each carried a copy (`kconfig/DESIGN:59-63`,
`kconfig/docs/DESIGN.kconfig:112-116`). System-wide rc files under `$XDG_CONFIG_DIRS` form a second
default channel, marked "default" entries in the map (`kconfig/DESIGN:24-26`).
**cosmic-config.** Two channels too: system-default key files under `$XDG_DATA_DIRS/cosmic/<id>/v<N>/`
*and* Rust `Default` in the entry type; `get_entry` starts from `Self::default()` and fills what
it can read (`libcosmic/cosmic-config-derive/src/lib.rs:192-202`). No mechanism keeps the two
equal (research/35 §4.1 already noted this).
**Transfer.** The spec's constraint 9 ("no consumer compiles in an independent default") is
exactly GSettings' shape and exactly what KConfigXT's own DESIGN was trying to reach from the
other side; cosmic shows the failure mode of not enforcing it. The generated artifact is the
compiled schema; `nixosOptionsDoc`-style projection is the compiler. **Converging.**
Trade-off carried from GSettings: an app that starts before the artifact exists has *no* default
— the spec's "explicit failure mode" is the price GSettings also pays (a missing schema aborts).

## 2. Preference vs state roots; per-user vs per-unit (spec §2)

**cosmic-config** is the only store with a *state* root: `Config::new_state` under
`$XDG_STATE_HOME`, with the reason in the source — "State is meant to be used to store items that
may need to be exposed to other programs but will change regularly without user action"
(`libcosmic/cosmic-config/src/lib.rs` ~286-288). dconf and KConfig predate `$XDG_STATE_HOME` and
put both in config. **Transfer.** The XDG Base Directory distinction the spec grounds itself in
is the newer, deliberate one; cosmic is the shipping proof it is workable. **Converging** (one
comparable plus the spec's authority; no comparable contradicts it).

**Per-unit (system-wide) preferences written at runtime.** Here the comparables are unanimous
in the *negative*:
- dconf: "No profile starting with a `system-db:` … source can ever be writable"
  (`dconf/engine/dconf-engine-profile.c:80-82`); the system db is admin-edited keyfiles compiled
  by `dconf update` with sufficient privilege (`dconf/docs/dconf-tool.xml:117-119,168-171`);
  `dconf-engine-source-system.c:29-35` never sets `writable`.
- KConfig: a library; system settings are "not KConfig's job" — Plasma calls `timedate1` for the
  clock (`plasma-workspace/geotimezoned/geotimezonemodule.cpp` ~145-166) and KAuth helpers for
  root work (`plasma-workspace/kcms/kfontinst/dbus/FontInst.cpp:863-873`).
- cosmic-settings-daemon: brightness through logind's `SetBrightness`, keyboard layout through
  `locale1` behind one polkit rule (`cosmic-settings-daemon/data/polkit-1/rules.d/`); no root
  helper of its own.
- systemd: no generic system key-value daemon exists; system configuration flows through
  drop-ins, credentials, confext — and, for the few things a session may change, one
  mini-service per domain with its own actions (`man/systemd-hostnamed.service.xml:32-34`,
  `src/hostname/org.freedesktop.hostname1.policy:19-26`). NEWS v195 calls them "mini-services
  which previously only provided support for changing time, locale and hostname settings from
  graphical DEs" (`systemd/NEWS:22269-22276`).
- accountsservice: a daemon owning `/var/lib/AccountsService/users/*` keyfiles with four polkit
  actions, existing because account data has no other D-Bus home
  (`accountsservice/README.md:9-14`, `data/org.freedesktop.accounts.policy.in`).
- NixOS: the system layer is a *build* product (`dconf update $out/db` in `postBuild`,
  `nixos/modules/programs/dconf.nix:238-241`).

**Why they all refuse a generic writable system store:** the reasons are stated per project —
dconf's is read-optimisation with one writer per db and the system db being the administrator's
(README:6-9, profile docs); systemd's is that each domain's file has its own validation and
its own authorization question (invalid hostname, uninstalled locale, unknown zone are refused
with `SD_BUS_ERROR_INVALID_ARGS` — `localed.c:152-161`, `timedated.c:687`), which a generic
key-value service cannot express. **Transfer:** every reason transfers; a headset changes
nothing about who may set the clock. **The spec's "preference, per-unit, written by the daemon
(polkit-gated)" row and the "privileged apply agent (the daemon's system half)" of §6 have no
comparable.** Signal (rule 7); §12 Q1.

## 3. Single writer + bus notification, or library + file watch (spec §8)

Three shapes ship:

| | dconf | KConfig | cosmic-config |
|---|---|---|---|
| writer | one session service; clients never write the db | every process writes its INI | every process writes its key files |
| reader | mmap of the binary db, "zero system calls" | parse INI on open, `reparseConfiguration` on demand | read one RON file per key |
| notification | D-Bus `Notify` from the service (`dconf/service/ca.desrt.dconf.xml:10-15`) | D-Bus `org.kde.kconfig.notify.ConfigChanged`, **opt-in** via the `Notify` write flag (`kconfig/src/core/kconfig.cpp:511-532`) | inotify per process (`Config::watch`), or the daemon's D-Bus `changed` after it watched the trees |
| concurrent writes | serialized in the service; client "fast" path keeps pending writes readable (`dconf/engine/dconf-engine.c:45-87`) | lock file + re-read + merge dirty entries only ("First, reparse the file on disk, to merge our changes with the ones done by other apps", `kconfigini.cpp:405-406`); lost update if two apps dirty the same key | none; last rename wins |

**Why dconf chose the service:** "read 1000s of times for each time the user changes one"; the
service "is only involved in writes … stateless and can exit freely at any time"; fsync latency
"up to 100ms" is hidden by the fast path (`dconf/README:1-29`). The whole-file rewrite that
forces a single writer is GVDB's: "Modifying … requires writing out the whole file … an external
process is needed to synchronise writes" (gvdb `README.md`, dconf subproject).
**Why KConfig has none:** its DESIGN never argues against a daemon; the architecture is the
evidence — cooperating Qt apps, INI you can edit by hand, merge-on-write good enough.
**Why cosmic-settings-daemon exists:** a "notification concentrator" so that N consumers do
not each hold recursive inotify watches — and it reconnects with backoff because "The settings
daemon has exited" is expected (`libcosmic/cosmic-config/src/dbus.rs` ~114-156). Values stay
recovery-readable files; the daemon is optional for correctness.

**Transfer.** The spec wants *validation* (type, range, enum, lock, declarative) on every write
and provenance in every signal. Only the dconf shape puts a validator between the writer and the
file; KConfig validates in-process against its compiled `.kcfg`, cosmic not at all (a bad file
"sticks" and the reader falls to `Default`). On a device where the schema is *generated* and the
consumer must not compile defaults in, the validator has to live in the one place that has the
artifact: the service. **Converging on the dconf shape (single session writer + bus signals)**,
with cosmic's rule kept: files stay readable without the daemon (the CLI reads them).
Trade-offs adopted with it: a write costs an IPC round trip (dconf hides it with the fast
path; the spec's coalescing per event-loop turn is the same idea on the signal side), and the
daemon is a process on the budget (§11).

## 4. Sparse stores, "Set always writes", Reset (spec §3)

**GSettings:** `g_settings_set_value` always calls `g_settings_write_to_backend` after type and
range checks (`glib/gio/gsettings.c:1650-1683`); *dconf-service* then filters no-op changes
(`dconf/common/dconf-changeset.c:775-841`) and skips the rewrite when nothing changed
(`dconf/service/dconf-writer.c:132-167`) — so an explicit set equal to the *stored* value is
dropped, but a set equal to the *default* with no stored value **is written**: the user value
exists afterwards, and `g_settings_get_user_value` distinguishes "user set it equal to the
default" from "following the default" (`gsettings.c:1313-1331,1358-1379`). `g_settings_reset`
writes NULL, i.e. removes the user value so the key follows the default again
(`gsettings.c:2432-2452`; `dconf/gsettings/dconfsettingsbackend.c:113-117`).
**KConfig:** the *opposite* policy, stated: "When entries are written to disk, it is checked
whether the entry to write is equal to the default, if so the entry will not be written"
(`kconfig/docs/DESIGN.kconfig:32-38`); `KCoreConfigSkeleton` reverts to default on equality
(`kcoreconfigskeleton.cpp:293-306`). Its reason is thin user files; the cost is exactly the one
the spec names — a later default change silently moves a value the user meant to pin.
**cosmic:** `set` always writes; only the derive's per-field setter dedupes against memory.
**Transfer.** The spec's rule ("equality is not absence of intent") is GSettings' behaviour and
KConfig's documented trade-off taken the other way. **Converging** with GSettings; provenance
(`Get` returning it, `Changed` carrying it) is `get_user_value` made explicit — cosmic's
`get_local`/`get_system_default` split is the same information.

## 5. Lockdown (spec §7)

**dconf:** locks live in a *non-first* (system) db: "If a lock … is installed into a database
then no database listed above that one … will be able to modify" (`dconf/docs/dconf-overview.xml:160-162`);
writability checks locks in sources `i >= 1` only (`dconf-engine.c:433-454`). GSettings exposes
it as `g_settings_is_writable` and `writable-changed` (`gsettings.c:2477-2500,875-891`), and the
UI is expected to grey the control. `org.gnome.desktop.lockdown` is a *separate* schema of
advisory booleans (`disable-command-line`, `disable-lock-screen`, `user-administration-disabled`, …
— `gsettings-desktop-schemas/schemas/org.gnome.desktop.lockdown.gschema.xml.in`), enforced by
each app, not by dconf. NixOS declares locks in the module (`programs.dconf.profiles.*.locks`,
`dconf.nix:135-141`) and compiles them.
**KConfig:** `[$i]` immutable markers on entry, group or file (`kconfig/docs/options.md:19-49`)
plus Kiosk `[KDE Action Restrictions]` read through `KAuthorized` (`kauthorized.h:28-36`).
**Transfer.** Locks as facts of the *system layer*, declared where the system layer is declared
(the profile module), reported to the UI as writability — this is the spec's §7 exactly, and
NixOS already does it for dconf. **Converging.** The spec's additional rule — security-relevant
consumers read the locked value from the root-owned artifact, not the bus — has no comparable
(dconf clients trust dconf) but is a strict subset of the mechanism: the artifact already
carries the value; it is a consumer discipline, not a daemon feature. Noted; not a rethink.

## 6. Relocatable / instance schemas (spec §1.1)

**GSettings:** relocatable schemas exist for "an 'account' … arbitrary number of accounts" and
per-window geometry (`glib/gio/gsettings.c:68-73,282-310`); the instance is a *path* supplied by
the app. Enumeration: `g_settings_list_children` lists the schema's declared children, not live
instances (`gsettings.c:2572-2593`); `dconf list` lists what is in the db
(`dconf/docs/dconf-tool.xml:138-141`). **Garbage collection: none.** Orphaned instance trees stay
until an app or `dconf reset -f` removes them. KConfig has groups (the same thing, untyped);
cosmic has no instances at all.
**Transfer.** The spec's template + `places.entry:<id>` is GSettings' relocatable schema with a
stable id instead of a path. **Converging** on creation-by-write and explicit deletion. The
spec's "orphans retained until a GC policy owned by the referent's component" is GSettings'
behaviour stated as a rule (no comparable GC's) — consistent with the evidence.

## 7. Migrations (spec §5)

**kconf_update** is the only shipping migration *engine* for a settings store. Its reason:
applications used to honour both old and new keys, "slower startup" and "code that will only be
used once"; kconf_update updates "configuration files without adding code to the application
itself" (`kconfig/src/kconf_update/kconf_update.qdoc:5-21`). In KF6 the `.upd` format is
**script-only** — `Version=6`, `Id=`, `Script=`, `ScriptArguments=`
(`kconfig/src/kconf_update/kconf_update.cpp:167-224`); the declarative `Key=old,new` /
`RemoveKey` DSL of KF5 is gone. Shipping `.upd` files are C++ or Python programs
(`kwin/kconf_update/kwin.upd`, `plasma-workspace/shell/kconf_update/plasma6.0-remove-old-shortcuts.upd`,
`libnotificationmanager/kconf_update/plasma6.4-migrate-fullscreen-notifications-to-dnd.upd`).
State: `kconf_updaterc` records done ids per file; it runs from kded at session start; a script
that exits non-zero is still marked done (`kconf_update.cpp` ~353-356, `gotId` 243-255) — no
infinite retry, no rollback.
**GSettings:** `gsettings-data-convert` (ships with GConf) maps GConf keys to GSettings keys from
`/usr/share/GConf/gsettings/*` keyfiles, "designed to be executed automatically, every time a
user logs in", idempotent, recording done sets in a user keyfile
(`glib/docs/reference/gio/migrating-gconf.md:374-451`); no value transforms. Beyond that,
schema changes in GNOME are handled by *the app* reading old keys, or by dconf dump/load scripts
(`lomiri-system-settings/debian/session-migrations/…gsettings-schema-name-change.sh:32-49`).
**cosmic:** `v<N>` directories; readers fall back **one** version for a *missing* key
(`libcosmic/cosmic-config/src/lib.rs` `get_local` → `previous`); no migration code exists
(`migrat*` finds nothing), old directories linger.
**Transfer.** Declared, login-time, idempotent, recorded migrations are the converging shape
(kconf_update, gsettings-data-convert). **Typed operations** (the spec's rename/delete/enum-map/
scale/split/merge) are the *KF5* DSL, which KDE abandoned for scripts because real migrations
needed code; **golden upgrade/downgrade fixtures in CI** have no comparable. So: declared
migrations **converge**; a fixed typed-op vocabulary is **partial** (one comparable, retired);
build-time graph assertion and golden tests are Mura's additions with no precedent. §12 Q2.

## 8. Invalid stored values (spec §4, §4.1)

**GSettings:** on read, a stored value failing `g_settings_schema_key_range_check` becomes NULL
and the key resolves to its **default**; the bad value is **not deleted** and — contrary to
research/35's recollection — **not warned about** on the read path (`glib/gio/gsettingsschema.c:1375-1416`,
`gsettings.c:1248-1273,1302-1305`); a wrong-typed backend value is likewise silently treated as
missing (`gsettingsbackend.c:695-723`). Warnings fire on out-of-range *set* (`gsettings.c:1672-1678`).
**KConfig:** `readEntry` returns the caller's default when conversion fails
(`kconfiggroup.cpp:256-268`); `KCoreConfigSkeleton` **clamps** ints to `<min>/<max>` and falls to
the builtin default for an unknown enum string (`kcoreconfigskeleton.cpp` ~466-475, ~617-632).
**cosmic:** parse errors are returned with the defaults; the file is never rewritten
("Corrupt keys stick", `cosmic-config-derive` `get_entry`).
**systemd:** on-disk garbage is logged and discarded ("Do not fail when the .conf file contains
an invalid setting, but discard the stored settings", `src/locale/localed-util.c:189-192`).
**Transfer.** Every comparable answers "fall back to the default, leave the file alone"; KConfig
alone clamps. **Nobody quarantines**, nobody tags by generation, nobody remounts on downgrade.
The spec's §4.1 solves a real problem the comparables also have (a value silently ignored is
lost intent), but with a mechanism none of them built. Signal (rule 7); §12 Q3.

## 9. Applying a change (spec §6)

**Desktop stores apply live or not at all.** GSettings has *delay-apply* for a dialog's "Save"
(`g_settings_delay`/`apply`/`has-unapplied`, "groups of settings … changed simultaneously and
atomically", `gsettings.c:239-261`) — a client-side batching, not a status machine. Consumers
(gnome-settings-daemon plugins, KCMs, cosmic-comp's `config_changed`) reload on the signal;
cosmic-comp leaves `// TODO Revert to default?` on an apply failure (`cosmic-comp/src/config/mod.rs`
~818). Unit reloads are the *domain daemon's* business: `localed` asks PID 1 to reload its
environment after writing `/etc/locale.conf` because otherwise "PID1 defaults would be stale"
(`src/locale/localed.c:308-316`). ChromeOS documents "powerd will need to be restarted" for most
prefs (`platform2/power_manager/docs/prefs.md`). NixOS applies at `switch-to-configuration`,
which reloads each logged-in user's manager (`switch-to-configuration-ng/src/main.rs:146-148,1538-1544,2421-2432`).
**Transfer.** A key whose effect needs a unit reload/restart exists in every comparable, and
every comparable puts the reload where the unit is owned — the domain service or the
activation step — never in the settings store. **No comparable** has a `pending/applied/failed`
transaction on a settings key, nor a settings daemon that restarts other units. Signal; §12 Q4.

## 10. Session stratum (spec §2 last row, §10)

GSettings' memory backend exists "to override the default for debugging"
(`glib/docs/reference/gio/overview.md:317-322`) and for tests; dconf has no non-persistent layer;
KConfig and cosmic none. **No comparable** ships memory-only settings with grant semantics.
The spec already lists it as open; the evidence says drop it until a consumer names the need.
§12 Q5.

## 11. The daemon on the budget (rule 6) — measured

Rust is the language (rule 6: it parses, validates, serves and holds state). The Rust desktop
comparables all speak D-Bus through **zbus**: cosmic-comp (`Cargo.toml:77`),
xdg-desktop-portal-cosmic (`:37`, `tokio` feature), cosmic-settings-daemon (5.11, `tokio`,
`current_thread` runtime — `Cargo.toml:62`, `src/main.rs:449+`). Nothing in `references/` uses
sd-bus from Rust or a hand-rolled wire protocol for a service. Measured on this host, a minimal
`org.mura.Settings1` service (Get/Set), release build, LTO, `opt-level=s`, stripped:

| build | binary | RSS after connect | threads | crates |
|---|---|---|---|---|
| Rust baseline (sleep) | 296 KB | 2.3 MB | 1 | — |
| zbus 5, `async-io` executor, blocking API | 1.13 MB | 3.2 MB | 4 | 92 |
| zbus 5, `tokio` (multi-thread default) | 1.23 MB | 4.5 MB | 34 (1 per CPU here) | 92 |

dconf-service on a desktop sits in the same 2–4 MB RSS band; cosmic-settings-daemon carries
tokio + notify + udev + audio + Wayland toolkit (its `Cargo.toml`) — the config-notify core is
the light subset. **Determination (converging evidence, measured):** zbus with the `async-io`
executor (or `tokio` `current_thread` as cosmic-settings-daemon does — never the multi-thread
default), blocking API where the daemon is single-threaded; ≈ +0.9 MB binary, +1 MB RSS, three
extra threads over the libc-only crates in the tree. Acceptable on the budget; recorded so the
next rung can hold the daemon to it. Bus-activation + exit-on-idle (systemd's mini-service
shape, `bus_event_loop_with_idle`, `DEFAULT_EXIT_USEC` 30 s — `src/shared/bus-util.c:123-170`,
`src/basic/constants.h:23-26`; dconf's "not activated … until the user modifies a preference")
is the comparables' answer to a daemon that mostly sleeps — but a *signal source* cannot exit
while it has subscribers, which is why dconf pushes the watch list into the bus daemon's match
rules (README:15-16) and cosmic's daemon stays up. That trade-off belongs to the process design.

## 12. Appliance and device OSes: the privileged device-wide store *does* ship

§2–§10 read the desktop stores. Devices with an administrator and no desktop have solved the
same problem differently, and four of them are now pinned. What each is, and why:

### 12.1 snapd — `snap set system key=value` (Ubuntu Core)

One JSON state file (`/var/lib/snapd/state.json`), one `config` tree per snap, `system` an alias
for `core` (`snapd/overlord/configstate/configstate.go:151-157`). Writes go through a
`Transaction` — "a copy of the configuration … which can be queried and mutated in isolation
from concurrent logic. All changes performed into it are persisted back into the state at once
when Commit is called" (`overlord/configstate/config/transaction.go:38-41`) — checkpointed with
`osutil.AtomicWriteFile` (temp, fsync, rename, fsync dir; `osutil/io.go:44-47`).
**There is no generic schema.** Every `system.*` key is an allow-listed Go handler with its own
`validate` and `handle` (`configcore/handlers.go`, `runwithstate.go`): `system.timezone` is a
regex then `timedatectl set-timezone`; `system.hostname` goes to `hostnamectl`;
`service.ssh.disable` masks a unit; `system.power-key-action` writes a logind drop-in; an
unknown key is refused. The order is **validate all → apply all → commit**: "All configuration
changes are persisted at once, and only after the snap's configuration hook returns
successfully" (`cmd/snapd/cli/cmd_set.go:41-42`); a failed apply leaves the state *un*changed
while side effects may be partial (no generic undo for core config). The work is a `Change` of
`Task`s with statuses `Do/Doing/Done/Abort/Undo/Undoing/Undone/Error/Wait`
(`overlord/state/change.go:36-76`), queryable by id — the reason for tasks with undo: "keeping
data around for a potential undo until there's no more chance of the task being undone"
(`taskrunner.go:154-158`). Authorization: the root socket, or polkit
`io.snapcraft.snapd.manage-configuration` at `auth_admin` (`data/polkit/io.snapcraft.snapd.policy:40-47`);
no agent → 401. Image defaults: `gadget.yaml` `defaults:` applied to the filesystem early in
boot, "before all the configuration is applied as part of normal execution of configure hook"
(`sysconfig/sysconfig.go:103-107`). Upgrades: numbered code patches on `state.json`
(`overlord/patch/`, level 6 sublevel 3); a downgrade across a level is **refused** — "cannot
downgrade: snapd is too old for the current system state" (`patch.go:112-113`). No transient
layer; no D-Bus change signal — the owning snap's `configure` hook *is* the apply channel.
Assumptions: root daemon, appliance, snaps own their config. GPL-3.0.

### 12.2 OpenWrt — UCI + procd + LuCI

UCI is an **untyped** text store: sections, options, lists of strings (`uci/uci.h:374-376`),
`/etc/config/<package>`; staged deltas in `/tmp/.uci` until `uci commit`, which write-locks,
**re-reads the file to merge other processes' deltas** ("other processes might have modified the
config as well. dump and reload", `uci/file.c:771-774`), then temp + fsync + rename. Its
**runtime layer** is a delta path that is "'overlays' for the active config, that will never be
committed" (`uci/uci.h:277-282`) — `uci -P /var/state` sets the savedir *and* makes commit a
no-op (`uci/cli.c:331-334`); init scripts record assigned interface names there; tmpfs, gone at
reboot. Types and validation live **outside** the store: LuCI's datatypes in the UI, procd's
per-service `validate` registry (`procd/service/validate.c`), base-files' `uci_validate_section`.
Apply is the init system's: services declare `procd_add_reload_trigger <config>`; a commit is
followed by `ubus call service event config.change` and procd reloads what registered
(`procd/service/service.c:788-820`, `trigger.c:297-306`). LuCI adds **apply with rollback**:
`uci apply {rollback:true, timeout:90}` then `confirm` — "the configuration changes must be
confirmed within a specific time interval, otherwise the device will begin to roll back the
changes in order to restore the previous settings" (`luci/…/ui.js:5470-5476`), because a router
misconfiguration locks you out. Authorization: an administrator session (rpcd ACL grants
`uci.set/commit/apply` to the LuCI role); no polkit, no per-key authorization. Defaults: the
image's `/rom/etc/config` and once-run `/etc/uci-defaults/*` scripts, deleted after running.
Migrations: scripts. Assumptions: single administrator, router, ubus not D-Bus. LGPL-2.1
(UCI, procd), Apache-2.0 (LuCI).

### 12.3 SteamOS — `steamos-manager` (Rust)

Two daemons: "one runs as the logged in user and exposes a public DBus API on the session bus,
and the second daemon runs as the `root` user. The root daemon exposes a limited DBus API on the
system bus for tasks that require elevated permissions to execute. The DBus API exposed on the
system bus is considered a private implementation detail" (`steamos-manager/README.md:88-96`).
The public surface is **typed per-feature interfaces**, not a key-value store:
`TdpLimit1`, `BatteryChargeLimit1`, `FanControl1`, `GpuPerformanceLevel1`, `PerformanceProfile1`,
`WifiPowerManagement1`, `LowPowerMode1`, `ScreenReader1`, `SessionManagement1`, … (`data/interfaces/*.xml`,
`src/manager/user.rs`). Ranges come from a **device contract file**: `data/devices/steam-deck.toml`
declares `[tdp_limit.range] min = 3 max = 15`, `[fan_speed] hwmon = "steamdeck_hwmon"`; a set
outside the range is refused (`src/power.rs:454-490`), a set inside is written to sysfs or
toggles a systemd unit (`hardware.rs:372-383`). Almost nothing is *stored*: `state.toml` holds
`default_login_mode`/`desktop_session` (user) and a debug inhibit (root); the hardware is the
store and is re-read. Authorization is Valve's single-seat posture — the system-bus policy lets
"Anyone … send messages to the service" (`data/system/com.steampowered.SteamOSManager1.conf:7-15`);
no polkit in the tree. Notification: zbus `PropertiesChanged` plus a relay of root signals onto
the session interface. Stack: zbus 5 on tokio **multi-thread**, `Type=notify-reload` units
(`Cargo.toml:8-39`, `data/system/steamos-manager.service:9-10`). Assumptions: Steam is the UI,
one wearer-class user, Deck-class hardware. MIT.

### 12.4 ChromeOS — device settings in `session_manager`

Device settings are a **signed policy blob** (`ChromeDeviceSettingsProto` inside a
`PolicyFetchResponse`) stored by `session_manager` in `/var/lib/devicesettings/`
(`platform2/login_manager/device_policy_service.cc:99`), written by `StorePolicyEx` on
`org.chromium.SessionManager`, which "Verifies the signature in @policy_blob and persists the
blob to disk. Device policy is stored in a root-owned location outside of any user's cryptohome.
It is verified with the device-wide policy key" (`dbus_bindings/org.chromium.SessionManagerInterface.xml:250-284`).
The key is the **owner's** (the first user) for consumer devices or the enterprise server's;
`PolicyKey` "holds the device owner's public key" and, once on disk, "blocks programmatic
replacement" (`policy_key.h:26-32`). Consumers `RetrievePolicyEx` and get
`PropertyChangeComplete` on persist (`session_manager_impl.cc:1495-1499`). Why signed rather than
root-writable: verified boot makes the RO image trustworthy and the RW stateful partition not;
signing lets the settings survive on RW without trusting root there (the platform's lockbox docs
state the same tamper-evidence goal; no login_manager document states it for this file — inferred
from `PolicyKey` and the D-Bus text). Assumptions: verified boot, a designated owner, a policy
model. BSD.

### 12.5 systemd-homed — user records

Per-user *preferences* — `timeZone`, `preferredLanguage`, `additionalLanguages`, `emailAddress`,
`iconName`, `location`, `shell`, `environment`, `umask`, `niceLevel`, `preferredSessionType` —
live in the JSON user record's `regular` section, "fields that shall apply unconditionally to the
user in all contexts, are portable and not security sensitive" (`systemd/docs/USER_RECORD.md:105-110`),
stored by the privileged daemon at `/var/lib/systemd/home/<user>.identity` and inside the home
(`~/.identity`, the LUKS header). Changed with `homectl update` → `UpdateHome`, polkit
`org.freedesktop.home1.update-home` (`auth_admin_keep`) or `update-home-by-owner` (`allow_active: yes`
— `src/home/org.freedesktop.home1.policy:42-59`). Read by everyone through varlink
`io.systemd.UserDatabase`; change notification only as `PropertiesChanged` on the Home object.
Assumptions: multi-user, portable homes. LGPL-2.1+. Relevant here as the freedesktop way to give
*per-user* preferences a privileged owner with an "owner may change own" polkit rule — not as a
device-wide store.

### 12.6 Android — SettingsProvider [external]

The generic privileged device store of the consumer platforms (`Global`/`Secure`/`System` tables,
per-user XML files `settings_{global,secure,system}.xml` via `AtomicFile`, writes coalesced 200 ms
and at most 2 s, a 40 KB quota per app — `SettingsState.java:123-127`; `WRITE_SECURE_SETTINGS`
per table and `isSettingRestrictedForUser` — `SettingsProvider.java:1521-1552`). Every setting
carries a runtime default (`Setting.defaultValue`, `isDefaultFromSystem`) and `reset()` returns
to it (`SettingsState.java:1951-1956`) — provenance and Reset again. Upgrades are numbered code
steps (`SETTINGS_VERSION = 226`, `onUpgradeLocked`); when the walk cannot reach the target the
database is **rebuilt and the loss recorded** in `Settings.Global.DATABASE_DOWNGRADE_REASON`
("Settings rebuilt! Current version …", `SettingsProvider.java:4000-4048`). A transient
(never-persisted) set exists — `Global.TRANSIENT_SETTINGS` — holding exactly one key, Wear OS's
`CLOCKWORK_HOME_READY` (`Settings.java:18289-18296`): a readiness flag, not a preference.
(lineage-22.2; verified from the raw files.)

## 13. Validation matrix, re-derived, and what falls out

| Spec | Mechanism | Comparables' positions | Status |
|---|---|---|---|
| §1 | artifact = compiled schema; no consumer defaults | GSettings; KConfigXT's goal; snapd gadget `defaults:`; steamos device TOML for ranges | **converging** |
| §1.1 | templates + instances, create-by-write, explicit delete, no GC | GSettings relocatable | **converging** |
| §2 | preference vs state roots | cosmic `new_state`; UCI `/var/state` for daemon-written state | **converging** |
| §2 | per-user preferences: sparse XDG stores, one session writer | dconf; homed for the identity subset | **converging** |
| §2, §6 | device-wide settings changeable at runtime | *desktops*: none, per-domain freedesktop services. *appliances*: snapd (one API, **per-key handlers**, admin/polkit `auth_admin`), steamos-manager (root half, **typed per-feature interfaces**, ranges from the device contract, hardware is the store), UCI (untyped, admin session), ChromeOS (owner-signed blob), Android (permissioned tables) | **exists — but never as a generic typed key-value writer; §13.1** |
| §2 | session (memory) stratum for *preferences* | UCI `-P` overlays and Android's transient set hold daemon **state/status**; GSettings memory backend is for debugging | **none for preferences; a state-only runtime layer has precedent — §13.5** |
| §3 | Set always writes; Reset; provenance | GSettings; Android `defaultValue`/`reset()`; snapd Transaction commit-at-once | **converging** |
| §4 | untouched keys follow the new default; explicit values survive | dconf, NixOS, snapd | **converging** |
| §4.1 | quarantine + generation tag + remount | desktops/UCI: default and leave the file; snapd: refuse a level downgrade; Android: rebuild and record the reason | **none — §13.3** |
| §5 | declared, idempotent, recorded migrations | kconf_update, gsettings-data-convert, uci-defaults, snapd patches, Android upgrade steps | **converging** |
| §5 | typed-op vocabulary in the artifact | KF5 DSL (retired); everyone current ships **numbered code** in the owning program | **converging against — §13.2** |
| §6 | apply transactions | desktops: signal only; snapd: validate-all → apply-all → commit, failure = not committed, status by Change; LuCI: commit then rollback unless confirmed; procd: reload triggers owned by init | **exists in three shapes, none the spec's — §13.4** |
| §7 | locks as system-layer facts; writability to the UI | dconf, NixOS, KConfig `[$i]`; ChromeOS policy | **converging** |
| §8 | bus shape; per-key coalesced signals | dconf; Android 200 ms coalescing; steamos `PropertiesChanged` | **converging** |
| — | Rust + zbus | cosmic-*, steamos-manager; measured §11 | **determined**; steamos's `rt-multi-thread` is the shape to avoid |

### 13.1 Device-wide settings — what the appliances actually built

The desktop stores refuse a runtime-writable system layer (§2); the appliances build one — and
every one of them builds it the same way: **one privileged API whose keys are each owned by
code that knows the domain**. snapd's `system.timezone` is a handler calling `timedatectl`;
steamos-manager's `TdpLimit1` is an interface whose range is a device-contract fact and whose
store is the hwmon attribute; UCI has no types at all and pushes validation into the service
that consumes the key. Nobody ships a privileged *generic typed* writer where the store validates
a range and the effect is someone else's problem — snapd tried the closest thing and made every
key a handler. Authorization is the administrator: root or polkit `auth_admin` (snapd), the admin
session (UCI), the owner's key (ChromeOS); steamos-manager's "anyone on the bus" is Valve's
single-seat posture and does not transfer (rule 3 is about the *user's* choices, rule 1 about
wheel + polkit per action).

Does Mura have such keys? Not in the contract today — every runtime preference there is
per-user. It **will** on the Frame: TDP/performance profile, fan, battery charge limit, Wi-Fi power
management are exactly steamos-manager's list on the same hardware class, and they are device
facts, not one wearer's preference. So the determination is two-part and both parts have
comparables:

- The **settings daemon (D7)** is the per-user store in the dconf shape — no system half, no
  `per-unit preference` row. Locked/declarative system values reach it through the artifact.
- Device-wide runtime knobs, when a target brings them, are a **separate privileged service in
  the steamos-manager/snapd shape**: typed per-domain handlers, ranges from `mura.hardware.*`,
  the hardware or the domain daemon as the store, polkit `auth_admin_keep` per action with wheel
  members granted where a comparable grants (`50-mura-timedate.rules` is the existing instance).
  Whether that service is one `org.mura.Manager1`-style program (steamos) or per-domain
  mini-services (systemd) is that rung's question, not D7's.

The open question that remains for the owner is only **whether `org.mura.Settings1` should
front those device knobs as keys** (snapd's single tree: `snap get system` shows everything) **or
leave them to their own interfaces** (steamos: a client asks `TdpLimit1`, not a settings key).
Both ship; the difference is whether the settings UI has one bus to talk to.

### 13.2 Migrations — code, numbered, recorded once

Every current comparable — kconf_update KF6, snapd `patchN.go`, Android `onUpgradeLocked`,
uci-defaults — migrates with **versioned code in the owning program**, recorded once (done-ids,
patch level, `SETTINGS_VERSION`, script deletion). The declarative vocabulary the spec proposes
is the KF5 DSL KDE retired plus gsettings-data-convert's key map. That is converging evidence
*against* the artifact carrying a migration language: **migrations are numbered Rust functions in
`mura-settingsd`, keyed by `(schema, fromVersion)`, recorded in the store header; the Nix side
only declares `schemaVersion`.** Applied (rule 8). What has no precedent and is dropped with it:
the build-time graph assertion (there is no graph when steps are linear code) and golden
downgrade fixtures (nobody tests downgrade; snapd refuses it).

### 13.3 Invalid values under a new schema — three positions, none quarantine

(a) desktops and UCI: resolve to the default, leave the file untouched, log — the user's copy
survives and an older schema reads it again; (b) snapd: refuse to *run* with state from a newer
level (`cannot downgrade`); (c) Android: rebuild the store and record why. The spec's §4.1 is
(a) plus an explicit surface (`ListQuarantine/Restore/Drop`) and remount-on-downgrade — which (a)
already gets for free by never rewriting. **Determination (converging on (a)):** invalid stored
values resolve to the default and are reported in `Get`'s provenance as `invalid` (so a UI can
say what was ignored); the file is not rewritten; the three quarantine methods leave. Where Mura
differs from all three: a *generation rollback* is a first-class NixOS operation, so the
downgrade case matters more here than on snapd — (a) handles it because the old value is still
on disk. Recorded, not asked: the comparables agree.

### 13.4 Apply — three shipping shapes

(a) desktop stores: signal; the effect's owner applies (KCM, gsd, cosmic-comp, localed
reloading PID 1). (b) snapd: **validate all, apply all, then commit** — a failed apply is
reported through the Change and the value is *not* persisted; side effects can be partial.
(c) LuCI: commit, then **roll back unless confirmed** within 90 s — for changes that can lock
you out. (d) the spec: durable `pending`, apply by a privileged agent, `failed` keeps the value.
(d) has no comparable and inverts (b)'s guarantee. For a *per-user preference* store the
consumers are session programs — (a) is the whole set of comparables and applies; `apply` stays
in the artifact as the KCM's "takes effect after restart" label. (b) and (c) belong to the
device-knob service of §13.1 (snapd/LuCI-class effects: a TDP that bricks, a network change that
disconnects) and are that rung's design input. **Determination for D7: (a).**

### 13.5 Session stratum — state has a runtime layer, preferences do not

UCI's `-P /var/state` and Android's `TRANSIENT_SETTINGS` are real non-persistent layers, and
both hold **status written by daemons** (assigned interface names; "home ready"), never a user's
preference override. GSettings' memory backend is for debugging. **Determination: the
`session` preference stratum is dropped; the artifact reserves `class = state, stratum = session`
(`$XDG_RUNTIME_DIR/mura/settings/`) as a hook with UCI's semantics — written by components,
never committed — to be opened by the first component that needs it.**

### 13.6 What this leaves for the owner

One question, with two shipping positions (§13.1): whether the per-user settings bus also
*fronts* device-wide knobs as read-through keys with `Set` proxied to their owning service
(snapd's single tree), or whether device knobs are reached only on their own interfaces
(steamos). Everything else in §13 converged once the appliance comparables were read, and is
applied in spec rev 3. The polkit-agent gap (registry #10) is not D7's under either position: it
is the prompt surface for every `auth_admin_keep` action on the device.
