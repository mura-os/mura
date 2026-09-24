# Multi-user: standard Linux accounts, the XR greeter picker, and the guest session

**Status:** accepted design, rev 3 (2026-09-23). Rev 3 is the **Linux-native reframe**: rev 2
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
faillock rules are spelled out with `conf=` (§3, §3.1).
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
   The picker's enumeration window (§2) follows login.defs (`UID_MIN`/`UID_MAX`, typically
   1000–60000 — the SDDM/tuigreet pattern, fidelity-checked against both trees); guest
   accounts allocate from a dedicated sub-range with a monotonic counter and no reuse before
   sweep completion (§4). Accepted and recorded: userborn's own diff state is slot-local
   (doc 41 §3.2 caveats) — harmless, since declared users are system components.

## 2. The greeter account picker

**The greeter is an ordinary Linux greeter** — the GDM/SDDM shape, at parity with the standard
set inventoried in [research/11 §11](../research/11-display-managers-greeters.md): the account
picker below; free-text username entry; a **power menu** (power-off/reboot always; suspend/
hibernate when login1 `Can*` says yes — granted without a root helper because the displayed
greeter session is logind-*active* and `org.freedesktop.login1.*` is `allow_active=yes`); a
**session chooser** from `wayland-sessions` `.desktop` files, hidden when only one session exists
(GDM's rule); a **clock**; an **accessibility menu** (large text, high contrast, dwell timing,
the on-screen keyboard toggle — the input-floor controls of [first-run-onboarding.md §4.4](first-run-onboarding.md));
and a **network menu that can join Wi-Fi**. For that last item Mura ships the GDM rule: a polkit
rule granting the `greeter` user `org.freedesktop.NetworkManager.settings.modify.system` when
`subject.local && subject.active`, so a network joined at the greeter is a *system* connection
that the person who then logs in can use — without it the profile would be `permissions=user:greeter`
and useless (research/11 §11.D; `gdm/data/polkit-gdm.rules.in`). It is `zxr --greeter`, launched
directly by greetd's `default_session` — nothing dispatches around it, and it never hosts
onboarding. Every element is operable at the input floor (head-aim + HMD button; dwell). It must
still render when it finds **zero pickable accounts** (corrupted userdb, userborn failure):
free-text username entry and the power menu stay available, never a dark headset.

Extends the G1 auth scene; greetd needs zero changes for the picker because
`create_session(username)` precedes authentication (doc 41 §1.4):

- **Enumeration:** NSS iteration over the login.defs-shaped UID window (contract default
  `1000–60000`). Free-text username entry is **always available** beside the picker (the
  gtkgreet fallback — an administrator may hide accounts from the list; hiding is not a lock).
- **Metadata** (display name, avatar, last session) in `state/accounts/<user>/`; last-user
  preselection (`state/accounts/last-user` — the SDDM/regreet pattern). Picker appears at ≥2
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
  password (through `passwd` — the welcome item and Cockpit drive it in a pty; no D-Bus path
  sets a password without `auth_admin`), a **non-secret `numeric-credential` hint** is written
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
- **A short numeric password is weak against remote guessing; the design carries that, the
  user chooses it.** `pam_faillock` with counters persisted on `/persist` (tmpfs counters
  reset on the reboot a locked device forces), scoped per-account with a device-level ladder
  above; and **sshd is key-only on every interface except the USB-gadget subnet**
  ([first-run-onboarding.md §5.3](first-run-onboarding.md)), so a short numeric password is never
  exposed to LAN guessing. **A passwordless account cannot administer**: `sudo` and polkit
  `auth_admin` stay standard (no `nullok`); setting a password with `passwd` — which asks no old
  password — is the gate. The terminal fallback for a forgotten credential is standard admin
  (`sudo passwd`) — recovery-environment reset exists for the machine, not per-user.

### 3.1 PAM services and polkit rules — the complete table (normative for `modules/os/policy.nix`)

