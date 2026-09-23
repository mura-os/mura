# 12 — Lock screens and the appliance login model

**Purpose:** validate the "lock screen as internal compositor policy state" hypothesis from
[zxr-shell-v2-composition.md](../architecture/zxr-shell-v2-composition.md) against how the Linux
ecosystem actually implements session locking (`ext-session-lock-v1`, swaylock, hyprlock,
kscreenlocker), define the PAM story for a headset locker, and define the appliance
(consumer-headset) login model from the Jovian/SteamOS and Quest/Android/visionOS precedents.
Feeds ADR 0007. Companion: research doc 11 covers the display-manager/greeter side proper
(greetd, SDDM internals, pre-session auth); this doc only situates the lock screen against it.

**Sources:** local clones under `references/` (`wayland-protocols`, `swaylock`, `hyprlock`,
`kscreenlocker`, `cage`, `jovian-nixos`) cited by file path; web sources cited inline as links and
marked **[external]**; anything not directly verified is marked **inferred** or **uncertain**.

---

## 1. Lock vs greeter vs display manager

A **lock screen** runs *inside* an existing session: the user is already authenticated, their
processes keep running, and the locker's only job is to (a) make the session's pixels and input
unreachable and (b) re-verify *that same user* before restoring access. A **greeter** runs *before*
any session exists: it authenticates *any* user and its output is a new logind session running that
user's chosen session command. A **display manager** is the supervisor that owns the greeter
lifecycle and session hand-off (doc 11's territory). The distinction matters mechanically, not just
conceptually: a locker calls `pam_authenticate` for the current uid and needs only the `auth` stack
(`references/kscreenlocker/README.pam` line 42: "KScreenLocker only uses the 'auth' entries"),
while a greeter must also open a session (`pam_open_session`, `pam_setcred`, logind registration).
On an appliance that auto-logs-in a single owner, the greeter degenerates to nothing — and the lock
screen becomes the *only* authentication UI the user ever sees, which is exactly the Quest/visionOS
model (§5).

## 2. `ext-session-lock-v1` in depth

Source: `references/wayland-protocols/staging/ext-session-lock/ext-session-lock-v1.xml`
(version 1, copyright 2021 Isaac Freund; still "staging/testing phase" per the XML preamble).
The protocol's own summary: "allows for a privileged Wayland client to lock the session and
display arbitrary graphics while the session is locked." Whether the protocol is restricted to a
compositor-launched client or exposed to all privileged clients is explicitly compositor policy
(XML lines 29–31). The client performs authentication and tells the compositor when to unlock.

### 2.1 The wire contract

**`ext_session_lock_manager_v1`** (the global):
- `destroy` — destructor; existing objects remain valid.
- `lock(id: new ext_session_lock_v1)` — asks the compositor to lock. The compositor **must**
  answer with exactly one of `locked` or `finished` on the new object.

**`ext_session_lock_v1`** (one lock attempt):
- event `locked` — the session is now locked; the client is responsible for lock graphics and for
  deciding when to unlock. May only be sent once the hard requirement below is met.
- event `finished` — the compositor refuses or terminates the lock (e.g. another lock already
  held, or the compositor "implements some alternative, secure way to authenticate and unlock the
  session" — XML lines 172–176; this clause is load-bearing for Mura, see §2.4). May be sent
  immediately on creation, or later even after `locked` (compositor policy).
- `get_lock_surface(id, surface: wl_surface, output: wl_output)` — gives a `wl_surface` the lock-
  surface role **for one specific output**. Errors: `role` (surface already has a role),
  `already_constructed` (buffer attached/committed before first configure), `duplicate_output`
  (second lock surface on the same output).
- `unlock_and_destroy` — destructor; unlocks (protocol error unless `locked` was received; before
  that, plain `destroy` must be used — errors `invalid_destroy`/`invalid_unlock`). The XML
  (lines 229–235) warns that a client exiting right after unlocking must roundtrip first because
  the request is asynchronous — swaylock does exactly this (`references/swaylock/main.c`
  lines 1269–1270: `unlock_and_destroy` then `wl_display_roundtrip`).

**`ext_session_lock_surface_v1`** (per-output lock surface):
- event `configure(serial, width, height)` — sent immediately on binding and on output geometry
  changes; width/height are **exact requirements**, mismatched commits are a protocol error
  (`dimensions_mismatch`).
- `ack_configure(serial)` — must precede the commit that responds to a configure; standard
  serial-consuming semantics (only the last ack before a commit counts; re-acking an old or
  already-consumed serial is `invalid_serial`).
- `destroy` — if a lock surface on an *active* output is destroyed before unlock, "the compositor
  must fall back to rendering a solid color" (XML lines 278–280).
- Errors: `commit_before_first_ack`, `null_buffer` (committing a null buffer is always an error —
  a lock surface can never be unmapped, only destroyed).

### 2.2 The compositor's obligations (the security guarantee)

From the `ext_session_lock_v1` description (XML lines 74–121), the normative core:

1. **On lock: normal clients cease to exist, visually and for input.** "The compositor must stop
   rendering and providing input to normal clients. Instead the compositor must blank all outputs
   with an opaque color such that their normal content is fully hidden." Only lock surfaces and, at
   the compositor's discretion, privileged surfaces (input methods, shell UI) may render.
2. **`locked` is a *presentation* fact, not a state-machine fact.** It "must not be sent until a
   new 'locked' frame ... has been presented on **all outputs** and no security sensitive
   normal/unlocked content is possibly visible." Rationale spelled out in the XML: a client that
   suspends the system after receiving `locked` would otherwise race the first locked frame, and
   unlocked content could flash on resume. The compositor may wait for the client to map lock
   surfaces (avoiding a blank flash) but must impose a time limit and blank+send `locked` anyway.
3. **Locker crash ≠ unlock.** "If the client dies while the session is locked, the compositor must
   not unlock the session in response. It is acceptable for the session to be permanently locked."
   The compositor may keep displaying the dead client's last-mapped lock surfaces or fall back to
   a solid color — policy. Recovery is also policy: it may accept a new `ext_session_lock_v1`
   from a fresh client, or itself auto-restart a locker instance (XML lines 111–121). This is the
   design's whole point — sway adopted it precisely because pre-lock-protocol swaylock could crash
   or be bypassed by input races
   ([sway PR #6879](https://github.com/swaywm/sway/pull/6879) **[external]**); the cost is that a
   compositor with a buggy recovery path can wedge into a permanently-blank session
   ([niri issue #2986](https://github.com/YaLTeR/niri/issues/2986) **[external]**).

### 2.3 How a locker client actually behaves (swaylock, hyprlock)

swaylock (`references/swaylock/main.c`):
- Hard-fails if `ext_session_lock_manager_v1` is missing (lines 1206–1209); requests the lock
  *before* creating surfaces (`lock()` + roundtrip, lines 1211–1218), then creates one lock
  surface per known `wl_output` (lines 1223–1226) and for hotplugged outputs later
  (`handle_wl_output_done` → `create_surface`, lines 201–206) — per protocol advice, so the
  compositor can show real lock graphics instead of a blank timeout frame.
- `configure` handler: store size, `ack_configure`, render (lines 161–170). Rendering is cairo
  into `wl_shm` pool buffers — a locker does not need GPU access.
- Blocks until the `locked` event (lines 1228–1233), and only then reports readiness: writes to
  `--ready-fd` and/or daemonizes (lines 1235–1245). This ordering makes
  `swayidle -w timeout ... 'swaylock -f'` and lock-before-suspend sequencing race-free
  ([swayidle(1)](https://man.archlinux.org/man/swayidle.1) **[external]**): nothing downstream
  (like `systemctl suspend`) proceeds until the compositor has *presented* locked frames.
- On PAM success: `unlock_and_destroy`, roundtrip, exit 0 (lines 1269–1270); on `finished`: log
  "is another lockscreen running?" and exit 2 (lines 243–247).

hyprlock (`references/hyprlock/src/core/LockSurface.cpp`) is the same protocol dance with a GPU
renderer: it additionally binds `wp_fractional_scale_v1` + `wp_viewport` to render at fractional
scales (lines 24–48), acks configure and re-renders on each geometry change (lines 53–80), and
draws with EGL. Both demonstrate the key division of labor: **the compositor owns the guarantee
(blanking, input isolation, crash policy); the locker owns only pixels and the auth decision.**

### 2.4 Mapping to Mura: lock as internal compositor policy state

The composition model (zxr-shell-v2-composition.md §7) makes the Mura compositor the single
OpenXR client that composites *everything* and submits one stereo projection layer to Monado. The
`ext-session-lock-v1` obligations translate almost verbatim into an internal state machine:

- "stop rendering normal clients" → **stop sampling client colour/depth buffers** in the
  composition pass; compose only a compositor-owned lock scene (environment + PIN pad).
- "stop providing input" → route the pointer-ray/keyboard/controller seat exclusively to the lock
  scene's UI; send `wl_keyboard.leave`/`wl_pointer.leave` to whatever had focus.
- "blank all outputs with an opaque color" → the lock scene *is* the opaque replacement; there is
  exactly one "output" (the composed stereo target), so the multi-output choreography collapses.
- "locked only after a locked frame presented on all outputs" → do not report "locked" (on D-Bus /
  logind, or to a suspend sequencer) until a composition pass containing **zero client samples**
  has been submitted via `xrEndFrame`. The suspend race the XML describes applies to us
  identically: lock-then-suspend (e.g. doff → lock → panel off) must order on the presented frame,
  not on the state flip.
- crash policy → there is no locker process to crash. The failure mode shifts to "compositor
  crashed," which on the appliance means the whole session restarts into the boot-locked state
  (§5.4) — strictly safer than the protocol's permanently-blank fallback.

**Do we need the protocol at all?** For the appliance default, no. The protocol exists to let a
*separate, unprivileged-ish client* provide lock graphics while the *compositor* enforces the
guarantee. When the compositor is also the locker, the XML itself blesses the internalization: the
`finished` event exists partly for compositors that implement "some alternative, secure way to
authenticate and unlock the session" (XML lines 174–176). Internal lock state is therefore not a
protocol violation — it is the anticipated degenerate case. What must be preserved is the
*semantics*, and they should be written into the compositor spec as invariants: (I1) no client
buffer is sampled and no input reaches a client while `locked`; (I2) "locked" is only externally
reported after a client-free frame has been submitted; (I3) unlock happens only via successful PAM
conversation (§4) or an explicitly configured policy (grace resume, §6).

**When to expose `ext-session-lock-v1` anyway:**
1. **Desktop/dev profile.** ADR 0006 keeps a windowed desktop mode and packages alternative
   sessions; there, users will run `swayidle`+`swaylock`/`hyprlock` and expect the standard
   protocol. Supporting it is cheap once the internal state machine exists: a locker's per-output
   lock surface maps to a fullscreen quad composed *as* the lock scene (our single logical output
   sends one `configure` with the panel-or-window resolution).
2. **Third-party lock experiences on the headset.** The protocol's shape is 2D-per-output; a
   plausible design is to compose a locker's single lock surface as a head-locked quad over a
   compositor-owned void — exactly how an unmodified swaylock would look in-headset, PIN typed via
   ray + virtual keyboard. A truly 3D lock scene would want the zxr-shell tier instead. Open
   question (§8).
3. **Crash-recovery parity.** If a third-party locker dies, follow the protocol: keep the last
   frame or fall back to the built-in lock scene (better than a solid color — we always have one),
   never unlock.

**logind integration is orthogonal and required either way**: kscreenlocker listens for the logind
session's `Lock`/`Unlock` signals, calls `SetLockedHint`, and locks on `PrepareForSleep`
(`references/kscreenlocker/ksldapp.cpp` lines 228–253, 350–351, wired in `logind.cpp`). The
Mura compositor should do the same directly: `loginctl lock-session` and
lock-before-suspend then work with zero extra components.

## 3. kscreenlocker: the locker-as-separate-process alternative

KDE's architecture inverts the ext-session-lock split. The lock *authority* lives in the session
(`KSldApp`, linked into kwin/ksmserver), and the lock *UI* is a spawned greeter process
(`kscreenlocker_greet`) that embeds QML themes and does PAM itself.

Mechanics, all from `references/kscreenlocker/`:
- **Spawn + privileged channel:** `KSldApp::startLockProcess()` launches the greeter binary with
  `--immediateLock`/`--graceTime`/`--nolock` args and, crucially, passes a duplicated compositor
  socket as `WAYLAND_SOCKET` (`ksldapp.cpp` lines 383–423, `setWaylandFd` lines 377–381) — the
  greeter connects to kwin over a *dedicated, pre-authenticated* connection rather than the public
  socket. The greeter's windows use layer-shell, top layer, exclusive keyboard interactivity
  (`greeter/greeterapp.cpp` lines 405–408), not ext-session-lock.
- **Unlock signal = process exit (or stdout).** The greeter authenticates via PAM in a worker
  (`greeter/pamauthenticator.cpp`, `greeter/worker/`) and then either prints `Unlocked\n` or exits
  0; `KSldApp` watches both (`ksldapp.cpp` lines 162–167, 183–197) and only then releases the lock
  (`doUnlock`).
- **Crash policy = supervised restart, then emergency mode.** Abnormal greeter exit does *not*
  unlock: KSldApp restarts it up to 4 times, forcing Qt software rendering after the first crash
  (suspected GPU-driver crashes), and finally maps a compositor-side `EmergencyWindow`
  (`ksldapp.cpp` lines 199–210, `emergencywindow.cpp`). This is the same "crash must not unlock"
  invariant as ext-session-lock, implemented as daemon supervision instead of protocol law.
- **Triggers integrated in the authority, not the UI:** global shortcut, KIdleTime idle timeout
  with inhibition checks, logind `Lock`/`Unlock`/`PrepareForSleep`, lock-on-start
  (`ksldapp.cpp` lines 110–156, 228–285). Grace time (`m_lockGrace`) allows unlock-without-auth
  shortly after an idle-triggered lock (`userActivity`/`isGraceTime`, lines 425–459) — a policy
  knob Mura wants for doff/don (§6).

**Tradeoffs vs internal state (and vs ext-session-lock):**
- *For separate process:* PAM, QML themes, and a whole UI toolkit stay out of the compositor
  address space (a compositor that `pam_authenticate`s in-process pulls libpam + modules into the
  most security-critical process); a wedged/leaking greeter is restartable without killing the
  session; themes are swappable.
- *Against:* the compositor must special-case the greeter (privileged socket, focus grab,
  restart supervision, emergency fallback) — kwin carries a bespoke trust channel that
  ext-session-lock standardized away; the unlock signal ("process printed a line / exited 0") is
  far weaker typed than `unlock_and_destroy`; four restart attempts of a crashing GPU greeter is
  exactly the kind of machinery a headset compositor shouldn't improvise around its render loop.
- *Middle path (what Mura should copy):* keep the **authority** internal (compositor owns
  lock state, triggers, logind), but push the **PAM conversation** out of process (§4.4) — the
  kscreenlocker split at the auth boundary rather than the UI boundary. The lock *scene* stays
  compositor-owned because on a headset the lock scene must render through the same
  Monado/distortion path as everything else; a separate GPU-rendering greeter process would be a
  second XR client for no benefit.

## 4. PAM integration

### 4.1 What a locker does with PAM (vs a greeter)

A locker re-authenticates the *current* user: it resolves the username from its own uid
(`getpwuid(getuid())` — `references/swaylock/pam.c` lines 86–92; hyprlock's
`getUsernameForCurrentUid()` — `references/hyprlock/src/auth/Pam.cpp` line 88), starts a service
(`pam_start("swaylock", username, &conv, &handle)`), and loops `pam_authenticate` per attempt. No
`pam_open_session`, no `pam_acct_mgmt` in swaylock; only the `auth` stack is consulted (confirmed
by `references/kscreenlocker/README.pam` lines 42–44). swaylock additionally calls
`pam_setcred(PAM_REFRESH_CRED)` after success (`pam.c` line 140) to refresh e.g. Kerberos-style
credentials. A greeter, by contrast, authenticates an *arbitrary* username and must run
`acct`/`session` stacks and register with logind — doc 11's scope.

### 4.2 The conversation function is the whole interface

PAM drives the UI, not vice versa: the module stack calls back into the client with an array of
messages, and the client must answer `PAM_PROMPT_ECHO_OFF`/`ECHO_ON` prompts with text responses.
Both local implementations are instructive:
- swaylock's conversation just hands over the one password it was given and *aborts* if asked
  twice (working around `pam_systemd_home`'s internal retries — `references/swaylock/pam.c`
  lines 42–57).
- hyprlock runs PAM on a dedicated thread whose conversation **blocks** (condition variable) until
  the UI thread submits input, and it surfaces `PAM_TEXT_INFO`/`PAM_ERROR_MSG` texts to the UI —
  including sniffing pam_faillock's "N left to unlock" message and pam_fprintd's "Place your
  finger" prompt (`references/hyprlock/src/auth/Pam.cpp` lines 18–77, 158–178). It also treats a
  wrong-but-repeated prompt as "same question, same answer" (Fedora `su` asks twice, lines 43–50).
- swaylock does **privilege/address-space separation**: it forks a comm child *before* locking
  (`initialize_pw_backend`, `pam.c` lines 13–23) and ships passwords over a pipe; the Wayland/UI
  process never links the PAM conversation, and a locked password buffer (`password-buffer.c`)
  holds the secret. This is the pattern to copy in-compositor (§4.4).

### 4.3 NixOS `security.pam.services.<name>`

On NixOS `/etc/pam.d/` is generated; a locker's service must be *declared* or `pam_start` will hit
a nonexistent service and deny. The canonical incantation is literally an empty attrset —
`security.pam.services.swaylock = {};` — which generates a sane default stack
([NixOS Discourse: "Swaylock won't unlock"](https://discourse.nixos.org/t/swaylock-wont-unlock/27275)
**[external]**). Upstream packages ship one-line configs (`references/swaylock/pam/swaylock`,
`references/hyprlock/pam/hyprlock`: `auth include login`), but on NixOS the module system replaces
them. Relevant options **[external, NixOS]**:
- `security.pam.services.<name>.unixAuth` / `.fprintAuth` — toggle pam_unix / pam_fprintd lines;
  `services.fprintd.enable = true` flips `fprintAuth` on for most services
  ([NixOS wiki: Fingerprint scanner](https://wiki.nixos.org/wiki/Fingerprint_scanner)).
- Ordering is a real footgun: NixOS places fprintd *before* pam_unix, so password entry waits on a
  fingerprint timeout; the (experimental) fix is
  `security.pam.services.<name>.rules.auth.fprintd.order = ...unix.order + 50`
  ([Discourse: fprint ordering](https://discourse.nixos.org/t/problems-loging-in-with-password-when-fprint-is-enabled/65900),
  [design preview: PAM rule ordering](https://discourse.nixos.org/t/design-preview-pam-rule-ordering-and-targets/66399)).
- Full-text override (`.text = ''...''`) remains the escape hatch for exotic stacks.

For Mura: define a first-class service, e.g. `security.pam.services.mura-lock`, owned by
the `mura.xr.shell` module, defaulting to `unixAuth` plus the PIN module (§4.4); and a separate
`mura-greeter` service only if/when a multi-user greeter exists (doc 11). Keeping the locker's
service name stable is what lets users add fprintd/u2f declaratively.

### 4.4 PIN / pattern / no-keyboard conversation design

A controller-ray (or gaze+pinch) PIN pad changes the *client side* of the conversation only. PAM
neither knows nor cares that the response text came from ray-hits on floating digits: the locker
collects digits into a buffer and submits the string as the `PAM_PROMPT_ECHO_OFF` response.
Concrete design constraints, derived from the sources above:

1. **Run the conversation out of the render loop.** hyprlock's blocking-thread model or swaylock's
   forked-child model; for Mura prefer swaylock's **separate process** (a tiny
   `mura-authd` helper spawned at lock time, socketpair protocol: `{prompt, echo flag} →
   {response}` plus one-way info/error texts). The compositor never links libpam; module crashes
   and multi-second hangs (fprintd timeout, network modules) can't stall `xrWaitFrame`. This is
   the kscreenlocker split applied at the auth boundary (§3).
2. **Render generic prompts, not just the PIN fast path.** The stack may inject arbitrary prompts
   (OTP, faillock lockout notices, homed passphrase). The lock scene therefore needs: a text
   prompt panel, a digit pad (fast path), and a fallback full virtual keyboard reachable via ray —
   the same requirement hyprlock solves by surfacing `PAM_TEXT_INFO` strings verbatim.
3. **PIN ≠ new auth database.** Two sane options: (a) the owner's account password *is* a numeric
   PIN (OOBE creates it; pam_unix verifies it; zero new code) — the Quest model, where the
   passcode is 4–16 digits and device-local
   ([Meta Quest passcode help](https://www.meta.com/en-gb/help/quest/1198803198189099/)
   **[external]**); or (b) a dedicated `pam_mura_pin.so` verifying an argon2-hashed PIN stored
   in per-unit system state (`/var/lib/mura/`, beside calibration — *not* $HOME), so the PIN
   unlocks the device while the account password stays strong, matching Android/visionOS
   layering. **Recommended: (b)**, with (a) as the MVP.
4. **Pattern unlock is just a PIN encoding** (sequence of cell indices submitted as text);
   Quest ships pattern and PIN equivalently **[external, same sources]**. No PAM impact.
5. **Rate limiting belongs in PAM**, not the UI: `pam_faillock` in the `mura-lock` stack;
   surface its "N left" info messages in the scene (hyprlock precedent,
   `references/hyprlock/src/auth/Pam.cpp` lines 63–67). Consumer precedent for hard fallbacks:
   Optic ID allows five failed biometric attempts before forcing the passcode
   ([Apple: Optic ID](https://support.apple.com/en-us/118483) **[external]**); Quest's forgotten
   passcode ends in factory reset
   ([Meta: factory reset](https://www.meta.com/help/quest/149134797159340/) **[external]**) —
   i.e. on an appliance the final fallback is *recovery/wipe*, not a root shell.
6. **Biometrics come later via the same seam.** hyprlock's fingerprint impl talks to fprintd
   directly over D-Bus (`net.reactivated.Fprint`, `Claim`/`VerifyStatus` —
   `references/hyprlock/src/auth/Fingerprint.cpp` lines 13–15, 120, 206) *in parallel with* PAM
   rather than through pam_fprintd, because a D-Bus client can drive retry UX ("finger not
   centered") that a synchronous PAM module can't. An eventual iris/face unlock (visionOS Optic ID
   as design reference: on-device template, hard passcode fallbacks after restart/48h/5-failures
   **[external, Apple links above]**) should follow that pattern: a parallel unlock path beside
   PAM, never replacing it.

## 5. The appliance login model

### 5.1 Jovian / SteamOS: auto-login without a visible greeter

Jovian's `jovian.steam.autoStart` is the working precedent for "boot straight into the session"
on NixOS (`references/jovian-nixos/modules/steam/autostart.nix`):

- It does **not** delete the display manager — it *hides* it. SDDM is enabled with
  `autoLogin { enable = true; user = cfg.user; }`, `autoLogin.relogin = true` (re-login after
  logout, so exiting the session loops back into it), `wayland.enable`, and
  `defaultSession = "gamescope-wayland"` (lines 76–89). The comment is explicit: "Valve uses the
  default X11 greeter, but ideally you'd never see it anyway" (line 84). The option docs assert
  the converse constraint: "Traditional Display Managers cannot be enabled in conjunction with
  this option" (lines 20–24) — autologin *owns* the DM slot.
- The session itself is a `wayland-sessions` desktop entry (`gamescope-wayland.desktop`) that
  execs `start-gamescope-session`, which hands off to a **systemd user unit tree**
  (`gamescope-session.service`/`.target`, plus `steam-launcher.service`, mangoapp, ibus —
  `references/jovian-nixos/pkgs/gamescope-session/default.nix` lines 160–179). Session = user
  units, not a monolithic script: crash/restart policy and environment live in systemd.
- **Session switching without a greeter:** SteamOS's "Switch to Desktop" is `steamos-manager`
  writing SDDM autologin drop-ins under `/etc/sddm.conf.d/` (Jovian patches the path —
  `references/jovian-nixos/pkgs/steamos-manager/fix-sddm-config-path.patch`, legacy name
  `zz-steamos-autologin.conf`), then bouncing the DM, which auto-logs into the *other* session;
  the desktop session is chosen via `steamosctl set-default-desktop-session` (autostart.nix
  lines 99–109 force it back to the NixOS config's choice).
- **Failsafe:** `display-manager.service` gets
  `ExecStartPre = rm -f /etc/sddm.conf.d/zzt-steamos-temp-login.conf` (line 92) — the stale
  one-shot autologin override is scrubbed on every DM start, "replicat[ing the] vendor failsafe
  in case the system is rebooted with a broken config." Generalizable lesson: any mutable
  "next boot, do X instead" state must be self-clearing.

### 5.2 The consumer-headset precedent (all **[external]**, some community-sourced)

- **Quest:** single owner account provisioned by phone app; the device boots into the owner's
  environment with no user-selection step; if a passcode is set, "your Meta Horizon profile on
  your headset will automatically lock when your headset goes to sleep or when you power on your
  headset" — i.e. **lock at boot and on sleep, not login at boot**
  ([Meta passcode help](https://www.meta.com/en-gb/help/quest/1198803198189099/)). Passcode is
  4–16 digits, device-local (not account-side —
  [MetaQuestSupport on reddit](https://www.reddit.com/r/OculusQuest/comments/unbfc4/is_the_pattern_password_based_on_account_or/),
  community); secondary profiles switch from the lock screen without knowing each other's
  passcodes; forgotten passcode → remote unlock via the phone app or factory reset.
- **Android Keyguard (the architecture underneath Quest):** the keyguard is not a separate session
  or process tree; it is a SystemUI component mediated by `KeyguardViewMediator`, which fields
  power-manager events (screen off, timeout), occlusion, and unlock verification, coordinating
  with window-manager policy
  ([KeyguardViewMediator.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/packages/SystemUI/src/com/android/systemui/keyguard/KeyguardViewMediator.java));
  `StatusBarKeyguardViewManager` manages the bouncer (credential UI). Android's lock is
  **shell-integrated policy state over an always-running user session** — the closest existing
  analogue to "lock as compositor policy state," proven at consumer scale.
- **Vision Pro:** Optic ID (iris) unlocks on don; passcode is the root credential with forced
  fallback after restart, 48h without unlock, or 5 biometric failures; biometric template in the
  Secure Enclave ([Apple](https://support.apple.com/en-us/118483),
  [Optic ID matching security](https://support.apple.com/guide/security/optic-id-matching-security-sec4518c1d57/1/web/1)).
  Design reference for the *ideal* headset unlock modality: don-to-unlock with a rarely-typed
  strong credential behind it.

### 5.3 What Mura should do

**Recommended appliance default:** no display manager UI, ever. Auto-login the owner account
directly into the zxr session and treat the compositor-integrated lock as the only auth surface:

- Session start: either greetd/agetty-style autologin or Jovian's headless-SDDM pattern — the
  mechanism choice belongs to doc 11; the *requirement* from this doc is: the session must come up
  with **lock state = locked** whenever a credential is enrolled (Quest's power-on lock), so the
  greeter-less boot is safe even though authentication happened "for free."
- Session structure: copy gamescope-session — a `wayland-sessions` entry that starts a systemd
  user target (`mura-session.target`) owning Monado (if user-slice), the compositor, and shell
  services, so restart/crash policy is systemd's.
- Multi-user: out of scope for the appliance default (Quest-style secondary profiles are a lock-
  screen feature, not a greeter feature — profile switch from the lock scene is the future hook).
- Dev/desktop profile keeps a normal DM/greeter (doc 11) and the ext-session-lock protocol path
  (§2.4) so standard tooling works.

### 5.4 Boot-time lock, concretely

Because auth is free at boot, the lock scene is the first interactive thing the user sees:
compositor starts → connects to Monado → composes the lock scene (needs only per-unit calibration
from system state, no $HOME secrets) → PIN entry → PAM (§4) → unlock reveals the (meanwhile
started) shell. This also cleanly covers "compositor crashed mid-session": systemd restarts the
session units and the compositor comes back up *locked* (I3 of §2.4), which is the internal-state
equivalent of ext-session-lock's crash guarantee.

## 6. HMD-specific triggers and policy

### 6.1 Doff/don (proximity) — the headset's lid-close

Detection: HMD proximity sensors surface in OpenXR as `XR_EXT_user_presence` —
`XrEventDataUserPresenceChangedEXT.isUserPresent`, queued on every change and once at session
start; systems without the sensor must report `supportsUserPresence = XR_FALSE`
([Khronos registry](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrEventDataUserPresenceChangedEXT.html)
**[external]**). Since the Mura compositor *is* the OpenXR client, presence events arrive
directly in its frame loop — no extra daemon, no D-Bus hop; doff/don feeds the same internal
policy state machine as idle and logind. (Monado's `XR_EXT_user_presence` coverage per target
headset is **unverified** — qualification-matrix item; fallbacks are the raw proximity evdev/IIO
device, or `XrSessionState` VISIBLE/FOCUSED transitions as a cruder proxy.)

### 6.2 Policy table (recommended defaults)

| Trigger | Immediate action | Re-auth required? |
|---|---|---|
| **Doff** (isUserPresent → false) | Blank/power down panels ≤2 s (power + OLED/LCD image-retention: Meta support advises short sleep timers against static-image retention — [Quest burn-in thread w/ Meta reply](https://www.reddit.com/r/Quest3/comments/1m4oh5p/boot_menu_burn_in_anything_to_be_done/) **[external, community]**); pause client frame scheduling; start grace timer | Not yet |
| **Don within grace window** (default ~30–60 s, configurable) | Wake panels, resume compositing | **No** — resume session (kscreenlocker's grace-time precedent, `ksldapp.cpp` lines 425–459) |
| **Don after grace window** | Wake into lock scene | **Yes** — PIN/biometric |
| **Idle timeout** (no input & no presence-derived activity for N min) | Dim → lock → panel off, staged like swayidle's timeout ladder **[external, swayidle(1)]** | Yes, after the lock stage |
| **Explicit lock** (user action, `loginctl lock-session`, D-Bus) | Lock immediately (locked frame before ack, §2.4-I2) | Yes |
| **Suspend/resume** (`PrepareForSleep`) | Lock *before* sleep, ordered on the presented locked frame (XML race rationale §2.2; kscreenlocker `lockOnResume`, `ksldapp.cpp` lines 243–253) | Yes |
| **Boot / session restart** | Start locked (§5.4) | Yes |
| **Grace-window resume with `requirePassword=false`-style config** | Resume | Policy knob, default off for enrolled-credential devices |

Rationale for the grace window: doffing a headset is *far* more frequent than closing a laptop lid
(adjusting fit, glancing at a phone, talking to someone). Quest itself only locks on sleep, not on
every doff **[external, §5.2]**; visionOS re-checks identity on every don but that requires
zero-friction iris auth. With PIN-only auth, doff→instant-lock would be hostile; doff→blank
immediately (privacy: bystanders can't peek at the panels anyway; power: mandatory) plus
lock-after-grace is the right default. Security-sensitive deployments set grace = 0.

### 6.3 Idle plumbing

Track idle **inside the compositor** (it owns the seat and presence events; "user activity may
include input events or a presence sensor... compositor-specific" —
[ext-idle-notify-v1](https://wayland.app/protocols/ext-idle-notify-v1) **[external]**), and:
- **Expose `ext-idle-notify-v1`** so external policy tools (swayidle equivalents, a future
  settings daemon) can observe idleness; head motion / presence counts as activity.
- **Honor `zwp_idle_inhibit_manager_v1`**: a fullscreen video player or an XR app in a cutscene
  legitimately blocks the dim/lock ladder. Cage shows the minimal compositor-side pattern —
  maintain the inhibitor list, feed `wlr_idle_notifier_v1_set_inhibited` on create/destroy
  (`references/cage/idle_inhibit_v1.c` lines 24–36, 50–69); like Cage, Mura can start with
  "any live inhibitor inhibits" and skip visibility filtering, since a headset shows one focused
  space. Note the protocol split ext-idle-notify makes: `get_idle_notification` respects
  inhibitors, `get_input_idle_notification` ignores them **[external, same source]** — the lock
  ladder should use the inhibitor-respecting form; a hard security lock timer (if configured)
  should use the input-only form.
- **Set logind `IdleHint`** from the same state so system-level suspend policy composes (swayidle
  `idlehint` precedent **[external, swayidle(1)]**; kscreenlocker's KIdleTime is the KDE-side
  equivalent, `ksldapp.cpp` lines 134–156 with the inhibition check).
- Seat handling during lock: route input exclusively to the lock scene; Cage's `seat.c` is the
  reference for the underlying wlroots seat mechanics (focus, `wlr_seat_*_notify_*` forwarding,
  touch→point mapping — e.g. lines 645–658 pointer enter/motion), which is the layer our
  "input only to locker" switch sits above.

### 6.4 What requires re-auth vs just resume — the rule

**Re-auth when identity continuity is broken or unknowable:** boot, resume-from-suspend, explicit
lock, doff beyond grace, idle beyond lock stage. **Resume when continuity is plausible:** doff
within grace, dim-only idle stage, compositor-internal mode switches (2D↔3D, passthrough toggle —
these never touch lock state). Presence-based *unlock* (don → unlocked without credential) must
never be default-on without a biometric — presence says "a head is here," not "the owner's head"
(the gap visionOS fills with Optic ID and Quest deliberately leaves open by locking only on
sleep).

## 7. Boot splash before Monado owns the panel

The problem: until Monado + the compositor own the display, nothing renders through the
distortion/IPD path, and the HMD panel is just a DRM/KMS display — usually rotated, high-DPI, and
viewed through lenses that expect barrel-pre-distorted per-eye content.

- **Plymouth** runs from the initrd on KMS; on UEFI it can draw on `simpledrm` (firmware
  framebuffer) before the real GPU driver loads — historically it deliberately *ignored* simpledrm
  and waited up to 8 s for the native driver (missing rotation/physical-size info), which Fedora
  42 fixed and made default
  ([Fedora change: PlymouthUseSimpledrm](https://www.fedoraproject.org/wiki/Changes/PlymouthUseSimpledrm)
  **[external]**); on NixOS the 8 s stall and the `plymouth.use-simpledrm` workaround are
  documented pain ([nixpkgs #266804](https://github.com/NixOS/nixpkgs/issues/266804),
  [NixOS wiki: Plymouth](https://wiki.nixos.org/wiki/Plymouth) **[external]**). None of this
  machinery knows about lenses: Plymouth would draw one upright logo across the full panel, which
  through HMD optics appears as two warped, chromatic half-images — technically "a splash,"
  visually broken. On many headsets the panel is also portrait-native (the `fbcon=rotate` hints in
  SteamOS's cmdline for Deck are the 2D analogue —
  `references/jovian-nixos/modules/steamos/boot.nix` lines 55–58), compounding the wrongness.
- **The static per-eye logo approach:** render the logo twice, once per eye half, pre-warped with
  the *per-unit* distortion/IPD calibration — which is exactly why calibration must live in system
  state readable at initrd/early-boot time, not in $HOME. This needs only a trivial KMS client (or
  a precomputed framebuffer image blitted by an initrd unit), no GPU driver beyond simpledrm.
  Precedent, **[external, community-sourced, uncertain]**: Quest's *modern* boot animation is not
  even static — a `vr_bootanimation` service renders a GLB 3D model live at boot
  ([teardown thread](https://www.reddit.com/r/MetaQuestVR/comments/1hn0bmy/fun_fact_the_boot_animation_is_not_a_prerecorded/)),
  i.e. Meta brings up enough of the VR display path *before* the OS shell to render per-eye;
  Quest's *recovery/boot menu* (pre-VR-stack), by contrast, is flat panel UI navigated by volume
  buttons — legible held-in-hands, wrong through lenses. That is the honest dichotomy: bring up
  real per-eye rendering early (expensive), accept a held-in-hands splash, or show nothing.
- **Recommendation:** appliance profile: **no Plymouth**. Boot with panels dark (or a power-LED
  heartbeat), optionally a static pre-distorted per-eye logo from system calibration as a
  stretch goal — its real value is "sign of life" during long boots/updates, and it must be
  fbcon/kmscon-suppressed regardless (`quiet`, no fbcon takeover — the LUKS-prompt-on-fbcon
  caveat Jovian notes at `boot.nix` lines 55–57 doesn't apply to a passwordless-unlock
  appliance). Dev/desktop profile: Plymouth as usual on the monitor, irrelevant to the HMD.
  Critically, the *lock/greeter* never needs pre-Monado rendering: §5.4's boot-locked compositor
  is the first legible UI, and everything before it is cosmetic.

## 8. Adopt / reject / open questions

**Adopt:**
1. **Lock = internal compositor policy state**, specified as the three invariants of §2.4
   (no client sampling/input while locked; "locked" externally reported only after a client-free
   frame is submitted; unlock only via PAM or explicit grace policy). This is validated by the
   protocol's own text (compositor-internal alternatives are anticipated via `finished`) and by
   Android's shell-integrated Keyguard at consumer scale.
2. **logind integration in the compositor**: `Lock`/`Unlock` signals, `SetLockedHint`,
   `PrepareForSleep`-ordered locking (kscreenlocker's trigger set, minus the process supervision).
3. **Out-of-process PAM conversation** (`mura-authd` over socketpair; swaylock's fork model /
   kscreenlocker's auth split), service `security.pam.services.mura-lock`, `pam_faillock`
   included, generic-prompt-capable lock UI with PIN-pad fast path (§4.4).
4. **Jovian's appliance session pattern**: autologin (mechanism per doc 11) + systemd user target
   as the session body + self-clearing one-shot boot overrides; **boot into locked state** when a
   credential is enrolled (Quest model).
5. **Doff/don via `XR_EXT_user_presence` in the compositor's OpenXR loop**; blank-on-doff
   immediately; grace-window resume; lock after grace/idle/suspend/boot (§6.2 table).
6. **`ext-idle-notify-v1` served + `zwp_idle_inhibit_v1` honored** (Cage's minimal pattern),
   idle ladder dim→lock→panel-off.
7. **No Plymouth on the appliance**; per-eye static pre-distorted splash only as a calibrated,
   system-state-driven stretch goal.

**Reject:**
1. **kscreenlocker's separate lock-UI process** (greeter-with-QML spawned per lock): the lock
   scene must render through the compositor's Monado path anyway; a second XR-rendering process
   buys isolation we take at the PAM boundary instead, at the cost of restart supervision and a
   bespoke trust channel.
2. **Mandatory `ext-session-lock-v1` for the built-in lock** — internal state is conformant in
   spirit and strictly simpler; the protocol is a *compatibility surface*, not the mechanism.
3. **A display manager UI on the appliance** — autologin owns the slot (Jovian precedent).
4. **Presence-based unlock without biometrics** — don ≠ owner.
5. **Full-panel undistorted boot splash** (Plymouth on the HMD panel) — visually broken through
   lenses; serves nobody.

**Open questions (for ADR 0007 or later):**
1. Should the desktop/dev profile's `ext-session-lock-v1` support extend to the headset session
   (third-party 2D lockers composed as a head-locked quad over a void)? Cheap once internal state
   exists, but it makes an external process the unlock authority — likely gate it behind the same
   privileged-client policy the XML anticipates, defaulting to internal-only on the appliance.
2. PIN storage: MVP as owner-password-is-PIN vs `pam_mura_pin.so` with argon2 hash in
   `/var/lib/mura/` — §4.4 recommends the module; decide enrollment UX (in-headset OOBE vs
   companion tool) alongside doc 11's first-boot story. **RESOLVED (2026-09-23):** option (b)
   ratified by [ADR 0017](../architecture/adr/0017-first-run-provisioning.md); the first-boot
   story is [first-run-onboarding.md](../architecture/first-run-onboarding.md) (in-headset OOBE
   via `zxr --oobe` + `mura-provisiond`; companion tool recorded as an open alternative).
3. Monado's actual `XR_EXT_user_presence` coverage per target device (qualification-matrix item);
   fallback path via raw proximity evdev/IIO.
4. Grace-window default (30 s? 60 s?) and whether "same head re-donned" heuristics may extend it —
   needs a privacy/security review before any such heuristic exists.
5. Multi-profile on the lock scene (Quest-style secondary users) — deferred; interacts with
   doc 11's session model and NixOS multi-user $HOME provisioning.
6. Should a Monado/runtime crash relock the session? Likely yes for consistency (any
   session-scope process crash relocks), but needs the §5.3 session unit-tree design first.
