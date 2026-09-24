# First run: the image is the installation, silent provisioning, the welcome surface, and factory reset

**Status:** accepted design (2026-09-23; **rev 2, 2026-09-24 — the image-is-the-installation
reframe**; **rev 2.1 same day — the [research/42](../research/42-input-bootstrap.md) review
ruled: input floor, welcome-surface contents, one credential, out-of-band mechanisms**;
**rev 2.2 same day — security review of the passwordless posture: admin requires a password,
`passwd` is the gate, SSH key-only off the USB subnet, Cockpit on trusted links, PSK hotspot,
hint mirror; all static**). Decision record: [ADR 0017](adr/0017-first-run-provisioning.md)
(amended in place).
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
tenant. Out-of-band provisioning (§5) adds sshd and, only while unprovisioned, a hotspot + a
small HTTP server — off the frame path, condition-shaped.

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
   you are in your session. A password is the wearer's choice: the welcome surface (§4) offers
   one, the lock engages only once a credential exists (ADR 0007), and **there is exactly one
   credential** — the Unix password; a short numeric one is a PIN (§4.2, [multi-user.md §3](multi-user.md)).
   A passwordless `mura` is a full *user*; **administration (`sudo`, polkit `auth_admin`
   actions) requires setting a password first** — `passwd` asks no old password for a
   passwordless account, and that is the gate (§5.3; the Steam Deck's `deck` account has exactly
   this posture, and so does NixOS itself). The Steam Deck's fixed `deck` account is the
   mechanism precedent. There is no "appliance wizard": what used to
   be called the appliance path is simply a declared image with `autoLogin` set.
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
| `/persist/userdb/` (multi-user profile; **its own class**, beside `mura/` — dir 0755, passwd/group 0644, shadow 0000; see [multi-user.md §1.1/§6](multi-user.md)) | userdb | survives | human rows removed; **declared accounts re-materialise at next boot** (userborn from the image), runtime-created accounts are gone |
| machine-id | `/etc/machine-id`, persisted here and committed **before D-Bus/logind start** | **survives** (one identity per unit, not per slot) | **rotated** — privacy; machine identity is not hardware identity |

Per-user preferences and remembered state stay in `$XDG_CONFIG_HOME` / `$XDG_STATE_HOME` on
`/home` (settings-schema §2); factory reset wipes `/home` wholesale.

## 3. F1 — silent machine provisioning

One-shot systemd units, no UI, no XR. Work: data-partition growth where the device needs it,
per-unit key generation into `identity/`, the `/persist/mura` skeleton (the setup service's
job), settings-store seeding (empty stores + the generation tag), nix-db rehydration where the
family requires it.

**Durable per-task markers are authoritative, `ConditionFirstBoot` is not.** Installing a fresh
root slot via an A/B update presents an empty `/etc/machine-id` and looks like first boot to
`ConditionFirstBoot`; provisioning must not re-run there. Rules:

- Each F1 unit gates on **absence of its own marker** under `state/provisioning/<task>` — on
  `/persist`, so it sees through slot replacement. No single "provisioned" bit exists, and no
  marker gates any UI.
- `ConditionFirstBoot` is used only for genuinely *slot-local* concerns (nix-db rehydration
  class — work that must re-run per new rootfs).
- machine-id: bound from `/persist` before D-Bus/logind start (early-boot, the standard
  image-based pattern), so identity is stable across updates and rotates only on factory reset.

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
3. **Speak — locale.** Preselect the image's declared locale; a short list (≈10, ordered by
   speaker population or as the build declares) with "more…" opening the full list with
   type-to-filter — usable because *walk* came first. If skipped, the list is re-sorted from
   the network's country once Wi-Fi is up. Over the web path (§5) locale comes from the phone's
   `Accept-Language` for free.
4. **Time zone** — one confirmation; derived from locale, refined from the network when
   available.
5. **Connect — Wi-Fi**, or skip. After locale because the passphrase prompt needs words. In-session
   the passthrough cameras may later scan a Wi-Fi QR from a phone (Quest 3 / Vive mechanism);
   never pre-login (§4.4).
