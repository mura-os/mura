# 57 — Recovery environments and boot-failure feedback: what shipping systems do, and why

**Research date:** 2026-09-25. **Question:** when a device fails to boot into a usable session,
how do shipping systems (a) tell the person holding it, (b) get it into a recovery environment,
(c) structure that environment and its factory reset, and why — so that the Mura recovery
environment ruled in [research/56 §3](56-defaults-from-comparables.md) is derived, not invented.
**Method:** AGENTS rules 7/8; pinned clones (`references/`, file:line) first, [external] sources
named and verified where the comparable is not cloned. **Budget impact** (overview invariant 9):
a dedicated recovery boot partition per image family (uefi-rauc: 512 MiB XBOOTLDR carrying one
kernel+initrd; Android-derived: a separate Mura recovery boot image only where the boot chain can
select an additional partition without replacing stock recovery). Runtime cost is recovery-only:
plymouth and the menu are absent from the normal frame path.

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

**What transfers.** Mobile NixOS and Lineage provide the artifact boundary Mura needs: a
dedicated recovery boot image containing stage 1, independently bootable from the normal OS
image. Their assumption that this image may replace the device's stock `recovery` partition does
not transfer: Mura adds its own partition and preserves stock/vendor recovery as the independent
install/reflash path. systemd provides the UEFI mechanism for that additional partition:
systemd-boot reads Type #1 entries, kernels and initrds from XBOOTLDR as well as the ESP
(`references/systemd/man/systemd-boot.xml:31-48`); XBOOTLDR mounts at `/boot` while the ESP is
`/efi` (`systemd-gpt-auto-generator.xml:260-270`). RAUC's exact A/B precedent is its
**Additional Rescue Slot**: a separate raw slot for common failures affecting both normal roots;
normal updates only write A/B, and the bootloader enters rescue after repeated failures or user
request (`references/rauc/docs/scenarios.rst:147-178`). Mura registers `mura_recovery` as
`rescue.0`, so ordinary rootfs bundles leave it untouched. postmarketOS confirms that the environment
itself belongs in small stage 1: its initramfs enters `debug_shell` on Volume-Down
(`pmaports/main/postmarketos-initramfs/init_2nd.sh:35-42`,
`init_functions.sh:1223-1241`) and on a hard failure (`init_functions.sh:1434-1441`). Mobile
Thus the recovery partition carries its own kernel+initrd but no third root filesystem.
`reboot recovery` continues to mean stock recovery and is never Mura's entry command.
Android-family selection of an additional Mura partition needs proof during each bring-up; it
cannot be assumed from AOSP's fixed partition names. SteamOS's full desktop on USB is the
root-filesystem outlier.

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
in the family's `repart.d`, and the recovery menu's reset is `systemd-repart --factory-reset=yes`
run from the recovery environment (stage 1 is the clean state `factory-reset.target` exists to
reach; `systemd-factory-reset request` would add only an EFI-variable write) — confirmed by the
person, never automatic (Lineage, Rescue Party, Quest all confirm). Remote
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
2. **The recovery environment is a dedicated Mura recovery boot partition/image**: its own copy
   of the kernel and systemd initrd booted to `mura-recovery.target`, with no recovery root
   filesystem and no dependency on the normal Mura boot files. On uefi-rauc the partition is
   XBOOTLDR (`mura_recovery`) with `recovery.conf`, reached with
   `systemctl reboot --boot-loader-entry=recovery.conf`; normal Mura boot files remain on the
   ESP. On Android-derived targets `reboot recovery` is reserved for stock/vendor recovery; an
   additional bootable Mura partition and selector is a bring-up gate, not assumed here.
   (Mobile NixOS/Lineage recovery-image boundary; systemd XBOOTLDR packaging; pmOS stage-1
   contents.)
3. **Automatic at the count** (ruled by the owner, research/56 §3): `mura-crashloop` at
   `crashLoopThreshold` reboots into it — Android's escalation shape *to a prompt*, without its
   automatic wipe levels.
4. **Contents:** sshd on the gadget (host key from `identity/ssh` when `/persist` mounts, else
   generated, fingerprint shown), the panel screen listing what is available, and offered actions
   — factory reset via systemd-repart's factory reset (repart `FactoryReset=yes` on `syspersist`
   and `home`, invoked directly from the recovery environment), slot switch, reboot, power off.
   Reflash is §6's.
5. **The panel screen's form:** flat text per eye, drawn by plymouth under the recovery
   illustration; the one shipping headset comparable (Quest) uses exactly a flat per-eye text
   menu driven by the hardware buttons. The menu is one program with three frontends — the HMD's
   buttons over evdev with Android recovery's key semantics, ssh/console, the web page on the
   cable/hotspot — specified in [specs/recovery-menu.md](../../specs/recovery-menu.md).

## 6. Open items

Button-driven menu on the panels — **ruled 2026-09-25** (status: implementation-path §4) as one Rust menu program
with three frontends (panels + HMD buttons over evdev, ssh/console, the web page in stage 1;
[specs/recovery-menu.md](../../specs/recovery-menu.md), implementation-path §4). Its key
semantics are Android recovery's (`recovery_ui/ui.cpp` `ProcessKey`: register on release,
auto-repeat ignored, 750 ms long press a distinct event; `recovery.cpp`: a destructive action
behind a separate confirm menu defaulting to the safe item), not Quest's second-press countdown,
for which no comparable's source was available; reflash from Mura recovery (RAUC bundle over
ssh; Lineage "Apply update") — the track's follow-up; the Android-derived families' separate
Mura-owned recovery partition **and** one-shot selector — each device's bring-up must prove the
bootloader can select an added partition without replacing stock recovery; otherwise that target
does not yet have Mura Recovery. Stock recovery remains the reflash-from-scratch path. The
recovery hotspot is **required** (owner clarification 2026-09-25): it exposes both sshd and
`mura-setup --recovery`, alongside those same services on the USB cable. Its standard Linux
shape is hostapd + systemd-networkd's address/DHCP server in stage 1, reusing the ruled per-boot
PSK; hwsim VM verification associates a simulated phone and reaches both services.
Radio/firmware, regulatory-domain and AP/ACS-mode qualification remain per-target bring-up
gates. NetworkManager is not added to recovery stage 1.
