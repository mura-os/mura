# First run: the image is the installation, silent provisioning, the welcome surface, and factory reset

**Status:** accepted design (2026-09-23; **rev 2, 2026-09-24 — the image-is-the-installation
reframe**; **rev 2.1 same day — the [research/42](../research/42-input-bootstrap.md) review
ruled: input floor, welcome-surface contents, one credential, out-of-band mechanisms**;
**rev 2.2 same day — security review of the passwordless posture: admin requires a password,
`passwd` is the gate, SSH key-only off the USB subnet, Cockpit on trusted links, PSK hotspot,
hint mirror; all static**; **rev 2.3, D1 — the account database, machine-id and other `/etc`
state persist through a mutable `/etc` overlay whose upper layer is the `etc-rw/` class**;
**rev 2.4, D2 — no `PermitEmptyPasswords` over SSH (OpenSSH's `none` probe poisons the PAM
handle for every later password login); the credential hint is a sticky-directory file with an
owner check, not a mirror; faillock is a schema value with `conf=`**; **rev 2.5, same day — the
regular-Linux-PC correction: sshd on every profile with upstream defaults (the rev 2.2 key-only
/ `Match Address` scoping withdrawn), the password is the wearer's choice, Cockpit dropped for
a bespoke setup web app that is the phone-facing instance of one `mura-setup` program (§5.1),
"set up" = an explicit `setup-complete` marker, the welcome surface's authority split verified
per card against the pinned clones (§4.3)**; **rev 2.6, same day — two rulings recorded (dismiss =
finish; Wi-Fi joined by a passwordless session user is user-scoped) and the time-zone step
rewritten as derived-after-Connect with its authority an open item surveyed in research/54**).
Decision record: [ADR 0017](adr/0017-first-run-provisioning.md) (amended in place).
**What this covers:** everything between "the image was flashed" and "a person is using their
session": what an installer would collect and where it lives here (§1), the persistent-state
classes (§2), silent machine provisioning F1 (§3), the first-session welcome surface F2 (§4),
out-of-band provisioning (§5), factory-vs-user calibration (§6), and factory reset as the
inverse (§7). Slots into the boot chain as the F-track
([implementation-path.md §2](implementation-path.md)).
**What rev 2 removed, explicitly:** the pre-login onboarding wizard (`zxr --oobe`), the
greetd dispatcher wrapper and its `/run/mura/provisioned` flag, the transactional
"provisioning marker" as a UI gate, provisiond's self-authorizing create-first-account
conversation, and the privacy/consent wizard step with its telemetry item (this project has no
telemetry; §4.1).
They were installer-brain: a desktop distro needs a pre-user session because an *installer*
hands over a machine with no users; a flashed image has no such gap (§1).
**Grounding:** "XDG" below means the Base Directory spec (per-user preference/state split,
[specs/settings-schema.md §2](../../specs/settings-schema.md)). Precedents cited for
*mechanism only*: GDM's `wants_initial_setup()` (runs a pre-user session only when zero users
exist — [research/41 §1.2](../research/41-multi-user-login-landscape.md)), which shows exactly
which gap a first-run wizard fills and therefore why an image with a declared account has none;
the Steam Deck (fixed `deck` account declared in the image, setup inside the auto-logged-in
session); postmarketOS (USB-network + SSH from the initramfs, [§5](#5-out-of-band-provisioning)).
Quest's forgotten-passcode-ends-in-wipe posture ([research/12 §6](../research/12-lock-screens-and-appliance-login.md))
is recorded as the anti-pattern §7 overrules.
**Budget impact** (overview invariant 9): F1 is one-shot boot-time work off the frame path; F2
is ordinary shell-plane session content on the session budget; neither adds a steady-state
tenant. Out-of-band access (§5) adds sshd (upstream defaults, always) and, only until setup is
finished, a hotspot + the `mura-setup` service — off the frame path, condition-shaped.

## 1. The image is the installation

On a desktop distribution an *installer session* collects the first user and password, the
locale, the network, and whether to log in automatically, then hands over a machine that is
ready to use. Mura has no installer session: **flashing is installing**, and everything an
installer would collect is **declared in the image** — `users.users.<name>` with
`isNormalUser`, `hashedPasswordFile`, `extraGroups = [ "wheel" ]`; locale; NetworkManager
profiles with path-based secrets; `mura.xr.session.autoLogin` or `.greeter`. Exactly as a
declared NixOS machine. Three consequences:

1. **The default image is a declared configuration too.** It declares user **`mura`**, no
   password, member of `wheel`, and `mura.xr.session.autoLogin = "mura"`. Power on, put it on,
   you are in your session. **The password is the wearer's choice** — which one, how strong,
   whether at all: the welcome surface (§4) offers one, the lock engages only once a credential
   exists (ADR 0007), and **there is exactly one credential** — the Unix password; a
   digits-only one gets a digit pad rendered for it, nothing more (§4.2, [multi-user.md §3](multi-user.md)).
   A passwordless `mura` is a full *user*; **administration (`sudo`, polkit `auth_admin`
   actions) requires setting a password first** — `passwd` asks no old password for a
   passwordless account, and that is the gate (§5.3; the Steam Deck's `deck` account has exactly
   this posture, and so does NixOS itself). So does SSH: OpenSSH refuses empty passwords by
   default, so on the shipped image `ssh mura@…` works after `passwd` — or from first boot with
   a key the builder declared (§5.3). The Steam Deck's fixed `deck` account is the mechanism
   precedent; the pre-2022 Raspberry Pi `pi:raspberry` default password is the rejected one
   (ADR 0017 alternatives). There is no "appliance wizard": what used to be called the
   appliance path is simply a declared image with `autoLogin` set.
2. **The first account of a multi-user image is declared in the image as well** — like every
   distro installer's first account, it is an ordinary user in `wheel`. A **build-time
   assertion** enforces this: a greeter profile must declare at least one human account
   (`isNormalUser`), with an escape hatch (`mura.xr.session.allowNoDeclaredAccount`) mirroring
   NixOS's own `users.allowNoPasswordLogin` for the administrator who insists
   ([lib/contract](../../lib/contract/default.nix)). Consequently **no runtime account-bootstrap
   screen exists**, no pre-login onboarding mode exists, and `mura-provisiond` has no
   self-authorizing conversation: the account database is never mutated by anything that is
   not an authenticated administrator. A greeter that nevertheless finds zero pickable accounts
   (corrupted userdb, userborn failure) still renders — free-text username entry and the power
   menu — and recovery is the standard one: root over TTY/SSH/recovery shell, `useradd`.
3. **"Setting up" is per-person and happens inside a session.** Whatever cannot be declared —
   a room's floor height and boundary, a person's IPD, a peripheral pairing, a password the
   wearer chose not to declare — is offered by the first-session welcome surface (§4), per
   account, skippable, re-runnable from settings.

On the multi-user profile the account database is mutable through standard tools
(`useradd` over SSH works — [multi-user.md §1](multi-user.md)); the in-headset settings UI is a
polkit-gated convenience path executing the same operations. Secrets (password hashes, Wi-Fi
credentials, Bluetooth link keys, device keys) are **never Nix option values** — the store is world-readable;
declared secrets are *paths* (`hashedPasswordFile`, NetworkManager keyfiles with
`psk-flags`/agent-owned secrets), runtime secrets live only under protected persistent storage.

## 2. Persistent-state classes (normative)

`/var/lib/mura` binds onto `/persist/mura` (pulled in by `mura-persist-setup.service`;
[families/uefi-rauc](../../families/uefi-rauc/default.nix) is the first implementation). The
subtrees are **classes with different lifecycles**, and every consumer and reset path must
treat them by class, never the tree as one blob:

| Class | Contents | A/B update | Factory reset |
|---|---|---|---|
| `factory/` | factory calibration (panel/optics/camera intrinsics, per-unit, flashed at manufacture or bring-up) | survives | **survives** (invariant 4) |
| `identity/` | device keys, attestation material | survives | survives; regenerated only by explicit re-provisioning |
| `enrollment/<user>/` | user calibration (`calibration/`, §6) and the non-secret `numeric-credential` hint that selects the digit pad ([multi-user.md §3](multi-user.md)); no secret material — the credential is the Unix password in the userdb | survives | **wiped** |
| `pairing/` (exists only when `mura.hardware.input.bluetooth`) | BlueZ state — `/var/lib/bluetooth` bound here: adapter settings and per-peer **link keys** (shared secrets with each paired controller/keyboard/phone) | survives | **wiped** — link keys are secrets; the next owner must not inherit the previous owner's paired devices (every phone and consumer headset does the same; bundled controllers are re-paired after a reset) |
| `state/` | F1 per-task markers (`state/provisioning/<task>`), update/migration bookkeeping, quarantine records | survives | reset per settings-schema policy |
| `/persist/etc-rw/` (**its own class**, beside `mura/`; every profile — rev 2.3, D1) | the writable upper layer of the **`/etc` overlay** (NixOS `system.etc.overlay`, mutable): the account database (`passwd`/`shadow`/`group`, userborn hybrid mode — [multi-user.md §1.1](multi-user.md)), `/etc/machine-id`, NetworkManager system connections, anything else written into `/etc` at runtime | survives (the generated lower layer is per slot; the upper is per unit) | **wiped** — declared accounts re-materialise at next boot (userborn from the image), runtime-created accounts and network profiles are gone, **machine-id rotates** (privacy; machine identity is not hardware identity) |

Per-user preferences and remembered state stay in `$XDG_CONFIG_HOME` / `$XDG_STATE_HOME` on
`/home` (settings-schema §2); factory reset wipes `/home` wholesale.

## 3. F1 — silent machine provisioning

One-shot systemd units, no UI, no XR. Work: data-partition growth where the device needs it,
per-unit key generation into `identity/` (the SSH host key in `identity/ssh/` is the first —
generated idempotently by sshd's own keygen unit, D1), the `/persist/mura` skeleton (the setup
service's job), settings-store seeding (empty stores + the generation tag), nix-db rehydration
where the family requires it.

**Durable per-task markers are authoritative, `ConditionFirstBoot` is not.** Installing a fresh
root slot via an A/B update presents an empty slot-local `/etc/machine-id` to a system without
a persisted `/etc`, and looks like first boot to `ConditionFirstBoot`; provisioning must not
re-run there. Rules:

- Each F1 unit gates on **absence of its own marker** under `state/provisioning/<task>` — on
  `/persist`, so it sees through slot replacement. No single "provisioned" bit exists, and no
  F1 marker gates any UI. The reference implementation is `mura-f1-seed-state.service`
  (`ConditionPathExists=!…/seed-state`; idempotent body; marker committed by `rename(2)`) in
  `modules/os/persist.nix` — later F1 tasks copy it.
- **The one marker that does gate surfaces is not an F1 marker.** `state/setup/setup-complete`
  (rev 2.5) records a *decision the wearer made* — "I have finished (or dismissed) setup" — in
  the welcome surface or the setup web app (§5). It is the only condition on the provisioning
  hotspot and the `mura-setup` service; it gates no F1 task and no session UI. It lives in a
  sticky world-writable directory (the `credential-hint` shape, §4.3): the session user can
  create it, only its owner or root can remove it, and finishing early is benign. Factory reset
  wipes it with the rest of `state/`.
- `ConditionFirstBoot` is used only for genuinely *slot-local* concerns (nix-db rehydration
  class — work that must re-run per new rootfs).
- machine-id lives in the persisted `/etc` overlay (§2): PID 1 generates and commits it into
  the upper layer on the very first boot, before D-Bus/logind start; identity is stable across
  updates and rotates only on factory reset (wiping `etc-rw/`).

**Interrupted first boot is recovered by construction:** every F1 unit is idempotent and its
marker is written atomically (`rename(2)`) after the work completes; a power cut mid-F1 re-runs
the incomplete units on the next boot. No unit depends on a *partially* provisioned sibling —
dependencies are on markers, not on unit start order.

## 4. F2 — the first-session welcome surface

### 4.1 What it is, and what it is not

The welcome surface is **ordinary session content in the shell plane** — the VR analogue of
GNOME Tour or Plasma's Welcome Center — shown in a person's *first* session on this device and
reachable from settings forever after. It is not a pre-login stage, not a mode of the
compositor, not gated by a marker, and never mandatory. Rules (normative):

- **Per-item gating.** Each item targets specific values (an IPD, a floor height, a boundary,
  a password, a Wi-Fi profile, a locale…). **An item is offered only if its target is
  undefined** — not declared in the Nix configuration, not present in state. Config-declared ⇒
  not offered. Everything satisfied ⇒ **the surface does not appear at all.**
- **Never a wall.** Every item is skippable; skipping is never a trap because every item is
  re-runnable from settings. The session behind the surface is fully usable while it is up.
- **Per person, not per device.** Device-level facts (locale, Wi-Fi, time) are system settings
  — declared or changed in-session like on any Linux machine; the surface may offer shortcuts
  to them but owns none of them. Person-level facts (IPD, floor, boundary, credential, PIN)
  recur for every account: the appliance's `mura`, a multi-user image's first admin, and every
  member added later meet the same surface on their first login ([multi-user.md §6](multi-user.md)).
- **No consent ceremony.** Shipped defaults for presence sharing and capture are documented and
  conservative; changing them is using settings, and provenance tracking already records the
  change as the user's ([specs/settings-schema.md §3](../../specs/settings-schema.md)). **This
  project has no telemetry — none exists, none is planned, and no setting refers to it.**

### 4.2 Contents — decided (research/42 review, 2026-09-24)

**See, then walk, then speak.** The items, in the only order that works when nothing is
configured: the wearer must first be able to see the display clearly, then be able to point
and click well, and only then can text be shown in a language they read. Each item is
per-item gated (§4.1) and skippable; the list is closed — anything else is a setting.

1. **See — IPD.** Language-free: no text, only shapes. What it does follows
   `mura.hardware.ipd.source` (ADR 0011, [device-contract.md](device-contract.md)):
   `manual` — an alignment target (two shapes to bring into overlap / a sharpness pattern)
   while the wearer turns the wheel; `manual-sensed` — the same target plus the live readout
   and an in-range indication, the sensed value stored as the user's preference;
   `motorized-auto` — the ADR 0011 fixation-target measurement drives the servo; `stored` /
   `fixed` — never offered. Output: one value into `enrollment/<user>/calibration/`, applied to
   the session's Monado. This screen is also the wearer's first use of head-aim + button, so the
   input floor (§4.4) is learned without a word. Precedents, mechanism only: Quest 3's live
   readout + in-range indicator, the Index's sensed slider value, Vision Pro's automatic servo
   ([research/42 §1](../research/42-input-bootstrap.md)). Floor height and boundary are **not**
   welcome items — they belong to spatial mapping and appear when a feature first needs them.
2. **Walk — peripherals.** Pair controllers and Bluetooth devices; language-free (icons, device
   names as reported). BlueZ just-works pairing for HID needs no dialog; numeric comparison
   renders digits ([research/42 §3.3](../research/42-input-bootstrap.md)). A USB keyboard needs
   no step (§4.4). Offered only when `mura.hardware.input.bluetooth` or a controller class exists.
3. **Speak — language.** The *account's* language (per-user, as on every desktop; the system
   locale is not a setup item). Preselect the image's declared locale; a short list (≈10,
   ordered by speaker population or as the build declares) with "more…" opening the full list
   with type-to-filter — usable because *walk* came first. If skipped, the list is re-sorted
   from the network's country once Wi-Fi is up. Over the web path (§5) it comes from the phone's
   `Accept-Language` for free.
4. **Connect — Wi-Fi**, or skip. After language because the passphrase prompt needs words. The
   connection's scope follows polkit like GNOME's "available to all users" box (§4.3): a
   system connection where the caller may, otherwise the user's own. In-session the passthrough
   cameras may later scan a Wi-Fi QR from a phone (Quest 3 / Vive mechanism); never pre-login
   (§4.4).
5. **Secure — set a password**, or skip. **One credential**: the Unix password, and **which
   password is the wearer's choice** — the OS does not grade it. If the wearer chooses digits
   only, the greeter and lock show a digit pad (the non-secret `numeric-credential` hint,
   [multi-user.md §3](multi-user.md)); no second module, no second secret. `pam_faillock`
   guards every password equally (§5.3).
6. **Time zone** (and hostname, if offered) — one confirmation, **derived, never typed**: on the
   web app from the phone's own zone (the browser's `Intl` zone, as `Accept-Language` gives the
   language), in-headset from the joined network / geoclue where available, else the image's
   default. It therefore sits **after Connect** and has **no relation to the password step**,
   which may not exist. *How the in-headset confirm/override is authorised* — `timedate1.set-timezone`
   is `auth_admin_keep` even for an active session
   (`references/systemd/src/timedate/org.freedesktop.timedate1.policy:32-38`) — is the one open
   authority question of this flow; it is surveyed from shipping first-run flows in
   [research/54](../research/54-first-run-authority.md) (GNOME never shows a logged-in user a
   system step; SteamOS grants the seat user `set-timezone`/`set-hostname` permanently through
   `holo-polkit-helpers`), with the recommendation "derive + a Mura rule for exactly these two
   actions in active local sessions" — **decider: the project owner** (rev 2.6; the rev 2.5
   "after the password card" text is withdrawn as nothing shipping does that).
