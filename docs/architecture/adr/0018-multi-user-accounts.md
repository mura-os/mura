# ADR 0018: Multi-user accounts and guest mode — real Unix accounts, userborn-persisted, provisiond-authored

**Status:** accepted
**Date:** 2026-09-23
**Context sources:** [multi-user.md](../multi-user.md) (the design this decides),
[research/41](../../research/41-multi-user-login-landscape.md) (evidence),
[ADR 0017](0017-first-run-provisioning.md) (the single-owner model this scopes),
[ADR 0007](0007-session-greeter-lock.md) (greeter/lock, unchanged),
[ADR 0016](0016-places-model.md) (places ownership item, resolved).
**Budget impact** (overview invariant 9): login/enrollment-time only; per-account storage bounded
by the account cap; guest teardown is a logout-path cost. Nothing on the frame path.

## Context

The multi-user profile (greeter, ADR 0007) existed with exactly one enrollable human. "Guest/
multi-account" had been carried as an inherited open question mislabeled a decision. Doc 41
establishes: greetd's `create_session(username)` makes a picker free; AOSP proves real per-person
uid separation on appliances; Quest ships the household model (4 accounts, boot picker,
per-account passcode); Vision Pro ships the guest model (transient, host-approved, transient
calibration); and NixOS's `mutableUsers = true` silently loses imperative accounts on an A/B
slot switch — the one genuinely dangerous trap in this space.

## Decision

1. **Accounts are real Unix accounts**, capped small (default 4, contract-tunable). No platform
   pseudo-accounts, no shared home (the Steam Deck failure mode is the anti-model).
2. **Durability via userborn `passwordFilesLocation` on the persisted userdb**
   (`/persist/userdb/` — its own state class and permission regime, multi-user.md §1.1): the
   account database survives A/B slot switches by construction. **The concrete NixOS wiring is
   part of this decision** (rev 2, red-team blockers 1–3): multi-user profile ⇒
   `services.userborn.enable = true` **and `users.mutableUsers = true`** (userborn's hybrid
   mode is what preserves provisiond rows; immutable mode drains them and remounts the files
   read-only — the "`mutableUsers = true` trap" rhetoric refers to the Perl regeneration
   path, not this arrangement); `/persist/userdb` mounted in the **initrd**, no `nofail`, a
   `RequiresMountsFor` drop-in on `userborn.service` (which runs `Before=sysinit.target` and
   would otherwise mkdir a root-slot decoy); dir `0755`, `passwd`/`group` `0644`, `shadow`
   `0000` (world-traversable NSS is non-negotiable — the userdb cannot live under the 0750
   `spatial/` tree). The appliance profile is untouched (fixed declared owner,
   `mutableUsers = false`, no userborn). ADR 0017's "never mutating the account database" is
   hereby **scoped to the appliance profile**; on the multi-user profile the database is
   mutable through exactly one authority.
3. **`spatial-provisiond` is that authority**: owner-authorized add/remove-account
   conversations; the greeter/OOBE scenes stay unprivileged. AccountsService is not shipped —
   the zxr greeter enumerates via NSS with a contract-declared UID window and keeps its own
   per-account metadata (doc 41 §3.4).
4. **Per-account enrollment**: `enrollment/<user>/` (PIN hash, user calibration incl. per-person
   IPD, privacy defaults) with a per-account `enrolled` marker; the device-level `provisioned`
   marker stays ADR 0017's. Member onboarding is the reduced per-user wizard on first login
   (launch-wait-exec continuation).
5. **Guest mode**: ephemeral account per session — provisiond add/remove around session
   lifecycle (LightDM's *lifecycle* contract; its auth mechanics do **not** transfer — LightDM's
   guest auth lives in its daemon, a seam greetd lacks, doc 41 §1.6). The greetd translation
   (rev 2, blockers 6–8): guest login rides the **normal greetd PAM service** through a
   guest-scoped sufficient branch gated by a root-owned check module verifying the owner grant
   **and** a provisiond-minted single-use token; enforcement lives privileged twice (provisiond
   refuses creation without the grant; the PAM gate re-verifies independently) — tile
   visibility is never enforcement. Teardown is a root-side unit bound to the session scope,
   ordered row-removal-last, and the boot-time sweep is `Before=greetd.service`. Per-session
   grants carry a don-window/auto-cancel; the standing toggle, grace-resumable doff, and
   optional (not mandatory) supervision are recorded deltas from the Vision Pro precedent.
   tmpfs/wiped home, transient calibration, conservative sharing/capture defaults, transient
   place set; a MAC-targetable session wrapper is reserved.
6. **Places partition by account** over a device-level anchor substrate (resolves places-model
   §9); shared household places are condition-shaped (added on MVP usage evidence, on the mode-5
   rights vocabulary).
7. **Lock/switching unchanged**: per-session PIN auth; switch user = logout → greeter picker;
   no concurrent graphical sessions (one HMD, one seat).
8. **ADR 0017 decision 2 is amended** (rev 2, contradiction C1): on the **multi-user profile,
   `pam_spatial_pin` joins greetd's login PAM stack** — member Unix passwords are locked and
   the PIN is the login credential, selected by PAM_USER under the multi-user.md §3 input
   contract (uid-window check before path construction, dummy-hash uniform failures, a
   `unix_chkpwd`-style helper for the root-0700 hash). The appliance profile keeps ADR 0017's
   wiring verbatim (`spatial-lock` only). Implementation-path B5 reflects both profiles.
9. **session-auth §5 is amended** (rev 2, contradiction C2): the `--greeter` restricted mode
   gains exactly **one** provisiond conversation — *create-guest*, gated by the grant flag and
   answered with the single-use token — and nothing else. The greeter's minimal-surface posture
   otherwise stands.

## Alternatives considered

- **systemd-homed**: architecturally the best fit for the partition layout (record travels with
  `/home`), rejected for now — NixOS support is imperative-only (nixpkgs #301337 open) and
  PIN-as-LUKS-passphrase needs a TPM-bound design. Condition-shaped: adopt when both resolve.
- **`mutableUsers = true` + plain `useradd`**: rejected — accounts die on slot switch
  (doc 41 §3.1); the trap this ADR exists to avoid.
- **Platform accounts** (profiles-as-state over one/few UIDs, the Quest-internal shape):
  rejected — trades away real isolation, PAM-native per-account credentials, and every
  XDG-per-user mechanism the stack already uses, to save a small amount of account plumbing.
- **AccountsService as picker source + CreateUser path**: rejected — a second privileged
  mutation authority beside provisiond, a patched-daemon dependency, and state we'd have to
  persist anyway; useful only to greeters we don't run.
- **Unlimited accounts**: rejected — unbounded home budgeting and picker UX for no demonstrated
  need (Quest's cap is 4).

## Consequences

- Contract: `spatial.xr.session.multiUser.{enable,maxAccounts,uidRange}` +
  `spatial.xr.session.guest.enable`; assertions (multiUser requires the multi-user profile;
  guest requires multiUser or an explicit appliance carve-out — decided: **guest requires the
  multi-user profile** in v1, since the appliance MVP has no greeter surface to grant it from).
- Registry: picker (greeter-scene extension), provisiond add-account conversation, guest
  lifecycle, userborn wiring rows; places/lock rows note the resolution.
- first-run-onboarding: §1 wording re-scoped (the softening the project owner requested);
  member-onboarding cross-ref; state-class table gains the multi-user rows (multi-user.md §6).
- Implementation joins the ladder after G2 (implementation-path §5.1 already records this).
- pam stacks: `spatial-guest` (autologin-class) joins `spatial-lock` in the NixOS-declared PAM
  set.