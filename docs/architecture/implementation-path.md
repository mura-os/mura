# The implementation path: distribution groundwork and the compositor, in dependency order

**Status:** accepted plan of record (2026-09-23; rev 2 same day — the boot-to-desktop coverage
review absorbed: stages B1a/B1b/B6a/B9, the F-track from
[first-run-onboarding.md](first-run-onboarding.md), and the lifecycle section; rev 3 / 3.1,
2026-09-24 — ADR 0017 rev 2 and the research/42 review absorbed; **rev 4, 2026-09-24 — two
axes**: the compositor rungs (R0/G/M) are joined by a **D-track** of NixOS distribution
groundwork with no compositor dependency, verified in the rung-2 VM with stand-ins; §2 regrouped
by dependency class; G2 reduced to a recorded swap; the pre-groundwork specifications named in
§5.1; **rev 4.1 same day — F2/F3/D2/D3 absorb first-run rev 2.5 / ADR 0017 rev 2.4: sshd
upstream on every profile, `mura-setup` one program in two instances, the `setup-complete`
marker, Cockpit dropped**).
**What this is:** the ordered build path from power-on to a zxr session, derived from the
dependency graph ([desktop-environment.md §6](desktop-environment.md)) — not a replacement for
it. Rungs are ordered only where a hard dependency exists; everything else is a parallel track.
**Base:** Rust + smithay, ratified ([ADR 0006 as amended](adr/0006-compositor-strategy.md);
evidence [research/39](../research/39-compositor-base-landscape.md)).
**Budget impact** (overview invariant 9): none at design time; every rung below inherits the
[budgets.md](budgets.md) partitions when it lands code, and R0's exit evidence includes the
first real frame-path measurements (GPU time, missed `xrWaitFrame` deadlines) that seed the
class-V (VM) budget column with data instead of estimates.

## 1. Two axes, and why the restricted modes are still the first compositor target

**What exists in code today** (rev 4, honest): the device contract and its tests
([lib/contract](../../lib/contract/default.nix), [tests/contract.nix](../../tests/contract.nix)),
a 37-line [modules/os](../../modules/os/default.nix), a 34-line [modules/xr](../../modules/xr/default.nix)
(the pinned nixpkgs provides `services.monado`, `services.greetd`, `services.cage`), the
uefi-rauc family with the persist skeleton, two device files, the protocol XMLs and specs, and
[pkgs/dev-session](../../pkgs/dev-session/default.nix) running sway. The design corpus
specifies far more than that. Almost all of the near-term difference is **NixOS module work
with no compositor dependency** — greetd and autologin from the contract, PAM/polkit posture,
persist classes and F1 units, out-of-band access, the session wrapper, mark-good — every piece
testable in the rung-2 VM with **sway as the stand-in session and gtkgreet+cage as the stand-in
greeter**. That is the **D-track** (§3c). It runs in parallel with the compositor axis and is
what makes the greeter milestone small when the compositor arrives.

**The compositor axis is unchanged in its rationale**: zxr's restricted modes are the cheapest
real compositor milestone, because ADR 0007 *disables* the client Wayland listening socket in
them ([specs/session-auth.md §5](../../specs/session-auth.md)). A restricted mode needs the
OpenXR loop, the Vulkan renderer, an internal (non-client) scene, input, and a greetd IPC
client — none of the 2D client tier, no protocol server, no window model, no Xwayland — and
every one of those pieces is the irreducible core of the session compositor. The restricted
modes are exactly two — `--greeter` and the lock scene it shares machinery with. The first
compositor build target is:

- **`zxr --greeter`** — the every-boot login scene (multi-user profile), an ordinary Linux
  greeter ([multi-user.md §2](multi-user.md)), operable at the input floor
  ([first-run-onboarding.md §4.4](first-run-onboarding.md)). G1 in §3. There is **no `--oobe`
  sibling**: first-run setup is shell-plane session content (F2, the welcome surface —
  [first-run-onboarding.md §4](first-run-onboarding.md), contents decided), which lands
  downstream of M1 like any other shell content.

