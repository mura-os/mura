# Multi-user: real accounts, the greeter picker, and guest mode

**Status:** accepted design (2026-09-23). Decision record: [ADR 0018](adr/0018-multi-user-accounts.md).
**Evidence base:** [research/41](../research/41-multi-user-login-landscape.md) (greeter/DM
mechanics, XR precedents, NixOS durability analysis).
**What this covers:** multiple human accounts on one headset — the account substrate and its A/B
durability, the greeter account picker, per-account enrollment and calibration, account
add/remove, guest mode, and the places/settings/lock fit. The single-owner appliance profile
(ADR 0017) is unchanged; this design is the multi-user profile's content.
**Grounding:** "XDG" = the Base Directory spec (per-user config/state split, settings-schema §2).
Account enumeration grounds in NSS/passwd semantics (doc 41 §1.3), not a new identity system.
**Budget impact** (overview invariant 9): login-time and enrollment-time work only; per-account
state adds storage (bounded by the account cap); nothing on the frame path. Guest teardown is a
logout-path cost.

## 1. The account model

**Accounts are real Unix accounts** (doc 41 §2.3: AOSP proves real per-person uid separation is
the right appliance substrate; §2.4: Steam Deck's shared home is the failure mode). The
multi-user profile:

- **A small fixed cap** (default 4, the Quest precedent; contract-tunable) of human accounts:
  one **owner** (created by the OOBE, uid stable) plus added members. The cap bounds
  home-partition budgeting, enrollment storage, and picker UX.
- **Durability: userborn with `passwordFilesLocation` on persist-backed state** (doc 41 §3.2).
  The entire `passwd`/`shadow`/`group` database lives under
  `/var/lib/spatial/state/userdb/` (state class: survives A/B and factory-reset *policy* applies
  — see §6) and is symlinked from `/etc` at boot. This is what makes runtime-created accounts
  survive an A/B slot switch; plain `mutableUsers = true` demonstrably does not (doc 41 §3.1).
- **Creation authority: `spatial-provisiond`** (the ADR 0017 enrollment authority) grows an
  **add-account conversation**: owner-authorized (owner PIN re-entry), it creates the Unix
  account (via userborn's database), the per-account enrollment directory, and the home skeleton.
  The OOBE UI and the greeter scene are unprivileged clients of it; AccountsService is not
  shipped (doc 41 §3.4 — zxr owns the greeter UI, so NSS enumeration + spatial's own metadata
  suffice).
- **systemd-homed** is the watched alternative (condition-shaped: adopted only when NixOS grows
  declarative homed support and the PIN-as-LUKS-passphrase coupling has a TPM-bound answer —
  doc 41 §3.3).

The appliance profile keeps ADR 0017 §1 verbatim: fixed declared owner, `mutableUsers = false`,
no userborn requirement. The strong "never mutating the account database" claim is
**profile-scoped** — on the multi-user profile the account database is mutable *through exactly
one authority* (provisiond → userborn's persisted files), never through ad-hoc `useradd`.

## 2. The greeter account picker

Extends the G1 auth scene (implementation-path); greetd needs zero changes because
`create_session(username)` precedes authentication (doc 41 §1.4):

- **Enumeration:** NSS iteration filtered by UID range (the SDDM/tuigreet pattern, doc 41
  §1.3/§1.4) — human accounts occupy a contract-declared UID window; the `greeter` and `guest`
  system users are outside it.
- **Per-account metadata** (display name, avatar, last-session): spatial's own store at
  `/var/lib/spatial/state/accounts/<user>/` — not AccountsService keyfiles. Written via
  provisiond (identity fields) and the settings daemon (preferences).
- **Last-user memory:** `state/accounts/last-user` (the SDDM/regreet precedent), preselected in
  the picker; single-account devices skip the picker entirely (picker appears at ≥2 entries or
  when guest is enabled).
- **Flow:** pick account → `create_session(name)` → PAM conversation renders per session-auth
  §2.3 (the per-account PIN pad) → `start_session`. The picker is scene furniture, not an auth
  step; mid-conversation `cancel_session` returns to it.
- **Switch user** = logout → greeter (lifecycle §3b: one HMD, one seat, no concurrent graphical
  sessions — v1 rule restated here as design, not schedule). No
  `DisplayManager.Seat.SwitchToGreeter` analog is needed; greetd restarting the dispatcher *is*
  the switch.

## 3. Per-account enrollment and calibration

The ADR 0017 state classes gain per-account structure:

- `enrollment/<user>/` — PIN hash (argon2, `pam_spatial_pin` selects by the PAM user), the
  per-account **user calibration** (IPD preference — per-person by nature — floor height,
  boundary preferences), privacy defaults. The **provisioning marker stays device-level**
  (`enrollment/provisioned` — the *device* is provisioned once, by the owner's OOBE); each
  account additionally has `enrollment/<user>/enrolled` written when its wizard pass completes.
- **Member onboarding** is a reduced F2: the first login of a new account runs the per-user
  wizard steps only (PIN, user calibration, privacy defaults) — locale/Wi-Fi/device identity are
  device-level and never repeat. Same dispatcher logic one level down: the session wrapper runs
  the member wizard as first session content when `enrolled` is absent (launch-wait-exec, the
  first-run-onboarding §4.1 continuation semantics).
- **Boundary** is device-level state (the room is shared), not per-account; per-account boundary
  *preferences* (visibility style) are enrollment-class.

## 4. Guest mode

The LightDM lifecycle contract with Vision Pro session semantics (doc 41 §1.6/§2.2):

- **Ephemeral by construction:** provisiond owns a guest add/remove pair — session start creates
  a fresh `guest-XXXX` account with a **tmpfs home** (or wiped-on-teardown directory on
  memory-constrained devices); session end (logout, doff past grace, power) destroys the account
  and its state. Nothing survives; there is no guest enrollment directory.
- **Entry:** a greeter tile, present only when enabled and **owner-granted**: enabling guest
  mode is an owner settings action; optionally per-session approval (the owner authorizes from
  their unlocked session before handing the device over — the Vision Pro handoff, without
  requiring a phone).
- **Authentication:** none — the dispatcher path uses an autologin-class PAM service
  (`spatial-guest`: real PAM session/environment, no credential exchange; the LightDM shape).
  `pam_spatial_pin` is not in this stack.
- **Calibration:** transient — the guest gets the safe default IPD plus an optional quick
  adjustment stored in the ephemeral home; erased at session end (Vision Pro semantics).
- **Restrictions (defaults, contract-tunable):** conservative capture/sharing (no capture
  consent grants persist; passthrough-excluded defaults per spatial-sharing §6), no places
  writes to persistent places (guest gets a transient place set that evaporates — the places
  model's transient kind does this for free, ADR 0016), no settings writes above session
  stratum, no provisiond conversations. Supervision: the owner may attach a mode-2 view share
  (spatial-sharing) — the Vision Pro mirroring affordance expressed through the existing
  sharing machinery, consent rules unchanged.
- **MAC hook reserved:** a wrapper analogous to LightDM's `lightdm-guest-session` exists so a
  future MAC profile can target guest sessions (doc 38's landscape; no policy shipped v1).

## 5. Places, settings, lock

- **Places** (resolves places-model §9's ownership item): place sets **partition by account** —
  each account's persistent places (pinned, anchored) live in its own store; the spatial anchor
  substrate (maps, anchors) is **device-level** (the room is shared; relocalization is not
  per-person), so two accounts may pin places at the same physical anchor without interference.
  **Shared household places** (one pinned place visible to all accounts) are a condition-shaped
  extension: added only when the multi-account MVP surfaces real demand; the model already
  admits it (a place store keyed device-level with an ACL — the mode-5 rights vocabulary,
  spatial-sharing §5, is the reserved shape).
- **Settings:** nothing new — per-user preferences/state are already `$XDG_CONFIG_HOME`/
  `$XDG_STATE_HOME` per account (settings-schema §2); per-unit strata are shared. The settings
  daemon's session half runs per login session as-is.
- **Lock:** unchanged (ADR 0007): the lock scene authenticates *the session's account* via its
  PIN; there is no cross-account unlock. Doff past grace on a guest session tears the session
  down instead of locking (nothing to return to). Boot-locked rule: with ≥1 enrolled account,
  boot lands in the greeter (the picker *is* the locked surface on multi-user).
- **Factory reset:** wipes every account's `enrollment/<user>/`, all member accounts from the
  persisted userdb (the owner account row is recreated by OOBE), all homes, and rotates
  machine-id — the ADR 0017 inverse, per-account-aware.

## 6. State-class amendments (first-run-onboarding §2 table, applied)

| Path | Class | A/B | Factory reset |
|---|---|---|---|
| `state/userdb/` (userborn passwd/shadow/group) | state | survives | member rows wiped; owner recreated by OOBE |
| `enrollment/<user>/` | enrollment | survives | wiped |
| `state/accounts/<user>/` (picker metadata) | state | survives | wiped |
| guest home + account | none (tmpfs/ephemeral) | n/a | n/a — never persists |

## 7. Conformance checks

1. Add account → A/B update → slot switch: the account logs in on the new slot (userdb
   persisted; the §3.1 trap demonstrably avoided).
2. Guest session end (each of: logout, doff-past-grace, power cut): no guest uid, home bytes,
   or place entries remain; power-cut cleanup happens at next boot (provisiond sweep).
3. Member wizard interrupted: next login re-enters at the first incomplete step; device-level
   steps never re-run.
4. Per-account PIN isolation: account A's PIN never unlocks account B's session; PAM user
   selection verified against a hostile picker input (username injection through
   `create_session` must reach PAM verbatim and nothing else).
5. Account cap: provisiond rejects add-account beyond the cap with a typed error.
6. Picker hidden with one account and guest disabled; appears with two, or one + guest.
7. Factory reset: owner-only OOBE re-runs; no member metadata survives anywhere (userdb,
   enrollment, accounts metadata, homes).

## 8. Open items

Each names its decider: shared household places (condition-shaped above; decider = MVP usage
evidence); per-session guest approval UX vs settings-only grant (decider: the guest UX pass at
implementation); homed adoption (condition-shaped, doc 41 §3.3); whether member accounts may
own OOBE-class device settings like Wi-Fi (decider: the settings polkit action inventory,
settings-schema §10).
