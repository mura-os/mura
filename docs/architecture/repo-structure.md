# Mura architecture: repository structure and patch management

**Status:** draft. Derived from [00-synthesis](../research/00-synthesis.md) and the layout precedents
in [01-mobile-nixos](../research/01-mobile-nixos.md) §2/§3, [02-postmarketos](../research/02-postmarketos.md)
§2, and [04-nix-imaging](../research/04-nix-imaging.md) §2/§9.

## Monorepo, with vendored sources kept out

Mura is a **single flake-based monorepo**. The device/SoC/family decomposition, the module
system, and the image builders all share one evaluation and one pinned nixpkgs; splitting them across
repos would reproduce the Mobile-NixOS/Tow-Boot duplication (their image-builder is copied between
repos — [01](../research/01-mobile-nixos.md) §10 item 9) with no benefit at this scale. The one
deliberate seam is **boot firmware**: if an `abl-uboot` boot component grows, it becomes a separate
flake input (the thin Mobile-NixOS↔Tow-Boot contract, [01](../research/01-mobile-nixos.md) §12), not
a second copy of shared infrastructure. See [adr/0001](adr/0001-monorepo-vs-subprojects.md).

Large upstream sources (nixpkgs, Monado, kernels, AOSP trees) are **never vendored into the tree**;
they enter as flake inputs or hash-pinned fetches. Reference clones for study live under
`references/` and are git-ignored, reproducible from `references/clone.sh` + `references/MANIFEST.json`.

## Layout

```text
flake.nix                      # pinned inputs, dev shell, checks, per-device outputs
flake.lock
lib/
  images/                      # image-variant deferred modules (image.modules), repart + android packer
  donor/                       # acquire/identify/parse/extract/qualify derivation builders
  contract/                    # device-contract option types + assertions
modules/
  os/                          # common distribution policy (device-independent); one file per concern — see the ownership table below
  xr/                          # Monado runtime, session, StardustXR shell wiring
  adaptation/                  # per-subsystem backend options: native | android-backed | device-specific
    android-compat/            # libhybris/android-headers/late-LXC building blocks (optional)
profiles/                      # declared *configurations* a device or image imports explicitly (NixOS's profiles/ shape):
  default.nix                  #   the default image: user `mura`, no password, wheel, autoLogin — the image is the installation
  multi-user.nix               #   the greeter profile (a declared human account is build-asserted)
  dev.nix                      #   SSH keys, serial console, VM conveniences — never in a shipped image
soc/
  msm8998/  sm8250/  sm8550/  sm8650/   # shared per-SoC integration (kernel base, firmware paths, DSP stack)
families/                      # shared definitions for near-identical models (plain imports)
devices/
  virtual-headset/             # x86_64 VM smoke target (no hardware)
  <vendor>-<model>/            # device contract, donor manifest, kernel cfg, patches, tests, contract file
pkgs/                          # overlay: XR components, kernels, tools (nixpkgs-xr pulled as input).
                               # Mura's own programs are Rust (AGENTS.md rule 6); tests/closure.nix
                               # proves the login-path closure carries no interpreter
  mura-authd/                  # the lock-path PAM helper + conformance harness (Rust; D5) — pkgs.mura.authd
  mura-session/                # the session wrapper greetd execs, `start`/`finalize` (Rust, libc only; D4 rev 3) — pkgs.mura.session
  mura-preflight/              # the XR preflight probe P1–P7 → /run/mura/preflight.json (Rust; D6) — pkgs.mura.preflight
  mura-setup/                  # the setup program's system instance — D3 STUB (Rust, libc only); `--recovery` = the
                               # recovery page in stage 1 — pkgs.mura.setup
  mura-recovery/               # the recovery menu: actions once, three frontends (panel/evdev+plymouth, shell, web via
                               # mura-setup) — specs/recovery-menu.md (Rust) — pkgs.mura.recovery
  mura-plymouth-theme/         # the boot / failure-feedback / recovery screen: the master illustration composited per
                               # device from the contract's panel geometry (called by modules/os/recovery.nix, not an overlay attr)
assets/
  branding/                    # checked-in artwork: recovery-mode.png (the mascot; 2048², 8 bpc RGBA, black background)
patches/                       # patch sets, organized per upstream + per donor build (see below)
protocols/                     # Mura Wayland protocol XMLs (zxr-shell-v2, the zspatial
                               # shell-integration family incl. zspatial-toplevel-export) + governance
                               # notes + CONVENTIONS.md; CI: wayland-scanner + xmllint (tests/protocols.nix)
specs/                         # normative non-Wayland contracts (IPC framings, storage formats,
                               # D-Bus/PipeWire interfaces) — the peer of protocols/
tests/                         # eval assertions (contract, persist, protocols, closure), VM tests (vm/: default-image,
                               # multi-user, oob, health, recovery), reproducibility + hardware tests
contracts/                     # reviewed, hash-bound donor contracts (the qualify-stage gate)
docs/
  research/                    # the seven research docs + synthesis
  architecture/                # this set
references/                    # git-ignored study clones (clone.sh + MANIFEST.json tracked)
```

