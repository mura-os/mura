# 41 — Multi-user login landscape: greeter mechanics, XR precedents, NixOS account durability

**Question.** How should Mura do real multi-account + guest on an A/B image-based NixOS
headset: what do existing greeters/DMs actually do (enumeration, picker, last-user, guest
lifecycle), what do the XR/appliance platforms ship, and which NixOS mechanism keeps runtime-
created Unix accounts durable across A/B slot switches? Feeds
[multi-user.md](../architecture/multi-user.md) and [ADR 0018](../architecture/adr/0018-multi-user-accounts.md).

**Method.** Code study of the pinned clones (`accountsservice`, `gdm`, `gnome-shell`, `sddm`,
`greetd`, `gtkgreet`, `regreet`, `tuigreet`, `kscreenlocker`, `lightdm`; MANIFEST 2026-09-23)
plus web research for the XR platforms and NixOS mechanics (marked [external]). Paths relative
to each repo root.

---

## 1. How Linux greeters enumerate and pick accounts

### 1.1 AccountsService — the enumeration daemon behind the rich pickers

`org.freedesktop.Accounts` wraps the passwd database in enumeration/CRUD D-Bus API plus a
per-user property store. Enumeration merges four generators (`src/daemon.c:832`): direct
`fgetpwent` over `/etc/passwd`, systemd-homed `ListHomes`, the cache dir, and explicitly
requested users. The "human user" filter (`src/user-classify.c:120`) is exactly three
mechanisms: `uid >= MINIMUM_UID` (compile-time, default 1000), a hardcoded name blacklist
(`root`, `gdm`, `lightdm`, `gnome-initial-setup`…), and a shell check (`nologin`/`false`/not in
`/etc/shells` ⇒ hidden). `ListCachedUsers` additionally drops `SystemAccount=TRUE` users
(`src/daemon.c:1588`). Per-user persistence is one keyfile at
`/var/lib/AccountsService/users/<name>` (session, language, icon, SystemAccount) — greeter
metadata that must live on persistent storage on an A/B system or the picker forgets
faces/names each update. **`CreateUser` shells out to `/usr/sbin/useradd -m`**
(`src/daemon.c:1838-1856`, polkit-gated); a homed build calls `home1.CreateHome` instead.

### 1.2 GDM: AccountsService end to end

The gnome-shell greeter (`js/gdm/loginDialog.js:391,1501`) builds its picker from
`AccountsService.UserManager` with live add/remove/lock updates; avatars are the AccountsService
`IconFile`; list ordering is login frequency (wtmp scan in the accountsservice daemon).
Per-user session/language memory is stored *in AccountsService per user*
(`daemon/gdm-session-settings.c:289,372-400`), not greeter-side. Zero users ⇒ GDM's daemon
checks `ListCachedUsers` (`daemon/gdm-display.c:262-298`) and launches **gnome-initial-setup as
the session instead of the greeter** (`wants_initial_setup()`, `gdm-display.c:1134`) — the
architectural precedent for our dispatcher (first-run-onboarding §4.1), with a per-boot marker
file (`gdm.ran-initial-setup`).

### 1.3 SDDM: direct passwd iteration, daemon-owned state file

`src/greeter/UserModel.cpp:66-157`: `getpwent()` loop filtered by `MinimumUid`/`MaximumUid`
(defaults from `login.defs`), `HideUsers`, `HideShells`. No AccountsService D-Bus use at all
(only the on-disk icons path as an avatar fallback). Last user/session live in a daemon state
file `/var/lib/sddm/state.conf` (`src/daemon/Display.cpp:503-510`); the last user is force-added
to the model even if filtered and preselected.

### 1.4 greetd and its greeters: the account choice precedes authentication

`greetd_ipc` (`src/lib.rs:67`): `create_session(username)` initiates the PAM conversation *for
that username* — the picker's only job is choosing the name before any credential exchange; the
PIN prompt then arrives as a generic `auth_message`. This confirms the session-auth §5 shape
needs **zero greetd changes** for multi-account: the zxr greeter scene adds a picker and passes
the chosen name. The three greetd greeters split on enumeration exactly like GDM vs SDDM:
gtkgreet has no list (free-text username, no state); **regreet consumes AccountsService**
(`src/sysutil.rs:56-98`, zbus `ListCachedUsers`) and keeps `last_user` + per-user last-session
LRU in `/var/lib/regreet/state.toml`; **tuigreet iterates passwd** with a `login.defs`-derived
UID range (`crates/tuigreet/src/info.rs:226,252`) and keeps `lastuser`/`lastsession(-<user>)`
files under `/var/cache/tuigreet/`.

### 1.5 Lock-screen "switch user"

kscreenlocker only exposes the button; plasma-workspace's `SessionManagement::switchUser()`
locks, then calls **`org.freedesktop.DisplayManager.Seat.SwitchToGreeter`** (the logind-adjacent
DM interface; `libkworkspace/sessionmanagement.cpp:273-303`) — the DM spawns a new greeter and
logind VT-switches as a consequence of the new session. Never a direct `ActivateSession`. For a
one-seat HMD this collapses to logout→greeter (our lifecycle §3b already says so).

