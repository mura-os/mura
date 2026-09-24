# 25 — Avatar driving: the Linux sensing path for expression + gaze

**Date:** 2026-09-22.
**Question:** which expression/gaze signals actually exist on Linux today, through which code, in
which schema — and what should the Persona avatar driver normalize to? Placement context:
[adr/0008](../architecture/adr/0008-perception-services-placement.md) (Monado owns sensors/clocks),
[device-contract.md](../architecture/device-contract.md) (typed capability declarations).

Sources: `references/monado/`, `references/wivrn/`, `references/openxr-docs/`,
`references/vrcfacetracking/`, `references/baballonia/`, `references/eyetrackvr/`,
`references/ofera/`, `references/lam-audio2expression/`, plus web-verified vendor/press material.
Every claim is marked **VERIFIED** (primary source read), **REPORTED** (secondary source), or
**UNKNOWN**. Read-only study; nothing was built or run.

---

## 1. The verified Linux pipeline: headset → WiVRn → Monado → OpenXR app

The one complete, working expression path on Linux today is WiVRn's FB-face2 relay. Every hop is
in the corpus:

```
headset (vendor OpenXR runtime)                      Linux server (WiVRn = Monado fork)
┌──────────────────────────────┐   UDP/TCP packets   ┌───────────────────────────────────┐
│ WiVRn client                 │ ──────────────────► │ wivrn_fb_face2_tracker            │
│  xr::fb_face_tracker2        │  tracking::fb_face2 │  (xrt_device, role "face")        │
│  xrCreateFaceTracker2FB      │  70 weights,        │  .get_face_tracking()             │──► oxr_get_face_expression_weights2_fb
│  (SET2_DEFAULT, VISUAL src)  │  2 confidences,     │ wivrn_eye_tracker                 │──► XR_FB_face_tracking2 app
│  xr::eye gaze action space   │  validity, XrTime   │  (xrt_device, role "eyes",        │──► XR_EXT_eye_gaze_interaction app
│  (EXT_eye_gaze profile)      │  EYE_GAZE pose      │   XRT_INPUT_GENERIC_EYE_GAZE_POSE)│
└──────────────────────────────┘                     └───────────────────────────────────┘
```

**Client side (on the headset, VERIFIED):**
- `wivrn/client/xr/fb_face_tracker2.cpp:33-45` creates the tracker with
  `XR_FACE_EXPRESSION_SET2_DEFAULT_FB` and exactly one requested data source,
  `XR_FACE_TRACKING_DATA_SOURCE2_VISUAL_FB`; `get_weights()` (line 47) fills a
  `wivrn::from_headset::tracking::fb_face2` with weights, confidences, `is_valid`,
  `is_eye_following_blendshapes_valid`, and the runtime's sample time.
- `wivrn/client/xr/face_tracker.cpp:25-58` (`face_tracker_supported`) probes, in priority order:
  `XR_ANDROID_face_tracking` → `XR_FB_face_tracking2` (visual) → `XR_HTC_facial_tracking`
  (eye/lip) → an unpublished Pico extension (`pico_face_tracker`, gated on hmd traits). Pico's
  proprietary blendshapes are remapped **into the fb_face2 packet** on-device
  (`client/xr/pico_face_tracker.cpp:75`).
- The stream loop polls the active tracker once per tracking tick and stores the result in the
  tracking packet's `face` variant (`client/scenes/stream_tracking.cpp:482-490`); eye gaze is a
  located action-space pose per tick, with a Pico workaround that re-expresses gaze relative to
  the view when the runtime can't (`stream_tracking.cpp:453-480`).
- Wire schema: `common/wivrn_packets.h:419-438` — `fb_face2 {XrTime time; float weights[70];
  float confidences[2]; bool is_valid; bool is_eye_following_blendshapes_valid;}`, plus `htc_face`
  (14 eye + 37 lip, separate sample times and actives) and `android_face` (68 params, 3 region
  confidences, state, calibration flag) in a `std::variant`. Capability advertisement travels in
  `headset_info_packet` (`face_tracking: face_type`, `eye_gaze: bool`, optional
  `microphone {channels, rate}` — lines 318-327).

