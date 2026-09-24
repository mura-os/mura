# 42 — Input bootstrap: what a headset can accept before anything is configured, and how text gets in

**Question.** On a freshly flashed headset with nothing configured, how do existing XR platforms
and Linux pre-login surfaces establish an input method, and how does a person enter text
(username, password, Wi-Fi passphrase) — at first boot, at the login screen, and in the first
session? Feeds [first-run-onboarding.md §4.2/§4.4/§5](../architecture/first-run-onboarding.md)
(welcome-surface contents, input requirement, out-of-band provisioning) and
[multi-user.md §2](../architecture/multi-user.md); companion to the
[doc 11 greeter-furniture addendum](11-display-managers-greeters.md).
This document owns bootstrap UX and policy. Native per-target IMU→Monado and wear-sensor paths are
canonical in [45](45-imu-3dof-monado-native-linux-audit.md) and
[53](53-proximity-presence-native-linux-audit.md); physical/stock availability here does not imply
those paths are qualified.

**Method.** Code study of the pinned clones (`references/`, MANIFEST 2026-09-24: gnome-shell,
gdm, sddm, plasma-workspace, kwin, lightdm, greetd family, squeekboard, wvkbd, bluez, systemd,
libinput, networkmanager, accountsservice, cockpit, wifi-connect, comitup, raspap, luci,
gnome-initial-setup, monado + galaxyxr fork, wivrn, wayvr, xrdesktop, simula, stardustxr-server,
kwin-vr, breezy-desktop/xr-linux-driver, jovian-nixos, pmaports/pmbootstrap, the archived Steam
Frame DTS) plus web research marked [external]. Paths are `<repo>/<path>:<line>` relative to each
clone. Consumer platforms are cited for **mechanism and measurement only** (AGENTS.md rule 2).
Two hypotheses were recorded before the survey so it could confirm or refute them (§7.1):
(i) IMU head-aim plus the HMD's own buttons is available on every target and could serve as a
universal input floor; (ii) declared configuration, USB HID, and an optional PIN pad make
pre-login text entry rare.

**Premise (not a finding).** Mura's greeter and lock run on IMU-only tracking
([ADR 0007](../architecture/adr/0007-session-greeter-lock.md) §Two profiles): no cameras ⇒ no
hand tracking, no eye tracking, no 6DoF controllers, no passthrough, before a session starts.

---

## 1. How XR platforms bootstrap input at first power-on [external]

Every shipping standalone headset assumes at power-on either paired 6DoF controllers or
camera-based hand/eye tracking, and most route the person through a phone app or account before
the device is usable. The fallback when controllers are missing is the same everywhere it
exists: **a head-locked crosshair with a headset button as "click"**. No platform documents an
IMU-only text-entry path beyond that; Wi-Fi passphrases are typed on an in-headset keyboard
driven by that crosshair, or handed over from a phone.

| Platform | Input assumed at power-on | Controllers absent | Wi-Fi entry | Account/phone mandatory | HW-button input mode |
|---|---|---|---|---|---|
| Quest 3/3S (Horizon OS) | Touch controllers | after 5–10 min at the pairing screen a "gaze" mode: head moves cursor, **volume buttons select** (documented only for warranty replacements) | in-headset keyboard, or a Wi-Fi QR from the Meta phone app **scanned by the headset's cameras** | Meta account + Horizon app for "initial pairing and setup" | yes — white dot + volume (also for the passcode dialog) |
| Apple Vision Pro | eyes + hands (Optic ID / eye setup in OOBE) | n/a | Quick Start proximity transfer from an iPhone/iPad, else typed in Settings | Apple Account sign-in is a setup step | double-click Digital Crown = "Set Up Manually"; top button = power |
| PICO 4 / 4 Ultra | controllers ("pull the trigger to proceed") | **Head Control Mode**: with no controller connected the head moves a crosshair and **Volume Up/Down click**; Vol-Down long-press recenters | in-headset list + password | PICO account (guides: create it in the phone app) | yes (Head Control Mode is a documented, first-class feature) |
| Samsung Galaxy XR (Android XR) | hands + eyes; auto-powers when worn | not required | "Copy accounts and info" from a Galaxy phone, or manual keyboard | Google account required | Top button (1/2/3 presses); Vol-Up+Vol-Down hold = TalkBack during setup |
| HTC Vive Focus Vision | controllers; VIVE Manager app "easiest" | manual setup by holding the **headset button** at the Welcome screen | via VIVE Manager: typed **or Wi-Fi QR** (headset cameras) | HTC account on the app path; the manual path bypasses it | yes — headset button starts/continues setup |
| Steam Deck (SteamOS OOBE) | touchscreen + gamepad | n/a | on-screen keyboard: touch, trackpads, or D-pad/stick + A | Steam account required (Steam Guard or phone QR) | power/volume only |
| Valve Steam Frame | controllers | **Aux button** (right side, above power) "navigate[s] the initial menus to log in… without controllers" | on-screen keyboard | Steam account; sign-in via Steam Mobile, typed, or **phone camera reading a QR through the headset lens** | yes (Aux = click) |
| Lynx R-1 | Ultraleap hand tracking + headset buttons | controllers optional/not shipped | Settings › Wi-Fi list + password field | no account store | long-press Action button opens the Lynx menu |

Sources: Quest setup https://www.meta.com/help/quest/10004693912934783/ , gaze fallback
https://www.meta.com/help/quest/1193849938692424/ , passcode via dot + volume
https://www.meta.com/help/quest/967070027432609/ , Wi-Fi QR https://www.meta.com/help/quest/1503826183789419/ ;
Vision Pro https://support.apple.com/guide/apple-vision-pro/turn-on-and-set-up-devd5d9e3a52/visionos ;
PICO 4 Ultra guide (Head Control Mode) https://p16-platform-static-va.ibyteimg.com/tos-maliva-i-jo6vmmv194-us/pico4-ultra-user-guide-eu.pdf ;
Galaxy XR https://www.samsung.com/us/support/answer/ANS10007502/ , https://support.google.com/android-xr/answer/16635797 ;
Vive Focus Vision https://www.vive.com/us/support/focusvision/category_howto/setting-up-the-headset-for-the-first-time.html ;
Steam Frame developer setup (Aux button) https://partner.steamgames.com/doc/steamhardware/steamframe/setup ,
quick-start video (QR through the lens) https://youtu.be/yD-QHvRv-q4 ; Lynx R-1
https://knowledge.vr-expert.com/kb/getting-started-with-the-lynx-r1/ .

Reading. Two platforms ship head-crosshair + hardware-button as a normal, documented feature
(PICO's Head Control Mode; Steam Frame's Aux button); Quest hides the same mechanism behind a
timeout. Every account-mandatory flow is a walled-garden gate and is cited here only to show
that the *input* problem underneath is solved identically by all of them. The two QR directions
matter for us: headset-scans-phone needs cameras (unavailable pre-login here);
phone-scans-headset works with a display only (Steam Frame's sign-in, Steam Deck's Steam Guard).

## 2. Text entry in VR

### 2.1 Mechanisms on commercial platforms [external]

All converge on: far ray + trigger/pinch, near direct-touch (poke), swipe over either,
dictation, and Bluetooth/USB keyboards. Quest lists "smartphone input" as a system-keyboard
capability; visionOS has no first-party phone-as-keyboard. Quest 3/3S renders tracked physical
keyboards through a passthrough cutout.
Quest: https://developers.meta.com/horizon/design/virtual-keyboard/ ,
https://developers.meta.com/horizon/documentation/native/android/mobile-keyboard-overlay/ ,
https://www.meta.com/help/quest/172903867975450/ ; visionOS:
https://support.apple.com/guide/apple-vision-pro/enter-text-and-use-dictation-tana14220eef/visionos ,
https://support.apple.com/en-us/118516 ; Galaxy XR: https://www.samsung.com/us/support/answer/ANS10007565/ .

### 2.2 Measured rates [external]