### 1.6 Guest sessions: the LightDM contract

The only real guest machinery in the corpus (`references/lightdm`): a distro-supplied
**`guest-account` script** with an `add`/`remove` contract — `add` invents a unique ephemeral
user and prints the username (`src/guest-account.c:57-94`); the daemon creates the account
lazily *when the guest session starts* and deletes it when the session stops
(`src/session.c:611,483-485`). Authentication is skipped: a dedicated greeter message
authenticates as guest with immediate `PAM_SUCCESS`, but the session still opens through a PAM
**autologin service** so a real PAM session/environment exists (`src/seat.c:1138-1143`).
Confinement hooks: a wrapper binary exists solely so MAC policy can target guest sessions.
SDDM's `SwitchToGuest` is an unimplemented stub. Verdict, precisely scoped: LightDM's
add/remove-script **lifecycle** and its real-PAM-session requirement are the proven template;
its **auth mechanics do not transfer** — the autologin-service selection lives in LightDM's
daemon, a seam greetd deliberately lacks, so a greetd-based guest needs its own gated PAM
branch (multi-user.md §4 is that translation).

---

## 2. XR/appliance precedents

### 2.1 Meta Quest: the household model

Up to **4 real accounts** (1 admin + 3), added by admin invitation; a **profile picker at device
startup**; per-account 4–16-digit passcode (forgotten ⇒ factory reset — matching our doc-12
posture); documented per-profile separation of passwords/messages/payments/progress (almost
certainly AOSP multi-user underneath — inference); App Sharing is admin-purchases-downward only.
Eye-tracking calibration is stored on-device per logged-in profile and deletable. **No consumer
guest mode** — the enterprise "Shared Mode" (MDM kiosk, session cleared + reboot at end) is the
only ephemeral story. [external: meta.com help 409013010128887, 195886835457001, 1198803198189099;
developers.meta.com multi-user blog; work.meta.com 1963330597356341]

### 2.2 Apple Vision Pro: one owner + the industry's best guest design

Still no multi-account in visionOS 26. The **Guest User** flow is the design to mirror: host
initiates/approves (from a nearby iPhone, or wearing the device first), optional per-app
allowlist, **5-minute don window** before auto-cancel, session ends on doff/lock; **guest eye/
hand calibration is transient** (erased at session end; optionally persisted to the *guest's
own* phone since visionOS 26, or kept on-device 30 days for one repeat guest); Optic ID/Apple
Pay/Persona blocked; approval-from-phone **auto-starts view mirroring** — supervision is part of
the flow, not an option. Honest fine print: the guest sees the owner's data inside any allowed
app — app gating, not a data universe. [external: support.apple.com 117742, 123024;
apple.com/legal/privacy/data/en/guest-user] *Design note: multi-user.md §4 adopts the transient
calibration and the don-window/auto-cancel, and records its deltas (standing toggle permitted;
grace-resumable doff; supervision optional rather than flow-mandatory) explicitly.*

### 2.3 AOSP multi-user: the substrate the headsets inherit