**Server side (Monado driver, VERIFIED):**
- `wivrn/server/driver/wivrn_fb_face2_tracker.cpp:39-63` registers an `xrt_device` of type
  `XRT_DEVICE_TYPE_FACE_TRACKER` with one input, `XRT_INPUT_FB_FACE_TRACKING2_VISUAL`, and
  `supported.face_tracking = true`. `update_tracking()` (line 70) rebases the headset sample time
  through the clock-offset estimator (`offset.from_headset(face->time)`); `get_face_tracking()`
  (line 87) serves interpolated weights at the app's requested timestamp from a tracking list and
  stamps `data_source = XRT_FACE_TRACKING_DATA_SOURCE2_VISUAL_FB`.
- `wivrn/server/driver/wivrn_eye_tracker.cpp:38-66` is the same pattern for gaze: an
  `XRT_DEVICE_EYE_GAZE_INTERACTION` device exposing `XRT_INPUT_GENERIC_EYE_GAZE_POSE`.
- HTC and Android equivalents exist (`wivrn_htc_face_tracker.cpp:95-124` serving
  `XRT_INPUT_HTC_EYE_FACE_TRACKING`/`_LIP_`, `wivrn_android_face_tracker.cpp`). The session wires
  exactly one of them to the static `face` role (`server/driver/wivrn_session.cpp:203-207`), and
  the eye tracker to the `eyes` role when the headset advertises gaze (`wivrn_session.cpp:195-198`).

**Monado OpenXR state tracker (VERIFIED):**
- `monado/src/xrt/include/xrt/xrt_device.h:479` defines the driver contract:
  `get_face_tracking(xdev, xrt_input_name facial_expression_type, at_timestamp_ns,
  xrt_facial_expression_set*)`. The union type (`xrt_defines.h:1987-1995`) carries FB2
  (70 weights + 2 confidences + `sample_time_ns` + validity, `xrt_defines.h:1963-1973`), HTC, and
  Android sets; input names `XRT_INPUT_FB_FACE_TRACKING2_{VISUAL,AUDIO}`, `XRT_INPUT_HTC_{EYE,LIP}
  _FACE_TRACKING`, `XRT_INPUT_ANDROID_FACE_TRACKING` at `xrt_defines.h:1230-1236`.
- `monado/src/xrt/state_trackers/oxr/oxr_face_tracking2_fb.c` (author **galister**, the same
  contributor as WiVRn's face path — the "galister implementation" is confirmed and is upstream)
  implements create/get: it resolves the system's `face`-role device, validates requested data
  sources against the device's registered inputs (`oxr_system.c:568-596` scans for the
  VISUAL/AUDIO input names), converts app time to monotonic, calls `xrt_device_get_face_tracking`,
  and copies weights/confidences/`dataSource` back out (`oxr_face_tracking2_fb.c:122-170`).
  HTC and Android state trackers sit alongside (`oxr_face_tracking_htc.c`,
  `oxr_face_tracking_android.c`).
- Build gates (`monado/CMakeLists.txt:406-408,430`): `XRT_FEATURE_OPENXR_FACE_TRACKING2_FB`,
  `_FACE_TRACKING_ANDROID`, `_FACIAL_TRACKING_HTC`, `XRT_FEATURE_OPENXR_INTERACTION_EXT_EYE_GAZE`
  — all default ON; extension list in `oxr/extension_support/oxr_extension_support.py:75-102`.
- Eye gaze is *not* a face-tracking call: `XR_EXT_eye_gaze_interaction` is an interaction profile
  (`/interaction_profiles/ext/eye_gaze_interaction`) served by the `eyes`-role device
  (`xrt_system.h:252`) through the normal pose/action path. Monado also has a native driver
  producing gaze on Linux: PSVR2 (`drivers/psvr2/psvr2_eye.c` parses per-eye gaze points from the
  wired protocol) — proof the `eyes` role works without WiVRn.