This mirrors the convergent device→SoC→family→common decomposition ([00](../research/00-synthesis.md)
§1) and Jovian's device-module-with-capability-flags layout ([04](../research/04-nix-imaging.md) §3),
which was the cleanest of the five Nix projects studied.

### `profiles/` — declared configurations, opt-in by import

A **profile** is what a desktop distribution's installer would have produced: the accounts,
the login profile, the conveniences. Here the image is the installation
([first-run-onboarding.md §1](first-run-onboarding.md)), so a profile is a NixOS module a device
or image **imports explicitly** — the shape of NixOS's own `profiles/`. Nothing in `modules/`
declares a user; `modules/` owns mechanism, `profiles/` owns the declared configuration.

| Profile | Declares | Used by |
|---|---|---|
| `profiles/default.nix` | `users.users.mura` (`isNormalUser`, no password, `wheel`), `mura.xr.session.autoLogin = "mura"`; locale/timezone defaults; nothing else | the default image of every device; the default-image VM fixture |
| `profiles/multi-user.nix` | `mura.xr.session.greeter = "zxr-greeter"`, `multiUser.enable`; declares **no** account — the importer must (the contract's declared-account assertion enforces it) | shared-device images; the multi-user VM fixture adds a `hashedPasswordFile` user |
| `profiles/dev.nix` | SSH with authorized keys, serial console, `PasswordAuthentication` for the VM only, port forwards, the stand-in swaps' comments | `devices/virtual-headset`; a developer's own image. **Never a shipped image** |

A self-builder who wants neither imports none of them and declares their own users
(`hashedPasswordFile`) — Path A in the corpus's older vocabulary.

### `modules/os/` — one file per concern (ownership table)

Each file consumes a slice of the `mura.*` contract and owns the NixOS options it sets; no two
files set the same NixOS option. Design authority in the right-hand column.

| File | Consumes | Owns (NixOS surface) | Design |
|---|---|---|---|
| `os/default.nix` | `mura.device.*`, `mura.hardware.soc`, `mura.deployment.bootScheme` | distro identity, D-Bus/polkit enable, `/etc/mura-device.json`, hostname/locale defaults | [overview.md](overview.md) |
| `os/session.nix` | `mura.xr.session.{autoLogin,greeter,allowNoDeclaredAccount,readinessTimeoutSeconds}`, `mura.xr.shell` | `services.greetd` (appliance `initial_session` / multi-user `default_session`), the greeter user, the session wrapper (`pkgs.mura.session`, `mura-session start`, D4 rev 3), the four static user units (`mura-compositor.service`, `mura-session.target` (B6/B6a), `mura-session-bindpid@`, `mura-session-shutdown.target`), `environment.d/60-mura.conf`, the `# STAND-IN` lines (sway as `ExecStart`, sway config `exec mura-session finalize`) | [implementation-path.md §2 (ii)](implementation-path.md), [specs/session-bootstrap.md](../../specs/session-bootstrap.md), ADR 0007 |
| `os/persist.nix` | `mura.hardware.input.bluetooth`, family mount facts | `/persist/mura` class skeleton, the `/etc` overlay (`system.etc.overlay`, upper on `/persist/etc-rw`) and userborn, `/var/lib/bluetooth` → `pairing/` bind, F1 reference units (SSH host key, `mura-f1-seed-state`) | [first-run-onboarding.md §2–§3](first-run-onboarding.md), [multi-user.md §1.1](multi-user.md) |
| `os/policy.nix` | `mura.xr.session.faillock.*`, `mura.xr.session.greeter` | PAM services per the posture table (`login` `allowNullPassword`; `mura-lock` without it; faillock rules with `conf=` on `login`, `sshd` and `mura-lock`; **standard sudo/polkit**); the greeter NetworkManager polkit rule; `services.logind.settings.Login.HandlePowerKey = "ignore"`. **Never touches `services.openssh`** — sshd is enabled with upstream defaults in `os/default.nix` (`mkDefault`), D2 posture correction | [first-run-onboarding.md §5.3](first-run-onboarding.md), [multi-user.md §3.1](multi-user.md) |
| `os/oob.nix` | `mura.hardware.input.{usbGadget,concurrentApSta}`, `mura.oob.hotspot.idleTimeoutMinutes` | USB gadget (initrd configfs, `mura-usb-gadget`), `systemd-networkd` DHCP server on `usb0` (NM-unmanaged; firewall 67/80), NetworkManager hotspot profile + per-boot PSK + `dnsmasq-shared.d` + the `mura-hotspot` supervisor (marker + no other active connection + idle timeout), NM `firewall-backend=iptables`, the `mura-setup` identity + `50-mura-setup.rules` + the system-instance unit (stub page on the gadget + hotspot addresses), Avahi `mura.local`. No Cockpit, no sshd `Match` block | [first-run-onboarding.md §5](first-run-onboarding.md) |
| `os/health.nix` | `mura.health.{crashLoopThreshold,deviceWaitSeconds}`, `mura.xr.calibration.paths`, `mura.qualification.readinessCheck` | `mura-preflight` (P1–P7 → `/run/mura/preflight.json` + `.summary`; greetd `Requires=` it), `mura-crashloop` (at the threshold: `mura.recovery.rebootCommand`, else `mura-recovery.target`) (B1b), `mura-readiness` → `boot-complete.target` (B9, target-reached). The slot half (`+N` arming in `mura-bootconf`, `mura-mark-good`, `mura.deployment.bootTries`) is the uefi-rauc family's | [implementation-path.md §3a, §3a-bis](implementation-path.md) |
| `os/recovery.nix` | `mura.hardware.{panel,displays}` (theme geometry); sets `mura.recovery.{rebootCommand,switchSlotCommand}` (family-provided) | plymouth in the initrd with the per-device Mura theme; `mura-preflight-feedback` (the failing checks + the ways in on the panels on the first hard failure; `plymouth-quit` skipped on the `/run/mura/preflight.failed` marker); the stage-1 `mura-recovery.target` (networkd on `usb0`, `mura-recovery-identity` — the device's host key from `/persist` or a generated one — `mura-recovery-sshd`, `mura-recovery-panel` (`mura-recovery panel`: the HMD buttons per `/etc/mura/recovery.json` from the contract, plymouth), `mura-setup-recovery` (`mura-setup --recovery` on the gadget/hotspot addresses); the actions — factory reset via `systemd-repart --factory-reset`, slot switch, reboot, power off — in `pkgs/mura-recovery`, specs/recovery-menu.md). The family adds the boot entry (`uefi-rauc`: `recovery.conf`, `FactoryReset=yes` on `syspersist`/`home`) | [implementation-path.md §4 Mura recovery environment](implementation-path.md), [research/57](../research/57-recovery-environments-and-boot-failure-feedback.md) |
| `xr/default.nix` | `mura.xr.{runtime,environment,monado.*}` | `services.monado`, the active runtime manifest | [device-contract.md §xr](device-contract.md) |
| `adaptation/*` | `mura.adaptation.*` | per-subsystem backend wiring | ADR 0003 |

Rule: a new NixOS option set in `modules/os` lands in the file whose *design* column governs it,
or the table gains a row first.

## The flake entry point

One integration path (avoiding Mobile NixOS's two-entry-point wart —
[01](../research/01-mobile-nixos.md) §10 item 4):

- `nixosModules.default` — the `mura.*` module set, usable in any NixOS config.
- `muraSystem = { device, ... }: …` — mirrors robotnix's `lib.robotnixSystem`
  ([04](../research/04-nix-imaging.md) §9 item 1); evaluates a device into its artifacts.
- `packages.<system>.<device>-<variant>` — named, discoverable flake outputs (no untyped
  `build = types.attrs` grab-bag — [04](../research/04-nix-imaging.md) §10 item 9).
- `checks.<system>.*` — `nix flake check` wiring: formatting, contract assertions, the
  `virtual-headset` build, per-donor golden-hash tests, tier-consistency checks.

A flake/non-flake bridge (Jovian's `nixpkgs.nix`, [04](../research/04-nix-imaging.md) §9 item 13)
keeps both consumption modes on identical pins.

## Patch management (many patches expected)

The project will carry many patches — kernel trees, Monado per-device drivers, occasional vendor
fixups. The corpus is emphatic about how this goes wrong: robotnix's per-vendor-branch patch forests
multiply rebase work at every upstream release ([04](../research/04-nix-imaging.md) §10 item 2).
Rules:

1. **Prefer config over patches.** Jovian reproduces almost all of Valve's kernel via
   `structuredExtraConfig` + `mkForce` and cmdline flags rather than source patches
   ([04](../research/04-nix-imaging.md) §5, §9 item 5). Reach for a patch only when config cannot
   express the change.
2. **Patches are pinned data with provenance.** Each patch series lives under `patches/<upstream>/`
   or `patches/<donor-buildid>/`, applied via `applyPatches`/`FetchContent`-equivalent at build time.
   The XR per-device driver is a `monado-rev` file + `patches/monado/<device>/*.patch` — exactly
   WiVRn's proven 11-patch pattern ([05](../research/05-xr-userspace.md) §9 item 2).
3. **Isolate per-donor-release patch directories** with automated rebase checking in the update
   pipeline, so a donor bump surfaces broken patches loudly
   ([04](../research/04-nix-imaging.md) §10 item 2).
4. **Ship the diff as a release artifact.** Following Tow-Boot, emit `savedefconfig`, the original
   and final `.config`, and a unified diff of every patch applied to a source tree
   ([01](../research/01-mobile-nixos.md) §9 item 12) — outstanding practice for a project that
   patches vendor trees.
5. **Upstream as exit strategy.** Track which patches are candidates for upstreaming (Monado
   explicitly invites driver upstreaming; nixos-apple-silicon shrank to kernel+U-Boot by upstreaming
   — [04](../research/04-nix-imaging.md) §9 item 11, §11 item 8). Layout separates overlay-per-
   component so a piece can be deleted when it lands upstream.

## Input pinning and update automation

At fleet scale (6+ headsets × {kernel, firmware, blobs, XR components}), manual hash bumps drown you
(Jovian's constants-in-file is fine for one device — [04](../research/04-nix-imaging.md) §10 item 8).
The design:

- **Lockfile-driven ingestion** for every donor input class, produced by tooling not typed by hand
  (robotnix `repo2nix` + `update.sh`; the un-automated parts of robotnix are exactly what rotted
  during its 3-year gap — [04](../research/04-nix-imaging.md) §2.1, §9 item 2).
- **nixpkgs-xr as a flake input** for the XR stack, reusing its nvfetcher daily-cron pin architecture
  (including the WiVRn→Monado cross-pin scrape) and cachix cache
  ([05](../research/05-xr-userspace.md) §2.4, §9 item 3).
- **Scheduled bump PRs** (nvfetcher/Renovate) for Mura's own pins.
- **Release/tracking discipline** (nixos-apple-silicon, [04](../research/04-nix-imaging.md) §9 item
  11): `main` on nixos-unstable, `release-YY.MM` per NixOS stable, dated tags with CI-built artifacts
  and a written on-hardware test protocol.

## Caches and redistribution boundaries

- Public artifacts (common userspace, XR components, kernels) are cacheable and should have a public
  binary cache (apple-silicon and robotnix both needed one for kernel/Mesa-class builds).
- Donor-derived artifacts (extracted vendor firmware) are legally non-redistributable: the
  `licensing.redistributable = false` flag mechanically forces `allowSubstitutes = false;
  preferLocalBuild = true` and exclusion from cache-push ([06](../research/06-donor-pipeline.md) §6).
  This likely means a private cache tier per device for its qualified-donor outputs.
- Signing keys live outside the store; a separate controlled stage signs qualified artifacts
  ([04](../research/04-nix-imaging.md) §9 item 8).