Real kernel-uid separation: `uid = userId × 100000 + appId` (`UserHandle`, `PER_USER_RANGE`),
per-user CE/DE **encryption keys** (FBE; CE keys unlocked by the user's credential), per-user
data roots (`/data/user/<id>` …), guest = a temporary secondary user (ephemeral by default, one
at a time). Android proves an appliance can do real per-person uid + per-person crypto without
a desktop identity stack. *Design note: Mura adopts the uid-separation half only —
per-account data-at-rest encryption (AOSP's CE-key half) is an explicit v1 non-goal in
multi-user.md §1, with homed as its designated carrier.*
[external: source.android.com multi-user + file-based encryption docs]

### 2.4 Steam Deck: the counter-example, confirmed

Single Unix `deck` user; multiple *Steam* accounts layered above; one shared device PIN; games
that write saves to fixed home paths collide across Steam accounts, with documented cloud-save
corruption incidents. Exactly the failure real accounts avoid. PICO: no multi-profile or guest
at all (community-confirmed absence). [external: steamdeck.com FAQ; community reports]

---

## 3. NixOS account durability on an A/B image

### 3.1 `mutableUsers` and the slot-switch trap

`update-users-groups.pl` regenerates `/etc/passwd`/`shadow`/`group` at every activation by
merging declared config with the **slot-local existing files** plus `/var/lib/nixos` id-maps
(which hold name→id memory only, never full records). Therefore: with `mutableUsers = true`,
imperative accounts survive rebuilds *on the same slot* but **die on an A/B slot switch** — the
new slot regenerates from declared config alone. Plain `mutableUsers = true` is a trap for this
image model — and, precisely stated, **the trap is the Perl script's regeneration semantics,
not the option value**: under userborn with persisted files (§3.2) the multi-user profile
*requires* `mutableUsers = true`, and that arrangement is safe. [external: nixpkgs
update-users-groups.pl]

### 3.2 userborn: the mechanism that makes persistence first-class

`services.userborn` (Rust replacement for the Perl script, in nixpkgs) manages all users
declaratively with diffing, and — decisive for us — **`passwordFilesLocation`**: the entire
`passwd`/`shadow`/`group` database is written to a chosen directory and symlinked from `/etc`
(subid files bind-mounted). Point it at persist-backed state and **the whole account database,
including provisiond-created accounts and PIN-password changes, survives slot switches by
construction**. Its `static = true` mode is asserted incompatible with switchable systems — not
our case. Three caveats the module source makes explicit (red-team-verified, load-bearing for
[multi-user.md §1.1](../architecture/multi-user.md)):

- **Imperative rows survive only in mutable mode** (`USERBORN_MUTABLE_USERS`); in immutable
  mode userborn *drains* any user absent from the declared config (shell → `nologin`, password
  locked) and its `ExecStartPost` **remounts the password files read-only** — provisiond could
  neither keep nor write accounts.
- **Unit ordering:** `userborn.service` has `DefaultDependencies=false`,
  `Before=sysinit.target`, no `RequiresMountsFor=` on its location, and an `ExecStartPre`
  `mkdir -p` — pointing `passwordFilesLocation` at a late mount silently creates a root-slot
  decoy database. The mount must be initrd-early plus a `RequiresMountsFor` drop-in.
- **userborn's own diff state is slot-local** (`/var/lib/userborn/previous-userborn.json`,
  a store symlink): after a slot switch the previous-config pointer dangles. Mild consequence
  (a declared-user removal between generations may not drain on the other slot), accepted and
  recorded in multi-user.md §1.1.

[external: github.com/nikstur/userborn; nixpkgs userborn.nix]

### 3.3 systemd-homed: architecturally ideal, NixOS-immature

Signed JSON user record in `~/.identity` (on the already-persistent `/home`), host state
confined to `/var/lib/systemd/home` (one directory for `/persist`), no `/etc/passwd` entry,
varlink userdb — built exactly for image-based OSes. But: NixOS support is
`services.homed.enable` + imperative `homectl` only (declarative tracking issue
NixOS/nixpkgs#301337 open, PoCs only), record-update friction, signing-key custody across
slots, and in LUKS mode the login credential *is* the disk passphrase — a 4–6-digit XR PIN is a
weak LUKS passphrase unless TPM-bound. [external: freedesktop homed docs; systemd USER_RECORD;
nixpkgs issue 301337]

### 3.4 AccountsService under NixOS

`services.accounts-daemon` ships a patched daemon; with `mutableUsers = false` NixOS sets
`NIXOS_USERS_PURE=true`, disabling the mutating methods entirely. Greeter-visible users must
have their shell in `environment.shells`; `/var/lib/AccountsService/` is more state to persist.
Since zxr owns its greeter UI, AccountsService is **not required** — SDDM/tuigreet prove direct
NSS enumeration with a UID range works, and our per-account metadata (display name, avatar,
last-session) can live in spatial's own state classes instead of a second daemon's keyfiles.

---

## 4. Recommendation (input to ADR 0018 — as revised for rev 3)

**Reading rule** (AGENTS.md / overview invariant 10, added after the rev-2 correction): §1 and
§3 — the Linux mechanics — are the *authorities* here. §2's closed platforms are context and
anti-patterns; their only admissible residue is mechanism-level (per-person calibration is real;
an ephemeral guest *session* is a good shape — whose actual Linux precedent is LightDM anyway).
Rev 2 of the design mistakenly promoted §2 to policy (an account cap, PIN-as-credential, an
"owner" role); ADR 0018 rev 3 rescinds all of it.

1. **Substrate: standard Linux accounts** — passwd/shadow, PAM, NSS, wheel + polkit. No cap
   (nothing in §1 caps accounts). The Steam Deck shared-home failure is the counter-model.
2. **Durability: userborn with `passwordFilesLocation` on persist-backed state** — ordinary
   PAM/greeter semantics; any account, however created (`useradd` over SSH included), survives
   A/B by construction. homed condition-watched (§3.3).
3. **Account admin is standard admin**: wheel + polkit per-action escalation; the in-headset UI
   is one convenience path (provisiond executes it); standard tools always work.
4. **Picker: NSS enumeration over the login.defs window** (SDDM/tuigreet pattern; fidelity
   note — SDDM's compiled-in fallback is 1000/65000 when login.defs is unparseable, tuigreet
   hardcodes 1000/60000) + free-text entry always available (gtkgreet) + spatial metadata/
   last-user state. `create_session(username)` needs no greetd changes (§1.4).
5. **Credentials: the Unix password, everywhere**; a PIN module is an optional stacked
   convenience (the fprintd model) because ray-keyboards are slow — never a replacement. GDM's
   `PAM_USER_UNKNOWN`-collapse is the uniform-failure precedent.
6. **Guest: the LightDM lifecycle** (ephemeral add/remove around the session; real PAM
   session) under a greetd-translated gated branch; optional, off by default; transient
   calibration is the one Vision Pro residue worth keeping, as mechanism.