**Net finding (VERIFIED):** on Linux today an OpenXR client can obtain (a) 70 FB2 expression
weights with 2 region confidences, validity, data-source tag, and rebased timestamps, and (b) a
combined gaze pose — from a Quest Pro (and Pico 4 Pro/E, HTC Focus/XR-Elite trackers, Galaxy XR
via the Android path) through WiVRn, or PSVR2 gaze natively. There is **no per-eye gaze pose, no
pupil diameter, and no raw eye/face camera** anywhere in this path.

---

## 2. Blendshape schema map

| Schema | Count | Tongue | Per-eye lids/gaze channels | Confidence/validity granularity | Where defined |
|---|---|---|---|---|---|
| ARKit-52 | 52 | 1 (`tongueOut`) | lids yes; gaze as 8 `eyeLook*` shapes | none in the list itself | canonical list mirrored in `lam-audio2expression/models/utils.py:32-84` |
| XR_FB_face_tracking2 | 70 | 7 (tip/dorsal/out/retreat) | lids yes (`EYES_CLOSED_L/R`); gaze as 8 `EYES_LOOK_*` weights + `isEyeFollowingBlendshapesValid` | 2 region confidences (upper/lower face) + `isValid` + `dataSource` | registry `xr.xml:7585-7663`; `xrt_defines.h:1690-1768` |
| XR_ANDROID_face_tracking | 68 | 5 (out/left/right/up/down) | lids yes (indices 12/13 mirror FB2) | 3 region confidences + state enum + `isCalibrated` | registry `xr.xml:8628-8702`; `XRT_FACE_PARAMETER_COUNT_ANDROID 68` |
| XR_HTC_facial_tracking | 14 eye + 37 lip | 8 lip-set tongue shapes | per-eye blink/wide/squeeze + directional eye shapes | per-set `isActive` + sample time only | registry `xr.xml:8412-8476`; `xrt_defines.h:1850-1851` |
| XR_ML_facial_expression | enum of blendshapes | — | — | **per-blendshape** VALID/TRACKED flag bits | registry `xr.xml:8997-8999` (Monado does not implement it) |
| Unified Expressions (VRCFT) | 88 shapes + eye struct | 12 | eye data lives in `UnifiedEyeData`: per-eye gaze Vector2, openness, pupil mm | per-field, set by the source module | `vrcfacetracking/.../UnifiedExpressions.cs` (88 counted), `UnifiedData.cs:9-24` |

Semantics: FB2 indices 0-62 and ANDROID indices 0-62 are name-identical (both FACS-flavoured,
fully left/right split); they differ only in the tongue tail (7 vs 5) — VERIFIED from the two
registry enum blocks. ARKit-52 is coarser (single `cheekPuff`, no `LIPS_TOWARD`, four-corner lip
funneler collapsed to one `mouthFunnel`); FB2→ARKit is a surjection with information loss, the
reverse requires splitting heuristics. UE-88 is a superset of all of the above minus per-shape
combined channels (it splits ARKit's `mouthRoll*`/`mouthShrug*` differently and adds
nose/neck/throat and 12 tongue channels); UE deliberately parks gaze, lids, and pupil **outside**
the shape list in `UnifiedEyeData`.

**Existing conversion code (all VERIFIED as code-in-hand):**
- Pico proprietary → FB2: `wivrn/client/xr/pico_face_tracker.cpp:75`.
- ARKit-45-subset + 6 eye channels → UE and → VRChat-native OSC: Baballonia's
  `src/VRCFaceTracking.Baballonia/BabbleExpressions.cs` (UE map) and
  `src/Baballonia/Services/ParameterSenderService.cs:30-80` (face model emits 45 ARKit-named
  mouth/jaw/cheek shapes — `Utils.FaceRawExpressions = 45` — eye model emits 6: per-eye X/Y/lid).
- SRanipal(HTC-37) → UE: `vrcfacetracking/.../Legacy/Lip/UnifiedSRanMapper.cs`.
- Meta-FB2 → MediaPipe/ARKit-style: OFERA `meta2mp_mapping.py` — a **learned** Ridge/linear
  regression on paired recordings, not a semantic table; then ARKit-51 → FLAME-68 MLP
  (`arkit2flame_model.py:40`) and `mediapipe-blendshapes-to-flame/` adapters.
- FB2 → UE for Quest Pro lives in out-of-corpus VRCFT modules (REPORTED; module registry).

