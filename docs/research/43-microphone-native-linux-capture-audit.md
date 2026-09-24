# 43 — Microphone hardware and native Linux capture path (six targets)

**Research date:** 2026-09-24.
**Question:** what microphone hardware exists on each Mura target, how does the stock OS reach it,
how much of a native Linux capture path is statically documented, and what still requires runtime
qualification?
**Method:** read-only source, donor-image, firmware-dump, package, and vendor-documentation audit.
No target hardware was available, and no claim below treats static configuration as proof that a
microphone records.

This document is the canonical per-target microphone audit. The shorter facts in
[07-device-landscape](07-device-landscape.md), the avatar consequence in
[25-avatar-driving-sensing](25-avatar-driving-sensing.md), and the Steam Frame donor inventory in
[33-steam-frame-donor](33-steam-frame-donor.md) point here rather than duplicate the six paths.
The generalized L0–L7 chain, canonical device/profile identities, accessory/mod inclusion rule and
shared-artifact ownership are now in
[44-hardware-enablement-audit-methodology.md](44-hardware-enablement-audit-methodology.md). This
audit's six base-device conclusions remain unchanged; any future USB/BT microphone accessory is a
separately qualified profile, never a promotion of the base headset.

## 1. Evidence labels and scope

- **VERIFIED** — an inspected source artifact, donor extract, firmware dump, package, or runtime log
  directly supports the claim. Pinned clones are cited as `references/<clone>/path:line`; other
  artifacts are marked **[external]** and linked.
- **VENDOR-DOCUMENTED** — a first-party specification, support page, or developer document states
  the fact, but no local source artifact proves the implementation.
- **COMMUNITY-REPORTED** — credible third-party teardown, port, log, or user report.
- **INFERRED** — adjacent-platform evidence makes the claim plausible but does not identify the
  shipping target.
- **UNKNOWN** — no defensible public evidence was found.

Consumer XR software is used only as engineering evidence. Its policy is not authority for Mura.
The audit distinguishes three axes that must never be collapsed:

1. **Physical hardware:** capsule count, placement, and external-microphone connectors.
2. **Stock path:** Android/SteamOS framework, HAL/PAL, DSP graph, and the channel modes exposed by
   the vendor OS.
3. **Native Mura/Linux path:** board DT and ASoC/PAL binding, firmware/topology, ALSA PCM or PAL
   endpoint, UCM/routing, PipeWire source, and session access.

### 1.1 What a complete native path means

For an ALSA-based Qualcomm target, the joined chain is:

```
physical DMIC
  → board pinctrl/mic-bias/audio-routing
  → LPASS VA/TX macro or external codec DAI
  → ADSP APR/GPR service + firmware/topology
  → ASoC machine-card FE/BE route
  → ALSA capture PCM + mixer controls
  → UCM device/profile
  → WirePlumber-selected PipeWire Audio/Source
  → application
```

Galaxy XR's known downstream-Linux path substitutes PAL/AGM/AudioReach and a PipeWire PAL plugin
for ALSA/UCM. That remains a native GNU/Linux userspace, but it depends on Samsung/Qualcomm donor
components and is not the default mainline-ALSA path.

