# Multi-user: standard Linux accounts, the XR greeter picker, and the guest session

**Status:** accepted design, rev 3 (2026-09-23). Rev 3 is the **Linux-native reframe**: rev 2
imported policy from closed consumer platforms (an account cap, PIN-as-the-credential, an
"owner" role); all of it is removed per [AGENTS.md](../../AGENTS.md) / overview invariant 10.
Rev 2's engineering corrections (userborn boot ordering, guest PAM gating, sweep ordering, PAM
input hardening) survive — they were correctness, not policy. **Rev 3.1 (2026-09-24):** the
first account is declared in the image and asserted at build; the runtime account-bootstrap
screen is gone ([ADR 0017 rev 2](adr/0017-first-run-provisioning.md)); **one credential** — the
separate `pam_mura_pin` module is withdrawn, a PIN is a numeric password with a rendering hint
(§3); the greeter's standard furniture and the input floor are normative (§2).
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
  because the persisted userdb (below) *is* `/etc`'s backing store, standard tools operate on
  it natively. The in-headset settings UI is a convenience path for the same operation: a
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
  with `passwordFilesLocation = /persist/userdb/` — the entire passwd/shadow/group database
  lives on the persistent partition and `/etc` symlinks into it. Accounts survive slot switches
  by construction, whoever created them and however.

### 1.1 The userborn wiring (normative; each rule closes a boot-breaking defect)

1. **Mode:** multi-user profile ⇒ `services.userborn.enable = true` **and
   `users.mutableUsers = true`**. Under userborn, hybrid mode is what *preserves*
   administrator-created rows; immutable mode drains any user absent from the declared config
   (shell → `nologin`, password locked) and remounts the files read-only. The "`mutableUsers =
   true` is a trap" line elsewhere in the corpus refers to the Perl regeneration path *without*
   userborn; here the value is required and safe. The appliance profile keeps
   `mutableUsers = false`, no userborn.
2. **Mount ordering:** `userborn.service` runs `Before=sysinit.target` with
   `DefaultDependencies=false` and will `mkdir -p` its location on the wrong filesystem if the
   mount isn't up. Therefore: `syspersist` + `/persist/userdb` mount in the **initrd**, no
   `nofail` on this path (a machine without its account database must not boot to a greeter),
   and a drop-in adds `RequiresMountsFor=/persist/userdb` to `userborn.service`.
3. **Permissions:** `/persist/userdb/` is `0755 root`; `passwd`/`group` `0644`; `shadow`
   `0000 root` — world-traversable because `getpwuid` is universal. This is why the userdb
   lives beside, not under, the `0750` `mura/` tree.
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
  password, a **non-secret `numeric-credential` hint** is written to `enrollment/<user>/` (by
  the same polkit-gated own-password action that set it; the hash itself cannot reveal its
  alphabet); the greeter and lock render a digit pad for that user, the full virtual-keyboard
  path otherwise — both operable at the input floor ([first-run-onboarding.md §4.4](first-run-onboarding.md)).
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
  above; and sshd accepts password authentication for empty/short passwords only on the
  physically-trusted subnets ([first-run-onboarding.md §5.3](first-run-onboarding.md)) — over the
  LAN, SSH password auth is standard sshd policy for the administrator to set. The terminal
  fallback for a forgotten credential is standard admin (`sudo passwd`) — recovery-environment
  reset exists for the machine, not per-user.

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
- **Factory reset** (recovery environment): removes human-window rows from the persisted
  userdb, wipes `enrollment/*` and homes, rotates machine-id; on the next boot userborn
  re-materialises the image's declared accounts, runtime-created accounts are gone, and each
  account's first session meets the welcome surface again. userborn's hybrid mode tolerates
  the external edit (load-bearing, stated). Reset is the device-transfer path, never
  credential recovery (first-run-onboarding §7).

## 7. Conformance checks

1. `useradd` over SSH → A/B slot switch → the account logs in on the new slot (userdb
   persisted; early-boot NSS resolves the persisted files, no root-slot decoy).
2. The one password works everywhere always: greeter, lock, SSH, TTY, `su`, `sudo`; a
   digits-only password renders the digit pad at greeter and lock, any other password the
   keyboard path; changing between them flips the rendering with no other state change.
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
