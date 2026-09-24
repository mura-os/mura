# ADR 0017: First-run provisioning — the image is the installation, silent F1, the welcome surface, out-of-band access

**Status:** accepted; **amended 2026-09-24 (rev 2)** — decisions 1, 3, 4 and 6 rewritten,
decision 7 added. Rev 1 (2026-09-23) designed a pre-login onboarding wizard; rev 2 records why
it does not exist.
**Date:** 2026-09-23 / 2026-09-24
**Context sources:** [first-run-onboarding.md](../first-run-onboarding.md) (the design this
decides), [research/41 §1.2](../../research/41-multi-user-login-landscape.md) (GDM's
zero-users rule — the mechanism evidence), [research/12](../../research/12-lock-screens-and-appliance-login.md)
§6 (PIN options; Quest posture as anti-pattern), [research/11](../../research/11-display-managers-greeters.md)
(greetd), [ADR 0007](0007-session-greeter-lock.md), [ADR 0018](0018-multi-user-accounts.md)
(standard Linux multi-user), `references/pmaports` + `references/pmbootstrap` (USB-network +
SSH first boot), [specs/settings-schema.md](../../../specs/settings-schema.md),
[implementation-path.md](../implementation-path.md) (the F-track), [AGENTS.md](../../../AGENTS.md)
/ overview invariant 10.
**Budget impact** (overview invariant 9): one-shot boot work (F1), shell-plane session content
(F2), sshd, and — only while unprovisioned — a hotspot plus a small HTTP server; nothing on the
frame path; no steady-state tenant.

## Context

A shipping image must get from "flashed" to "a person is using it" without imperative system
mutation, on an A/B-updated, image-based NixOS where the store is world-readable and per-unit
state is sacred (invariant 4). Rev 1 solved this the way desktop distributions do — a pre-user
onboarding session under the display manager (`gnome-initial-setup` shape) — and imported with
it a seven-step wizard, a dispatcher that selected sessions from runtime state, a transactional
marker that gated the UI, and a root daemon conversation authorised by the *absence* of state.
Pulling on the design showed why desktops need that machinery and why this OS does not: an
installer hands over a machine with no users; a flashed image is a declared configuration and
has no such gap. Rev 2 follows that observation to its conclusions.

## Decision

1. **The image is the installation; the default image is a declared configuration.** There is
   no installer session. Everything an installer collects is declared in the image
   (`users.users.<name>` with `isNormalUser`/`hashedPasswordFile`/`wheel`, locale, NetworkManager
   profiles with path-based secrets, `mura.xr.session.autoLogin` or `.greeter`). The **default
   image declares user `mura`, no password, `wheel`, `autoLogin = "mura"`** — power on and you
   are in your session; a password is the wearer's choice, offered by the welcome surface. On the
   appliance profile the declared account is the account (`users.mutableUsers = false`, the Steam
   Deck `deck` mechanism); on the multi-user profile ADR 0018 governs mutability (userborn,
   standard tools). "Paths" are gone from the vocabulary: there is the image and the two profiles
   it may declare. *Amends rev 1's "fixed declared owner account": the name is `mura`, and the
   word "owner" is not a role (ADR 0018 rev 3).*
2. **PIN = doc 12 option (b), ratified — as an optional convenience.** A dedicated
   `pam_mura_pin` module verifies an argon2-hashed PIN stored in `enrollment/<user>/secret/`,
   **stacked beside the Unix password** on the greeter and lock stacks (the fprintd model —
   ADR 0018 rev 3, [multi-user.md §3](../multi-user.md)); passwords remain the login credential
   on every surface. Both stacks are declared through NixOS modules, never hand-edited. The
   rev-1 "owner-password-is-PIN appliance bridge" is **withdrawn** — with passwords primary it
   has no purpose. This closes ADR 0007's open question.
3. **No pre-login onboarding exists. F2 is a first-session welcome surface.** The multi-user
   profile's greetd `default_session` is `zxr --greeter`, full stop; the appliance profile's
   `initial_session` is the session. The dispatcher wrapper, the `/run/mura/provisioned` flag,
   `zxr --oobe`, the wizard ladder, the commit ceremony, and the privacy/consent step are
   **removed**. What remains is ordinary shell-plane session content (first-run-onboarding §4):
   per-item gated (offered only when the target value is undefined; all defined ⇒ never shown),
   skippable, re-runnable from settings, per person. Its contents and the input floor it must be
   operable at are **open questions whose decider is the findings review of research/42 (input
   bootstrap) and the research/11 greeter-furniture addendum** — the surface is not specified
   until what a headset can accept as input is known. Privileged writes go through standard
   mechanisms (polkit-gated account actions, NetworkManager D-Bus policy, `localed`);
   `mura-provisiond` keeps only the XR-specific writes (PIN hash) and the guest-token gate, and
   **has no conversation authorised by the absence of state**.
