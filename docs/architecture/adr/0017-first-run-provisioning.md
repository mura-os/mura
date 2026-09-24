# ADR 0017: First-run provisioning — the image is the installation, silent F1, the welcome surface, out-of-band access

**Status:** accepted; **amended 2026-09-24 (rev 2)** — decisions 1, 3, 4 and 6 rewritten,
decision 7 added; **rev 2.1 same day** — the [research/42](../../research/42-input-bootstrap.md)
review ruled: decision 2 (one credential), decision 3 (welcome contents; provisiond = guest gate
only), decision 7 (Cockpit, portal, passwordless wiring), decision 8 added (the input floor),
decision 9 added (`pairing/` class). Rev 1 (2026-09-23) designed a pre-login onboarding wizard;
rev 2 records why it does not exist.
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
2. **One credential — the Unix password; a PIN is a numeric one** *(rev 2.1; supersedes rev 2's
   stacked `pam_mura_pin`)*. No dedicated PIN module and no `enrollment/<user>/secret/` exist.
   A digits-only password writes a **non-secret `numeric-credential` hint** to
   `enrollment/<user>/` that makes the greeter and lock render a digit pad; any other password
   renders the keyboard path ([multi-user.md §3](../multi-user.md), ADR 0018 rev 3.1). Standard
   PAM stacks everywhere, declared through NixOS modules, never hand-edited; `pam_faillock` and
   the §5.3 SSH scoping carry the weakness of a short numeric password the user chose. Doc 12
   option (b) is thereby **not** adopted; the rev-1 "owner-password-is-PIN bridge" stays
   withdrawn. This closes ADR 0007's open question.
3. **No pre-login onboarding exists. F2 is a first-session welcome surface.** The multi-user
   profile's greetd `default_session` is `zxr --greeter`, full stop; the appliance profile's
   `initial_session` is the session. The dispatcher wrapper, the `/run/mura/provisioned` flag,
   `zxr --oobe`, the wizard ladder, the commit ceremony, and the privacy/consent step are
   **removed**. What remains is ordinary shell-plane session content (first-run-onboarding §4):
   per-item gated (offered only when the target value is undefined; all defined ⇒ never shown),
   skippable, re-runnable from settings, per person. **Contents (rev 2.1, ruled after the
   research/42 review): see → walk → speak** — IPD (language-free, per `ipd.source` class),
   peripherals (controllers/Bluetooth), locale (declared preselect, short list, type-to-filter),
   time zone, Wi-Fi or skip, set a password or skip, a "how to reach this device" card
   (first-run-onboarding §4.2). Floor height and boundary are spatial-mapping's, not the
   surface's. Privileged writes go through standard mechanisms (a Mura polkit rule for
   frictionless own-password change, NetworkManager D-Bus policy, `localed`/`timedated`, the
   BlueZ agent API); `mura-provisiond` is left with **exactly one load-bearing job, the guest
   token gate** (plus the polkit-gated account-admin convenience path) and **has no conversation
   authorised by the absence of state**.
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
5. **Secrets are never Nix option values.** Password hashes, Wi-Fi credentials, Bluetooth link keys, and device keys exist
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
   over the LAN. Both share the physical-possession trust class of a TTY. **Rev 2.1 rulings:**
   the web app **is Cockpit** (`services.cockpit`; its NetworkManager page joins Wi-Fi, `passwd`
   sets the password, PAM login) plus a "Mura setup" Cockpit plugin page for the guided flow and
   a static captive-portal launcher in front — a bespoke web app only if Cockpit's Wi-Fi dialog
   fails on hardware; RaspAP/LuCI/wifi-connect/comitup rejected as the tool. Portal = NM AP mode
   + shared IPv4 with a `dnsmasq-shared.d` wildcard address and DHCP option 114, probe redirect
   to the one static launcher page (first-run-onboarding §5.2). Passwordless `mura`: while no
   password is set, `PermitEmptyPasswords` only under `Match Address` for the USB/hotspot
   subnets with `nullok` on the sshd and sudo stacks; the setup page offers a password first
   (§5.3). A native phone app is optional sugar over the same surfaces, its app-store dependency
   recorded as an ethos cost; BLE GATT provisioning (Improv/Fast Pair class) rejected for v1.