**Recommendation for Mura:** normalize the driver's semantic channel namespace to
**Unified Expressions (88) + a separate typed gaze/lid/pupil block + jaw & head pose**, i.e. UE's
factoring, not its C# types. Reasons: (1) it is the only schema that is a superset of FB2, HTC,
ANDROID *and* ARKit — every verified source maps into it without loss, and published mappings
exist in code for each; (2) it already separates gaze/lids/pupil from shapes, matching the Persona
control interface (semantic blendshapes + gaze + jaw + head pose); (3) FB2's two region
confidences and ANDROID's three map cleanly onto per-channel `confidence`, and the per-channel
`validity`/`source` metadata the avatar design requires has a precedent in XR_ML's per-blendshape
VALID/TRACKED bits. Keep **FB2 as the OpenXR-facing wire schema** (it's what Monado serves and
what apps request); the UE-normalized form is internal to the avatar driver service.

---

## 3. Per-target-device sensing matrix

Legend: ✔=present, ✘=absent, cells marked V/R/U = VERIFIED/REPORTED/UNKNOWN.

| Signal | Quest Pro (WiVRn) | Steam Frame | Play for Dream MR | Lynx R1 | Galaxy XR |
|---|---|---|---|---|---|
| Combined gaze pose | ✔ V — `wivrn_eye_tracker.cpp`; Oculus EYE_TRACKING permission in `hmd_traits.cpp:163` | ✔ R — eye-tracking cameras drive foveated streaming (Valve interview, vr.org spec); exposure via SteamVR OpenXR **U** | ✔ R — Tobii XR5 design-win PR, vendor spec lists eye-tracking cameras | ✘ R — eye tracking removed in 2021 redesign (UploadVR) | ✔ V — `XR_EXT_eye_gaze_interaction` in Android XR extension list; WiVRn traits `EYE_TRACKING_FINE` (`hmd_traits.cpp:308`) |
| Per-eye gaze pose | ✘ V — WiVRn transports one `EYE_GAZE` device; per-eye only as FB2 `EYES_LOOK_*` weights | U — no public API statement | U — Tobii Ocumen "supported" (PR) but Linux exposure unverified | ✘ (no hardware) | ✔ R — `XR_ANDROID_eye_tracking` coarse/fine per-eye poses (Android dev docs); not transported by WiVRn today V |
| Eyelid openness | ✔ V — FB2 `EYES_CLOSED_L/R` + `LID_TIGHTENER` weights (indices 12/13, 28/29) | U | U | ✘ | ✔ V — ANDROID params 12/13 `EYES_CLOSED_L/R` (registry) |
| Face expression weights | ✔ V — 70 FB2 weights end-to-end (§1); visual source only | ✘ R — no face sensors; MIPI/PCIe expansion port earmarked for future face-camera add-ons (vr.org) | U — no vendor claim of face tracking found; likely ✘ | ✘ (no sensors) | ✔ V — 68 ANDROID params; full WiVRn+Monado path exists (`wivrn_android_face_tracker.cpp`, `oxr_face_tracking_android.c`); VRCFT docs confirm working eye+face on device R |
| Raw eye-camera access | ✘ R — Horizon OS exposes no eye images to apps | U (SteamOS is Linux; Valve has not stated) | U | n/a | ✘ R — Android XR permissions expose weights/poses only |
| Any mouth view | inward face cameras exist, images not app-accessible R | ✘ today; Babble-style mouth cam via MIPI expansion is plausible R | U | ✘ — no downward camera claim found U | face-tracking sensors see the lower face (visual FT works) R; raw images ✘ R |
| Mic hardware / relay | ✔ V — AAudio→WiVRn mono relay + virtual PipeWire source | ✔ V — dual raw DMICs in donor DT/UCM; stock processing publishes mono | ✔ V — vendor specifies 4 omnidirectional capsules; no native path | ✔ R — production count 2 under the lenses; mainline DT has no capture DAI | ✔ V — vendor specifies 6 capsules; downstream PAL PipeWire source statically defined, runtime U |
| Native on-device capture | ✘ — this column is the stock-runtime WiVRn reference path | S2 — complete static DT→ADSP→ALSA/UCM→PipeWire closure; R1+ U | A0 — no PFDM-specific board/DSP/userspace artifacts | A0 — generic drivers/firmware exist, board capture topology absent | S1 — PAL/AudioReach source definition exists; real samples U |
| WiVRn/Monado path exists | ✔ V — traits `seacliff` (`hmd_traits.cpp:184`) | ✘ V — SteamOS/SteamVR device, not an Android WiVRn client (upstream README: "Non-Android VR ✖") | ✘ V — upstream README marks Play for Dream unsupported (issue #465) despite a traits entry (`hmd_traits.cpp:222`) | native Monado target; traits entry exists for the vendor-runtime client (`hmd_traits.cpp:217`) | ✔ V — traits `SM-I610` (`hmd_traits.cpp:303`), upstream README ✓ |

Notes: the Quest Pro column applies to Quest 3/3S with audio-driven FB2 weights on-headset
(REPORTED, Meta docs); WiVRn would still transport them as the same fb_face2 packet but its client
requests VISUAL only (VERIFIED, `fb_face_tracker2.cpp:34`) — see §4. Steam Frame's gaze reaching a
*Linux Monado* session (as opposed to SteamVR) has no evidence either way: **UNKNOWN, and it is a
kill-gate question** (§5). For Lynx R1 the honest native cell set is head pose plus procedural
output: microphone hardware is reported, but its published mainline DT has no capture path.

This is an expression/gaze source matrix, not the six-target hardware inventory: Quest Pro remains
because it is the verified FB2 reference device. Target Quest 1 has two microphones
community-reported and a downstream CM710x blueprint; target Quest 3 has four firmware-verified
microphones and stock one-to-four-channel modes. Neither has a qualified native Mura capture
source. Physical counts, stock modes, complete per-device paths, and the A0/S1/S2/R1–R5 labels are
canonical in [43](43-microphone-native-linux-capture-audit.md). WiVRn's forced-mono relay is not
`mura.xr.sensing.micChannels`.

---

## 4. Degraded modes

**Audio→expression.** The FB2 spec itself defines the degraded mode: data source
`XR_FACE_TRACKING_DATA_SOURCE2_AUDIO_FB` — "face tracking uses audio data to estimate expressions…
the runtime **must not** use visual data for this source"; VISUAL "**may** also use audio to
further improve quality" (VERIFIED, `fb_face_tracking2.adoc` enumerant descriptions; registry
`xr.xml:7662-7663`). Monado's state tracker already routes it: if a tracker was created
audio-only, `oxr_get_face_expression_weights2_fb` queries the device with
`XRT_INPUT_FB_FACE_TRACKING2_AUDIO` (`oxr_face_tracking2_fb.c:139-140`) — but **no device in the
corpus registers that input** (WiVRn's tracker registers VISUAL only,
`wivrn_fb_face2_tracker.cpp:59`). The claim that a WiVRn fork wired the audio source could **not**
be verified — web search found only ImSapphire's Pico-4-Pro fork, which maps Pico blendshapes into
FB2 *visual*. Mark: **UNKNOWN/unconfirmed**. Consequence: an audio-driven face device is a small,
well-defined gap — a Monado `xrt_device` exposing `XRT_INPUT_FB_FACE_TRACKING2_AUDIO`, fed by a
PipeWire capture.

