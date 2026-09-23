# 29 — Eye-tracking hardware and IPD mechanisms per target

**Research date:** 2026-09-22  
**Scope:** Oculus Quest 1, Lynx R1, Samsung Galaxy XR, Play For Dream MR, and
Valve Steam Frame; Meta Quest Pro, Meta Quest 3, and Apple Vision Pro are reference designs.

## Executive result

Only three primary targets contain eye-tracking hardware: Galaxy XR, Play For Dream MR, and
Steam Frame.
Quest 1 and production Lynx R1 do not.
Galaxy XR and Play For Dream use eye tracking to close a motorized auto-IPD loop.
Steam Frame uses eye tracking for foveated streaming/rendering but retains a manual IPD dial.

The most important porting result is that none of the targets has publicly documented ordinary
V4L2 access to its eye cameras.
Android XR intentionally gives applications derived eye poses, not inward-camera images; PFDM is
the unusual partial exception, exposing Y8 eye-camera frames through a proprietary
**enterprise-only** SDK rather than documenting a Linux camera node.
Valve similarly documents an OpenXR gaze pose, not raw frames.
This leaves the SLPI/FastRPC/vendor-service question from
[03-android-compat.md §11](03-android-compat.md#11-open-questions) open: donor acquisition must
capture the complete vendor eye-tracking service/DSP closure, not just a sensor kernel driver.

## Evidence rules and terminology

- **Official** means a vendor product, support, privacy, or developer document.
  **Teardown/community** evidence is labeled because component identification and unpublished
  behavior can change by hardware revision.
- **Unknown** means no reliable public evidence was found; it does not mean absent.
  **Unverified** marks a plausible or reported detail that lacks first-party confirmation.
- `manual-sensed` means the user moves the optics and the device reports the mechanism position.
  It does not mean that cameras measure the wearer's anatomical IPD.
- `motorized-auto` means eye tracking estimates pupil separation and software drives the optical
  assemblies.
  The established PCCR/model-fit and servo-loop principle is not re-derived here.
- `vendor-DSP` in the access column means “available only behind a vendor runtime/HAL/service in
  public evidence,” not proof that a particular computation runs on SLPI.
  Exact SLPI, CDSP, FastRPC, ISP, or CPU placement remains **unverified** unless stated.
- Rendering IPD is the separation of the two reported view poses.
  A mechanical number, a user's anatomical IPD, and runtime view separation must not be conflated.

## Qualification matrix

| Device | Eye cameras | IPD source | ET capability class | Eye-camera access class | Iris auth |
|---|---|---|---|---|---|
| Oculus Quest 1 (`monterey`) | None; four cameras are outward Insight sensors | `manual-sensed` | none | N/A | no |
| Lynx R1 | None in production; six non-eye cameras | `manual-sensed` | none | N/A | no evidence / unsupported |
| Samsung Galaxy XR (`SM-I610`) | 4 × 640×640 inward cameras | `motorized-auto` | binocular gaze + foveation + auto-IPD + iris | `vendor-DSP` (exact DSP path unverified) | **yes**, unlock and app-password authentication |
| Play For Dream MR (`anorak`) | present; count/resolution/rate **unknown** | `motorized-auto` | gaze + foveation + auto-IPD | `vendor-DSP`; proprietary enterprise raw-Y8 escape hatch | **unknown**, no public claim found |
| Valve Steam Frame (`deckard`) | 2 inward cameras; resolution/rate **unknown** | `manual` (position sensing unverified) | gaze + foveated streaming/rendering | `unknown`; runtime API only, no public V4L2 contract | no documented iris auth |
| Meta Quest Pro (reference) | 2 eye cameras, plus 3 face cameras | `manual-sensed` | binocular gaze + foveation + expressions | `vendor-DSP` (raw images withheld) | no; Meta says ET is not identification |
| Meta Quest 3 (reference) | none | `manual-sensed` | none | N/A | no |
| Apple Vision Pro (reference) | 4 inward IR cameras | `motorized-auto` | gaze + foveation + auto-IPD + iris | proprietary R1/visionOS stack | **yes**, Optic ID |

## Oculus Quest 1 (`monterey`)

### Eye cameras and illumination

- **Absent.**
  Meta's launch description says the four ultra-wide-angle sensors are for Insight head,
  Guardian, and Touch tracking, not eye tracking
  ([official](https://www.meta.com/blog/introducing-oculus-quest-our-first-6dof-all-in-one-vr-system-launching-spring-2019/)).
- A teardown likewise places four interchangeable camera modules at the visor corners and describes
  their use for 6DoF/controller tracking and passthrough
  ([teardown](https://roadtovr.com/oculus-quest-teardown-disassembly/)).
- There are therefore no inward eye-camera resolution/rate or eye illuminators to recover.
  The Touch controllers' IR emitters are unrelated to eye tracking.

### IPD mechanism

- Quest 1 has mechanically coupled, continuously sliding lens/display assemblies.
  The advertised accommodation is approximately 58–72 mm; an OS update displayed measured values
  of roughly 59–71 mm on one unit
  ([independent report](https://roadtovr.com/quest-digital-ipd-indicator-readout-update/)).
- A physical teardown found a linear slide potentiometer aligned with the lens housing and two guide
  rods carrying the lens assemblies
  ([teardown](https://medium.com/badvr/oculus-quest-headset-disassembly-2f404b004a3c)).
- Classification: **manual-sensed**.
  The user supplies force; the potentiometer reports position so stock software can display a value
  and adjust rendering.
  There is no motor.

### Software, authentication, and calibration

- Eye-camera exposure and ET are N/A.
  Stock fixed foveation is not eye-tracked foveation.
- Quest 1 has no vendor iris-unlock feature.
  Adding iris auth is impossible without additional eye-facing imaging hardware.
- The public partition map includes `persist`, `private`, and `vision`
  ([community firmware research](https://github.com/QuestEscape/research/blob/master/README.md)),
  but no public evidence maps IPD potentiometer calibration to any one partition or path.
  That location is **unknown**.
- Donor handling should still preserve those partitions verbatim.
  Do not infer from the partition name that `vision` necessarily contains the IPD transfer curve.

## Lynx R1

### Eye cameras and illumination

- **Absent in the shipping R1.**
  The six cameras are two 6DoF grayscale cameras, two 400×400 hand-tracking cameras, and two RGB
  passthrough cameras; Lynx's capture API exposes only RGB, tracking, and hand-tracking categories
  ([official](https://portal.lynx-r.com/documentation/view/video-capture)).
- Contemporary production coverage gives the same 2 B&W + 2 IR hand + 2 RGB topology
  ([independent](https://roadtovr.com/lynx-r-1-mixed-reality-headset-hands-on/)).
- An early enterprise concept contemplated a centrally hidden eye camera, but production removed eye
  tracking
  ([historical design report](https://roadtovr.com/lynx-r-1-optics-detailed-pre-order-available/)).
  That prototype claim must not enter the R1 device contract.

### IPD mechanism

- Each lens is moved independently by hand across a vendor-stated 56–76 mm range
  ([official setup guide](https://portal.lynx-r.com/documentation/view/getting-started)).
- Lynx firmware release notes say an update added an IPD-value display and that IPD values configure
  camera distance in VR
  ([official firmware notes](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/)).
- Classification: **manual-sensed**.
  The public material does not disclose sensor type, resolution, transfer function, or whether the
  two lens positions are sensed independently.
  There is no motor.

### Software, authentication, and calibration

- Eye-camera exposure and ET are N/A.
  The hand-tracking IR cameras are not substitute eye cameras and should not be described as such.
- No Lynx documentation advertises iris authentication.
  Treat iris unlock as unsupported.
- Lynx gives unusually concrete factory-calibration paths:
  `/mnt/vendor/persist/qvr/device_calibration.xml`,
  `/mnt/vendor/persist/qvr/svrapi_lens_left.csv`, and the vendor-misspelled
  `/mnt/vendor/persist/qvr/svrapi_lens_rigth.csv`
  ([official firmware page](https://portal.lynx-r.com/downloads/firmware/lynx-r-1/)).
- The vendor warns that camera/IMU/gyro alignment is calibrated after manufacture and that moving
  components invalidates it
  ([official setup guide](https://portal.lynx-r.com/documentation/view/getting-started)).
  The restore guide warns not to select “Erase All Before Download,” because calibration/distortion
  data can be destroyed
  ([official](https://portal.lynx-r.com/documentation/view/updating-your-device)).
- Whether the IPD-sensor endpoints/curve are in `device_calibration.xml` is **unknown** until a
  user-supplied calibration profile is inspected.

## Samsung Galaxy XR (`SM-I610`)

### Eye cameras and illuminators

- Samsung specifies **four inward eye-tracking cameras at 640×640 each**, plus six head/hand cameras
  and two high-resolution passthrough cameras
  ([official support specification](https://www.samsung.com/us/support/answer/ANS10007499/)).
- Samsung says those four cameras detect pupil position and predict gaze direction
  ([official product page](https://www.samsung.com/us/xr/galaxy-xr/galaxy-xr/)).
- Teardown reporting places the four eye cameras on the inner optical frame with the pancake lenses
  and displays
  ([teardown summary](https://www.sammobile.com/news/galaxy-xr-more-repairable-other-mixed-reality-headsets/)).
- Camera frame rate, sensor model, lens FoV, IR wavelength, number/placement of eye illuminators,
  and synchronisation topology are **unknown** publicly.
  Iris recognition implies controlled near-IR capture, but the emitter implementation should not be
  invented from that implication.

### IPD mechanism

- Samsung lists 54–70 mm and explicitly labels adjustment “Automatic”
  ([official](https://www.samsung.com/us/support/answer/ANS10007499/)).
- Hands-on reporting confirms eye-guided, motorized horizontal lens movement rather than a manual
  slider
  ([independent](https://www.uploadvr.com/samsung-galaxy-xr-android-xr-first-impressions-hands-on/)).
- Classification: **motorized-auto**.
  Motor type, lead-screw/belt/gear design, encoder type, physical travel, homing method, precision,
  and a public motor-control API are all **unknown**.
- Android XR publicly exposes eye poses but no application API for commanding the IPD actuator
  ([official extension list](https://developer.android.com/develop/xr/openxr/extensions)).

### Eye-camera software boundary

- Android XR processes eye/face images in real time and deletes them; apps receive derived gaze,
  eye state, and pose only after permission
  ([official Android XR privacy description](https://support.google.com/android-xr/answer/16668380?hl=en-GB)).
- Native/OpenXR apps request dangerous `android.permission.EYE_TRACKING_COARSE` or
  `android.permission.EYE_TRACKING_FINE`; `XR_ANDROID_eye_tracking` returns eye poses/states, while
  interaction should use `XR_EXT_eye_gaze_interaction`
  ([official developer documentation](https://developer.android.com/develop/xr/openxr/extensions/XR_ANDROID_eye_tracking)).
- The standard Android “selfie” camera surface is an avatar reconstruction, not raw inward-camera
  frames; Google also says non-standard headset cameras are not app-addressable
  ([Google statement reported by Android Authority](https://www.androidauthority.com/android-xr-camera-access-3526531/)).
- Access classification: **vendor-DSP/runtime**, not V4L2 as an application contract.
  The exact Galaxy XR service names, Camera HAL IDs, ISP routing, SLPI/CDSP participation, FastRPC
  libraries, and binder interfaces are **unverified**.
  This is precisely the unresolved DSP-access boundary in
  [03-android-compat.md §11](03-android-compat.md#11-open-questions).
- Stock uses include gaze input, eye/face avatars, eye-tracked foveation, alignment/auto-IPD, and
  iris authentication
  ([official platform privacy description](https://support.google.com/android-xr/answer/16668380?hl=en-GB)).

### Iris authentication and calibration

- **Verified iris auth:** Samsung says iris recognition unlocks Galaxy XR and enters passwords in
  certain apps
  ([official launch specification](https://news.samsung.com/global/introducing-galaxy-xr-opening-new-worlds)).
- Samsung Pass generally supports iris-capable Galaxy devices and supported apps
  ([official Samsung Pass support](https://www.samsung.com/us/support/answer/ANS10002529/)),
  but a Galaxy-XR-specific Samsung Pass integration page was not found.
  Therefore “Samsung Pass on Galaxy XR” is **unverified**; only unlock/app-password use is verified.
- Eye aiming has per-user calibration under Settings → Input → Eye calibration
  ([official setup guide](https://www.samsung.com/us/support/answer/ANS10007502/)).
- Google says eye/face measurements used for input and lens alignment are retained on-device until a
  factory reset
  ([official](https://support.google.com/android-xr/answer/16668380?hl=en-GB)).
- Filesystem path, partition, schema, relation to iris templates, and factory-vs-user calibration
  separation are **unknown**.
  Iris template storage should be treated as credential material, not donor calibration.

## Play For Dream MR (`anorak`)

### Eye cameras and illuminators

- PFDM officially lists eye-tracking cameras and an 11-camera/7-sensor array with 22 IR lights
  ([official product page](https://pfdm.ai/products/mr-headset),
  [official overview](https://pfdm.ai/)).
- The public material does **not** break out how many of the eleven cameras face the eyes, nor eye
  resolution, frame rate, FoV, shutter, sensor vendor, or placement.
  All are **unknown**.
- The 22 IR lights serve the system as a whole; assigning all of them to the eyes would be false.
  The split among eye, hand/controller, environment, or depth illumination is **unknown**.

### IPD mechanism

- The vendor states automatic IPD over 51–78 mm and says the headset calibrates to the wearer's eyes
  ([official product/support](https://pfdm.ai/pages/play-for-dream-mr-support-service-center)).
- Independent reporting ties eye tracking to automatic IPD and foveated rendering
  ([UploadVR](https://www.uploadvr.com/play-for-dream-mr-xr2-plus-gen-2-headset/)).
- Classification: **motorized-auto**.
  Motor type, travel, gearing, encoder, homing, precision, and actuator API are **unknown**.
  The top “Digital Dial” instruction is a user action that starts/accepts alignment, not evidence of
  manual lens motion.

### Eye-camera software boundary

- Consumer SDK eye tracking exposes a Unity `XR Device` pose/rotation abstraction
  ([official developer mirror](https://playfordreamdevelopers.github.io/com.yvr.core-mirror/Documentation/MultiModalInteraction/EyeTracking.html)).
- Unusually, PFDM's official `CameraSample-Unity` lets **enterprise devices** select “Eye Tracking,”
  acquire a frame, or subscribe to a stream; all such tracking-camera data is Y8
  ([official GitHub](https://github.com/PlayForDreamDevelopers/CameraSample-Unity)).
- This is evidence that privileged raw frames can cross the vendor boundary.
  It is **not** evidence of V4L2, unrestricted consumer access, sensor resolution/rate, or a stable
  Linux ABI.
- Access classification: **vendor-DSP/proprietary service**, with an enterprise raw-Y8 API.
  Exact binder service, native library, DSP firmware, FastRPC dependency, and camera node topology
  remain **unknown**.
- Vendor/independent sources describe gaze interaction, foveated rendering, and auto-IPD.
  No public source found an iris-recognition or biometric-unlock feature; iris auth is **unknown /
  not advertised**, not proven impossible.

### Calibration

- No public firmware research identifies PFDM eye calibration, IPD motor calibration, camera
  intrinsics/extrinsics, or per-unit optical files.
- Partition/path/schema are **unknown**.
  Preserve every `persist`, `calib`, `factory`, sensor-NV, and vendor-private partition from the
  owned donor before any porting work.
- Consumer per-user gaze calibration, factory camera geometry, and actuator endpoint calibration
  must be modeled as three separate datasets even if the stock OS stores them together.

## Valve Steam Frame (`deckard`)

### Eye cameras and illumination

- Steam Frame has **two inward eye-tracking cameras**; independent hardware inspection places them
  above the eyes
  ([Tom's Hardware](https://www.tomshardware.com/virtual-reality/valve-steam-frame-3-review)).
- Public specifications do not state sensor model, resolution, frame rate, shutter, FoV, wavelength,
  or eye-illuminator count.
  These are **unknown**.
- Valve confirms the cameras drive foveated streaming through interviews
  ([Valve engineer interview](https://www.tomshardware.com/virtual-reality/valve-engineers-discuss-the-duality-of-the-steam-frame-and-pricing-valves-newest-vr-headset-pivots-steamos-to-arm)).
  General IR illuminators are documented for low-light headset/controller tracking; whether the same
  emitters illuminate eyes is **unverified**.

### IPD mechanism

- A top dial manually moves lens separation; one measured production-review range is 57.6–69.6 mm
  ([independent review](https://www.tomshardware.com/virtual-reality/valve-steam-frame-3-review)),
  while public summaries round the target range to 60–70 mm
  ([iFixit device page](https://www.ifixit.com/Device/Steam_Frame)).
- Classification: **manual**.
  Public evidence does not establish a position sensor, encoder, motor, or software readout, so
  `manual-sensed` would currently overclaim.
- The runtime must still publish correct view poses for rendering.
  How it derives the physical lens setting is an unresolved qualification test.

### SteamOS/SteamVR exposure

- Valve's official native-engine guidance exposes eye gaze through
  `XR_EXT_eye_gaze_interaction`
  ([Steamworks](https://partner.steamgames.com/doc/steamhardware/steamframe/engines/custom)).
- Eye-tracked rendering uses the `XR_FB_foveation*` and `XR_META_foveation_eye_tracked` extension
  family; Steamworks recommends runtime-provided VRS
  ([official Unreal guidance](https://partner.steamgames.com/doc/steamhardware/steamframe/engines/unreal)).
- Foveated streaming sends low-resolution full views plus high-resolution gaze regions and requires
  no game changes
  ([technical demonstration](https://tech.yahoo.com/ar-vr/articles/tried-trick-steam-frames-clever-203000704.html)).
  The exact on-wire gaze representation and whether gaze leaves the headset as explicit coordinates
  are **not publicly documented**.
- No official raw eye-camera or V4L2 API was found.
  Classification is **unknown/runtime-private**, even though the host OS is Linux.
  Linux underneath SteamVR does not imply stable `/dev/video*` access.
- Valve's debug workflow can record tracking datasets and optionally camera data under
  `~/.config/openvr/config/cv/xrservice/datasets/`
  ([official debugging guide](https://partner.steamgames.com/doc/steamhardware/steamframe/debugging)).
  That is a privileged diagnostic capture, not an application camera contract.

### Authentication and calibration

- No Valve document advertises iris authentication or eye-based unlock.
  Treat it as unsupported for ADR-0007 unless Valve documents otherwise.
- Public paths/formats for factory eye-camera calibration, per-user gaze calibration, and IPD dial
  calibration were not found; all are **unknown**.
- The `xrservice` configuration/dataset tree is a discovery lead, not proof that persistent
  calibration lives there.
  Per-unit state may sit outside the RAUC rootfs and must be captured separately.

## Reference designs: Quest Pro, Quest 3, and Vision Pro

### Meta Quest Pro

- Quest Pro has one inward IR eye camera per eye plus three separate face cameras
  ([iFixit teardown](https://www.ifixit.com/News/68350/if-the-meta-quest-pro-is-the-future-of-computing-well-never-be-able-to-fix-it)).
- A component/BOM teardown identifies OVM6211-class 400×400 global-shutter cameras at up to 120 fps
  and nine IR LEDs around each lens
  ([community teardown analysis](https://yxqc.porsven.com/135103993078.html)).
  Meta does not publish those details, so they remain **teardown-sourced/unverified by Meta**;
  public gaze output reaches up to 90 Hz
  ([evaluation](https://ar5iv.labs.arxiv.org/html/2403.07210)).
- IPD is manual, continuous, and accommodated over 55–75 mm
  ([specification](https://www.shu.edu/documents/meta-quest-pro-specs.pdf)); stock setup guides the
  user's physical adjustment rather than motorizing it
  ([Meta](https://www.meta.com/help/quest/869053370725118/)).
- Apps request `com.oculus.permission.EYE_TRACKING` and receive per-eye poses/confidence through
  Meta/OpenXR APIs, never raw frames
  ([official API reference](https://beta.developers.meta.com/horizon/documentation/native/android/move-ref-api/)).
- Meta says ET is not identification, raw images are deleted, and calibration remains on-device;
  there is no stock iris unlock and its calibration path is **unknown**
  ([official privacy notice](https://www.meta.com/legal/quest/eye-tracking-privacy-notice/)).

### Meta Quest 3

- Quest 3 has no eye camera/ET; iFixit's teardown describes the omission as a cost saving
  ([teardown report](https://roadtovr.com/meta-quest-3-teardown-ifixit-repair/)).
- A bottom-left wheel manually drives continuous lens spacing and stock UI shows its value
  ([official fit instructions](https://www.meta.com/en-gb/help/quest/10004693912934783/),
  [observed readout](https://www.phonearena.com/news/how-to-adjust-ipd-on-meta-quest-3_id153181)).
- It is **manual-sensed** (roughly 58–70/71 mm travel), but sensor type is **unknown**.
  There is no iris auth.

### Apple Vision Pro

- Apple officially specifies four eye-tracking cameras and a 51–75 mm IPD range
  ([official technical specifications](https://www.apple.com/apple-vision-pro/specs/)).
- Independent silicon analysis identifies 1.8 MP monochrome Sony stacked BSI rolling-shutter eye
  sensors (frame rate **unknown**)
  ([TechInsights](https://www.techinsights.com/blog/sony-18mp-15mm-pixel-eye-tracking-camera-apple-vision-pro-device-essentials)).
- Apple documents multiple near-IR emitters behind the lenses
  ([Optic ID](https://support.apple.com/en-us/118483)); teardown shows a stepper motor and lead screw
  ([iFixit video transcript](https://www.youtube.com/watch?v=JVJPAYwY8Us)).
  Travel, step count, homing, and protocol are **unknown**; class is **motorized-auto**.
- visionOS withholds precise gaze; apps learn confirmed selection, not what the user merely viewed
  ([official developer Q&A](https://developer.apple.com/news/?id=prl6dp5r)).
- **Optic ID** uses the same IR camera/LED system, creates an encrypted iris representation in the
  Secure Enclave, discards normal-operation images, and never backs templates up
  ([Apple Platform Security](https://support.apple.com/guide/security/optic-id-matching-security-sec4518c1d57/1/web/1)).
- It has passcode fallback and supports unlock, purchases, and app sign-in
  ([official user guide](https://support.apple.com/guide/apple-vision-pro/set-a-passcode-and-use-optic-id-tanf79745605/1.0/visionos/1.0)).
- Eye setup stores an encrypted on-device eye-geometry model; paths are not public
  ([Apple privacy overview](https://9to5mac.com/wp-content/uploads/sites/6/2024/02/Apple_Vision_Pro_Privacy_Overview.pdf)).

## Privacy and regulatory treatment of gaze

- OpenXR's cross-vendor `XR_EXT_eye_gaze_interaction` surface is a pose at
  `/input/gaze_ext/pose`, with optional sample time—not a camera-frame interface
  ([Khronos](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrEyeGazeSampleTimeEXT.html)).
  That is the correct default application boundary for Mura.
- Industry practice is on-device image processing, ephemeral raw frames, and permission-gated
  derived poses:
  [Meta](https://www.meta.com/legal/quest/eye-tracking-privacy-notice/),
  [Android XR](https://support.google.com/android-xr/answer/16668380?hl=en-GB), and
  [Apple](https://developer.apple.com/news/?id=prl6dp5r) all implement variants of this boundary.
- “Derived” does not mean harmless.
  Research surveys show gaze can reveal identity, interests, emotional/cognitive state, and physical
  or mental health attributes
  ([privacy survey](https://doi.org/10.1007/978-3-030-42504-3_15)).
- Under GDPR Article 4(14), data becomes biometric data when specific technical processing of
  physical/physiological/behavioral characteristics allows or confirms unique identification;
  Article 9 specially restricts biometric processing **for unique identification** and health data
  ([official GDPR text](https://eur-lex.europa.eu/legal-content/EN/TXT/PDF/?uri=CELEX:02016R0679-20160504)).
  Ordinary gaze interaction is still personal data when linked to a user even when it is not being
  used for identification.
- Illinois BIPA explicitly includes retina or iris scans in “biometric identifier”
  ([official statute](https://ilga.gov/legislation/publicacts/95/095-0994.htm)).
  A gaze vector is not automatically an iris scan, but retaining eye images/templates or using gaze
  behavior to identify a person can trigger materially different obligations.
- Mura should expose only gaze pose/status to ordinary clients, require explicit per-app
  permission, show a persistent use indicator, prohibit silent retention, and keep raw-eye capture
  behind a privileged diagnostic capability with visible consent.
- Per-user ET calibration and iris templates are credentials/profile data.
  Factory intrinsics, actuator endpoints, and lens geometry are per-unit system calibration.
  They need separate storage, backup, access-control, and deletion semantics.
- ADR-0007's current rule—eye cameras off pre-auth—should remain the default.
  A future iris-unlock mode must be a narrowly scoped exception, not a reason to start the full
  session tracking stack in the greeter.

## Donor-pipeline implications

The donor pipeline in [donor-pipeline.md](../architecture/donor-pipeline.md) must add an
`eyeTracking` artifact domain and distinguish redistributable firmware from per-unit/per-user data.

- **Quest 1:** no ET payload.
  Preserve the IPD sensor's kernel/input driver, userspace conversion logic if needed, and per-unit
  `persist`/`private`/`vision` blobs until the potentiometer calibration location is known.
- **Lynx R1:** no ET payload.
  Preserve IPD sensor support and the three documented `/mnt/vendor/persist/qvr` calibration files;
  qualify them as per-unit, `backupOnlySensitive`, and non-transplantable.
- **Galaxy XR:** extract eye-camera sensor/ISP firmware, illuminator and IPD-actuator drivers,
  vendor camera providers, Android XR eye/iris services, their VINTF/init/SELinux metadata, and all
  ADSP/CDSP/SLPI/FastRPC dependencies discovered from the pinned donor.
  Keep user eye measurements and iris templates out of donor artifacts.
- **Play For Dream MR:** capture the same firmware/service closure plus the exact enterprise camera
  SDK/native libraries and entitlement checks.
  Preserve all unknown factory/calibration partitions verbatim; do not redistribute them.
- **Steam Frame:** extract `/lib/firmware`, kernel modules/DT, Valve `xrservice`/SteamVR tracking
  components, udev/service units, and any Qualcomm camera/DSP payloads from the pinned RAUC/rootfs
  donor.
  Search outside RAUC slots for per-unit calibration before assuming the public image is complete.
- **Reference donors:** Quest Pro demonstrates the Android permission/runtime boundary; Vision Pro
  is design evidence only and not a usable donor; Quest 3 contributes only sensed-manual-IPD
  mechanism evidence.

For every target, the qualified manifest should separately name:

1. immutable eye-camera/ISP/DSP firmware;
2. vendor ET runtime/HAL/services and dependency closure;
3. factory camera/illuminator/IPD-actuator calibration;
4. per-user gaze calibration;
5. biometric templates/keys.

Only (1) and legally permitted parts of (2) are candidates for a normal firmware artifact set.
Item (3) is per-unit protected state.
Items (4) and (5) must never enter Nix derivations, binary caches, logs, diagnostics by default, or
another headset.
