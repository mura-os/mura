# ADR 0017: First-run provisioning — the image is the installation, silent F1, the welcome surface, out-of-band access

**Status:** accepted; **amended 2026-09-24 (rev 2)** — decisions 1, 3, 4 and 6 rewritten,
decision 7 added; **rev 2.1 same day** — the [research/42](../../research/42-input-bootstrap.md)
review ruled: decision 2 (one credential), decision 3 (welcome contents; provisiond = guest gate
only), decision 7 (Cockpit, portal, passwordless wiring), decision 8 added (the input floor),
decision 9 added (`pairing/` class); **rev 2.2 same day** — security review of the passwordless
posture: decisions 3 and 7 amended (no polkit own-password rule — `passwd` is the gate; no
`nullok` on sudo/polkit — admin requires a password; SSH key-only off the USB subnet; Cockpit
bound to trusted links; hotspot WPA2 with an in-headset PSK; all static); **rev 2.3 same day,
D2** — decision 7: no `PermitEmptyPasswords`/`nullok` over SSH (OpenSSH+PAM finding, measured
in the VM test); decision 3: the credential hint is an owner-checked file in a sticky
directory, not a root-published mirror; **rev 2.4 same day — the regular-Linux-PC correction:**
decision 7 rewritten (sshd on every profile with upstream defaults, the key-only/`Match`
scoping withdrawn; the setup web app is bespoke and lives with the hotspot, Cockpit dropped;
"set up" = the explicit `setup-complete` marker), decision 2 (the password is the wearer's
choice; passwordless + declared key), decision 3 (welcome-surface authority verified per card;
time zone after the password — **withdrawn in rev 2.5**), decision 10 added (`mura-setup`, one
program in two instances), seven alternatives recorded; **rev 2.5 same day** — decision 3: the
time-zone step is derived after Connect and independent of the password step, its in-headset
authority an open item surveyed in research/54; dismiss = finish; Wi-Fi user-scoped for a
passwordless session user (rulings 2026-09-24). Rev 1 (2026-09-23)
designed a pre-login onboarding wizard; rev 2 records why it does not exist.
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
(F2), sshd, and — only until setup is finished — a hotspot plus the `mura-setup` service;
nothing on the frame path; no steady-state tenant.

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
   are in your session; a password is the wearer's choice, offered by the welcome surface. The
   declared account is the account on every profile; **its password is mutable state**
   (`initialHashedPassword = ""`, `users.mutableUsers = true`, userborn's persisted userdb — the
   ADR 0018 wiring, on the default image too; rev 2.2 correction found at D0: `mutableUsers =
   false` would re-impose the empty password at every activation). "Paths" are gone from the
   vocabulary: there is the image and the two profiles
   it may declare. *Amends rev 1's "fixed declared owner account": the name is `mura`, and the
   word "owner" is not a role (ADR 0018 rev 3).*
2. **One credential — the Unix password; a PIN is a numeric one** *(rev 2.1; supersedes rev 2's
   stacked `pam_mura_pin`)*. No dedicated PIN module and no `enrollment/<user>/secret/` exist.
   A digits-only password writes a **non-secret `numeric-credential` hint** as the user's own
   file in the sticky `state/credential-hint/` directory (D2; rev 2.1 said `enrollment/<user>/`)
   that makes the greeter and lock render a digit pad; any other password
   renders the keyboard path ([multi-user.md §3](../multi-user.md), ADR 0018 rev 3.1). Standard
   PAM stacks everywhere, declared through NixOS modules, never hand-edited. **The password is
   the wearer's choice and the OS does not grade it** (rev 2.4): `pam_faillock` guards every
   password equally; the rev 2.2 idea that SSH scoping should "carry the weakness" of a short
   numeric password is withdrawn with that scoping. Doc 12 option (b) is thereby **not**
   adopted; the rev-1 "owner-password-is-PIN bridge" stays withdrawn. This closes ADR 0007's
   open question. Consequence for the passwordless default account (rev 2.4): OpenSSH refuses
   empty passwords, so on the shipped image SSH follows `passwd`; a builder's declared key
   (`users.users.mura.openssh.authorizedKeys.keys`) gives SSH from first boot.