**LAM_Audio2Expression contract (VERIFIED from repo):** input 16 kHz mono audio (librosa load at
`sr=16000`, `inference_streaming_audio.py:45`), chunked in 1 s windows with a carried `context`
for streaming (`infer_streaming_audio(audio_chunk, sample_rate, context)`, line 53); encoder is
pretrained wav2vec2 (`configs/wav2vec2_config.json`); output `output['expression']` with
`expression_dim=52` (`configs/lam_audio2exp_config_streaming.py:45`) indexed by the exact ARKit-52
list (`models/utils.py:32-84`), exported at 30 fps. Caveats: the repo's engine code is
CUDA-oriented (`engines/launch.py:128-129` asserts GPU for distributed paths); the README claims
"real-time" but a **CPU real-time figure and the checkpoint size are not stated in the repo —
UNKNOWN** (weights are a separate HF/ModelScope download, `pretrained_models/
lam_audio2exp_streaming.tar`; we did not download it). Both need a spike measurement before being
load-bearing. Note the output is eye+brow+mouth ARKit-52, but only mouth/jaw channels are
meaningfully audio-inferable; eye/brow channels from audio must be treated as synthesized noise.

**Procedural blinks and the flagging rule.** With gaze-only devices (Steam Frame class) eyelid
openness doesn't exist; with audio-only (Lynx R1) nothing does. Community practice synthesizes
blinks (EyeTrackVR even at the source: its output set is per-eye gaze X/Y + `EyeLid` + pupil
dilation over OSC, `EyeTrackApp/osc/VRChatOSCSender.py:260-274`, with dedicated blink logic in
`blink.py`/`intensity_based_openness.py`). The Persona rule follows from FB2's own design: the
spec distinguishes `isValid`, `isEyeFollowingBlendshapesValid` (set XR_FALSE when the user denied
eye tracking even while face is valid — VERIFIED, `fb_face_tracking2.adoc:40-58`) and `dataSource`.
**Every channel the driver emits carries `source ∈ {sensor, derived, audio-inferred, procedural,
default}` and per-channel validity; synthesized channels are never presented as zeros or as sensor
data.** A silent zero is indistinguishable from "mouth deliberately still" — corrupting both
avatars and any downstream calibration.

