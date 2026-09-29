# Multi-user: standard Linux accounts, the XR greeter picker, and the guest session

**Status:** accepted design, rev 3 (2026-09-23; rev 3.7 2026-09-25: `50-mura-timedate.rules`
and the faillock wording sourced to Linux-PAM, research/56). Rev 3 is the **Linux-native reframe**: rev 2
imported policy from closed consumer platforms (an account cap, PIN-as-the-credential, an
"owner" role); all of it is removed per [AGENTS.md](../../AGENTS.md) / overview invariant 10.
Rev 2's engineering corrections (userborn boot ordering, guest PAM gating, sweep ordering, PAM
input hardening) survive — they were correctness, not policy. **Rev 3.1 (2026-09-24):** the
first account is declared in the image and asserted at build; the runtime account-bootstrap
screen is gone ([ADR 0017 rev 2](adr/0017-first-run-provisioning.md)); **one credential** — the
separate `pam_mura_pin` module is withdrawn, a PIN is a numeric password with a rendering hint
(§3); the greeter's standard furniture and the input floor are normative (§2). **Rev 3.3
(2026-09-24, D1):** the account database persists through a **mutable `/etc` overlay whose
upper layer is on `/persist`** — the `/persist/userdb` + symlink design is withdrawn (§1, §1.1).
**Rev 3.4 (2026-09-24, D2):** the credential hint is an owner-checked file in a sticky
directory (no mirror unit); sshd carries no `nullok` and never `PermitEmptyPasswords`; the
faillock rules are spelled out with `conf=` (§3, §3.1). **Rev 3.6 (same day, D5):** `mura-lock`
without `nullok`, faillock for an unprivileged caller (§3.1). **Rev 3.5 (same day):** the
regular-Linux-PC correction — sshd with upstream defaults on every interface (the key-only
scoping withdrawn), the password is the user's choice, Cockpit gone, the `mura-setup` polkit
rule set added and the "no rule for sessions" principle stated (§3, §3.1). **Rev 3.8
(2026-09-29):** the network menu leaves the greeter's furniture and `50-mura-greeter-network.rules`
is withdrawn — Wi-Fi before a user exists is onboarding's, after login the session panel's (§2,
§3.1; owner's ruling).
**Decision record:** [ADR 0018](adr/0018-multi-user-accounts.md).
**Evidence base:** [research/41](../research/41-multi-user-login-landscape.md) — its Linux
mechanics sections (§1, §3); the closed-platform sections are context and anti-patterns.
**The frame:** multi-user on Mura **is standard Linux multi-user** — passwd/shadow, PAM,
NSS, wheel + polkit. The XR layer adds exactly four things: per-user calibration, the spatial
greeter scene (operable at the head-aim + button input floor), a digit-pad *rendering* for
numeric passwords, and the A/B durability wiring. Nothing else is special.
**Budget impact** (overview invariant 9): login/enrollment-time work; nothing on the frame path.

## 1. Accounts

Ordinary Unix accounts. **No cap** — no Linux system limits how many accounts its administrator
creates, and neither does this one. Practical notes are notes, not limits: the picker scrolls
past a handful of entries, and `/home` sizing/quotas are the administrator's business (§8).

- **Creation is standard.** `useradd`/`userdel`/`passwd` over SSH or a TTY work, period —
  because `/etc` itself is persistent (the overlay below), standard tools operate on the real
  files natively. The in-headset settings UI is a convenience path for the same operation: a
  polkit-gated admin action that `mura-provisiond` executes (it is *a* path, not an
  authority — the only place provisiond remains load-bearing is the guest token gate, §4).
- **Admin is wheel + polkit.** No "owner" role exists. The first account is **declared in the
  image** — the image is the installation ([first-run-onboarding.md §1](first-run-onboarding.md))
  — and is a normal user in `wheel`, like every desktop installer's first account; a greeter
  image without one fails to build (`mura.xr.session.allowNoDeclaredAccount` is the escape
  hatch, the `users.allowNoPasswordLogin` pattern). No runtime "create the first account"
  screen exists. Privilege is per-action escalation (sudo in a terminal, polkit prompts in UI,
  authenticated requests to root daemons); **no session — autologin, greeter, or logged-in —
  ever carries ambient root**, on any profile. The appliance profile's `autoLogin = "mura"`
  (the default image's declared user) names an ordinary unprivileged account. Resetting another
  user's forgotten credential is `sudo passwd <user>`-class standard admin — not a designed
  feature of this OS, and never factory reset.
- **Durability across A/B (the one genuinely novel problem):** on an image-based A/B system,
  `/etc/passwd` is slot-local, so conventionally-created accounts would vanish at the next OTA
  (doc 41 §3.1 — this bites a Linux PC exactly as hard as anything else). The fix: **userborn**
  in hybrid mode on a **persisted `/etc` overlay** — `/etc` is an overlayfs whose generated
  lower layer comes from the image and whose writable upper layer (`/persist/etc-rw/`) lives on
  the persistent partition (NixOS `system.etc.overlay`, `mutable = true`; the mechanism NixOS
  itself pairs userborn with). passwd/shadow/group are ordinary files inside that overlay, so
  `useradd`, `passwd` and `chpasswd` behave exactly as on any Linux machine and their writes
  land on `/persist`. Accounts survive slot switches by construction, whoever created them and
  however. *(Rev 3.3, found at D1: rev 2/3's design — `passwordFilesLocation = /persist/userdb`
  with `/etc` symlinking into it — does not work: shadow-utils write `shadow+` and `rename(2)`
  it over `/etc/shadow`, which replaces the symlink with a slot-local file; a bind-mounted file
  makes the rename fail with `EBUSY`. Verified in the VM before the change.)*

### 1.1 The userborn wiring (normative; each rule closes a boot-breaking defect)

1. **Mode:** multi-user profile ⇒ `services.userborn.enable = true` **and
   `users.mutableUsers = true`**. Under userborn, hybrid mode is what *preserves*
   administrator-created rows; immutable mode drains any user absent from the declared config
   (shell → `nologin`, password locked) and remounts the files read-only. The "`mutableUsers =
   true` is a trap" line elsewhere in the corpus refers to the Perl regeneration path *without*
   userborn; here the value is required and safe. **The default image uses the same wiring**
   (rev 3.2, found at D0): its declared `mura` account has no password until the wearer sets one
   with `passwd`, and that password is mutable state that must survive reboots and A/B slot
   switches — so `mutableUsers = true` + userborn's persisted userdb on every profile, with the
   account itself still declared (`initialHashedPassword = ""`; userborn's hybrid mode keeps the
   declared row and leaves the password alone). `mutableUsers = false` would re-impose the empty
   password at every activation.
2. **Mount ordering:** the `/etc` overlay is a **stage-1** mount (NixOS mounts it in the
   initrd), so its upper layer must be there first: `syspersist` is `neededForBoot` with **no
   `nofail`** (a machine without its account database must not boot to a greeter — the B1b
   recovery ladder answers a missing persist, not a silent boot), and `/.rw-etc` is a stage-1
   bind of `/persist/etc-rw` ordered before NixOS's `rw-etc` initrd service and the overlay
   mount. `userborn.service` (`Before=sysinit.target`, `DefaultDependencies=false`) then finds
   `/etc` already persistent; no drop-in is needed (rev 3.3 — the `RequiresMountsFor` drop-in
   of rev 2/3 addressed the symlink design). The stage-2 skeleton service
   `mura-persist-setup` carries `DefaultDependencies=false` and `Before=local-fs.target`
   because the `/var/lib/mura` bind pulls it in (found at D1: the default dependencies made an
   ordering cycle that the family's old `nofail` had hidden).
3. **Permissions:** the account files keep shadow-utils' own modes inside the overlay
   (`passwd`/`group` `0644`, `shadow` `0000 root`); `/persist/etc-rw/` is `0755 root` and the
   overlay's `upper/` inherits `/etc`'s world-traversability (`getpwuid` is universal). No
   separate `userdb/` directory exists (rev 3.3).
4. **Uid discipline:** the persisted files are the single allocation ledger both slots share.
   The picker's enumeration window (§2) follows login.defs (`UID_MIN`/`UID_MAX`: NixOS's own
   1000–29999 — its nixbld range starts at 30000; Debian's 60000 would overlap it — the SDDM/
   tuigreet pattern, fidelity-checked against both trees); guest
   accounts allocate from a dedicated sub-range with a monotonic counter and no reuse before
   sweep completion (§4). Accepted and recorded: userborn's own diff state is slot-local
   (doc 41 §3.2 caveats) — harmless, since declared users are system components.

## 2. The greeter account picker

**The greeter is an ordinary Linux greeter** — the GDM/SDDM shape, at parity with the standard
set inventoried in [research/11 §11](../research/11-display-managers-greeters.md): the account
picker below; free-text username entry; a **power menu** (power-off/reboot always; suspend/
hibernate when login1 `Can*` says yes — granted without a root helper because the displayed
greeter session is logind-*active* and `org.freedesktop.login1.*` is `allow_active=yes`); a
**session chooser** from the session list the module system writes (`/etc/greetd/environments` today, the gtkgreet mechanism; `wayland-sessions` `.desktop` files if a second shell ships — research/78 §4), hidden when only one session exists
(GDM's rule); a **clock**; an **accessibility menu** (large text, high contrast, dwell timing,
the on-screen keyboard toggle — the input-floor controls of [first-run-onboarding.md §4.4](first-run-onboarding.md)).
**No network menu** *(ruled 2026-09-29, rev 3.8; rev 3.1–3.7 listed "a network menu that can
join Wi-Fi" with GDM's polkit rule)*: of every greeter inventoried in research/11 §11.D only GDM
carries a pre-auth network UI — because GDM's greeter *is* gnome-shell, which brings its network
menu along (`gnome-shell/js/ui/sessionMode.js:56-62`), and GDM 48 added the polkit rule to make
that already-present menu useful after login (`gdm/NEWS:283-285` "Allow changing global network
settings"; `gdm/data/polkit-gdm.rules.in:1-8`). SDDM, LightDM, gtkgreet, regreet, tuigreet ship
no network UI; cosmic-greeter shows a read-only status icon (`cosmic-greeter/src/networkmanager.rs:60-63`,
active connections only, no connect path). Mura's greeter has no such menu and nothing else in
it needs the network: **Wi-Fi before any user exists is onboarding's path** — the `mura-setup`
web app over the gadget or the provisioning hotspot ([first-run-onboarding.md §5](first-run-onboarding.md),
its `50-mura-setup.rules` carrying `settings.modify.system`) — **and after login it is the
session panel's** ([shell-plane.md §3.3](shell-plane.md)). The GDM-parity rule that rev 3.1
shipped for the greeter user is therefore withdrawn as a grant with no consumer (§3.1). It is
`zxr --greeter`, launched directly by greetd's `default_session` — nothing dispatches around it,
and it never hosts onboarding. Every element is operable at the input floor (head-aim + HMD
button; dwell). It must
still render when it finds **zero pickable accounts** (corrupted userdb, userborn failure):
free-text username entry and the power menu stay available, never a dark headset.

Extends the G1 auth scene; greetd needs zero changes for the picker because
`create_session(username)` precedes authentication (doc 41 §1.4):

- **Enumeration:** NSS iteration over the login.defs-shaped UID window (contract default
  `1000–29999`, NixOS's own `UID_MAX`; the contract reaches the greeter as `MURA_UID_MIN/MAX` on
  its command line and is never written to `/etc/login.defs` — doing so broke the multi-user
  login, research/78 §9 F9). Free-text username entry is **always available** beside the picker (the
  gtkgreet fallback — an administrator may hide accounts from the list; hiding is not a lock).
- **Last-user preselection:** `state/accounts/last-user`, written by the greeter itself after
  `start_session` into **its own directory** (`state/accounts`, `greeter:greeter 0755`, a
  tmpfiles rule in `modules/os/session.nix`) — the regreet/tuigreet pattern (regreet
  `/var/lib/regreet/state.toml`, tuigreet `/var/cache/tuigreet/lastuser`, both greeter-owned
  directories their NixOS modules create; research/78 §9 F7). SDDM's root daemon writing
  `state.conf` does not transfer: greetd has no root greeter daemon. Display names come from
  GECOS (G1). **Per-user metadata** (avatar, last session) in `state/accounts/<user>/` is an
  **open item**: a greeter-owned directory cannot hold it, a `1777` directory would let any local
  user forge what the login screen shows pre-auth, and AccountsService's answer is a root daemon
  writing on the user's behalf over D-Bus — decided when avatars land. Picker appears at ≥2
  entries or when guest is enabled; recorded as an accepted, Quest-independent disclosure that
  a login screen shows account names (GDM and SDDM do too) — the *lock* remains
  non-enumerating (session-auth §2.2), and greeter PAM failures are uniform (§3, the GDM
  precedent: `PAM_USER_UNKNOWN` collapses into generic failure).
- **Calibration:** greeter scenes render on factory calibration + the device-default IPD;
  per-user calibration applies at session start, post-auth.
- **Flow:** pick (or type) a name → `create_session(name)` → the PAM conversation renders per
  session-auth §2.3 → `start_session`. TOCTOU (account removed between pick and PAM) produces
  the uniform failure + a list refresh. **Switch user** = logout → greeter (one HMD, one seat).

## 3. Credentials: one credential — the Unix password; a numeric one is a PIN

**The Unix account password is the only credential.** SSH, TTY, `su`, `sudo`, the greeter, the
lock — one credential, one PAM stack, like every Linux machine. *(Rev 3.1, ADR 0018 decision 3
as amended: the separate optional `pam_mura_pin` module of rev 3 is withdrawn — a PIN is simply a
short numeric password, and Unix does not care.)*

- **The digit pad is a rendering choice, not a credential.** When a user sets a digits-only
  password (through `passwd` — the welcome surface drives it in a pty; the setup web app uses
  AccountsService under `mura-setup`'s scoped rule; no D-Bus path sets a password without
  `auth_admin` otherwise), a **non-secret `numeric-credential` hint** is written
  by the user as their own file `state/credential-hint/<user>` (the hash itself cannot reveal
  its alphabet). That directory is **sticky and world-writable** (`1777`, the `/tmp` shape;
  rev 3.4, D2): any user creates their own file, only its owner or root can replace or remove
  it, and the greeter — which runs as `greeter` and needs the hint pre-auth — reads it only after
  checking the file is owned by the account it is about to prompt. No mirror unit, nothing
  runs as root for it. The alphabet leak is bounded to local users and covered by faillock.
  The greeter and lock render a digit pad for that user,
  the full virtual-keyboard path otherwise — both operable at the input floor
  ([first-run-onboarding.md §4.4](first-run-onboarding.md)).
  Session-auth's `style=secret` fast path keys off this hint plus service config, never
  prompt-text parsing ([specs/session-auth.md §2.3](../../specs/session-auth.md)). A user may
  change to a strong password at any time; the hint follows. No `enrollment/<user>/secret/`
  exists.
- **Greeter input contract (normative, kept from rev 2):** the username arrives
  attacker-controlled over greetd IPC; the greeter validates charset/length before
  `create_session`, PAM failures are uniform (GDM's `PAM_USER_UNKNOWN` collapse), and the
  numeric-hint lookup requires `uid` in the enumeration window **before any path construction**
  (a nonexistent user renders the keyboard path, never an error).
- **The password is the user's choice, and the OS does not grade it** (rev 3.5). Digits-only,
  short, long, none: the wearer picks their own security profile, as on any Linux machine.
  What the OS provides is the same for every password: `pam_faillock` with counters persisted
  on `/persist` (tmpfs counters reset on the reboot a locked device forces), scoped per-account
  with a device-level ladder above, on the greeter, the lock and SSH alike; and **sshd with
  OpenSSH's own defaults on every interface** ([first-run-onboarding.md §5.3](first-run-onboarding.md)) —
  the rev 3.1–3.4 "key-only except the USB subnet" scoping was hardening beyond what any
  distribution ships and is withdrawn (ADR 0017 alternatives). **A passwordless account cannot
  administer**: `sudo` and polkit `auth_admin` stay standard (no `nullok`); setting a password
  with `passwd` — which asks no old password — is the gate; and because OpenSSH refuses empty
  passwords, it is also what turns SSH on for that account (a builder's declared key does so
  from first boot). The terminal fallback for a forgotten credential is standard admin
  (`sudo passwd`) — recovery-environment reset exists for the machine, not per-user.

### 3.1 PAM services and polkit rules — the complete table (normative for `modules/os/policy.nix`)

Every PAM service Mura declares, and every polkit rule it ships. Anything not in this table is
NixOS's default. `nullok` = `security.pam.services.<n>.allowNullPassword`.

| PAM service | Declared by | `nullok` | faillock | Notes |
|---|---|---|---|---|
| `greetd` | NixOS greetd module; **policy.nix pins `nullok` explicitly** (nixpkgs has flipped between setting `allowNullPassword` on `greetd` directly and substacking the `login` service, which carries it — `programs/shadow.nix:253-258` in the pinned revision; D0 verified the latter) | yes | yes (counters on `/persist`) | login for the greeter and autologin; the greeter renders the digit pad from the owner-checked hint file |
| *faillock itself* | policy.nix: three rules per service — `preauth` (required, before `pam_unix`), `authfail` (`[default=die]`, after it), `account` (required; resets on success) — each with **`conf=/etc/security/faillock.conf`**, because nixpkgs builds Linux-PAM with its sysconfdir inside the store and pam_faillock otherwise runs on compiled defaults (deny 3, 10 min, `/run/faillock`); NixOS's own `logFailures` is one argument-less `authfail` line and never blocks anything | — | — | `dir = /var/lib/mura/state/faillock`, `deny`/`unlock_time` from `mura.xr.session.faillock.{deny,unlockSeconds}` (defaults 5 / 300 s; constraint 9 — schema values), `silent`; the `faillock` CLI reads no conf file, so it is aliased with `--dir` |
| `greetd-greeter` | NixOS greetd module | — | — | the greeter user's own session; `pam_permit`-class, never a human |
| `mura-lock` | `modules/os/policy.nix` (D5; authd's service, [specs/session-auth.md](../../specs/session-auth.md)) | **no** (rev 3.6 — `pam_authenticate` always runs with `PAM_DISALLOW_NULL_AUTHTOK`, so `nullok` here would be inert; "no credential ⇒ no lock engages" is the state machine's T2, not PAM's) | yes | authd runs **as the user**; `pam_faillock` is built for that (`EACCES`/`ENOENT` on the tally → `PAM_SUCCESS`, tallies `0660 user:root`; Linux-PAM `pam_faillock.c:206-210, 306-308`) provided the tally directory is traversable — `state/faillock` is `0755` (was `0750`). Consequence: the lock updates the user's *existing* tally and honours a lockout; only root callers (greeter, sshd) create a tally, and a user can edit their own — Linux-PAM's stated design for this caller (*"files … created as owned by the user. This allows pam_faillock.so module to work correctly when it is called from a screensaver"*, `pam_faillock.8`), inherited alike by kscreenlocker, swaylock and hyprlock ([research/56 §7](../research/56-defaults-from-comparables.md)). VM-verified: five wrong unlocks lock the account; the right password is refused while locked |
| `mura-guest` | policy.nix, only when `guest.enable` | — | — | the gated branch: root check module on the enable flag + provisiond single-use token (§4) |
| `sshd` | NixOS openssh module, **upstream defaults** (rev 3.5): `services.openssh.enable = mkDefault true` in `modules/os/default.nix`, no `settings` overrides, no `Match` blocks; policy.nix adds only the faillock rules | **no** (NixOS default) | yes | password auth on every interface; the empty password is refused (`PermitEmptyPasswords no`, OpenSSH's default — and unusable here: its `none` probe authenticates with an empty password in the parent while the real attempt runs in a forked helper, and `pam_setcred` replays the probe's failure for every password login once the account has one, D2 finding). A passwordless account gets SSH after `passwd`, or from first boot with a key declared via `users.users.<n>.openssh.authorizedKeys.keys` (first-run §5.3) |
| *(no `cockpit` service)* | — | — | — | Cockpit is not part of Mura (rev 3.5); the setup web app is `mura-setup`, which has no PAM login — link possession authorises it (first-run §5.1). A user who installs Cockpit gets NixOS's own service defaults |
| `sudo` | NixOS default | **no** | — | **standard**: a passwordless account cannot `sudo`; `wheelNeedsPassword` default |
| `passwd` (password stack) | NixOS default | yes (NixOS's own `password` stack) | — | the gate: no old password asked for a passwordless account |
| `polkit-1` | NixOS default | **no** | — | `auth_admin` prompts cannot be satisfied by an empty password; standard |

| polkit rule | Grants | To | Condition | Why |
|---|---|---|---|---|
| *(no rule for the `greeter` user — rev 3.8, ruled 2026-09-29)* | — | — | — | Rev 3.1–3.7 shipped `50-mura-greeter-network.rules` (`settings.modify.system` for `greeter` when `subject.local && subject.active`, GDM's `polkit-gdm.rules.in:1-8`). Withdrawn: the greeter carries no network menu (§2), so the grant had no consumer — GDM ships its rule because its greeter has the UI (`gdm/NEWS:283-285`), and every greeter without one ships no such rule (research/11 §11.D). What the displayed greeter session can still do under NetworkManager's **own** defaults (`references/networkmanager/data/org.freedesktop.NetworkManager.policy.in`): activate existing connections (`network-control`, `allow_active=yes`, `:67-75`), scan (`wifi.scan`, `:77-85`), and add connections scoped to itself (`settings.modify.own`, `allow_active=yes`, `:105-113`); `settings.modify.system` stays `auth_admin_keep` (`:115-123`). Nothing in `pkgs/mura-greeter` calls NetworkManager |
| `50-mura-setup.rules` (rev 3.5; lands with `mura-setup`, D3) | exactly: `org.freedesktop.NetworkManager.settings.modify.system`, `org.freedesktop.timedate1.set-timezone`, `org.freedesktop.hostname1.set-static-hostname`, `org.freedesktop.accounts.user-administration`, BlueZ agent registration | the `mura-setup` system identity only (`subject.user`) | none beyond the identity — the service itself exists only while `state/setup/setup-complete` is absent | the gnome-initial-setup pattern (`references/gnome-initial-setup/data/20-gnome-initial-setup.rules.in:8-30`, which grants its setup user whole action prefixes; Mura names the exact actions). The privileged work is done by the standard daemons; the setup web app has no root helper (first-run §5.1) |

| `50-mura-timedate.rules` (rev 3.7; D-track sweep, [research/56 §9](../research/56-defaults-from-comparables.md)) | exactly: `org.freedesktop.timedate1.set-timezone`, `org.freedesktop.hostname1.set-static-hostname`, `org.freedesktop.hostname1.set-hostname` | members of `wheel` | `subject.local && subject.active && subject.isInGroup("wheel")` | Ubuntu's `policykit-desktop-privileges` grant and reason — *"Administrators … without being asked for their password … It does not change privileges for non-Administrators … the user has full control over the hardware anyway"* (`com.ubuntu.desktop.pkla:11-14,41-44`, [external]) — with phosh's condition set (`phosh-mobile-settings/data/phosh-mobile-settings.rules.in`); narrower than the appliance comparables (SteamOS `allow_any=yes` helper, pmOS Plasma `org.kde.timezone.rules` YES for anyone). On the appliance profile the passwordless seat user is in wheel, so the in-headset time-zone confirm never prompts; on the multi-user profile wheel members get the same and others keep systemd's `auth_admin_keep`. `set-ntp` is not included (Mura's zone is derived; flagged in research/56 §9) |

That is the whole list: `mura-setup`'s scoped set and the wheel time-zone rule (the greeter
rule of rev 3.1–3.7 is gone, rev 3.8). The rev-3.6 "pending" row (`50-mura-session-timedate.rules` on `subject.local &&
subject.active` alone, from research/54's SteamOS reading) was superseded by research/56 §9: no
shipping system grants the clock on session state alone — appliances grant everyone, desktops
grant the admin group. **No other rule relaxes anything for ordinary sessions** — the welcome
surface runs as the logged-in user with active-session authority only (first-run §4.3): a
passwordless user's Wi-Fi is a user-scoped connection (`settings.modify.own`,
`allow_active=yes`, `policy.in:105-113`). Rejected and recorded
(ADR 0017 rev 2.2, 2.4): a rule relaxing `org.freedesktop.accounts.change-own-password`
(escalation vector); `nullok` on sudo (root for any session process); a rule granting the
active session the setup actions while setup is unfinished (the dynamic passwordless-window
mechanism). login1's defaults already grant the displayed greeter session
power-off/reboot/suspend (`allow_active=yes`, research/11 §11.A) — no rule needed.

**logind** (also policy.nix): `services.logind.settings.Login.HandlePowerKey = "ignore"` (and
`HandlePowerKeyLongPress = "ignore"`) so the compositor owns the power key through libinput
(first-run §4.4; the SteamOS-on-Frame arrangement); `KillUserProcesses` default; nothing else.

## 4. The guest session (optional, off by default)

A **Linux feature with a decade of LightDM precedent** (doc 41 §1.6): an ephemeral account
created at session start, destroyed at session end. Off by default; enabled by the
administrator (`mura.xr.session.guest.enable` or at runtime via the polkit-gated setting).

- **Mechanism (the greetd translation — LightDM's lifecycle transfers, its daemon-resident
  auth does not):** guest login rides the normal greetd PAM service through a sufficient
  branch scoped to the guest uid sub-range, gated by a root-owned check module verifying (a)
  the admin's enable flag and (b) a provisiond-minted single-use token for exactly this fresh
  account. Enforcement is privileged twice (provisiond won't mint without the flag; the PAM
  gate re-verifies) — tile visibility is never enforcement, and a raw-socket
  `create_session("guest-…")` with guest disabled fails uniformly at both points.
- **Lifecycle:** tmpfs (or wiped-directory) home; transient calibration (default IPD + quick
  adjust, erased at teardown); teardown by a root-side unit bound to the session scope,
  row-removed-last; after power loss the **sweep runs `Before=greetd.service`** so a
  half-torn-down guest is never loginable and its uid is never reused early.
- **Defaults** (all administrator-tunable — defaults, not locks): no persistent place writes
  (a transient place set, ADR 0016), session-stratum settings only, conservative
  capture/sharing consent. An optional per-session don-window/auto-cancel knob exists for the
  hand-the-headset-to-a-visitor case. The greeter's provisiond surface remains exactly one
  conversation (create-guest, gated) — session-auth §5 as amended.
- A MAC-targetable session wrapper is reserved (the LightDM AppArmor pattern; no policy
  shipped by default).

## 5. Encryption: the user's choice, supported — never adjudicated

The design *offers* the standard Linux choices and wires none of them shut:

| Layer | Mechanism | Who chooses |
|---|---|---|
| Full disk / partitions | LUKS on `home`/`syspersist` (family/image configuration) | the administrator building or configuring the image |
| Per-home | systemd-homed (LUKS/fscrypt homes) or plain fscrypt | each user / the administrator |
| None | supported | the administrator |

Engineering guidance recorded, not enforced: a short numeric PIN is a weak LUKS passphrase
unless TPM-bound — a user coupling PIN-unlock to home encryption should do it through
TPM-backed enrollment or accept the tradeoff (their call). homed's NixOS integration is still
imperative-only (doc 41 §3.3); the condition-shaped rule stands: first-class homed support is
added when NixOS grows declarative homed users. Family wiring for LUKS options lands with the
family's own options (uefi-rauc owns its partition scheme).

## 6. Places, settings, lock, per-user state

- **Places partition by account** over a device-level anchor substrate (the room is shared;
  relocalization is not per-person) — resolves places-model §9. Shared places between users:
  condition-shaped on the mode-5 rights vocabulary, added when someone wants it.
- **Settings:** per-user preferences/state are already per-account via XDG strata; nothing new.
- **Per-user XR state:** `enrollment/<user>/calibration/` (0700 `<user>`: IPD, floor, boundary
  prefs) and the non-secret `state/credential-hint/<user>` hint file (§3). First login of
  a new account meets the same first-session welcome surface every account does
  ([first-run-onboarding.md §4](first-run-onboarding.md)) — per-item gated, skippable,
  re-runnable from settings; **never a wall between a user and their machine**.
- **Lock:** per-session, ADR 0007 unchanged; a locked session holds the seat (reboot lands in
  the greeter — recorded consequence; a destructive owner-confirmed "log out other session"
  lock affordance is an open item, §8). Boot with any credentialed account lands in the
  greeter.
- **Factory reset** (recovery environment): wipes the `/etc` overlay's upper layer
  (`/persist/etc-rw/` — the account database, machine-id, network profiles in one stroke),
  `enrollment/*`, `pairing/` and homes; on the next boot userborn
  re-materialises the image's declared accounts, runtime-created accounts are gone, and each
  account's first session meets the welcome surface again. userborn's hybrid mode tolerates
  the external edit (load-bearing, stated). Reset is the device-transfer path, never
  credential recovery (first-run-onboarding §7).

## 7. Conformance checks

1. `useradd` over SSH → A/B slot switch → the account logs in on the new slot (userdb
   persisted; early-boot NSS resolves the persisted files, no root-slot decoy).
2. The one password works everywhere always: greeter, lock, SSH, TTY, `su`, `sudo`; a
   digits-only password renders the digit pad at greeter and lock, any other password the
   keyboard path; changing between them flips the rendering with no other state change. A
   passwordless account logs in everywhere it is allowed to and **cannot `sudo`** or pass a
   polkit `auth_admin` prompt; `passwd` succeeds for it without an old password.
3. Guest disabled: raw-socket guest `create_session` fails at both privileged points; guest
   teardown on logout/doff/power-cut leaves no uid, bytes, or places; sweep completes before
   greetd starts.
4. `create_session` with nonexistent/out-of-window/system/removed-after-pick/path-hostile
   usernames: uniform failure, dummy-hash timing, no `enrollment/` path touched.
5. No account cap exists: creating account #41 works; the picker scrolls.
6. Admin actions (create account, reset another's password) require escalation and succeed
   from an unprivileged session via polkit/sudo; no session has ambient root (audit of
   /proc/*/status capabilities across greeter/autologin/user sessions).
7. Faillock counters survive reboot; device ladder trips across accounts.
8. Factory reset: surgical userdb wipe is transactional; interrupted reset re-enters
   consistently.

## 8. Open items

Each names its decider: `/home` sizing + optional per-account quotas (administrator tooling,
not policy — decider: multi-user implementation round; ext4 project quotas are the candidate
*offered* mechanism); the "log out other session" lock affordance (decider: lock UX pass,
destructive-confirm design); shared places (condition-shaped, mode-5 vocabulary); homed
first-class support (condition-shaped on NixOS declarative homed); LUKS family options
(decider: uefi-rauc family options round, with the Frame workstream).
