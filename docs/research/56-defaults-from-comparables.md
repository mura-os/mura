# 56 — Defaults from comparables: the agent-chosen numbers and postures, re-derived

**Research date:** 2026-09-25. **Method:** AGENTS.md rules 7 and 8 — for every default, threshold and
posture an agent chose and tagged `[mine]` during the D-track (D2–D6), find the shipping projects
that solved the same problem for the same kind of user and device, state what each chose and
**why** (from its source, comments and commit messages), whether that reason transfers to Mura, and
what adopting it trades off; then either apply (converging evidence with reasons that transfer) or
put the item to the owner with the comparables' actual positions as the options. Citations are
`references/` file:line at the pinned revisions (MANIFEST.json); commit messages and anything not in
the pinned clones are marked **[external]** with the source. The pinned clones are shallow, so
history was read on the upstream forges.

**Budget impact** (overview invariant 9): none on the frame path; boot-time and radio-time effects
are the subject of items 3–6.

Items: 1 compositor restart limit · 2 readiness timeout · 3 boot tries and the crash-loop threshold ·
4 the blessing tier (stability window) · 5 device wait · 6 hotspot idle timeout · 7 faillock and the
user-run unlock · 8 helper hardening (dumpable) · 9 in-session time-zone/hostname authority ·
10 found on the way (perl on PATH).

## 1. Compositor restart limit — `StartLimitBurst=3` / `StartLimitIntervalSec=60s` (session.nix)

**Mura:** `mura-compositor.service` restarts in place on failure (ADR 0007: a crash returns the
wearer to a locked session, not to a greeter), `RestartMode=direct`, 3 restarts per 60 s, then the
session ends and the B1b ladder applies.