---

## 5. Adopt / reject / open questions for Mura

**Adopt (backed by verified code):**
1. **Monado owns the expression clock domain**, extending ADR 0008 to face/gaze: the avatar driver
   service consumes `get_face_tracking`/gaze poses *inside or beside Monado*, never by opening
   sensors itself. WiVRn's clock-offset rebase (`offset.from_headset(face->time)`) is the model:
   by the time weights reach a consumer they are already in the server monotonic domain.
2. **FB2 as the OpenXR wire schema, UE-factored superset as the internal one** (§2). The driver
   service should also *re-publish* whatever it synthesizes as a Monado face device (an
   `xrt_device` with the FB2 visual and/or audio input), so ordinary OpenXR clients (Overte,
   Resonite-likes) get degraded-mode Personas for free through the standard extension.
3. **Device contract options** under `mura.xr.sensing.*` (typed, per device-contract
   conventions): `gaze = none|combined|per-eye`, `eyelid = none|weights|openness`,
   `faceWeights = none|fb2-visual|fb2-audio|android|htc`, `mouthCamera = none|internal|addon`,
   `micChannels = int`, each with a `provenance` note. For microphones, [43 §1.2 and §10](43-microphone-native-linux-capture-audit.md)
   are the population source and runtime gate: `micChannels` is the logical channel width of the
   native session's default capture source after routing/processing, never the physical capsule
   count or WiVRn relay width. It remains zero until that path reaches R4.
   Assertions: `faceWeights != none` requires the matching Monado build flag category
   (`XRT_FEATURE_OPENXR_FACE_TRACKING2_FB` etc.); `fb2-audio` requires a mic.
4. **Baballonia/EyeTrackVR as the add-on-hardware path** for devices with expansion (Steam Frame
   MIPI port, USB mouth cams): it is cross-platform .NET with explicit Linux support (tarball
   releases, `Baballonia.LibV4L2Capture` project — VERIFIED in-tree), ONNX models
   (`faceModel.onnx` 45 ARKit shapes, `eyeModel.onnx` 6 channels), and its output already maps to
   UE. Integration shape: a source feeding the avatar driver, *not* OSC into apps.

**Reject:**
- Normalizing to ARKit-52 internally (loses FB2/UE tongue, per-corner lip detail; it's the
  *degraded-mode* schema, not the canonical one).
- OSC (`/avatar/parameters/*`) as the system transport — it is the community's app-level hack;
  Mura has Monado's device layer.
