# 54 — First-run authority: how shipping Linux first-run flows let the first user set system state

**Status:** research, 2026-09-24. Written to settle one recurring question once, from source:
*how does a first-run flow set time zone, hostname, network and accounts for a first user who has
no password yet?* Mura's default image autologins a passwordless `mura` ([first-run-onboarding.md §1](../architecture/first-run-onboarding.md)),
and systemd's `timedate1.set-timezone` / `hostname1.set-static-hostname` are `auth_admin_keep`
even for an active session (`references/systemd/src/timedate/org.freedesktop.timedate1.policy:32-38`).
The design corpus went back and forth on this (first-run rev 2.5 §4.2 item 6 "after the password
card"); this document replaces that reasoning with what comparable projects ship.
**Method** ([docs/README.md](../README.md) rules): code study of pinned clones in `references/`
(MANIFEST.json) with file:line citations; [external] where no clone exists. Design conclusions are
limited to adopt/reject *candidates*; the ruling is the project owner's (§5).
**Budget impact:** none — a research document.

## 1. The question, stated as steps

A first-run flow is a sequence of steps; some touch only the user's own state (language, IPD,
peripherals), some touch *system* state (time zone, hostname, system-wide network, accounts). For
each project below: which steps exist, in what order, and **who performs the system steps under
what authority** — a dedicated setup identity with polkit rules; a `pkexec` helper with its own
polkit action; a per-user setting instead; automatic derivation; deferred to settings; or simply
not offered to a logged-in user.

## 2. Survey

### 2.1 gnome-initial-setup (GNOME, Fedora, Endless, Debian…) — `references/gnome-initial-setup`

Two modes, one binary (`gnome-initial-setup/gnome-initial-setup.c:218-249`):

- **New-user mode** runs *before any user exists*, under the dedicated `gnome-initial-setup`
  account in a GDM-hosted session. Page order (`gnome-initial-setup.c:64-77`): welcome → language
  → keyboard → **network** → privacy → **timezone** → software → account → password →
  (parental controls) → summary. System steps are authorised by
  `data/20-gnome-initial-setup.rules.in:8-30`: any member of group `gnome-initial-setup` gets
  `YES` for whole action prefixes — `org.freedesktop.hostname1.*`, `NetworkManager.*`,
  `locale1.*`, `accounts.*`, `timedate1.*`, `realmd.*` — when `subject.local`, `auth_admin`
  otherwise.
- **Existing-user mode** runs after login as the real user (autostart). The page table marks
  pages `new_user_only`: **timezone, software, account, password, parental controls are never
  shown to a logged-in user** (`gnome-initial-setup.c:69-75`, the `TRUE` column). What remains is
  language, keyboard, network, privacy, summary — all per-user or already-allowed actions.
  Vendors tune this with `vendor.conf` (`[pages] skip=… existing_user_only=… new_user_only=…`,
  `README.md:104-109`).
- **Time zone is derived, then confirmed**: the page asks geoclue for a location and calls
  `timedate1.SetTimezone(tzid, interactive=TRUE)` (`pages/timezone/gis-timezone-page.c:42-47,
  116-130, 191-227`); the network page comes first so geoclue has connectivity.
- Network: `nm_client_add_and_activate_connection_async` with no explicit permissions
  (`pages/network/gis-network-page.c:602-606`) — the setup identity's grant makes it a system
  connection in new-user mode; a logged-in non-admin gets whatever NM's policy gives them.

**Shape:** system steps belong to a *setup identity before login*; a logged-in user's welcome
never contains a system step. Time zone is automatic with a confirm.

### 2.2 SteamOS 3 (Steam Deck) — `references/jupiter-hw-support` (tag `jupiter-20260914.1`, the source Jovian-NixOS packages)

The Deck's `deck` account is passwordless and autologged-in — Mura's exact posture. Valve's
first-run (Steam client in gamescope: language → time zone → Wi-Fi → Steam login [external:
Steam Deck OOBE]) sets system state through **`pkexec` helpers with their own polkit actions**:

- `usr/bin/holo-polkit-helpers/holo-set-timezone` — re-execs itself via
  `pkexec --disable-internal-agent`, then `timedatectl set-timezone "$1"`
  (`holo-set-timezone:1-10`); `holo-set-hostname` likewise → `hostnamectl set-hostname`
  (`holo-set-hostname:1-10`). Sixteen such helpers exist (`ls usr/bin/holo-polkit-helpers`:
  devkit mode, enable sshd, format device, select branch, update, reboot, …).
- Each has a dedicated action in `usr/share/polkit-1/actions/org.valve.holo.policy` with
  **`allow_any=yes`, `allow_inactive=yes`, `allow_active=yes`** — no password, no session-state
  condition, for anyone (`org.valve.holo.policy:88-109`). The companion rules file only widens
  udisks actions for `wheel` (`usr/share/polkit-1/rules.d/org.valve.holo.rules:3-17`).
