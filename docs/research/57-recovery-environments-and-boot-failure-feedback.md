# 57 — Recovery environments and boot-failure feedback: what shipping systems do, and why

**Research date:** 2026-09-25. **Question:** when a device fails to boot into a usable session,
how do shipping systems (a) tell the person holding it, (b) get it into a recovery environment,
(c) structure that environment and its factory reset, and why — so that the Mura recovery
environment ruled in [research/56 §3](56-defaults-from-comparables.md) is derived, not invented.
**Method:** AGENTS rules 7/8; pinned clones (`references/`, file:line) first, [external] sources
named and verified where the comparable is not cloned. **Budget impact** (overview invariant 9):
boot-time only — plymouth in the initrd and one more BLS entry; nothing on the frame path.

## 1. Two failure classes, two mechanisms

Every comparable separates **bad code** (a broken update) from **bad state** (persist data,
settings, calibration). Code is the bootloader's: systemd-boot/RAUC/U-Boot/Android A/B fall back
to the other slot. State is the OS's: a recovery environment that can reset it. Android has both
and automates the escalation (Rescue Party); pmOS has neither beyond an initramfs shell and a
cable; systemd provides the framework for the state half and leaves the trigger to the OS. Mura's
gap (research/56 §3): a state fault in an already-good slot is invisible to the bootloader.

## 2. How the person is told

| Comparable | Surface | What it says | Why (stated) |
|---|---|---|---|
| postmarketOS initramfs — `pmaports/main/postmarketos-initramfs/init_functions.sh:1291-1297`, `:588`, `:1086-1101`, `:1407-1418`; `postmarketos-bootsplash/20-plymouth.conf` | plymouth on the panel (`plymouth update --status=error` + `display-message`), then a debug-shell getty | the failure in one line + a troubleshooting URL ("Unable to mount root partition\nhttps://postmarketos.org/troubleshooting"; "Boot anyways by pressing Volume-Up…"); the shell prints how to continue, read the log, and `pmos_logdump` to expose logs over USB; the logs disk carries "Something went wrong and your device did not boot properly… open a new issue… attach the following file" | the splash is the only surface a phone has; `plymouth.ignore-serial-consoles` because "if plymouth detects *any* serial console, it disables the splash" |
| Mobile NixOS — `boot/init/lib/task.rb:77-83,102`, `boot/error/main.rb:304-313`, `boot/splash/ui.rb:283`, `boot/init/tasks/splash.rb:23-24` | its own LVGL splash, then a "sad phone" error screen | code + title + message ("Hung Tasks"; "N seconds left until boot is aborted"; "Booting to recovery menu"), actions: cancel time-out, power off, reboot modes | "Fail with a black backdrop, and force the message to stay up 60s"; "Don't fail the boot if the splash fails" |
| GDM — `gdm/data/gdm.service.in:8-20`; greetd — `greetd/greetd.service:3` | none of their own; they take the display from plymouth | — | GDM "quits plymouth on its own… if it fails for any reason, make sure plymouth still stops" (`OnFailure=plymouth-quit.service`); greetd orders after `plymouth-quit-wait` |
| Meta Quest (bootloader) — **[external, meta.com/help/quest/149134797159340, verified 2026-09-25]** | a flat text menu on the panels, one per eye | Boot device / Factory reset / Power off (Quest 2: also sideload); navigated with Volume, confirmed with Power; the wipe asks "Yes, erase and factory reset" | evidence only (rule 2): a shipping headset presents recovery as flat text duplicated per eye, driven by the hardware buttons, with a confirm on the destructive action; a PC-side "software update tool" exists for an unresponsive device |
| Android Rescue Party — **[external, source.android.com/docs/core/tests/debug/rescue-party, verified]** | the last level reboots into recovery, which *prompts* | "prompts the user to perform a factory reset"; recovery "must … provide a way for users to confirm any destruction of user data before proceeding… should also give the user the option of attempting to boot their device again" | "Because each rescue level can add up to five minutes before a device is operable again, device manufacturers shouldn't add custom rescue levels. Increased time with an inoperable device makes users more likely to initiate a support or warranty inquiry instead of self-recovering their device." |
| XR clones in `references/` (monado-galaxyxr, wivrn, alvr, envision, archive-steam-frame, halium-docs) | — | — | **no evidence** of an OS boot-failure screen; the Frame archive only lists `steamos-factory-reset` and `factory-reset.target` by name |