- Building on per-eye gaze or pupil diameter as required channels — no verified Linux path
  delivers them today (Galaxy XR's `XR_ANDROID_eye_tracking` is the only candidate and WiVRn
  doesn't transport it).
- Treating "Steam Frame has eye tracking" as "Mura gets gaze on Steam Frame" — different
  runtime stack, zero evidence of third-party exposure.

**Open questions / kill-gates (per device, before any model work):**
1. **Signals-present gate:** on the physical device, enumerate that the expected OpenXR extension
   actually lists and that a tracker create + first `xrGetFaceExpressionWeights2FB`/gaze locate
   returns `isValid` within N seconds. (Cheap conformance-style check; runs under
   `mura.qualification.acceptanceTests`.)
2. **Timestamp-usability gate:** sample weights at app rate for 60 s; require monotonic
   `sample_time_ns`, jitter within budget, and skew vs. Monado's clock bounded — WiVRn's rebase
   makes this pass by construction, but Android-path and future native drivers must prove it.
3. Quest 3/3S audio-source weights through WiVRn: does the client requesting VISUAL-only refuse on
   hardware whose runtime is audio-only? (Spec says create fails with unsupported sources —
   `fb_face_tracking2.adoc` — so WiVRn likely silently loses face on Quest 3: **UNKNOWN**, test.)
4. Play for Dream: WiVRn upstream says unsupported (#465) while a traits entry exists — track
   upstream; until it flips, PFDM contributes no expression signals.
5. LAM CPU real-time + checkpoint size (§4) — measure before adopting as the default degraded
   mode; fallback is procedural visemes from mic energy, which still must be flagged `procedural`.
6. Whether Monado should grow the ML-style **per-blendshape** valid/tracked bits internally — the
   registry precedent exists (`XR_ML_facial_expression`); upstream conversation needed.

---

## 10-line summary

1. VERIFIED end-to-end Linux path: WiVRn client creates `XR_FB_face_tracking2` (70 weights, visual source) on-headset, ships weights+confidences+validity+XrTime, server rebases clocks and serves them as a Monado `xrt_device` via `get_face_tracking`; galister's `oxr_face_tracking2_fb.c` is upstream Monado and delivers them to any OpenXR app.
2. Eye gaze is a separate verified path: `XR_EXT_eye_gaze_interaction` combined pose only (WiVRn `eyes`-role device; PSVR2 native driver proves the role sans WiVRn); no per-eye pose or pupil on Linux today.
3. Parallel verified schemas: HTC 14 eye+37 lip and ANDROID-68 (Galaxy XR) have complete WiVRn+Monado plumbing too; ANDROID-68 ≡ FB2's first 63 + 5 tongue.
4. Schema decision: normalize internally to the Unified-Expressions factoring (88 shapes + separate gaze/lid/pupil block), keep FB2 as the OpenXR wire schema; conversion code exists for every source (Pico→FB2, SRanipal→UE, Baballonia→UE, OFERA's learned FB2→ARKit→FLAME).
5. Device matrix: Quest Pro & Galaxy XR = full gaze+face verified; Steam Frame = gaze hardware yes but Linux exposure UNKNOWN; Play for Dream = eye tracking reported, WiVRn unsupported; Lynx R1 = head pose plus reported mic hardware, but its mainline DT has no capture path. Microphone qualification for all six targets is doc 43, not capsule count.
6. FB2's AUDIO data source is real in spec and plumbed through Monado's state tracker, but no device registers the AUDIO input; the rumored WiVRn audio fork could not be verified (found only a Pico visual fork).
7. LAM_Audio2Expression: 16 kHz wav2vec2 → ARKit-52 @30 fps with streaming context, VERIFIED contract; CPU real-time and weight size UNVERIFIED — spike before relying on it.
8. Baballonia is the verified add-on path (Linux tarballs, V4L2 capture, ONNX 45-shape face + 6-channel eye models) for mouth cams on expansion-port devices.
9. Rule adopted: every emitted channel carries source ∈ {sensor, derived, audio-inferred, procedural, default} + validity; synthesized channels are never silent zeros — FB2's `isEyeFollowingBlendshapesValid`/`dataSource` and XR_ML's per-shape bits are the precedents.
10. Kill-gates per device before model work: extension-enumerates + first-valid-sample check, and a 60 s timestamp monotonicity/jitter/skew check against Monado's clock.