- These are **permanent**, not first-run-scoped: the Deck's settings UI uses the same helpers
  later. Valve decided that on a personal device the seat user may always set the time zone and
  hostname.

**Shape:** the seat user gets *specific* system actions for free, forever, through narrow
helpers; the general `auth_admin` posture stays for everything else.

### 2.3 elementary OS initial-setup — `references/elementary-initial-setup`

Runs **before login as the `lightdm` user** (`data/initial-setup.desktop.in:4` `Exec=pkexec
io.elementary.initial-setup`). Pages (`src/Views/`): language → keyboard → network → account.
**No time-zone step.** System steps use a rule scoped to `subject.user == "lightdm" &&
subject.local && subject.active` granting exactly `hostname1.set-hostname`,
`hostname1.set-static-hostname`, `accounts.user-administration`,
`io.elementary.pantheon.AccountsService.ModifyAny` (`data/io.elementary.initial-setup.rules:1-20`);
the hostname view checks `Polkit.Permission` for `set-static-hostname` before calling
(`src/Views/AccountView.vala:381-397`). Time zone is left to the settings app after login, where
elementary's Date & Time panel asks for admin authentication like GNOME's.

**Shape:** pre-login identity with an exact action list; no system step is offered in-session.

### 2.4 Lomiri / Ubuntu Touch — `references/lomiri-system-settings`

The wizard is a separate package (`lomiri-system-settings-wizard`, not cloned — [external]) that
runs as the single `phablet` user. The time-date plugin calls
`org.freedesktop.timedate1.SetTimezone(tz, interactive=false)`
(`plugins/time-date/timedate.cpp:33-38, 169`) — it *assumes* polkit says yes without a prompt,
which Ubuntu Touch arranges distribution-side for `phablet` [external: UT polkit localauthority
overrides]. The repo itself ships no polkit rule (`find … -name '*.pkla' -o -name '*.rules'`
empty).

**Shape:** SteamOS's — the one seat user may set the time zone unauthenticated — done by
distribution policy rather than a helper.

### 2.5 Calamares — `references/calamares`

An installer: `SetTimezoneJob` runs as root in the target (`src/modules/locale/SetTimezoneJob.cpp:38-53`,
`timedatectl set-timezone` or a `/etc/localtime` symlink). No first-*boot* authority question
arises because the installer *is* root. Mura has no installer; this is the case the corpus calls
"the image is the installation".

### 2.6 plasma-welcome (KDE) and phosh-tour (Phosh) — `references/plasma-welcome`, `references/phosh-tour`

Both are **tours** for the logged-in user: plasma-welcome opens KCMs (`src/app.cpp:86`
`KAuthorized::authorizeControlModule`) and performs no system writes of its own; phosh-tour's
pages are informational (`src/pt-*-page.c`). Time zone is left to settings (KDE's Date & Time KCM
authenticates via KAuth; Phosh via the GNOME panel's polkit prompt).

**Shape:** a post-login welcome does not touch system state at all.

### 2.7 Summary table

| Project | Where system steps run | Authority for time zone / hostname | In-session welcome touches system state? |
|---|---|---|---|
| gnome-initial-setup | pre-login, setup identity | group rule, action prefixes (`20-gnome-initial-setup.rules`) | **no** — those pages are `new_user_only` |
| SteamOS | in the seat session (passwordless `deck`) | `pkexec` helpers, `allow_any=yes`, permanent | **yes**, via helpers |
| elementary | pre-login, `lightdm` identity | rule with exact actions (hostname, accounts); no time zone | **no** |
| Ubuntu Touch | in the seat session (`phablet`) | distro polkit override, `interactive=false` | **yes** |
| Calamares | installer as root | root | n/a |
| plasma-welcome, phosh-tour | in-session tour | none | **no** |

Two camps: **(A) system steps happen under a setup identity before any login and are never
offered in-session** (GNOME, elementary); **(B) the single seat user of a personal device may
set a short, named list of system things without a password, permanently** (SteamOS, Ubuntu
Touch). Nobody ships "type your password to confirm the time zone" in a first-run flow, and
nobody orders steps around a password that might not exist.

## 3. Mura's flow, step by step, both instances

The `mura-setup` program runs as two instances sharing one library
([first-run-onboarding.md §5.1](../architecture/first-run-onboarding.md)): the **system
instance** (own identity, scoped polkit grant — camp A's setup identity, but out-of-band over
cable/hotspot rather than pre-login on the display) and the **session instance** (the
in-headset welcome surface, as the logged-in user). Authority per step today:

| Step | Touches | System instance (web app) | Session instance (in-headset) |
|---|---|---|---|
| 1 See — IPD | user calibration | n/a (needs the headset) | own files |
| 2 Walk — peripherals | BlueZ pairing | BlueZ default policy | BlueZ default policy |
| 3 Speak — language | the account's language | AccountsService `user-administration` (grant) | `change-own-user-data` = yes |
| 4 Connect — Wi-Fi | NM connection | `settings.modify.system` (grant) → system connection | `settings.modify.own` → user-scoped; system scope when polkit allows (ruled 2026-09-24) |
| 5 Secure — password | the account's password | AccountsService `user-administration` (grant) | `passwd` (nullok) |
| 6 **Time zone / hostname** | **system** | `timedate1.set-timezone`, `hostname1.set-static-hostname` (grant) | **`auth_admin_keep` — the open question** |
| 7 Finish | the marker | sticky `state/setup/` | sticky `state/setup/` |

Only step 6 in the session instance lacks a standard authority. Every other step is either
per-user or already granted.

## 4. Candidates for step 6, judged on UX first

The wearer's experience is the criterion ("good UX wins"). The step is a *confirmation*: the
zone can be derived — from the phone's own time zone when setup runs on the web app (the
browser's `Intl` zone, free, exactly as `Accept-Language` gives the language), from the joined
network's country or geoclue in-headset (GNOME's mechanism), and the device's clock is NTP-set
regardless. Nobody should have to type anything for it.

