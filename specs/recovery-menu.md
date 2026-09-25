# Recovery menu: one program, three ways in

**Status:** rev 2 (2026-09-25) — normative for `pkgs/mura-recovery`, `mura-setup --recovery`,
the stage-1 half of `modules/os/recovery.nix`, and the dedicated-family recovery-image boundary.
Revised when Android-derived partition selection lands and when the button proof runs on
hardware.
**Design source:** [research/56 §3](../docs/research/56-defaults-from-comparables.md) (the ruling),
[research/57](../docs/research/57-recovery-environments-and-boot-failure-feedback.md) (the
comparables and their reasons), [implementation-path.md §4 "Mura recovery environment"](../docs/architecture/implementation-path.md),
ADR 0017 decision 10 (one program, several instances — the `mura-setup` shape).
**Grounding:** "stage 1" is the systemd initrd (`boot.initrd.systemd`); "the panels" are the HMD's
displays driven by plymouth on the DRM device; key names are Linux evdev `KEY_*` codes as the
device contract declares them (`mura.hardware.input.hmdButtons`).
**Budget impact** (overview invariant 9): stage 1 only. One process asleep in `poll(2)` on the
input devices; a plymouth message per redraw; nothing on the frame path, nothing in a normal boot.

## 1. The problem

This program runs from the **dedicated Mura recovery boot partition**: its own kernel+systemd
initrd copy, booted to `mura-recovery.target`, with no recovery root filesystem. It is not a mode
that depends on the normal Mura boot files, and it never replaces or consumes stock/vendor
recovery. On uefi-rauc the partition is XBOOTLDR (`mura_recovery`); Android-derived targets need
an additional bootable Mura partition proven during device bring-up.

The recovery environment ([recovery.nix](../modules/os/recovery.nix)) had one way in: an ssh key
declared by an administrator. A wearer without one — the appliance profile by default — saw a
panel telling them to ssh and had nothing to do it with. Every shipping recovery gives the person
holding the device a keyless path: Android/Lineage recovery's on-device menu on the volume and
power keys, Quest's bootloader menu on the same keys, SteamOS's desktop on external media. This
spec is Mura's: **one program that owns the recovery actions and the menu**, reached from the
panels with the HMD's own buttons, from a shell over ssh or the console, and from the setup web
page on the cable or hotspot.

## 2. Actions — `mura-recovery action <name>`

The only place the actions live. Every frontend calls them; none re-implements one.

| Action | Does | Preconditions | Exit |
|---|---|---|---|
| `status` | prints: the last preflight summary (`/run/mura/preflight.summary`), the host-key source and fingerprint (`/run/mura-recovery/{keysource,fingerprint}`), the gadget address, the hotspot SSID/PSK when `/run/mura/hotspot.env` exists | — | 0 |
| `factory-reset --confirmed` | `systemd-repart --dry-run=no --factory-reset=yes --definitions=/etc/repart.d <disk>` where `<disk>` is the parent of the persist partition (sysfs `/sys/class/block/<part>/..`; the device itself when it is not a partition); then `sync` and `systemctl reboot` unless `MURA_RECOVERY_NO_REBOOT` is set | `--confirmed` present; `/etc/repart.d` non-empty | 0 done · 1 refused or repart failed · 2 usage |
| `switch-slot` | runs the family's `switchSlotCommand`, then `systemctl reboot` | the command is configured | 0 · 1 · 2 (no command) |
| `reboot` | `systemctl reboot` | — | 0 |
| `poweroff` | `systemctl poweroff` | — | 0 |

`factory-reset` without `--confirmed` exits 2 and does nothing: the confirmation is the
frontend's job (§3), the flag is the frontend's statement that it happened. The reset deletes and
re-creates exactly the `FactoryReset=yes` partitions (`repart.d(5)`); the slots and the ESP are
untouched; the host key in `identity/` goes with the state, so the headset has a new SSH
identity afterwards — every frontend says so before asking.

## 3. Menu model

One state machine, shared by the panel and the shell frontends (`menu.rs`; unit-tested).

```
Main                                   Confirm (entered from "Factory reset…")
  > Try again (reboot)      [default]    > Cancel                        [default]
    Factory reset…                         Erase everything
    Switch system slot   (if configured)
    Power off
    Show details
```

- `Try again` is first and selected by default — Android's boot-loop prompt (`recovery.cpp`
  `prompt_and_wipe_data`: *"Try again"* / *"Factory data reset"*). `Factory reset…` is never the
  default and never acts directly: selecting it opens **Confirm**.
