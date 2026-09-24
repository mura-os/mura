# 49 — Power, thermal and charging native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Hardware chain only; frame/compute budgets and idle/docked policy remain in their architecture docs.

## Verdict

| Device/profile | Native evidence | State |
|---|---|---|
| Quest 1 / `monterey:base` | downstream charger/gauge/fan/thermal blueprint; final-build pairing absent | A0 |
| Lynx R1 / `lynx-r1:base` | stock chain; mainline charger/fuel-gauge disabled, fan unproved | A0 |
| Galaxy XR / `sm-i610:base` | required external pack and cooling documented; low-level path unknown | A0 |
| Play For Dream / `pfdm-mr:base` | battery/charging/fan inventory only | A0 |
| Steam Frame / `deckard:base` | strongest DT/ABI/services; custom source/proprietary orchestration/runtime absent | S1 |
| Quest 3 / `eureka:base` | stock module/policy chain; no native board source | A0 |
| Quest 3 battery strap/dock | qualifying official profiles; native protocol unknown | A0 |

Galaxy's in-box external battery is base, not accessory. Arcturus qualifies in this domain only as
an A0 expansion-powered delta; no draw/thermal evidence is published.

## Per-target L0–L7

### Quest 1

Meta's build-specific source defines PMI8998 SMB2 charger, Gen3 gauge, TSENS/QPNP thermal and an
`oculus,fan` PWM/tach driver; production DT selects a battery profile but capacity conflicts with
teardown labeling
([external] [Meta kernel source](https://github.com/facebookincubator/oculus-linux-kernel/commit/589280fc40ddbcc2287024c8b672568a0fdd68e7)).
Stock sysfs/Health/Thermal paths are known in outline, and cycle state under `/persist/battery` is
per-unit. The source predates final v50, so exact profile/firmware pairing, native port, UPower/
policy and sustained-load results remain absent.

### Lynx R1

Stock DT has SMB5/FG4, Type-C/PD, thermal zones and a fan abstraction. Pinned native config enables
the drivers, but board DT leaves PM8150B charger/fuel gauge disabled and exposes only
`simple-battery`; no fan binding/runtime is demonstrated. Proprietary DSP firmware is pinned, but
battery profile, gauge NVM and fan curve are not. Official firmware notes prove stock fan/charge
behavior only. Exact pack topology must be measured before treating reported 8600 mAh/17.2 Wh as
contract facts.

### Samsung Galaxy XR

The required shipping configuration includes the external quick-swap pack, so it is
`sm-i610:base`. Samsung documents pack charging/use while charging; teardown confirms active
cooling ([external] [Samsung charging guide](https://www.samsung.com/us/support/answer/ANS10007500/)).
Pack PD/gauge/authentication, headset conversion, DT/drivers, firmware/profile, standard Linux
ABI, Health/Thermal services and hot-unplug safety are unknown. The Samsung source archive must be
pinned before S1.

### Play For Dream MR

Vendor documents 5060-mAh-class rear battery and 30-W+ USB-C charging; fan and observed runtime are
community evidence. No controller/gauge/DT/kernel, firmware/profile, fan/thermal configuration,
Linux ABI or consumer is public. Generic `anorak`/SM8550 evidence does not join this board.

### Valve Steam Frame

- **L0/L1:** MP DT contains PM8550B charging, MAX17320 pack gauge/strap EEPROM, MP2652 cross-
  charger, hotswap hardware, Type-C PDOs, SLG4AX fan, MAX34417 rail monitors and board/battery/
  charger/fan/radio thermistors
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:2488-2519,2611-2619,5316-5890`).
- **L2:** exact DT/config/modules exist, but custom charger/gauge/fan/hotswap source is missing.
- **L3:** `update_pack_nvm.py` recognizes pack vendors and proves gauge NVM is protected pack-
  specific state. Never run permanent-write/factory paths during installation.
- **L4:** expected stock ABI includes multiple `power_supply` devices, TCPM, fan hwmon, rail power,
  TSENS/PMIC zones/IIO.
- **L5/L6:** stock units include Deckard charger, fan-control, power-monitor and Type-C logger.
  Fan logic is reported MIT; charger/power orchestration is proprietary. No Mura UPower/policy or
  runtime capture exists.
- **L7:** S1, not S2. Acquire corresponding GPL sources and replace/pin proprietary consumers before
  static closure.

The announced hot-swap pack remains a reserved hook until shipped. Base teardown and software
support do not prove a two-pack shipping topology.

### Meta Quest 3 and official profiles

Base stock artifacts identify an internal battery, BQ27Z561 gauge, PMXR PMIC/BCL sensors, USB-PD/
connector thermal path, fan, Health/Thermal HAL and mitigation policy; no public Eureka board
source/native boot exists.

- `eureka:acc:elite-strap-battery`: official USB-C second battery with separate telemetry and
  `external_battery.ko`; native protocol/node unknown.
- `eureka:acc:charging-dock`: official pogo/inductive route with `charging_dock.ko`; native dock
  ABI and thermal behavior unknown.

The intersection—dock charging headset then strap—is a required combined-profile test, not a new
profile row.

## Runtime/release evidence bundle

R1 enumerates exact battery/USB/pack supplies, TCPM contract, zones/cooling/hwmon and mandatory
services. R2 proves SOC/current/charge progression, PD/AICL stability and fan response under XR+
charge. R3 validates units, pack identity/profile, controlled SOC, thermistor mapping, fan scale/
stall and revision binding. R4 exposes correct UPower multi-battery state and a standard power/
thermal consumer without granting session raw writes. R5 covers charger-only boot, suspend,
thermal load, taper/low SOC, fan/sensor faults, PD/dock/accessory removal and recovery without
brownout/runaway/stuck state.

Before sustained XR, preflight must verify mandatory pack identity, gauge, thermal and fan nodes.
Missing fan tach/mandatory sensor selects conservative diagnostics; kernel/PMIC cutoffs remain the
hard safety layer. This audit invents no thresholds.

## Contract/licensing consequences

No current capability option models this domain; use qualification/readiness records only. Standard
`power_supply`, thermal, hwmon, IIO, Type-C and UPower are the native floor. Policy remains a
registry gap. Per-unit gauge/NVM/cycle identity is never redistributed. Frame custom-driver
corresponding source and PFDM/Quest 3 board source are explicit acquisition gaps.

## Contradictions / deciders

- Quest pack label versus DT capacity/profile: production label+batt-id+runtime design values.
- Lynx stock working chain versus disabled native charger/FG: native bind/runtime.
- Galaxy/PFDM `anorak`: no transfer.
- Frame 20.9 versus 21.6 Wh and base versus hotswap support: SKU/label capture.
- Quest 3 18 W requirement versus observed higher draw: PD trace, without replacing vendor floor.