7. **Finish** — the closing card: "how to reach this device" (`ssh mura@<address>`, the
   USB-cable path, §5) and *finish setup*, which writes `state/setup/setup-complete` (§3) and
   ends the provisioning hotspot and web app. **Dismissing the surface counts as finishing**
   (ruled 2026-09-24): one marker, one meaning — closing the surface on first boot writes the
   marker too; the surface stays re-runnable from settings, the card is idempotent, and the
   hotspot/web app return only as an administrator setting or after a factory reset.

No consent ceremony (§4.1). Locale-list ordering heuristics beyond the above are the welcome
surface's UX design at G1 (decider named in §9).

### 4.3 Authority split

The surface is **unprivileged session UI running as the logged-in user** — the gnome-tour /
plasma-welcome shape — and it is the in-session instance of the one `mura-setup` program (§5.1).
Every card uses the standard mechanism an *active local session* already has (rev 2.5,
verified against the pinned clones):

| Card | Mechanism | Authority for a passwordless active user |
|---|---|---|
| IPD | the settings store, `enrollment/<user>/calibration/` | own files |
| Peripherals | BlueZ agent API | BlueZ's D-Bus policy admits any caller (`references/bluez/src/bluetooth.conf:26-28`, `context="default"`) |
| Language | AccountsService `SetLanguage` on the own account | `org.freedesktop.accounts.change-own-user-data` = `yes` (`references/accountsservice/data/org.freedesktop.accounts.policy.in:10-16`) |
| Wi-Fi | NetworkManager `AddConnection` | `settings.modify.own` = `yes` (`references/networkmanager/data/org.freedesktop.NetworkManager.policy.in:105-113`) → the user's own connection; `settings.modify.system` is `auth_admin_keep` even when active (`:115-123`; the `modify_system` build flag is refused upstream, `references/networkmanager/meson.build:557-560`) → a system connection only once a password exists. The card asks `CheckAuthorization` and picks the widest scope allowed — GNOME's "available to all users" behaviour, not a Mura rule. **Ruled 2026-09-24: user-scoped** (over a greeter-style rule or moving the card) |
| Password | **`passwd` itself, driven in a pty** (the pattern Cockpit's `passwd_self` uses) | NixOS's PAM `password` stack carries `nullok`, so no old password is asked ([research/42 §3.5](../research/42-input-bootstrap.md)) |
| Time zone, hostname | `timedated` / `hostnamed` | `auth_admin_keep` (`org.freedesktop.timedate1.policy:32-38`) — **open; decider: the project owner**, from [research/54 §4](../research/54-first-run-authority.md): derive automatically + a Mura rule granting active local sessions exactly `set-timezone` / `set-static-hostname` (SteamOS's `holo-set-timezone` shape, narrowed) is the recommendation |
| Finish | the `setup-complete` file | sticky `state/setup/` (§3) |

No polkit rule relaxes `change-own-password` (`auth_admin` by default,
`org.freedesktop.accounts.policy.in:20-26`): making it `allow_active=yes` would let any session
process set the wearer's password (lock-out, then escalate). The `numeric-credential` hint is written by the user into their own file
`state/credential-hint/<user>` — a **sticky, world-writable directory** (`1777`, the `/tmp`
shape; rev 2.4, D2): any user creates their own file, only its owner (or root) can replace or
remove it, and the greeter reads a hint only after checking the file's owner is the account it
is about to prompt. No mirror unit, no root involvement ([multi-user.md §3](multi-user.md)).
`mura-provisiond` (root,
private socket, the mura-authd shape) is **left with exactly one load-bearing job — the guest
token gate** ([multi-user.md §4](multi-user.md)) plus the polkit-gated account-admin
convenience path; it writes no credentials and has no conversation authorised by the absence of
state. The surface writes only runtime state — preferences into the settings stores with
provenance, network profiles into NetworkManager, calibration into `enrollment/<user>/`, the
hint and the finish marker into their sticky directories — never generated `/etc` files or
NixOS configuration.

### 4.4 Input requirement — decided (conformance, normative)

**The input floor is IMU head-aim plus the HMD's own buttons; dwell where a button is
unusable. Every pre-login scene (greeter, lock) and every welcome-surface item is fully
operable at the floor**, on every target, with nothing configured. Grounding
([research/42 §4, §7](../research/42-input-bootstrap.md)): every relevant Monado driver keeps a
3DoF IMU path; every target has power + volume and most a third button
(`mura.hardware.input.hmdButtons`, `selectRole`); PICO ships this as "Head Control Mode" and
the Steam Frame as its Aux button (mechanism precedents); measured cost ≈10 WPM.

- **Buttons reach the compositor through libinput** as ordinary key events. logind's power-key
  handling is `HandlePowerKey=ignore` or inhibited by the session (`handle-power-key`) — the
  Steam Deck's `powerbuttond` arrangement, and exactly what SteamOS on the Steam Frame does
  (`10-logind-no-powerbutton.conf`; the Aux→click semantic lives in the compositor-side
  consumer, not in udev) — never grabbed inside Monado (the Galaxy XR fork's `EVIOCGRAB` is
  recorded and not followed). Volume keys are never logind's.
- **A two-key vocabulary per target** (`mura.hardware.input.{selectRole,backRole}`;
  [research/42 §4.3a](../research/42-input-bootstrap.md)): *select* = the vendor's own
  head-cursor click where one is documented (Steam Frame Aux = `KEY_SELECT`; Meta/PICO volume
  keys, Vol+ by default — the Android Switch-Access convention), *back* = Vol− by default,
  *recenter* = a long press of select (the cross-vendor convention). On the Galaxy XR the only
  candidate is the Top button, which **is** the PMIC power key: select and power share
  `KEY_POWER` there and the compositor disambiguates short press (select) from long press
  (power menu) — allowed by the contract, tested. A dedicated select button emits `KEY_SELECT`
  (353) at the device-tree level; because xkeyboard-config maps only keycodes ≤255, the
  compositor consumes 353 raw for its own scenes and translates it to Return for ordinary
  clients (or ships a hwdb `KEYBOARD_KEY_…=enter` remap).
- **Constraint 7's stabiliser** ([zxr-shell-v2-composition.md §7.3](zxr-shell-v2-composition.md):
  deadzone, smoothing, dwell, magnetism, event-time compensation) has the greeter as its first
  consumer. Defaults: dwell 400–600 ms, targets ≥2.5–3° with ≥12 mm spacing — from the settings
  schema, never compiled in (constraint 9).
- **USB HID works at the greeter with zero configuration** (logind `TakeDevice` + libinput
  hotplug); the auth scene accepts hardware-keyboard focus.
- **On-screen keyboards are clients**: zxr implements layer-shell + `virtual-keyboard-v1` +
  `input-method-v2`, so the wearer may run any keyboard (wvkbd, squeekboard, Mura's own —
  implementation-time choice); the compositor's auth scene keeps its own floor-operable digit
  pad and minimal text entry so the greeter never depends on an external client.
- **Not part of the floor**: hands, eyes, optical controllers, passthrough (perception plane
  down pre-login); Monado's `qwerty` driver (SDL-bound); any QR shown *inside* the headset
  (lens distortion, absurd ergonomics — rejected outright).

## 5. Out-of-band access and the setup web app

**Decision (rev 2.5 restates it in the words of the mandate):** the headset is an ordinary
Linux PC. It is reachable from a device the wearer already holds — from first boot, on every
profile — through the two mechanisms a Linux PC has, and nothing more is invented:

- **SSH, on with upstream defaults, on every profile.** The postmarketOS pattern for the
  *transport*: the device presents a USB Ethernet gadget from the initramfs onward
  (`references/pmaports/main/postmarketos-initramfs/init_functions.sh:12-15, 836-963`), runs a
  tiny DHCP server for the plugged-in computer, and sshd is on
  (`references/pmbootstrap/pmb/install/_install.py:463-470`). OpenSSH's own configuration for
  the *policy*: password authentication on every interface — cable, hotspot, LAN alike — and
  `PermitEmptyPasswords no`. Nothing scopes sshd per link (the rev 2.2 key-only/`Match Address`
  posture is withdrawn — ADR 0017 alternatives): exposure to the public internet is the
  network's job, exactly as for a laptop, and `pam_faillock` guards every password. A
  passwordless `mura` therefore has SSH **after `passwd`** — run in the headset or on the setup
  web app below — or **from first boot with a key the builder declared**
  (`users.users.mura.openssh.authorizedKeys.keys`, standard NixOS; `profiles/dev.nix`). A
  profile may turn sshd off (`services.openssh.enable` is `mkDefault true` in `modules/os`).
- **The setup web app** — Mura's replacement for the consumer platforms' phone companion app,
  for a wearer who cannot yet use the headset display. It is the phone-facing instance of the
  one `mura-setup` program (§5.1), reached over the **USB gadget** or over a **headset-hosted
  Wi-Fi hotspot with a captive portal**, and **never over the LAN** (LAN access to the device is
  SSH — or Cockpit, if the wearer installs it; not ours). Its authorisation is possession — the
  cable, or the hotspot's PSK — the same trust as SSH over the cable: "you plugged it in / you
  read the code off the display". Both the hotspot and the web app exist **until setup is
  finished** (below); afterwards both are ordinary administrator settings, never automatic
  again.

**"Set up" means: the wearer said so.** Finishing or dismissing setup — in the welcome surface
(§4.2 item 7) or in the web app — writes `state/setup/setup-complete` (§3). That marker, and
nothing else, ends the provisioning surfaces. Nothing reads `shadow` or asks NetworkManager
whether "a network is configured" to decide; the rev 2.1 condition ("no network profile *and*
no password") is withdrawn because its second half was a runtime detection of the passwordless
state, which §5.3 forbids. Factory reset wipes the marker and the surfaces return.

**The provisioning hotspot** is condition-shaped on two observable facts: the marker is absent
**and** NetworkManager has no active connection other than the hotspot's own profile (an NM
fact; without it, on chips lacking concurrent AP+STA the AP would prevent joining a configured
network on every boot). The USB gadget link is under `systemd-networkd`, unmanaged by NM, and so
never counts (§5.4 — stated because moving it under NM would silently kill the hotspot). It is
**WPA2 with a per-boot random 8-digit PSK displayed inside the headset**; the wearer reads it and
types it on the phone. Radio range is not the trust class of a cable: an *open* hotspot would
have offered "set `mura`'s password" to anyone within Wi-Fi range for as long as setup stayed
unfinished (indefinitely, for an offline wearer). A generous **idle timeout with no client
associated** (a schema-declared default, proposed 10 min; it never counts down while a phone is
connected) is per-boot radio hygiene, not a state change: the hotspot returns at the next boot
while setup is unfinished. The captive-portal page is a launcher ("open `http://mura.local`");
the real UI is built for a normal browser tab (portal mini-browsers are hostile by design).

### 5.1 `mura-setup` — one program, one library, two instances (decided, rev 2.5)

The onboarding logic is written **once**. The requirement behind this section is *no duplicate
implementation of the same setup flow* for the headset and the phone; the solved-before shape
is gnome-initial-setup's, which runs one binary in two modes — pre-login as its own user
(`GIS_DRIVER_MODE_NEW_USER`) and after login as the real user (`GIS_DRIVER_MODE_EXISTING_USER`),
sharing every page (`references/gnome-initial-setup/gnome-initial-setup/gnome-initial-setup.c:218-249, 279`).

- **The library**: the card list and state machine of §4.2; the D-Bus calls to NetworkManager,
  `timedated`, `hostnamed`, AccountsService and BlueZ; the `setup-complete` marker. **Every card
  means the same thing in both instances** — "language" is always the autologin account's
  language (AccountsService `SetLanguage`), never the system locale; "Wi-Fi" is always "the
  widest connection scope polkit allows this caller" (§4.3). Which cards an instance may
  complete is the library asking polkit `CheckAuthorization` per card and greying or reordering
  — one rule evaluated in two contexts, not two implementations. Both instances converge on
  *system state* (NetworkManager, AccountsService, the settings store), so a card completed on
  the phone shows as completed in the headset and vice versa with no IPC between them — as
  gnome-initial-setup and gnome-tour coexist.
- **Instance 1 — the system service** (`mura-setup.service`, its own identity `mura-setup`).
  Privilege is the standard daemon-identity model (fwupd, colord): **scoped polkit rules grant
  the `mura-setup` identity exactly the setup actions** — `org.freedesktop.NetworkManager.settings.modify.system`,
  `org.freedesktop.timedate1.set-timezone`, `org.freedesktop.hostname1.set-static-hostname`,
  `org.freedesktop.accounts.user-administration` for the profile's autologin account *if any*
  (password and language), and BlueZ agent registration — the same shape as
  `references/gnome-initial-setup/data/20-gnome-initial-setup.rules.in:8-30`, narrowed from
  prefixes to the exact actions. The privileged work is done by the standard daemons; there is
  no bespoke root helper, no PAM login (authorisation is link possession), no faillock. It
  starts the **HTTP server** — the captive-portal launcher and the web app, one service on
  port 80 of the gadget and hotspot addresses only (`FreeBind=yes`; the hotspot address exists
  only while the AP is up) — the balena wifi-connect / comitup shape. It is
  `ConditionPathExists=!/var/lib/mura/state/setup/setup-complete`, runs whether or not anyone
  is logged in (so the greeter profile gets the phone flow pre-login too), and the hotspot's NM
  profile is `PartOf` it. The rev 2.1 tool for this instance was **Cockpit**; it is dropped as
  the tool (a permanent admin console with LAN-binding rules was never the mandate) and kept as
  a mechanism reference (the `passwd_self` pty flow, the NetworkManager Wi-Fi dialog shape).
- **Instance 2 — the session client**: the welcome surface of §4, running as the logged-in user
  with ordinary active-session authority (§4.3), *not* gated on the marker (it is re-runnable
  from settings). On the greeter profile it appears in the first session of whichever declared
  account logs in; the password card is absent when that account already has a hash.

**Rejected and recorded** (ADR 0017 alternatives): a *single* process serving HTTP and rendering
in-headset — a system identity cannot connect to the session user's Wayland socket
(`$XDG_RUNTIME_DIR`, mode 0700) without a bespoke compositor socket and a cross-user client;
that is ordinary Unix session isolation, not a zxr policy, and the two-instance shape reaches
the no-duplication goal without it. Running setup as `mura` with a polkit rule that grants the
active session the setup actions while the marker is absent — the dynamic "passwordless
window" mechanism §5.3 forbids, widened to every session process. A webview rendering the web
app in-headset. Serving the web app on the LAN once a password exists — that re-grows a PAM
login and per-user privilege, i.e. Cockpit's job. A native phone app is optional sugar over the
same SSH/HTTP surfaces — no bespoke daemon — and its app-store dependency is recorded as an
ethos cost. BLE GATT credential provisioning (Improv/Fast Pair class) is rejected for v1: a
bespoke unauthenticated privileged surface, and Web Bluetooth has no iOS Safari.

### 5.2 Portal mechanics (decided; handoff rewritten rev 2.5)

The hotspot is NetworkManager AP mode with `ipv4.method=shared` — NM runs dnsmasq itself; a
`dnsmasq-shared.d` fragment adds the wildcard `address=/#/<gateway>` (so `mura.local` resolves
on the hotspot regardless of phone mDNS support) and DHCP option 114 (RFC 8910) with the launcher
URL. The launcher answers the phone OSes' cleartext probes (`generate_204`,
`hotspot-detect.html`, `connecttest.txt`) with a redirect, so the OS opens its portal sheet; the
sheet shows one static page: "open `http://mura.local`" — nothing more survives Apple's CNA or
Android's portal WebView. Both OSes will ask "no internet — stay connected?"; the page and the
in-headset display say yes.