6. **Secure — set a password**, or skip. **One credential**: the Unix password. If the wearer
   chooses digits only, the greeter and lock show a digit pad (the non-secret
   `numeric-credential` hint, [multi-user.md §3](multi-user.md)); no second module, no second
   secret. `pam_faillock` and the §5.3 SSH scoping carry a short numeric password's weakness.
7. **How to reach this device** — a closing card: `ssh mura@<address>`, `http://mura.local`,
   the USB-cable path (§5). Shown once, always in settings.

No consent ceremony (§4.1). Locale-list ordering heuristics beyond the above are the welcome
surface's UX design at G1 (decider named in §9).

### 4.3 Authority split

The surface is **unprivileged session UI**. Privileged writes go through the standard
mechanisms the rest of the desktop uses: NetworkManager's D-Bus policy for network profiles,
`localed`/`timedated` for locale and time, BlueZ's agent API for pairing, and — for item 6 —
**`passwd` itself, driven in a pty** (Cockpit's `passwd_self` mechanism): NixOS's PAM `password`
stack carries `nullok`, so a passwordless account is asked for no old password
([research/42 §3.5](../research/42-input-bootstrap.md)). No polkit rule relaxes
`change-own-password`: making it `allow_active=yes` would let any session process set the
wearer's password (lock-out, then escalate), which is why AccountsService defaults it to
`auth_admin`. The `numeric-credential` hint is written by the user into their own
`enrollment/<user>/`; a root unit publishes the greeter-readable mirror
`/run/mura/credential-hint/<user>` (`0640 root:greeter`, [multi-user.md §3](multi-user.md)).
`mura-provisiond` (root,
private socket, the mura-authd shape) is **left with exactly one load-bearing job — the guest
token gate** ([multi-user.md §4](multi-user.md)) plus the polkit-gated account-admin
convenience path; it writes no credentials and has no conversation authorised by the absence of
state. The surface writes only runtime state — preferences into the settings stores with
provenance, network profiles into NetworkManager, calibration and the numeric hint into
`enrollment/<user>/` — never generated `/etc` files or NixOS configuration.

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

## 5. Out-of-band provisioning

**Decision:** the headset is an ordinary Linux host, reachable from a device the wearer already
holds — from first boot, on every profile. Two surfaces, one trust class:

- **SSH over a USB Ethernet gadget.** The postmarketOS pattern: the device presents a USB
  network interface from the initramfs onward
  (`references/pmaports/main/postmarketos-initramfs/init_functions.sh:12-15, 836-963`), runs a
  tiny DHCP server for the plugged-in computer, and sshd is on
  (`references/pmbootstrap/pmb/install/_install.py:463-470`). Plug in a cable, `ssh
  mura@<device>`, and `nmcli`, `passwd` (then `useradd`, `sudo`) are all available before the
  display has shown anything. Physical possession of the cable is the authorisation — the same
  trust as sitting at a TTY; it yields the *user* `mura`, and administration follows a `passwd`
  (§5.3).
- **A local web UI**, served over the same USB link, over a headset-hosted Wi-Fi hotspot with a
  captive portal, and over the LAN once the device is on one. **The provisioning hotspot exists
  only while the device is unprovisioned** — condition-shaped: it comes up automatically only
  while no network profile is configured *and* no password is set; it goes down when either
  changes, or after a generous idle timeout with **no client associated** (a schema-declared
  default, proposed 10 min; it never counts down while a phone is connected); afterwards it is
  an ordinary administrator-controlled setting, never automatic again. **It is WPA2 with a
  per-boot random 8-digit PSK displayed inside the headset** — the wearer reads it and types it
  on the phone. Radio range is not the trust class of a cable: an *open* hotspot would have
  handed a user shell as `mura` to anyone within Wi-Fi range for as long as the device stayed
  unprovisioned (indefinitely, for an offline wearer). The captive-portal page is a launcher
  ("open `http://mura.local`"); the real UI is built for a normal browser tab (portal
  mini-browsers are hostile by design).

### 5.1 The web surface — Cockpit (decided)

The configuration web app **is Cockpit** (`services.cockpit`, a first-class NixOS module): it
joins Wi-Fi (its NetworkManager page scans and creates `wpa-psk` connections), sets the user's
own password through `passwd`, sets hostname and time zone, and authenticates through PAM —
scored against the alternatives in [research/42 §6.3](../research/42-input-bootstrap.md)
(RaspAP, LuCI, balena wifi-connect, comitup rejected as the tool: hostapd-bound, OpenWrt-bound,
or unpackaged and provisioning-only; the latter two remain mechanism references for the portal).
Two additions, both Cockpit-shaped: a **"Mura setup" Cockpit plugin** page (`services.cockpit.plugins`)
mirroring §4.2's items for the phone — the guided flow Cockpit lacks, plus the locale page it
does not have; and the **static captive-portal launcher** in front of it. A bespoke web app is
written only if Cockpit's Wi-Fi dialog fails on real hardware (condition-shaped). **Cockpit's
socket is bound to the USB-gadget and hotspot addresses only**; exposing it on the LAN is an
explicit administrator setting (with `mura.local` advertised over mDNS and PAM accepting an
empty password, an unrestricted Cockpit would be a user shell for the whole LAN — §5.3).

### 5.2 Portal mechanics (decided)

The hotspot is NetworkManager AP mode with `ipv4.method=shared` — NM runs dnsmasq itself; a
`dnsmasq-shared.d` fragment adds the wildcard `address=/#/<gateway>` (so `mura.local` resolves
on the hotspot regardless of phone mDNS support) and DHCP option 114 (RFC 8910) with the launcher
URL. The launcher answers the phone OSes' cleartext probes (`generate_204`,
`hotspot-detect.html`, `connecttest.txt`) with a redirect, so the OS opens its portal sheet; the
sheet shows one static page: "open `http://mura.local`" — nothing more survives Apple's CNA or
Android's portal WebView. Both OSes will ask "no internet — stay connected?"; the page and the
in-headset display say yes. AP→STA handoff: the page announces "now at `mura.local` on your
network" *before* the hotspot drops; on chips without concurrent AP+STA
(`mura.hardware.input.concurrentApSta`) the hotspot goes down, the phone rejoins its own network,
and the same URL works over the LAN. Mechanism references: balena wifi-connect (wildcard DNS +
`Host` redirect + 20 s handoff wait), comitup (DHCP option 160) — [research/42 §6.2](../research/42-input-bootstrap.md).