Every PAM service Mura declares, and every polkit rule it ships. Anything not in this table is
NixOS's default. `nullok` = `security.pam.services.<n>.allowNullPassword`.

| PAM service | Declared by | `nullok` | faillock | Notes |
|---|---|---|---|---|
| `greetd` | NixOS greetd module; **policy.nix pins `nullok` explicitly** (nixpkgs has flipped between setting `allowNullPassword` on `greetd` directly and substacking the `login` service, which carries it — `programs/shadow.nix:253-258` in the pinned revision; D0 verified the latter) | yes | yes (counters on `/persist`) | login for the greeter and autologin; the greeter renders the digit pad from the owner-checked hint file |
| *faillock itself* | policy.nix: three rules per service — `preauth` (required, before `pam_unix`), `authfail` (`[default=die]`, after it), `account` (required; resets on success) — each with **`conf=/etc/security/faillock.conf`**, because nixpkgs builds Linux-PAM with its sysconfdir inside the store and pam_faillock otherwise runs on compiled defaults (deny 3, 10 min, `/run/faillock`); NixOS's own `logFailures` is one argument-less `authfail` line and never blocks anything | — | — | `dir = /var/lib/mura/state/faillock`, `deny`/`unlock_time` from `mura.xr.session.faillock.{deny,unlockSeconds}` (defaults 5 / 300 s; constraint 9 — schema values), `silent`; the `faillock` CLI reads no conf file, so it is aliased with `--dir` |
| `greetd-greeter` | NixOS greetd module | — | — | the greeter user's own session; `pam_permit`-class, never a human |
| `mura-lock` | `modules/os/policy.nix` (authd's service, [specs/session-auth.md](../../specs/session-auth.md)) | yes | yes | no credential ⇒ no lock engages (ADR 0007) |
| `mura-guest` | policy.nix, only when `guest.enable` | — | — | the gated branch: root check module on the enable flag + provisiond single-use token (§4) |
| `sshd` | NixOS openssh module; policy.nix forces `pam_unix` back into the stack (`unixAuth`, which NixOS drops when the global `PasswordAuthentication` is off) | **no** (rev 3.4) | yes | reachable *with a password* only from the USB-gadget subnet (`Match Address` → `PasswordAuthentication` + `KbdInteractiveAuthentication yes`, first-run §5.3); key-only elsewhere. **Never `PermitEmptyPasswords`**: OpenSSH's `none` probe then authenticates with an empty password in the parent while the real attempt runs in a forked helper, and `pam_setcred` replays the probe's failure for every password login once the account has one (D2 finding). Passwordless first contact over the cable is Cockpit or the session; SSH follows `passwd` or a declared key |
| `cockpit` | NixOS cockpit module; policy.nix sets `nullok` | yes | yes | socket bound to gadget + hotspot addresses only (first-run §5.4) |
| `sudo` | NixOS default | **no** | — | **standard**: a passwordless account cannot `sudo`; `wheelNeedsPassword` default |
| `passwd` (password stack) | NixOS default | yes (NixOS's own `password` stack) | — | the gate: no old password asked for a passwordless account |
| `polkit-1` | NixOS default | **no** | — | `auth_admin` prompts cannot be satisfied by an empty password; standard |

| polkit rule | Grants | To | Condition | Why |
|---|---|---|---|---|
| `50-mura-greeter-network.rules` | `org.freedesktop.NetworkManager.settings.modify.system` | the `greeter` user | `subject.local && subject.active` | GDM parity (`gdm/data/polkit-gdm.rules.in`): a Wi-Fi network joined at the greeter becomes a system connection the logged-in user can use (research/11 §11.D) |

That is the whole list. Rejected and recorded (ADR 0017 rev 2.2): a rule relaxing
`org.freedesktop.accounts.change-own-password` (escalation vector); `nullok` on sudo (root for
any session process). login1's defaults already grant the displayed greeter session
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
  prefs) and the non-secret `enrollment/<user>/numeric-credential` hint (§3). First login of
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