The ASoC machine driver is the board-specific glue between codec, CPU DAI, platform/DSP, clocks,
regulators, and routes; a generic SoC driver or a booting ADSP is not a sound card
([external] [ASoC machine-driver documentation](https://docs.kernel.org/sound/soc/machine.html)).
UCM then maps mixer controls and capture PCMs into named use cases
([external] [ALSA UCM](https://alsa-project.org/alsa-doc/alsa-lib/group__ucm.html)). WirePlumber's
ALSA monitor creates PipeWire nodes and prefers UCM when ACP and UCM are enabled
([external] [WirePlumber ALSA configuration](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/alsa.html)).

### 1.2 `micChannels` is not capsule count

`mura.xr.sensing.micChannels` means the logical channel count available to the Mura
audio-inference consumer through the native session's default capture source, after the
board-supported routing and default processing/downmix. It is **not**:

- the number of physical capsules;
- the largest channel mode advertised by stock Android;
- the raw ALSA width when the normal PipeWire source downmixes it;
- or WiVRn's relayed channel count.

Consequently, vendor documentation alone never justifies a non-zero declaration. The contract
default remains `0` until a device passes the runtime gates in §10. Steam Frame, for example, has
two physical/raw DMIC channels but a stock processed mono application source; the future device
declaration must use the interface actually consumed by the audio-inference rung.

## 2. Summary verdict

| Device | Physical microphones | Stock path | Native static path | Native runtime evidence | `micChannels` now |
|---|---|---|---|---|---:|
| Oculus Quest 1 (`monterey`) | 2 **COMMUNITY-REPORTED**, Android config corroborates | complete downstream Android/CM710x path **VERIFIED** | downstream blueprint complete; mainline MSM8998 Q6 + CM710x missing | none | 0 |
| Lynx R1 | built-in vendor-confirmed; production count 2 **COMMUNITY-REPORTED** | WCD938x/Bolero/QDSP6 and Android card registration **VERIFIED** | current mainline DT is playback-only **VERIFIED** | none beyond initramfs boot | 0 |
| Samsung Galaxy XR (`SM-I610`) | 6 **VENDOR-DOCUMENTED** | Android voice path exists; low-level stock DT/codec unknown | downstream-Linux PAL/AudioReach source definition **VERIFIED** | playback reported; microphone recording unknown | 0 |
| Play For Dream MR | 4 omnidirectional **VENDOR-DOCUMENTED** | working DreamOS microphone reported; internals unavailable | adjacent Anorak QXR only **INFERRED** | none | 0 |
| Valve Steam Frame (`deckard`) | 2 **VENDOR-DOCUMENTED**, DT/UCM corroborate | complete SteamOS ALSA/UCM/PipeWire chain **VERIFIED** | complete static chain **VERIFIED** | vendor mic bug/fix proves stock use; no Mura capture log | 0 |
| Meta Quest 3 (`eureka`) | 4 **VERIFIED** from firmware metadata | complete PAL/AudioReach path with 1–4-channel modes **VERIFIED** | generic SM8550 support exists; Eureka board path missing | none | 0 |

“Complete static chain” means every intended identifier and packaged component joins on paper. It
does not mean the device probes, samples advance, physical channels are known, or the active user
can record.

## 3. Oculus Quest 1 (`monterey`)

### 3.1 Physical and stock path

- **COMMUNITY-REPORTED:** two microphones sit on the lower face, one on each side of the nose. Meta
  publicly describes a microphone array but does not publish the count.
- **VERIFIED [external]:** the final Android configuration sets `input_mic_max_count=2`, identifies
  `builtin_mic_1` and `builtin_mic_2`, and maps built-in capture modes in
  [`audio_platform_info.xml`](https://dumps.tadiphone.dev/dumps/oculus/monterey/-/blob/vr_monterey-user-10-QQ3A.200805.001-49845030259200400-release-keys/system/system/vendor/etc/audio_platform_info.xml).
- **INFERRED:** the capsules are digital MEMS. The public artifacts prove digital-microphone
  routing but do not identify a capsule part.

Meta's published Linux 4.4 tree provides an unusually complete downstream blueprint
([external] [kernel commit `589280f`](https://github.com/facebookincubator/oculus-linux-kernel/commit/589280fc40ddbcc2287024c8b672568a0fdd68e7)):

- machine driver `sound/soc/msm/msm8998-cm710x.c`;
- codec/DSP drivers `sound/soc/codecs/cm710x.c` and `cm710x-spi.c`;
- `CONFIG_SND_SOC_MSM8998_CM710X=y`;
- codec firmware `firmware/CM710X.bin`;
- DT sound card `qcom,msm8998-asoc-snd-cm710x`, model
  `msm8998-cm710x-snd_card`;
- CM710x at I²C address `0x2c`, SPI control, 19.2 MHz MCLK, and primary-MI2S capture
  ([external] [`vs1.dtsi`](https://github.com/facebookincubator/oculus-linux-kernel/blob/589280fc40ddbcc2287024c8b672568a0fdd68e7/arch/arm/boot/dts/qcom/vs1.dtsi#L67-L158)).

The codec DAI accepts two-channel 48 kHz capture and several sample formats. The machine driver's
default primary-MI2S TX configuration is 48 kHz S16_LE mono, while Android policy exposes mono and
stereo application profiles. Thus “two capsules,” “two codec channels,” and “default one-channel
capture” are three different facts.

Two firmware domains are required:

1. Qualcomm ADSP (`adsp.mdt` plus segments), which hosts the Q6 services.
2. C-Media `CM710X.bin`, downloaded by the codec driver.

The exact die being CM7104 is **INFERRED**, not verified. Public CM7104 documentation matches the
dual ADC/DMIC, I²C/SPI, firmware-loaded DSP, and I²S design, but the Quest source names only the
CM710x family.

Stock Android uses `audio.primary.msm8998.so`; its mixer route joins `MultiMedia1` to
`PRI_MI2S_TX`, enables `Mic Switch`, and enables `Mic Array Switch` for AEC
([external] [mixer paths](https://dumps.tadiphone.dev/dumps/oculus/monterey/-/blob/vr_monterey-user-10-QQ3A.200805.001-49845030259200400-release-keys/system/system/vendor/etc/mixer_paths_cm710x_pvt_revB.xml)).

### 3.2 Native status

Mainline can boot the MSM8998 ADSP, but MSM8998's Elite/APR audio stack remains incomplete and
CM710x has no upstream ASoC driver
([external] [MSM8998 status](https://linux-msm.github.io/mainline-status/soc/msm8998)).
A native path therefore requires:

1. current APR/Q6ASM/Q6AFE PCM/routing support;
2. a modern ASoC port of the CM710x codec, SPI, and machine drivers;
3. Monterey DT routes, regulators, GPIOs, and clocks;
4. both firmware closures;
5. a UCM profile for the selected frontend PCM, `PRI_MI2S_TX`, Mic and Mic Array controls;
6. PipeWire/WirePlumber exposure and runtime qualification.

No native `arecord`, UCM, PipeWire, channel-order, or suspend/resume evidence was found.

## 4. Lynx R1

### 4.1 Physical and stock path

- **VENDOR-DOCUMENTED:** built-in microphones exist, can be disabled in system settings, and the
  3.5 mm TRRS jack accepts a wired microphone
  ([external] [Lynx firmware notes](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/),
  [jack documentation](https://portal.lynx-r.com/documentation/view/getting-started)).
- **COMMUNITY-REPORTED:** the production headset has two microphones beneath the lenses
  ([external] [VR Expert](https://knowledge.vr-expert.com/kb/does-the-lynx-r1-have-integrated-microphones/)).
  An early prototype report said four; that number must not populate a production contract.

The public downstream dump identifies the stock kernel path:

- `qcom,kona-asoc-snd`, model `kona-mtp-snd-card`;
- Bolero LPASS TX/RX/VA/WSA macros;
- WCD938x over SoundWire and WSA881x speaker amplifiers;
- ADSP APR/GPR audio services and `avs/audio` protection domain;
- multiple analog and digital microphone routes
  ([external] [dumped DT](https://github.com/ellyq/lynx-mainline/blob/main/dumps/lynx-dumped-dt.dts#L28221-L28252)).

Those generic routes do not prove how many channels the PCB populates. The module inventory and
boot log show WCD938x, Bolero, Q6, APR, and ADSP components loading and the Android card eventually
registering as `card0`
([external] [modules and logs](https://github.com/ellyq/lynx-mainline/tree/main/dumps)).
The precise WCD9380-versus-WCD9385 variant, stock raw formats, channel map, beamforming, and
`tinycap` results remain **UNKNOWN**.

Official firmware is downloadable, so `audio_policy_configuration*.xml`, `mixer_paths*.xml`, PAL/
HAL libraries, ACDB, and firmware can be extracted without owning the headset. They were not in
the public community dump inspected for this audit.

### 4.2 Native status

The postmarketOS port reaches a Linux 6.13 initramfs. The pinned package identifies the exact
device kernel source/version (`references/pmaports/device/testing/linux-lynx-r1/APKBUILD:1-40`);
its config enables SM8250/QDSP6 ASoC
(`references/pmaports/device/testing/linux-lynx-r1/config-lynx-r1.aarch64:5025-5055`), and the device
package pulls Lynx-specific ADSP/CDSP/SLPI firmware
(`references/pmaports/device/testing/device-lynx-r1/APKBUILD:10-21`). The firmware package pins
the donor commit and installs `adsp*.mbn`/`adsp*.jsn`
(`references/pmaports/device/testing/firmware-lynx-r1/APKBUILD:1-46`).

Those foundations are useful, but the board DT compiled in the published
[external] [`linux-lynx-r1-6.13.0-r2.apk`](http://mirror.postmarketos.org/postmarketos/main/aarch64/linux-lynx-r1-6.13.0-r2.apk)
is **playback-only**:

- ADSP remoteproc and APR/Q6 core, AFE, ASM, ADM, and routing exist;
- the sound card is `qcom,sm8250-sndcard`, model `Lynx R1`;
- it has a `MultiMedia1` frontend and primary-TDM playback backend;
- it has no capture DAI link, microphone `audio-routing`, or populated capture-codec block.

This is a static implementation gap, not merely a missing hardware test. Native enablement needs
the capture DAI and physical routes first, then UCM and WirePlumber policy. No Lynx-specific UCM,
`arecord -l`, or PipeWire source was found.

## 5. Samsung Galaxy XR (`SM-I610`)

### 5.1 Physical and stock path

Samsung specifies a six-microphone array, with use-case-dependent beamforming and software noise
rejection
([external] [Samsung specification](https://news.samsung.com/global/introducing-galaxy-xr-opening-new-worlds)).
The exact codec, stock machine-driver compatible, DT audio routes, and firmware names remain
**UNKNOWN** from public stock artifacts. Samsung's source portal lists SM-I610 releases, but no
matching archive is pinned in this workspace.

### 5.2 Downstream GNU/Linux path

The public Galaxy XR Kubuntu/KDE/Monado work uses Samsung's downstream kernel and mounts the
Android filesystem at `/.oldroot`; it is not a mainline Galaxy XR port. Its
`audioreach-config-anorak` package is specific to the Galaxy XR bring-up and harvests donor files
rather than redistributing them:

- `acdbdata/anorak_qxr/QXR_acdb_cal.acdb`;
- `QXR_workspaceFileXml.qwsp`;
- `card-defs.xml`;
- `usecaseKvManager.xml`;
- `mixer_paths.xml`;
- `resourcemanager.xml`.

Source package:
[external] [`audioreach-config-anorak`](https://ppa.launchpadcontent.net/lightofmysoul/gxr/ubuntu/pool/main/a/audioreach-config-anorak/).
The shared codename `anorak` must not be used to transfer these facts to Play For Dream.

The package graph includes AudioReach graph services, PAL, AGM, TinyALSA, audio utilities, and a
PipeWire PAL plugin. Static backend definitions include:

- primary TDM capture: 4 channels, 48 kHz, 24-bit;
- codec DMA TX3: mono, 48 kHz, 16-bit;
- voice-assistant TX0: mono, 48 kHz, 16-bit.

PipeWire declares `pal_source_speaker_mic`, description `Built-in Microphone`, class
`Audio/Source`, with stereo FL/FR positions. No Galaxy-specific UCM is used because this route is
PipeWire → PAL → AGM/AudioReach.

This is strong **VERIFIED static** evidence, but not runtime microphone proof. Public reports say
sound works on the Linux port; no `wpctl status`, `pw-dump`, `pw-record`, `/proc/asound/pcm`, or
recorded native microphone sample was found. Six physical capsules, a stereo PipeWire node, and
mono backend definitions likely meet in donor beamforming/calibration, but the effective output
layout is **UNKNOWN**.

No public mainline Galaxy XR board DTS or mainline ALSA card exists. Generic SM8550-like support
must not be represented as proven XR2+ Gen 2 board compatibility.

## 6. Play For Dream MR (`anorak`)

### 6.1 Physical and stock path

Play For Dream specifies four omnidirectional microphones, two speakers, DTS:X Ultra, and USB-C
audio
([external] [official product page](https://pfdm.ai/products/mr-headset)). Capsule model,
placement, PDM topology, calibration, and raw application channel access are **UNKNOWN**.

DreamOS is Android 14-derived. Qualcomm/Lineage's adjacent `anorak_pro_qxr` configuration describes
a PAL/AGM/AudioReach design with four microphone metadata entries and one-, two-, three-, and
four-channel processed modes. This is **INFERRED platform-family evidence only**; it is not a PFDM
vendor partition, codec identification, or calibration set.

Community recordings prove that the stock microphone operates, but disagree on quality. One
long-term report describes high ambient/breath sensitivity, voice distortion, speaker feedback,
weak echo cancellation, and working selectable USB-C microphones
([external] [review](https://note.com/fleabaneh/n/n07220438ab1f)); another found acceptable
performance. These reports cannot separate the physical array from DreamOS/streaming processing.

### 6.2 Native status

No public PFDM board DTS, vendor kernel source, full firmware, ADSP image, topology/calibration,
ALSA inventory, UCM profile, PipeWire source, or native Linux port was found. The official
downloads page exposes manuals rather than firmware. Generic SM8550 LPASS VA/DMIC support is an
adjacent possibility, not a device path, because PFDM's exact silicon mapping remains unverified.

An owned-device dump is the present prerequisite. ADB is community-reported after enabling
developer options, but partition-level extraction still needs root/recovery and must preserve
per-unit calibration.

## 7. Valve Steam Frame (`deckard`)

### 7.1 Hardware, kernel, and firmware

Valve specifies a dual-microphone array
([external] [Steam Frame product page](https://store.steampowered.com/sale/steamframe)). The
reconstructed donor's production DTB and rootfs provide the most complete static path in the set.

The DT has:

- `qcom,sm8650-lpass-va-macro` with 2.4 MHz DMIC sampling clock
  (`references/archive-steam-frame/frame-archive-deckard-20260921.6090922-0.5.0/extracted/sm8650-mp.dts:3734-3747`);
- sound card `qcom,lpass-sndcard`, model **`SM8250 LPASS`** — a compatibility name, not the SoC
  identity;
- a `VA Capture` DAI link to the VA macro
  (`…/sm8650-mp.dts:9434-9468`).

The extracted kernel config enables QDSP6, Q6V5 ADSP remoteproc, LPASS VA/RX/TX/WSA macros, and
Qualcomm SoundWire. WCD937x/938x/939x codec drivers are disabled: built-in microphone capture
does not require an external WCD codec. MAX98390 is the playback amplifier.

The donor contains:

```
/usr/lib/firmware/qcom/sm8650/adsp.mbn
/usr/lib/firmware/qcom/sm8650/adsp_dtb.mbn
/usr/lib/firmware/qcom/sm8650/SM8650-MTP-tplg.bin
/usr/lib/firmware/qcom/sm8650/SM8650-QRD-tplg.bin
```

### 7.2 ALSA, UCM, and PipeWire

The installed UCM profile matches `SM8250 LPASS`:

- speaker PCM `hw:${CardId},0`;
- microphone PCM `hw:${CardId},1`;
- DMIC0 and DMIC1 enabled;
- DMIC2 and DMIC3 explicitly zeroed.

`deckard-audio-setup.service` runs `soundsetup.sh`, which selects the speaker tuning, clears stale
WirePlumber microphone route state, installs the PipeWire filter-chain links, and currently chooses
the CPU AEC path. Valve's public
[external] [`deckard-audio-config` package](https://holo-packages.steamos.cloud/archlinux-deckard-hotfixes/deckard-audio-config-20260914.1-1-any.pkg.tar.zst)
contains the same configuration.

WirePlumber publishes a 48 kHz stereo S16LE built-in capture node. The product PipeWire chain then
uses:

1. a two-channel product-specific LV2 EQ;
2. a mixer that publishes mono;
3. WebRTC AEC3 with mono echo reference and gain controller 2;
4. proprietary `audiofilter.so` noise suppression.

The reusable open path is kernel/DT + ADSP firmware + ALSA/UCM + PipeWire/WebRTC AEC. Valve's
product EQ/noise binaries are donor/reference components with a separate licensing and
redistribution question.

SteamOS 0.3.0 fixed a vendor-described “garbled audio” microphone bug
([external] [patch notes](https://store.steampowered.com/news/app/4165890/view/711161056325533925)).
That proves the stock microphone path was exercised, not that Mura's NixOS image records.
No headset-generated `arecord -l`, `/proc/asound/pcm`, `wpctl status`, successful WAV, channel
identity, or suspend/resume evidence was present in the donor.

## 8. Meta Quest 3 (`eureka`)

### 8.1 Physical and stock path

Quest 3 has four built-in omnidirectional microphones **VERIFIED [external]** from
[`microphone_characteristics.xml`](https://dumps.tadiphone.dev/dumps/oculus/eureka/-/blob/eureka-user-14-UP1A.231005.007.A1-50974260049300520-release-keys/vendor/etc/microphone_characteristics.xml#L35-L91).
The file records sensitivity, orientation, and geometry. Stock modes use different subsets:

- one-microphone handset paths;
- two-microphone speaker/AEC paths;
- three-microphone normal/voice-assistant paths;
- four-microphone raw/HDR/ultrasound paths.

The digital routes use LPASS TX/VA and SoundWire DMIC inputs. The stock Android 14 kernel loads a
downstream `waipio.c` machine module (`qcom,waipio-asoc-snd`), LPASS macro, SoundWire-DMIC, GPR,
SPF/Q6, ADSP-loader, AK4333 output, and MAX98388 amplifier modules. AK4333 is output-only; it is not
the microphone ADC.

ADSP uses downstream `qcom,anorak-adsp-pas`; `adspua.jsn` defines the `audio_pd`/`avs/audio`
service. Android uses `audio.primary.anorak.so`, PAL/AudioReach, AGM, and donor calibration.
Application policy exposes built-in capture at 48 kHz plus mode-specific processed/raw profiles
([external] [Eureka firmware dump](https://dumps.tadiphone.dev/dumps/oculus/eureka/)).

### 8.2 Native status

Mainline SM8550 has ADSP, AudioReach, LPASS digital-codec, DMIC, and SoundWire foundations, but
Eureka's Anorak board enablement is absent. Missing pieces include:

- Anorak clocks, pinctrl, interconnects, PAS compatibility, and board DTS;
- donor ADSP firmware and an interoperable AudioReach topology;
- LPASS TX/VA and board SoundWire-DMIC routing;
- an upstream equivalent or port of downstream `qcom,swr-dmic`;
- Quest-specific sound-card links and UCM;
- runtime access despite the locked boot chain.

There is no native Eureka boot, ALSA card, UCM profile, PipeWire source, or recording evidence.
Processed beamforming/AEC likely needs donor graph and calibration assets; whether clean raw capture
can avoid them is **UNKNOWN**.

## 9. WiVRn relay is a separate path

WiVRn supports Quest 1, Quest 3, and Galaxy XR and explicitly does not support Play For Dream in
the pinned README (`references/wivrn/README.md:18-31`). Its Android client requests
`android.permission.RECORD_AUDIO` for every Android HMD
(`references/wivrn/client/hmd_traits.cpp:149-160`), opens an AAudio low-latency input using either
`VOICE_COMMUNICATION` or `UNPROCESSED`, and sends PCM to the server
(`references/wivrn/client/audio/android/audio.cpp:149-169`).

Capability probing deliberately advertises **one** channel even where Android reports two because
some headsets then fail:

```
references/wivrn/client/audio/android/audio.cpp:327-341
```

The server creates a PipeWire `Audio/Source`, mono for that advertised stream
(`references/wivrn/server/audio/audio_pipewire.cpp:178-253`). This is a complete and useful
stock-Android-to-Linux relay. It is not native on-device capture and does not populate
`micChannels`.

## 10. Qualification ladder

The following evidence states are Mura's audit vocabulary. Numeric durations/counts remain
implementation-time policy and must be recorded with the qualification result rather than implied
here.

- **A0 — inventory:** hardware or stock artifacts exist, but the native joins are incomplete.
- **S1 — board-source coverage:** exact revision, DT, driver family, firmware/topology, and
  userspace route are identified and pinned.
- **S2 — static closure:** schemas/build pass and every identifier joins through the complete
  native chain with hashes/licenses recorded. This is the maximum claim available without runtime
  evidence.
- **R1 — runtime enumeration:** ADSP/topology load; components bind; capture PCM/PAL endpoint,
  UCM route where applicable, and PipeWire source exist.
- **R2 — raw transport:** activated capture advances without stalls/DSP faults and yields
  time-varying, non-silent samples under a known stimulus.
- **R3 — physical transduction:** controlled stimulus/occlusion establishes physical channel
  identity, silence floor, clipping, rate, channel map, and internal-versus-external selection.
- **R4 — session integration:** `pw-record` and an ordinary application work; mute/default/profile
  behavior is correct; inactive sessions release the card and the active user can claim it.
- **R5 — robustness/quality:** cold boot, suspend/resume, profile/session switching, XR-load
  capture, latency/SNR, and any claimed AEC/beamforming behavior pass explicitly recorded bounds.

For `fb2-audio`, R4 is the minimum source-availability gate. Its own model/timestamp qualification
remains the Persona S-1 gate in [25 §5](25-avatar-driving-sensing.md): microphone existence does
not prove audio-inferred facial-expression quality.

### Runtime evidence bundle

For an ALSA/UCM path, archive at least:

```
dmesg remoteproc/ASoC/firmware messages
/proc/asound/cards
/proc/asound/pcm
arecord -l
amixer contents
alsaucm listcards and active HiFi/Mic state
wpctl status and wpctl inspect <source>
pw-record from the selected source
```

Also record ADSP restart behavior, suspend/resume, headset/USB/Bluetooth microphone switching,
channel-isolation stimuli, gain/clipping, latency/xruns, and whether processing changes the channel
width.

## 11. Architecture consequences

**Adopt:**

1. Keep physical mic facts, stock paths, and native logical channels separate in every device
   qualification record.
2. Use standard ALSA/UCM/PipeWire/WirePlumber for native paths. A PAL donor path is a
   device-specific or Android-backed adaptation, not a reason to replace standard Linux audio.
3. Keep `micChannels = 0` until R4; record raw width separately from the default processed source.
4. Treat firmware/topology/UCM as one versioned board closure. A donor bump can invalidate mixer
   names, PCM numbers, DSP ABI, or calibration.
5. Preserve per-unit calibration and donor licensing boundaries.

**Reject:**

- assigning `micChannels` from the marketing capsule count;
- treating Android recording or WiVRn relay as native support;
- claiming a working mic from a codec driver, generic SoC DT, UCM file, or PipeWire node alone;
- transferring facts between Galaxy XR and PFDM merely because both use `anorak`;
- importing proprietary EQ/noise processing as a mandatory Mura component without a separate
  licensing and budget decision.

**Open questions with deciders:**

1. **Steam Frame logical width:** processed mono versus a future two-channel Mura source. Decider:
   the R4 application-facing capture result and the audio-inference consumer contract.
2. **Quest 1 CM710x firmware freedom:** whether clean raw capture is possible without the binary
   DSP image. Decider: codec-driver bring-up and firmware-disabled measurement.
3. **Lynx physical wiring:** which of the generic WCD/VA routes reaches the shipping capsules.
   Decider: board DT/donor extraction plus R3 channel isolation.
4. **Galaxy PAL dependency:** whether mainline ALSA can replace donor PAL/ACDB for raw capture.
   Decider: SM-I610 source/DT audit and a native board spike.
5. **PFDM topology:** exact silicon, codec, DMIC routing, and firmware acquisition. Decider:
   owned-device read-only donor capture.
6. **Quest 3 raw path:** whether upstream LPASS/SoundWire can expose usable raw channels without
   stock PAL/ACDB. Decider: Anorak mainlining plus R2/R3 tests.