**What transfers.** (1) The surface is the panel, immediately, on the first failure — pmOS and
Mobile NixOS both show the failure as it happens; nobody waits three boots to say something.
(2) The content is: what failed, how to reach the device, where to read more — pmOS's one line +
URL, Mobile NixOS's code + actions. (3) The form on a headset is flat text per eye driven by the
hardware buttons — the one shipping headset comparable does exactly that at the bootloader
level. (4) Android's stated reason for keeping escalation short is Mura's too: time with an
inoperable device is the cost; feedback and the way in must come first, the drastic step soon.

## 3. How recovery is entered

| Comparable | Entry | Mechanism |
|---|---|---|
| systemd — `docs/FACTORY_RESET.md:128-137`; `man/systemd-boot.xml:550-554`; `man/systemctl.xml:2777-2784` | a **boot menu entry** that boots the normal initrd into a unit (`rd.systemd.unit=factory-reset.target`, or `systemd.factory_reset=1`); a one-shot entry selection from the running system (`systemctl reboot --boot-loader-entry=ID` → `LoaderEntryOneShot`) | the same kernel and initrd, a different target — systemd's documented shape |
| Mobile NixOS — `doc/in-depth/android/boot.adoc:23-30`; `boot/init/tasks/recovery.rb:19-51` | the recovery partition holds "a fully functional stage-1 able to boot in the system"; entered by `reboot recovery` or held keys | *"Tinkering on-device is, thus, less scary as one can flash a boot.img knowing that they can reboot to recovery if it fails"* |
| Android / Lineage — **[external]** `android_bootable_recovery/recovery_ui/device.cpp:34-52` (lineage-22.2) | `reboot recovery` (bootloader control block) or a key chord | a separate recovery image the OS ships on the recovery partition |
| SteamOS — **[external, Valve recovery instructions, verified via mirrors]** | Volume-Down + Power → boot manager → "EFI USB Device" | a bootable USB image with a desktop; no recovery partition (UEFI) |
| pmOS — `init_functions.sh` `check_keys`, `debug_shell`, `fail_halt_boot` | held Volume keys, or any hard failure | the *same* initramfs frozen in a shell |

