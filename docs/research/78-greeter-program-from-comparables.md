# 78 — The greeter program from comparables: who speaks the auth daemon, how the conversation runs, what the program must do

**Date:** 2026-09-28. **What this is:** the comparables pass that precedes G1's two deliverables —
`mura-greeter` (the program) and `zxr --greeter` composing it. Earlier passes settled the
*mechanisms around* the program: [research/11](11-display-managers-greeters.md) read greetd's
IPC to completeness (§2.1), the gtkgreet+cage kiosk shape (§2.4) and inventoried the
pre-authentication furniture (§11); [research/12](12-lock-screens-and-appliance-login.md) read
the lock side (`ext-session-lock`, kscreenlocker's process, the PAM conversation);
[research/41](41-multi-user-login-landscape.md) how greeters pick accounts; [research/75 §3.1](75-shell-plane-from-comparables.md)
one row per greeter (process, toolkit, seams); [research/77](77-shell-layer-mechanics-from-comparables.md)
the compositor half the program maps on. What none of them read is **how the greeter programs
are built**: their conversation state machines and failure paths, who speaks the authority and
over what channel and why, how sessions are enumerated and started, how prompts render, what the
runtime hands them, how they exit, and how their daemons supervise them. Those are the questions
a program has to answer before it is written, and the one decision session-auth §5 left open
("whether zxr holds `$GREETD_SOCK` and relays, or the client speaks greetd itself") depends on
them. Every cite is `<clone>/<path>:<line>` under `references/` (greetd with its `agreety` and
`fakegreet`, gtkgreet, regreet, tuigreet, cosmic-greeter + daemon, sddm, lightdm, gdm,
gnome-shell, kscreenlocker) or a repo path. Consumer XR platforms appear as mechanism evidence
only (rule 2). **Budget impact** (overview invariant 9): a research document; §7 costs the
program shapes.

## 0. Summary

- **Every greetd greeter speaks greetd itself, from the UI process, over `$GREETD_SOCK`.**
  agreety, gtkgreet, regreet, tuigreet and cosmic-greeter all open the socket greetd put in
  their environment and run the `create_session → auth_message* → start_session → exit` loop
  in-process; none has a compositor relaying for it, and greetd hands the socket to the greeter
  *session* by PAM env, which any child of that session inherits (`greetd/src/session/worker.rs:150-153`).
  The display managers that do split UI from authority (LightDM, GDM, SDDM, kscreenlocker) split
  it because **they own PAM** and the UI must not — the split is around the PAM worker, not
  around the socket to an authority that already owns PAM. **Determination (§9): in greeter
  mode `mura-greeter` speaks greetd directly; zxr relays nothing.**
- **The exit is the handoff.** greetd starts the session only when the greeter process
  terminates ("The session will start after the greeter process terminates",
  `greetd/man/greetd-ipc-7.scd:50`), gives it 5 s, then SIGTERMs and after 10 s SIGKILLs
  (`greetd/src/context.rs:294-297, 308-330`). All four greeters `exit(0)` on the second
  `success`. The greeter process greetd spawned is **zxr**, so zxr must exit when its program
  does — cage's kiosk rule (`cage/cage.c`: exit when the primary client exits, research/11
  §2.4), which also makes a crashed program a greetd restart (`Restart=always`, `RestartSec=1`,
  `StartLimitBurst=5/30 s`, `greetd/greetd.service:11-14`) — greetd's own supervision, the one
  ADR 0007 names for greeter mode.
- **The conversation contract has four rules the greeters converge on:** render `visible`/
  `secret` prompts verbatim as a text field (masked for `secret`); **acknowledge `info`/`error`
  with an empty response or the session stalls** (cosmic-greeter's comment,
  `cosmic-greeter/src/greeter.rs:1229-1235`; regreet auto-ACKs, `regreet/src/model.rs:290-298`);
  on `error{auth_error}` cancel and re-`create_session` for the same user with a **generic**
  failure text — tuigreet refuses to show greetd's description because it "may contain entered
  information, sometimes passwords" (`tuigreet/crates/tuigreet/src/ipc.rs:113-114`); on
  `auth_message{error}` (a PAM_ERROR_MSG mid-conversation) show it and stay in the session
  (`cosmic-greeter/src/greeter/ipc.rs:117-119`).
- **Sessions come from a list the system writes, not from the greeter's imagination**: three
  greeters scan `wayland-sessions`/`xsessions` `.desktop` files and turn `Exec` into
  `start_session.cmd` with `XDG_SESSION_TYPE`/`XDG_CURRENT_DESKTOP`/`XDG_SESSION_DESKTOP` in
  `env`; gtkgreet reads `/etc/greetd/environments`, which Mura's module already writes
  (`modules/os/session.nix:198`). **Determination:** the module system stays the source and the
  program reads the file it already writes; the chooser is hidden with one entry (GDM's rule).
- **The lock mode is the other half of the same program, and there the authority is zxr.**
  kscreenlocker's UI spawns its own PAM worker over private D-Bus and the compositor learns of
  the unlock from the UI's **exit code 0** (`kscreenlocker/ksldapp.cpp:168-196`); LightDM and GDM
  keep PAM in a daemon-owned worker and feed the UI **typed prompts** over a channel the daemon
  hands it (two pipes in env, `lightdm/src/greeter-session.c:65-69`; a private D-Bus server,
  `gdm/daemon/gdm-session.c:2086-2122`). Mura's lock state machine, nonce revocation and I2/I3
  were compositor invariants (session-auth §2.4, §3). *Superseded the same day (§9 det. 6, owner):*
  the lock program is a **resident unit on the public socket** that locks through
  `ext-session-lock-v1` and **owns its `mura-authd` conversation** (kscreenlocker's worker,
  swaylock's PAM child, cosmic-greeter's PAM thread); the compositor keeps the lock state and its
  triggers, and trusts the locker's `unlock_and_destroy` as every compositor does.
- **Supervision of a socketpair child is the compositor's in every comparable that has one**
  (kscreenlocker restarts its greeter three times then shows an emergency window,
  `ksldapp.cpp:199-210`; KWin restarts its input method); the "user unit with
  `Restart=on-failure`" of ADR 0007's amendment has no comparable for a client that arrives over
  a socketpair. **Owner item Q1.**
- **The harness exists upstream:** `fakegreet` — greetd's own test double that runs a greeter
  command with a fake `GREETD_SOCK`, asks `User:`/`Password:`/`7 + 2:`, accepts
  `user`/`password`/`9`, sleeps 2 s on failure and 5 s on start (`greetd/fakegreet/src/main.rs:53-112`).
  G1's exit criterion runs against it unmodified.

## 1. What greetd is, as the program sees it

- **Launch.** greetd runs `default_session.command` as `default_session.user` (default `greeter`,
  `greetd/config.toml:13-16`) as a PAM session of class `greeter` (`worker.rs:47`) with
  `GREETD_SOCK=<listener path>` put into the PAM environment "early as we are about to reuse the
  environment" (`worker.rs:150-153`), plus `XDG_SEAT=seat0`, `XDG_SESSION_CLASS=greeter`
  (`worker.rs:215-217`). Mura's command is `zxr --greeter` (the stand-in today is
  `cage -s -- gtkgreet`, `modules/os/session.nix:48, 221-223`); the program is zxr's child and
  inherits the environment.