8. **The input floor is a conformance requirement** *(rev 2.1)*: IMU head-aim plus the HMD's
   own buttons, dwell where a button is unusable; every pre-login scene and every welcome item
   is fully operable at the floor on every target with nothing configured
   (first-run-onboarding §4.4). Buttons reach the compositor through libinput with logind's
   power-key handling ignored or inhibited (the Steam Deck `powerbuttond` arrangement, never a
   grab inside Monado); constraint 7's stabiliser is the greeter's first consumer (dwell
   400–600 ms, targets ≥2.5–3°, from the settings schema); USB HID works at the greeter with
   zero configuration; zxr implements layer-shell + `virtual-keyboard-v1` + `input-method-v2`
   so the wearer runs any on-screen keyboard, while the auth scene keeps its own floor-operable
   digit pad. Contract facts: `mura.hardware.input.{hmdButtons,selectRole,controllers,bluetooth,concurrentApSta,proximitySource}`
   ([lib/contract](../../../lib/contract/default.nix)). Grounding: research/42 §4, §7 (every
   relevant Monado driver keeps a 3DoF path; PICO Head Control Mode and Steam Frame Aux as
   mechanism precedents; ≈10 WPM measured). A Monado 3DoF HMD driver per Mura target is new
   code (none exists upstream for IIO/SSC) — implementation work, not a design gap.
9. **`pairing/` is a persistent-state class** *(rev 2.1)*: `/var/lib/bluetooth` (adapter state
   and per-peer link keys) bound onto `/persist/mura/pairing/`, exists only when
   `mura.hardware.input.bluetooth`; survives A/B updates, **wiped by factory reset** — link keys
   are shared secrets and the next owner must not inherit the previous owner's paired devices
   (the phone and consumer-headset norm; bundled controllers re-pair after a reset).

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
  `references/pmbootstrap/pmb/install/_install.py:275, 1324`): not adopted — a printed/known
  default password is a credential nobody chose; the subnet-scoped empty-password wiring of
  decision 7 keeps "no password" honest until the wearer sets one.
- **A stacked optional PIN module** (`pam_mura_pin`, the fprintd model; rev 2's decision 2):
  withdrawn in rev 2.1 — a second credential and a second secret store for what is, in Unix
  terms, just a short password; the digit pad is a rendering keyed off a non-secret hint.
- **BLE GATT credential provisioning** (Improv Wi-Fi / Fast Pair class): recorded in research/42
  as the bespoke-surface alternative to the hotspot; rejected for v1.
- **Headset-shows-QR for phone pairing**: rejected outright — the display is behind lenses.
- **Monado `qwerty`/evdev driver as the button path**: rejected — SDL-window-bound and disables
  other drivers; the compositor reads HMD buttons via libinput (decision 8).
- **Grabbing the power key inside Monado** (Galaxy XR fork): recorded, not followed — logind
  ignore/inhibit + libinput is the standard arrangement.

## Consequences

- The F-track in [implementation-path.md](../implementation-path.md) §2 changes shape: B3 loses
  the dispatcher (greetd runs the greeter or the session directly), F2 becomes shell-plane
  session content downstream of M1 rather than a restricted compositor mode, and G2 needs a
  *declared* account rather than "enrollment".
- Registry: the dispatcher wrapper and the `--oobe` mode are removed; the PIN-module row becomes
  the digit-pad rendering row; `mura-provisiond` shrinks to the guest token gate; rows for the
  welcome surface (specified), out-of-band access (specified: Cockpit + portal), and the
  pre-login Bluetooth pairing agent; the input-method row gains its protocol set.
- The contract drops `mura.xr.session.provisioning.*`, gains
  `mura.xr.session.allowNoDeclaredAccount` plus the declared-account assertions, and gains the
  `mura.hardware.input.*` facts with the `selectRole` assertion
  ([lib/contract](../../../lib/contract/default.nix), tests in `tests/contract.nix`).
- ADR 0018 rev 3.1 amends its decision 3 in step; ADR 0007's PIN pointer is updated.
- The word "telemetry" is removed from the corpus; no setting refers to it.
- The recovery environment (where factory reset runs) remains a named open item
  (first-run-onboarding §9), joined to each family's recovery story.