- **Confirm** header: *"Erase everything this headset has stored? Accounts, settings, Wi-Fi,
  pairings. The headset gets a new SSH identity. THIS CANNOT BE UNDONE."* Items `Cancel`
  (default) and `Erase everything` — Android's `ask_to_wipe_data` (*" Cancel"*, *" Format data"*,
  default 0). `Cancel`, or the back key, returns to Main with `Factory reset…` still selected.
- `Show details` prints the `status` text and returns on any key.
- Inputs: `Next`, `Prev`, `Select`, `Back`. Output: at most one action per input.

## 4. Key semantics (panel frontend)

Roles come from the device contract, as evdev codes in `/etc/mura/recovery.json` (§6):

| Role | Input | Note |
|---|---|---|
| `volumeDown` | `Next` | also `KEY_DOWN` |
| `volumeUp` | `Prev` | also `KEY_UP` |
| `selectRole` | `Select` | also `KEY_ENTER`; on the Galaxy XR `select` shares `power`'s code — a short release selects, a long press is ignored |
| `backRole` | `Back` | also `KEY_ESC`; when `backRole` is `volumeDown` the same release is `Next` on Main and `Back` on Confirm |

A code that carries two roles resolves in this order: on Confirm, `Back` first; otherwise
`Select`, `Next`, `Prev`, `Back`. So on a three-button target with the contract's defaults
(`selectRole = volumeUp`, `backRole = volumeDown`) volume-up **selects** and volume-down **moves**
— the list wraps, so one direction reaches every item (Android recovery's menus wrap the same way).
`Prev` exists only where a dedicated select button frees volume-up.

Rules, all from Android recovery's `RecoveryUI::ProcessKey` (`recovery_ui/ui.cpp`):

1. **A key registers on release** (value 0), never on press (1); auto-repeat (2) is ignored. A
   held key is therefore one input, whatever the kernel's repeat rate.
2. A press held **≥ 750 ms** registers as *long* on release and the menu ignores it (the
   comparable's *"750 ms == 'long'"*; reserved for a future chord).
3. **Confirm can only be entered by a release**, so a key that was already down when Confirm
   appears cannot select in it; and its default is `Cancel`, so the next release of the same key
   that opened it does nothing destructive.
4. `KEY_POWER` on its own is never `Select` — it is the PMIC key and logind's business in stage 2;
   only the contract's `selectRole` code selects.
5. The keyboard fallback codes (`KEY_UP/DOWN/ENTER/ESC`) are accepted on every target — Android
   accepts `KEY_UP/KEY_DOWN` beside the volume keys — which is also what lets the VM test drive
   the menu through QEMU's PS/2 keyboard.

## 5. Rendering (panel frontend)

plymouth's client protocol caps a message at 255 bytes; one screen is **one message ≤ 200
bytes**: the title line, then the items, `> ` marking the selected one. A redraw is
`plymouth hide-message --text=<previous>` then `display-message --text=<new>`; the illustration
and the ways-in lines (`ssh root@172.16.42.1`, host-key fingerprint, help URL) are drawn once at
start and stay — the menu replaces only its own message. If plymouth is not running (no DRM
device in stage 1) the panel frontend logs that and keeps the key loop running so the shell and
web frontends are unaffected.

## 6. Configuration — `/etc/mura/recovery.json` (stage 1, from the contract)

```json
{ "keys": { "next": [114, 108], "prev": [115, 103], "select": [353, 28], "back": [114, 1] },
  "switchSlotCommand": null, "persistDevice": "/dev/disk/by-partlabel/syspersist",
  "gadgetAddr": "172.16.42.1", "docsUrl": "https://mura.dev/recovery", "longPressMs": 750 }
```

Codes are the contract's `hmdButtons` resolved through the evdev table (`KEY_POWER` 116,
`KEY_VOLUMEUP` 115, `KEY_VOLUMEDOWN` 114, `KEY_SELECT` 353, `KEY_ENTER` 28, `KEY_UP` 103,
`KEY_DOWN` 108, `KEY_ESC` 1) with the keyboard fallbacks appended.

## 7. Frontends

| Frontend | Runs as | How | Confirms by |
|---|---|---|---|
| `mura-recovery panel` | `mura-recovery-panel.service` in stage 1 (`Restart=on-failure`) | opens every `/dev/input/event*`, `poll(2)`, §4 → §3 → §2; draws per §5 | the Confirm screen (§3) |
| `mura-recovery` (bare; `shell` is an alias) | root over ssh (administrators' keys, recovery.nix) or the console — what the banner says to type | the same items as a numbered prompt; `Show details` inline | typing `yes, erase` |
| `mura-setup --recovery` | `mura-setup-recovery.service` in stage 1, root (no other identity exists there) | the stub's HTTP on the gadget + hotspot addresses (`IP_FREEBIND`): `GET /` status + buttons; `POST /reboot`; `POST /factory-reset` → `400` unless the form field `confirm=erase` is present, else `mura-recovery action factory-reset --confirmed` | one confirmed POST (the Quest app's "Factory reset → Reset" is one confirmed tap); possession of the cable or the per-boot PSK authorises it, as for setup (first-run §5) |

The web frontend never wipes on a `GET`, never on a `POST` without the field, and never serves
on the LAN (the two listen addresses only). Both recovery transports are required: USB gadget
and a per-boot-PSK recovery hotspot; each exposes sshd and this web frontend. A target has not
completed recovery bring-up until its radio/firmware, regulatory domain and AP mode pass in
stage 1.

## 8. Conformance checklist

VM (`tests/vm/recovery.nix`, stage 1 through the test driver's initrd backdoor, keys through
QEMU's keyboard):

1. The panel menu is drawn on plymouth with `Try again` selected, in one message ≤ 200 bytes;
   `/etc/mura/recovery.json` carries the contract's roles with the keyboard fallbacks appended.
   **Verified.**
2. `down` moves the selection and wraps after the last item; `up` moves back; `ret` on
   `Show details` shows the ways in and any key returns. **Verified.**
3. A `down` held for two seconds (QEMU `sendkey down 2000`; the kernel auto-repeats meanwhile)
   is one *long* release and changes nothing; `esc` on Main changes nothing. **Verified.**
4. `ret` on `Factory reset…` shows Confirm with `Cancel` selected and the "cannot be undone"
   header; `esc` there returns to Main; `ret` there cancels and nothing is erased. **Verified.**
5. `down` then `ret` on Confirm erases: repart ran, the `syspersist` partition is re-created
   empty with a new partition UUID, the disk still has exactly that one partition, and the panel
   is back on Main. **Verified.**
6. Bare `mura-recovery` over ssh from the cable's host end, with a wheel member's key, lists the
   same items under the banner carrying the panel's fingerprint; `2` then anything but
   `yes, erase` prints `Not erased.` and touches nothing. **Verified.**
7. `GET /` on the gadget address serves the recovery page with the fingerprint; `POST
   /factory-reset` without `confirm` → `400`, nothing erased; with `confirm=erase` → `200` and the
   reset. **Verified.**
8. `mura-recovery action factory-reset` without `--confirmed` exits 2 and touches nothing.
   **Verified.**
9. The button devices may appear after the panel started (udev coldplug — the VM's PS/2
   keyboard does): the panel picks them up (inotify on `/dev/input`). **Verified** (the VM).
10. The recovery hotspot generates an eight-digit per-boot PSK, hostapd brings up a
    `Mura-Recovery-*` AP on hwsim, a simulated phone associates, and both the web page and
    administrator-key SSH answer on `10.42.0.1`; restarting hostapd retains the credentials
    already shown on the panel. **Verified.**

Hardware (recorded in the implementation-path track, pending): the volume/select keys reach
evdev in stage 1 on each target (the input drivers — gpio-keys, the PMIC power key — in the
initrd beside the DRM driver); the menu text is legible per eye at the theme's font size; the
Galaxy XR's shared select/power code behaves per §4 rule 4.
Each target's radio firmware, regulatory domain and AP/ACS support must also pass the hotspot
proof on hardware.

Not verifiable in the VM: `KEY_POWER`'s exclusion (§4 rule 4 — QEMU's keyboard has no power
key reaching stage 1) and the reboot after the reset (`MURA_RECOVERY_NO_REBOOT` in the test).

## 9. Open items

**The reset's EBUSY window** (found by the VM test): the kernel refuses to drop a partition anyone
holds open — udev's blkid re-probe after a close-for-write, a mount from the shell — and
`systemd-repart` then leaves the on-disk table without the partition while the kernel still
knows it. The action settles udev first and, on failure, re-reads the table (`BLKRRPART`) and
retries, five attempts a second apart; each attempt is idempotent. A partition still *mounted*
(someone in the shell frontend inspecting `/persist`) is unmounted by the action first, whoever
mounted it — Android recovery's `EraseVolume` calls `ensure_volume_unmounted` before
`format_volume` ([external] `android_bootable_recovery/install/wipe_data.cpp`, lineage-22.2):
a wipe under a live mount is undefined and the wipe was just confirmed, which transfers as is.
If a mount will not go (a shell's cwd inside it) the reset fails before touching the disk and
says so, as Android's does ("Failed to unmount volume!").

Long press (§4 rule 2) is reserved — a chord for "show details" or "power off" once a target
needs it (decider: the first hardware proof); the shell frontend's `Show details` over a serial
console with no keyboard; whether the web frontend should also offer `switch-slot` (decider: the
uefi-rauc manual proof).