| Technique | WPM | Error | Source |
|---|---|---|---|
| Controller ray + trigger (Speicher et al. 2018) | 15.4 | ~1 % uncorrected | https://dl.acm.org/doi/10.1145/3173574.3174221 |
| **Head pointing + controller click** (same) | **10.2** | comparable | same |
| Controller raycast (Boletsis & Kongsvik) | 16.7 | 11.1 % total | https://ijvr.eu/article/view/2917 |
| **Head-directed + click** (same) | **10.8** | 10.2 % total | same |
| **Head + dwell** "DwellType" (Yu et al. CHI '17) | **10.6** | 95.8 % accuracy | https://dl.acm.org/doi/10.1145/3025453.3025964 |
| Head + tap / head gesture-typing (same) | 15.6 / 19.0 → 24.7 after 60 min | 2.0 % (tap) | same |
| Gaze + dwell 550 ms in VR (Rajanna & Hansen ETRA '18) | 10.2 | gaze+click beat dwell | https://psycnet.apa.org/doi/10.1145/3204493.3204541 |
| Classic dwell eye-typing, 450–1000 ms dwell | 5–10 | — | https://pokristensson.com/pubs/HamidKristenssonETRA2024.pdf |
| Desktop keyboard in VR (Grubert et al. 2018) | 26.3 (≈58 % of baseline) | — | https://pokristensson.com/pubs/GrubertEtAlVR2018a.pdf |
| TV remote numeric keypad, 10 sessions | 9.3 → 17.7 | — | https://dl.acm.org/doi/10.1145/985692.985773 |
| TV 5-key remote, dual-cursor QWERTY, 10 min | ≈6.7 | — | https://doi.org/10.1093/iwc/iwz017 |
| TV freehand gesture keyboards, 5 days | 5.6–8.5 | — | https://purehost.bath.ac.uk/ws/files/13289951/ITV_final_version.pdf |

Reading. Head-pointing with a click sits at ~10 WPM (≈50 cpm); a 12-character WPA passphrase is
15–20 s. D-pad grid typing is 2–3× slower than head-aim. No published figure exists for
head-pointer + a single HMD hardware button at consumer scale; the nearest are Speicher's
head+click (10.2) and Yu's TapType (15.6, with prediction).

### 2.3 Open-source XR shells (code)

None of the OSS shells implements dwell; every keyboard is a click-driven virtual keyboard aimed
by a ray or fingertip. Only WayVR has a head-only pointer mode — and its clicks arrive over IPC.

- **WiVRn lobby** (Dear ImGui): keyboard shown only while `WantTextInput`, on its own layer —
  `wivrn/client/scenes/lobby_gui.cpp:1078-1082`, `client/constants.h:87-88`; layouts
  QWERTY/AZERTY/symbols/digits with long-press diacritics — `client/scenes/lobby_keyboard.cpp:41-219`;
  digit pad auto-selected for `CharsDecimal` fields — `lobby_keyboard.cpp:470-474`; repeat/dwell
  code present but commented out — `lobby_keyboard.cpp:290-345`. Pointer arbitration: trigger
  threshold 0.7 — `constants.h:43`; hand touch-vs-aim hysteresis 0.18/0.22 m — `constants.h:46-47`,
  `client/render/imgui_impl.cpp:614-626`; mouse-button events fire only on `trigger_clicked ||
  fingertip_touching` — `imgui_impl.cpp:796-800`. **No head-gaze click**; head pose only places
  the GUI — `client/scenes/lobby.cpp:114-124`; with neither controllers nor hands the lobby is
  non-interactive. **PIN pairing**: 6-digit pad, auto-submit at six digits, "Input the PIN displayed
  on the dashboard" — `lobby_gui.cpp:150-236`.
- **WayVR** (Rust): YAML keyboard layout, labels from the XKB keymap —
  `wayvr/wayvr/src/res/keyboard.yaml:17-56`, `src/overlays/keyboard/mod.rs:56-71`; fcitx5 IME
  following over D-Bus — `mod.rs:53, 92, 204-233`; keys injected as virtual keys —
  `mod.rs:487-493`; **modifier-as-click-button** (right-click = Shift, middle = configurable) —
  `mod.rs:482-486`, click button chosen by controller roll and rendered as laser colour —
  `src/backend/input.rs:216-227, 430-436`. **Hands-free modes** `None | Hmd | HmdPinch |
  EyeTracking | EyeTrackingPinch` — `wayvr/wayvr-ipc/src/packet_client.rs:24-30`; in `HmdOnly`
  the pointer is the smoothed head pose (lerp factor clamped 0.1–1.0) and click/grab/scroll come
  from `wayvrctl` IPC, not from any device — `src/backend/openxr/input.rs:380-419`; crosshair
  reticle — `input.rs:454-467`. Whisper push-to-talk STT → clipboard → Ctrl+V —
  `src/overlays/whisper.rs:304-324`. No dwell.
- **xrdesktop**: keys are `G3kButton` click targets registered per window —
  `xrdesktop/src/xrd-shell.c:121-146`; keyboard bound to one destination window and hidden when it
  closes — `xrd-shell.c:366-376, 570-579`. G3k source not pinned.
- **Simula**: physical keyboard only — `simula/addons/godot-haskell-plugin/src/Plugin/SimulaServer.hs:97-172`,
  `Input.hs:26-110`; head-ray cursor set absolutely on shortcuts, no smoothing —
  `SimulaServer.hs:1106-1109, 1627-1628, 174-176`.
- **StardustXR server**: keyboard input routed to the handler nearest the pointer hit within
  `KEYBOARD_FOCUS_MARGIN = 0.05` m — `stardustxr-server/src/objects/input/mouse_pointer.rs:52-54, 203-212`;
  no on-screen keyboard.
- **kwin-vr**: ray parented to the XR camera with configurable offset — `kwin-vr/src/plugins/vr/qml/XrScene.qml:177-179`,
  `kwinvr.kcfg:181-199`; raw distance-ordered picking — `VrPicking.qml:34-64`; the only
  stabiliser is a 100 ms post-press motion inhibit — `kwinvrinputfilter.cpp:30, 107-117`; clicks
  from controller actions — `VrInputBindings.qml:24-43`. (Users asked for smoothing/magnetism:
  [doc 31 §2.11](31-kwin-vr.md).)
- **breezy-desktop / xr-linux-driver**: head angular velocity → `REL_X/Y` uinput mouse with
  sensitivity 30 and sub-pixel carry — `xr-linux-driver/src/outputs.c:365-369, 584-592`,
  `src/config.c:35-36`.

## 3. Linux pre-login input handling (code)

### 3.1 On-screen keyboards and accessibility at the greeter

GDM's greeter is a full gnome-shell in `gdm` session mode whose OSK is the shell's in-process
`Keyboard` actor, typing into shell widgets through Clutter — no Wayland OSK protocol. Plasma's
login compositor is `kwin_wayland --inputmethod plasma-keyboard`, spawning the OSK as an
`input-method-unstable-v1` client; SDDM explicitly refuses Qt VirtualKeyboard on Wayland. LightDM
core and all three greetd greeters ship no OSK.

- `gdm` mode panel: `right: ['dwellClick', 'keyboard', 'quickSettings']`, components
  `networkAgent` + `polkitAgent` — `gnome-shell/js/ui/sessionMode.js:51-65`; `unlock-dialog`
  adds `a11y` — `sessionMode.js:67-79`. Login-dialog accessibility button: High Contrast, Zoom,
  Large Text, Screen Reader, **Screen Keyboard**, Visual Alerts, Sticky/Slow/Bounce/Mouse Keys —
  `gnome-shell/js/gdm/loginDialog.js:327-350`.
- OSK shown when `screen-keyboard-enabled`, or automatically in touch mode —
  `gnome-shell/js/ui/keyboard.js:23-24, 974-977`; injection via `seat.create_virtual_device` and
  `Main.inputMethod.commit` — `keyboard.js:1856, 1980, 1994`. GDM's dconf profile forces
  `always-show-universal-access-status=true` — `gdm/data/dconf/defaults/00-upstream-settings:12-13`.
- SDDM: `InputMethod` default `qtvirtualkeyboard` — `sddm/src/common/Configuration.h:48`,
  cleared on Wayland ("has to be done by the compositor instead") —
  `sddm/src/greeter/GreeterApp.cpp:355-358`. Plasma: `CompositorCommand=kwin_wayland … --inputmethod
  plasma-keyboard` — `plasma-workspace/sddm-wayland-session/plasma-wayland.conf:4-7`; OSK toggle
  over `org.kde.KWin /VirtualKeyboard` — `plasma-workspace/components/loginlockscreen/Footer.qml:46-60`;
  KWin bridges text-input-v2/v3 to `zwp_input_method_v1` — `kwin/src/inputmethod.cpp:110-122, 572, 864-899`.
- LightDM core: no OSK/a11y code (`lightdm/src`; greeter hints only, `src/seat.c:578-580`);
  lightdm-gtk-greeter's `onboard` hook is [external]. gtkgreet: layer-shell keyboard interactivity
  only — `gtkgreet/gtkgreet/window.c:23-25`; regreet defers input to cage — `regreet/README.md:296-300`;
  tuigreet is a TTY program.

### 3.2 The two Wayland OSK lineages

Both squeekboard and wvkbd are layer-shell clients that type via `zwp_virtual_keyboard_v1`
(fake keycodes against a keymap) and optionally bind `zwp_input_method_v2`. Virtual-keyboard-v1
needs **no text-input cooperation from the focused client** — the compositor implements the
manager and routes keys to keyboard focus. Neither has dwell or switch scanning.

- squeekboard binds layer-shell, virtual-keyboard-manager, input-method-manager —
  `squeekboard/src/server-main.c:115-123, 176-194`; text via `commit_string` when the IM is
  active and no modifier is held, else keycodes — `src/imservice.c:39-53`, `src/submission.rs:115-180`.
- wvkbd: dies without `virtual_keyboard_manager` — `wvkbd/main.c:1232-1234`; IM used only for
  show/hide — `main.c:525-534, 1256-1260`; keys → `zwp_virtual_keyboard_v1_key` —
  `wvkbd/keyboard.c:281-294`; input is `wl_pointer`/`wl_touch` only — `main.c:36-37, 87-111`.
- Implication for zxr: reusing either requires layer-shell + virtual-keyboard-v1 (and ideally
  input-method-v2) in the compositor — the registry rows 152/180 gap
  ([component-registry.md](../architecture/component-registry.md)).

### 3.3 Bluetooth pairing before login

bluetoothd is a root system daemon; an agent is **any D-Bus client** owning an `org.bluez.Agent1`
object — no logged-in session is required. With no agent registered, bluetoothd falls back to
`NoInputNoOutput`, so SSP "just works" pairing (most keyboards/mice) completes with zero
interaction when the device is `Pairable`; numeric-comparison or PIN pairing needs an agent.
Paired-device state lives under `/var/lib/bluetooth/<adapter>/<peer>/info`. PAN/NAP (IP over
Bluetooth) is still compiled in by default.

- `RegisterAgent(agent, capability)`, `RequestDefaultAgent` — `bluez/src/agent.c:1032-1037`;
  capabilities `DisplayOnly | DisplayYesNo | KeyboardOnly | NoInputNoOutput | KeyboardDisplay` —
  `agent.c:951-966`, `bluez/doc/org.bluez.AgentManager.rst:25-54`; first agent becomes default —
  `agent.c:277-281`; no agent → "No agent available" — `bluez/src/device.c:7699-7707`.
- No agent ⇒ IO capability `NOINPUTNOOUTPUT` when pairable — `agent.c:125-137`,
  `bluez/src/adapter.c:9510-9526`; just-works incoming → `RequestAuthorization`, numeric
  comparison → `RequestConfirmation` — `device.c:7796-7800`, `bluez/doc/org.bluez.Agent.rst:118-124`.
- Storage `${localstatedir}/lib/bluetooth` — `bluez/configure.ac:505-511`, layout `/%s/%s/info` —
  `bluez/src/adapter.c:5164, 6305`. HID requires bonding (`ClassicBondedOnly`) —
  `bluez/profiles/input/input.conf:18-31`. D-Bus policy: any `context="default"` may talk to
  `org.bluez` — `bluez/src/bluetooth.conf:10-27`.
- Network profile compiled `if NETWORK` (default on) — `bluez/Makefile.plugins:45-51`,
  `configure.ac:179-181`; `NetworkServer1.Register(uuid, bridge)` / `Network1.Connect(uuid)` —
  `bluez/doc/org.bluez.NetworkServer.rst:25-30`, `org.bluez.Network.rst:24-27`.
- gnome-shell builds the Bluetooth indicator unconditionally, greeter included —
  `gnome-shell/js/ui/panel.js:318-334, 364-371`, `js/ui/status/bluetooth.js:49`.

### 3.4 USB HID hotplug into the greeter seat

Two independent mechanisms make a keyboard plugged in *during* the greeter work with zero
configuration: udev tags input devices `seat` (and DRM/sound/etc. `uaccess`, ACL'd to the seat's
active session uid), and the compositor that is the session controller calls
`login1.Session.TakeDevice`, receiving an fd that logind revokes (`EVIOCREVOKE`) on switch-away.
Session class `greeter` is allowed to `TakeDevice`. libinput enumerates `input` devices with
`ID_SEAT` (default `seat0`) and follows the udev monitor for `add`/`remove` of `event*` nodes.

- `uaccess` tags — `systemd/rules.d/70-uaccess.rules.in:14-49`; `SUBSYSTEM=="input" … TAG+="seat"` —
  `rules.d/71-seat.rules.in:12-15`; `uaccess` builtin ACLs the active uid —
  `systemd/src/udev/udev-builtin-uaccess.c:48-63, 123`; `seat_set_active` retriggers —
  `src/login/logind-seat.c:413-447`.
- `TakeDevice` requires `SESSION_CLASS_CAN_TAKE_DEVICE` + controller — `src/login/logind-session-dbus.c:563-610`;
  greeter class included — `src/login/logind-session.h:63`; revoke on pause —
  `src/login/logind-session-device.c:99-110, 255, 275-330`. greetd sets `XDG_SESSION_CLASS=greeter` —
  `greetd/greetd/src/session/worker.rs:47, 217`.
- libinput seat assignment and hotplug — `libinput/src/udev-seat.c:82-101, 170, 201-218, 262-284, 398`;
  `open_restricted` — `libinput/src/libinput.h:3613-3631`.

### 3.5 Accounts with an empty password

pam_unix refuses an empty password unless `nullok` [external: pam_unix(8)]. NixOS maps
`security.pam.services.<n>.allowNullPassword` (default false) to `nullok`, **turns it on for
greetd**, and the *password-change* stack always has `nullok` — so `passwd` for a blank-password
user asks no old password. sshd's `PermitEmptyPasswords` defaults to `no` [external: sshd_config(5)];
NixOS `services.openssh.settings` is freeform and `extraConfig` takes `Match` blocks. sudo's PAM
service inherits `allowNullPassword = false`, so a passwordless wheel user cannot `sudo` unless
`wheelNeedsPassword = false`. AccountsService `SetPassword` takes a pre-hashed string, never asks
the old password, and is polkit-gated: own account → `change-own-password` = `auth_admin` on all
three contexts.

- `allowNullPassword` — `nixos/modules/security/pam.nix:530-543`, wired at `pam.nix:1218, 1316`;
  password stack `nullok = true` — `pam.nix:1403-1408`.
- greetd module: `security.pam.services.greetd = { allowNullPassword = true; … }` —
  `nixos/modules/services/display-managers/greetd.nix:78-82`.
- sshd `settings` freeform — `nixos/modules/services/networking/ssh/sshd.nix:471-483`;
  `unixAuth = cfg.settings.PasswordAuthentication == true` — `sshd.nix:879`; `extraConfig` —
  `sshd.nix:737, 893`.
- sudo `wheelNeedsPassword` — `nixos/modules/security/sudo.nix:57-62, 260`; PAM service without
  `allowNullPassword` — `sudo.nix:321-324`.
- accountsservice `SetPassword` → `chpasswd -e` — `accountsservice/src/user.c:4025-4050`; polkit
  action selection — `user.c:4052-4055`; policy defaults — `accountsservice/data/org.freedesktop.accounts.policy.in:10-36`.
- Greeters themselves ship no `nullok`: GDM forbids null tokens only for remote displays —
  `gdm/daemon/gdm-session-worker.c:1363-1364, 1387-1392`; SDDM ships no PAM files; LightDM's
  `data/pam/lightdm` lacks `nullok`. A PAM success with no prompt takes gnome-shell straight to
  `verification-complete` — `gnome-shell/js/gdm/authPrompt.js:658-672`.

### 3.6 gnome-initial-setup (the wizard we do not have)

A libadwaita assistant run by GDM as a kiosk session under the `gnome-initial-setup` system user
in *new-user* mode, driving GDM's greeter D-Bus API to create and log in the account; in
*existing-user* mode it currently exits immediately. Pages: welcome, language, keyboard,
network, privacy, timezone*, software*, account*, password*, parental-controls*, summary
(* = new-user only) — `gnome-initial-setup/gnome-initial-setup/gnome-initial-setup.c:62-79, 243-250, 322-326`;
modes — `gnome-initial-setup/gis-driver.h:43-45`, `gis-driver.c:854-874`; kiosk session —
`data/gnome-initial-setup.session:1-3`. Its keyboard page sets XKB/IBus sources, not an OSK —
`pages/keyboard/gis-keyboard-page.c:44, 111`. The OEM-preinstall gap it fills does not exist
for a declared image ([ADR 0017 rev 2](../architecture/adr/0017-first-run-provisioning.md)).

## 4. What exists with the cameras off (code)

### 4.1 Monado: 3DoF is the floor every relevant driver keeps

Monado has one shared 3DoF fusion helper, `m_imu_3dof` (complementary gyro integration with
gravity correction) — `monado/src/xrt/auxiliary/math/m_imu_3dof.h:34-91`, `m_imu_3dof.c:180-227`.
Every HMD driver relevant to a standalone headset carries this path and treats SLAM as a runtime
addition: without SLAM the driver reports `orientation_tracking=true, position_tracking=false`
and returns orientation with a constant position.

- WMR: "We always have at least 3dof HMD tracking" — `monado/src/xrt/drivers/wmr/wmr_hmd.c:1674-1696`;
  dispatch `slam_enabled && slam_over_3dof ? slam : 3dof` — `wmr_hmd.c:1185-1189`, 3DoF predict —
  `:1096-1116`. Rift S: same — `rift_s/rift_s_tracker.c:52-65, 353-364`. Vive/Index —
  `vive/vive_device.c:198-201, 521-525`. PSVR — `psvr/psvr_device.c:982-988, 1179-1180`. Android
  sensors: gyro-only fusion — `android/android_sensors.c:169-175, 446-447`. Also Rift CV1, xreal_air,
  rokid, blubur_s1, North Star (`north_star/ns_hmd.c:490-494`).
- **No IIO driver exists in upstream Monado** (no `iio` hits in `src/`, `doc/`, CMake). The
  Galaxy XR fork reads the IMU from the Qualcomm SSC over `AF_QIPCRTR` QMI, orientation-only —
  `monado-galaxyxr/src/xrt/drivers/galaxyxr/galaxyxr_ssc.c:5-8`, README `:65-69, 100`,
  `galaxyxr_hmd.c:951-952`. A Mura HMD driver for Frame/Lynx/Quest (IIO or SSC) is new code.

### 4.2 Controllers without optical tracking

Controllers with their own IMU deliver **buttons + orientation-only poses**; Monado has no
arm model or "orientation-only ray" abstraction — drivers set `ORIENTATION_VALID|TRACKED` without
position bits, and some hard-code a plausible position.

- WMR controllers: fusion — `monado/src/xrt/drivers/wmr/wmr_controller_base.c:503, 585`; position
  **hard-coded** `{±0.2, 1.2, −0.5}` — `:460-471`; `position_tracking=false` — `:581-582`.
- Rift S Touch — `rift_s/rift_s_controller.c:205-206, 529-531, 548-552, 620`. Vive/Index
  controllers over the USB dongle (lighthouse decoding is a `@todo`) — `vive/vive_controller.c:622-626,
  914, 1227-1228`. PS Move "Orientation-only tracking" — `psmv/psmv_driver.c:680-691, 814-816`.
  Daydream / Arduino 3DoF — `daydream/daydream_device.c:246, 331-334`, `arduino/arduino_device.c:283, 356-359`.
- `qwerty` (keyboard/mouse as HMD + two WMR-profile controllers — `qwerty/qwerty_device.c:340, 377-389`,
  bindings `qwerty/qwerty_sdl.c:177-274`) reads events **only from the SDL debug-GUI window** —
  `monado/src/xrt/auxiliary/util/u_debug_gui.c:63-66, 182-183, 283`; enabling it disables other
  drivers unless `QWERTY_COMBINE` — `state_trackers/prober/p_prober.c:60-61, 347-365`. It cannot
  drive a headless greeter from evdev as-is.

### 4.3 The HMD's own buttons, and how they reach userspace

Every target has a power button and a volume rocker; several have a third button
(Quest 3S action button, Lynx "R" action button, Steam Frame **Aux**, Galaxy XR top button, Vive
headset button); most have a proximity/wear sensor. [external] Steam Frame: https://partner.steamgames.com/doc/steamhardware/steamframe/setup ;
Quest 3/3S https://www.meta.com/help/quest/617966963105359/ ; Lynx https://portal.lynx-r.com/documentation/view/getting-started ;
Galaxy XR https://www.samsung.com/us/support/answer/ANS10007502/ ; PICO 4 https://www.picoxr.com/global/products/pico4e/specs ;
Vive XR Elite https://dl4.htc.com/Web_materials/Manual/Vive_XR/VIVE_XR_Elite_UG.pdf .

**Donor-verified on the Steam Frame** (archived DTS): PMIC power key `linux,code=<0x74>`
(KEY_POWER) and `resin` `0x72` (KEY_VOLUMEDOWN) —
`archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:5572-5584`;
`gpio-keys` "Volume Up" `0x73` and **"Select" `0x161` (KEY_SELECT)** — `:9186-9204`; a
`vishay,vcnl4040` IIO proximity/light sensor — `:2300-2308`; an `allegro,als31300` 3D Hall sensor
(plausibly the IPD wheel; inference) — `:2622-2626`. Galaxy XR power = `KEY_POWER` on the PMIC
pwrkey evdev — `monado-galaxyxr/src/xrt/drivers/galaxyxr/README.md:115-120`.

**Kernel → userspace.** PMIC and `gpio-keys` buttons are ordinary evdev devices. logind tags
`ID_INPUT_KEY` nodes `power-switch` — `systemd/rules.d/70-power-switch.rules:12-13`, opens them
— `src/login/logind-core.c:347, 365` — but **does not `EVIOCGRAB`**: it uses `EVIOCSMASK` to
receive only KEY_POWER/POWER2/SLEEP/SUSPEND and SW_LID/DOCK — `src/login/logind-button.c:33-36, 529-577`.
The compositor therefore sees the same events through libinput, which treats `ID_INPUT_KEY`
devices as keyboards and reports every EV_KEY code including KEY_POWER/VOLUME*/SELECT as
`LIBINPUT_EVENT_KEYBOARD_KEY` — `libinput/src/evdev.c:82-83, 2021-2023`, `src/evdev-fallback.c:34, 618`.
Two ways to stop logind acting on the power key: `HandlePowerKey=ignore` (default `poweroff`,
`systemd/src/login/logind.conf.in:28`; early return — `logind-action.c:356-360`; if all handlers
are ignore, logind never opens button devices — `logind.c:224`, `logind-core.c:728-745`) or a
session-scoped `Inhibit("handle-power-key")` — `logind-inhibit.c:517-518`, polkit action
`inhibit-handle-power-key` — `logind-dbus.c:3723`, honoured unless `PowerKeyIgnoreInhibited=yes` —
`logind-action.c:373-378`. Volume keys are never logind's. The Steam Deck precedent does exactly
this: `HandlePowerKey=ignore` "Conflicts with powerbuttond" — `jovian-nixos/modules/steam/steam.nix:124-127`,
with a user-level libevdev `steamos-powerbuttond` forwarding short/long presses —
`pkgs/powerbuttond/default.nix:37`, `pkgs/powerbuttond/jovian.patch:8-20`. The Galaxy XR fork
instead `EVIOCGRAB`s the power device and re-exports it as `/user/head/input/system/click` —
`monado-galaxyxr/src/xrt/drivers/galaxyxr/galaxyxr_hmd_input.c:44, 166-186` — a design choice, not
a necessity.

### 4.3a Which button is "select"? Per-target conventions (added 2026-09-24)

Follow-up asked after the review: do we *know* the vendor's confirm convention per target, its
physical location, and the evdev code — or was the `selectRole = "volumeUp"` default an
inference? Answer: it was an inference that the survey below now grounds; one target changes
the picture (the Galaxy XR's only candidate is the power key). Repo targets first, reference
devices after. `FRAME` = `archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0`.

**Valve Steam Frame (deckard).** Buttons: **Aux** (right side, "just above the power button"),
power (right, recessed, below Aux), volume ± (left), mechanical IPD dial with lock. Vendor:
the Aux "controls the cameras or selecting options in the menu without controllers"; Valve's
developer setup says "navigate the initial menus to log in using the Aux button" [external:
https://partner.steamgames.com/doc/steamhardware/steamframe/setup ;
https://www.pcgamer.com/hardware/vr-hardware/steam-frame-specs-availability/ ]. Codes,
donor-verified: gpio-keys `"Select"` → `linux,code = <0x161>` (**`KEY_SELECT` = 353**),
`linux,can-disable` — `FRAME/extracted/sm8650-mp.dts:9199-9205`; `"Volume Up"` `0x73` —
`:9191-9197`; PMIC `pwrkey` `0x74` — `:5572-5577`; `resin` `0x72` (Vol−, `disable-wake-source`)
— `:5579-5585`. Those four are the *only* `linux,code` nodes in the DTS; no touchpad or
capacitive surface. SteamOS side (rootfs read from `FRAME/images/rootfs.img`): power button
tagged `STEAMOS_POWER_BUTTON=1` via `usr/lib/udev/hwdb.d/70-steamos-power-button.hwdb:52-53`,
`HandlePowerKey=ignore` in `etc/systemd/logind.conf.d/10-logind-no-powerbutton.conf`; **no
hwdb or udev remap of `KEY_SELECT` anywhere** — the "Aux = click" semantic lives inside
Steam/SteamVR, i.e. the *compositor-side* consumer, exactly the arrangement A2 adopts.

**Meta Quest 1 (monterey).** Power (right side), volume ± (right underside), mechanical IPD
slider. Vendor no-controller convention: the head-gaze fallback where "the volume button is used
to select or interact" [external: https://developers.meta.com/horizon/design/interactions-input-modalities/ ,
https://beta.developers.meta.com/horizon/design/head/ ]; the boot menu is "volume buttons to
navigate, power button to select" [external: https://www.meta.com/help/quest/1081950390666891/ ].
Codes [external, Meta's archived kernel `oculus-quest-kernel-master`]: PON `kpdpwr` `<116>`
(`msm-pm8998.dtsi:42-46`), Vol+ gpio-keys `<115>` (`msm8998-mtp.dtsi:585-592`), Vol− PON
`resin` `<114>` (`msm-pm8998.dtsi:48-52`).

**Meta Quest 3 (eureka).** Power (left side while worn, near USB-C), volume ± (right
underside), mechanical IPD wheel; the side **double-tap** for passthrough is IMU software — no
extra gpio key exists in `eureka-base.dtsi` / `anorak-oculus-base.dtsi` [external, Meta's
`oculus-quest3-kernel-master`]. Same head-gaze convention (either volume key clicks). Codes:
`pmk8550_pwrkey` `KEY_POWER` (`pmk8550.dtsi:26-30`), Vol+ gpio-keys `KEY_VOLUMEUP`
(`anorak-oculus-base.dtsi:261-276`), Vol− `resin` `KEY_VOLUMEDOWN` (`pmk8550.dtsi:32-36`).
**Quest 3S** (reference): adds an **action button** bottom-right, `mr_toggle`, `linux,code =
<KEY_SWITCHVIDEOMODE>` (227) — a *mode-switch* code, not a select code
(`oculus/panther/panther-base.dtsi:151-163` [external]).

**Lynx R-1.** Power (right of faceplate; 2 s on/off, short = standby), volume rocker (right), two
long top buttons **L** (left) and **R** (right), central mechanical eye-relief release; per-lens
IPD sliders. Vendor: **R** opens the Lynx Menu (quit/capture/screenshot/quick settings) and is
Android `KEYCODE_SOFT_RIGHT` since firmware 1.1.8; **L** has "no pre-defined action",
`KEYCODE_SOFT_LEFT`, developer-assignable [external: https://portal.lynx-r.com/documentation/view/getting-started ,
https://portal.lynx-r.com/documentation/view/lynx-menu-4 , firmware notes
https://portal.lynx-r.com/downloads/firmware/lynx-r-1/ ]. The vendor's default navigation is
Ultraleap hand tracking ("point and hold"); no head-cursor mode is documented. Mainline:
postmarketOS `device-lynx-r1` (dtb `qcom/sm8250-lynx-r1`) with `CONFIG_INPUT_PM8941_PWRKEY=y`
and `CONFIG_KEYBOARD_GPIO=y` — `pmaports/device/testing/linux-lynx-r1/config-lynx-r1.aarch64:3067, 2907`;
the DTS itself is not vendored (Gaps), so L/R/volume codes are unverified.

**Samsung Galaxy XR (SM-I610).** **Top button** (1× Launcher, 2× camera, 3× eye calibration,
hold = assistant, hold >7 s force restart, hold to power on), volume ± (side unspecified;
Top+Vol− short = screenshot, long = power menu), **touchpad on the right of the headband**
(double-tap passthrough, touch-and-hold recenter), motorized IPD [external:
https://www.samsung.com/us/support/answer/ANS10007517/ , https://www.samsung.com/us/support/answer/ANS10007549/ ,
https://www.samsung.com/us/support/answer/ANS10007511/ ]. **No head-aiming mode exists**:
aiming is "hand and eye" or "hand only" (Android XR lists hands, eyes, voice, BT peripherals,
6DoF controllers [external: https://developer.android.com/design/ui/xr/guides/foundations ]).
Code, fork-verified: the Top button **is the PMIC power key** — `pmic_pwrkey`, `/dev/input/event2`,
`KEY_POWER`, grabbed with `EVIOCGRAB` and re-exported as a Vive-Pro system click —
`monado-galaxyxr/src/xrt/drivers/galaxyxr/galaxyxr_hmd_input.c:23, 37, 44, 174`. Volume and
touchpad nodes are not handled by the fork (no public kernel; Anorak convention would put Vol+
on gpio-keys and Vol− on `resin` — inference). Consequence: on this target **select and power
are the same key**; short press = select, long press = power menu — the compositor must own
`KEY_POWER` (A2) and disambiguate by duration.

**Play For Dream MR (PFDM-D3).** Top button (hold 1.5 s on / 4 s off; short = camera quick
action) and a **Digital Dial** (rotary + push: short = Home, hold 1 s = recenter, rotate =
immersion or volume, hold during fit = IPD) [external: manual
https://cdn.shopify.com/s/files/1/0915/0647/5306/files/Play_For_Dream_MR_User_Manual.pdf §3–4];
reviewers report the dial press "acts as a push button for navigating menus" [external:
https://www.youtube.com/watch?v=IF3p_5M3QTM ]. Default navigation is hand + eye tracking. No
public kernel; dial encoding (`REL_DIAL`/`REL_WHEEL` vs key pair) unknown.

**PICO 4 Ultra** (reference). Power, volume ±, proximity. **Head Control Mode** when no
controller is connected: head crosshair, "click the Volume Up/Down button", **Vol− hold ≥1 s =
recenter**; PICO's own video maps Vol+ ≙ trigger, Vol− ≙ Home [external:
https://p16-platform-static-va.ibyteimg.com/tos-maliva-i-jo6vmmv194-us/pico4-ultra-user-guide-apac.pdf ,
https://www.youtube.com/watch?v=UcIOsjcmF74 ].

**Cross-vendor convention.** *Confirm* has two camps: Android-based platforms (Meta, PICO) reuse
the **volume keys** as click in their head-cursor fallback (Meta: either key; PICO: Vol+ ≙
trigger), and Google's Switch Access default recipe is Vol+ = Select, Vol− = Next [external:
https://developer.android.com/guide/topics/ui/accessibility/testing ]; Valve adds a **dedicated
button emitting `KEY_SELECT`**, and Android's `Generic.kl` maps Linux 353 → `DPAD_CENTER`
(activate) [external: https://android.googlesource.com/platform/frameworks/base.git/+/android-5.0.2_r1/data/keyboards/Generic.kl ]
— so both camps agree that **353 means activate**. *Back/cancel* has no HMD-button convention
(PICO: Vol− = Home; Lynx: R = menu; Meta/Samsung/PFD put Home on a button, none has "back").
*Recenter* is always a **long press** (PICO Vol− ≥1 s, PFD dial 1 s, Samsung touchpad hold).
*Power* long-press = power menu everywhere, short = sleep.

**The `KEY_SELECT > 255` consequence.** `KEY_OK 0x160`, `KEY_SELECT 0x161`
(`libinput/include/linux/linux/input-event-codes.h:424-425`); devices with a key in the
`KEY_OK..BTN_DPAD_UP` block get `ID_INPUT_KEY` (`systemd/src/udev/udev-builtin-input_id.c:34-36,
354-362`), so libinput delivers 353 as an ordinary keyboard key. But xkeyboard-config maps only
keycodes ≤255 ("Key codes below cannot be used in X … `= 361; // KEY_SELECT 353`" [external:
xkeyboard-config `keycodes/evdev`]), so GTK/Qt clients receive **no keysym** for it. A dedicated
select button should still emit `KEY_SELECT` at the device-tree level (Valve and Android agree),
and the compositor must consume raw evdev 353 for its own scenes; for ordinary clients either a
hwdb `KEYBOARD_KEY_<scancode>=enter` remap (`systemd/hwdb.d/60-keyboard.hwdb:60-70`) or a
compositor-side translation to Return is required. Quest 3S's `KEY_SWITCHVIDEOMODE` (227) *is*
≤255 and has a keysym, but its semantics are wrong for select.

**Recommended contract defaults** (inference where the vendor documents nothing; the user
adjudicates): deckard — `select = KEY_SELECT` (vendor-documented), back = Vol−, recenter = hold
select; eureka/monterey — `selectRole = volumeUp`, back = Vol− (Meta allows either key as click;
Vol+ = confirm keeps a two-key vocabulary and matches PICO and Switch Access), recenter = hold
Vol− (borrowed from PICO); SM-I610 — `select = KEY_POWER` short press (the Top button), back =
Vol−, recenter = touchpad hold if drivable else hold Vol−; PFDM-D3 — select = dial press
(vendor: Home), back = Top short, recenter = dial hold (vendor); Lynx R-1 — select = R
(vendor: menu), back = L, recenter = hold R — flagged: hand tracking is the vendor default there,
but pre-login has no cameras, so the buttons are the floor regardless.

**Gaps.** Lynx R-1 mainline DTS (L/R/volume codes) not fetched (Anubis-gated GitLab); Galaxy XR
volume/touchpad device nodes unknown without `evtest` on hardware; Play For Dream dial encoding
unknown; Steam Frame Aux hold/double-press semantics after login undocumented (Valve's Feature
Guide page is JS-only); Quest 3 double-tap thresholds closed.

### 4.4 Proximity / don-doff

Wear sensors arrive as proprietary HID fields (Rift S `rift_s_hmd.c:345`; PSVR2 `psvr2.c:308`;
libsurvive `survive_driver.c:669-692`), as IIO proximity on Qualcomm boards (Frame `vcnl4040`,
Lynx STK3X3X — [doc 07](07-device-landscape.md) line 78), or via the SSC (Galaxy XR fork
`galaxyxr_hmd_input.c:129-137`). Monado's hook is `XRT_INPUT_GENERIC_HEAD_DETECT` +
`xrt_device_supported.presence` — `monado/src/xrt/include/xrt/xrt_defines.h:955`, `xrt_device.h:323`,
surfaced as `XR_EXT_user_presence` — `oxr_session.c:765-786`; upstream only PSVR2 sets it
(`psvr2.c:1233, 1247`). No `SW_*` evdev switch is used by any pinned driver. Matches
[doc 12](12-lock-screens-and-appliance-login.md) lines 411-419 (per-headset presence unverified).

### 4.5 Head-aim pointer implementations and the repo's own constraint

None of the pinned XR implementations has dwell or magnetism; WayVR's head-pose lerp
(§2.3) and kwin-vr's 100 ms post-press inhibit are the only stabilisers. The repo already
specifies the stage they lack — composition constraint 7: "Gaze/ray input is stabilized before
it is arbitrated. A stabilization stage (deadzone + smoothing + dwell, with target magnetism as a
policy option, and event-time compensation …) sits between the pose source and hit arbitration"
([zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md) lines 306-313), and
[spatial-a11y.md](../architecture/spatial-a11y.md) lines 20-21 make that one stage the motor-access
path. A greeter operable by head-aim is therefore the *first consumer* of constraint 7, not a new
mechanism.

## 5. Pointing without hands: accessibility baselines [external + repo]

[Doc 37 §Motor](37-accessibility-atspi.md) already records visionOS Dwell Control, GNOME/KWin
dwell status, and Caribou's death. New here:

- **visionOS Pointer Control**: pointer = eyes, head, wrist, or index finger; adjustable
  sensitivity/size; triple-click Digital Crown shortcut —
  https://support.apple.com/guide/apple-vision-pro/use-a-pointer-to-navigate-tan3869c8a85/visionos .
- **Horizon OS v85 (2026) Voice Control**: hands-free 2D-panel interaction "using only voice
  commands and head movements", with a head-tracked cursor — https://www.meta.com/help/quest/172903867975450/ ;
  no switch access documented — https://developers.meta.com/horizon/design/accessibility/ .
- **Android XR**: TalkBack (Vol-Up+Vol-Down during setup), magnification; head aiming is only a
  developer option — https://support.google.com/android-xr/answer/16659361 ,
  https://developer.android.com/develop/xr/interaction-framework .
- **Linux**: GNOME Hover Click — https://help.gnome.org/gnome-help/a11y-dwellclick.html ; KWin
  dwell clicker merged Aug 2026 for Plasma 6.8 (motion threshold, click types via D-Bus, progress
  wheel) — https://invent.kde.org/plasma/kwin/-/merge_requests/9762 . Still no maintained Wayland
  switch-scanning tool.
- **Dwell timing**: ~600 ms generic, 400 ms for alphanumerics, 800 ms for icons, ≥1000 ms judged
  unusable — https://doi.org/10.1016/j.displa.2021.101997 ; dwell-typing literature 180–600 ms with
  adaptive shortening — https://www.iplab.cs.tsukuba.ac.jp/paper/journal/isomoto_etra2023.pdf ;
  Rajanna & Hansen used 550 ms in VR (§2.2).
- **Target size**: head pointing 2.04 bit/s vs gaze 1.85 vs mouse 2.75 at 300 ms dwell, effective
  width ≈2° — https://dl.acm.org/doi/10.1145/3206343.3206344 ; Meta guidance ≥2.5–3° angular
  collider, 12 mm spacing — https://developers.meta.com/horizon/design/styles_inputs_hit_targets/ ;
  WCAG 2.5.8 24×24 px (AA), 2.5.5 44×44 (AAA) — https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html .
  A larger-than-view keyboard *lowered* head-typing rate (9.15 vs 10.15 WPM) from neck strain (§2.2).

## 6. Out-of-band provisioning: the headset as a Linux host you reach from a device you hold

Ruled in scope with intent to adopt ([first-run-onboarding.md §5](../architecture/first-run-onboarding.md),
ADR 0017 rev 2 decision 7); this section supplies the mechanisms and the comparison the Phase 3
review decides on.

### 6.1 Transports before any credential exists

- **USB Ethernet gadget + DHCP + sshd (postmarketOS).** configfs gadget from the initramfs —
  `pmaports/main/postmarketos-initramfs/init_functions.sh:12-15, 836-881`; `unudhcpd` hands the
  host `172.16.42.2`, device `172.16.42.1` — `init_functions.sh:911-963`, `pmbootstrap/pmb/config/__init__.py:322`;
  kept up after boot — `pmaports/main/postmarketos-base/postmarketos-base-openrc.post-install:3-16`;
  sshd on by default — `pmbootstrap/pmb/install/_install.py:463-470, 1341`. Authorisation = a
  build-time user password — `_install.py:275, 1324`.
- **Headset-hosted hotspot (NetworkManager AP mode).** `802-11-wireless.mode=ap` → supplicant
  `mode=2` — `networkmanager/src/core/supplicant/nm-supplicant-config.c:625-647`; key-mgmt `none`,
  `wpa-psk`, `sae` (FT variants excluded in AP mode) — `nm-supplicant-config.c:1011-1090`;
  `ipv4.method=shared` spawns dnsmasq with a fixed argument set (`--conf-file=/dev/null`,
  `--dhcp-range`, router/DNS options, optional `ANDROID_METERED`, `--conf-dir=/etc/NetworkManager/dnsmasq-shared.d`)
  — `src/core/dnsmasq/nm-dnsmasq-manager.c:140-230`, called from `src/core/devices/nm-device.c:13991-14023`;
  NAT via the firewall backend — `nm-device.c:13939-13940`. NM's `connectivity.uri/response` is
  portal *detection* on upstream links, not serving — `man/NetworkManager.conf.xml:1538-1568`,
  `src/core/nm-connectivity.c:399, 589-605`. Concurrent AP+STA depends on the chip (per-target
  fact, [doc 07](07-device-landscape.md)).
- **Bluetooth PAN/NAP**: §3.3 — IP over BT with the phone's own pairing dialog as ceremony.
- **BLE GATT provisioning** [external]: Improv Wi-Fi (open standard; BLE service
  `00467768-6228-2272-4663-277478268000` or serial; optional physical-authorisation step; returns a
  URL after joining) — https://www.improv-wifi.com/ble/ , https://www.improv-wifi.com/serial/ ;
  Google Fast Pair hands Wi-Fi to Matter commissioning; Apple WAC is MFi-only —
  https://developer.apple.com/documentation/technotes/tn3111-ios-wifi-api-overview . Requires a
  bespoke GATT service on the headset — recorded as the bespoke-surface alternative, not adopted.
- **Wi-Fi Easy Connect (DPP)** [external]: Android 10+ initiator-only; a display-only device can
  be an *Enrollee* showing a `DPP:` QR — but the phone cannot see a QR inside a headset (Steam
  Frame's through-the-lens trick notwithstanding) — https://source.android.com/docs/core/connect/wifi-easy-connect ;
  certified-device count remains small — https://doi.org/10.1007/s10207-025-00988-3 . WPS is
  deprecated (Android 9) — https://en.wikipedia.org/wiki/Wi-Fi_Protected_Access .

### 6.2 Captive-portal mechanics [external]

OS captive detection is a cleartext HTTP probe expecting a fixed reply: Android
`connectivitycheck.gstatic.com/generate_204` → 204; Apple `captive.apple.com/hotspot-detect.html`
→ "Success"; Windows `msftconnecttest.com/connecttest.txt`; Firefox `detectportal.firefox.com/success.txt`
— https://isc.sans.edu/diary/33172 . A redirect means "portal"; a timeout means "no internet".
RFC 8910 (DHCP option 114 / RA option 37) + RFC 8908 (HTTPS `application/captive+json` API with
`user-portal-url`, `venue-info-url`) are consumed by Android 11+ and iOS 14/15+, not by Windows or
ChromeOS — https://www.rfc-editor.org/rfc/rfc8908.html ,
https://developer.android.com/about/versions/11/features/captive-portal ,
https://github.com/hgot07/VenueInfoHandler/blob/main/OS-status.md . With no upstream, Android
shows "no internet access — stay connected?" and iOS "keep trying Wi-Fi / use cellular", neither
controllable by the portal — https://github.com/iiab/iiab/issues/1376 ,
https://developer.apple.com/forums/thread/685191 . Apple's CNA is a stripped WebKit sheet
(cookies destroyed on close, no storage, limited JS, "Done" only after a full navigation) —
https://github.com/msmitty12/Captive-Network-Portal-Behavior ; Android's portal WebView opens only
when the probe is *redirected* and rejects self-signed HTTPS — https://www.splashaccess.com/captive-portal-android/ .
Android's resolver gained `.local` mDNS in Android 12 (not 10/11) and Chromium browsers still
show `.local` failures in 2025 — https://www.esper.io/blog/android-dessert-bites-26-mdns-local-47912385 ,
https://stackoverflow.com/questions/79405699/ . Standard fallback: a raw IP or `http://neverssl.com`
— https://www.scivision.dev/android-captive-hotspot-connected-no-internet/ .

How the provisioning tools handle this (code): **wifi-connect** runs its own dnsmasq with a
wildcard `--address=/#/<gateway>` — `wifi-connect/src/dnsmasq.rs:9-19` — and an `iron`
middleware that 302-redirects any `Host` ≠ gateway — `src/server.rs:114-131`; hotspot with
optional passphrase — `src/network.rs:456`; handoff: stop portal, connect STA, wait ≤20 s, else
recreate the portal — `src/network.rs:215-269`. **comitup** sets `address=/#/10.41.0.1` and DHCP
option 160 (captive-portal URI) — `comitup/conf/dns-hotspot.conf:1-6`; hotspot over NM D-Bus
(`mode: ap`, `10.41.0.1/24`, optional PSK) — `comitup/comitup/nm.py:265-299`; candidates = saved
infrastructure connections — `nm.py:242-254`; HOTSPOT→CONNECTING→CONNECTED state machine —
`comitup/comitup/states.py:106-160, 178-228`; Flask `/`, `/confirm`, `/connect` —
`comitup/comitup_web/comitupweb.py:97-129`. Neither special-cases the probe URLs; both rely on
wildcard DNS + redirect, and neither uses DHCP option 114.

### 6.3 The web tool — scored comparison

Task list scored: join Wi-Fi (PSK; enterprise), set own password, locale/timezone, hostname,
see SSH status; PAM/Unix login; privilege model; footprint; portal-tab friendliness; NM-native;
nixpkgs/NixOS module; maintenance.

| Tool | Wi-Fi join (PSK) | Own password | Locale / TZ / hostname | Login | Privilege model | Portal-tab friendly | NM-native | nixpkgs / module | Notes |
|---|---|---|---|---|---|---|---|---|---|
| **Cockpit** | **Yes** — scans (`RequestScan`), lists APs, `WiFiConnectDialog` creates an `802-11-wireless` infrastructure connection with `wpa-psk` and activates it — `cockpit/pkg/networkmanager/network-interface.jsx:84-143`, `pkg/networkmanager/interfaces.js:1157-1218, 810-821` | Yes — drives `passwd` in a pty, so a blank-password user gets no old-password prompt on NixOS — `cockpit/pkg/users/password-dialogs.js:19-48` | TZ via `timedate1` — `pkg/lib/serverTime.js:52, 128-132`; hostname (superuser) — `pkg/systemd/overview-cards/configurationCard.jsx:33-43`; **no locale page** | PAM service `cockpit` in `cockpit-session` — `cockpit/src/session/session.c:416-425` | polkit/sudo per action (`superuser: "require"`) — `pkg/users/shell-dialog.js:68` | Full SPA; heavy for a CNA sheet — target a normal tab | Yes (NM D-Bus) | `services.cockpit.{enable,package,plugins,settings,port,openFirewall}` — `nixos/modules/services/monitoring/cockpit.nix:51-106`; PAM `startSession` — `cockpit.nix:148-150`; nixpkgs builds with `--with-admin-group=root` ("TODO: really? Maybe wheel?") — `pkgs/by-name/co/cockpit/package.nix:176-188` (machine channel nixos-26.05) | General admin console (terminal, services, storage, users). My recollection that its Wi-Fi was display-only was **wrong** — the dialog exists |
| balena wifi-connect | Yes (hotspot → portal → connect) | No | No | none (open portal) | runs as root; NM D-Bus | Designed for the portal | Yes | **not in nixpkgs** | Rust, provisioning-only; exits after connecting |
| comitup | Yes | No | No | none (optional hotspot PSK) | root Python + Flask | Designed for the portal; DHCP opt 160 | Yes | **not in nixpkgs** | Debian-centric; "appliance mode" NAT — `comitup/conf/comitup.conf:47-54` |
| RaspAP | Yes (router semantics) | No | No | its own web login | `www-data` sudo to `wpa_cli`/hostapd — `raspap/installers/raspap.sudoers:10-17` | No | **No** — hostapd/dnsmasq/dhcpcd directly — `raspap/installers/common.sh:23-63` | not in nixpkgs | PHP router admin UI, not provisioning |
| OpenWrt LuCI | — | — | — | — | ubus/rpcd/uci — `luci/modules/luci-base/htdocs/luci-static/resources/rpc.js:7-16`, `uci.js:25-45` | — | No (netifd) | — | Not portable off OpenWrt; mechanism reference only |

Reading. Cockpit already covers every provisioning task except locale, authenticates through
PAM (so the passwordless-`mura` question in §6.5 applies identically), is a first-class NixOS
module, and is maintained by a large upstream. Its cost is weight and a UI built for a normal
browser tab — exactly the "portal as launcher" shape already adopted as the candidate design.
The purpose-built tools are lighter but bring their own hostapd/DNS stacks or are unpackaged.
Candidate for Phase 3: **Cockpit as the web surface, with a minimal static portal-launcher page
in front of it**; a purpose-built page only if Cockpit's Wi-Fi dialog fails on real hardware.

### 6.4 Authorising the hotspot — ruled

An open provisioning hotspot is the same trust class as SSH over the USB cable, **provided it
exists only while the device is unprovisioned** (no network profile configured *and* no password
set; afterwards an ordinary administrator setting, never automatic). PSK or numeric-comparison
(WiVRn's PIN pattern, §2.3) are recorded as hardening alternatives, not adopted. The in-headset
display problem (a phone cannot scan a QR shown inside the headset) rules out headset-shows-QR as
the *only* path; Steam Frame's through-the-lens sign-in shows it is not impossible.

**Review outcome (2026-09-24, after the security review of the Phase-4 wiring):** the open
hotspot was **superseded** — WPA2 with a per-boot random 8-digit PSK displayed inside the
headset (the wearer reads it and types it on the phone), plus a schema-declared idle timeout that
counts only while no client is associated. Reason: radio range is not the trust class of a cable,
and "unprovisioned" can persist indefinitely for an offline wearer; with the original sudo
wiring an open hotspot would have been one `sudo` from root, and even with standard sudo it
would have exposed a user shell as `mura`. Recorded in first-run-onboarding §5 / ADR 0017 rev 2.2.

### 6.5 The passwordless default user vs SSH and web login

`mura` ships with no password (ADR 0017 rev 2). Consequences and candidate resolutions (§3.5):

- greetd/zxr greeter: NixOS already sets `allowNullPassword` for greetd — works.
- sshd: `PermitEmptyPasswords no` by default → SSH login **fails**. Candidates: (a) a *default
  password* on the default image (the postmarketOS model, changed at first opportunity);
  (b) `Match Address 172.16.42.0/24` + `PermitEmptyPasswords yes` scoped to the USB link, plus
  `nullok` on the sshd PAM stack — treats "you plugged a cable in" as TTY-equivalent; (c) the web
  UI over the USB/hotspot link as the first thing that *sets* a password, SSH afterwards.
- Cockpit: PAM `cockpit` service; same three candidates apply (its `passwd` pty flow then sets
  the password without an old one).
- `sudo`: fails for a passwordless wheel user unless `wheelNeedsPassword = false`; `passwd` on
  one's own account works (password stack `nullok`). AccountsService `SetPassword` needs
  `auth_admin` even for one's own account by default — a Mura polkit rule would be needed to make
  a "set a password" welcome-surface item frictionless.
- A declared `hashedPasswordFile` user has none of these problems.

**Review outcome (2026-09-24):** candidate **(b) adopted for the USB-gadget subnet only** —
sshd global `PasswordAuthentication no`, a `Match Address` block for the gadget subnet with
`PasswordAuthentication yes` + `PermitEmptyPasswords yes`, `nullok` on the sshd stack; never over
radio. **(c) adopted** as the setup page's first item. **(a) rejected** (a default password
nobody chose). **`nullok` on sudo rejected** — `sudo -S <<< ""` from any session process would be
root; a passwordless account cannot administer until `passwd`, which asks no old password (the
Steam Deck / NixOS posture). **The polkit own-password rule rejected** — `allow_active=yes` on
`change-own-password` lets any session process set the wearer's password; `passwd` in a pty (the
Cockpit mechanism) needs no rule. Cockpit's socket bound to gadget + hotspot addresses. The
`numeric-credential` hint is mirrored for the greeter at `/run/mura/credential-hint/<user>`
(`0640 root:greeter`). Everything static — no mechanism detects the passwordless state.
Recorded in first-run-onboarding §5.3 / ADR 0017 rev 2.2 / multi-user §3.

**D2 correction (2026-09-24, measured in the VM test):** the `PermitEmptyPasswords` half of
(b) is withdrawn. OpenSSH's initial `none` method, with that option on, performs a real PAM
authenticate with an empty password in the parent `sshd-session`; the subsequent
keyboard-interactive/password attempt runs in a **forked** helper (nixpkgs' OpenSSH is not built
with `USE_POSIX_THREADS`), so the parent's PAM handle keeps the failed probe as its cached chain
and `pam_setcred` replays it — `Permission denied` with NixOS's `likeauth`, `Failure setting user
credentials` without. Net effect: every SSH password login fails as soon as the account *has* a
password, i.e. right after the wearer follows the design's own advice. So: the gadget-subnet
`Match Address` block keeps `PasswordAuthentication` + `KbdInteractiveAuthentication yes` only;
no `nullok` on sshd; a passwordless `mura` reaches the device over the cable through Cockpit
((c), PAM has no such probe) or through the session, and SSH follows `passwd` — or an
authorized key, the ordinary self-builder answer. The hint mirror is also gone: the hint is the
user's own file in a sticky `state/credential-hint/` directory, owner-checked by the greeter
(first-run rev 2.4, multi-user rev 3.4, ADR 0017 rev 2.3). Two more nixpkgs facts found on the
way: Linux-PAM's sysconfdir is inside the store, so `pam_faillock` needs `conf=` to see
`/etc/security/faillock.conf`; and OpenSSH ≥ 9.8's `PerSourcePenalties` throttles a source
address after failures independently of PAM.

### 6.6 Native companion app

Optional sugar over the same SSH/HTTP surfaces (KDE Connect / gnome-remote-desktop class); the
app-store dependency is the ethos cost. Nothing in the survey requires one.

## 7. Verdicts, candidates, open questions

### 7.1 Hypotheses

- **(i) IMU head-aim + HMD buttons as a universal floor — confirmed with one caveat.** Every
  relevant Monado driver keeps a 3DoF path (§4.1); every target has power + volume and most a
  third button, all reaching the compositor through libinput as ordinary key events once logind's
  power-key handling is set to ignore or inhibited (§4.3); PICO and Steam Frame ship exactly this
  mode as a documented feature, Quest as a hidden fallback (§1). Measured cost: ~10 WPM for
  head-aim + click (§2.2). Caveat: **no Monado driver yet exists for any Mura target's IMU**
  (IIO or SSC) — the floor is universal in principle, and new code per target in practice.
- **(ii) Declared config + USB HID + optional PIN make pre-login text entry rare — confirmed, and
  strengthened by §6.** USB keyboards work at the greeter with zero configuration (§3.4); Path-A
  declaration removes the need entirely; a 10-target PIN pad at head-aim speed is a few seconds;
  and out-of-band provisioning (§6) removes headset typing altogether for anyone with a phone or
  laptop.

### 7.2 Adopt / reject candidates (for the Phase 3 review — none adopted here)

| # | Candidate | Tag | Evidence |
|---|---|---|---|
| A1 | **Input floor = head-aim reticle + HMD buttons (+ dwell where a button is unusable); every pre-login and welcome scene fully operable at the floor** | [research] | §1 PICO/Frame/Quest; §4.1–4.3; §2.2 rates |
| A2 | **Buttons via libinput; `HandlePowerKey=ignore` or a session inhibitor** (Steam Deck's `powerbuttond` shape) rather than grabbing evdev in Monado (Galaxy XR fork shape) | [research] | §4.3 |
| A3 | **Constraint-7 stabiliser (deadzone/smoothing/dwell/magnetism) is the greeter's first consumer**; dwell 400–600 ms default, ≥2.5–3° targets | [research] | §4.5, §5 |
| A4 | **USB HID at the greeter: zero configuration, keyboard focus into the auth scene** | [research] | §3.4 |
| A5 | **BlueZ agent in the greeter scene with `NoInputNoOutput`/`DisplayYesNo` capability; `/var/lib/bluetooth` as a device-level state class** | [research]; class = [mine] | §3.3 |
| A6 | **OSK for pre-login scenes: in-compositor keyboard (gnome-shell shape), or virtual-keyboard-v1 + layer-shell to reuse wvkbd/squeekboard** | [research]; choice = decider G1 design | §3.1–3.2 |
| A7 | **Web surface = Cockpit behind a static portal-launcher page; minimal purpose-built page only on failure** | [mine, from §6.3 scores] | §6.3 |
| A8 | **Hotspot = NM AP mode + shared IPv4 with a `dnsmasq-shared.d` wildcard address + DHCP option 114**; portal page is the launcher | [research] | §6.1–6.2 |
| A9 | **Passwordless-`mura` resolution**: (a) default password, (b) USB-scoped `PermitEmptyPasswords`, (c) web-sets-password-first | [user decides] | §6.5 |
| R1 | Head-aim via Monado `qwerty`/evdev driver | reject | §4.2 — SDL-bound, disables other drivers |
| R2 | BLE GATT provisioning (Improv/Fast Pair) | reject for v1 (bespoke privileged surface) | §6.1 |
| R3 | Headset-shows-QR as the only phone path | reject | §6.4 |
| R4 | RaspAP / LuCI / wifi-connect / comitup as the tool | reject (hostapd stacks / unpackaged / provisioning-only) | §6.3 |

### 7.3 Open questions, each naming its decider

- **Welcome-surface contents** given A1/A4/A5 — decider: the Phase 3 review.
- **Pre-login/welcome input conformance requirement** wording — Phase 3 review (A1).
- **Passwordless default user** (A9) — the user.
- **Greeter Wi-Fi scope**: connections made pre-login are `permissions=user:greeter` unless a GDM-style
  polkit rule lifts the greeter to `settings.modify.system` (doc 11 addendum §D) — the user
  (discretionary policy).
- **Device-contract input facts** implied: per-target HMD buttons (codes, which is "select"),
  controller class (none / 3DoF-IMU / optical), Bluetooth presence, concurrent AP+STA support,
  proximity sensor source — decider: Phase 4 contract round.
- **Which Monado driver work is on the critical path**: an IIO/SSC 3DoF HMD driver per target
  precedes any in-headset greeter on that target — decider: implementation-path (scheduling
  lives there).
- **Paired-peripheral state class** (`/var/lib/bluetooth` survives factory reset?) — the user.

## Gaps

- No official Meta page documents the retail "wait 5–10 min → gaze mode"; timeout and Wi-Fi
  reachability in that mode are unverified. PICO's Head Control Mode is documented for the home
  screen; OOBE coverage unstated. Steam Frame Aux semantics beyond "navigate initial menus" are
  undocumented.
- No published WPM for head-pointer + single HMD button; nearest proxies in §2.2.
- Monado: no IIO driver upstream; Frame IMU location (hid-over-spi vs SSC) unverified; per-target
  `XR_EXT_user_presence` unverified except PSVR2 and the Galaxy XR fork. Quest 1/3 and Play For
  Dream gpio-keys codes assumed by analogy (no DTS dump in the repo).
- Linux-PAM and OpenSSH sources are not pinned; `nullok` and `PermitEmptyPasswords` are [external].
- Cockpit plugin bundling and the absence of wifi-connect/comitup/raspap were checked against the
  machine's nixos-26.05 channel, not the pinned nixpkgs revision (which lacks `pkgs/`).
- Plasma's Breeze SDDM theme QML is not in the pinned plasma-workspace (moved to
  `plasma-login-manager` [external]).
- G3k keyboard source not pinned. Improv Wi-Fi has no NM/iwd integration found; DPP enrollee
  support in NetworkManager not inspected.
- RFC 8908 support status rests on a 2023 third-party matrix plus 2020–21 vendor announcements;
  no 2026 change found.