**The stand-in rule (stated once, applies everywhere):** sway is the stand-in *session*;
`cage` + `gtkgreet` is the stand-in *greeter*. Both are development fixtures carrying the same
footnote `pkgs/dev-session` already carries ("the nested compositor is sway until zxr's M1
lands; swap `COMPOSITOR_CMD` then"). They are never decisions and **never in a shipped image**;
the swap-out points are G2 (greeter) and M1 (session), each recorded as an exit criterion.

**The first-profile question, settled:** the **default image is the first *shipped* image** —
a declared configuration (`profiles/default.nix`: user `mura`, no password, `wheel`,
`autoLogin = "mura"`), no greeter at all; the image is the installation (ADR 0017 rev 2). **G2
remains the first *greeter* milestone**, not the first image: the multi-user chain (greetd →
greeter → PAM → session wrapper → session) holds the hard ordering problems — seat handoff,
PAM authority, device release — and rev 4 proves them **on the D-track with the stand-in
greeter (D0/D4)** so that G2 becomes the swap of one line. The end state that first *feels* like
Mura is unchanged: the VM powers on, lands in an XR auth scene with zero manual steps, and login
hands off to a real session.

## 2. The boot chain, grouped by dependency class

Each stage lists what exists today and what must be built. "VM" = the rung-2 virtual headset;
stages marked ▲ are forced decisions this path surfaces. Classes: **(i) boot and persistent
state**, **(ii) the login chain**, **(iii) out-of-band access and policy**, **(iv) compositor
deliverables**, **(v) health**. Classes (i)–(iii) and (v) are D-track (compositor-free);
class (iv) is the compositor axis.

### (i) Boot and persistent state

| # | Stage | Exists today | To build |
|---|---|---|---|
| B1 | Firmware → bootloader → initrd | per-family (uefi-rauc proven in the Frame workstream; android-bootimg gated on the Lynx spike) | nothing for this path — the VM boots systemd-boot already |
| B1a | Persistent state + hardware readiness | uefi-rauc mounts `syspersist` rw with `/var/lib/mura` bound via `mura-persist-setup.service` (pull-in dependency, not tmpfiles ordering) and the state-class skeleton (`factory/ identity/ enrollment/ pairing/ state/`; machine-id its own class — [first-run-onboarding.md §2](first-run-onboarding.md)) | the VM device gains the same persist layout; validation of the mounts + **factory** calibration presence/version *before* Monado starts; firmware/module/udev discovery with a device-wait timeout policy; machine-id committed from `/persist` **before D-Bus/logind start**; `/var/lib/bluetooth` bound onto `pairing/` when `mura.hardware.input.bluetooth`. Device access is **logind/libseat ACL acquisition, never permanent group membership** — the seat broker grants/revokes DRM+evdev per session; only nodes logind cannot broker (hidraw/IMU/camera) get narrowly scoped per-VID/PID udev rules (`TAG+="uaccess"` or a `mura-xr` group documented as seat-revocation-exempt, with rationale). An explicit stage, not "NixOS default" |
| F1 | First-boot machine provisioning | uefi-rauc state skeleton (§B1a) | silent provisioning per [first-run-onboarding.md §3](first-run-onboarding.md): per-unit keys, settings-store seeding, partition growth. Each unit gated on its **own durable per-task marker on `/persist`, not `ConditionFirstBoot`** (a fresh A/B root slot looks like first boot to the latter); idempotent units + atomic markers = interrupted-first-boot recovery. No marker gates any UI |

### (ii) The login chain

| # | Stage | Exists today | To build |
|---|---|---|---|
| B2 | ▲ Seat broker | ADR 0007 names logind/seatd as the DRM-master/hidraw broker and leaves "logind vs seatd on the appliance image" open | **forced at D0**: greetd's session worker needs a seat. Default: logind (NixOS default, zero work, `SetLockedHint` needs it anyway per ADR 0007, and B1a's ACL model assumes it); seatd remains an appliance-minimization option to revisit with image-size work |
| B3 | greetd from the contract | contract options `mura.xr.session.{autoLogin,greeter,allowNoDeclaredAccount}` + profile-exclusivity and declared-account assertions ([lib/contract](../../lib/contract/default.nix)); research [11](../research/11-display-managers-greeters.md); the VM currently bypasses this (getty autologin → `exec sway` in [devices/virtual-headset](../../devices/virtual-headset/default.nix) lines 67-71, with permanent `video`/`input` membership and `password = "mura"` — **the block D0 replaces**) | `modules/os/session.nix`: `services.greetd` for both profiles — appliance `initial_session` autologins the declared user into the session; multi-user `default_session` runs the greeter **directly** as the `greeter` user (`cage -s -- gtkgreet` until G2, `zxr --greeter` after — the line carries `# STAND-IN — replaced at G2 by zxr --greeter`). **No dispatcher, no runtime-state session selection** (ADR 0017 rev 2) |
| B5 | Login authority | greetd's session worker is the **sole** login PAM authority; the greeter is an unprivileged greetd IPC client (session-auth §1, review-hardened) | PAM stacks declared via NixOS modules per the [first-run-onboarding.md §5.3](first-run-onboarding.md) posture table: **one credential**, `allowNullPassword` on greetd and the lock (a digits-only password selects the digit-pad rendering via the mirrored `numeric-credential` hint, ADR 0018 rev 3.1, multi-user.md §3), **standard sudo/polkit** (admin requires a password), faillock on `/persist`, the guest-scoped gated branch where guest is enabled (multi-user.md §4). The greeter's greetd client half (`create_session` → `post_auth_message_response` → `start_session`) is the compositor's (class iv) |
| B6 | Session start | `mura.xr.shell` contract enum (zxr/stardust/wayvr/kwin-vr/none) | `mura-session.target` (systemd user target owning Monado + compositor + shell services; crash/restart semantics per ADR 0007), enumerating sessions from the module system; sway as the stand-in member until M1 |
| B6a | User-session bootstrap contract | **implemented at D4, ported at D4 rev 3**: the wrapper is **`mura-session`** (`pkgs/mura-session`, Rust, libc only; uwsm was the D4 evidence and left under AGENTS rule 6 — three Python interpreter starts, unit generation and a `daemon-reload` per login, ~4 s of the VM's 5.96 s login), verified in both VM fixtures; [specs/session-bootstrap.md](../../specs/session-bootstrap.md) rev 3 | what D4 proved and what remains: `pam_systemd` establishes the login session; the compositor runs as the static `mura-compositor.service` and acquires the seat from inside the user unit through `$XDG_RUNTIME_DIR/mura/session.env` (`XDG_SESSION_ID` to the unit only — the libseat fallback); environment in three classes — *static* via `environment.d` + the wrapper's explicit `set-environment`; *compositor-created* (`WAYLAND_DISPLAY`) published **after** readiness by `mura-session finalize` (sway) / native `sd_notify` (zxr); *dependent* services on the standard `graphical-session(-pre).target`, `mura-session.target` the wrapper's `--wait` target, ordered before `graphical-session.target`. Manager-correct lifetimes: the wrapper process in the session scope is the only cross-manager coupling and returns only after teardown (verified: logout → greeter, no DRM race). Crash restart inside the session needs `RestartMode=direct` (systemd ≥ 254 fires `OnFailure=` per failure). **Remaining for G3**: zxr calls `sd_notify` itself and the sway `finalize` line leaves; the greeter-Monado→session-Monado handoff (the stand-in greeter runs no Monado) |

### (iii) Out-of-band access and policy

| # | Stage | Exists today | To build |
|---|---|---|---|
| F3 | Out-of-band access | sshd **on every profile with upstream defaults** (`modules/os/default.nix`, D2 posture correction; VM-tested: password auth everywhere, the empty password refused, a declared fixture key logs in from first boot); pmOS pattern studied (`references/pmaports`, `references/pmbootstrap`); the posture table [first-run-onboarding.md §5.3](first-run-onboarding.md) | per [first-run-onboarding.md §5](first-run-onboarding.md): USB Ethernet gadget from the initramfs + `systemd-networkd` DHCP (NM-unmanaged link); the **provisioning hotspot** condition-shaped on "`setup-complete` absent and no other active NM connection", WPA2 with the in-headset PSK, per-boot idle timeout (NM AP/shared mode, `dnsmasq-shared.d` wildcard + DHCP option 114, probe redirect); **`mura-setup`** — the bespoke setup program's system-service instance (own identity, scoped polkit rules, the launcher + web app on port 80 of the gadget + hotspot addresses, never the LAN; ADR 0017 decision 10) — Cockpit is dropped. The gadget + hotspot + service half has no compositor dependency; the PSK display does (a compositor scene) — until G1 the PSK is also printed to the serial console/journal on the dev profile |
| F4 | Input floor (policy half) | research/42 §4 covers UX/policy; [research/45](../research/45-imu-3dof-monado-native-linux-audit.md) covers the per-target native IMU→Monado chain and [research/53](../research/53-proximity-presence-native-linux-audit.md) presence; contract `mura.hardware.input.*` has `selectRole`/`backRole` assertions | `HandlePowerKey=ignore` (or a session inhibitor) so the compositor owns the power key via libinput; the constraint-7 stabiliser and button handling are compositor-owned; **a runtime-qualified Monado 3DoF HMD driver per target** is each in-headset greeter's prerequisite — physical/stock IMU and a node/service alone do not satisfy it |

### (iv) Compositor deliverables

| # | Stage | Exists today | To build |
|---|---|---|---|
| B4 | `zxr --greeter` | the mode's restrictions and exit contract are normative ([session-auth §5](../../specs/session-auth.md)); per-unit calibration paths in the contract; safe default IPD pre-auth; the standard furniture set (multi-user.md §2, research/11 §11) | the binary itself: G1's deliverable (§3), running on R0's core; swapped in for the stand-in greeter at G2 |
| B7 | The session | rung-1/rung-2 loops run sway as the stand-in session | zxr session mode: M1 onward (§3) |
| F2 | First-session welcome surface | design in [first-run-onboarding.md §4](first-run-onboarding.md); ADR 0017 rev 2.4 | shell-plane session content (downstream of M1's window model) **and the session instance of `mura-setup`** (one library with the D3 web-app instance; ADR 0017 decision 10): per-item gated, skippable, re-runnable; **see → walk → speak** (IPD language-free per `ipd.source` class → peripherals → the account's language → Wi-Fi/skip → time zone + hostname (derived after Connect; in-headset authority per research/54, pending ruling) → one password/skip via `passwd` → finish, which writes `setup-complete`; dismiss = finish); every item operable at the §4.4 input floor; runs as the logged-in user with active-session authority only (§4.3, verified per card). **`mura-provisiond`** is left with the guest token gate only |
| B8 | Lock | lock state machine + invariants specified (ADR 0007, session-auth §2–§3); `ext-session-lock-v1` dev-profile-only | `mura-authd` + the in-compositor lock states — authd is D-track (D5, testable against sway); the lock *scene* is the compositor's |

### (v) Health

| # | Stage | Exists today | To build |
|---|---|---|---|
| B1b | XR preflight + recovery ladder | **implemented at D6** (`modules/os/health.nix`: `mura-preflight` P1–P7 before greetd, `mura-crashloop` counter, `mura-recovery.target`; VM-verified incl. the forced ladder); **ruled 2026-09-25** ([research/56 §3](../research/56-defaults-from-comparables.md)): the counter stays for the cross-slot gap systemd's boot counting cannot see (a persistent state fault in an already-good slot), feedback moves to the *first* hard failure (the plymouth message, §4 track), and the counter's step at the threshold is an automatic reboot into the Mura recovery environment (§4 track, landed); where the family provides no entry, the stage-2 `mura-recovery.target` — the failure-feedback state — instead. Flat-output fallback on a docked connector is the docked-mode rung's; the greeter's a11y exposure of a soft result is G1's | gate before greeter/session start: runtime-created Vulkan device, GPU match, factory-calibration validity, backend-defined DRM/IMU transport enumeration, advancing sensor/pose proof, and a dedicated Monado probe compositor/client reaching stable per-eye presentation. Profile-dependent accessory checks remain qualification-only until attachment policy exists. Crash-loop threshold → flat/SSH/diagnostic fallback; failure must never leave a permanently dark headset |
| B9 | Session-ready gate + update mark-good | **implemented at D6**: `mura-readiness` (blessing tier) `RequiredBy=boot-complete.target` on every profile; uefi-rauc arms `+N` in `set-primary`, `systemd-bless-boot` strips it, `mura-mark-good` tells RAUC — three transitions; `mura.qualification.readinessCheck` stays the device's named check the tier consumes | see §3a: readiness tiers, the mark-good service, and systemd-boot boot-counting wired explicitly in the uefi-rauc family |

## 3. The rung ladder

```mermaid
flowchart TD
    subgraph dtrack [D-track: distribution groundwork, compositor-free, VM-verified]
        D0["D0 session from the contract\ngreetd: autologin -> sway | cage+gtkgreet stand-in\nprofiles/, Monado wired, two VM fixtures"] --> D1["D1 persist + F1 in the VM\nclasses incl. pairing/, machine-id class\nper-task markers"]
        D0 --> D2["D2 policy module\nPAM posture table, HandlePowerKey=ignore\ngreeter NM rule, faillock on /persist"]
        D1 --> D3["D3 out-of-band F3\nUSB gadget (dummy_hcd or renamed NIC)\nPSK hotspot + portal (mac80211_hwsim)\nmura-setup service, scoped polkit, setup-complete"]
        D2 --> D3
        D0 --> D4["D4 session wrapper B6a\nmura-session.target around sway\nspecs/session-bootstrap.md"]
        D4 --> D5["D5 mura-authd + lock path\n(sway session)"]
        D1 --> D6["D6 preflight B1b + mark-good B9\nboot counting in uefi-rauc"]
    end
    subgraph ctrack [Compositor axis]
        R0["R0 bring-up spike\nsmithay skeleton in the dev-session slot\n4 measured gates (doc 39 s5)"] --> G1["G1 zxr --greeter in dev-session\nOpenXR loop + ash renderer + auth scene\nfake greetd; input-floor exit criteria"]
        R0 --> M1["M1 spatial 2D desktop in a window\nxdg-shell, ray-to-pointer, move/rotate/resize"]
        G1 --> G2["G2 = the swap\nzxr --greeter replaces the stand-in in D0's module\ngtkgreet leaves the closure"]
        M1 --> F2["F2 welcome surface\nshell content, per-item gated"]
        M1 --> M2["M2 mixed 2D/3D composition\nzxr-shell-v2 protocol goes live"]
        G2 --> G3["G3 full handoff\nzxr greeter -> wrapper -> zxr session"]
        M1 --> G3
        M2 --> M3["M3 renderer-agnostic proof"]
        M3 --> M4["M4 headset output via Monado"]
    end
    D0 -.-> G2
    D4 -.-> G3
    D5 -.-> G3
    D6 -.-> B9["B9 session-ready gate\nmark-good + boot-counting (s3a)"]
    G3 --> B9
    SETTINGS["parallel: settings daemon (VM testbed)"] -.-> G3
    PERC["parallel: perception intake harness"] -.-> M4
```

### 3c. The D-track — distribution groundwork (compositor-free)

Every rung is NixOS module work verified in the rung-2 VM. Two **VM fixtures** exist from D0 on
and every later rung must pass on both: the *default-image shape* (`profiles/default.nix`:
`mura`, no password, autologin into the stand-in session) and the *multi-user shape*
(`profiles/multi-user.nix` + a declared `hashedPasswordFile` user, the stand-in greeter). The
security acceptance checks are [first-run-onboarding.md §8](first-run-onboarding.md) items 12–15.

| Rung | Deliverable | Depends on | VM verification |
|---|---|---|---|
| **D0** | `profiles/{default,multi-user,dev}.nix` ([repo-structure.md](repo-structure.md)); `modules/os/session.nix` consuming `mura.xr.session.*` → `services.greetd` (autologin → sway; greeter → `cage -s -- gtkgreet`, stand-in); `modules/xr` Monado stub removed; the VM device drops the getty hack and permanent groups; both fixtures in `checks`. **B2 forced: logind** | — | both VMs boot with zero manual steps — one straight into sway as `mura`; one to gtkgreet where the declared user logs in and logout returns to gtkgreet; a greeter fixture with no declared user fails to *evaluate*; `sudo` as passwordless `mura` fails, `passwd` succeeds without an old password |
| **D1** | `modules/os/persist.nix`: `/persist` (family: `syspersist` stage 1, no `nofail`; VM: a second virtual disk as the stand-in) + class skeleton incl. `pairing/`; **`/etc` as a mutable overlay whose upper layer is `/persist/etc-rw/`** (userborn hybrid mode on every profile — the account database, machine-id, network profiles all persist through it; multi-user.md §1.1 rev 3.3); `/var/lib/bluetooth` → `pairing/` when the device has an adapter; F1 reference: SSH host key in `identity/ssh/` + the marker-gated `mura-f1-seed-state` pattern; `tests/persist.nix` pins the image layout in `nix flake check` | D0 | VM tests: `/etc` is an overlay with its upper on `/persist`; `passwd` persists across a reboot into the upper; F1 unit ran once and is condition-skipped on the second boot; machine-id stable; `useradd` survives a reboot (multi-user); no failed units, no ordering cycles |
| **D2** | `modules/os/policy.nix`: the §5.3 posture table as declared PAM services (`login` nullok, faillock on `login` + `sshd` with `conf=`); standard sudo/polkit; faillock counters on `/persist`; the greeter NetworkManager polkit rule; `HandlePowerKey=ignore`; the sticky `state/credential-hint/` directory. **sshd is not policy.nix's**: `modules/os/default.nix` enables it with upstream defaults (`mkDefault`; posture correction, first-run rev 2.5) | D0 | sshd answers with `PasswordAuthentication yes`, `PermitEmptyPasswords no`, no `Match`; the empty password is refused and a declared fixture key logs in before any password exists; after `passwd`, password SSH works; faillock locks after `faillock.deny` failures with its tally on `/persist` and refuses the right password while locked; `sudo` fails until `passwd`; the greeter profile ships the NetworkManager rule and the default image does not (its effect is exercised at D3, which brings NetworkManager); a user writes their own hint file and another user cannot remove it; a session process cannot set the password except through `passwd`'s PAM conversation |
| **D3** | F3 — **landed**: `modules/os/oob.nix` — configfs NCM gadget from the initrd (`mura-usb-gadget`, first UDC), `systemd-networkd` DHCP on `usb0` (NM-unmanaged; firewall opens 67/80 there); hotspot as an NM AP profile with a per-boot PSK (`mura-hotspot-psk`), `dnsmasq-shared.d` wildcard + option 114, the `mura-hotspot` supervisor (marker absent AND no other active connection; per-boot idle timeout from `mura.oob.hotspot.idleTimeoutMinutes`; NM `firewall-backend=iptables` so shared mode opens its own DHCP/DNS); the **`mura-setup` identity** with `50-mura-setup.rules` (exact actions), the **stub** launcher/web page on `:80` of the gadget + hotspot addresses only (`IP_FREEBIND`, `ConditionPathExists=!…/setup-complete`, exits when the marker appears; `pkgs/mura-setup`, Rust — ported from the D3 Python under AGENTS rule 6); sticky `state/setup/`; `mura.hardware.input.usbGadget`; Avahi `mura.local` | D1, D2 | `nix build .#vm-test-oob` (both `dummy_hcd` and `mac80211_hwsim` are in the NixOS kernel — no renamed-NIC fallback needed): gadget bound from the initrd, host end leases from `usb0`, SSH by key + launcher over the cable; `mura-setup` under its identity, conditioned, refusing the LAN address; hotspot up with the PSK, wrong PSK refused, right PSK associates + lease + wildcard DNS + probe `302`; yields to a dummy uplink and returns; idle timeout takes the radio down for the boot; `POST /finish` writes the marker → both stop; marker survives a reboot; removing it brings both back |
| **D4** | B6a: the session wrapper + `mura-session.target` around sway. First **uwsm** (AGENTS rule 1; evaluated against the spec and adopted, rev 2), then **`mura-session`** (rev 3, AGENTS rule 6: uwsm's mechanism as a libc-only Rust binary over four static user units — `mura-compositor.service` with `Restart=on-failure` + `RestartMode=direct`, `StartLimitBurst=3`/60 s, `TimeoutStartSec` from `mura.xr.session.readinessTimeoutSeconds`, `UnsetEnvironment=WAYLAND_DISPLAY DISPLAY`, no unit-private `PATH`; `mura-session.target`; `mura-session-bindpid@`; `mura-session-shutdown.target`), `environment.d/60-mura.conf`, sway's config `exec mura-session finalize` (STAND-IN); [specs/session-bootstrap.md](../../specs/session-bootstrap.md) rev 3 | D0 | **landed**: compositor in `mura-compositor.service` with the seat acquired from inside the unit (`XDG_SESSION_ID` in the unit, not the manager); the wrapper is the leader's child in the session scope and `bindpid`-bound; static vars present, `WAYLAND_DISPLAY` names a bound socket, nothing of greetd's leaked; `kill -9 sway` → restarted in the same logind session; logout → gtkgreet with no DRM race; a never-ready stub torn down at the 30 s bound; `mura-session.target` + `monado.socket` active; no ordering cycle; no `uwsm`/`python` process for the login user; login → graphical session 5.96 s → 1.65 s (VM); `tests/closure.nix` proves the login-path closure carries no interpreter |
| **D5** | **landed, then hardened** — `pkgs/mura-authd` (Rust; the first Rust in the tree): `mura-authd` per [specs/session-auth.md](../../specs/session-auth.md) §2 (rev 4: nonce in `MURA_AUTHD_NONCE` not argv, not dumpable / `PDEATHSIG` / `FD_CLOEXEC` / `RLIMIT_CORE=0`, strict responses — NUL, duplicate or out-of-range index → `failure(internal)`, `--service` allowlist, §2.5 threat model, §6 item 9 framing scenarios) — seqpacket JSON framing with the 64 KiB / truncation / empty / unknown-type rules, nonce discipline, `pam_start("mura-lock")` → `pam_authenticate(PAM_DISALLOW_NULL_AUTHTOK)` → `pam_acct_mgmt` → `pam_end`, fail-delay callback → `delay_ms`, coarse failure reasons, zeroised buffers; `mura-authd-harness` (test-only compositor stand-in) + `pam_mura_test.so` (test-only module); `security.pam.services.mura-lock` (no `nullok`, faillock) and `state/faillock` `0755` for the unprivileged caller | D4 | `vm-test-default-image`: passwordless account refused (`DISALLOW_NULL_AUTHTOK`); §6 item 1 (kill mid-prompt → EOF, fresh nonce succeeds), 2 (5 s PAM sleep, caller ticks 25×), 3 (success on a revoked nonce ignored — the compositor's rule, demonstrated), 7 (two prompts + info → one `prompt_batch`/`respond_batch`); plus stale-nonce ignored, cancel → `failure(abort)`, wrong password → `failure(auth)`, faillock locks the lock after `deny` failures and refuses the right password until reset; `vm-test-multi-user`: the fixture account unlocks. Items 4 (composition introspection), 5 (into *locked*), 6 (greeter mode) wait for zxr (G1/G2/G3) |
| **D6** | **landed** — `modules/os/health.nix`: `mura-preflight` (`pkgs/mura-preflight`, Rust — ported from the D6 Python under AGENTS rule 6; P1–P7, exit 0/1/2, `/run/mura/preflight.json`), `greetd` `Requires=` it; `mura-crashloop` (OnFailure counter in `state/health/`) + `mura-recovery.target`; `mura-readiness` → `boot-complete.target` (pulled in on every profile); contract `mura.health.{crashLoopThreshold,deviceWaitSeconds,readinessStabilitySeconds}`, `mura.deployment.bootTries`. `families/uefi-rauc`: `mura-bootconf set-primary` arms `+N`; systemd ≥260's assessment-aware loader `preferred` names the primary while `default` names the opposite fallback; `mura-mark-good.service` after `systemd-bless-boot`, RAUC status file on `/persist` | D1 | VM (`vm-test-default-image`, `vm-test-multi-user`): preflight finished before greetd started, report present, readiness → `boot-complete.target`, counter 0; `vm-test-health`: forced hard failure → no greeter/session, counter 1; threshold → recovery target with sshd up; passing boot blessed, counter reset. **Slot fallback and the bless/RAUC transitions**: deckard uefi-rauc QEMU image, manual proof step (§3a status) |
| **D7** | **landed** — the settings store re-derived from the stores' source ([research/58](../research/58-settings-stores-from-comparables.md); [specs/settings-schema.md](../../specs/settings-schema.md) rev 3, [specs/settings-daemon.md](../../specs/settings-daemon.md) rev 1): `lib/settings` compiles `mkSetting`-annotated contract options into `/etc/mura/settings-schema.json` (the only default channel — GSettings' compiled schema on NixOS's `programs.dconf` shape); `pkgs/mura-settingsd` (Rust, zbus on `async-io`) serves `org.mura.Settings1` on the session bus — D-Bus-activated, resident, one writer of sparse per-(schema, instance) JSON under XDG with fsync+rename, `declarative` by default / `runtime` opt-in, Set-always-writes/Reset/provenance incl. `invalid`, relocatable `places.entry` instances, locks as artifact facts, additive numbered code migrations; `mura-settings` CLI with `--direct`; `modules/os/settings.nix` (`mura.settings.{templates,schemaVersions,locks}`, the `Type=dbus` user unit, the `system.userActivationScripts` generation hook). Seed keys: `xr.passthrough.{latencyMode,upperLimbVisibility}` (runtime), `hardware.ipd.meters` (runtime only when `ipd.source = stored`). Ruled shape C: `stratum = device` reserved; the same crate's `--system` mode serves it when a target declares a device key (an assertion refuses one until then) | — | `nix build .#vm-test-settings` (10 subtests): activation on first `Get`; Set-equal-to-default creates the override and `Reset` returns to following; declarative/locked/range/type refused without a write; instances create/list/delete; one `Changed` per accepted change with value + provenance; an invalid stored value → default + `invalid`, file byte-identical; `kill -9` mid-burst → no torn file, same resolution direct and over the bus; the lying-daemon check (`--direct` reads the artifact); a `specialisation` switch → the moved default advances, the pinned instance value survives, `Changed` + `GenerationChanged` reach the session; RSS 3.5 MB, 4 threads. `tests/closure.nix` fences the binary. Migration item 9 at the unit level (the table is empty at rev 1) |

Order is dependency, not calendar: D0 first (everything imports the profiles and the session
module), then D1/D2/D4 in any order, D3 after D1+D2, D5 after D4, D6 after D1; D7 whenever.

**Status (explicit):** D0 landed 2026-09-24 (`profiles/`, `modules/os/session.nix`, the two
fixtures in `checks`) and was verified by hand the same day: the default-image VM autologins
straight into the sway stand-in as the passwordless `mura`; the multi-user VM boots to the
gtkgreet stand-in, refuses an undeclared username uniformly, and logs the declared fixture
account in to sway. **T0** landed the same day: both criteria are now VM tests
(`nix build .#vm-test-default-image` / `.#vm-test-multi-user`, on demand). **D1** landed the
same day and passes both VM tests; it produced one design correction — the
`passwordFilesLocation`/symlink userdb of multi-user rev 2/3 does not survive shadow-utils'
`rename(2)`; the account database now persists through a mutable `/etc` overlay on `/persist`
(multi-user.md rev 3.3, first-run rev 2.3). **D2** landed the same day
(`modules/os/policy.nix`, `mura.xr.session.faillock.*`) and passes both VM tests; it produced a
second design correction — `PermitEmptyPasswords` over the gadget subnet is withdrawn because
OpenSSH's `none` probe poisons the parent's PAM handle for every later password login
(first-run rev 2.4, multi-user rev 3.4, ADR 0017 rev 2.3, research/42 §6.5) — and three
nixpkgs facts now encoded in the module: NixOS drops `pam_unix` from sshd when the global
`PasswordAuthentication` is off (`unixAuth` forced), Linux-PAM's sysconfdir is in the store
(`pam_faillock conf=`), and OpenSSH's `PerSourcePenalties` throttles independently of PAM
(disabled in the test harness only). **D2 posture correction** (same day, first-run rev 2.5 /
ADR 0017 rev 2.4): the key-only / `Match Address` sshd scoping, the `unixAuth` override, the
dev-profile loosening and the harness overrides were removed as over-hardening beyond upstream;
sshd is now enabled with OpenSSH's defaults on every profile in `modules/os/default.nix`, the VM
tests carry a fixture SSH key to prove the self-builder path, and both fixtures pass. Not yet
exercised: logout returning to the greeter (D4); §8 check 13's greeter half (needs a Wi-Fi greeter, G2); check
15 (needs the D-Bus surfaces, D5/D7). **D4** landed the same day: **uwsm** is the wrapper (evaluated against the spec's four requirements — seat via `env_session.conf`, readiness via `Type=notify` + `TimeoutStartSec`, lifetime via `signal-handler.sh` `--wait`, native `sd_notify` for zxr later — and adopted; no Mura wrapper written). Three mechanisms found on the way and encoded: target units implicitly order `After=` their `Wants=` (the rev-1 `mura-session.target` shape was an ordering cycle, silently deleted); systemd ≥ 254 fires `OnFailure=` on every failure even with `Restart=`, so in-session crash restart needs `RestartMode=direct`; a restarted compositor must `UnsetEnvironment=` the previous instance's `WAYLAND_DISPLAY` or it runs nested. The wrapper's exit status is not the failure signal (it waits on a target). Spec rev 2 records all of it; conformance items 1–7 verified, item 8 (two Monados) waits for a greeter that runs one (G1/G2). **D3** landed 2026-09-24 (`modules/os/oob.nix`, `tests/vm/oob.nix`): the gadget, the hotspot, the
`mura-setup` identity + stub and the marker, all VM-verified with real kernel devices (`dummy_hcd`,
`mac80211_hwsim`). Facts encoded: NM shared mode opens DHCP/DNS only with its iptables firewall
backend; the NixOS firewall needs `usb0` 67/80 and the hotspot address :80 opened; `qemu-vm.nix`
disables `wpa_supplicant` by `mkVMOverride` (priority 10 — the VM device and the harness override
at 5). NCM only, the Linux gadget example identity `1d6b:0104`, fixed gadget serial, random MACs
— all of which the **USB identity + descriptor correctness** row below owns (research/55); the
web UI itself is its own rung. **D6** landed the same day (`modules/os/health.nix`,
family boot counting + mark-good, `tests/vm/health.nix`): preflight, ladder and readiness gate
VM-verified; slot fallback + bless + RAUC state recorded as a manual proof on the deckard image.
**D5** landed the same day (`pkgs/mura-authd`, `security.pam.services.mura-lock`): the helper and
its harness pass session-auth §6 items 1/2/3/7 in both fixtures; two facts encoded — `nullok` on
`mura-lock` is inert under `PAM_DISALLOW_NULL_AUTHTOK` (dropped), and an unprivileged locker only
reaches its faillock tally through a traversable directory (`state/faillock` `0755`; creation
stays root's). **Post-D-track sweep, 2026-09-25**: (a) the language ruling — every Mura program
on the boot, login or session-start path is Rust; uwsm (D4) and the Python `mura-preflight` (D6)
and `mura-setup` stub (D3) were ported (`pkgs/mura-session`, `pkgs/mura-preflight`,
`pkgs/mura-setup`; spec rev 3; login → graphical session 5.96 s → 1.65 s in the VM), and
`tests/closure.nix` in `nix flake check` proves the login-path closure (four Mura programs +
greetd + systemd + util-linux) carries no interpreter while pinning the toplevels' nixpkgs-side
Python as a shrinking allowlist; (b) D5 hardening (session-auth rev 4); (c) AGENTS.md rules 6–8
(think embedded; comparables with their reasoning; the evidence gate) and the list of behaviours
that cost time; (d) [research/55](../research/55-usb-identities-and-gadget-policy.md) — the USB
identity posture ruled, the work scheduled as a §4 track; (e) [research/56](../research/56-defaults-from-comparables.md)
— every agent-chosen default re-derived: applied where the comparables converge with a reason
that transfers (compositor `StartLimit` 3/60 s = plasmashell's; hotspot idle 10 min = AOSP's;
user-run unlock + faillock = Linux-PAM's screensaver design; `PDEATHSIG` fatal / dumpable
best-effort = kscreenlocker's and systemd's; the in-session time-zone/hostname grant to active
local wheel members = Ubuntu's `policykit-desktop-privileges` shape, `50-mura-timedate.rules`,
VM-verified from inside the user manager); three further rulings the same day — the blessing tier
is target-reached (the 20 s window removed; every system blesses on a target), the device wait is
10 s with P5/P6 soft (GDM's and pmOS's shape), `environment.defaultPackages` emptied — and **one
item ruled after discussion**: the crash-loop counter stays for the cross-slot gap, its step is an
automatic reboot into a Mura-owned recovery environment (the §4 track), feedback moves to the
first failure. **D7** landed 2026-09-25 after the comparables pass it required
([research/58](../research/58-settings-stores-from-comparables.md): the desktop stores, then —
on the owner's challenge — the appliance OSes' privileged stores; five spec mechanisms had no
comparable and were ruled or converged: the polkit-gated per-unit writer became the reserved
`device` stratum in the same crate's system mode (shape C), quarantine became `invalid`
provenance over an untouched file, apply transactions became a label, the migration DSL became
additive code steps under NixOS's rollback constraint, the preference session stratum was
dropped). The D-track's compositor-free rungs are complete.

### R0 — the bring-up spike (risk retirement, not a decision gate)

The smithay skeleton dropped into the dev-session slot, measured against the four gates of
[doc 39 §5](../research/39-compositor-base-landscape.md): real projection-layer presentation on
a runtime-created Vulkan device; zero-CPU-copy dmabuf import with explicit sync end-to-end;
window behavior under churn (resize/popups/kill-mid-frame, no unresolved GPU waits — previewing
M4's stopping rule); Xwayland early. **Entry (since 2026-09-26): the program spec exists** —
[specs/zxr-core.md](../../specs/zxr-core.md) rev 1, derived from [research/59](../research/59-xr-compositor-architecture-from-comparables.md)
and [research/60](../research/60-de-abstractions-mapped-to-xr.md) with the motorcar/wxrc lineage
read first, and the 2026-09-26 rulings (ADR 0006/0012 amendments: calloop owns the thread and
`xrWaitFrame` runs on a dedicated thread; xwayland-satellite, no longer an R0 output; no
compositor-side windowed backend — Monado's simulated HMD is the dev backend; WM policy in-process
with a bounded protocol later; every shell component its own process). The §5.1 rule "no rung
before its spec" applied to R0. Exit: a written result per gate + the instrumentation numbers
([research/61](../research/61-r0-bring-up-results.md)), spec rev 2 from what R0 taught, the
fallback trigger evaluated. Registry rows it moves: none directly (it's evidence, not a
component), but every authority-plane "specified" row becomes buildable on its skeleton.

### G1 — the greeter scene in dev-session

`zxr --greeter` as a window: R0's OpenXR loop + renderer, the internal auth scene (generic
prompt rendering per session-auth §2.3's style set — the digit pad keys off `style=secret` plus
the user's mirrored `numeric-credential` hint, never prompt text), a fake greetd speaking the
JSON IPC over `$GREETD_SOCK`, session list from a static config, the standard furniture of
multi-user.md §2 (power menu, clock, session chooser, accessibility). **No Wayland listening
socket** — assert it in the harness (`ss`/`lsof`, the session-auth §6.6 conformance check,
minus PAM which greetd owns). Exit: prompt → response → `start_session` acknowledged → clean
teardown, on the simulated HMD, with `--rotate` proving the scene is really mura — **and the
whole exit path driven at the input floor**: simulated head-aim plus one key event standing in
for `hmdButtons.<selectRole>`, then once more with the key masked (dwell only); a hardware
keyboard typing into the auth scene (first-run-onboarding §4.4, §8 checks 10–11).

### G2 — the swap (the first shippable greeter)

D0's `modules/os/session.nix` runs the stand-in greeter; G2 replaces that one line with
`zxr --greeter` and nothing else changes — greetd, PAM, the wrapper, the seat handoff and the
declared-account assertion were proven on the D-track. **The swap is an obligation, recorded in
three places**: this exit criterion, the `# STAND-IN — replaced at G2 by zxr --greeter` comment
on the `default_session` line, and the registry's greeter row. Exit: the multi-user VM fixture
cold-boots → XR auth scene with zero manual steps → real PAM conversation (greetd worker) →
the stand-in session; **gtkgreet and cage are no longer in the closure**; re-lock/logout returns
to the XR greeter; zero pickable accounts still renders free-text entry + power menu; the power
menu powers the VM off with no authentication (login1 `allow_active`, research/11 §11.A); a
Wi-Fi profile added at the greeter is a system connection; a passwordless declared account logs
in with no prompt and a digits-only one gets the digit pad; the greeter process never loads PAM
symbols (session-auth §6.6).

### M1 — the spatial 2D desktop (composition §7.5)

The 2D tier on R0's skeleton, in the windowed dev backend: xdg-shell + baseline globals
(registry protocol-server row), ray→window-local `wl_pointer`, move/rotate/resize planes,
copy/paste, popups/menus — with composition §7.3's constraints 1–9 baked in from the start (no
window↔output binding, pluggable hover/focus, placement volumes, arbitration state machine,
settings from the schema artifact). Acceptance is the composition table's: "terminal + editor:
type, select, copy/paste, open menus, move/rotate/resize planes". When M1 lands, dev-session's
`COMPOSITOR_CMD` swaps sway for zxr, rung 1 becomes zxr's own loop, and **sway leaves the
`mura-session.target` member list** (the second stand-in swap).

### G3 — the full handoff

greetd `start_session` forks the **B6a session wrapper** (`mura-session start`), which brings
up `mura-session.target` (B6: Monado + zxr-session + shell services) under the environment and
lifetime contract of [specs/session-bootstrap.md](../../specs/session-bootstrap.md) rev 3 — zxr
replaces sway as `mura-compositor.service`'s `ExecStart`, calls `sd_notify(READY=1)` itself once
its socket is bound and publishes its variables, and the stand-in `mura-session finalize` line
leaves with sway; ADR 0007's
crash/restart and boot-locked-restart rules apply. Needs M1 (a session someone can use) + G2
(the greeter) + D5 (authd) for lock. Exit: VM boots → XR greeter → login → **zxr session** →
doff-grace/lock/unlock cycle works end to end → logout tears down through the wrapper and
returns to the greeter without racing device release; the spec is revised from what G3 taught.

### §3a — B9: session-ready tiers, mark-good, and boot-counting

"Session ready" has **two tiers**, and only the first ever gates an update:

- **G3-minimum (the blessing tier):** the session target reached — `mura-compositor.service`
  active (appliance) or greetd + its greeter (multi-user) — the shape every shipping system
  blesses on (systemd `boot-complete.target`, RAUC mark-good after `multi-user.target`,
  mobile-nixos boot-control; research/56 §4, ruled 2026-09-25 — the earlier "stability interval"
  had no comparable; a crash after blessing is the compositor unit's `StartLimit` matter, not a
  boot failure); systemd watchdog health (`WatchdogSec` on both
  processes); writable `/persist/mura` verified; per-unit settings-store migration completed;
  input path confirmed (a synthetic event round-trips); crash-loop counter (B1b) at zero.
  **Blessing is profile-specific**: default image = a stable `mura` session (locked only if a
  credential exists); multi-user = a stable *greeter* — mark-good never waits for a human to log
  in, so `/home` and per-user migration state can never gate it.
- **Desktop-usable (post-login qualification, never blocks blessing):** PipeWire + audio policy
  including the intended default capture source, declared channel width, and ordinary-client
  recording ([research/43 §10](../research/43-microphone-native-linux-capture-audit.md) R4);
  virtual keyboard/input method, settings daemon, polkit agent, portals, launcher +
  notifications — each a registry row, several honestly **missing** today (the registry's gap
  list is the work queue; without a polkit agent privileged operations silently fail, and
  without a general virtual keyboard the headset cannot satisfy M1's "type in terminal/editor"
  outside the desktop-window harness).

The **mark-good service** runs `mura.qualification.readinessCheck` against the blessing tier.
The attempt/fallback mechanics are systemd-native and must be **explicitly wired in the
uefi-rauc family** (it sets `boot.loader.systemd-boot.enable = false` — manual ESP install —
so NixOS wires none of this automatically): RAUC `set-primary` arms the target slot's boot entry
with a `+N`-tries BLS suffix; systemd-boot decrements tries across attempts and falls back to
the previous slot's entry when exhausted; the readiness unit is a prerequisite of
`boot-complete.target`; `systemd-bless-boot` performs the entry rename (the systemd-boot
blessing); RAUC slot-status marking via the custom bootconf backend is the third, separate step.
Three distinct transitions — readiness, boot blessing, RAUC state — each observable on its own.

**Implementation status (explicit, D6 landed 2026-09-24):** `modules/os/health.nix` carries the
readiness gate — `mura-readiness.service` (blessing tier: the autologin user's compositor unit,
or the greeter on the multi-user profile, active — a target reached, since the 2026-09-25 ruling
— and `/persist/mura` writable; it resets the crash-loop counter) is `RequiredBy=boot-complete.target`,
which every profile now reaches (transition 1, VM-verified on both fixtures). The uefi-rauc
family wires the slot side: `mura-bootconf set-primary S` arms the slot's ESP entry with
`+<mura.deployment.bootTries>` (`a.conf` → `a+3.conf`; `loader.conf` selects by entry ID `a`/`b`,
which survives the counter renames), upstream `systemd-bless-boot` strips the counters after
`boot-complete.target` (transition 2), and `mura-mark-good.service` runs `rauc status mark-good`
after that (transition 3); RAUC's status file moved from `/tmp` to `state/health/`. Transitions
2 and 3 need an ESP with counted entries and RAUC — the deckard uefi-rauc image (doc 33 §9's
QEMU proof), not the virtual headset, and are **recorded as a manual proof step**: arm `+3`,
force two failed boots, watch systemd-boot fall back, then a good boot → bless → RAUC good.

### §3a-bis — B1b: the preflight probe contract

The probe is one executable (`mura-preflight`) run as a system unit `Before=` greetd (or the
autologin session) and `After=` the persist mount and udev settle; it is also what the
`readinessCheck` blessing tier re-runs post-session. Checks, in order, each a named result:

| # | Check | Pass condition | Fail class |
|---|---|---|---|
| P1 | persist | `/persist/mura` writable; class dirs present with the §2 modes | hard |
| P2 | factory calibration | files at `mura.xr.calibration.paths` exist, parse, and carry a version the runtime accepts | hard (no Monado start; diagnostic target) |
| P3 | display path | the DRM connector the contract names is present; `vk-display`/window backend as configured | hard |
| P4 | Vulkan | a runtime-created Vulkan device on the expected GPU (vendor/device ID match) | hard |
| P5 | tracking nodes | IMU (and camera, where the profile needs it) device nodes present within the device-wait timeout | **soft** (ruled 2026-09-25, research/56 §5: wait ~10 s, then proceed degraded, as GDM and postmarketOS do for their hardware class; the greeter starts and shows the result) |
| P6 | Monado probe | Monado's drivers initialise within the device wait (the first composited frame is the session's) | **soft** (same ruling) |
| P7 | input floor | at least one evdev device exposes `hmdButtons.<selectRole>` (or a keyboard is present) | soft (warn; the greeter still starts — dwell remains) |

Exit codes: `0` all pass; `1` a soft check failed (start, log, expose in the a11y menu); `2` a
hard check failed — the unit fails, and `greetd.service` `Requires=` it, so this boot has no
greeter or session. **The probe never modifies persistent state**; the crash-loop counter is a
separate unit's: `OnFailure=mura-crashloop.service` increments `state/health/crashloop` on
`/persist`, and at `mura.health.crashLoopThreshold` consecutive hard failures reboots into the
Mura recovery environment (§4 track; automatic, ruled 2026-09-25; where the family provides no
recovery entry — the VM — it starts the stage-2 `mura-recovery.target`, the failure-feedback
state: sshd, gadget and hotspot up, nothing graphical). The first hard failure already shows what failed and how to reach the
device on the panels (plymouth) and on the setup launcher. A blessed boot
(`mura-readiness`) resets the counter. Results are written as `/run/mura/preflight.json` for the
readiness check and for `mura-device.json`-style tooling. **Landed at D6** (`mura-preflight`;
Python at D6, Rust since the AGENTS rule 6 port — `pkgs/mura-preflight`; `modules/os/health.nix`): P1 real; P2 = every declared `calibration.paths` file exists
and is non-empty (the version check is the runtime's); P3 = a connected connector, or any DRM
card for the `window` backend; P4 via `vulkaninfo --summary`; P5 = an IIO accel+gyro within
`mura.health.deviceWaitSeconds`, n/a when the runtime simulates tracking; P6 = `monado-cli probe`
succeeds within the wait (needs a HOME/XDG home — found at D6; the first-frame criterion is the
blessing tier's, where the session's Monado runs); P7 from `/proc/bus/input/devices` key
bitmaps. VM: `nix build .#vm-test-health` forces a P2 failure — no greeter, counter 1; second
boot → threshold (2 in the test) → `mura-recovery.target` with SSH reachable; a passing boot is
blessed and the counter returns to 0.

### M2–M4 — widening the session (composition §7.5, unchanged)

M2: the `zxr-shell-v2` protocol server goes live (generated from the rev-2 XML via the
`checks.protocols`-validated scanner path) — a GL ray-march client and a Vulkan raster client
intersect and pass in front of/behind M1 windows, no CPU readback. M3: the CPU reference client
against a single-process ground truth (catches matrix/depth-origin/clip bugs). M4: headset
output via Monado — head motion drives all clients from one frame snapshot; stopping a client
never creates an unresolved GPU wait; a rootless Xwayland app participates. M4 runs entirely in
rung 1 (simulated HMD); real-HMD output stays behind the display-path feasibility gate
(desktop-environment §6.6).

## 3b. Lifecycle: resume, doff, logout, user switch

Resume is a boot sub-path, not an event: after suspend, the session re-enters through a reduced
B1a/B1b — GPU/DRM/USB re-initialization, Monado restart-or-restore, tracking and calibration
revalidation — and **the lock is asserted before any restored client frame is exposed**
(ADR 0007 I1–I3; the L2 trace ordering from session-auth §3.1 applies to the resume edge
exactly as to the suspend edge). Device loss mid-session (HMD unplug on dev hardware, tracking
loss) routes through the same revalidation. The doff ladder and the docked-mode branch are
ADR 0015's (doff-grace suppression while docked-in-use); logout tears down through the B6a
wrapper (user target stopped, wrapper returns, greetd restarts the greeter);
user switching on the multi-user profile is logout + login (no concurrent graphical sessions —
one HMD, one seat), recorded as the deliberate v1 simplification.

## 4. Parallel tracks (no compositor dependency, not on the D-track ladder)

Per desktop-environment §6.4, the session cluster is HMD-independent. Two remaining tracks have
conformance checklists that are ready-made test plans (authd moved onto the D-track as D5):

- **The settings daemon** ([specs/settings-schema.md](../../specs/settings-schema.md) rev 3,
  [specs/settings-daemon.md](../../specs/settings-daemon.md)): landed as D7 (§3c). What remains
  on the track: the `--system` mode's handlers and polkit action granularity, built with the
  first target that declares a `device` key (the Frame's TDP/fan/charge-limit class —
  steamos-manager's list; snapd's one action vs systemd's per domain is that rung's question);
  `state, session` opened by the first component that names the need; the first real schema
  migration (a numbered additive step in `pkgs/mura-settingsd`). M1's constraint-9 compliance
  consumes the daemon as built.
- **The perception intake harness** ([specs/perception-intake.md §8](../../specs/perception-intake.md)):
  fake producer + test consumer exercising registration, generations, overrun, epoch teardown,
  and the structural never-block check — validates the protocol before either real end exists.
  Feeds the M4-adjacent perception work without gating it. **Status (2026-09-25): landed** —
  `pkgs/mura-perception-intake` (Rust, libc only): the protocol *library* (`perception_intake`:
  §7 framing + SCM_RIGHTS, §3 tables, §4 memfd register, DRM syncobj timelines and udmabuf as raw
  ioctls) the real ends are meant to link, plus the test-only `intake-fake-producer` /
  `intake-test-consumer`; `nix build .#vm-test-perception-intake` (12 subtests on the VM's
  virtio-gpu render node + `/dev/udmabuf`): verified §8 scenarios plus rev 3's separately
  labelled alternative observation for item 2, registration over `REGISTER_MORE`, epoch
  supersession, the §7 framing rules, `hand_top`; check 4 traced with strace between the
  consumer's pass markers (window = `clock_gettime`, `ioctl` query/signal, `recvmsg(MSG_DONTWAIT)`
  only). **The harness found a protocol gap**: fence-only reclamation (§4 rev 2) cannot tell a
  never-used generation from one in flight and wedges the pool under a slow consumer; rev 3
  adds the consumer's *use page* (`pending[]` + `intent`, one fd in `REGISTER_ACK`) and a two-flag
  slot agreement (Dekker). `linux-drm-syncobj-v1.xml:210-222` is precedent for the consumer's
  release/declaration **obligation**, not for this Mura-specific shared-page mechanism. Marked ⚠
  in the spec: **the owner rules on rev 3**. Until then §8.2's rev-2 drop expectation is not
  marked verified; the observed rev-3 behavior is zero drops for a conforming stalled consumer,
  while OVERRUN remains verified for a consumer exceeding its declaration. A full pending table
  now refuses selection before GPU submission (VM-proven), so no live use can be undeclared.
  Reserved hook: the in-thread seccomp never-block guard for zxr's intake.
- **Mura recovery environment** ([research/56 §3](../research/56-defaults-from-comparables.md),
  [research/57](../research/57-recovery-environments-and-boot-failure-feedback.md),
  [specs/recovery-menu.md](../../specs/recovery-menu.md); ruled 2026-09-25). The OS owns its
  recovery as a **dedicated Mura recovery boot partition/image**, separate from normal Mura boot
  artifacts and from the hardware's stock/vendor recovery. It contains its own kernel and a
  separately evaluated, stripped systemd initrd but no third root filesystem (Mobile
  NixOS/Lineage's recovery-image boundary; pmOS's stage-1 contents); recovery-only daemons do
  not ship in the normal ESP initrd. On uefi-rauc it is the `mura_recovery` XBOOTLDR partition: one UKI binds
  kernel+initrd+`rd.systemd.unit=mura-recovery.target`, and stable `recovery.conf` points to it;
  the normal entries and boot files stay on the ESP. It is readonly RAUC `rescue.0`: ordinary
  A/B bundles leave it untouched (RAUC's Additional Rescue Slot shape). A future recovery update
  contract must stage versioned whole UKIs; raw overwrite of the sole rescue image is not
  accepted. On Android-derived targets stock/vendor recovery
  remains the independent install/reflash path (`reboot recovery` still means that); each
  device's bring-up must prove that its boot chain can select an **additional** Mura recovery
  partition before setting `mura.recovery.rebootCommand`.
  Trigger: `mura-crashloop` at `crashLoopThreshold`, automatically; where the family provides no
  entry (`mura.recovery.rebootCommand` unset — the VM), the step enters the stage-2
  `mura-recovery.target` instead: sshd, gadget and hotspot up, nothing graphical. Feedback on the
  *first* hard failure is plymouth in the normal initrd (`mura-preflight-feedback`).
  Contents: sshd and the recovery web app on both the USB-gadget address and a per-boot-PSK
  recovery hotspot (host key from `identity/ssh` when `/persist` mounts, else generated and its
  fingerprint shown; wheel members' and root's declared keys only), and
  **one menu program, three ways in** (`pkgs/mura-recovery`, the `mura-setup` shape, ADR 0017
  decision 10; Rust under rule 6 — it parses input and holds state): the actions live once —
  status; factory reset; slot switch where the family has one; reboot; power off — and every
  destructive one is *offered* behind a confirm, never automatic (rule 3). The factory reset is
  systemd-repart's (`FactoryReset=yes` on `syspersist`/`home` in the family's `repart.d`,
  `systemd-repart --factory-reset=yes` deleting and re-creating exactly those partitions),
  invoked directly from the recovery environment: stage 1 *is* the "well-defined clean state"
  that `factory-reset.target` exists to reach, so `systemd-factory-reset request` + reboot would
  add only the EFI-variable write (the U-Boot risk below). It unmounts the partition first
  (Android recovery's `EraseVolume` shape) and rides out udev's `BLKPG`/`BLKRRPART` EBUSY
  window. Frontends: (a) `mura-recovery panel` — the HMD's buttons over raw evdev in stage 1
  (`/etc/mura/recovery.json` from the contract's `hmdButtons`/`selectRole`/`backRole`, Android
  recovery's keyboard fallbacks appended; on the three-button default volume-up selects,
  volume-down moves, the list wraps), Android recovery's key semantics (register on release,
  auto-repeat ignored, ≥750 ms long press ignored — a held key cannot confirm), a separate
  Confirm screen defaulting to `Cancel`, drawn as plymouth messages ≤200 bytes under the
  per-device theme (`pkgs/mura-plymouth-theme`, `assets/branding`); devices that appear after the
  panel started are picked up (inotify — udev's coldplug lands them late); (b) bare
  `mura-recovery` over ssh/console — what the banner says to type — `yes, erase` to confirm;
  (c) `mura-setup --recovery` on the gadget/hotspot addresses, `POST /factory-reset` refused
  without `confirm=erase`.
  **Status (2026-09-25): landed** —
  `modules/os/recovery.nix`, `pkgs/mura-recovery`, `pkgs/mura-plymouth-theme`,
  `mura-setup --recovery`; uefi-rauc's dedicated XBOOTLDR partition and
  `FactoryReset=yes` definitions; `tests/persist.nix` pins that the recovery entry+UKI
  are absent from the normal ESP and present on `mura_recovery`. Proof:
  `vm-test-recovery` (eight subtests: environment up and drawn on plymouth,
  per-boot-PSK hwsim hotspot association with ssh+web reachable,
  keys through QEMU's keyboard incl. a 2 s hold ignored, the Confirm/Back/Cancel flow, the shell
  over ssh from the cable's host end with the administrator's key and the banner's fingerprint,
  the device's own host key once `/persist` is readable, the shell's wrong answer, the web `400`,
  the wipe once through the web form over a left-behind mount and once through the panel —
  exactly the `syspersist` partition re-created); `vm-test-health` (the first hard failure's
  panel message; `plymouth-quit` skipped on the marker).
  **Deckard image proof: green 2026-09-25.** `frame-recovery-proof-image` forces P2 hard and a
  threshold of two, cycles the first failure, then the production counter writes
  LoaderEntryOneShot. systemd-boot selects `recovery.conf` from the separate
  `mura_recovery` XBOOTLDR partition; a test-only service inside stage 1 prints
  `RECOVERY_PROOF_OK`, all five recovery units `active`, `selected=recovery.conf`, and the
  embedded `rd.systemd.unit=mura-recovery.target` command line to ttyAMA0. The proof
  also corrected copied-without-reason foundations: BLS IDs include `.conf`; slot selection uses
  systemd ≥260's assessment-aware `preferred` with the other slot as `default` (an exhausted
  primary therefore falls back instead of being explicitly reselected; the aarch64
  `frame-bootconf-test` exercises plain and exhausted-entry re-arming); the sole ESP is `/efi`
  (systemd/mkosi's semantic layout, research/33 §10), with XBOOTLDR at `/boot`; and
  `frame-vm-run` seeds its writable pflash from QEMU's initialized variables template rather
  than a zero file. **Still hardware-only:** `--boot-loader-entry` writes an EFI variable at
  runtime, which the Frame's U-Boot UEFI only persists with a variable store configured. If
  that proof fails, bring-up must implement and prove the file-based bootconf fallback before
  shipping (write `default recovery.conf`; recovery restores the prior slot glob); it is not
  wired today. The recovery hotspot mechanism is landed and VM-proven for AP association,
  the `10.42.0.1` address, and both ssh+web (hostapd 2.4 GHz ACS + systemd-networkd in stage 1,
  no NetworkManager); networkd's DHCP server is configured, while an independent-client lease
  remains part of the hardware proof. Per target: the volume/select keys reach evdev
  in stage 1 (the input driver in the initrd beside the DRM driver); legibility of the menu text
  per eye; the Galaxy XR's shared select/power code (spec §4 rule 4); radio firmware,
  regulatory-domain and AP-mode qualification for the required recovery hotspot (hostapd +
  systemd-networkd in stage 1; no NetworkManager there).
  **Later in the track:** reflash from recovery (a RAUC bundle over ssh); `switch-slot` on the
  web page (decider: the uefi-rauc manual proof).
- **USB identity + descriptor correctness** ([research/55](../research/55-usb-identities-and-gadget-policy.md);
  posture ruled 2026-09-25: the comparables' pattern — a distro-wide well-known default overridden
  per device with the device's own identity, values from research/55 §4, confirmed when the
  hardware enumerates; USB-IF rejected). Depends on the first target with a real UDC booting
  Mura (the virtual headset's `dummy_hcd` proves the mechanism, not the identity). Work:
  `mura.hardware.usb.{idVendor,idProduct}` in the device contract (default the comparables'
  `18d1:d001`; per-device = OEM VID + a PID none of that device's stock compositions use, the
  Motorola-potter rule — Frame `28de`/not `2460`, Quest `2833`/outside `0081–0186`, `5009–500a`,
  Galaxy XR `04e8`, PFDM `34e2`/not `4f07`; Lynx → default), device-level `ef/02/01` for the NCM
  IAD, per-unit serial and stable locally-administered MACs from persisted identity, explicit
  UDC selection per target, `WINNCM`/RNDIS only as the Windows 10 test decides (research/55 §5).
  Exit: research/55 §8's runtime qualification matrix on that target; the D3 development identity
  `1d6b:0104` leaves `oob.nix`.

Each track also exercises its NixOS module wiring in the VM — the module system grows with the
daemons, not in a big-bang at the end.

## 5. The deferral register (the only place scheduling language lives)

**Standing rule** (docs README): design docs and ADRs *specify* — they state designs,
condition-shaped rules ("X exists only when Y does"), non-goals with reserved hooks, or open
questions that name their decider. Statements about *when* or *in what order* live here and
nowhere else. Anything phrased as "deferred" elsewhere is a defect to sweep into this section.

### 5.1 Deferred by this path

- **Pre-groundwork specifications, and the rule that binds them**: a D-track rung does not
  start before its specification exists — D0 needs `profiles/` and the module-ownership table
  ([repo-structure.md](repo-structure.md)); D2 needs the posture table
  ([first-run-onboarding.md §5.3](first-run-onboarding.md)) and the PAM/polkit table
  ([multi-user.md §3](multi-user.md)); D3 needs the gadget/hotspot specifics
  ([first-run-onboarding.md §5.4](first-run-onboarding.md)); D4 writes
  [specs/session-bootstrap.md](../../specs/session-bootstrap.md) as it goes; D6 needs the
  §3a-bis probe contract. All six exist as of rev 4.
- **Shell-plane presentation** — launcher, panels, pager/overview, OSD, notifications UI: all
  downstream of M1's window model and the places implementation; registry status honest
  (missing), by design.
- **Places implementation** beyond what M1's window model needs; the model is specified
  (ADR 0016) and its protocol drafted (`zxr-workspace-v1`), but residency/currency machinery
  waits for a session that has windows worth organizing.
- **Delegation consumer** (`zspatial-toplevel-export-v1`): staged behind M1 per ADR 0014 M-A.
- **kwin-vr packaging** (ADR 0013's reserved optional session): unscheduled; the
  `mura.xr.shell = kwin-vr` contract enum change lands with the packaging work.
- **Multi-account + guest activation on the ladder**: designed in
  [multi-user.md](multi-user.md) / ADR 0018; the picker extends the greeter scene after G2;
  guest's provisiond token gate and sweep units are D-track work after D2.
- **F2 (welcome surface)** lands after M1 as shell content; its contents are decided
  (first-run-onboarding §4.2); it is the session instance of `mura-setup`, sharing the library
  the D3 service instance is built on. **F3**: sshd is already on (D2); the gadget + hotspot +
  `mura-setup` service half lands at D3 with no compositor dependency; the in-headset PSK
  display is a compositor scene (G1+), so the dev profile prints the PSK to serial/journal until
  then; the setup **web UI** itself (the phone-facing pages behind the D3 stub) is its own rung
  after D3 — its toolchain is not decided: rule 6 puts any interpreter build step on the budget,
  rule 7 asks how the comparable portals (wifi-connect, comitup, the gnome-initial-setup pages)
  build theirs and why, before the rung starts. Whether F2's time-zone card needs the **polkit agent** (registry
  gap #10) or a Mura rule is research/54's pending ruling. **F4**: the logind half is D2; the constraint-7 stabiliser
  and button handling are G1's exit criteria; the **per-target Monado 3DoF HMD driver** precedes
  any *in-headset* greeter on that target and belongs to each device's bring-up ladder — the
  rung-1/rung-2 harnesses (simulated HMD) need none of it.
- **The stand-in swaps**: gtkgreet+cage out at G2; sway out at M1. Both recorded as exit
  criteria; neither stand-in is ever in a shipped image.
- **All hardware-gated work**: the Lynx spike rule stands (design-backlog standing rule);
  the Steam Frame donor workstream continues in parallel on its own ladder; nothing in this
  path requires hardware before M4's exit. Per-device microphone support likewise advances only
  through the A0/S1/S2/R1–R5 evidence states in
  [research/43](../research/43-microphone-native-linux-capture-audit.md); `fb2-audio` cannot pass
  S-1 from a vendor mic specification or stock-Android recording. The same shared method and
  per-domain gates for IMU, display, camera, radios, power, playback, boot, eyes/IPD and presence
  are canonical in [research/44–53](../research/44-hardware-enablement-audit-methodology.md);
  these evidence docs do not add rungs or reorder this path.
- **Docked mode, sharing bridges, avatar, mapping**: each behind its own recorded gate
  (ADR 0015; spatial-sharing; S-1/R-1; M0), joined to this path only after G3.

### 5.2 Satellite registers (gate detail lives there; order authority lives here)

The four review-disposition backlogs record *what* each gate must prove; this section owns the
claim that the gated work waits:

- [design-backlog.md](design-backlog.md) — the **Lynx R1 spike** gate (Android-family donor/
  update/backend machinery) + pre-release design items.
- [perception-design-backlog.md](perception-design-backlog.md) — the **P-1 BSP kill-test** gate
  (camera/timestamp/GPU-path reality) + pre-prototype specification items (its #5/#8 are now
  specs) + pre-release qualification.
- [mapping-design-backlog.md](mapping-design-backlog.md) — the **M0 foundations spike** gate
  (keyframe packet, online mapper, reset epochs) + design-before-milestone items.
- [avatar-design-backlog.md](avatar-design-backlog.md) — the **S-1 sensing / R-1 render**
  kill-gates + pre-implementation items; where the audio-inferred rung is selected, S-1 consumes
  research/43's R4 native-source result before model/timestamp qualification.

## 6. Standing references

The dependency graph that orders this: [desktop-environment.md §6](desktop-environment.md).
Milestone acceptance: [zxr-shell-v2-composition.md §7.5](zxr-shell-v2-composition.md).
R0 gates and base evidence: [research/39 §5](../research/39-compositor-base-landscape.md).
Session/greeter/lock contracts: [ADR 0007](adr/0007-session-greeter-lock.md) +
[specs/session-auth.md](../../specs/session-auth.md) + [specs/session-bootstrap.md](../../specs/session-bootstrap.md).
First-run/onboarding (the F-track): [first-run-onboarding.md](first-run-onboarding.md) +
[ADR 0017](adr/0017-first-run-provisioning.md).
Update health gating: [images-and-updates.md §Health-gated success](images-and-updates.md).
The dev loops this path runs on: [README §Development](../../README.md).