4. **A greeter image declares its first account; the build asserts it.** Multi-user profile ⇒
   at least one `isNormalUser` account, else evaluation fails, with the escape hatch
   `mura.xr.session.allowNoDeclaredAccount` (the `users.allowNoPasswordLogin` pattern —
   NixOS's own check is silent for us because the profile runs `mutableUsers = true` under
   userborn). GDM's runtime fallback (`ListCachedUsers` empty → initial-setup) exists for OEM
   preinstall; our OEM-preinstall equivalent is the default image, so the fallback and its
   privileged surface are deleted rather than translated. A greeter facing zero pickable accounts
   at runtime still renders (free-text username, power menu); recovery is standard root. **F1's
   per-task markers on `/persist` remain authoritative over `ConditionFirstBoot`** (a fresh A/B
   slot resembles first boot); no single "provisioned" marker exists and none gates UI.
   machine-id persists across updates and **rotates on factory reset**.
5. **Secrets are never Nix option values.** PIN hashes, Wi-Fi credentials, and device keys exist
   only as runtime state under protected persistent storage, or as declared *paths*
   (`hashedPasswordFile`, keyfile secrets) — the store is world-readable.
6. **Factory reset is the class-wise inverse and the device-transfer path — never credential
   recovery** (first-run-onboarding §7): wipe `enrollment/` + `/home` + human userdb rows, rotate
   machine-id, preserve `factory/` + `identity/`; declared accounts re-materialise at next boot.
   A forgotten credential is fixed by `sudo passwd <user>` from another `wheel` user, or root
   over TTY/SSH/recovery shell. Doc 12 §6's "final fallback is wipe, not a root shell" is
   **overruled by invariant 10**. *Reverses rev 1's decision 6.*
7. **Out-of-band provisioning exists on every profile from first boot.** The device is an
   ordinary Linux host reachable from a device the wearer holds: (a) SSH over a USB Ethernet
   gadget (the postmarketOS initramfs pattern, `references/pmaports/main/postmarketos-initramfs/init_functions.sh:12-15, 836-963`);
   (b) a local web UI over the USB link, over a **headset-hosted open hotspot with a captive
   portal that exists only while the device is unprovisioned** (no network profile configured
   *and* no password set; afterwards an administrator-controlled setting, never automatic), and
   over the LAN. Both share the physical-possession trust class of a TTY. The portal page is a
   launcher; the real UI targets a normal browser tab. The web tool (Cockpit vs purpose-built),
   portal mechanics, and the passwordless-default-user login wiring (identical for SSH and web)
   are open questions decided by the research/42 findings review. A native phone app is optional
   sugar over the same surfaces, its app-store dependency recorded as an ethos cost.

## Alternatives considered

- **A pre-login onboarding wizard under greetd** (rev 1's design; `gnome-initial-setup` shape):
  withdrawn — it exists on desktops to fill the gap an installer leaves; a declared image has no
  gap, and the wizard machinery (dispatcher, marker-as-gate, self-authorising provisiond path)
  was the largest pre-auth privileged surface in the design. Rule 3 of AGENTS.md ("no mandatory
  wizards standing between the user and their machine") settles the residue.
- **GDM's runtime zero-users fallback translated to greetd** (a one-screen account bootstrap):
  rejected — see decision 4; a build assertion catches the misconfiguration, userborn
  re-materialises declared accounts after any wipe, and the fallback would have kept alive the
  one provisiond conversation with no authenticated principal.
- **Imperative user creation at first run** (`mutableUsers = true` on the appliance profile):
  rejected — the declared account already exists; on multi-user, ADR 0018's userborn wiring is
  the mutability mechanism, driven by standard tools.
- **systemd-homed**: not adopted for v1 (ADR 0018 rev 3 keeps it condition-shaped on NixOS
  declarative homed support).
- **`ConditionFirstBoot` as the first-run signal**: rejected — wrong across A/B slot
  replacement in both directions.
- **A default password on the default image** (postmarketOS's `pmbootstrap install` sets one,
  `references/pmbootstrap/pmb/install/_install.py:275, 1324`): not decided here — one of the
  candidates for the passwordless-default-user question (decision 7, first-run §9).
- **BLE GATT credential provisioning** (Improv Wi-Fi / Fast Pair class): recorded in research/42
  as the bespoke-surface alternative to the hotspot; not adopted.

## Consequences

- The F-track in [implementation-path.md](../implementation-path.md) §2 changes shape: B3 loses
  the dispatcher (greetd runs the greeter or the session directly), F2 becomes shell-plane
  session content downstream of M1 rather than a restricted compositor mode, and G2 needs a
  *declared* account rather than "enrollment".
- Registry: the dispatcher wrapper and the `--oobe` mode are removed; `mura-provisiond` shrinks to
  PIN enrollment + guest token gate; new rows for the welcome surface, sshd-over-USB-gadget, the
  provisioning hotspot/portal, and the web provisioning tool.
- The contract drops `mura.xr.session.provisioning.*` and gains
  `mura.xr.session.allowNoDeclaredAccount` plus the declared-account assertions
  ([lib/contract](../../../lib/contract/default.nix), tests in `tests/contract.nix`).
- The word "telemetry" is removed from the corpus; no setting refers to it.
- The recovery environment (where factory reset runs) remains a named open item
  (first-run-onboarding §9), joined to each family's recovery story.