- **The loop** (agreety, the reference, `greetd/agreety/src/main.rs:80-133`):
  `create_session{username}` → each `auth_message` answered by `post_auth_message_response`
  (`visible`/`secret` with the typed text, `info`/`error` with `None`) → `success` →
  `start_session{cmd, env}` → `success` → exit. `error{auth_error}` → `cancel_session`, retry
  (agreety up to `max-failures` 5, `:168-190`); `error{error}` → fatal. greetd cancels the session
  itself on error (`greetd_ipc/src/lib.rs:65-66, 84-88`).
- **The handoff.** `start_session` schedules; the session starts when the greeter process exits
  (`man/greetd-ipc-7.scd:50`; `context.rs:276-300` — `alarm::set(5)`, "We give the greeter 5
  seconds to prove itself well-behaved before we lose patience and shoot it in the back
  repeatedly"; `:308-330` TERM each second, KILL after 10 s). On the greeter's exit with a
  scheduled session, greetd starts it (`:344-380`); on exit **without** one, greetd returns
  `"greeter exited without creating a session"` (`:383-385`), which propagates out of the main
  loop (`server.rs:274`) — greetd exits and systemd restarts it (`greetd.service:11-14`).
- **The message types** are four: `visible`, `secret`, `info`, `error` (`greetd_ipc/src/lib.rs:103-117`);
  the protocol "makes no assumption about the questions" (`:148-153`, research/11 §2.1).
- **`fakegreet`** (`greetd/fakegreet/src/main.rs`): binds a socket, sets `GREETD_SOCK`, runs the
  greeter command via `sh -c`; the conversation is `User:` (visible) → `Password:` (secret) →
  `7 + 2:` (visible); `user`/`password`/`9` succeed, anything else sleeps 2 s and answers
  `auth_error("nope")`; `start_session` sleeps 5 s then succeeds (`:53-112`, `:150-180`).

## 2. The greetd greeters, as built

| | gtkgreet | regreet | tuigreet | cosmic-greeter |
|---|---|---|---|---|
| **program** | C, GTK3 (`gtkgreet/gtkgreet/main.c:4`) | Rust, Relm4/GTK4 (`regreet/README.md:10-13`) | Rust, ratatui TUI (no Wayland) | Rust, libcosmic/iced (`cosmic-greeter/src/main.rs:48-49`) |
| **runs under** | cage or sway: `cage gtkgreet`; `exec 'gtkgreet; swaymsg exit'` (`gtkgreet/man/gtkgreet.1.scd:50-63`) | `dbus-run-session cage -s -mlast -d -- regreet` — `-s` "to prevent locking yourself out", `-mlast` "a single-monitor application" (`regreet/README.md:157-166`) | the VT | `cosmic-greeter-start` → `exec cosmic-comp cosmic-greeter` as user `cosmic-greeter` (`cosmic-greeter-start.sh:1-3`; `cosmic-greeter.toml:7-9`) |
| **surface** | toplevel, or layer-shell `top` all edges + exclusive zone with `-l` (`window.c:42-49`, `main.c:26, 100-106`) | fullscreen toplevel on the first monitor (`model.rs:150-187`), no layer-shell | — | layer-shell `Top`, all anchors, exclusive keyboard (`greeter.rs:1333-1364`) |
| **speaks greetd** | in-process, a new connection per round trip (`proto.c:110-139`) | in-process, one stream (`client.rs`) | in-process (`ipc.rs`) | in-process subscription (`greeter/ipc.rs`) |
| **username** | free text (`gtkgreet.c:128-129`) | AccountsService list or manual (`model.rs:485-500`) | typed or NSS `--user-menu` (`info.rs:226-249`) | daemon's list + "Enter name manually…"; changing user cancels (`greeter.rs:1460-1462`) |
| **secret / visible** | `GtkEntry`, `GTK_INPUT_PURPOSE_PASSWORD` + `visibility=FALSE` for secret (`window.c:106-111`); prompt verbatim (`:100`) | `PasswordEntry` / `Entry` (`templates.rs:111-116`); prompt verbatim (`component.rs:241-242`) | masked or dotted (`tuigreet-types/src/lib.rs:64-83`); verbatim (`ipc.rs:132`) | `.password()` text input (`greeter.rs:918`); verbatim |
| **info / error message** | label, no field, Continue still present (`window.c:119-125, 175`); empty response (`actions.c:96-103`) | shown, then **auto** `PostAuthMessageResponse{None}` — "immediate greetd updates when using authentication procedures that don't use text input … Fingerprint then Password" (`model.rs:290-298, 360-412`) | shown, ACK `None` (`ipc.rs:142-165`) | shown, ACK `None` — "If we don't ACK, greetd will wait forever and the UI will appear stuck" (`greeter.rs:1229-1235`); PAM_ERROR_MSG "a failed attempt, not a dead session" → no cancel (`ipc.rs:117-119`) |
| **`error{auth_error}`** | cancel, red "Login failed", back to username (`actions.c:46-61`) | "Login failed: …" with description, cancel (`model.rs:382-397`) | cancel, **generic** text, re-`create_session` same user (`ipc.rs:113-114, 273-281`) | localized from the description (PERM_DENIED, MAXTRIES…), cancel, reconnect (`ipc.rs:16-36, 160-175`) |
| **sessions** | `-c` + `/etc/greetd/environments` lines; `cmd` = one string, no `env` (`config.c:11-34`; `proto.c:159-164`) | `.desktop` in `/usr/share/{xsessions,wayland-sessions}` or `$XDG_DATA_DIRS`; `Hidden`/`NoDisplay`; shlex `Exec`; `env` `XDG_SESSION_TYPE` + TOML (`sysutil.rs:110-204`; `model.rs:577-591`) | same scan; `cmd` one string; `env` `XDG_SESSION_{TYPE,DESKTOP}`, `DESKTOP_SESSION`, `XDG_CURRENT_DESKTOP` (`info.rs:49-60, 292-375`; `ipc.rs:343-398`) | same scan; `/usr/bin/env` + vars + `Exec` (`greeter.rs:153-162, 227-273`) |
| **remembers** | nothing | `/var/lib/regreet/state.toml`: last user, per-user last session (`cache/mod.rs:22-28`) | `/var/cache/tuigreet/{lastuser,lastsession…}` (`info.rs:26-29, 152-223`) | cosmic-config `last_user`, per-uid `last_session` (`cosmic-greeter-config/src/lib.rs:18-22`) |
| **power** | none | shell `reboot`/`poweroff` (`constants.rs:42-45`) | `shutdown -h/-r now`, `loginctl suspend` — "provided by both systemd and elogind" (`power.rs:68-78`) | logind D-Bus `power_off`/`reboot`/`suspend` with confirm dialogs (`logind.rs:37-53`; `greeter.rs:1657-1696`) |
| **furniture** | clock (`gtkgreet.c:75-91`), CSS `-s`, background `-b` | clock, `[GTK]`/`[background]`/CSS TOML | time/issue/greeting, battery from sysfs (`info.rs:383-407`), caps-lock via `kbdinfo` (`:410-417`) | clock (`time.rs`), UPower battery, NetworkManager, caps-lock (`greeter.rs:923-924`), keyboard layout via a cosmic protocol (`keyboard_layout_wayland.rs`), a11y screen reader/magnifier/high contrast/invert (`greeter.rs:694-723`), **spawns `cosmic-osk overlay`** (`greeter.rs:284-297`), per-user wallpaper/theme |
| **exit** | `exit(0)` on the second `success` (`actions.c:17-26`) | `process::exit(0)` (`model.rs:618-620`) | "greetd acknowledged session start, exiting" (`ipc.rs:172`) | destroy layers, `exit(0)` (`greeter.rs:1698-1709`) |
| **without `GREETD_SOCK`** | stderr, exit (`proto.c:115-116`) | panic (`client.rs:54-56`) | exit with message (`greeter.rs:457-460`) | a UI state "GREETD_SOCK variable not set" (`greeter.rs:334-341`) |

**cosmic-greeter's root daemon.** `cosmic-greeter-daemon` (root, D-Bus `com.system76.CosmicGreeter`,
`debian/cosmic-greeter-daemon.service:1-11`) exists to read **per-user** wallpaper, theme, xkb and
a11y config for the picker, and does it by assuming each user's identity — "IMPORTANT: this
function is critical to the security of this proxy … A good test is to see if the /etc/shadow
file can be read with a non-root user, it should fail" (`daemon/src/main.rs:14-16, 88-89`);
the greeter falls back to a bare `pwd` list when the daemon is absent (`greeter.rs:81-91, 130-135`).
The daemon is for per-user *appearance*, not for greetd — research/41 already settled Mura's
picker metadata as root-written `state/accounts/<user>/` (multi-user §2), so no daemon.