**What transfers.** systemd's and Mobile NixOS's shape coincide: the recovery environment is the
same stage-1, booted to a different unit. On a UEFI family that is a BLS entry and
`--boot-loader-entry`; on an Android-derived family it is the same initrd packaged as the
recovery image and `reboot recovery`. A third root filesystem (SteamOS's full desktop on USB) is
the outlier and needs external media.

## 4. What the environment offers, and how factory reset works

| Comparable | Offers | Factory reset | Remote access | Why |
|---|---|---|---|---|
| systemd — `docs/FACTORY_RESET.md:10-20,24-62,96-105`; `NEWS:9887-9893`; `man/repart.d.xml:904-908`; `man/systemd-repart.xml:138-144`; `units/factory-reset*.target`, `units/systemd-factory-reset-*.service*`, `units/systemd-repart.service:24` | the framework: `factory-reset.target` (request + reboot), `factory-reset-now.target` (execute in the initrd), Varlink `io.systemd.FactoryReset` for UIs | request: `systemctl start factory-reset.target` → `systemd-factory-reset request` sets the `FactoryResetRequest` EFI variable (UEFI; non-UEFI passes `systemd.factory_reset=1` on the next cmdline) → reboot; execute: the generator pulls `factory-reset-now.target`, `systemd-repart` (ordered `Before=factory-reset-now.target`) deletes every `FactoryReset=yes` partition and "immediately re-creat[es] these partitions anew empty"; `systemd-factory-reset complete --retrigger`; boot continues | — | "Factory reset always takes place during early boot, i.e. from a well-defined 'clean' state"; the target is "a framework where to plug in the implementation"; UIs "should first check if requesting a factory reset is supported at all via the Varlink service" |
| Lineage Recovery — **[external]** `device.cpp:34-59` | Reboot system now · Apply update · Factory reset (Format data/factory reset, cache, system) · Advanced (Enter fastboot, Reboot to bootloader/recovery, Mount system, View logs, Enable ADB, Enter rescue, Power off) | format data | ADB (sideload; "Enable ADB") | the OS owns its recovery; the vendor's is not assumed |
| SteamOS recovery image — **[external]** | Re-image Steam Deck · Clear local user data · Reinstall SteamOS · Recovery tools (a terminal) | reimage / clear home partitions | a desktop with a terminal | full reinstall from external media |
| Mobile NixOS recovery menu — `recovery-menu/main.rb:21-36` | pick a generation, reboot modes, power off | not a wipe | none | pseudo-A/B safety net |
| pmOS debug shell — `init_functions.sh:1086-1101,1192-1194` | continue boot, read the log, `pmos_logdump` to a USB mass-storage "PMOS_LOGS", optional on-screen keyboard | none in the initramfs | telnet on `172.16.42.1:23` over the USB gadget ("ACM gadget mode is not supported on some old kernels so this exists as a fallback"); not ssh | the developer's cable |
| Android Rescue Party — **[external]** | levels reset settings, then `rebootPromptAndWipeUserData` | recovery's `--prompt_and_wipe_data`, confirmed by the person | adb | see §2 |

**What transfers.** Factory reset is systemd's: mark `syspersist` and `home` `FactoryReset=yes`
in the family's `repart.d`, and the recovery menu's reset is `systemctl start factory-reset.target`
— confirmed by the person, never automatic (Lineage, Rescue Party, Quest all confirm). Remote
access in recovery is standard on every comparable (telnet, adb, a terminal); Mura's is sshd on
the gadget, which NixOS already provides in the systemd initrd for LUKS unlock
(`boot.initrd.network.ssh`) — rule 1. A slot switch (`mura-bootconf set-primary`) is Mobile
NixOS's "pick a generation". Reflash from recovery (Lineage's "Apply update", SteamOS's
"Reinstall") is the follow-up in the track.

## 5. Determinations for Mura (rule 8: converging, applied)

1. **Feedback on the first hard failure, on the panels, with the ways in.** plymouth in the
   normal initrd; on a hard preflight failure, `plymouth display-message` with the failing
   check, `ssh mura@172.16.42.1`, the hotspot SSID and PSK when up, and the docs URL; the setup
   launcher shows the same. (pmOS, Mobile NixOS; Rescue Party's time-cost reason.)
2. **The recovery environment is the same initrd booted to `mura-recovery.target`**, packaged per
   family: a `recovery.conf` BLS entry on uefi-rauc, reached with
   `systemctl reboot --boot-loader-entry=recovery`; the recovery boot image on the Android-derived
   targets, reached with `reboot recovery`. (systemd's boot-menu shape; Mobile NixOS's
   recovery-is-stage-1.)
3. **Automatic at the count** (ruled by the owner, research/56 §3): `mura-crashloop` at
   `crashLoopThreshold` reboots into it — Android's escalation shape *to a prompt*, without its
   automatic wipe levels.
4. **Contents:** sshd on the gadget (host key from `identity/ssh` when `/persist` mounts, else
   generated, fingerprint shown), the panel screen listing what is available, and offered actions
   — factory reset via `factory-reset.target` (repart `FactoryReset=yes` on `syspersist` and
   `home`), slot switch, reboot. Reflash follows.
5. **The panel screen's form:** flat text per eye, drawn by plymouth under the recovery
   illustration; the one shipping headset comparable (Quest) uses exactly a flat per-eye text
   menu driven by the hardware buttons. Button navigation of the menu (volume/power → plymouth
   keystrokes) is the natural next step for the track; this rung's actions are taken over ssh and
   the console.

## 6. Open items

Button-driven menu on the panels (Quest's shape) — decider: the recovery track after the
input-floor rung (F4); reflash from recovery (RAUC bundle over ssh; Lineage "Apply update") —
the track's follow-up; the Android-derived families' `reboot recovery` and cmdline-carried
`systemd.factory_reset=1` — with each device's bring-up; whether the hotspot comes up in the
recovery initrd (NetworkManager is not in stage 1; the gadget is) — decider: the track, after
measuring what the recovery initrd costs.
