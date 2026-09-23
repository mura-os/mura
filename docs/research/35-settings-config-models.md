# 35 — Runtime settings and configuration models

**Status:** research input for component-registry gap 9; evidence and mapping, not a design decision.  
**Date:** 2026-09-23.

## 0. Question, constraints, and findings

spatial-os currently has no runtime user-settings path: its declared surfaces are the build-time `spatial.*` Nix contract and `spatial.xr.environment` variables injected into Monado ([component registry §6](../architecture/component-registry.md#6-service-plane)).

The missing service must eventually support the HMD settings surface in [ADR 0012 §4.5](../architecture/adr/0012-de-modularity-spinout-seams.md#4-the-zxr-private-protocol-surface-kept-minimal): IPD, render scale, refresh, recentering, passthrough, and the KWin-VR-derived general/input/head-gaze/follow-mode/advanced checklist.

The normative constraint is stronger than “have a settings daemon.” Interaction-policy constraint 9 requires **one source of truth for defaults and sizing**: the Nix module/contract must emit both runtime schema and runtime defaults ([composition §7.3](../architecture/zxr-shell-v2-composition.md#73-2d-windows-are-first-class-from-the-first-milestone-not-bolted-on)).

The forbidden failure is the KWin-VR pattern in which C++ follow-mode defaults and `.kcfg` defaults disagree ([research 31 §2.10](31-kwin-vr.md#210-follow-mode-the-complete-algorithm-and-the-policy-collision)).

The evidence supports four high-level findings:

1. **GSettings/dconf is the strongest semantic precedent:** compiled typed schema, separately compiled vendor defaults, per-key notification, layered site policy, and key writability.
2. **COSMIC is the closest implementation-shape precedent:** Rust types, versioned file-per-key RON, atomic replacement, explicit config/state separation, and one daemon distributing changed key names over the session bus.
3. **OSTree supplies the clearest reconciliation rule:** old default + local value + new default, preserving local divergence while advancing untouched defaults.
4. **No surveyed desktop generates its complete runtime schema from the OS module evaluator.** NixOS already has the missing producer mechanisms (`evalModules`, `nixosOptionsDoc`, `pkgs.formats.*`, `system.build`); combining those with a GSettings/COSMIC-shaped consumer is precedent composition, not invention.

The rebuild/runtime tension is narrow but real: it bites when a key has both an explicit runtime user value and a changed build default, or when a new schema invalidates that value. It does not justify copying every effective value into mutable storage.

## 1. “XDG” grounding and persistence vocabulary

“XDG” here means the freedesktop **Cross-Desktop Group Base Directory Specification**, not the `xdg_*` Wayland protocol namespace and not xdg-desktop-portal. This is the three-way distinction made in [research 30 addendum A6](30-wayland-de-anatomy-protocol-seams.md#addendum-a6--xdg-disambiguation-and-cross-desktop-specs).

The [Base Directory Specification 0.8](https://specifications.freedesktop.org/basedir/latest/) gives these paths distinct meanings:

- `$XDG_CONFIG_HOME` (default `~/.config`) is user-specific **configuration**.
- `$XDG_STATE_HOME` (default `~/.local/state`) is restart-persistent state that is less portable or important than user data: history, current layout, open files, and undo state are examples.
- `$XDG_DATA_HOME` (default `~/.local/share`) is user data.
- `$XDG_RUNTIME_DIR` is private, local, login-lifetime IPC/state and must not survive full logout or reboot.
- `$XDG_CONFIG_DIRS` and `$XDG_DATA_DIRS` are ordered system search paths; a format using them must define replacement or merge behavior.

A user-selected render scale, follow-mode preference, or hand-cutout policy is therefore **configuration** and naturally belongs below `$XDG_CONFIG_HOME`, not `$XDG_STATE_HOME`.

An automatically remembered workspace, last settings page, or daemon checkpoint is state and belongs below `$XDG_STATE_HOME`. [`cosmic-config`](https://raw.githubusercontent.com/pop-os/libcosmic/1dc9aa37/cosmic-config/src/lib.rs) implements exactly this split: `Config::new()` uses the XDG config directory; `Config::new_state()` uses the state directory for values that change regularly without user action.

Neither directory fits per-unit calibration. ADR 0007 requires lens/distortion calibration and device credentials in `/var/lib/spatial/` or vendor persist because greeter, lock, and early rendering need them before login; per-user IPD is applied after login ([ADR 0007](../architecture/adr/0007-session-greeter-lock.md#cross-cutting-requirements)).

Vocabulary used below:

- **build fact/default:** immutable artifact selected by Nix evaluation;
- **per-unit system state:** device-specific mutable state under `/var/lib/spatial/`;
- **per-user preference:** intentional user choice under `$XDG_CONFIG_HOME`;
- **per-user operational state:** restart-persistent, non-preference state under `$XDG_STATE_HOME`;
- **session-ephemeral:** `$XDG_RUNTIME_DIR` or memory only.

## 2. GSettings and dconf

### 2.1 Schema, values, and what GNOME apps expect

GSettings is the typed API/schema layer; dconf is its usual GNOME backend. A `.gschema.xml` declares schema ID/path, keys, GVariant types, defaults, ranges/enums, summaries, descriptions, and children. `glib-compile-schemas` produces `gschemas.compiled`; runtime lookup scans `glib-2.0/schemas` under `$XDG_DATA_HOME` and `$XDG_DATA_DIRS` ([compiler manual](https://man.archlinux.org/man/core/glib2/glib-compile-schemas.1.en), [GSettings API](https://docs.gtk.org/gio/class.Settings.html)).

The local GNOME Shell reference is representative: [`org.gnome.shell.gschema.xml.in`](../../references/gnome-shell/data/org.gnome.shell.gschema.xml.in) declares typed defaults for extensions, favorites, switcher behavior, keybindings, world clocks, weather, and app-picker layout. Shell constructs settings by schema ID and reads typed values; it does not parse an application-owned preferences file.

With dconf's normal `user-db:user` profile, the writable binary database is `$XDG_CONFIG_HOME/dconf/user` ([dconf(7)](https://man.archlinux.org/man/dconf.7.en)). Reads are local/mmap-oriented; a stateless D-Bus writer serializes writes, emits changes, and may exit when idle ([dconf README](https://github.com/GNOME/dconf/blob/main/README)).

This separation is directly relevant: “schema generated by Nix” need not put user values in the Nix store. GSettings proves that immutable schema/default artifacts and mutable per-user values can be independently located and resolved at read time.

GNOME applications expect:

- the named schema to exist at runtime; constructing settings for an absent schema fails ([schema-source API](https://docs.gtk.org/gio/type_func.SettingsSchemaSource.get_default.html));
- typed lookup with a discoverable default and range;
- reset semantics that reveal the next lower layer;
- an explicit “is this writable?” result;
- key- or group-granular live notification;
- optional delayed apply for grouped edits ([GSettings API](https://docs.gtk.org/gio/class.Settings.html)).

The local Shell tree subscribes to keys such as `changed::dynamic-workspaces`, `changed::color-scheme`, and `changed::enabled-extensions`. [`st-settings.c`](../../references/gnome-shell/src/st/st-settings.c) receives changed key names and updates only corresponding interface, mouse, accessibility, or lockdown properties.

dconf's writer emits a prefix plus changed relative key names. Its GSettings backend translates these to single-key, path, or multi-key notifications and emits value changes when writability changes ([backend source](https://github.com/GNOME/dconf/blob/main/gsettings/dconfsettingsbackend.c)).

### 2.2 Vendor defaults, administrator defaults, and lockdown

Application defaults live in schemas. Distribution/vendor overrides are `*.gschema.override` keyfiles beside schema sources; `glib-compile-schemas` folds them into the compiled artifact. Groups are schema IDs, values are serialized GVariant, and conventional `00_`–`99_` prefixes establish increasing priority ([compiler manual](https://man.archlinux.org/man/core/glib2/glib-compile-schemas.1.en)).

GNOME says these overrides are for distributors changing defaults, while dconf databases are for site administration and mandatory settings ([administrator guide](https://help.gnome.org/system-admin-guide/overrides.html)). The local [`00_org.gnome.shell.gschema.override`](../../references/gnome-shell/data/00_org.gnome.shell.gschema.override) changes Mutter's GNOME-session defaults without patching Mutter's schema.

A dconf profile is an ordered database list. The first DB is writable (normally the user DB); later system DBs provide read-only defaults ([profile guide](https://help.gnome.org/system-admin-guide/dconf-profiles.html)). Locks under `/etc/dconf/db/<db>.d/locks/` make selected paths non-writable; the locking system layer defeats a higher user value ([administrator guide](https://wiki.gnome.org/Projects/dconf/SystemAdministrators)).

The effective layering is:

```text
application schema default
  -> compiled distribution/vendor override
  -> site/system dconf default
  -> user value, unless a system lock makes the key non-writable
```

Lockdown is observable, not just write failure. GSettings exposes writability and emits `writable-changed`; UI bindings can disable controls. For an appliance profile, this is the cleanest precedent: a lock is policy over the same key/schema, not a second hidden default in a consumer.

### 2.3 The relocation problem

A fixed-path schema names one backend subtree. A relocatable schema omits `path` and must be instantiated with `g_settings_new_with_path()`; one schema can describe many accounts, windows, or other instances ([GSettings relocatable schemas](https://docs.gtk.org/gio/class.Settings.html)). CLI operations similarly require `SCHEMA:PATH` ([gsettings(1)](https://man.archlinux.org/man/core/glib2/gsettings.1.en)).

Vendor overrides are keyed by schema ID, so they change the schema default for **every** relocated instance; they cannot choose one dynamic path. Per-instance provisioning must target the actual backend path. The practical failure is visible in custom-keybinding deployment: declaring the fixed parent list does not populate each relocated child instance ([reported example](https://lists.snapcraft.io/archives/foundations-bugs/2017-October/334986.html)).

For spatial objects, “one schema for every controller/window/place” still needs stable instance IDs, lifecycle, garbage collection, and migration. Schema generation cannot invent object identity.

## 3. KConfig, KConfigXT, and KDE KCMs

### 3.1 Cascading files and `kdeglobals`

KConfig presents groups and keys over INI-style files. A regular application config such as `myapprc` is merged across the `QStandardPaths` config hierarchy: the user file is normally `$XDG_CONFIG_HOME/myapprc`, while system files come from `$XDG_CONFIG_DIRS` ([KConfig introduction](https://develop.kde.org/docs/features/configuration/introduction/)).

Most applications have their own `*rc` file. `kdeglobals` is a separate shared config blended into app config by default; callers can request `NoGlobals`, `NoCascade`, or `FullConfig` ([KConfig API](https://api.kde.org/kconfig.html)). KConfig is therefore a merge API over many files, not one desktop settings database.

System/organization defaults are not copied into user files. With no user entry, a changed system default becomes effective; a user entry remains an override. Keys, groups, or whole files can be immutable via `[$i]` ([KDE administration guide](https://userbase.kde.org/KDE_System_Administration/Configuration_Files)).

`kwriteconfig6` is correspondingly a file/group/key writer, not a universal schema-aware daemon. Without `--file` it targets `kdeglobals`; with `--file` it targets one application's config ([tool source](https://github.com/KDE/kde-runtime/blob/master/kreadconfig/kwriteconfig.cpp)).

### 3.2 KConfigXT schema and KCM pattern

KConfigXT's `.kcfg` XML describes groups, keys, types, enums, defaults, bounds, labels, tooltips, and change signals. The build generates typed C++ getters/setters, providing one application source of truth ([KConfigXT guide](https://develop.kde.org/docs/features/configuration/kconfig_xt/)).

This resembles constraint 9 in direction but not ownership: `.kcfg` emits application code, while spatial-os requires the evaluated Nix contract to emit schema/defaults for all trusted consumers. KConfigXT also allows `code="true"` C++ defaults, which would recreate the forbidden compiled-in policy channel.

Plasma System Settings pages are KConfig Modules. A managed KCM owns a generated config object, tracks dirty state, supports Apply/Reset/Defaults, and can show whether a value is defaulted or immutable ([KConfigXT KCM pattern](https://develop.kde.org/docs/features/configuration/kconfig_xt/), [KCM guide](https://develop.kde.org/docs/features/configuration/kcm/)).

ADR 0012's KWin-VR page taxonomy is thus a settings-coverage checklist, not an argument to adopt KConfig storage.

### 3.3 Notification is opt-in

`KConfigWatcher` emits `configChanged(group, names)` and reparses before delivery, but only when a writer used `KConfigBase::Notify`; this flag uses D-Bus and implies persistence ([watcher API](https://api.kde.org/kconfigwatcher.html), [write flags](https://api.kde.org/kconfigbase.html)).

The local KWin reference watches `kwinrc`, `kdeglobals`, input, night-light, and accessibility files this way ([`options.cpp`](../../references/kwin/src/options.cpp), [`workspace.cpp`](../../references/kwin/src/workspace.cpp)).

This is weaker than dconf or COSMIC as a universal mutation path: raw edits and writers omitting `Notify` do not satisfy live update. A spatial daemon cannot equate “a file eventually changed” with a validated key mutation.

## 4. cosmic-config and cosmic-settings-daemon

### 4.1 Versioned file-per-key storage

`cosmic-config` identifies config by string ID plus integer version. `Config::new(name, version)` finds system defaults below the `cosmic/` data hierarchy (normally `/usr/share/cosmic/<name>/v<version>/`) and user overrides below `$XDG_CONFIG_HOME/cosmic/<name>/v<version>/` ([source](https://raw.githubusercontent.com/pop-os/libcosmic/1dc9aa37/cosmic-config/src/lib.rs)).

Each key is a separate RON file. `get()` reads local first and falls back to the same system-default key; `get_local()` and `get_system_default()` expose the layers separately. `Config::new_state()` uses `$XDG_STATE_HOME/cosmic/...` and has no system-default layer ([same source](https://raw.githubusercontent.com/pop-os/libcosmic/1dc9aa37/cosmic-config/src/lib.rs)).

Writes serialize one key and atomically replace its file. A transaction queues multiple keys but currently loops over independently atomic replacements; its own source notes the missing whole-transaction apply. It is crash-safe per key, not all-keys atomic.

Types and fallback defaults also exist in Rust. The local `CosmicCompConfig` derives `CosmicConfigEntry`, declares `#[version = 1]`, and implements Rust `Default` ([config crate](../../references/cosmic-comp/cosmic-comp-config/src/lib.rs)).

That is useful typing but not constraint-9-safe alone: system-default files and Rust `Default` can diverge. A spatial-os producer would have to eliminate or mechanically check the second default channel.

### 4.2 Watchers and daemon distribution

Direct `Config::watch()` recursively watches the user directory, filters atomic-write temporaries, and reports changed relative keys. `cosmic-comp` inserts `ConfigWatchSource` into calloop and reloads named compositor, shortcut, window-rule, and toolkit keys ([local consumer](../../references/cosmic-comp/src/config/mod.rs)).

The session daemon centralizes watches. libcosmic calls `watch_config(id, version)` or `watch_state`, receives a D-Bus object, listens for changed-key signals, reloads those keys, and reconnects with backoff if the daemon disappears ([D-Bus source](https://raw.githubusercontent.com/pop-os/libcosmic/66263a76/cosmic-config/src/dbus.rs)).

The daemon is a notification concentrator; values remain recovery-readable files rather than living only inside a mandatory broker.

Transferable strengths:

- inspectable, versioned system defaults and sparse per-user overrides;
- atomic file replacement and distinct preference/state roots;
- named-key events through the session bus;
- consumers recover by rereading authoritative files.

Material gaps are complete schema introspection, generic range/enum validation, first-class lockdown, provenance across changed build defaults, and cross-key transactional atomicity.

## 5. systemd-homed and userdb: identity, not preference storage

systemd JSON user records contain account identity, authentication material, resource controls, environment, locale/time zone/session preferences, storage, per-machine overrides, signatures, bindings, and runtime status. `systemd-homed` embeds portable portions in the home and augments them with host-local data ([JSON User Records](https://systemd.io/USER_RECORD/), [home format](https://github.com/systemd/systemd/blob/main/docs/HOME_DIRECTORY.md)).

`systemd-userdbd` multiplexes records over Varlink and reads `.user` drop-ins from `/usr/lib/userdb`, `/etc/userdb`, `/run/userdb`, and `/run/host/userdb` ([systemd-userdbd(8)](https://man7.org/linux/man-pages/man8/systemd-userdbd.service.8.html)).

This is plausible only for account attributes that travel with identity: locale, time zone, preferred session, perhaps a profile selector. No evidence was found of GNOME, KDE, or COSMIC storing ordinary desktop preferences there. Signed whole-record privileged updates are also a poor match for frequent per-key UI writes.

Mapping: userdb may supply identity inputs to defaults/policy; it does not replace the settings daemon.

## 6. Image-based OS precedents

### 6.1 Silverblue/OSTree

OSTree deployments place read-only vendor defaults in `/usr/etc` and keep deployment-specific `/etc` writable. Upgrade performs:

```text
old /usr/etc default + current /etc + new /usr/etc default -> new /etc
```

Files unchanged from the old default advance to the new default; locally changed files survive. `ostree admin config-diff` reports local divergence ([atomic upgrades](https://ostreedev.github.io/ostree/atomic-upgrades/), [deployment model](https://ostreedev.github.io/ostree/deployment/)).

This is the clearest rebuild-semantic precedent but too coarse as settings storage: one edit pins a whole file, while a settings service can preserve divergence per key. Its key lesson is provenance: old default is needed to distinguish an override from a value that merely equaled the prior effective value.

### 6.2 SteamOS

SteamOS keeps an A/B read-only root while making `/etc` an OverlayFS with a lower image layer and an upper directory under `/var/lib/overlays/etc/upper`. `/home` and selected `/var` paths persist; root edits after `steamos-readonly disable` may be replaced by updates ([partition analysis](https://github.com/randombk/steamos-teardown/blob/master/docs/partitions.md), [update analysis](https://iliana.fyi/blog/build-your-own-steamos-updates/)).

This proves an appliance can expose mutable preferences without a mutable base. It does not solve schema, notification, or semantic merge; OverlayFS copy-up hides a new lower file as a whole. It is filesystem-layout precedent, not settings-daemon precedent.

### 6.3 Android

Android separates app-private preferences from platform settings. App-owned values use `SharedPreferences`; global/secure/system device preferences go through SettingsProvider with per-user and write-permission rules ([SharedPreferences](https://developer.android.com/training/data-storage/shared-preferences), [Settings API](https://developer.android.com/reference/android/provider/Settings)).

Modern SettingsProvider keeps settings in memory and asynchronously persists per-type XML in each user's system directory; the old SQLite helper is frozen ([provider source](https://android.googlesource.com/platform/frameworks/base/+/91fc934bb2e5ea59929bb2f574de6db9b5100745/packages/SettingsProvider/src/com/android/providers/settings/SettingsProvider.java), [migration](https://android.googlesource.com/platform/frameworks/base/+/683914bfb13908bf380a25258cd45bcf43f13dc9)). Clients observe setting URIs through `ContentObserver`.

Product builds customize resource defaults through build-time overlays; Android also has partition-prioritized runtime overlays with mutability controls ([build overlays](https://source.android.com/docs/setup/create/new-device), [RRO policy](https://source.android.com/docs/core/runtime/rros)). Permissions, secure/global table rules, user restrictions, and device policy provide lockdown.

Android is strong precedent for a privileged system-settings API distinct from app preferences and for image/vendor defaults feeding a runtime provider. It is weaker on one inspectable typed schema: keys are spread across framework constants, resources, provider code, and migrations.

## 7. NixOS interplay: the payload core

### 7.1 The module evaluator as schema producer

A NixOS option declaration carries name/location, type, default, description, example, read-only status, and declaration provenance. `nixosOptionsDoc` turns evaluated options into machine-readable JSON and rendered docs ([implementation](https://github.com/NixOS/nixpkgs/blob/defb5cb01c4bc40e87f6918446e8916395634691/nixos/lib/make-options-doc/default.nix)). This is the strongest direct precedent for **schema from the build system**.

`pkgs.formats.json {}` supplies a merge-compatible Nix type and `generate` function producing an evaluated Nix-store artifact; modules conventionally expose it through `/etc` or a unit ([settings-option guide](https://github.com/NixOS/nixpkgs/blob/48273d596109a034cf154e450dee69705ca2d620/nixos/doc/manual/development/settings-options.section.md)).

`system.build` is an extensible attrset of derivations, so a module may expose independently buildable schema/default artifacts ([definition](https://github.com/NixOS/nixpkgs/blob/456e8a9468b9d46bd8c9524425026c00745bc4d2/nixos/modules/system/build.nix)).

The current spatial contract already declares typed defaults and assertions in [`lib/contract/default.nix`](../../lib/contract/default.nix); [registry §7.1](../architecture/component-registry.md#71-contract-namespace--component-map) maps namespaces to consumers. The missing step is projection: identify which evaluated options are build facts, runtime keys, system-state locations, or locks, then emit the runtime view.

A generated key record could expose:

```text
stable ID; type; enum/range; build default; description;
stratum; writability/lock policy; apply/restart semantics; schema version
```

This is a capability mapping, not a settled format. GSettings proves type/range/default/writability; KConfigXT proves UI metadata/generated accessors; COSMIC proves versioned filesystem keys.

### 7.2 One default channel and sparse user values

Constraint 9 forbids:

```text
Nix default A -> generated runtime artifact says A
             \-> compositor/settings UI independently compiles default B
```

Generated accessors may have no independent normal fallback: absent user data resolves to the generated build default. A hard safety fallback for a corrupt/missing artifact must be an explicit failure mode, not a second normal policy default.

The useful layered model is:

```text
evaluated build default -> optional profile/vendor override -> optional explicit user value
```

The first two may already be Nix priorities (`mkDefault`, device module, image/profile module) resolved before generation. A separate runtime vendor layer is useful only if provenance, reset, or appliance policy needs it.

User storage must remain sparse. If the user never wrote a key, a rebuild changing its default should change the effective value. Copying all effective defaults into the user DB at first login loses that property.

### 7.3 Home Manager shows the ownership collision

NixOS can generate system dconf profiles containing defaults and locks ([`programs.dconf`](https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/programs/dconf.nix)). Home Manager instead writes typed values into the user's dconf DB during activation, records managed keys per generation, resets keys removed from the new generation, then runs `dconf load` ([module](https://github.com/nix-community/home-manager/blob/master/modules/misc/dconf.nix)).

That is declarative convergence, but a runtime edit to a Home-Manager-owned key is overwritten at next activation. Activation also must reach the correct user D-Bus/dconf context; historical failures show that system activation and a live user settings bus are not equivalent ([issue 2106](https://github.com/nix-community/home-manager/issues/2106)).

The evidence distinguishes two legitimate modes:

- **declaratively owned key:** rebuild/switch reasserts the Nix value;
- **runtime-owned key with build default:** rebuild changes only the fallback and preserves an explicit user value.

The eventual schema must mark this ownership; otherwise “Nix emits defaults” silently becomes “Nix overwrites preferences.”

### 7.4 Generations, specialisations, and live consumers

`nixos-rebuild switch` builds `config.system.build.toplevel`, adds a profile generation, activates it, and makes it the boot default ([nixos-rebuild](https://wiki.nixos.org/wiki/Nixos-rebuild)). Activation updates `/etc`, then systemd computes reload/restart actions ([switch internals](https://github.com/NixOS/nixpkgs/blob/master/nixos/doc/manual/development/what-happens-during-a-system-switch.chapter.md)).

A new store artifact is therefore insufficient: long-running consumers need generation-change notification, reload, or restart. Per-key user writes must never mutate the store artifact.

Specialisations are additional evaluated configurations selectable at boot or runtime switch ([guide](https://wiki.nixos.org/wiki/Specialisation)). They can emit different schema/default/lock artifacts for appliance, development, or hardware modes. They do not merge user changes; switching specialisation exercises the same reconciliation rules.

### 7.5 Where rebuild versus runtime writes bites

For key `k`, retain:

```text
D_old(k) = prior generation's effective build default
D_new(k) = new generation's effective build default
U(k)     = absent, or an explicit user value
```

Then:

- absent `U(k)` means the effective value advances from `D_old` to `D_new`;
- present and valid `U(k)` remains effective across rebuild;
- reset removes `U(k)` and reveals `D_new`;
- a newly locked key exposes the enforced system value and non-writability;
- a new schema rejecting `U(k)` requires migration/quarantine/reset/error policy;
- renamed, split, or merged keys require schema-version migration;
- cached `D_old` requires consumer reload/restart at activation.

The hard case is not “NixOS versus runtime settings” generally. It is whether `U(k)` denotes durable intent when `D_new` changes, and what happens when the new schema cannot represent it. OSTree distinguishes this per file; a settings service can do it per key.

## 8. Mapping to spatial-os

### 8.1 Strata and matching precedents

| Spatial stratum | Examples | Persistence | Best-fitting precedent |
|---|---|---|---|
| Build-time facts/defaults | panel geometry, supported refresh set, backends, profile defaults | Nix store/current generation | NixOS options + generated schema; GSettings compiled schema/default semantics |
| Per-unit system state | lens/distortion calibration, device enrollment, protected calibration domains | `/var/lib/spatial/` or vendor persist | ADR 0007; Android protected device settings |
| Per-user preferences | IPD preference, render scale, passthrough, comfort/input/follow mode | `$XDG_CONFIG_HOME/spatial/...`, sparse overrides | GSettings layering; COSMIC config files |
| Per-user operational state | last workspace/page, restore bookkeeping, recents | `$XDG_STATE_HOME/spatial/...` | COSMIC `new_state`; XDG state definition |
| Session-ephemeral | recenter transaction, capability probe, subscriptions, pending edits | memory or `$XDG_RUNTIME_DIR` | session D-Bus/daemon patterns |

This corrects two tempting conflations: `$XDG_STATE_HOME` is not the default for preferences, and `/var/lib/spatial/` must not absorb user choices merely because HMD hardware consumes them.

### 8.2 Evidence-bounded minimum daemon shape

Without selecting names, formats, or protocol, precedents bound a minimum:

1. **Schema source:** generated from evaluated `spatial.*` declarations, carrying stable IDs, type/range/enum, default, description, stratum, version, and writability. Consumers have no independent normal defaults.
2. **Storage resolver:** immutable generated defaults plus sparse preference overrides; separate state root; explicitly separate privileged per-unit state. Atomic replacement per key and versioned migration are the COSMIC baseline.
3. **Mutation authority:** one session service validates writes, resets, and grouped changes against the active schema. Recovery-readable files may remain authoritative, but direct unvalidated edits cannot be the notification contract.
4. **Notification bus:** session-bus events naming changed keys/groups, writability changes, and generation. GSettings/dconf provides semantics; COSMIC provides the Rust daemon shape.
5. **Lockdown:** appliance-profile policy marks keys non-writable and supplies enforced values; the development profile may leave the same schema writable. UI must introspect this.
6. **Rebuild handoff:** activation publishes the new generation, then consumers reload or restart. Untouched keys follow defaults; valid explicit values survive; invalid values enter an explicit migration/error path.

This minimum is implied by precedent and existing consumers; it does not select dconf, KConfig, a COSMIC-derived crate, or a new implementation.

### 8.3 How the zxr HMD settings API rides it

ADR 0012 §4.5 keeps HMD/runtime configuration off monitor output-management protocols. Its narrow zxr API is an application/control seam backed by Monado capability checks; it should not become an independent persistence store ([ADR 0012](../architecture/adr/0012-de-modularity-spinout-seams.md#4-the-zxr-private-protocol-surface-kept-minimal)).

Mapped flow:

```text
settings UI / KCM-like client
  -> settings mutation API (validate, persist/reset, report lock)
  -> changed-key event
  -> owning component applies through capability-checked zxr/Monado mechanism
  -> applied/rejected/deferred status returns to UI
```

Some HMD “settings” are not preferences. Panel modes and calibration are facts/system state; user refresh **policy** or render scale may be preferences; recenter is usually an action/session transition; tracking quality is status. One flat key-value bucket would erase ownership.

The KWin-VR taxonomy in [research 31 §2.7](31-kwin-vr.md#27-ops-surface-kcm-preflight-leasing) enumerates the surface. Its divergent follow-mode defaults are precisely why persistence must resolve through generated schema rather than KCM and compositor constants.

### 8.4 Open questions for design

- Which `spatial.*` options are exportable runtime keys, and how is that annotation represented without confusing facts and preferences?
- Is the active artifact JSON/CBOR, generated Rust, GSettings schema, or multiple mechanically checked representations?
- Are user preferences file-per-key, transactional DB, or journal plus snapshots?
- Which groups need true all-or-nothing commits rather than per-key atomic replacement?
- What stable instance-ID model replaces relocated paths for controllers, places, windows, and devices?
- Do preferences survive every profile/specialisation, or are values namespaced by profile?
- How are rename/split/merge migrations declared and tested?
- When a schema rejects a value, is it quarantined, reset with notification, clamped, or allowed to block activation?
- Which keys are declaratively reasserted like Home Manager, versus runtime-owned with a Nix fallback?
- Can appliance lockdown change live, and what acknowledgment proves consumers stopped using the former value?
- Which settings apply immediately, at an OpenXR frame boundary, after restart/relogin, or only after reboot?
- How does UI distinguish preference, action, status, calibration workflow, and capability absence?
- What audit view shows build default, profile override, user override, effective value, lock owner, and apply result?
- Does the future conversational Nix editor in [design-backlog scope decisions](../architecture/design-backlog.md#scope-decisions) edit build configuration, make runtime writes, or explicitly offer both?

## 9. Bottom line

The strongest precedent for schema-from-build-system is **NixOS's evaluated option metadata and artifact generators**, with GSettings proving that compiled schema/vendor defaults can remain separate from mutable user values. No surveyed DE already provides the exact Nix-module-to-runtime-registry pipeline.

The best-supported storage/notification shape is **COSMIC-like sparse, versioned, atomically replaced user keys under `$XDG_CONFIG_HOME`, separate `$XDG_STATE_HOME` operational state, and a session D-Bus daemon emitting per-key changes**, augmented with GSettings-style validation, reset/writability, and profile lockdown. Per-unit calibration stays in `/var/lib/spatial/`.

The rebuild/runtime tension bites at explicit user overrides and schema migration: untouched values follow the new generated default; valid explicit intent survives; locks and invalid values need visible policy. Copying defaults into user storage or compiling fallback defaults into consumers destroys that distinction and violates constraint 9.