**Mode by identity.** One cosmic-greeter binary is greeter or locker by the uid's name
(`main.rs:34-38`); the locker half uses `ext-session-lock` (research/75 §3.1).

## 3. The display managers and the locker: where the split is, and why

| | UI process | PAM process | channel UI ↔ authority | what the UI receives | after success | stated reason |
|---|---|---|---|---|---|---|
| **SDDM** | `sddm-greeter(-qt6)` as user `sddm`, a `greeter`-class session started through `Auth` (`sddm/src/daemon/Greeter.cpp:179-236`) | `sddm-helper` `PamBackend` (`helper/backend/PamBackend.cpp:220-226`) | `QLocalSocket` `sddm-<display>-<random>`, chowned to `sddm` (`SocketServer.cpp:46-58`; `Display.cpp:249-268`) | in this pin **no live prompts**: `Login{user, password, session}` up front; `LoginSucceeded`/`LoginFailed`/`InformationMessage` back (`Messages.h:26-42`; `Display.cpp:521-559`) | `LoginSucceeded`; the greeter is stopped 5 s after the session starts (`Display.cpp:512-513, 562-566`) | "the SDDM user has special privileges that skip password checking so that we can load the greeter" (`Display.cpp:336-337`) |
| **LightDM** | the greeter binary in a greeter session | `lightdm --session-child` (`src/session.c:621-630`; `session-child.c:105-161`) | **two pipes** whose fds the daemon puts in the greeter's env, `LIGHTDM_TO_SERVER_FD`/`LIGHTDM_FROM_SERVER_FD`, daemon ends `CLOEXEC` (`src/greeter-session.c:47-69`) | typed: `SERVER_MESSAGE_PROMPT_AUTHENTICATION` carrying the PAM style → `show-prompt` SECRET/QUESTION, `show-message` INFO/ERROR (`src/greeter.c:377-406`; `liblightdm-gobject/greeter.c:658-679`) | `START_SESSION` → the seat usually stops the greeter and reuses the display server (`seat.c:1211-1229`) | the pipe race comment (`src/greeter.c:81-83`); PAM isolated in the child |
| **GDM** | gnome-shell in a `gdm-greeter` launch-environment session (`gdm-launch-environment.c:53-54, 506-508`) | `gdm-session-worker` (`gdm-session-worker.c:1235-1283`) | two private D-Bus servers on a unix socket dir — "for worker", "for greeters and such" (`gdm-session.c:2020-2122`); the greeter gets the address from `Manager.OpenSession` (`gdm-manager.c:834-846`) | typed signals on `org.gnome.DisplayManager.UserVerifier`: `InfoQuery`, `SecretInfoQuery`, `Info`, `Problem`, `VerificationComplete/Failed` (`gdm-session.xml:40-93`); parallel services `gdm-password`/`gdm-smartcard`/`gdm-fingerprint` (`gnome-shell/js/gdm/authServicesLegacy.js:17-19`) | `VerificationComplete` + `SessionOpened`; the greeter calls `StartSessionWhenReady` (`gdm-session.xml:137-140`) | separate servers with peer-uid checks |
| **kscreenlocker** | `kscreenlocker_greet`, spawned by `ksldapp` with `WAYLAND_SOCKET` when `setWaylandFd` was set (`ksldapp.cpp:377-422`) | `kscreenlocker_worker`, spawned **by the greeter** over a private `QDBusServer` whose address it writes to the worker's stdin (`greeter/pamauthenticator.cpp:295-310`; `worker/main.cpp:262-312`) | greeter ↔ ksld: **exit code** (0 = unlock) and a stdout line (`ksldapp.cpp:162-196`); greeter ↔ worker: D-Bus `Prompt`/`MaskedPrompt`/`InfoMessage`/`ErrorMessage` vs `Authenticate`/`Cancel` (`org.kde.plasma.screenlocker.worker.xml:8-18`) | typed prompts from its own worker | greeter exits 0 → `doUnlock()` | "All the blocking PAM tech is in subprocesses that will get reaped … once they notice we are gone" (`greeterapp.cpp:320-321`); restart 3× with software rendering, then the emergency window (`ksldapp.cpp:199-210`) |

