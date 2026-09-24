# 48 — Wi-Fi/Bluetooth native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Hardware/firmware to NetworkManager/BlueZ; provisioning/pairing UX remains in doc 42/onboarding.

## Verdict

| Device/profile | Native evidence | State |
|---|---|---|
| Quest 1 / `monterey:base` | complete downstream WCN3990 blueprint; no native board/BDF/userspace closure | A0 |
| Lynx R1 / `lynx-r1:base` | QCA6390 DT/drivers; packaged radio firmware/board data missing | A0 |
| Galaxy XR / `sm-i610:base` | Wi-Fi 7/BT 5.4 only below stock feature level | A0 |
| Play For Dream / `pfdm-mr:base` | Wi-Fi 7/BT 5.3 only below stock feature level | A0 |
| Steam Frame / `deckard:base` | WCN7850 DT, firmware and stock services; source/runtime joins incomplete | S1 |
| Quest 3 / `eureka:base` | WCN6856 teardown evidence; host BT path unknown | A0 |

No dongle profile qualifies. Frame's bundled Wi-Fi adapter is PC-side; peripherals consume the
headset adapter rather than replacing it.

## Per-target L0–L7

### Quest 1

Meta's build-specific kernel declares WCN3990 SNOC/ICNSS WLAN and WCN3990 BTFM supplies/clocks
([external] [DT](https://github.com/facebookincubator/oculus-linux-kernel/blob/589280fc40ddbcc2287024c8b672568a0fdd68e7/arch/arm/boot/dts/qcom/msm8998.dtsi#L3075-L3124)).
Stock uses qcacld/ICNSS/BTFM rather than mac80211. Native ath10k needs exact WLAN DSP, Quest BDF/
regulatory data and BT payloads from the selected donor; none is hash-bound. No native netdev,
`hci0`, NetworkManager, BlueZ, pairing or AP+STA evidence exists. Quest controllers use a
proprietary SyncBoss radio, not ordinary Bluetooth.

### Lynx R1

The source-verified radio is QCA6390, not the older QCA6174 hardware-note claim: mainline DT uses
PCI `17cb:1101` and `qcom,qca6390-bt`. Kernel config enables ath11k PCI, QCA HCI UART and WCN power
sequencing, but `device-lynx-r1` depends only on GPU/DSP firmware
(`references/pmaports/device/testing/device-lynx-r1/APKBUILD:10-21`) and
`firmware-lynx-r1` has no WLAN/BT subpackage
(`references/pmaports/device/testing/firmware-lynx-r1/APKBUILD:1-18`).
No radio enumeration log exists. Exact BDF/calibration/MAC source, `iw phy`, `hci0`, NM/BlueZ and
AP+STA are runtime gates.

### Samsung Galaxy XR

Samsung vendor-documents tri-band Wi-Fi 7 and Bluetooth 5.4
([external] [Samsung specification](https://www.samsung.com/uk/xr/galaxy-xr/galaxy-xr-silver-shadow-sm-i610nzsaeub/)).
Chip, bus, PMU, PCI/UART IDs, DT, modules, firmware, board data, regulatory/calibration and native
ABI are unknown; the official source archive is unpinned. Stock pairing/Wi-Fi is not native
evidence. Simultaneous Wi-Fi/BT behavior needs runtime adjudication.

### Play For Dream MR

Vendor docs list Wi-Fi 7/BT 5.3 and stock streaming. No chip, bus, rails, DT/kernel, firmware,
board/regulatory data, HCI transport or native interface exists publicly. PFDM developer repos are
SDK samples, not board support; FreeXR's one unlocked unit provides no radio chain.

### Valve Steam Frame

- **L0–L2:** production DT identifies WCN7850 WLAN PCI `17cb:1107`, GENI-UART BT at 3.2 Mbaud and
  WCN7850 PMU supplies/enables/clock
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:2094-2118,3147-3169,9336-9401`).
  Config enables ath12k, mac80211/cfg80211/rfkill and QCA HCI; corresponding source is missing.
- **L3:** donor has WCN7850 `amss.bin`, `m3.bin`, exact `board_77.bin`, regdb and QCA BT firmware/
  NV candidates plus signed regulatory DB. Per-unit EEPROM MACs must not be transplanted.
- **L4–L6:** stock scripts name `wlan0`/`hci0`; rootfs contains NetworkManager, BlueZ, hostapd,
  iwd, wpa_supplicant and soft-AP tooling. No captured bind, `iw`, HCI, pairing or suspend result.
  BT blob selection remains unproven.
- **L7:** S1 static source coverage only. Current Mura declaration says native/BT present while the
  evaluated image does not enable NetworkManager or BlueZ; declarations are not qualification.

The vendor “dual radio” claim conflicts with one visible PCI function/`wlan0`; `lspci` and
simultaneous `iw phy/dev` traces must identify whether this is internal multi-radio operation or
hidden firmware virtualization.

### Meta Quest 3

Teardown identifies WCN6856 Wi-Fi 6E and nRF52833 BLE. WCN6856 suggests ath11k, but no Eureka
board DT/BDF/firmware selection exists. The nRF host transport/firmware and whether it exposes
standard HCI are unknown; do not assume it is the host Bluetooth controller merely from the chip
presence. No native netdev or `hci0`.

## Artifact and identity boundaries

Radio firmware may be hash-bound donor input, but BDF/board data, calibration, regulatory
certification and per-unit MAC/NV must remain profile/revision-bound. Frame's mixed firmware is
`localOnly`; Lynx standard QCA payload availability does not prove the correct BDF; Quest/Galaxy/
PFDM board closures are proprietary or unavailable. MACs and link keys are never copied between
units; `/var/lib/bluetooth` remains device pairing state.

## Runtime evidence bundle

R1 requires exact firmware/BDF selection, regulatory/MAC identity, PCI/SNOC/UART binding, `iw phy`,
netdev, rfkill, `hci0`, BlueZ Adapter1 and NetworkManager Device. R2 proves STA traffic, BT pairing/
traffic, AP DHCP/NAT and firmware recovery. R3 verifies certified bands/DFS/6-GHz power, unique
MAC/NV, coexistence and proprietary-controller separation. R4 proves persistent NM station,
ordinary BlueZ pre-login/session pairing and provisioning AP handoff; `concurrentApSta=true` needs
both advertised combination and real simultaneous test. R5 covers cold boot, rfkill, suspend,
XR streaming/coexistence and crash recovery.

## Contract consequences

`wifiBt.backend=native` does not itself enable firmware, NetworkManager or BlueZ. `bluetooth=true`
currently also gates persistent state/UI despite no `hci0`; operational use must wait for R1.
Every `concurrentApSta` stays `null` until R4. Quest 3 may require separate Wi-Fi and BT backend
classes, but research does not license a new option. Standard mac80211/BlueZ/NM remains the default.

## Contradictions / deciders

- Lynx QCA6174 note versus QCA6390 source: physical `lspci -nn`/marking.
- Frame dual-radio marketing versus one PCI function: `iw phy/dev` simultaneous traces.
- Frame Wi-Fi MAC service with no located activation path: stock systemd/journal.
- Galaxy simultaneous radio limits: stock/native traffic test.
- Combined `wifiBt` backend versus potentially separate Quest 3 nRF path: transport discovery.
