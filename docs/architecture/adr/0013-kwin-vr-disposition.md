# ADR 0013: Disposition of the KWin VR approach — design donor + reserved session, not the backbone

**Status:** accepted (draft)
**Date:** 2026-09-23
**Context sources:** [31-kwin-vr](../../research/31-kwin-vr.md) (code-level study of the fork,
its core-patch series, the out-of-tree Qt/XWayland surface, and the Monado `galaxyxr` fork),
pinned clones `references/kwin-vr`, `references/kwin-vr-patches`, `references/monado-galaxyxr`,
`references/xrinfo`. Re-examines [ADR 0006](0006-compositor-strategy.md) (which weighed
StardustXR/WayVR/greenfield but never "adapt an existing DE"); composes with
[ADR 0007](0007-session-greeter-lock.md), [ADR 0008](0008-perception-services-placement.md),
[ADR 0011](0011-eye-tracking-ipd.md), [ADR 0012](0012-de-modularity-spinout-seams.md).

## Context

[KWin MR !8671](https://invent.kde.org/plasma/kwin/-/merge_requests/8671) ("KWin VR",
lightofmysoul) turns KWin/Plasma into a daily-driven 3D VR desktop: a ~16.8k-line in-process
plugin rendering every window, decoration, shadow, and subsurface as 3D objects in one
Qt Quick 3D XR scene, headgaze/keyboard-first input via a synthetic absolute pointer, physical +
virtual screens, follow mode, lock integration — running on Monado/WiVRn across AR glasses,
PC VR, and (via a Monado fork) Samsung Galaxy XR on Kubuntu 26.04. It is the one working
"adapt a mature DE to VR" existence proof, and ADR 0006 chose its opposite (build the zxr
compositor from the wxrc lineage) without evaluating it.

The study (doc 31) establishes the facts this ADR decides on:

- **The core-patch surface is small and enumerated**: 20 commits, ~530 changed lines. Three of
  the five "serious" seams are ~30-LOC settable-callback extractions (hover resolution, pointer
  position limits, popup placement bounds); only 2D↔VR move/resize (97 LOC through KWin's central
  interactive-move state machine) and window↔output-binding prohibition (four window classes,
  four documented residual bugs) are invasive — and both exist *only because a 2D compositor
  binds windows to outputs*.
- **The topology independently validates ADR 0006's shape**: one compositor process → one OpenXR
  stereo projection layer; ray→plane→`wl_pointer` input; zero-copy dmabuf import of client
  buffers; lock as composition policy. Proven at daily-driver quality down to an Intel UHD 600.
- **The substrate forecloses our 3D tier**: Qt Quick 3D XR owns the OpenXR session, swapchains,
  and frame loop inside `kwin_wayland`; `XrView` runs `depthSubmissionEnabled: false`; there is
  no ingestion path for client-rendered colour+depth, hence no sort-last composition — the
  entire reason zxr-shell-v2 exists ([composition doc](../zxr-shell-v2-composition.md) §2).
- **The patch-carry cost is five upstreams**: the KWin fork (rebased per release, parallel 6.5/6.6
  branches), a per-Qt-point-release qtquick3d+qtbase series (passthrough/overlay/sRGB approved
  for Qt 6.11; RGBA16 approved-unscheduled; async-render and mono pending), two unmerged XWayland
  MRs, the Monado fork, optionally Mesa radeonsi multiview (closed draft) and two Plasma patches.
- **KDE is unlikely to merge it**: both maintainers' positions (size/maintenance, `if (isVr)`
  bitrot, output-model invasiveness) are quoted in doc 31 §6; the author's own reading is that it
  "naturally grows into a fork". Notably, Vlad Zahorodnii's preferred integration — *"we could
  provide info about windows, thumbnails, perhaps a more convenient way to deal with input, and
  let them compose"* — is a KWin-maintainer restatement of our ADR 0012 authority-plane/seam
  architecture, argued independently from the opposite direction.