**The reading.** Where the daemon *owns PAM*, it owns the conversation and the UI is a typed
prompt terminal on a channel the daemon hands it (LightDM pipes-in-env, GDM private bus, SDDM
socket); where the UI owns PAM it pushes PAM into its own worker and the compositor learns the
outcome from the UI's lifetime (kscreenlocker). Nobody puts a relay between a UI and an authority
that already owns PAM — the greetd greeters talk to greetd. Two shapes therefore transfer, one per
Mura mode:

- **Greeter mode:** the authority is greetd, it owns PAM, it handed the socket to the session
  → the program speaks greetd (the greetd greeters, 5 for 5 with agreety).
- **Lock mode:** the authority is zxr — it owns the lock state (I1–I3), the nonce and its
  revocation (session-auth §2.4), and spawns `mura-authd` (§2.1) — so the program is LightDM's
  and GDM's typed terminal, on a channel zxr hands it. kscreenlocker's exit-code shape would move
  the `success` observation out of the compositor, which is exactly what §2.4's revocation
  forbids ("a `success` bearing an invalidated nonce is ignored" needs the compositor to be the
  one reading it).

## 4. The runtime the program lives in

- **Greeter mode:** greetd → `zxr --greeter` (PAM session, user `greeter`, `GREETD_SOCK`,
  `XDG_SESSION_CLASS=greeter`, `XDG_RUNTIME_DIR` of the greeter user) → zxr spawns the program
  with `WAYLAND_SOCKET` (research/77 §5.3; kscreenlocker's `setWaylandFd`) and the inherited
  `GREETD_SOCK`; no listening socket exists (gate 8 B-1). The program is the only member; the
  OSK (squeekboard now, `mura-osk` later) is the second trusted child (shell-plane §3.2), as
  cosmic-greeter spawns `cosmic-osk` (`greeter.rs:284-297`) and phosh shows squeekboard on its
  lock (research/75 §3.2).
- **The exit path, concretely:** program `start_session` → `success` → program exits 0 → zxr
  sees the trusted client disconnect (`ClientData::disconnected`, `trusted_lost`, research/77
  §5.3) → **zxr exits** (cage's rule, `cage/cage.c:157-206, 689, 724`, research/11 §2.4) within
  greetd's 5 s (zxr's teardown is bounded at 500 ms, spec §9) → greetd starts the session. A
  program crash takes the same path and greetd exits with "greeter exited without creating a
  session" → systemd restarts greetd after 1 s, five times per 30 s (`greetd.service:11-14`) —
  the same failure class as gtkgreet crash-looping under cage. zxr needs no knowledge of
  success; the two paths are one line.
- **Lock mode (in-session):** zxr spawns the program with `WAYLAND_SOCKET` and — the
  determination of §3 — a second fd in env carrying session-auth's `prompt_batch` /
  `respond_batch` (LightDM's `LIGHTDM_TO/FROM_SERVER_FD`). The `mura-authd` socketpair stays
  zxr's; the nonce never reaches the program. Who restarts the program when it dies is **Q1**.
- **Sessions:** `/etc/greetd/environments` is already written by the module
  (`modules/os/session.nix:198`, "mura-session start"); greetd joins `start_session.cmd` and
  runs it through `/bin/sh -c` under the PAM environment (`greetd/src/session/worker.rs:239-245, 277-288` — `exec cmd.join(" ")`, or sourcing the profile first),
  so gtkgreet's one-string `cmd` and regreet's argv are equivalent on the wire; `env` carries
  `XDG_SESSION_TYPE=wayland` and the desktop names (tuigreet/cosmic's set — the values `mura`
  are a proposal, flagged; nothing reads them yet). The chooser
  is hidden with one entry (GDM's rule, multi-user §2). *Reconciliation flagged:* multi-user §2
  says "from `wayland-sessions` `.desktop` files", session-auth §5 and shell-plane §3.1 say
  "from `mura.xr.shell` values" — the module writing a file the program reads satisfies both;
  which file (`environments` today, `.desktop` if a second shell ever ships) is a one-line
  change in the module.

## 5. What the program renders, from the comparables

- **Prompts:** the text field with the prompt verbatim (all five); masked for `secret`. Mura
  adds the digit pad keyed off `secret` + the user's `numeric-credential` hint (session-auth
  §2.3, ADR 0018) — no comparable parses prompt text, and none should (`greetd_ipc/src/lib.rs:148-153`).
- **Messages:** `info` inline, `error` inline and *stay in the conversation* (cosmic's
  PAM_ERROR_MSG rule); both ACKed with an empty response immediately (regreet's reason: the
  fingerprint-then-password flow).
- **Failure:** `auth_error` → cancel, **generic** text ("Authentication failed" — tuigreet's
  leak argument, and multi-user §2's "greeter PAM failures are uniform"), keep the username,
  re-`create_session` (tuigreet's soft reset); session-auth's `delay_ms` is honoured before the
  retry UI in lock mode; greetd has no delay of its own beyond PAM's (`fakegreet` sleeps 2 s).
- **Account picker:** research/41 §1.4 and multi-user §2 — NSS over the UID window, free-text
  entry always beside it (gtkgreet's fallback), last-user preselection in root-owned state (the
  regreet/tuigreet/SDDM pattern), zero-accounts still renders entry + power.
- **Furniture:** clock (all), power menu over **logind D-Bus** (cosmic; SDDM's daemon; LightDM's
  `power.c` — not shell commands: `allow_active` is what makes it work without a helper,
  multi-user §2), suspend/hibernate only when `Can*` says yes, confirm dialogs with a timeout
  (cosmic `greeter.rs:1657-1696`), caps-lock indicator (cosmic, tuigreet), battery and network
  (cosmic UPower/NM; multi-user §2's Wi-Fi join with the GDM polkit rule), the a11y menu
  (first-run-onboarding §4.4's input-floor controls; cosmic's screen reader/magnifier/
  high-contrast), the OSK as a sibling trusted client, not the program's own widget.
- **Layouts:** cosmic's keyboard-layout switcher rides a cosmic protocol; Mura's seat keymap is
  `input.keyboard.*` (settings, zxr's) and greeter mode has no user store — the system default
  applies; a switcher is not G1's.
- **Theming:** CSS/TOML in the greetd greeters, per-user look via cosmic's daemon; Mura's is
  shell-plane §5 (units and keys), nothing at G1.

## 6. The lock program, from the comparables

- One binary, two modes, selected by zxr's argument (`--lock`) rather than by uid (cosmic's
  trick works because its greeter has its own user; Mura's lock program runs as the session
  user and is spawned by zxr, which knows the mode).
- **Unlock authority stays in zxr**: the program never sees `success` as an unlock — it renders
  what the relay sends and zxr flips the mode when *it* reads `success` from `mura-authd` under
  a live nonce (session-auth §2.4, §3). kscreenlocker's exit-code unlock is the shape *not*
  taken, for the revocation reason (§3).
- **The scene while the program is absent** is the opaque frame (ADR 0007; gate 8 G2: 0.02
  members per frame, mode stays); who restarts the program is Q1.
- **What the lock UI shows beyond the prompt:** kscreenlocker's fallback theme has clock,
  switch-user (`LockScreen.qml:14, 30-31, 84`), and KWin's `kde_lockscreen_overlay_v1` lets a
  privileged client (the OSD) draw above the lock (`kwin/src/wayland/lockscreen_overlay_v1.cpp:29-36`)
  — Mura's trusted connection is that bit (research/77 §4.3), so the OSK and an OSD are
  composed while locked without a protocol.

## 7. Cost

Not re-measured here: research/75 §5.4 measured the Slint program shape on nested zxr (12.5 MB
binary, 20.5 MB RSS / 12.6 MB PSS, 6 threads, 31 ms to a mapped plane, idle-clean, text-input
and AT-SPI gates), and the greeter is not resident in a session (spawned on lock). The
conversation client is a few hundred lines (agreety is 192; `greetd_ipc` is the protocol crate,
already Rust); the relay backend is the same message shapes over an fd. The two backends add no
process, no thread, no bus in greeter mode; the lock relay is one fd on zxr's state loop.

### 7a. Stage B — the platform gate (host, nested zxr, 2026-09-28)

The tiny `Username / Password / Log in` scene (Slint std-widgets, `EmbedForSoftwareRenderer`) on
the sctk platform (`mura-slint-platform`: `Platform` + `WindowAdapter` + `WindowAdapterInternal`,
`SoftwareRenderer` into two `wl_shm` `ARGB8888` slots on the frame-callback cadence, seat
keyboard/pointer/touch, `zwlr_layer_surface_v1` and `ext_session_lock_surface_v1` roles,
`zwp_text_input_v3`; `mura-slint-accesskit`: upstream's translation on `accesskit_unix`), a
debug build, dev-session `--zxr` (simulated HMD, no mirror):

| leg | result |
|---|---|
| layer role, `zxr --greeter`, over the socketpair | maps as `overlay`/`mura-greeter`, 1920×1493 on the head frame, `Exclusive` keyboard, focus override taken (`layer_focus_overrides=1`); one `shm_uploads`, `commits=2` over 600 frames — a still scene costs nothing after its frame |
| keyboard (`zxr ctl type/key`) | `alice`, Tab, `secret`, Return → the scene's `login("alice", 6 chars)`; Slint's `InputMethodRequest::{Enable,Update,Disable}` follow the field focus (content type `Password` on the second field) |
| pointer | `wl_pointer` enter/motion/`button 272` from the head ray, the relative pointer device and the controller ray (after its first commit; spatial-input §5); a click on `Log in` fires the callback |
| OSK (unmodified squeekboard, second trusted client) | `zwp_text_input_v3.enter` arrives once an input method is bound (Smithay's rule); the field's `enable`+`commit` → squeekboard `activate`, maps `top`/`osk` 1920×497+0+996; a controller click on its centre key → `zwp_input_method_v2.commit_string("v")` → the scene's `commit_string` + `done` → the focused field reads `v`. **Under a full-frame `overlay` greeter the OSK is occluded** (same head frame, `overlay` above `top`; the click lands on the greeter) — phoc's `phoc_layer_shell_update_osk` rule is the fix, in zxr (G1; shell-plane §4). squeekboard also hides itself while the host gsettings `screen-keyboard-enabled` is false (it reads that as "a physical keyboard is present", `server-context-service.c:49,116`); the harness gives it a private keyfile backend |
| session-lock role, public socket, plain `zxr` | `ext_session_lock_v1.lock` → lock surface configured 1920×1493 → `locked` (zxr: mode `Locked`, I2 confirmed after a frame with no untrusted sample) → keys reach the lock surface → `unlock_and_destroy` → zxr `unlocked`, mode `Normal`; the program exits cleanly |
| AT-SPI | the scene registers on the a11y bus (`accesskit_unix` → `org.a11y.atspi.Registry` `Embed`); tree: window → `Mura` (label), `Username` (role 79 entry), `Password` (40 password-text), `Log in` (43 push-button, `Action` `click`), status; `org.a11y.atspi.Action.DoAction(0)` on `Log in` → the callback fires. `Text` interfaces are absent on the entries (accesskit_atspi_common 0.19's coverage). The host's a11y bus socket was stale, so the harness runs a private session bus + `at-spi-bus-launcher` + `at-spi2-registryd` for the client |
| a11y prerequisite found | `EmbedForSoftwareRenderer` silently compiles the accessibility tree away (`i-slint-compiler/lib.rs:342-346`, "HACK: … not supported with backend that support software renderer anyway"): the first tree had 1 node; with the hack removed, 6. Patch carried (Phase 4's Slint patch) |
| extraction size | `accesskit.rs` 1011 lines → `mura-slint-accesskit` 877 (reflowed); semantic changes marked `// mura:`: the `Host` trait (3 methods), mpsc + wake for the three `accesskit` handlers (they run on AccessKit's thread), `set_window_focused`, `focus_node` asking the host. No `i_slint_core` change |
| the platform | 1 228 lines (lib 156, state 683, adapter 161, text_input 186, pixel 42); one thread, one connection, one calloop loop; preedit is injectable after all (`WindowEvent::internal(InternalKeyEvent { UpdateComposition… })`, the path winit's `Ime::Preedit` takes, `winitwindowadapter.rs:1442-1460`) — the "gap" in research/75 was the public API only |

**Decision (plan §Phase 1):** the extraction is contained ⇒ proceed on Slint + sctk; GTK4 is
not brought to the owner. Release-profile measurements (binary, RSS, time-to-map) are taken on
the program itself at the G1 gate (§7b).

### 7b. G1 — the program on nested zxr (host, 2026-09-28; spec §12 gate 9)

`mura-greeter` (release profile, `opt-level = "s"`, LTO) under greetd's **`fakegreet`
unmodified** → `dev-session --zxr -- --greeter --trusted mura-greeter --osk squeekboard`, Monado's
simulated HMD (which wobbles), the injector for keys, the controller ray and the relative pointer:

| leg | result |
|---|---|
| the kiosk | greetd's `fakegreet` spawns the session command, zxr admits the program as the primary and the OSK as its child; the scene maps `overlay`/`mura-greeter`, **zone 0** (the OSK's exclusive band shrinks it to 1920×996 while the keyboard is up — the power and accessibility controls stay reachable), `Exclusive` keyboard, the override taken; **no listening socket** (gate 8 (g), unchanged) |
| the conversation, keyboard | `user` ⏎ → `create_session` → `Password:` (secret, masked) → `password` ⏎ → `7 + 2:` (visible) → `9` ⏎ → `success` → `start_session{cmd: ["mura-session","start"], env: [XDG_SESSION_TYPE=wayland, XDG_SESSION_DESKTOP=mura, XDG_CURRENT_DESKTOP=Mura]}` → `success` → **the program exits 0 3.7 ms later, zxr exits with status 0** (`greeter mode: the greeter program exited`), fakegreet's `start` returned. Wrong answer: `error{auth_error}` → `cancel_session` → generic "Authentication failed" inline → the same user's `create_session` again (tuigreet's soft reset), the password prompt back with the error still shown; the description never rendered |
| the conversation, ray | the controller ray clicks `Next` (592×88) and `Log in` (285×72) — the hit lands where the AT-SPI `Component.GetExtents` says the button is; a press whose release drifts off the button (the simulated head wobbles ≈20 px per 100 ms under a world-fixed ray) does not click, as a button should |
| the conversation, dwell | the scene's `Dwell click` toggle (`zxr ctl a11y dwell on` over `ZXR_CONTROL`, which zxr now exports to its trusted children) turns dwell on: `a11y: dwell commit kind=Head count=…` once a second. **On a head-locked plane the head ray always dwells on the plane's centre** and its commits take the pointer from the controller ray, so a controller-dwell on a button never settles; a dwell-only login is possible only with the scene on a frame the head does not carry (the wearer's placement table row) or with dwell restricted to the non-head rays — flagged (§9, a spatial-input item, not the greeter's) |
| the OSK | squeekboard maps on the field's `enable` (the `enter`→`enable`→`activate` chain of §7a) and is raised above the scene by zxr (phoc's rule, `osk_raised=true`); a click on its key reaches the focused field as `commit_string` (§7a); its exclusive band is respected by the zone-0 scene |
| kill mid-scene | `SIGTERM` to the program ⇒ `trusted primary gone … status=143` ⇒ zxr exits 143 within the tick (I3: nothing was unlocked; greetd's `Restart=always` is the restart) |
| lock mode (`--lock --lock-now`, the public socket, `mura-fake-authd` for the PAM helper) | `ext_session_lock_v1.lock` → zxr `Locked` → `locked` → `SetLockedHint(true)` → the conversation (`info` + `secret` in one batch: shown inline, the field masked) → wrong password → `failure{auth, 2000}` → generic text, retry after the delay → `kill -9` the locker ⇒ mode stays `Locked`, the scene opaque (I3) → a second instance locks again (`relocks=1`, cosmic-comp's rule) → right password → `success` → `unlock_and_destroy` → zxr `Normal`, `SetLockedHint(false)`, exit 0. The real `mura-authd` path is the same records over the same socketpair (its harness and the VM tests cover PAM) |
| AT-SPI | frame `Mura login` → panel `Login form` (entry `Username`, button `Next`; then the prompt label, the masked entry, `Cancel`, `Log in`), buttons `Power off`/`Restart`/`Sleep` (each behind logind `Can*`), panel `Accessibility` (`Dwell click`, `High contrast`, `Large text`); `Action.DoAction(0)` on `Log in` sends the response — the tree is the scene |
| cost, greeter mode | binary **11.3 MB** (nix, stripped; research/75 §5.4's winit program was 12.5 MB); **RSS 25.8 MB, PSS 13.7 MB, 5 threads** (main, accesskit_unix's, zbus's `async-io` executor + `blocking-1` + connection task); loop running → first frame **7 ms**; `start_session` accepted → zxr exit 3.7 ms; while a field is focused the caret blink redraws the dirty region twice a second (2 `shm_uploads`/s), nothing else moves |
| cost, lock mode | RSS 27.6 MB, PSS 14.5 MB, 6 threads (+ the conversation thread); `lock` → first frame 6 ms → `locked` 13 ms; a still scene: zero redraws |
| closure | 34 paths, 96.7 MB with fontconfig/freetype/libxkbcommon (libxkbcommon pulls libx11 — an nixpkgs feature flag to revisit); no interpreter (tests/closure.nix's fence holds with the program as a root); no image decoder (the scene carries no images; `EmbedFiles`) |

**What moved because of the gate:** (1) the greeter's exclusive zone is **0**, not cosmic-greeter's
−1 — cosmic has no OSK; Mura's is a sibling layer surface whose band the scene must leave
(flagged, rule 4); (2) zxr exports **`ZXR_CONTROL`** to its trusted children so the input floor's
toggles are one control-socket line from the scene (no bus); (3) no Slint fork after all: the
AccessKit translation is a crate beside Slint depending on `i-slint-core`'s internals at the
pinned version, and the compiler's `EmbedTextures` hack is sidestepped by carrying no images
(`EmbedFiles`), so the package patches nothing (shell-plane §4 corrected).

## 8. Verdicts against the tree

- `specs/session-auth.md` §5 — the open decision closes: the program speaks greetd (greeter
  mode); zxr relays `mura-authd` over an fd in env (lock mode); the session list is the module's
  file; §6 item 6a's harness is `fakegreet`.
- `docs/architecture/shell-plane.md` §3.1 — "Unit: started by zxr … its restart is its user
  unit's `Restart=on-failure`" is the Q1 contradiction; §3.1 "Seams: the socketpair only" gains
  the `GREETD_SOCK`/auth-fd line; "session list from `mura.xr.shell` (not a wayland-sessions
  scan)" and multi-user §2's ".desktop" reconcile as §4 says.
- `docs/architecture/adr/0007-session-greeter-lock.md` amendment — "its user unit restarts it"
  vs the socketpair channel: Q1.
- `docs/architecture/multi-user.md` §2 — the session-chooser sentence.
- `pkgs/zxr` — greeter mode needs the cage rule (exit when the trusted client is gone and the
  mode is `Greeter`); lock mode needs the relay fd (`--lock` spawn variant of
  `shell/filter.rs::spawn_trusted` with a second socketpair) once the lock machine exists; the
  program itself is a new package (`pkgs/mura-greeter`, Slint).
- `modules/os/session.nix` — G2's one-line swap (`greeterCommand`), plus the polkit rule for the
  greeter user's Wi-Fi (multi-user §2, research/11 §11.I), plus `XDG_CURRENT_DESKTOP` in the
  environments line if the program is to pass it.

## 9. Determinations and owner items

**Determined (converging comparables, acting):**

1. **Greeter mode: the program speaks greetd itself** over the inherited `GREETD_SOCK`; zxr
   relays nothing (agreety, gtkgreet, regreet, tuigreet, cosmic-greeter — 5/5; no relay
   comparable exists).
2. **zxr exits when its trusted greeter client exits, in greeter mode** (cage's rule; greetd's
   handoff and greetd's supervision are the same mechanism — ADR 0007's "greetd's
   default_session supervision").
3. **The conversation rules:** verbatim prompts, masked `secret`, immediate empty ACK of
   `info`/`error`, `auth_message{error}` stays in session, `error{auth_error}` → cancel +
   generic text + re-create for the same user, never the description (tuigreet's reason).
4. **Sessions from the module's file** (`/etc/greetd/environments`, already written), chooser
   hidden with one entry; `start_session.env` = `XDG_SESSION_TYPE=wayland`,
   the desktop names (values proposed, flagged).
5. **Power over logind D-Bus**, `Can*`-gated, confirm with timeout (cosmic).
6. **Lock mode — superseded 2026-09-28 (owner):** the first version of this determination had
   zxr own the `mura-authd` conversation and relay typed prompts to a socketpair child (LightDM's
   channel), which kept the ADR's hybrid alive and inherited Q1. **Ruled:** the lock program is
   a **resident user unit on the public socket** that locks through `ext-session-lock-v1`, waits
   for logind's `Session.Lock`, **owns its `mura-authd` conversation** (kscreenlocker's worker,
   swaylock's PAM child, cosmic-greeter's PAM thread — §3) and unlocks with `unlock_and_destroy`;
   the compositor's triggers fire `loginctl lock-session` (swayidle's exec shape) and the
   compositor keeps the lock on client death (every compositor, §3). The nonce discipline becomes
   the program's; the compositor never vetoes an unlock — no comparable does, and a trigger during
   a stale unlock simply re-locks. ADR 0007 amendment 2, session-auth rev 6, shell-plane rev 0.3.
7. **The G1 harness is greetd's `fakegreet`**, unmodified.
8. **One binary, `--lock` selects the mode** (cosmic selects by uid; zxr knows the mode).

**Q1 — who restarts the lock program when it dies — ruled 2026-09-28 (owner): the unit (option
(c) below, the COSMIC/wlroots shape), because the comparables are unanimous *within each channel*
— a socketpair child is supervised by its spawner (kscreenlocker, KWin's IM), a session component
on the public socket by the session's supervisor (cosmic-session, systemd units) — and the ADR's
hybrid existed nowhere. Greeter mode keeps the socketpair (greetd's kiosk); the OSK stays zxr's
child in every mode with KWin's bounded restart (shell-plane §3.2). The question as put:**
*Why a decision:* ADR 0007's
amendment says "its own user unit with `Restart=on-failure`", but a client admitted over a
socketpair is the compositor's child and no unit can hold that fd; every comparable with a
socketpair child supervises it itself — kscreenlocker three restarts then an emergency window
(`ksldapp.cpp:199-210`), KWin its input method. *Options (the comparables' positions):*
(a) **zxr restarts its child** with a bounded backoff and, exhausted, keeps the opaque scene
(kscreenlocker's shape minus the emergency window — nothing to draw, I3 holds; the ADR's
rejection of "hand-rolled four tries" was of the emergency window as much as the loop);
(b) **a user unit + a second, trusted listening socket** that only zxr's spawn knows the path
of — units restart, the trusted bit comes from the listener (smithay's per-listener `ClientData`,
research/77 §5.3); no comparable does exactly this, and any same-uid process could connect;
(c) **a user unit on the public socket with `ext-session-lock`** — swaylock/cosmic-greeter's
locker shape, the seam ADR 0007 keeps for the desktop profile; the lock is then the protocol's,
not the trusted connection's, and the OSK-on-lock needs the trusted bit anyway. *Consequence:*
(a) is one loop in `shell/filter.rs` and keeps the ADR's channel; (b) changes the admission
design; (c) changes the ADR. Greeter mode is unaffected (determination 2).

**Flagged at the G1 gate (rule 4; §7b), for the owner:**

- **F1 — the greeter's exclusive zone is 0.** cosmic-greeter and gtkgreet `-l` use −1 (cover
  everything; neither has an OSK). With squeekboard a sibling layer surface, zone 0 lets zxr
  size the scene to the area the OSK leaves, so the bottom controls stay reachable. Alternative:
  −1 and a layout that keeps the controls in the upper part (phosh's lockscreen shape). Chosen 0;
  one line in `pkgs/mura-greeter/src/main.rs`.
- **F2 — dwell on a head-locked plane.** The head ray always dwells on the plane's centre and
  its commits take the pointer, so a controller-dwell on the scene cannot settle while the scene
  is on the head frame. Two shapes, neither chosen here: the head ray does not dwell on members
  of the frame it carries (a head-locked plane's centre is not a choice the wearer made), or the
  scene lives on a frame the head does not carry (the wearer's `shell.place:mura-greeter` row).
  The input floor's promise (first-run-onboarding §4.4: head-aim + dwell alone can log in)
  depends on the answer. A spatial-input item, not the greeter's; comparables to be read before
  it is decided. **Resolved 2026-09-28 (owner ruling, spatial-input §13):** neither shape — the
  premise was wrong. Nothing the wearer aims at is head-locked: the scene lives on the **body**
  frame, the default for every surface that asks for nothing (PICO Head Control Mode, Cardboard,
  HoloLens tag-along, the visionOS HIG), and dwell is one global commit for every surface (KWin's
  dwell clicker), anchored on the hit point on the target in the plane's pixels, so a moving head
  over a still plane settles where it points. Gate 9 (j) re-measures.
- **F3 — `start_session.env` values** `XDG_SESSION_TYPE=wayland`, `XDG_SESSION_DESKTOP=mura`,
  `XDG_CURRENT_DESKTOP=Mura` (determination 4 said "proposed"): now written; the wrapper sets
  `XDG_CURRENT_DESKTOP` itself (session.nix), so the greeter's value is the same name.
- **F4 — the `ZXR_CONTROL` environment variable** for trusted children (the scene's dwell
  toggle is `a11y dwell on|off` on zxr's control socket). No comparable hands a control socket
  to its greeter; it is the harness's existing seam reused, not a new interface, and it is
  greeter-mode-only in practice (a lock program on the public socket does not get it).
- **F5 — the picker's display name is the GECOS full name**, and a `nologin`/`false` shell hides
  an account from the list (tuigreet/SDDM's filters). Free-text entry stays beside it.
- **F6 — `mura-greeter-lock.service` is `wantedBy graphical-session.target`** in every profile
  (the lock exists whenever a session does); ADR 0007's `session.lock.enabled` governs the
  *triggers*, not the unit.
- **F7 — last-user lives in the greeter's own directory (resolved 2026-09-28).** The program
  wrote `state/accounts/last-user` but nothing created the directory, and multi-user.md §2 called
  it root-owned state — no root writer exists (greetd runs the greeter as `greeter`). The
  comparables: regreet writes `/var/lib/regreet/state.toml` as the greeter user
  (`regreet/src/constants.rs:33-34`, `cache/mod.rs:52-63`), tuigreet `/var/cache/tuigreet/lastuser`
  (`tuigreet/crates/tuigreet/src/info.rs:26-27`); both directories are created `greeter greeter
  0755` by their NixOS modules (`programs/regreet.nix:176-188`, `greetd.nix:143-146`). SDDM's
  root daemon writing `state.conf` does not transfer. Done as a tmpfiles rule in
  `modules/os/session.nix` (tmpfiles runs after the account database; the persist skeleton runs
  before it), asserted in `tests/vm/multi-user.nix`. Per-user `<user>/` metadata is an open item
  (multi-user.md §2): not a `1777` directory — pre-auth names and avatars must not be forgeable
  by any local user; AccountsService's root daemon is the shape to read when avatars land.
- **F8 — the OSK follows the surface it types into, not a body seed (resolved 2026-09-28).**
  The gate showed squeekboard "comically large and close": the `osk` seed row (body, −35°,
  0.5 m, from wayvr's keyboard) resolved to the head frame because no body frame existed, and its
  numbers were a loose conversion of wayvr's (0, −0.65, −0.5) m anyway. Reading the sources: the
  body frame itself was a taxonomy entry in the wave-4 protocol draft (`698c151`) with the
  comparables fitted to it afterwards; research/36 §7 had already found the keyboards' real
  convergence — *bound to the focused panel* (WiVRn a fixed offset below its GUI,
  `wivrn/client/constants.h:87-88`; xrdesktop per window; visionOS/Quest near the field; Meta's
  virtual-keyboard API lets the runtime place it) — and wayvr was the outlier. Done: the
  placement table's `typed` value as the `osk` seed (spec §4 rev 3.14; shell-plane §3.2), the OSK
  under a world window arranged against the window's rectangle and hung below it with WiVRn's
  offset, and the body frame built as every comparable derives it (spec §5 rev 3.14) so `body`
  rows and the bar seed mean what they say. The bar seed keeps the old provenance — flagged in
  shell-plane §6.
- **F9 — a regression found and fixed (2026-09-28): the G1 commit wrote the contract's
  `multiUser.uidRange` (1000–60000) into `/etc/login.defs`**, and `vm-test-multi-user`'s login
  stopped working (the greeter's respawn stalled ≈20 s and the typed login was lost; bisected by
  reverting the two module changes one at a time — the lock unit was innocent, `security.loginDefs`
  was the cause). `nix flake check` builds the fixtures but does not run the VM tests, which is
  how it slipped. NixOS's own `UID_MAX` is 29999 because its `nixbld` uid range starts at 30000
  (`nixos/modules/misc/ids.nix`, `programs/shadow.nix:102-106`); Debian's 60000 overlaps it.
  Fixed: login.defs is left NixOS's; the contract default becomes 29999 with that reason; the
  option reaches the greeter as `MURA_UID_MIN`/`MURA_UID_MAX` in its environment (read by
  `accounts.rs` over login.defs) on G2's greeter command line. The exact mechanism by which the
  larger UID_MAX stalled the greeter session was not root-caused (flagged; the VM run is the
  evidence, twice). **Lesson recorded:** a module change under `modules/os/` needs the VM test
  that exercises it run before commit, not only `nix flake check`.
- **F10 — a second regression found and fixed (2026-09-28): `mura-greeter-lock.service` locked
  the session at every login.** `vm-test-default-image`'s D5 found it sideways (its `pgrep
  mura-authd` picked up the *lock's* helper). Two mistakes in the program's start-up: (1) the
  session lookup used `GetSessionByPID(0)`, which cannot work from a systemd user unit
  (`user@UID.service` is outside every session scope; cosmic-greeter's `GetSessionByPID(parent)`,
  `cosmic-greeter/src/logind.rs:91-93`, works only because its parent is cosmic-session *inside*
  the scope); (2) "no session found ⇒ lock now" was my misreading of cosmic's fallback — cosmic
  locks immediately only when **logind itself is unreachable** (`locker.rs:721-726`), re-locks at
  start only when its lockfile says the session was already locked (`:712-717`), and a failed
  session lookup is a warning (`logind.rs:94-96`). Fixed to cosmic's rules: the session is
  `XDG_SESSION_ID` or logind's `User.Display` (the property logind keeps for exactly this
  question), `LockedHint` true at start re-locks (the compositor or the locker came back while
  locked — the hint this program sets), logind unreachable locks, a missing session warns and
  stays resident. Verified on the host from outside the session scope (`Display=2`, resident,
  no lock) and by both VM fixtures.

## 10. Sources

Pinned clones (`references/MANIFEST.json`): `greetd` (`greetd/`, `greetd_ipc/`, `agreety/`,
`fakegreet/`, `man/`), `gtkgreet`, `regreet`, `tuigreet`, `cosmic-greeter` (+ `daemon/`, `debian/`,
`cosmic-greeter-config/`), `sddm`, `lightdm`, `gdm`, `gnome-shell`, `kscreenlocker`, `kwin`, `cage`,
`phosh`, `squeekboard`. Repo: `modules/os/session.nix`, `specs/session-auth.md`,
`docs/architecture/{multi-user,shell-plane,first-run-onboarding}.md`, ADR 0007/0018,
research/11, /12, /41, /75, /77. Pin caveats recorded: this SDDM has no `PamConvMsg` (prompts do
not reach its QML greeter); this kscreenlocker has no `--ksldfd` (the unlock is the exit code).
