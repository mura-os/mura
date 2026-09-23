# Multi-user: real accounts, the greeter picker, and guest mode

**Status:** accepted design, rev 2 (2026-09-23; the greetd/PAM + NixOS red-team absorbed — 8
blockers: userborn boot ordering, the `mutableUsers` mode inversion, the userdb permission
wall, the guest PAM invocation/enforcement/sweep gaps, and the two cross-artifact
contradictions on PAM wiring and the greeter's privileged surface).
**Decision record:** [ADR 0018](adr/0018-multi-user-accounts.md).
**Evidence base:** [research/41](../research/41-multi-user-login-landscape.md).
**Scope:** multiple human accounts on one headset — substrate and A/B durability, the greeter
picker, per-account enrollment and calibration, add/remove, guest mode, places/settings/lock
fit. The single-owner appliance profile (ADR 0017) is unchanged.
**Grounding:** "XDG" = the Base Directory spec (settings-schema §2). Enumeration grounds in
NSS/passwd semantics; no new identity system.
**Budget impact** (overview invariant 9): login/enrollment-time work; per-account storage
bounded by the cap + quotas (§8); guest teardown is a logout-path cost. Nothing on the frame
path.

## 1. The account model

**Accounts are real Unix accounts** (doc 41 §2.3/§2.4: AOSP's uid separation is the right
substrate; Steam Deck's shared home is the failure mode). One explicit non-goal up front:
**per-account data-at-rest protection is not provided in v1** — AOSP pairs uid separation with
per-user credential-unlocked encryption keys, and we adopt only the uid half; member data is
DAC-protected, so root, physical disk access, and the owner (§5, PIN reset) can read it. The
systemd-homed condition (ADR 0018 alternatives) is the designated carrier for the crypto half.

- **A small fixed cap** (default 4, Quest precedent; contract-tunable): one **owner** plus
  members. Bounds userdb size, enrollment storage, picker UX — and, with §8's quotas, bytes.
- **Durability: userborn with `passwordFilesLocation` on the persisted userdb** (doc 41 §3.2).
  The account database lives at **`/persist/userdb/`** — its own top-level directory and state
  class, *not* under `spatial/` (see the permission rule below) — and `/etc`'s
  `passwd`/`shadow`/`group` are **static image symlinks** into it (they dangle until the mount
  is up; hence the ordering rules).
- **Creation authority: `spatial-provisiond`** — owner-authorized add-account conversations;
  the wizard/greeter scenes stay unprivileged clients. AccountsService is not shipped (doc 41
  §3.4).

### 1.1 The userborn wiring (normative — each rule closes a boot-breaking defect)

1. **Mode:** the multi-user profile sets `services.userborn.enable = true` **and
   `users.mutableUsers = true`**. Under userborn, hybrid mode is what *protects*
   provisiond-created rows: immutable mode drains any user absent from the declared config
   (shell → `nologin`, password locked) and then **remounts the password files read-only**, so
   provisiond could neither keep nor write accounts. The corpus's "`mutableUsers = true` is a
   trap" rhetoric (doc 41 §3.1, ADR 0017) refers to the *Perl regeneration semantics without
   userborn*; with userborn + persisted files the option value is required and safe. The
   appliance profile keeps `mutableUsers = false` and no userborn.
2. **Mount ordering:** `userborn.service` runs `Before=sysinit.target` with
   `DefaultDependencies=false` and will happily `mkdir -p` its location — on the wrong
   filesystem — if the mount isn't up. Therefore: the `syspersist` partition and the
   `/persist/userdb` path are mounted **in the initrd** (the same early treatment machine-id
   already requires), the mount units for this path carry **no `nofail`** (a system without its
   account database must not boot to a greeter), and the profile ships a drop-in on
   `userborn.service` with `RequiresMountsFor=/persist/userdb` so the decoy-directory failure
   mode is structurally impossible. Conformance check 1 asserts early-boot NSS resolves against
   the persisted files on the first boot after a slot switch.
3. **Permissions:** `/persist/userdb/` is `0755 root`, `passwd`/`group` `0644`, `shadow`
   `0000 root` — world-traversable because `getpwuid` is universal (greeter NSS, logind, D-Bus
   policy all need it). This is why the userdb cannot live under the `0750` `spatial/` tree,
   and why `shadow` — credential material — gets its own class row (§6) rather than the
   `state/` bookkeeping class.
4. **Uid discipline:** the persisted files are the **single allocation ledger** both slots
   share — that, not userborn, is the cross-generation collision guard. provisiond allocates
   strictly inside the contract `uidRange` by reading them; it **rejects usernames colliding
   with declared users** (userborn owns declared names destructively); guest uids come from a
   **dedicated sub-range above `uidRange`** (never <1000: system-uid heuristics in
   logind/polkit bite) with a monotonic counter in `state/` and no reuse before sweep
   completion (§4). Accepted consequence, recorded: userborn's own diff state
   (`/var/lib/userborn/`) is slot-local, so a *declared*-user removal between generations may
   not drain on the other slot — harmless here (declared users are system components), noted
   in doc 41 §3.2.

## 2. The greeter account picker

Extends the G1 auth scene; greetd needs zero changes **for the picker** because
`create_session(username)` precedes authentication (doc 41 §1.4 — the guest path is different,
§4):

- **Enumeration:** NSS iteration filtered by the contract `uidRange` (SDDM/tuigreet pattern).
  **Posture, recorded as an accepted disclosure:** the picker shows names/avatars/last-user to
  anyone holding the device (the Quest model) — the *lock* remains non-enumerating
  (session-auth §2.2's no-account-existence-leak rule is untouched), the greeter enumerates
  only inside `uidRange`, and PAM failures at the greeter are uniform (§3) so the IPC confirms
  nothing outside the picker's own display.
- **Metadata** (display name, avatar, last session): `state/accounts/<user>/`; last-user
  memory in `state/accounts/last-user`. Picker hidden with one account and guest disabled.
- **Calibration:** the picker (like every greeter scene) renders on **factory calibration +
  the device-default IPD** — identity precedes calibration by construction; per-account
  calibration applies only at session start, post-auth (§3).
- **Flow:** pick → `create_session(name)` → PAM per session-auth §2.3 → `start_session`.
  `cancel_session` returns to the picker. **TOCTOU:** an account removed between pick and
  `create_session` produces the same uniform failure and a picker refresh; the greeter
  subscribes to provisiond change events for live refresh.
- **Switch user** = logout → greeter (one HMD, one seat; no concurrent sessions). greetd
  restarting the dispatcher *is* the switch.

## 3. Per-account enrollment, PIN, calibration

- `enrollment/<user>/` splits by confidentiality: **`secret/`** (PIN hash — argon2) stays
  `0700 root`; **`calibration/`** (IPD preference, floor, boundary prefs, privacy defaults) is
  **owned by that uid** (`0700 <user>`), because the user's own session (Monado/zxr as that
  uid) must read it at session start without a privileged hand-off. The device-level
  `provisioned` marker is unchanged; each account has `enrollment/<user>/enrolled`.
- **`pam_spatial_pin` input contract (normative):** the username arrives attacker-controlled
  over greetd IPC. The module validates charset/length, resolves through NSS, and requires
  `uid ∈ uidRange` **before any path construction** (no traversal, no probing of system
  accounts); for nonexistent/out-of-range users it verifies against a **dummy argon2 hash** so
  timing and failure shape are uniform. Because the hash is root-`0700` and the verifying
  context may be unprivileged, verification goes through a `unix_chkpwd`-style helper (root
  socket or setuid, decided at implementation) — named here so it cannot be improvised.
- **Rate limiting:** per-account `pam_faillock` counters **persisted on `/persist`** (tmpfs
  counters reset on the reboot a boot-locked device forces — pointless otherwise), plus a
  **device-level ladder** above them so four accounts do not quadruple the physical attempt
  budget; the terminal fallback stays doc 12's (reset).
- **Forgotten member PIN (decided):** an **owner-authorized provisiond conversation resets a
  member's PIN**, re-entering the member wizard's PIN step at next login — factory-resetting
  the whole device for one member's PIN is disproportionate. Recorded honestly: this means the
  owner can enter a member's account; with §1's no-crypto non-goal, the owner-as-threat model
  is already accepted, and this makes it explicit.
- **Member onboarding:** reduced F2 on first login (PIN, user calibration, privacy defaults) —
  device-level steps never repeat; launch-wait-exec continuation (first-run-onboarding §4.1).
  Boundary stays device-level; per-account boundary *preferences* are enrollment-class.

## 4. Guest mode

LightDM's **lifecycle** contract + Vision Pro's **session** semantics — with the mechanism
translated to greetd, because the LightDM evidence covers the lifecycle only: LightDM's guest
auth lives *in its daemon*, a seam greetd deliberately lacks (doc 41 §1.6; the "zero greetd
changes" claim in §2 is scoped to the picker).

- **Invocation (the greetd translation):** guest login goes through the **normal greetd PAM
  service** with a guest-scoped branch: a `pam_succeed_if`-guarded sufficient block for uids
  in the guest sub-range, gated by a **root-owned check module** that verifies (a) the owner
  grant flag is present and (b) a **provisiond-minted single-use token** exists for exactly
  this fresh guest account. No separate PAM service selection is needed from greetd.
- **Enforcement lives privileged, twice:** (a) **provisiond** refuses the create-guest
  conversation unless the root-owned grant flag (an owner settings action) is present — an
  account that doesn't exist cannot be logged into; (b) the **PAM gate** independently
  re-verifies grant + token as root, so a stale row fails closed. Tile visibility is scene
  furniture, never enforcement. A hostile greetd client calling `create_session("guest-…")`
  with guest disabled hits (a)+(b) and gets the uniform failure.
- **The greeter's privileged surface (session-auth §5 amendment):** the greeter gains exactly
  **one** narrowly-scoped provisiond conversation — *create-guest* — gated as above; nothing
  else (no add-account, no PIN operations from the greeter).
- **Lifecycle:** provisiond mints `guest-<n>` (monotonic counter, §1.4) at session start with
  a tmpfs home (or wiped directory on memory-constrained devices); **teardown is a root-side
  unit bound to the session scope** (the B6a wrapper runs as the guest uid and cannot delete
  accounts). Teardown order is normative: kill session → wipe home/transient places/tmp/spool
  → remove the userdb row **last**. After power cut, the **sweep is a hard prerequisite of
  greetd** (`Before=greetd.service`, `RequiredBy=`), so a half-torn-down guest row is never
  loginable and its uid is never reused before the sweep completes.
- **Session semantics** (deliberate deltas from Vision Pro, recorded): per-session grants
  carry a **don window with auto-cancel** (Vision Pro's 5-minute shape); the standing
  "enabled" toggle — a standing passwordless greeter tile — remains available but is the
  *weaker* mode, and the doc says so; guest doff uses the ordinary grace window (a returning
  guest resumes; acceptable for a guest, recorded as a choice); supervision (owner view via
  mode-2 sharing, consent rules unchanged) is **optional here, mandatory in the precedent** —
  a recorded delta, not an oversight.
- **Restrictions (defaults):** conservative capture/sharing (no persistent consent grants;
  passthrough-excluded); transient place set only (ADR 0016's transient kind); no settings
  writes above session stratum; **no provisiond conversations from inside the session** (the
  bracketing create/teardown calls happen outside it — the greeter's gated call before, the
  root teardown unit after). Calibration transient: default IPD + quick adjust in the
  ephemeral home, erased at teardown. A MAC-targetable session wrapper is reserved (doc 38).

## 5. Places, settings, lock

- **Places** (resolves places-model §9 ownership): place sets **partition by account** over a
  **device-level anchor substrate** (the room is shared; relocalization is not per-person).
  Shared household places: condition-shaped — added on MVP usage evidence, on the mode-5
  rights vocabulary (spatial-sharing §5).
- **Settings:** per-user preferences/state are already per-account via XDG strata
  (settings-schema §2); per-unit strata shared.
- **Lock:** per-session PIN auth, no cross-account unlock. **The lock-hostage consequence is
  accepted and recorded:** a locked member session holds the device — reboot *is* the switch
  (boot-locked lands in the picker; the locked session's unsaved state is lost). A destructive
  owner-confirmed "log out other user" lock tile is an open item (§8). Guest doff past grace
  tears down instead of locking. Boot with ≥1 enrolled account lands in the greeter.
- **Factory reset:** the recovery environment removes **all `uidRange` rows including the
  owner** from the persisted userdb (surgical, transactional, alongside the marker/enrollment
  wipe — userborn's hybrid mode tolerates the external edit, a load-bearing assumption stated
  here), wipes `enrollment/*` and homes, rotates machine-id; OOBE recreates the owner. A power
  cut mid-reset leaving a userdb row without enrollment is safe by construction: a missing
  `enrolled` marker re-enters the wizard.

## 6. State classes (amends the first-run-onboarding §2 table)

| Path | Class | Perms | A/B | Factory reset |
|---|---|---|---|---|
| `/persist/userdb/` (userborn passwd/group/shadow) | **userdb** (its own class) | dir 0755; passwd/group 0644; shadow 0000 | survives | all `uidRange` rows removed; owner recreated by OOBE |
| `enrollment/<user>/secret/` | enrollment | 0700 root | survives | wiped |
| `enrollment/<user>/calibration/` | enrollment | 0700 `<user>` | survives | wiped |
| `state/accounts/<user>/`, `state/accounts/last-user`, guest uid counter | state | 0750 | survives | wiped |
| guest home + account | none (tmpfs/wiped) | — | n/a | never persists |

## 7. Conformance checks

1. Add account → A/B slot switch → **early-boot NSS on the new slot resolves the persisted
   files** (no root-slot decoy) and the account logs in.
2. Guest teardown on logout, doff-past-grace, and power cut: no uid, home bytes, journal-side
   state, or place entries remain; **the boot-time sweep completes before greetd starts**; a
   crafted stale guest row without a valid token fails login uniformly.
3. `create_session` with: nonexistent user, out-of-range user, declared system user, removed-
   after-pick user, and path-hostile username — all produce the uniform failure with
   dummy-hash timing; none touches `enrollment/` paths.
4. Guest disabled: `create_session("guest-…")` from a raw socket fails at both enforcement
   points independently (grant absent; token absent).
5. Per-account PIN isolation; member PIN reset by owner re-enters the member PIN wizard step.
6. Faillock: counters survive reboot; the device ladder trips across accounts.
7. Account cap and quota (§8) enforced by provisiond with typed errors; picker
   presence/absence per §2.
8. Factory reset: userdb surgery + wipe is transactional; interrupted reset re-enters
   consistently; owner-only OOBE re-runs.
9. `mutableUsers` mode assertion: the multi-user profile fails evaluation if
   `users.mutableUsers = false` while `multiUser.enable = true` (the drain-and-remount trap).

## 8. Open items

Each names its decider: **per-account home quotas + `/home` sizing** (ext4 project quotas are
the candidate; decider: the multi-user implementation round — cap=4 bounds count, not bytes,
and 512M/4 is plainly small); shared household places (condition-shaped; decider: MVP usage
evidence); per-session guest approval UX vs standing toggle default (decider: the guest UX
pass); the "log out other user" lock tile (decider: the same UX pass, with the destructive-
confirm design); homed adoption (condition-shaped, doc 41 §3.3 — also the carrier for the §1
data-at-rest non-goal); whether members may own device settings like Wi-Fi (decider: the
settings polkit action inventory, settings-schema §10).