- **The Monado `galaxyxr` fork is valuable independently of KWin**: dual-DRM-lease direct-mode
  display backend, blob-free SSC/QMI sensor access with per-unit `efs` factory calibration and
  live motorized-IPD readout, titan-server passthrough fused into the distortion pass at 90 fps,
  Linux-side eye tracking, and gaze-driven `VK_KHR_fragment_shading_rate` foveation — all
  Monado-side, all on our side of the ADR 0008 plane boundary (doc 31 §5).

## Decision

Four parts; the first reaffirms ADR 0006, the rest extract the value.

### 1. Not the backbone — ADR 0006 stands

Mura does **not** build its compositor from KWin VR. Decisive reasons, in order:

1. **3D-tier foreclosure.** The Qt-owned OpenXR session and depthless submission make
   client-rendered colour+depth composition (zxr-shell-v2's core capability) structurally
   unreachable without forking Qt Quick 3D XR's render internals — a sixth upstream, deeper than
   all the others.
2. **Perception-plane mismatch.** ADR 0008's Monado-side environment/hand-cutout dmabuf layers
   have no ingestion point in a Qt-composed scene; passthrough arrives only as a runtime-level
   Qt feature. The privacy boundary survives, but the composition policy (per-client cutout,
   boundary breach response as composition policy) does not fit.
3. **Session-model inversion.** KWin VR is desktop-first (boot a 2D Plasma session, toggle VR,
   with physical outputs as first-class residents); the appliance profile (ADR 0007) is
   boot-into-XR with no 2D session ever existing. Retrofitting boot-locked greeter-less XR onto
   Plasma's session machinery is new work the fork does not contain.
4. **Five-upstream patch carry** at wxrc-2019 breadth against a 250k-line C++/Qt codebase whose
   maintainers have signalled non-merge — the exact failure mode ADR 0006 §Context diagnosed in
   the wxrc lineage, now with bigger numbers.

### 2. Adopted as a design donor for the zxr 2D tier (patterns, not code)

The following are adopted into zxr's design, credited to doc 31, because they are the empirical
record of what a working 2D-in-VR tier needed:

- **The five core seams, mirrored as first-class concepts** (not retrofits): hover/focus
  resolution as a pluggable policy; an unbounded pointer/cursor space (no output-clamped input);
  popup placement against *placement volumes* rather than output rectangles; 2D↔3D window
  transitions as a designed state (KWin's 97-LOC fork of move/resize is the cost of *not*
  designing it); and **no window↔output binding at all** — the two genuinely invasive KWin
  patches are problems our window model deletes by construction. These map onto the existing
  registry rows (window model, input routing, space model) and the [dependency
  graph](../desktop-environment.md) §6.2; they sharpen those rows' requirements rather than
  adding nodes.
- **XR-init preflight in a separate process** with a same-GPU check (`kwinvr-xrtest` pattern,
  including its lesson: the preflight crash once cascaded into KWin; the GPU-match gate came
  later). zxr's OpenXR bring-up gets the same shape: probe process, GPU identity check against
  the compositor's render device, refusal instead of in-process discovery-by-crash. Composes
  with ADR 0007's supervision story.
- **Dmabuf-feedback format filtering**: advertise to clients only formats the XR render path can
  actually import (the fork's `eglbackend` drm-format-filter commit) — directly applicable to
  our `zwp_linux_dmabuf_v1` feedback.
- **Decoration/shadow-as-geometry** construction and the **follow-mode / headgaze / grab-all /
  recenter interaction vocabulary** as design references for 3D decorations and the spatial
  window-manipulation UX (registry §5 rows).
- **The KCM settings taxonomy** (general/input/headgaze/follow/advanced pages) as a completeness
  checklist for the zxr HMD settings API (ADR 0012 §4 item 5).

### 3. `kwin-vr` reserved as an optional packaged session — design only, no packaging now

`mura.xr.shell` reserves the value `kwin-vr` beside `stardust` and `wayvr` (ADR 0006
consequences). If and when packaged, it follows the monado-rev model: pinned fork revision +
patch series (KWin fork, Qt series, XWayland MRs, Monado fork) built from the pinned
`references/` recipe. Its value: a KDE-maturity 2D-in-VR fallback and a working comparison
target for zxr's M1/M4 acceptance tests (composition doc §7.5). **No packaging work is
scheduled by this ADR** — scheduling lives in
[implementation-path.md §5](../implementation-path.md); the contract enum change lands with the
packaging work itself, whenever that is scheduled there.

### 4. The Monado `galaxyxr` fork enters the Galaxy XR adaptation evidence base

Independent of everything above: the fork is the primary public evidence for Galaxy XR bring-up
(doc 07 gains it), and three of its pieces are candidate donors for our per-device adaptation
work, gated on the device's unlock reality (first-firmware only; Dec-2025 update removes unlock,
Apr-2026 adds a rollback barrier):

- the dual-DRM-lease direct-mode `comp_target` (display bring-up pattern for dual-SDE panels),
- the SSC/QMI sensor path with per-unit `efs` calibration decoding — which lands exactly in
  ADR 0007's "per-unit calibration is system state" slot, and whose live motorized-IPD readout
  plugs into ADR 0011's `mura.hardware.ipd` model,
- Linux-side eye tracking + gaze-driven FSR foveation as an ADR 0011-adjacent pattern (foveation
  itself is not yet decided anywhere — flagged as a new open item for the eye-tracking backlog).

### Upstream watch-list (adopted as dependencies to track, not work)

Qt 6.11 (passthrough/overlay/sRGB landed?); Qt RGBA16 + async-render + PRIMARY_MONO;
XWayland [xserver!2118](https://gitlab.freedesktop.org/xorg/xserver/-/merge_requests/2118) /
[!2119](https://gitlab.freedesktop.org/xorg/xserver/-/merge_requests/2119) — these two fix
X11-app focus/pointer behaviour beyond output bounds and therefore affect **our** rootless
Xwayland integration too, independent of KWin; Mesa radeonsi `OVR_multiview` (relevant to any
GL-multiview path; our Vulkan renderer uses `VK_KHR_multiview`, unaffected).

## Rationale

- The two decisions that matter — *don't build from it*, *do mine it* — both follow from the same
  study: the topology and interaction layer are validated and transferable; the substrate and
  session model are foreclosing and not.
- The maintainer convergence is strong independent confirmation: the KWin maintainer, arguing
  from KWin's interests, arrives at the seam architecture ADR 0012 chose (windows/thumbnails/
  input provided over narrow interfaces; composition elsewhere). Two teams, opposite starting
  points, same boundary.
- The five-seam evidence is the most valuable single artifact: it is an empirical, diff-verified
  enumeration of exactly where a 2D window manager's assumptions break in 3D — the checklist
  zxr's window model gets to satisfy by design instead of by patch.
- Reserving the session (rather than packaging now) matches this workstream's exploration scope
  and keeps ADR 0006's "optional sessions" posture consistent: StardustXR, WayVR, kwin-vr — three
  alternatives, three different architectures, all packaged only as evidence needs them.

## Alternatives considered

- **Build Mura's compositor from KWin VR** (amend ADR 0006): rejected — Decision §1's four
  reasons; the 3D-tier foreclosure alone is disqualifying, and it is the tier that
  differentiates Mura from every 2D-in-VR product.
- **Ignore it**: rejected — it is the only shipping evidence of a complete 2D tier's WM-core
  requirements, it contains the best available Galaxy XR bring-up code, and its maintainer
  discussion independently validates our modularity architecture.
- **Contribute our seams to KWin** (make KWin a consumer of zxr-style integration): not ours to
  decide and out of scope; but if KDE lands Vlad's preferred shape (window info + thumbnails +
  input forwarding over protocols), a future KWin could consume the same seam family ADR 0012
  §4 defines. *Superseded update:* Mura is now specifying that shape itself —
  [ADR 0014](0014-toplevel-delegation-protocol.md) / `zspatial-toplevel-export-v1`, with a KWin
  producer MR as milestone M-B; the revisit trigger is M-B's outcome.
- **Package kwin-vr now**: rejected as scope — this workstream is exploration and design; the
  reserved enum value plus the pinned recipe in `references/` is the complete design artifact.

## Consequences

- ADR 0006 gains a pointer to this ADR as the evaluated "adapt an existing DE" alternative; its
  decision stands unamended.
- zxr's 2D-tier design (composition doc §7.3, registry window-model/input rows) inherits the
  five mirrored seams and the preflight/format-filter patterns as requirements — sharpened
  wording, no new components.
- Doc 07's Galaxy XR section gains the `monado-galaxyxr` bring-up evidence; gaze-driven foveation
  is recorded as evidence under [28 §Open questions item 7](../../research/28-eye-tracking-stack.md)
  (ADR 0011 unchanged; foveation policy placement remains open).
- `mura.xr.shell = kwin-vr` is reserved in documentation; contract/packaging changes land with the packaging work (implementation-path §5 owns the ordering).
- The references manifest pins the four fork repos for reproducible future study.

## Amendment 2026-09-26 — the input seams given their model (research/63)

Decision 2 mirrored five seams from the fork's record, two of them input seams: hover/focus
resolution as a pluggable policy and an unbounded pointer space. They named *where* the policy
sits, not *what* it is. [research/63](../../research/63-xr-input-focus-selection-from-comparables.md)
derived the model from the comparables (the pinned XR shells, the OpenXR standard and Monado's
status, the Wayland seat model, the 2D desktops' focus policy, MRTK3/StereoKit/godot-xr-tools,
and visionOS/Horizon/Android XR/HoloLens as mechanism evidence). The owner ruled the three forks
the evidence split on; the converging items were acted on under rule 8. The design is
[spatial-input.md](../spatial-input.md); this amendment records the rulings.

1. **Tiered targeting, one active mode, degrade by precision** (owner's rule: "highest precision
   with eye gaze and degrade down"). With eye tracking, gaze targets whatever commits — pinch,
   controller trigger, HMD button, dwell (visionOS's position, incl. for tracked controllers).
   Without eyes and with controllers in hand, the controller aim ray targets. Without eyes and
   with hands, the hand ray targets. The floor is the head ray (research/42). Direct touch
   overrides rays inside a distance band. The tier comes from the device contract and the
   runtime's reported capabilities, never from code. Meta's opposite choice with eyes on
   (controllers switch targeting to their ray) is the recorded dissent.
2. **Two transports, by device class.** Hands and gaze are **touch-class**: the client sees
   `wl_touch` — a position at `down`, drags as touch motion, each hand a contact (two-handed
   zoom/rotate are two contacts) — and never a hover position. Mice, trackpads and controllers
   (when controllers target) are **pointer-class**: `wl_pointer` with hover, cursor and axis.
   This is the visionOS/Android XR model in Wayland terms; it makes gaze privacy a property of
   the transport rather than a policy.
3. **Gaze never reaches a client**, with one specified exception: scrolling the gazed element
   from a stick or wheel enters the pointer at the gaze point, sends `axis`, leaves — disclosure
   only on the user's scroll action, the class of a tap.
4. **One logical pointer per seat**, handed to the pointer-class device that last committed
   (kwin-vr, xrdesktop, WiVRn); hands need no such rule.
5. **Focus follows the commit, never hover.** Activation is serial-validated (`xdg-activation`
   tokens carry the commit's serial; tokens without one are urgency-only); refused activation is
   urgency, presented by a shell component, never by the compositor moving anything. Universal
   across the four 2D desktops' defaults, the XR shells and visionOS; Jacob 1990 is the reason.
6. **The mouse pointer is placed by gaze-warp, degrading to head-warp**; on a plane it moves in
   plane-local coordinates; between planes it is an angular ray from the head until it lands.
7. **Hand aim/pinch/poke synthesis is the runtime's** (`XR_EXT_hand_interaction` over Monado's
   Mercury joints, `ht_ctrl_emu`'s shape); zxr synthesizes from joints only as a bridge behind
   the same interface. The perception service produces layers, never input.

Numbers in the design (pinch hysteresis, hover ramp, near/far band, dwell, eyes→head timeout) are
stand-ins from the comparables until measured on Mura's trackers; the design marks each.
