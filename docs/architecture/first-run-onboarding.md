# First run: the image is the installation, silent provisioning, the welcome surface, and factory reset

**Status:** accepted design (2026-09-23; **rev 2, 2026-09-24 — the image-is-the-installation
reframe**). Decision record: [ADR 0017](adr/0017-first-run-provisioning.md) (amended in place).
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
   one, the lock engages only once a credential exists (ADR 0007), and `sudo`/`passwd`/SSH
   behave as they do on any Linux box with a passwordless account — the exact wiring for that
   default is an open question with a decider (§9). The Steam Deck's fixed `deck` account is
   the mechanism precedent. There is no "appliance wizard": what used to be called the
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
polkit-gated convenience path executing the same operations. Secrets (PIN hashes, Wi-Fi
credentials, device keys) are **never Nix option values** — the store is world-readable;
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
| `enrollment/<user>/` | optional PIN hash (`secret/`, root), user calibration (`calibration/`, §6) | survives | **wiped** |
| `state/` | F1 per-task markers (`state/provisioning/<task>`), update/migration bookkeeping, quarantine records | survives | reset per settings-schema policy |
| `/persist/userdb/` (multi-user profile; **its own class**, beside `mura/` — dir 0755, passwd/group 0644, shadow 0000; see [multi-user.md §1.1/§6](multi-user.md)) | userdb | survives | human rows removed; **declared accounts re-materialise at next boot** (userborn from the image), runtime-created accounts are gone |
| machine-id | `/etc/machine-id`, persisted here and committed **before D-Bus/logind start** | **survives** (one identity per unit, not per slot) | **rotated** — privacy; machine identity is not hardware identity |

Per-user preferences and remembered state stay in `$XDG_CONFIG_HOME` / `$XDG_STATE_HOME` on
`/home` (settings-schema §2); factory reset wipes `/home` wholesale. Whether paired-peripheral
state (`/var/lib/bluetooth`) is a device-level class that survives reset is an open question
(§9).

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

### 4.2 Contents — open question, decider named

*What the surface must be able to offer is not decided here.* It depends on what a headset can
accept as input with nothing configured, and how text is entered on it — the subject of the
input-bootstrap study ([research/42](../research/42-input-bootstrap.md), in progress) and the
greeter-furniture addendum to [research/11](../research/11-display-managers-greeters.md).
**Decider:** the findings review of those two documents. Candidate items recorded so the study
knows what to test, none adopted: establishing/upgrading the input method (pair a controller or
Bluetooth peripheral), user calibration (IPD, floor, boundary — §6), Wi-Fi, "set a password"
(and optionally a PIN — [multi-user.md §3](multi-user.md)), locale.

### 4.3 Authority split

The surface is **unprivileged session UI**. Privileged writes go through the standard
mechanisms the rest of the desktop uses: polkit-gated actions for account operations (own
password change, AccountsService-class), NetworkManager's D-Bus policy for network profiles,
`localed`/`timedated` for locale and time. `mura-provisiond` (root, private socket, the
mura-authd shape) remains for the XR-specific writes only — a PIN hash into
`enrollment/<user>/secret/` and the guest-token gate ([multi-user.md §4](multi-user.md)); it has
**no** conversation that is authorised by the absence of state. The surface writes only runtime
state — preferences into the settings stores with provenance, network profiles into
NetworkManager, enrollment into `enrollment/` — never generated `/etc` files or NixOS
configuration.

### 4.4 Input requirement — open question, decider named

Every pre-login scene (greeter, lock) and the welcome surface must be operable with whatever
input the device has *before anything is configured*. What that floor is on each target, and
what it costs to enter text with it, is what [research/42](../research/42-input-bootstrap.md)
establishes; the conformance requirement is written when its review rules.

## 5. Out-of-band provisioning

**Decision:** the headset is an ordinary Linux host, reachable from a device the wearer already
holds — from first boot, on every profile. Two surfaces, one trust class:

- **SSH over a USB Ethernet gadget.** The postmarketOS pattern: the device presents a USB
  network interface from the initramfs onward
  (`references/pmaports/main/postmarketos-initramfs/init_functions.sh:12-15, 836-963`), runs a
  tiny DHCP server for the plugged-in computer, and sshd is on
  (`references/pmbootstrap/pmb/install/_install.py:463-470`). Plug in a cable, `ssh
  mura@<device>`, and `nmcli`, `passwd`, `useradd` are all available before the display has
  shown anything. Physical possession of the cable is the authorisation — the same trust as
  sitting at a TTY, which on this OS is already root-equivalent (invariant 10).
- **A local web UI**, served over the same USB link, over a headset-hosted Wi-Fi hotspot with a
  captive portal, and over the LAN once the device is on one. **The provisioning hotspot is
  open and exists only while the device is unprovisioned** — condition-shaped: it comes up
  automatically only while no network profile is configured *and* no password is set; it goes
  down when either changes; afterwards it is an ordinary administrator-controlled setting, never
  automatic again. Same trust class as the USB link. The captive-portal page is a launcher
  ("open `http://mura.local`"); the real UI is built for a normal browser tab (portal
  mini-browsers are hostile by design).

Open questions, each with its decider (§9): the web tool (Cockpit — `services.cockpit` exists in
NixOS — versus a purpose-built provisioning page, scored in research/42 §6), portal mechanics
(probe handling, `.local` resolution on phones, RFC 8910/8908), and the passwordless-default-user
login interaction, which is **identical for SSH and web** (`PermitEmptyPasswords`/`Match
Address` scoping, PAM `nullok`, own-password change via polkit). A native phone app is optional
sugar over the same SSH/HTTP surfaces — no bespoke daemon — and its app-store dependency is
recorded as an ethos cost.

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

The inverse of provisioning, per the §2 class table: wipe `enrollment/`, wipe `/home`, remove
human rows from the persisted userdb, reset `state/` per policy, **rotate machine-id**, preserve
`factory/` and `identity/`. Next boot: declared accounts re-materialise from the image
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
6. The welcome-surface process holds no capability to write `enrollment/` directly (fs
   permissions + no privileged sockets beyond provisiond/NetworkManager/polkit-gated D-Bus).
7. Factory reset: `factory/` and `identity/` byte-identical before/after; machine-id rotated;
   declared accounts log in on the next boot; the welcome surface reappears.
8. Out-of-band: SSH over the USB gadget reachable from the first boot; the provisioning hotspot
   is up only while unprovisioned and is down within one state change of either condition
   flipping; it never comes up automatically on a provisioned device.
9. Multi-user G2 fixture: pre-seeded declared accounts authenticate with no first-run UI ever
   having run.

## 9. Open items

Each names its decider: **welcome-surface contents** (decider: the findings review of
research/42 + the research/11 addendum); **the pre-login/welcome input requirement** (same
decider); **the web provisioning tool** — Cockpit vs purpose-built, scored in research/42 §6
(same decider); **portal mechanics** (same decider); **the passwordless default user's login
wiring** across greeter/SSH/web/`sudo`/`passwd` (same decider — candidates: PAM `nullok` scoped
to local/USB, `PermitEmptyPasswords` under `Match Address`, own-password via polkit-gated
AccountsService-class action, a welcome-surface nudge to set one); **paired-peripheral state
class** — does `/var/lib/bluetooth` survive factory reset (same decider); recovery-environment
design (where factory reset executes — owner: each family's recovery story; the Frame workstream
shapes the first one); account-layering (store accounts, cloud identity) — a non-goal,
explicitly out of OS scope; whether boundary drawing moves entirely to first passthrough use on
controller-less devices (decider: the welcome-surface UX design at G1 implementation).