**The flow completes on the link it started on** — there is no "continue at `mura.local` on
your network" handoff (rev 2.1's; withdrawn: it needed the web app on the LAN, §5). On chips
with concurrent AP+STA (`mura.hardware.input.concurrentApSta = true`) the Wi-Fi card activates
and verifies the new connection immediately while the phone stays on the hotspot. On chips
without, Wi-Fi activation is part of *finish*, and **the marker is the last write**: finish →
AP down → STA up → on success (or if Wi-Fi was skipped) write `setup-complete`; if the STA fails
within the handoff window, the AP returns with the marker still absent and the page reports the
error when the phone reconnects. Mechanism references: balena wifi-connect (wildcard DNS +
`Host` redirect + 20 s handoff wait), comitup (DHCP option 160) —
[research/42 §6.2](../research/42-input-bootstrap.md).

### 5.3 The passwordless default user — the static posture (decided; rev 2.2 security review, corrected rev 2.5)

`mura` ships with no password. **Everything below is ordinary static NixOS configuration** —
there is no "while the account has no password" mechanism, no unit watching `shadow`, no
drop-ins toggled at runtime. Each line is harmless once a password exists, and each stays
correct if the wearer later removes it. **Where upstream already has a default, Mura keeps it**
(rev 2.5): the rev 2.2 review had scoped sshd per link and bound the web tool to trusted
addresses to protect a short numeric password from LAN guessing; that was hardening beyond
what any Linux distribution ships, nothing in the mandate asked for it, and the wearer's choice
of password is theirs (§1). Per-service posture (the table is the specification;
[multi-user.md §3](multi-user.md) carries the PAM/polkit summary):