### 5.3 The passwordless default user — the static posture (decided; security review 2026-09-24)

`mura` ships with no password. **Everything below is ordinary static NixOS configuration** —
there is no "while the account has no password" mechanism, no unit watching `shadow`, no
drop-ins toggled at runtime. Each line is harmless once a password exists, and each stays
correct if the wearer later removes it. Per-service posture (the table is the specification;
[multi-user.md §3](multi-user.md) carries the PAM/polkit summary):

| Surface | Posture | Why |
|---|---|---|
| Greeter / autologin (`greetd`) | `allowNullPassword` (NixOS's own default for greetd); faillock | a passwordless account logs in without a prompt — that is the default image |
| Lock (`mura-lock` via authd) | `allowNullPassword`; faillock | no credential ⇒ no lock engages (ADR 0007); a numeric one renders the digit pad |
| **`sudo`** | **standard — no `nullok`**, `wheelNeedsPassword` default | a passwordless `mura` is a full *user*; **administration requires a password**. `nullok` here would make `sudo -S <<< ""` from any session process, or any shell obtained as `mura`, into root |
| **polkit `auth_admin` actions** | **standard** — no Mura rule relaxes them | same reasoning; the only Mura polkit rule is the greeter's NetworkManager rule ([multi-user.md §2](multi-user.md)) |
| Setting the first password | **`passwd`** (own account; the welcome item and Cockpit drive it in a pty) | NixOS's PAM `password` stack has `nullok`: no old password is asked. This is the admin gate. No polkit own-password rule (an escalation vector) |
| **sshd** | **global `PasswordAuthentication no`** (key-only) **+** `Match Address <usb-gadget-subnet>` → `PasswordAuthentication yes`, `PermitEmptyPasswords yes`; `nullok` on the sshd PAM stack | "you plugged the cable in" is TTY-equivalent trust and yields the *user*; over the LAN and the hotspot, password auth is refused — which also protects a short numeric password from remote guessing once one exists |
| **Cockpit** | PAM `cockpit` with `nullok`; **socket bound to the gadget and hotspot addresses only**; LAN exposure an administrator setting | reachable only on physically- or PSK-authorised links; its `passwd` flow is the first thing the setup page offers; admin operations inside Cockpit use sudo and therefore also wait for a password |
| Hotspot | WPA2, per-boot 8-digit PSK shown in-headset; exists only while unprovisioned; idle timeout (§5) | radio range is not cable possession |
| A declared `hashedPasswordFile` user | none of this applies | Path A |

Consequences worth stating: a person who never sets a password keeps a fully usable device and
simply cannot administer it — the welcome surface's "set a password" item says so in those
words; SSH from a laptop is a `passwd` away from `sudo`; nothing in the system ever depends on
detecting the passwordless state.

A native phone app is optional sugar over the same SSH/HTTP surfaces — no bespoke daemon —
and its app-store dependency is recorded as an ethos cost. BLE GATT credential provisioning
(Improv/Fast Pair class) is rejected for v1: a bespoke unauthenticated privileged surface, and
Web Bluetooth has no iOS Safari.

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
(link keys), wipe `/home`, remove human rows from the persisted userdb, reset `state/` per
policy, **rotate machine-id**, preserve `factory/` and `identity/`. Next boot: declared accounts re-materialise from the image
(userborn on the multi-user profile, the declared user on the appliance profile), runtime-created
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
8. Out-of-band: SSH over the USB gadget reachable from the first boot; the provisioning hotspot
   is up only while unprovisioned and is down within one state change of either condition
   flipping; it never comes up automatically on a provisioned device; a phone joining it is
   shown the launcher page and reaches Cockpit at `http://mura.local` in a normal tab.
9. Multi-user G2 fixture: pre-seeded declared accounts authenticate with no first-run UI ever
   having run.
10. **Input floor (§4.4):** with no controller paired, no peripheral attached, and cameras off,
    the greeter, the lock, and every welcome item complete using only head-aim and
    `hmdButtons.<selectRole>` (and, with the select button masked, dwell alone); the power key
    reaches the compositor and does not power the device off while a scene owns it.
11. A USB keyboard plugged in during the greeter types into the auth scene with no
    configuration; a just-works Bluetooth keyboard pairs from the greeter's agent.
12. Passwordless `mura`: greeter login succeeds; SSH with an empty password succeeds from the
    USB subnet and is **refused** from every other interface; **`sudo` fails** and polkit
    `auth_admin` actions fail; `passwd` succeeds without an old password; after a password is
    set, `sudo` works and SSH password auth is still refused off the USB subnet (key-only). A
    digits-only password yields the digit pad at greeter and lock; a mixed password yields the
    keyboard path; the greeter reads the hint only through `/run/mura/credential-hint/`.
13. A Wi-Fi network joined at the greeter is a system connection visible to the user who then
    logs in ([multi-user.md §2](multi-user.md)).
14. Cockpit answers only on the gadget and hotspot addresses until the administrator enables it
    on the LAN; the hotspot refuses association without the in-headset PSK, comes up only while
    unprovisioned, and goes down after the idle timeout with no client associated.
15. No process in the session can change the account password except through `passwd`'s own
    PAM conversation (no D-Bus path sets a password without `auth_admin`).

## 9. Open items

Each names its decider: **locale short-list ordering and the visual design of the IPD alignment
target** (decider: the welcome-surface UX design at G1 implementation); **a Monado 3DoF HMD
driver per target** — none exists upstream for IIO or the Qualcomm SSC; the input floor is
universal in principle and new driver code per target in practice (owner: each device's
bring-up workstream; ordering in implementation-path §5.1); **whether Cockpit's Wi-Fi dialog
holds up on real hardware** (condition-shaped fallback in §5.1; decider: F3 implementation);
recovery-environment design (where factory reset executes — owner: each family's recovery
story; the Frame workstream shapes the first one); account-layering (store accounts, cloud
identity) — a non-goal, explicitly out of OS scope. Resolved by the research/42 review and no
longer open: welcome contents (§4.2), input requirement (§4.4), web tool and portal (§5.1–5.2),
passwordless wiring (§5.3), paired-peripheral class (§2, wiped), boundary placement (spatial
mapping, not the welcome surface).
