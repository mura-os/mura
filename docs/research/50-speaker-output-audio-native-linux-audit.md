# 50 — Speaker/output-audio native-Linux audit

**Date:** 2026-09-24. **Method:** [44](44-hardware-enablement-audit-methodology.md).
Playback only; microphone capture is [43](43-microphone-native-linux-capture-audit.md). HRTF,
default-route and per-app policy remain a component-registry gap.

## Verdict

| Device/profile | Native evidence | State |
|---|---|---|
| Quest 1 / `monterey:base` | downstream CM710x/MAX98927 blueprint; no modern native/UCM path | A0 |
| Lynx R1 / `lynx-r1:base` | stock WCD938x/WSA881x; native endpoint empty/amp disabled | A0 |
| Galaxy XR / `sm-i610:base` | donor-dependent PAL sinks; physical/kernel joins incomplete | A0 |
| Play For Dream / `pfdm-mr:base` | physical/stock DTS:X facts only | A0 |
| Steam Frame / `deckard:base` | dual MAX98390→ASoC→UCM→PipeWire; opaque/provenance boundaries | S1 |
| Quest 3 / `eureka:base` | teardown/stock PAL evidence; no native board route | A0 |

Qualifying alternate-output profiles remain independently A0/S1: Lynx TRRS, PFDM USB-C, Frame UAC,
and Quest 3 supported wired/USB-C/BT accessories. Radio closure is doc 48. Historical Quest 1
in-ear headphones stay rejected until first-party archival evidence is pinned.

## Per-target L0–L7

### Quest 1

Meta's exact old source joins open-ear stereo and dual jacks through CM710x, paired MAX98927 amps,
primary MI2S and `qcom,msm8998-asoc-snd-cm710x`
([external] [Meta kernel](https://github.com/facebookincubator/oculus-linux-kernel/tree/589280fc40ddbcc2287024c8b672568a0fdd68e7)).
`CM710X.bin` and Qualcomm ADSP are required; exact die/calibration are unknown. Stock Android mixer/
HAL works, but no current CM710x port, final-build pairing, UCM or native PipeWire sink exists.
Route names such as `speaker-safe` with identical bodies are not independent safety proof.

### Lynx R1

Stock dump identifies Bolero/WCD938x, SoundWire and up to two selected WSA881x amps. Native config
enables SM8250/QDSP6/WCD foundations but disables WSA881x, while the packaged board playback link's
codec block is empty. Pinned ADSP exists; topology/ACDB/protection/UCM does not. No `aplay`,
WirePlumber sink or speaker test. TRRS is vendor-documented but requires its own MBHC/UCM runtime
qualification.

### Samsung Galaxy XR

Samsung documents two woofer+tweeter assemblies. The GXR package harvests QXR ACDB/QWSP/mixer/
resource XML from `/.oldroot` and defines `pal_sink_speaker_db`/`pal_sink_speaker_ll`
([external] [GXR audio package](https://ppa.launchpadcontent.net/lightofmysoul/gxr/ubuntu/pool/main/a/audioreach-config-anorak/)).
This proves a donor-dependent PipeWire→PAL→AGM/AudioReach route, not physical codec/amp topology,
channel/crossover mapping or runtime playback. No archived `wpctl`/`pw-play` bundle means A0.

### Play For Dream MR

Vendor specifies two stereo speakers, DTS:X Ultra and Type-C audio. No amp/codec/bus, board DT,
ADSP/topology/calibration, PAL/ALSA endpoint, UCM, PipeWire sink or Linux port is public. Generic
Anorak QXR and Galaxy donor files are adjacent evidence only.

### Valve Steam Frame

- **L0/L1:** dual drivers per ear; DT has two MAX98390 amps at `0x38`/`0x3d` with left/right
  prefixes (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:2284-2337`).
- **L2:** `qcom,lpass-sndcard` `SPKR Playback`; MAX98390 driver is enabled
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:9435-9468`,
  `references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/config-6.18.0-deckard:5037-5039`).
- **L3:** ADSP/topology, MAX98390 DSM payload, product filter and per-unit EEPROM vendor/
  temperature/resistance closure. `soundsetup.sh` fails muted on missing/unknown calibration.
- **L4/L5:** speaker PCM is `hw:${CardId},0`; UCM programs both channels, boost/current and gain.
- **L6:** WirePlumber publishes stereo S16LE/48-kHz built-in playback and targets product
  Boulder/Denver filters. SteamVR spatial LADSPA is policy/proprietary, not required for basic
  hardware playback.
- **L7:** S1 after review: exact static joins are strong, but per-file provenance for compiled
  filters, custom kernel source, runtime enumeration/acoustics and licensing remain incomplete.

Four physical drivers versus two electrical amp channels is unresolved. Safe gain depends on
matching DSM and per-unit calibration; never copy settings to USB/BT outputs.

### Meta Quest 3

Product/teardown evidence identifies integrated stereo, AK4333 and MAX98388/MAX98361 parts, but
exact topology is unknown. Stock downstream Waipio/Anorak AudioReach/PAL modules work; no Eureka
board DTS/machine route, pinned topology/calibration, UCM/PAL native route or PipeWire sink exists.
The Meta-sold Razer Hammerhead HyperSpeed qualifies as an A0 USB-C accessory profile; descriptor
and native behavior remain unverified.

## Runtime evidence bundle

R1 requires firmware/topology/card/codec/amps/mixers/UCM/PipeWire sink under the active session.
R2 proves deterministic PCM without xrun/DSP fault/silence. R3 establishes FL/FR, polarity,
physical-driver/crossover mapping, mute, actual format/rate and safe amplifier telemetry. R4 proves
ordinary playback, volume/mute/default selection, active-seat ownership and profile switching. R5
covers cold boot, suspend, ADSP restart, hotplug, XR load, thermal behavior, latency/drift and safe
recovery.

Safe gain starts muted, validates exact firmware/protection/per-unit calibration, ramps from
minimum while measuring SPL/current/temperature/protection/brownout, and never applies built-in
tuning to an accessory. Numeric limits require a later policy/qualification decision.

Archive `/proc/asound/{cards,pcm}`, `aplay -l`, `amixer`, `alsaucm`, `wpctl`, `pw-dump`,
deterministic stereo/impulse stimuli, xrun/latency traces, amp telemetry, SPL/temperature and
suspend logs.

## Contract/licensing consequences

Use existing audio adaptation and qualification surfaces; do not invent speaker-channel/profile
options. Standard ALSA/UCM/PipeWire/WirePlumber is the default. Galaxy is a device-specific
donor-backed exception. Preserve local-only ADSP/DSM/ACDB and per-unit EEPROM data. Stock DTS:X,
Atmos, Meta and Valve spatial behavior is engineering evidence, not Mura policy.

## Contradictions / deciders

- Frame `SM8250 LPASS` card string on SM8650: compatibility name, runtime card/DT join.
- Frame four drivers/two amps and EEPROM backing node: schematic or R3 isolation/source.
- Quest CM710x versus CM7104 inference: package marking/firmware metadata.
- Lynx four candidate WSA nodes versus max two selected: runtime SoundWire enumeration.
- Galaxy stereo PW/4-channel TDM/four drivers: donor graph plus R3 crossover test.
- Quest 3 amp/DAC topology: board DT/mixer trace.