| Surface | Posture | Why |
|---|---|---|
| Greeter / autologin (`greetd`) | `allowNullPassword` (NixOS's own default for greetd); faillock | a passwordless account logs in without a prompt — that is the default image |
| Lock (`mura-lock` via authd) | `allowNullPassword`; faillock | no credential ⇒ no lock engages (ADR 0007); a numeric one renders the digit pad |
| **`sudo`** | **standard — no `nullok`**, `wheelNeedsPassword` default | a passwordless `mura` is a full *user*; **administration requires a password**. `nullok` here would make `sudo -S <<< ""` from any session process, or any shell obtained as `mura`, into root |
| **polkit `auth_admin` actions** | **standard** — no Mura rule relaxes them for sessions | same reasoning; Mura's polkit rules are the greeter's NetworkManager rule ([multi-user.md §2](multi-user.md)) and the `mura-setup` identity's scoped set (§5.1) |
| Setting the first password | **`passwd`** (own account; the welcome surface drives it in a pty); the web app via AccountsService `user-administration` under `mura-setup`'s rule | NixOS's PAM `password` stack has `nullok`: no old password is asked. This is the admin gate. No polkit own-password rule (an escalation vector) |
| **sshd** | **upstream defaults** — on, `PasswordAuthentication yes` and `KbdInteractiveAuthentication yes` on every interface, `PermitEmptyPasswords no`; faillock on its PAM stack; no `Match` blocks (rev 2.5) | a regular Linux PC. The empty password is not a credential anywhere, so a passwordless account gets SSH after `passwd` or with a declared key. `PermitEmptyPasswords` is not merely left at its default, it is *unusable* here (D2 finding): with it, sshd's initial `none` method runs a real PAM authenticate with an empty password in the parent process, the actual authentication runs in a *forked* helper, and the parent's PAM handle keeps the failed probe as its cached chain — `pam_setcred` then fails **every** password login the moment the account *has* a password (measured in the D2 VM test) |
| **`mura-setup` (web app)** | no login; served on the gadget and hotspot addresses only; scoped polkit for its identity (§5.1) | authorisation is possession of the cable or the in-headset PSK — TTY-equivalent trust, the SSH-over-USB argument; never on the LAN, where the empty password would otherwise become "set `mura`'s password" for anyone on the network |
| Hotspot | WPA2, per-boot 8-digit PSK shown in-headset; exists until setup is finished; idle timeout (§5) | radio range is not cable possession |
| A declared `hashedPasswordFile` user | none of this applies | Path A |

Consequences worth stating: a person who never sets a password keeps a fully usable device and
simply cannot administer it — the welcome surface's "set a password" item says so in those
words; SSH from a laptop is a `passwd` away (in the session or on the web app over the cable),
and `sudo` one step further; a self-builder's declared key gives SSH from the first boot;
nothing in the system ever depends on detecting the passwordless state. The faillock ladder
(`mura.xr.session.faillock.{deny,unlockSeconds}`, defaults 5 / 300 s) is shared by the greeter,
the lock and SSH, with its tally in `state/faillock/` on `/persist`; nixpkgs' Linux-PAM does not
read `/etc/security/faillock.conf` unaided, so every `pam_faillock` line carries `conf=` and the
`faillock` CLI needs `--dir` (aliased). The default-password alternative (pmOS demo images, the
pre-2022 Raspberry Pi `pi:raspberry`) is rejected in ADR 0017: a well-known `wheel` credential
reachable over the network until changed.

### 5.4 USB gadget and hotspot specifics (decided; discretionary values flagged)

**USB gadget** (`modules/os/oob.nix`, from the initramfs — the pmOS pattern,
`references/pmaports/main/postmarketos-initramfs/init_functions.sh:836-963`):

| Item | Decision | Note |
|---|---|---|
| Gadget function | **NCM** (`usb_f_ncm`) as `ncm.usb0`; D3 ships NCM only. Correct IAD device marking (`ef/02/01`), Windows-10 `WINNCM`, and a separately selectable RNDIS composition are open outcomes of the host matrix, not adopted follow-ups | [mine] — NCM is standards-track and class-bound on Linux; macOS, Android-host and Windows-version behavior still needs the physical matrix in [research/55](../research/55-usb-identities-and-gadget-policy.md). pmOS's NCM→RNDIS fallback tests local function creation, not the host OS |
| Interface name | `usb0` (udev-stable) | matches pmOS; `mura-setup`'s listening address keys off the subnet, not the name |
| Subnet | `172.16.42.1/24` device side, DHCP pool `172.16.42.2–.20` | pmOS mechanism precedent (`references/pmbootstrap/pmb/config/__init__.py:322`); one-device operation is VM-proven, while simultaneous-headset route collision is an open decider in research/55 |
| DHCP server | **`systemd-networkd` `[DHCPServer]`** on `usb0` (`EmitDNS=no`, `EmitRouter=no` — the link is not a route to anywhere); the link is **unmanaged by NetworkManager** | the NixOS-native answer; pmOS's `unudhcpd` is the reference, not the tool. NM-unmanaged is load-bearing: the hotspot condition counts NM's active connections (§5) |
| Lifetime | the gadget stays configured after boot as an ordinary interface — it is the debugging path forever, not a first-boot special | consistent with "the device is a Linux host" |
| Vendor/product strings | `Mura` / `<device codename>` from `mura.device.*`; current development VID/PID `0x1d6b:0x0104` (Linux Foundation legacy multifunction-gadget example, **not** pmOS's default and not a Mura allocation); serial `mura-<codename>` in stage 1 | shipping identity strategy is explicitly open ([research/55](../research/55-usb-identities-and-gadget-policy.md)); per-unit serial, stable host/device MACs, descriptor composition/PIDs and simultaneous-device addressing are part of that decision |
| sshd | on, upstream defaults — nothing gadget-specific (§5.3) | — |

**Hotspot** (NetworkManager; exists until setup is finished and while no other connection is active, §5):

| Item | Decision | Note |
|---|---|---|
| Profile | NM connection `mura-setup` (`ensureProfiles`, PSK/SSID from `/run/mura/hotspot.env`): `802-11-wireless.mode=ap`, `band=bg` (2.4 GHz for phone compatibility), `ipv4.method=shared`, `autoconnect=false`; brought up and down by `mura-hotspot.service` (`PartOf=mura-setup.service`), a 5 s supervisor loop over two inputs NM cannot express — the marker and "no other active connection" | NM runs dnsmasq and NAT itself (`references/networkmanager/src/core/dnsmasq/nm-dnsmasq-manager.c:140-230`) |
| SSID | `Mura-<last 4 hex of machine-id>` | two headsets in a room stay distinguishable |
| Security | WPA2-PSK; **PSK = 8 random digits generated per boot**, shown inside the headset (dev profile: also on the serial console/journal until the compositor scene exists) | ruled (§5); digits only so it is typeable on any phone keyboard |
| Subnet | `10.42.0.1/24` (NM shared-mode default) | keep NM's default; distinct from the gadget subnet so the two listening addresses never coincide |
| DNS | `/etc/NetworkManager/dnsmasq-shared.d/mura-portal.conf`: `address=/#/10.42.0.1` (wildcard — every name resolves to the headset, so `mura.local` works without phone mDNS) and `dhcp-option=114,http://10.42.0.1/` (RFC 8910) | balena wifi-connect / comitup mechanism (research/42 §6.2) |
| Launcher + web app | one HTTP service — the `mura-setup` system instance — on `:80` of the hotspot and gadget addresses (`FreeBind=yes`); answers `/generate_204`, `/hotspot-detect.html`, `/connecttest.txt`, `/success.txt` with a `302` to `/`; `/` is the launcher ("open `http://mura.local`", plus the raw IP) and the web app lives behind it in a normal tab | the portal sheet is a launcher; nothing else survives Apple's CNA / Android's portal WebView |
| Idle timeout | `mura.oob.hotspot.idleTimeoutMinutes`, **default 10, with no station associated**; never counts down while a client is connected; per-boot (the hotspot returns next boot while setup is unfinished) | [mine] — generous by design; the user is indifferent above "not annoying" |
| Wi-Fi join / finish | concurrent AP+STA (`mura.hardware.input.concurrentApSta = true`): activate and verify immediately, phone stays on the hotspot. Otherwise part of *finish*: AP down → STA up → marker on success; STA failure within the handoff window → AP returns, marker absent, error shown on reconnect | §5.2; wifi-connect's 20 s handoff wait is the reference |
| LAN | **never** — `mura-setup` binds the two addresses above and nothing else | §5, §5.3 |

VM verification (implementation-path §3c D3, **landed** — `nix build .#vm-test-oob`): the NixOS
kernel ships `mac80211_hwsim`, `dummy_hcd`, `usb_f_ncm`/`usb_f_rndis` and configfs as modules,
so both halves run for real — `dummy_hcd` puts the gadget's *host* end in the same kernel (the
cdc_ncm side takes a lease from `usb0`'s DHCP server, logs in over SSH by key and loads the
launcher), two `hwsim` radios are the headset's AP and the "phone" STA (associates only with the
PSK, is captured by the wildcard DNS and the probe redirects). Three facts the VM taught, now in
`modules/os/oob.nix`: the NixOS firewall must open UDP 67 / TCP 80 on `usb0` (an interface-scoped
rule; sshd's 22 is open everywhere already) and TCP 80 to the hotspot address; NetworkManager's
shared mode opens its dnsmasq DHCP/DNS ports **only with its iptables firewall backend** (the
nftables backend writes NAT/forward rules alone and assumes firewalld) — the module pins
`firewall-backend=iptables`; and `qemu-vm.nix` force-disables `wpa_supplicant` on the assumption
that VMs have no radio (`mkVMOverride`), which the virtual headset overrides. Two VM-only
topology artefacts are confined to the test: the gadget's host end (`usb1`) and the phone radio
are NM-unmanaged so they do not count as the headset's own connections.

## 6. Factory calibration vs user calibration (two stages, normatively distinct)

- **Factory calibration** (`factory/`): panel/optics/camera intrinsics. A **B1a precondition** —
  Monado does not start without it, and the greeter and welcome surface render *with* it plus
  the default IPD. It predates first boot (manufacture/bring-up flashing) and survives everything
  short of re-manufacture.
- **User calibration** (`enrollment/<user>/calibration/`): IPD preference, floor height,
  boundary. Offered by the welcome surface (§4) or by the feature that first needs it (a
  boundary when passthrough is requested), refined any time later from settings; wiped by factory
  reset.

The two never share a store, a lifecycle, or a validity check. B1b's preflight validates
*factory* calibration; a missing *user* calibration simply routes to the welcome surface or the
in-session calibration UI.

## 7. Factory reset

The inverse of provisioning, per the §2 class table: wipe `enrollment/`, wipe `pairing/`
(link keys), wipe `/home`, **wipe `etc-rw/`** (the `/etc` overlay's upper layer — which is
what removes runtime-created accounts and network profiles and **rotates machine-id** in one
stroke), reset `state/` per policy, preserve `factory/` and `identity/`. Next boot: declared
accounts re-materialise from the image (userborn hybrid mode on every profile), runtime-created
accounts are gone, and each account's first session meets the welcome surface again. Reset is a
recovery-environment operation (not an in-session `rm`).

**Reset is the device-transfer / clean-slate path. It is never the credential-recovery path.**
A forgotten password or PIN is fixed the way it is fixed on every Linux machine: another `wheel`
user runs `sudo passwd <user>`; or root over a TTY, SSH (§5), or the recovery environment's
shell. Doc 12 §6's Quest posture — "after the faillock ladder is exhausted the only way forward
is reset; on an appliance the final fallback is recovery/wipe, not a root shell" — is **overruled
by overview invariant 10**: this machine has root, and it belongs to its wearer.

## 8. Conformance checks

1. A/B update across an existing installation: F1 does not re-run (per-task markers seen
   through slot replacement); machine-id unchanged; `ConditionFirstBoot`-gated slot-local units
   do re-run.
2. Power cut mid-F1: next boot re-runs exactly the incomplete units; no half-written state.
3. The default image boots to a usable `mura` session with zero interaction; the welcome
   surface is present and dismissible; the session behind it is live.
4. A fully declared image (user with `hashedPasswordFile`, locale, network profiles, IPD)
   boots to the greeter or session with **no** welcome surface.
5. Build fails for a greeter profile with zero declared human accounts unless
   `allowNoDeclaredAccount` is set; a greeter facing zero pickable accounts at runtime still
   renders free-text entry and the power menu.
6. The welcome-surface process holds no capability to write another user's `enrollment/` or any
   credential directly (fs permissions + no privileged sockets beyond provisiond's guest gate,
   NetworkManager, BlueZ, and polkit-gated D-Bus).
7. Factory reset: `factory/` and `identity/` byte-identical before/after; `pairing/` empty
   (no link key survives); machine-id rotated; declared accounts log in on the next boot; the
   welcome surface reappears.
8. Out-of-band: sshd answers on every interface from the first boot with upstream defaults
   (no `Match` blocks); an image with a declared key logs in over the USB gadget before any
   password exists. The provisioning hotspot is up while `setup-complete` is absent and no
   other NM connection is active, and is down within one state change of either fact
   flipping; it never comes up automatically once setup is finished; a phone joining it is
   shown the launcher page and reaches the setup web app at `http://mura.local` in a normal
   tab. The web app answers on the gadget and hotspot addresses and on **no** LAN address,
   before or after a password exists.
9. Multi-user G2 fixture: pre-seeded declared accounts authenticate with no first-run UI ever
   having run.
10. **Input floor (§4.4):** with no controller paired, no peripheral attached, and cameras off,
    the greeter, the lock, and every welcome item complete using only head-aim and
    `hmdButtons.<selectRole>` (and, with the select button masked, dwell alone); the power key
    reaches the compositor and does not power the device off while a scene owns it.
11. A USB keyboard plugged in during the greeter types into the auth scene with no
    configuration; a just-works Bluetooth keyboard pairs from the greeter's agent.
12. Passwordless `mura`: greeter login succeeds; **`sudo` fails** and polkit `auth_admin`
    actions fail; the empty password is **refused over SSH** (`PermitEmptyPasswords no`) while
    a declared key logs in; `passwd` succeeds without an old password; after a password is
    set, `sudo` works and SSH password auth works on every interface; the faillock ladder locks
    after `faillock.deny` failures with its tally under `/persist/mura/state/faillock/`, and
    the right password is refused while locked. A digits-only password yields the digit pad at
    greeter and lock; a mixed password yields the keyboard path; the greeter reads a hint from
    `state/credential-hint/<user>` only when that file is owned by `<user>`. (VM tests
    `vm-test-default-image` / `vm-test-multi-user`.)
13. A Wi-Fi network joined at the greeter, or through the `mura-setup` web app, is a system
    connection visible to the user who then logs in ([multi-user.md §2](multi-user.md)); one
    joined in the welcome surface by a passwordless user is that user's own connection, and a
    system connection once the caller can satisfy `settings.modify.system`. The welcome
    surface's time-zone card prompts through the polkit agent and succeeds only once a password
    exists.
14. `mura-setup.service` and the hotspot stop within one state change of `setup-complete`
    appearing and return after a factory reset; the hotspot refuses association without the
    in-headset PSK, yields to any other active NM connection, and goes down after the idle
    timeout with no client associated (returning at the next boot while setup is unfinished).
    The web app never binds a LAN address.
15. No process in the session can change the account password except through `passwd`'s own
    PAM conversation (no D-Bus path sets a password without `auth_admin`).

## 9. Open items

Each names its decider: **locale short-list ordering and the visual design of the IPD alignment
target** (decider: the welcome-surface UX design at G1 implementation); **a Monado 3DoF HMD
driver per target** — none exists upstream for IIO or the Qualcomm SSC; the input floor is
universal in principle and new driver code per target in practice (owner: each device's
bring-up workstream; ordering in implementation-path §5.1); **how the in-headset time-zone /
hostname confirm is authorised** — [research/54 §4](../research/54-first-run-authority.md)
recommends derive + a Mura rule for exactly `timedate1.set-timezone` and
`hostname1.set-static-hostname` in active local sessions (decider: the project owner; ruled
2026-09-24 and no longer open: dismiss = finish, Wi-Fi user-scoped);
recovery-environment design (where factory reset executes — owner: each family's recovery
story; the Frame workstream shapes the first one); account-layering (store accounts, cloud
identity) — a non-goal, explicitly out of OS scope. Resolved by the research/42 review and no
longer open: welcome contents (§4.2), input requirement (§4.4), web tool and portal (§5.1–5.2),
passwordless wiring (§5.3), paired-peripheral class (§2, wiped), boundary placement (spatial
mapping, not the welcome surface).
