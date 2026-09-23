# ADR 0018: Multi-user — standard Linux accounts on an A/B image, plus an optional guest session

**Status:** accepted, rev 3 (2026-09-23). Rev 3 is the Linux-native reframe: rev 2 had imported
policy from closed consumer platforms — an account cap, PIN-as-the-login-credential, an "owner"
role above ordinary Unix — in violation of what became [AGENTS.md](../../AGENTS.md) /
overview invariant 10. Those are **rescinded**. Rev 2's engineering corrections (userborn boot
ordering, guest PAM gating, sweep ordering, PAM input hardening) are retained.
**Design:** [multi-user.md](../multi-user.md) rev 3.
**Evidence:** [research/41](../../research/41-multi-user-login-landscape.md) — the Linux
mechanics (§1, §3) are the authority; the closed-platform material (§2) is context and
anti-pattern except for two mechanism-level residues (per-person calibration; the
ephemeral-guest session shape, whose actual precedent is LightDM).
**Budget impact** (overview invariant 9): login/enrollment-time only; nothing on the frame path.

## Context

The multi-user profile (ADR 0007's greeter) needed its account story. The genuinely novel
problem is A/B durability: `/etc/passwd` is slot-local, so conventionally-created accounts
vanish at the next OTA — this bites a Linux PC in headset form exactly as hard as any
appliance. Everything else about multi-user is a solved Linux problem and is treated as such.

## Decision

1. **Standard Linux multi-user.** Accounts are passwd/shadow entries; authentication is PAM;
   enumeration is NSS over a login.defs-shaped UID window; admin is **wheel + polkit
   per-action escalation**. **No account cap. No "owner" role** — the appliance profile's
   `autoLogin = "owner"` names an ordinary unprivileged account; no session on any profile
   carries ambient root. Account creation/removal/credential-reset are standard admin
   operations (`useradd`, `sudo passwd`, …); the in-headset settings UI is a polkit-gated
   convenience path executing the same operations via `mura-provisiond`, which is *a* path,
   not the authority.
2. **Durability via userborn `passwordFilesLocation = /persist/userdb/`** with the normative
   wiring of multi-user.md §1.1 (initrd mount, no `nofail`, `RequiresMountsFor` drop-in,
   `0755`/`0644`/`0000` perms, `users.mutableUsers = true` under userborn on this profile —
   hybrid mode is what preserves administrator-created rows). The appliance profile keeps
   ADR 0017's fully-declarative arrangement. A consequence worth naming: because the persisted
   userdb backs `/etc` natively, **standard tools just work** — SSH in and `useradd`; no
   Mura-specific tooling is ever required for account management.
3. **Passwords primary; PIN optional.** The Unix password is the login credential everywhere
   (greeter, lock, SSH, TTY). `pam_mura_pin` is an optional per-user convenience stacked
   beside it (the fprintd model), existing because ray-keyboard password entry is painful.
   A user may lock their own password to go PIN-only — their choice. This **rescinds** rev 2's
   decision 8 and restores ADR 0017's greetd wiring to "standard, plus the optional stacked
   PIN module"; the module's hardened input contract (multi-user.md §3) stands.
4. **The greeter picker** is NSS enumeration (window default 1000–60000 per login.defs; the
   SDDM/tuigreet pattern, fidelity-checked) + free-text username entry always available +
   spatial metadata/last-user state. No AccountsService dependency. Uniform PAM failures (the
   GDM precedent: `PAM_USER_UNKNOWN` collapses into generic failure).
5. **Guest session: optional, off by default, admin-enabled** — the LightDM lifecycle
   (ephemeral account created at session start, destroyed at end) under the greetd translation
   (normal PAM service + guest-range sufficient branch gated by a root check on the enable
   flag + a provisiond single-use token; enforcement privileged twice; teardown root-side,
   row-removed-last; sweep `Before=greetd.service`). Transient calibration. All guest
   restrictions are administrator-tunable defaults, not locks.
6. **Encryption is the user's/administrator's choice, supported at every standard layer**
   (LUKS at image/partition level as family options; homed/fscrypt per-home; or none), never
   adjudicated by this design. Guidance recorded: short-PIN-as-LUKS-passphrase wants TPM
   binding. homed first-class support is condition-shaped on NixOS declarative homed users.
7. **Places partition by account** over a device-level anchor substrate (resolves
   places-model §9); per-user XR state is `enrollment/<user>/` (root-held secret material;
   user-held calibration). Per-user first-login setup (calibration, optional PIN) is
   **skippable session content, never a wall**.
8. **session-auth §5's amendment stands, minimized:** the greeter's only provisiond
   conversation is create-guest, gated as in decision 5.

## Rescinded from rev 2 (and why)

- **`maxAccounts` cap** — imported from a closed platform's household policy; no Linux system
  caps accounts. Deleted from design, contract, and tests.
- **PIN as the login credential with locked passwords by design** — appliance credential
  scheme; replaced by passwords-primary + optional stacked PIN.
- **"Owner" as a role** — replaced by wheel + polkit everywhere; owner-authorized →
  admin-authorized; "owner resets member PIN" was always just `sudo passwd` and is no longer
  presented as a designed feature.
- **"No per-account encryption in v1" as a decision** — the design offers the standard
  choices instead of deciding for the user.
- **Vision Pro don-window/supervision as design defaults** — demoted to an optional knob and
  ordinary mode-2 sharing respectively.

## Alternatives considered

- **systemd-homed now**: architecturally attractive (records travel with `/home`), still
  imperative-only on NixOS (nixpkgs #301337); condition-shaped adoption stands.
- **Plain `mutableUsers = true` without userborn**: accounts die on slot switch (doc 41 §3.1).
- **AccountsService for the picker**: a patched-daemon dependency and a second mutation path
  for greeters we don't run; NSS suffices. (Its D-Bus API remains available to any user who
  installs software expecting it — nothing blocks it; we just don't depend on it.)
- **Rebuild-to-add-accounts (fully declarative multi-user)**: remains *available* to any
  administrator who prefers declared users — declared and imperative rows coexist under
  userborn's hybrid mode. The design simply doesn't require a rebuild for a runtime `useradd`.

## Consequences

- Contract: `mura.xr.session.multiUser.enable` (selects the userborn wiring),
  `multiUser.uidRange` (picker window, default 1000–60000), `guest.enable`; the cap option and
  its assertion/tests are deleted; profile-coupling assertions stand.
- ADR 0017 decision 2's rev-2 amendment is rescinded (greetd stack: standard + optional
  stacked PIN); ADR 0017's appliance scope is unchanged.
- Registry rows reworded (no cap, no owner-role); implementation-path B5/G2 wording updated
  ("at least one account with a credential exists").
- The recovery environment owns factory reset's userdb surgery (multi-user.md §6).