| Comparable | What | Why (stated) | Transfers? |
|---|---|---|---|
| Plasma `plasmashell` — `plasma-workspace/shell/plasma-plasmashell.service.in:5-6,10` | `Restart=on-failure`, **`StartLimitIntervalSec=60s` / `StartLimitBurst=3`** | commit 9149b81e (2022-12-12) **[external, invent.kde.org]**: *"`plasmashell` takes around ~4 seconds to start and crash. Default Systemd limit of 5 times in 10s is too much causing infinite crashing and restarting."* | **Yes.** Same mechanism (a restarting session component under the user manager), same failure (systemd's 5-in-10 s default cannot catch a component whose start-and-crash cycle exceeds 2 s). Our compositor with Monado behind it will take seconds to start and crash. |
| KWin `kwin_wayland_wrapper` — `kwin/src/helpers/wayland_wrapper/kwin_wrapper.cpp:10-18` | restarts kwin on any non-zero exit, keeps the Wayland socket, gives up after **10** crashes (no time window) | commit 0c6a8e7b **[external]**: a deliberate `--replace` resets the count *"so that you can run it more than 10 times without exiting to the logout screen"* — the limit exists to return the user to the login path on a crash storm; **the number 10 is not explained** | Shape transfers (restart the compositor in place, fall to the login path on a storm); the count does not carry a reason |
| GNOME Shell — `gnome-shell/data/org.gnome.Shell@.service.in` | **`Restart=no`**; `OnFailure=` disable-extensions + `gnome-session-shutdown.target` | unit comment: *"On wayland we cannot restart"* | No — GNOME cannot restart its compositor because clients die with it; Mura's design (ADR 0007) requires the restart, and Plasma shows `QT_WAYLAND_RECONNECT=1` (`startplasma.cpp:812-813`) as the client side of it |
| cosmic-comp (`Restart=never`), niri (no restart, `BindsTo=graphical-session.target`), uwsm (`Restart=no`, `OnFailure=` shutdown) | compositor death = session end | none stated | No (same camp as GNOME) |
| systemd default — `systemd/src/basic/constants.h:20-21` | 5 starts / 10 s | rate-limiting; the numbers are not justified | The generic default the Plasma commit found inadequate |

**Determination — applied (converging, reason transfers).** Among the comparables that restart a
session component in place, the one with a stated reason uses exactly 3/60 s and the reason is
Mura's; the other (KWin, 10) states no reason for its number. `[mine]` becomes *sourced*; the
values stay. Recorded in `specs/session-bootstrap.md §5` and `session.nix`.

## 2. Readiness timeout — `mura.xr.session.readinessTimeoutSeconds = 30` (`TimeoutStartSec`)

| Comparable | Value | Why |
|---|---|---|
| uwsm `wayland-wm@.service` (D4 evidence) | 30 s | none stated; README still says 10 s in one place |
| Plasma `plasmashell` `TimeoutSec=40sec` (`plasma-plasmashell.service.in:14`) | 40 s | present since the first systemd-startup unit; none stated |
| Weston docs example unit | 60 s (+ `WatchdogSec=20`) | example only |
| SteamOS `gamescope-session.service` (Jovian packaging **[external]**) | `TimeoutStartSec=5`, notify after a 3 s env handoff | none stated; a fast-starting fixed session |
| systemd default `DefaultTimeoutStartSec` — `systemd/meson_options.txt:212-215` | 90 s (system and user) | none stated |

**Determination — no reasoned comparable; keep as a measured bound.** Every value (5–90 s) is a
guess by its author. The honest derivation is measurement on the target: the VM's sway is ready in
~1 s (`specs/session-bootstrap.md §9`); zxr + Monado on the SoC is unknown. The 30 s default stays a
schema value with the existing decider (real-hardware measurement at G1); it is a bound against a
hang, not a policy, and it is not `[mine]`-tagged.

## 3. Boot tries and the crash-loop threshold — `bootTries=3`, `crashLoopThreshold=3`

| Comparable | Mechanism | Count | Why |
|---|---|---|---|
| systemd Automatic Boot Assessment — `systemd/docs/AUTOMATIC_BOOT_ASSESSMENT.md:82-87,103-107`; `src/boot/boot.c:1248-1254,1789-1790` | BLS `+N` counter; at 0 the entry is sorted last and the next entry boots | walkthrough uses **3** (`echo 3 >/etc/kernel/tries`) | **not explained** |
| RAUC — `rauc/docs/reference.rst:145-156`; `src/bootloaders/uboot.c:8`; `barebox.c:9` | `boot-attempts` reset on `mark-good` | **3** (U-Boot, Barebox defaults) | *"should match the bootloader's reset value"* — i.e. the bootloaders' convention, itself unexplained; GRUB example uses 1 because of GRUB scripting limits (`integration.rst`) |
| mkosi UKI example `[external, mkosi/docs/root-verity.md:195-196]` | `TriesLeft=3` | none |
| GDM — `gdm/daemon/gdm-local-display-factory.c:52,313` | greeter display failures before giving up | **5** (lifetime, no window) | `/* oh shit */` |
| SDDM `Display.cpp:56,163-171` | tty failures before exit | >5 | avoid infinite retry when the VT is stolen |
| Android Rescue Party | escalating reboots/resets | **[external, unverified]** — not asserted | — |

**Determination.** *`bootTries=3`* — converging convention across the whole A/B ecosystem
(systemd, RAUC, U-Boot, Barebox, mkosi), none explaining it; applied as the ecosystem's value, tag
becomes *convention*. *`crashLoopThreshold=3`* — the count mirrors that convention, but the
**mechanism** it counts (a userspace ladder that, after N hard preflight failures, boots into a
recovery target instead of relying on the bootloader's slot fallback) has **no in-tree comparable**:
systemd and RAUC let a failed boot simply not be blessed and let the bootloader fall to the other
slot; GDM/SDDM count *display* failures, not boots. The only shipping system with a userspace
"N bad boots → recovery" ladder is Android's Rescue Party, which is not in the pinned corpus. The
ladder's stated purpose at D6 (a fault common to both slots — missing calibration — must not
ping-pong between slots forever) is Mura's own reasoning. **→ Owner (§11 Q1):** keep the ladder
(rule 2: Android as engineering evidence, to be pulled into `references/` and cited) or drop to the
systemd/RAUC shape (no blessing → slot fallback only; recovery is the user's explicit choice).

## 4. The blessing tier — `readinessStabilitySeconds = 20`

**Mura:** `mura-readiness` blesses the boot (resets the counter, reaches `boot-complete.target`)
only after the compositor (appliance) or greeter (multi-user) has been continuously up for 20 s.

| Comparable | "Good boot" means | Stability window? | Why |
|---|---|---|---|
| systemd — `systemd/man/systemd.special.xml:168-181`; `docs/AUTOMATIC_BOOT_ASSESSMENT.md:36-41,136-158` | `boot-complete.target` **reached**; units that must succeed are ordered before it and `Requires=`d by it; `systemd-boot-check-no-failures.service` optionally blocks it when any unit failed | **No.** The doc's *illustrative* DE unit (`graphical-session-good.service`, "one minute after the user has logged in") is a fictional example, not a mechanism | success = a synchronisation point in the dependency graph |
| RAUC — `rauc/docs/integration.rst:1472-1491,1815-1820`; `reference.rst:2617-2626` | `rauc status mark-good` from a unit ordered after "all relevant other services came up", typically `WantedBy=multi-user.target` | **No** | *"wait for the system to be fully started"* — dependency ordering, not time |
| mobile-nixos — `modules/boot-control.nix:17-31` | Android `boot-control --mark-successful` oneshot at `multi-user.target` | **No** | delegates to the vendor slot logic |
| SteamOS A/B | not in the pinned clone (`jovian-nixos/support/manifest/mappings.toml` excludes the steamos-efi packages) | unknown | — |

**Determination — no comparable; rethink candidate → Owner (§11 Q2). Ruled 2026-09-25: (a), the
comparables' shape — `mura-readiness` succeeds when the compositor unit (or greetd + greeter) is
active and `/persist` is writable; `readinessStabilitySeconds` removed.** Every shipping system
blesses on *a target reached*, never on *N seconds of stability*. The comparables' shape for Mura
is: `mura-readiness` succeeds the moment the compositor unit (or greetd + its greeter) is active and
`/persist` is writable — i.e. `graphical-session.target`/greeter reached — and a crash *after*
that is a session matter handled by item 1, not a boot failure (exactly systemd's model). The
window buys one thing: a compositor that dies within 20 s of its first activation is not blessed.
Item 1 already ends the session after three such deaths in 60 s, which un-blesses nothing but does
end in a failed `mura-session.target` → the next boot's counter. Options are the comparables'
(target-reached, drop the option) or Mura's invention (keep the window, with the systemd doc's
fictional "one minute" as the only echo).

## 5. Device wait — `mura.health.deviceWaitSeconds = 20` (P5 IIO nodes, P6 `monado-cli probe`)

| Comparable | Waits for | Value | Then | Why |
|---|---|---|---|---|
| GDM — `gdm/daemon/gdm-local-display-factory.c:52-54,496-503,566-571` | a primary GPU (`CanGraphical`, udev `master-of-seat`) | **10 s** (`SEAT0_GRAPHICS_CHECK_TIMEOUT`) | **proceeds anyway**: *"It appears that your system does not have a primary GPU! Proceeding with any GPU"*, *"udev timed out, proceeding anyway."* | a greeter is better than no greeter |
| postmarketOS initramfs — `pmaports/main/postmarketos-initramfs/init_functions.sh:1330-1338`; `deviceinfo_schema.toml:127-128` | `/dev/fb0` for the splash | **10 s** (100 × 0.1 s) | error message, continues | splash needs a framebuffer; per-device opt-out `no_framebuffer` |
| systemd device jobs — `systemd/man/systemd-system.conf.xml:671-679`; `systemd.unit.xml:1182-1190` | any `.device` a job waits on | **90 s** (`DefaultDeviceTimeoutSec`) | job fails | generic default, not explained |
| `systemd-udev-settle` — `systemd/man/systemd-udev-settle.service.xml:36-44` | the whole udev queue | 120 s (unit 180 s) | — | **discouraged**: *"There can be no guarantee that hardware is fully discovered at any specific time … Services that, based on configuration, expect certain devices to appear, may warn or report failure after a timeout. This timeout should be tailored to the hardware type."* |
| Monado drivers — `survive_driver.c:51-52` (3.5 s, *"just start without those devices"*), `steamvr_lh.cpp:404` (3 s), `rift_driver.c:1448-1450` (5 s, *"wait for display/controller init"*) | already-opened devices to settle | 3–5 s | degrade or continue | driver-specific settle, not boot gating |
| mobile-nixos — `modules/initrd-boot-gui.nix:42-47`; `devices/uefi-x86_64/default.nix:22-23` | input devices for the passphrase UI | opt-in, default 0 | — | *"only necessary on 'slow' busses"*, USB |

**Determination — contested on two axes → Owner (§11 Q3). Ruled 2026-09-25: (a), the GDM/pmOS
shape — `deviceWaitSeconds` default 10, P5 and P6 become soft checks; the greeter starts and the
report carries the result.** Everything that waits for a *class of
hardware at boot* uses **~10 s**, then **proceeds degraded** (GDM, pmOS); systemd's own guidance is
that the timeout is per hardware type and that a service "may warn or report failure", not block
boot for everyone. Mura's P5/P6 wait 20 s and then fail **hard** (no greeter, counter++). The value
(10 vs 20) and the semantics (proceed-degraded vs hard-fail) are both Mura's; the comparables'
position is 10 s + proceed. The counter-argument is XR-physical (a headset with no tracking has no
usable session, unlike a PC with a secondary GPU) — that is the "genuine XR-physical reason, stated
in writing" rule 1 allows, and it is the owner's to state or reject.

## 6. Hotspot idle timeout — `mura.oob.hotspot.idleTimeoutMinutes = 10`

| Comparable | Device class | Empty-AP teardown? | Value | Why |
|---|---|---|---|---|
| comitup — `comitup/comitup/states.py:29-30,151-173` | Raspberry-Pi-class headless, mains | **No** — the 180/360 s timers retry the upstream connection while the AP stays up | — | always reachable |
| raspap | Pi router | **No** | — | persistent AP |
| balena wifi-connect — `wifi-connect/docs/command-line-arguments.md:142-146` | IoT gateway | optional `--activity-timeout`, **default 0 = off**; exits only if the portal was never opened | — | unattended devices may bound the wait |
| NetworkManager | — | **No** AP inactivity teardown exists | — | — |
| Android AOSP — `packages/modules/Wifi/.../config.xml:185-187` **[external, verified 2026-09-25]** | phone, battery | **Yes**: `config_wifiFrameworkSoftApShutDownTimeoutMilliseconds` | **600000 ms = 10 min** | comment: *"delay in milliseconds before shutting down soft AP when there are no connected devices"* — battery |
| iOS Personal Hotspot | phone, battery | yes | ~90 s **[external, unverified]** | — |

**Determination — the in-tree comparables' reason does not transfer; the battery comparables'
does.** Every in-tree provisioning AP is mains-powered and stays up; their reason (always reachable)
is not Mura's. The only devices that tear down an empty AP are battery devices, and the one whose
source is verifiable ships exactly 10 minutes. The teardown itself follows from rule 6 (a radio on
a battery); the value is Android's, cited as engineering evidence (rule 2), and the mechanism is
already condition-shaped (up only until `setup-complete` or another connection). **Applied:** the
tag becomes *sourced [external, AOSP]*; the value stays; the hotspot supervisor's per-boot idle
counter is the mechanism AOSP describes. Owner may still prefer wifi-connect's "off by default".

## 7. faillock and the user-run unlock — `state/faillock` 0755, tallies 0660 user:root

| Comparable | Unlock PAM runs as | Why | Faillock consequence |
|---|---|---|---|
| kscreenlocker — `greeter/pamauthenticator.cpp:295-308`; `greeter/worker/CMakeLists.txt:28` | **the user** (`kscreenlocker_worker`, a child of the greeter, PAM service `kde`) | commit 132adacf **[external, GitHub KDE]**: *"kcheckpass existed because historically we needed to be root to check passwords. This hasn't been true for tens of years. There are no security benefits"* | inherits pam_faillock's user-mode behaviour |
| swaylock — `swaylock/pam.c:14-18,100-106` | **the user**; the PAM build *refuses* to run setuid | *"This code does not run as root"* | same |
| hyprlock — `hyprlock/src/auth/Pam.cpp:63-66` | **the user**; parses pam_faillock's *"left to unlock"* text for its UI | — | same, and designed around faillock's messages |
| GNOME — `gnome-shell/js/gdm/userVerifier.js:324-327`; `gdm/daemon/gdm-session-worker.c:1487-1491,1776-1782,2344-2345` | **root** (`gdm-session-worker` via `OpenReauthenticationChannel`) | reauth reuses GDM's login worker; the stated reason is semantic (*"the user is already logged in after all"* — skip account/credential gates), not privilege | root can create tallies |
| Linux-PAM `pam_faillock` — man page `pam_faillock.8.xml:232-234`; `pam_faillock.c:206-207,321-322`; `faillock.c:80` **[external, github.com/linux-pam, verified 2026-09-25]** | — | *"Individual files with the failure records are created as owned by the user. This allows pam_faillock.so module to work correctly when it is called from a screensaver."* Tallies are opened `0660`; `EACCES`/`ENOENT` on open → `PAM_SUCCESS` | **upstream designed the module for user-run lockers**; the tally directory must be traversable (0755) for that to work |

**Determination — applied (converging; upstream's own design).** Three of four Wayland lockers run
PAM as the user for the reason KDE states; Linux-PAM documents user-owned tallies as the mechanism
that makes faillock work "from a screensaver". Mura's `mura-authd` (user, `mura-lock`) with a 0755
tally directory is that design verbatim. Nobody pre-creates tallies; the "first failure not
counted until a root path has created the tally" gap is upstream's accepted behaviour. The
`[mine]`/"accepted trade" wording in `multi-user.md §3.1` and `specs/session-auth.md` becomes
*upstream design, cited*. GNOME's root path is not adopted: it exists because GDM already had a
root worker, and ADR 0007 rejects a root-run helper for the lock.

## 8. Helper hardening — `PR_SET_DUMPABLE` failure fatal vs best-effort (mura-authd)

| Comparable | `PR_SET_PDEATHSIG` fails | `PR_SET_DUMPABLE` fails | Why |
|---|---|---|---|
| kscreenlocker worker — `greeter/worker/main.cpp:365-376`; `prctls.h` | **fatal** (`return 1`), plus `getppid()==1` orphan exit | **warn and continue**: *"We'll continue but it is a bit unexpected."* | commit 88a497e4 **[external]**: dumpable off to block same-uid `ptrace`; testing mode re-enables it |
| systemd — `src/basic/process-util.c:1637-1641`; `src/coredump/coredump.c:34-35`; `src/cryptenroll/cryptenroll.c:1240-1241` | **fatal** in the fork helper | `(void)` ignored (*"never enter a loop"*); `mlockall` ignored (*"A delicious drop of snake oil"*) | lifecycle correctness is fatal; secrecy hardening is best-effort |
| gdm — `gdm-session-worker-job.c:122-124` | ignored | — | — |
| swaylock — `password-buffer.c:20-72` | — | `mlock` `EPERM` → continue unlocked; other errors → allocation fails | keep secrets out of swap when the kernel allows |

**Determination — applied (converging).** Lifecycle prctls (`PDEATHSIG`) are fatal; secrecy
prctls (`DUMPABLE`, `mlockall`) are best-effort, in both comparables that set them. `mura-authd`
currently inverts this for `DUMPABLE` (exit 2). Change: `PR_SET_DUMPABLE` failure logs and
continues; `PR_SET_PDEATHSIG` stays fatal. The D5 test that reads `/proc/<pid>/environ` as the same
uid still expects `Permission denied` (the prctl succeeds in practice); no test change.

## 9. In-session time-zone / hostname authority for the logged-in user

Context: research/54 recommended (i)+(ii): derive after Connect, and a Mura polkit rule granting
active local sessions `timedate1.set-timezone` + `hostname1.set-static-hostname`. Rule 7 asks why
the comparables do what they do.

| Comparable | Who may set it without a password | Mechanism | Condition | User model | Why (stated) |
|---|---|---|---|---|---|
| systemd default — `systemd/src/timedate/org.freedesktop.timedate1.policy:32-40` | nobody (`auth_admin_keep` on any/inactive/active) | `.policy` | — | general multi-user | *"Authentication is required to set the system timezone."* |
| gnome-initial-setup — `data/20-gnome-initial-setup.rules.in` | the **setup identity** (group `gnome-initial-setup`) | `.rules` → `yes` | `subject.local` | first run, no user yet | *"without being interrupted by password dialogs"* |
| elementary initial-setup — `data/io.elementary.initial-setup.rules` | the **setup identity** (`lightdm`) | `.rules`, exact actions (hostname, accounts; **no timezone**) | `local && active` | first run | — |
| SteamOS — `jupiter-hw-support/usr/share/polkit-1/actions/org.valve.steamos.policy:120-128`; `usr/bin/holo-polkit-helpers/holo-set-timezone`; Steam runs as a `systemd --user` unit (`jovian-nixos/pkgs/gamescope-session/default.nix:146-175`) | **anyone** (`allow_any/inactive/active = yes` on a pkexec helper called with `--disable-internal-agent`) | pkexec helper | none | passwordless single-seat appliance | not written down; `--disable-internal-agent` means "never prompt", and the grant works regardless of session attribution for a user-unit process |
| postmarketOS Plasma overlay — `pmaports/extra-repos/systemd/plasma-workspace/org.kde.timezone.rules:1-13` | **any user or system service** | `.rules` → `YES` | none (`set-ntp` requires `active`) | single-user phone | rule comment: *"Allow any user or system service to change the system time zone"*; NTP because *"changing the time zone manually"* toggles it |
| Ubuntu `policykit-desktop-privileges` — `com.ubuntu.desktop.pkla:11-14,41-44`; `debian/control:12-24` **[external, git.launchpad.net, verified 2026-09-25]** | **members of `admin`/`sudo`** | `.pkla` `ResultActive=yes` for `timedate1.set-timezone`, `set-time`, `set-ntp`, `hostname1.set-static-hostname`, `set-hostname`, `locale1.*` | active session + admin group | desktop/laptop | *"allow Administrators to run common actions without being asked for their password … It does not change privileges for non-Administrators … So this satisfies the typical desktop/laptop use case where the user has full control over the hardware anyway."* |
| phosh-mobile-settings — `data/phosh-mobile-settings.rules.in:1-8`; `meson.options` | configured group (wheel/sudo) | `.rules` → `YES` (locale/keyboard only) | `active && local && isInGroup` | phone | *"Allow users in this group, typically sudo or wheel, to modify system settings like locale without requiring a password"* |
| KDE `geotimezoned` — `plasma-workspace/geotimezoned/geotimezonemodule.cpp:163-166` | whoever passes polkit (may prompt) | `SetTimezone(tz, interactive=true)` | distro's | desktop auto-TZ | *"Not really ideal to allow interactive authorization as it will potentially cause an unsolicited PolKit prompt, depending on distro configuration. On the other hand the feature will not work at all otherwise."* |
| GNOME `gsd` datetime **[external, gitlab.gnome.org]** | only if polkit already says yes | checks `polkit_permission` first; otherwise *"No permission to set timezone"* and does nothing | distro's | desktop auto-TZ | auto-TZ silently no-ops on stock systemd defaults |
| Ubuntu Touch / Lomiri — `lomiri-system-settings/plugins/time-date/timedate.cpp:169` | the phone user (grant lives in the image, Ubuntu's pkla shape) | `SetTimezone(tz, false)` | — | single-user phone | — |

**What the comparables converge on.** (a) First-run: the *setup identity* is granted, local (and
active), exact actions or namespaces — already Mura's `50-mura-setup.rules` shape (camp A). (b) For
the *logged-in user*, no shipping system inspects whether the account has a password. Single-seat
appliances grant the action to everyone (SteamOS, pmOS Plasma). The largest desktop distro grants
it to **active sessions of the admin group** and leaves non-admins on systemd's prompt, with a
stated reason that is Mura's invariant 10 almost word for word (*"the user has full control over
the hardware anyway"*); phosh's optional rule and pmOS's NetworkManager rule use the same
`active && local && group` shape. Upstream GNOME/KDE stay on `auth_admin_keep`, and their own
automatic time-zone features then either no-op or prompt unsolicited — which is the failure
research/54 set out to avoid.

**Determination — applied (converging on shape and reason).** One rule, both profiles:
`50-mura-timedate.rules` grants `org.freedesktop.timedate1.set-timezone`,
`org.freedesktop.hostname1.set-static-hostname` and `org.freedesktop.hostname1.set-hostname` to
subjects that are `local && active && isInGroup("wheel")` — Ubuntu's grant and reason, phosh's
condition set, narrower than SteamOS/pmOS (`allow_any`), exact actions like elementary. On the
appliance profile the passwordless `mura` is in wheel, so the in-headset confirm never prompts; on
the multi-user profile wheel members get the same and others get systemd's default. `set-ntp` is
not included (Ubuntu includes it; pmOS ties it to manual zone changes; Mura's zone is derived, so
the case does not arise — flagged, not decided). The system instance (`mura-setup`) additionally
gets `set-timezone`/`set-static-hostname` in `50-mura-setup.rules` if missing (camp A). **One thing
verified, not assumed:** polkit's attribution of `subject.active` to a process inside the user
manager (our compositor's children) — probed in the VM with `systemd-run --user -M mura@`; Ubuntu's
rule working under GNOME's systemd-managed session is the prior that it does.

## 10. Found on the way

- **`perl` is on the system PATH** of both toplevels: NixOS's `environment.defaultPackages`
  (`perl rsync strace`), which nixpkgs describes as *"packages that aren't strictly necessary for a
  running system, entries can be removed for a more minimal NixOS installation"*
  (`nixos/modules/config/system-path.nix:111-123` at the pinned nixpkgs). No comparable NixOS
  appliance in `references/` (mobile-nixos, jovian) removes them. Rule 6 says remove; rule 3 says
  the wearer may install anything. **Ruled 2026-09-25: removed (`environment.defaultPackages = []`);
  `tests/closure.nix` fences perl on PATH too.**
- **The residual `python3` in the toplevel closures** is nixpkgs-side (systemd-boot's installer,
  `nixos-rebuild-ng`, mesa, gstreamer, flatpak via the portal, speech-dispatcher/pyxdg via the
  stand-in desktop stack); `tests/closure.nix` pins it as a shrinking allowlist. The login-path
  closure (four Mura programs + greetd + systemd + util-linux, 97 paths) has none.

## 11. Decisions for the owner (rule 8), with the comparables' positions as the options

- **Q1 — the crash-loop ladder (§3).** (a) Keep `crashLoopThreshold=3` → `mura-recovery.target`;
  its only shipping comparable is Android's Rescue Party (to be cloned into `references/` and cited
  as engineering evidence, rule 2). (b) systemd/RAUC shape: no userspace ladder — an unblessed boot
  is the bootloader's to fall back from; recovery is the user's explicit choice (`bootTries` stays).
- **Q2 — the blessing tier (§4).** (a) systemd/RAUC/mobile-nixos shape: bless when the compositor
  unit (or greeter) is active and `/persist` is writable; drop `readinessStabilitySeconds`. (b) Keep
  the 20 s window — no comparable; Mura's invention. **Ruled (a), 2026-09-25.**
- **Q3 — device wait (§5).** (a) GDM/pmOS shape: 10 s, then proceed degraded (greeter starts, P5/P6
  become `soft`). (b) Keep 20 s + hard fail, stating the XR-physical reason in writing (rule 1).
  (c) 10 s + hard fail (the comparables' value, Mura's semantics). **Ruled (a), 2026-09-25.**
- **Q4 — `perl rsync strace` on PATH (§10).** (a) Drop `environment.defaultPackages` (rule 6; nixpkgs
  names this the minimal-installation path; the wearer installs what they want). (b) Keep NixOS's
  default. **Ruled (a), 2026-09-25.**

Applied without asking (converging evidence with transferring reasons): §1, §6, §7, §8, §9, and
`bootTries` in §3. §2 keeps its existing decider.