3. **No pre-login onboarding exists. F2 is a first-session welcome surface.** The multi-user
   profile's greetd `default_session` is `zxr --greeter`, full stop; the appliance profile's
   `initial_session` is the session. The dispatcher wrapper, the `/run/mura/provisioned` flag,
   `zxr --oobe`, the wizard ladder, the commit ceremony, and the privacy/consent step are
   **removed**. What remains is ordinary shell-plane session content (first-run-onboarding §4):
   per-item gated (offered only when the target value is undefined; all defined ⇒ never shown),
   skippable, re-runnable from settings, per person. **Contents (rev 2.1, ruled after the
   research/42 review): see → walk → speak** — IPD (language-free, per `ipd.source` class),
   peripherals (controllers/Bluetooth), the account's language (declared preselect, short list,
   type-to-filter), Wi-Fi or skip, set a password or skip, **then** time zone (and hostname),
   then a "finish" card carrying "how to reach this device" (first-run-onboarding §4.2). Floor
   height and boundary are spatial-mapping's, not the surface's. **Rev 2.4 — the surface runs
   as the logged-in user with active-session authority only, and each card's mechanism is
   verified against the pinned clones (first-run §4.3):** the account's language via
   AccountsService `change-own-user-data` (`yes`), Bluetooth via BlueZ's default D-Bus policy,
   Wi-Fi via NetworkManager `settings.modify.own` (`yes`) — a user-scoped connection for a
   passwordless user, a system connection once `modify.system`'s `auth_admin_keep` can be met —
   and the password item drives **`passwd` in a pty** (rev 2.2: no polkit rule relaxes
   `change-own-password`, which would let any session process set the wearer's password).
   Time zone and hostname are `auth_admin_keep`; **rev 2.5**: the step is *derived* (phone zone
   on the web app, network/geoclue in-headset) after Connect and does not depend on the password
   step, which is optional; how the in-headset confirm is authorised is an open item with the
   survey in [research/54](../../research/54-first-run-authority.md) recommending a Mura rule
   for exactly those two actions (SteamOS precedent) — decider: the project owner. Rev 2.4's
   "after the password card" is withdrawn: no shipping first-run flow orders a step behind an
   optional credential. Rev 2.1's "via `localed`/`timedated`" as an unprivileged session write
   could not have worked for a passwordless wheel user. Ruled 2026-09-24: **dismiss = finish**
   (closing the surface writes `setup-complete`), and a passwordless session user's Wi-Fi is a
   **user-scoped** connection widened only when polkit allows. The surface is
   the in-session instance of the one `mura-setup` program (decision 10). `mura-provisiond` is
   left with **exactly one load-bearing job, the guest token gate** (plus the polkit-gated
   account-admin convenience path) and **has no conversation authorised by the absence of
   state**.
4. **A greeter image declares its first account; the build asserts it.** Multi-user profile ⇒
   at least one `isNormalUser` account, else evaluation fails, with the escape hatch
   `mura.xr.session.allowNoDeclaredAccount` (the `users.allowNoPasswordLogin` pattern —
   NixOS's own check is silent for us because the profile runs `mutableUsers = true` under
   userborn). GDM's runtime fallback (`ListCachedUsers` empty → initial-setup) exists for OEM
   preinstall; our OEM-preinstall equivalent is the default image, so the fallback and its
   privileged surface are deleted rather than translated. A greeter facing zero pickable accounts
   at runtime still renders (free-text username, power menu); recovery is standard root. **F1's
   per-task markers on `/persist` remain authoritative over `ConditionFirstBoot`** (a fresh A/B
   slot resembles first boot); no single "provisioned" marker exists and no F1 marker gates UI.
   (Rev 2.4: the one marker that gates the *provisioning surfaces* — hotspot, setup web app —
   is `state/setup/setup-complete`, a recorded user decision, decision 10; it is not an F1
   marker and gates no session UI.) machine-id persists across updates and **rotates on
   factory reset**.
5. **Secrets are never Nix option values.** Password hashes, Wi-Fi credentials, Bluetooth link keys, and device keys exist
   only as runtime state under protected persistent storage, or as declared *paths*
   (`hashedPasswordFile`, keyfile secrets) — the store is world-readable.
6. **Factory reset is the class-wise inverse and the device-transfer path — never credential
   recovery** (first-run-onboarding §7): wipe `enrollment/` + `/home` + human userdb rows, rotate
   machine-id, preserve `factory/` + `identity/`; declared accounts re-materialise at next boot.
   A forgotten credential is fixed by `sudo passwd <user>` from another `wheel` user, or root
   over TTY/SSH/recovery shell. Doc 12 §6's "final fallback is wipe, not a root shell" is
   **overruled by invariant 10**. *Reverses rev 1's decision 6.*
7. **Out-of-band access exists on every profile from first boot — the headset is a regular
   Linux PC** *(rewritten rev 2.4; rev 2.1/2.2/2.3 wording superseded, recorded under
   Alternatives)*. Two mechanisms, nothing invented (first-run-onboarding §5):
   (a) **sshd, on, with OpenSSH's own defaults** — password authentication on every interface,
   `PermitEmptyPasswords no`, no `Match` scoping; `services.openssh.enable = mkDefault true` in
   `modules/os` so a profile may turn it off. The transport a wearer without a network uses is
   the postmarketOS USB Ethernet gadget from the initramfs
   (`references/pmaports/main/postmarketos-initramfs/init_functions.sh:12-15, 836-963`) with a
   `systemd-networkd` DHCP server on `usb0`, NM-unmanaged. Internet exposure is the network's
   job, as for any laptop; `pam_faillock` guards every password. A passwordless `mura` has SSH
   after `passwd` or, on an image the wearer built, from first boot with a declared key.
   (b) **The setup web app — Mura's replacement for the consumer phone companion app** — is
   the phone-facing instance of `mura-setup` (decision 10), reached over the USB gadget or over
   a **headset-hosted WPA2 hotspot with a captive portal** whose per-boot 8-digit PSK is shown
   inside the headset (rev 2.2: radio range is not cable possession). It is **never served on
   the LAN**; its authorisation is possession of the cable or the PSK — the same TTY-equivalent
   trust as SSH over the cable. **Both the hotspot and the web app exist until the wearer
   finishes or dismisses setup**, which writes `state/setup/setup-complete`; afterwards both are
   ordinary administrator settings, never automatic again. The hotspot is additionally
   condition-shaped on "no active NetworkManager connection other than its own", is `PartOf`
   `mura-setup.service`, and has a per-boot idle timeout with no client associated (schema
   value, proposed 10 min). Portal = NM AP mode + shared IPv4 with a `dnsmasq-shared.d`
   wildcard address and DHCP option 114, probe redirect to the launcher page; **the flow
   completes on the link it started on** — no LAN handoff; on chips without concurrent AP+STA,
   Wi-Fi activation is part of *finish* and the marker is the last write (first-run §5.2).
   **`sudo` and polkit `auth_admin` stay standard — a passwordless account cannot administer
   until it sets a password with `passwd`**, which asks no old password. Nothing in the system
   detects "no password" at runtime. A native phone app is optional sugar over the same
   surfaces, its app-store dependency recorded as an ethos cost; BLE GATT provisioning
   (Improv/Fast Pair class) rejected for v1.
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
10. **`mura-setup` — one program, one library, two instances** *(rev 2.4)*. The onboarding
    logic is written once and run twice, the gnome-initial-setup pattern
    (`GIS_DRIVER_MODE_NEW_USER` / `_EXISTING_USER`,
    `references/gnome-initial-setup/gnome-initial-setup/gnome-initial-setup.c:218-249`): a
    **system-service instance** under its own identity with **scoped polkit rules for exactly
    the setup actions** (NM `settings.modify.system`, `timedate1.set-timezone`,
    `hostname1.set-static-hostname`, `accounts.user-administration` for the autologin account
    if any, BlueZ agent — the shape of `data/20-gnome-initial-setup.rules.in:8-30`, narrowed
    to exact actions), serving the captive-portal launcher and the web app on port 80 of the
    gadget and hotspot addresses only, `ConditionPathExists=!…/setup-complete`, the standard
    daemons doing the privileged work (no bespoke root helper, no PAM login — link possession
    authorises); and the **session instance** — the welcome surface of decision 3, running as
    the logged-in user with active-session authority, not marker-gated. Every card means the
    same thing in both instances (the library asks polkit `CheckAuthorization` per card and
    greys or reorders; both converge on system state). Cockpit is dropped as the tool and kept
    as a mechanism reference. Budget impact: one condition-shaped system service that exits
    with the marker; nothing on the frame path.

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
- **Imperative user creation at first run**: rejected — the declared account already exists on
  every profile. (`mutableUsers = true` itself is *not* rejected: it is what lets the wearer's
  `passwd` persist; ADR 0018's userborn wiring is the mutability mechanism on every profile.)
- **`mutableUsers = false` on the default image** (rev 2's "Steam Deck `deck` mechanism"
  reading): rejected at D0 — it regenerates `shadow` from configuration at every activation and
  would erase the password the wearer set.
- **systemd-homed**: not adopted for v1 (ADR 0018 rev 3 keeps it condition-shaped on NixOS
  declarative homed support).
- **`ConditionFirstBoot` as the first-run signal**: rejected — wrong across A/B slot
  replacement in both directions.
- **A default password on the default image** (postmarketOS's `pmbootstrap install` sets one,
  `references/pmbootstrap/pmb/install/_install.py:275, 1324`; the pre-2022 Raspberry Pi
  `pi:raspberry`): rejected, reconfirmed rev 2.4 — a well-known `wheel` credential reachable
  over the network until changed, which is exactly why the Raspberry Pi Foundation dropped
  theirs; pre-expiring it (`chage -d 0`) would risk greetd autologin on `PAM_NEW_AUTHTOK_REQD`.
  The passwordless account (the Steam Deck's `deck`) plus a builder-declared SSH key is the
  standard pair.
- **SSH key-only everywhere except a USB-subnet `Match Address` block** (rev 2.2's posture,
  with `PermitEmptyPasswords` in the block until rev 2.3): **withdrawn rev 2.4** — hardening
  beyond what any Linux distribution ships, nothing in the mandate asked for it, it existed to
  compensate for a password choice that is the wearer's, and every subsequent problem (the
  `unixAuth` override, the dev-profile loosening, the harness overrides, the
  `PermitEmptyPasswords` dead end) grew from it. Upstream defaults plus `pam_faillock`.
- **`PermitEmptyPasswords yes` on any link** (rev 2.2): rejected at D2 by measurement — with it,
  OpenSSH's initial `none` method authenticates with an empty password in the parent process
  while the real attempt runs in a forked helper, and `pam_setcred` replays the cached failure
  for every password login once the account has one.
- **Cockpit as the setup web app** (rev 2.1; scored in research/42 §6.3): **superseded rev 2.4**
  by the bespoke `mura-setup` (decision 10) — the mandate is a provisioning companion that
  lives with the hotspot, not a permanent admin console with socket-binding rules and a LAN
  toggle; Cockpit stays a mechanism reference (`passwd_self` pty flow, NM Wi-Fi dialog). A
  wearer who wants Cockpit installs it; that is not Mura's concern.
- **Ending provisioning on "a network profile exists and a password is set"** (rev 2.1's
  hotspot condition): superseded rev 2.4 by the explicit `setup-complete` marker — the second
  half was a runtime detection of the passwordless state, which decision 7 forbids; the network
  half survives only as "the hotspot yields to an active connection".
- **A single setup process serving HTTP and rendering in-headset** (considered rev 2.4):
  rejected — a system identity cannot connect to the session user's Wayland socket
  (`$XDG_RUNTIME_DIR`, mode 0700) without a bespoke compositor socket and a cross-user client;
  ordinary Unix session isolation, not a zxr policy. The goal behind it — no duplicate
  onboarding implementation — is met by one program run as two instances (decision 10).
- **Running setup as `mura` with a polkit rule granting the active session the setup actions
  while unfinished** (considered rev 2.4): rejected — the dynamic passwordless-window mechanism
  below, widened to every process in the session.
- **A webview rendering the web app in-headset** (considered rev 2.4): rejected — a browser
  engine in the image for a page we would author natively anyway.
- **Serving the web app on the LAN once a password exists** (considered rev 2.4): rejected —
  it re-grows a PAM login and per-user privilege, i.e. Cockpit's job; LAN access is SSH.
- **A stacked optional PIN module** (`pam_mura_pin`, the fprintd model; rev 2's decision 2):
  withdrawn in rev 2.1 — a second credential and a second secret store for what is, in Unix
  terms, just a short password; the digit pad is a rendering keyed off a non-secret hint.
- **BLE GATT credential provisioning** (Improv Wi-Fi / Fast Pair class): recorded in research/42
  as the bespoke-surface alternative to the hotspot; rejected for v1.
- **`nullok` on the sudo stack while passwordless** (rev 2.1's wiring): rejected in the rev 2.2
  review — it made `sudo -S <<< ""` from any session process, or any shell obtained as `mura`
  over USB or radio, into root. Admin requires a password; `passwd` is the gate.
- **A Mura polkit rule granting `change-own-password` without `auth_admin`** (rev 2.1): rejected
  — any session process could set the wearer's password; AccountsService defaults to
  `auth_admin` for this reason, and `passwd` already asks no old password for a passwordless
  account.
- **An open provisioning hotspot** (rev 2.1): superseded by WPA2 with an in-headset PSK — radio
  range is not cable possession, and the unprovisioned state can last indefinitely offline.
- **A dynamic "while no password" mechanism** (path unit on `shadow`, toggled drop-ins):
  rejected — with the three rulings above every posture is static and nothing needs to detect
  the passwordless state.
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
  welcome surface (specified), out-of-band access (specified: sshd upstream + hotspot/portal),
  `mura-setup` (rev 2.4: one row, both instances), and the pre-login Bluetooth pairing agent;
  the input-method row gains its protocol set; the polkit-agent gap is cross-referenced from F2.
- Rev 2.4 code consequence: `modules/os/default.nix` enables sshd (`mkDefault`), and
  `modules/os/policy.nix` no longer touches `services.openssh` at all (D2 posture correction,
  commit "D2 posture correction"); the VM tests carry a fixture SSH key to prove the
  self-builder path.
- The contract drops `mura.xr.session.provisioning.*`, gains
  `mura.xr.session.allowNoDeclaredAccount` plus the declared-account assertions, and gains the
  `mura.hardware.input.*` facts with the `selectRole` assertion
  ([lib/contract](../../../lib/contract/default.nix), tests in `tests/contract.nix`).
- ADR 0018 rev 3.1 amends its decision 3 in step; ADR 0007's PIN pointer is updated.
- The word "telemetry" is removed from the corpus; no setting refers to it.
- The recovery environment (where factory reset runs) remains a named open item
  (first-run-onboarding §9), joined to each family's recovery story.