| # | Candidate | Precedent | Wearer sees | Cost |
|---|---|---|---|---|
| (i) | **Derive automatically in the system instance; in-headset card is display + "change in settings"** | GNOME existing-user mode never shows the page; g-i-s derives via geoclue | a card saying "Time zone: Europe/Berlin — from your network" with no action, or nothing | an override in-headset still needs (ii)/(iii) or the settings app's admin prompt; on a headset that never touches the web app and has no location source, the zone stays the image default until a password exists |
| (ii) | **A Mura polkit rule granting active local sessions exactly `timedate1.set-timezone` and `hostname1.set-static-hostname`/`set-pretty-hostname`** | SteamOS (`holo-set-timezone`, `holo-set-hostname`, `allow_any=yes`); Ubuntu Touch | one-tap confirm/override in-headset, no typing, any order; the same in settings later | a Mura loosening of systemd's default for two low-harm actions; permanent (as Valve's is); scoped tighter than Valve's (`subject.local && subject.active`, not `allow_any`); the same shape as the greeter's NetworkManager rule already shipped |
| (iii) | SteamOS-literal: `pkexec` helper scripts with their own actions | SteamOS | same as (ii) | an extra binary and action namespace for what a rule on the standard action does directly; no benefit over (ii) on a systemd/polkit system |
| (iv) | Card after the password; polkit prompt | none — no shipping first-run flow does this | types the password twice; the card vanishes if the password is skipped | the design the corpus had; rejected by the survey |

**Recommendation [mine]: (i) + (ii).** Derive first (system instance from the phone or network;
in-headset from geoclue/network when available), so the common case asks nothing; grant the
two actions to active local sessions so the in-headset confirm/override — and the settings app
afterwards — never prompt. Step 6 moves to **after Connect** (so it can be derived) and is
independent of Secure. This is camp B narrowed to two named actions, with camp A's automatic
derivation in front of it. It adds one Mura polkit rule beside the greeter's; multi-user.md §3.1
lists every rule and would gain this row.

**Rejected [mine]:** (iv) outright; (iii) as redundant on polkit; widening (ii) to `allow_any`
(Valve's choice — a non-local or inactive session has no business changing the clock); widening
it to `locale1`/`accounts`/NM system (those steps already have standard per-user paths).

## 5. Decider

**Ruled 2026-09-25 — (i)+(ii) in direction, with (ii)'s shape re-derived.** Under AGENTS rule 7
this survey's reading of (ii) was incomplete: it took SteamOS's helper as *the* precedent without
its reason and without the desktop comparables. [research/56 §9](56-defaults-from-comparables.md)
adds them (Ubuntu's `policykit-desktop-privileges`: admin group + active session, with a stated
reason; pmOS Plasma's grant-everyone rule; phosh's `active && local && group` shape; GNOME/KDE's
auto-zone features no-oping or prompting on stock defaults) and lands the rule as
`50-mura-timedate.rules` for `wheel` members in active local sessions — not "active local
sessions" alone, which no shipping system does. The original text of this section follows.

The project owner rules on §4. Consequences of (i)+(ii) for the corpus: first-run §4.2 item 6
reworded (derived; after Connect; no password dependency) and §4.3 row updated; multi-user §3.1
polkit table gains `50-mura-session-timedate.rules`; ADR 0017 decision 3 amended and the
"after the password card" text struck; `modules/os/policy.nix` gains the rule (D-track; no rung
dependency — F2 consumes it after M1). Nothing in D3/D5/D6 waits on this.

## 6. What this survey did not cover

Android, visionOS and Horizon OS first-run flows — consumer platforms with no polkit and a
vendor account model; anti-pattern by AGENTS.md rule 2 and not evidence for authority design.
Fedora's and Endless's `vendor.conf` contents [external; not cloned] — they configure page skips,
not authority, and g-i-s's own page table already answers the question.
