# 70 — Input bring-up: the ruled architecture built, tested and measured (M1 input gate)

**Research date:** 2026-09-26 (§9 addendum 2026-09-27). **Question:** does the input architecture ruled in
[research/68 §9](68-input-architecture-from-comparables.md) (in-compositor on the state loop; one
OpenXR action set as the XR seam; smithay's `InputBackend` as the non-XR seam; a closed enum of
source kinds; KWin's stage order) work end to end on the nested host, what does it cost per
tick, and does the §9.1 trigger fire? Plan: `input_module_implementation_and_gate` (2026-09-26).
Design: [spatial-input.md](../architecture/spatial-input.md) §1a–§13; contract:
[spec §8](../../specs/zxr-core.md). Predecessors: research/63 (the input model), research/66
(the reserved system input), research/69 (the buffer-hold policy this gate was to follow).
**Labels** as research/65: **measured (host)** — AMD Strix Halo, RADV, Monado simulated HMD at
60 Hz, 32 cores, structure not absolute time; **analytic**; **hardware-deferred** — trackers,
eye gaze, the bridge's cost at 90 Hz, everything the simulated HMD cannot produce.
**Budget impact:** a results document; the numbers feed spec §12's input rows and
[budgets.md](../architecture/budgets.md).

## 0. Summary

- **Built** (`pkgs/zxr/src/input/`, 22 files, 8.2 k lines, 123 unit tests): the nine-slot static
  chain `reserved → mode → a11y → stabilize → tier → hit → grabs(no-op) → im → seat`, the
  `SourceKind` enum (`Head, Gaze, Hand(L|R), Controller(L|R), Pointer, Keyboard`), the OpenXR
  action set (`khr/simple_controller`, `ext/hand_interaction_ext`, `ext/eye_gaze_interaction`,
  Touch and Index profiles; `MNDX_system_buttons` by raw name; spaces located in the one batched
  `xrLocateSpaces` the tick already makes), the §10 hand bridge, libinput and EIS intake on the
  state loop, `wl_touch` and `wl_pointer` transports, cursors (reticle quad, client cursor quad
  from `set_cursor` surfaces or `cursor-shape-v1` names via the Xcursor theme), plane emphasis
  through `XR_KHR_composition_layer_color_scale_bias`, focus/activation, the text-entry seam,
  presence (`XR_EXT_user_presence`), idle activity (`ext-idle-notify`, `idle-inhibit`), and a
  test-only injector (`zxr ctl source …`, `zxr ctl present`, `zxr ctl mode`, `zxr ctl a11y`).
- **Functional harness** (nested, outside the repo): 10 of 10 checks pass — head-ray floor
  parity, action set attached, synthetic-hand touch into a GTK menubar, synthetic controllers with
  left→right pointer handoff, EI keyboard/pointer typing into foot, xdg-activation urgency without
  a serial (focus unchanged), input-method binding (GTK entry + wvkbd), reserved `system` press
  consumed at slot 0, locked-mode gate, user presence off/on (§2).
- **Census (measured):** 7.07 runtime calls per frame with the action set attached, controllers
  present or not — `xrWaitFrame`, `xrBeginFrame`, `xrLocateViews`, `xrLocateSpaces` (one, batched,
  all six action spaces), `xrSyncActions`, `xrEndFrame`, ≈1.08 `xrPollEvent`; the panel path adds
  its three per dirty panel (10.0 under a committing client). Up from 5.07 (research/65) by the
  two the design named. `xrSyncActions` costs 21 µs without controllers and 45 µs with two (the
  per-device IPC round trips inside Monado, `oxr_input.c:2045-2050`); `xrGetActionState*` is 14
  calls per frame at 0 µs — client-side reads, no IPC (§3.1).
- **Latency (measured):** per-event dispatch keeps intake age at 0 (nothing queues to the tick);
  the event→`xrEndFrame` interval is bounded by the display period by construction — 8.4–8.7 ms
  per-event mean and 16.2–16.4 ms oldest-event mean under a 1 kHz pointer stream, idle or with
  the research/62 §8 storms behind it; **the §9.1 trigger does not fire** on the state-loop
  shape, read with the caveat in §3.2. 0 missed deadlines in every trial of the clean run.
- **Wake-ups (measured):** idle 1.7–1.9 k slices/s across zxr's threads with the action set
  attached, against 1.0 k on the spine (research/65's baseline) — the two extra IPC calls per
  frame, each a blocking recv; 2.8 k with the 1 kHz stream (one dispatch per event, as ruled);
  33–51 k under the client storms (the storms', research/69). RSS anon +0.6 MB, binary +1.5 MB.
- **Owner items** from the build, one line each in §5 and §6: the stand-ins every lane numbered
  (all flagged, none measured on trackers), the judgments the lanes recorded, four Monado upstream
  items, and the theme/size settings key the cursor theme needs.

## 1. What was built, where

| stage / seam | file (`pkgs/zxr/src/input/`) | design | comparables in the file header |
|---|---|---|---|
| types, `Chain`, `Input` state, injector, per-event `dispatch`, per-tick `tick` | `mod.rs` | §1a | KWin `input.h:366-393` order; smithay `InputBackend` |
| reserved system input (short/long/double/chord; `system` action, `hmdButtons.systemRole`, `SYSTEM_GESTURE`/`MENU_PRESSED` flags) | `reserved.rs` | native-openxr-apps §6 | research/66 §12–13; Meta/PICO press windows [external] |
| mode gate (greeter/lock), presence doff/don → `xr_suspended`, cancels, `xdg_toplevel.suspended` | `mode.rs` | ADR 0007; §1a presence row | KWin `LockScreenFilter`; `XR_EXT_user_presence` |
| a11y: dwell-as-commit, pointer gain | `a11y.rs` | §13 | KWin `dwellclicker.cpp:150-152` |
| user activity (idle ladder, `ext-idle-notify`) | `activity.rs` | ADR 0007 | KWin `input.cpp:3169-3172` spy shape |
| stabilization: low-pass, target lock, relaxation, event-time compensation | `stabilize.rs` | §4 | MRTK3 select threshold; wxrc `input.c:300-307` |
| tier arbiter, gaze quality (blink vs loss), controller "held", tracking loss | `tier.rs`, `quality.rs`, `held.rs`, `loss.rs` | §3 (ruled) | WiVRn `constants.h:42-53`, `imgui_impl.cpp:726-768`; xrdesktop `xrd-input-synth.c:198-205` |
| hit test over the scene's member pass | `hit.rs` | §4, spec §5a | scene arenas (research/62) |
| touch transport (`wl_touch`, contact ids, cancel on loss), pointer transport (`wl_pointer`, plane-local, warp, handoff, gaze-scroll), cursors, emphasis | `touch.rs`, `pointer.rs`, `cursor.rs`, `emphasis.rs`, `seat.rs` | §5, §7, §8, §9 | smithay `touch/mod.rs:392-403`, `pointer/mod.rs`; niri `input/mod.rs:3542-3547`; MRTK3 reticle |
| the OpenXR action set, spaces as frames, samples per tick | `actions.rs` | §1a XR seam, §2 | `input.adoc:499-505, 826-830`; wayvr/xrdesktop/WiVRn bindings |
| the hand bridge (pinch/poke/ready/system gesture from joints) | `bridge.rs` | §10 | StereoKit `input_hand.cpp:395-409`; Monado `ht_ctrl_emu` |
| focus and activation (commit serials, tokens, urgency, new-window rule) | `focus.rs` | §6 (ruled) | mutter `window.c:2013, 2125`; smithay `xdg_activation` |
| text entry (`text-input-v3` → `input-method-v2`, `virtual-keyboard-v1`, physical-key suppression) | `text.rs` | §12 | KWin `inputmethod.cpp:864-925`; StereoKit `platform.cpp:258` |
| libinput on the state loop (libseat session, `hmdButtons` roles) | `libinput.rs` | §8, §1a (ruled 9.1) | smithay `backend/libinput/mod.rs:583,707`; anvil `udev.rs:227-307`; libinput `udev-seat.c:82-99` |
| EIS server (libei sender clients; `Flags::EMULATED`) | `ei.rs` | §1a | libei `README.md:32-71`; mutter/KWin/cosmic EIS |
| cursor theme (`cursor-shape-v1` names → Xcursor images) | `theme.rs` | §7 | KWin `cursor.cpp:117-126`; wlroots `xcursor.c:515-563`; niri `cursor.rs:189-193` |

Outside the module: `xr.rs` (action set attach, `xrSyncActions`, presence event, colour scale/bias
on emphasised quads), `state.rs` (seat with touch, cursor-shape, idle notify/inhibit, focus/IM
states), `main.rs` (`input::tick` after `xrLocateViews`; the reticle and cursor panels as band-5
quads; the event→end latency close after `xrEndFrame`), `journal.rs` (§3's counters),
`control.rs` (the injector grammar). smithay features added: `backend_libinput`,
`backend_session_libseat`, `backend_libei`, `backend_udev`; `default.nix` gains `libinput`,
`seatd`, `udev`; one new crate, `xcursor`.

**Per-event, not per-tick.** The spine first queued every sample to the tick (the "tick-bound"
shape §1a records as a rethink candidate with no comparable). Measured on the integrated chain
that put the oldest event's age at 14.8 ms mean / 31.5 ms max; dispatching each libinput/EI/
injector event through the chain when it arrives (the ruled §9.1 shape) gave 8.3 / 14.0 ms on
the same script. XR samples stay at the tick — the standard freezes them between syncs.

## 2. Functional harness (nested; measured)

Monado simulated HMD, `SIMULATED_LEFT/RIGHT=simple` for the controller trials, clients from the
dev-session PATH, `ZXR_NO_LIBINPUT=1` on the host (libseat would take the user's seat). Scripts
outside the repo (`/tmp/mura-input/harness/nested.sh`; the EI sender and the activation client
are two small Rust programs there).

| check | result | evidence |
|---|---|---|
| head-ray floor parity (foot focused, seat slot consuming) | PASS | `focus_changes=1 seat=238 frames=243` |
| OpenXR action set attached, spaces located every tick | PASS | `locate_spaces_ticks=180`, `profiles=4 spaces=6 hand_interaction=true`; no attach/sync error |
| synthetic hand pinch → `wl_touch` down on a GTK3 menubar (aim ray hits the menubar's row) | PASS | menubar `y=0.374` rotated `qx=0.196`; `seat_delta=14 stale_delta=0` |
| synthetic controllers: right select commits, left select takes the pointer, right takes it back | PASS | `focus_commits_delta=1 pointer_handoffs=3` |
| EI keyboard + pointer (reis sender: motion, click, `type "echo hi"`, Enter) → foot's shell | PASS | `ei_events=21 keys_emulated=16`; typed file `e c h o   h i \n` |
| xdg-activation without a serial: requester marked urgent, focus stays on foot | PASS | `urgent=true` on the requester row; `urgency_marks=1`; foot keeps `*` |
| input-method binding: GTK entry `text-input-v3` → wvkbd `input-method-v2` | PASS | IM binding logged; wvkbd `-L 200` |
| reserved `system` press consumed at slot 0, nothing forwarded | PASS | `reserved_delta=2 focus_delta=0`; log "reserved: summon" |
| locked mode: XR samples consumed at the mode slot; lifted on `mode normal` | PASS | `locked_delta=184; normal_delta=0` |
| user presence off → doff (XR suspended), on → don | PASS | `off_delta=1 on_delta=1` |

Two harness expectations were rewritten during integration, both harness errors rather than
compositor ones: the floor check assumed the seat consumes *every* tick (true of the spine's
head-floor, false once the seat consumes only hits), and the controller check counted
`focus_changes` where the only window already had focus (now `focus_commits`). The lock check
read its counter before `mode normal` had landed on the loop (a race with the 60 Hz injector).

Cursor path (`cursor-smoke.sh`, gtk3-demo under a controller pointer, `XCURSOR_THEME=breeze_cursors`):
4 panel swapchains (two windows, the reticle, the cursor), 9 panel passes over 451 frames (the
cursor redrawn on its two name changes, never per motion), `input_cursor_named_ticks=0`, 0 missed.

## 3. The gate numbers (measured, host)

Harness `gate.sh` (lane H): three trials per scenario, 3 s CPU/wake-up interval, medians; the
1 kHz pointer is `zxr ctl source pointer delta` on absolute `CLOCK_MONOTONIC` deadlines (achieved
999.98–999.99 Hz); wake-ups are scheduler slices summed over `/proc/<pid>/task/*/schedstat`; CPU
from `/proc/<pid>/stat`. Storm clients: vkcube `--present_mode 1` (MAILBOX, research/62 §8) and
glmark2 EGL swap-interval 0, both under research/69's release-at-replacement policy.

### 3.1 Census

| scenario | calls/frame | `xrSyncActions` mean/max µs | `xrLocateSpaces` mean µs | `xrGetActionState*`/frame |
|---|---:|---:|---:|---:|
| idle, no controllers | 7.07 | 21 / 1420 | 38 | 0 |
| idle, two simulated controllers | 7.07 | 45 / 315 | 61 | 14 (0 µs — cached reads) |
| 1 kHz pointer + vkcube MAILBOX | 10.0 | — | — | — |
| 1 kHz pointer + glmark2 EGL | 9.95 | — | — | — |

The seven: `xrWaitFrame` (the wait thread), `xrBeginFrame`, `xrLocateViews`, `xrLocateSpaces`
(one call, the head-relative view space plus the six action spaces — the batching the design
required), `xrSyncActions`, `xrEndFrame`, `xrPollEvent` ≈ 1.08 (the runtime-event timer, spec
§7). The census does not depend on the device count because Monado's per-device work is *inside*
`xrSyncActions` (`oxr_input.c:2045-2050`; three RPCs with two controllers and the HMD): it shows
as the call's time, 21 → 45 µs, not as calls. The plan's "13 + N_devices" expectation was the
unbatched shape; the batched locate and the client-side action-state reads keep it at seven.

### 3.2 Latency and the §9.1 trigger

Run at host load 6–7 on 32 cores (the earlier two runs, at load 15–24 from unrelated jobs on the
host, are kept in the harness log and not read here; they showed 1–42 missed deadlines in the
storms and the same latency structure).

| scenario | pointer Hz | oldest event→end mean/max ms | per-event event→end mean ms | event age mean/max µs | wake→end mean/max µs | missed deadlines |
|---|---:|---:|---:|---:|---:|---:|
| idle, no stream (both census rows) | 0 | — | — | 0/0 | 262/4781 · 248/4072 | 0 |
| 1 kHz pointer, idle client | 999.99 | 16.2 / 17.4 | **8.4** | 0/0 | 203/4311 | 0 |
| 1 kHz pointer + vkcube MAILBOX (13–29 k commits/s) | 999.99 | 16.4 / 17.6 | **8.7** | 0/0 | 324/4929 | 0 |
| 1 kHz pointer + glmark2 EGL swap-interval 0 | 999.99 | 16.4 / 17.8 | **8.7** | 0/0 | 398/9579 | 0 |

The display period is 16.67 ms (60 Hz simulated).

Two counters, and what each can say. `input_event_to_end_us_*` (oldest) is the age of the
*oldest* event a tick carried, at that tick's completed `xrEndFrame`. Under a saturating 1 kHz
stream every tick carries an event that arrived just after the previous `xrEndFrame`, so this
number is one display period plus the wake→end time by construction (16.7 + 0.2–0.4 ms) and
cannot be read against "one display period" — it equals it. `input_event_to_end_per_event_us_mean`
averages the interval over *every* event of the tick: the expected half period plus wake→end
(≈ 8.3 + 0.3 ms), which is what the wearer's finger sees on average. `input_event_age_us_*` (event
timestamp → chain processing) stays at 0 under every storm: nothing queues behind the loop's
worst dispatch, which is the failure §9.1's thread option exists to prevent. **Reading:** the
state-loop shape holds under both storms — intake is not delayed by client dispatch, the frame
period is the bound, missed deadlines are the storm's (research/69), not intake's. The trigger is
not met; no libinput thread. **Caveat, labelled:** the stream is the control socket's injector,
not a libinput fd — a real 1 kHz mouse goes through smithay's libinput source at the same calloop
priority, so the dispatch path is the same but the syscall path (one `read` per batch vs one
`connect`+`write`+`read` per event) is heavier here, not lighter; the host cannot exercise
libinput (§4).

### 3.3 Wake-ups and CPU

| scenario | wake-ups (slices/s, all threads) | zxr CPU ms/s | calls/frame |
|---|---:|---:|---:|
| spine baseline (research/65 harness, 5.07 calls, no action set) | 1 034 | 10 | 5.08 |
| idle, no controllers | 1 746 | 10 | 7.08 |
| idle, two controllers | 1 920 | 13 | 7.08 |
| 1 kHz pointer, idle client | 2 825 | 27 | 7.08 |
| 1 kHz pointer + vkcube MAILBOX | 36 562 | 297 | 10.07 |
| 1 kHz pointer + glmark2 EGL | 47 591 | 350 | 10.04 |

Five threads in the process (the state loop, the `xrWaitFrame` thread, and RADV's); the slices
are summed over all of them.

The idle rise from research/65's 1.0 k/s (spine, 5.07 calls) to ≈ 1.6 k/s is the two added IPC
round trips per frame at 60 Hz, each a blocking `recv` on Monado's socket — and the reason
spatial-input §1a names `xrSyncActions`' per-device fan-out the runtime's largest per-tick input
cost (upstream item: sync across devices in one message, as `xrLocateSpaces` batches spaces).
The 1 kHz stream adds ≈ 1 k slices/s: one dispatch per event, the ruled shape; the tick-bound
alternative (§1a's rethink candidate) would remove them at the cost of a frame of responsiveness
and is still not the default. CPU: idle 10–20 ms/s (1–2 % of a core) with the action set synced,
against 10 ms/s unsynced on the spine; the storms' 200–400 ms/s are the clients' commit rates
(research/69 §0), not input.

### 3.4 Memory and closure

Nested with foot after 5 s, the `nix build .#zxr` binaries: master `VmRSS` 57.1 MB (anon 7.6, file
46.0), this branch 61.2 MB (anon **8.2**, file 49.4) — +0.6 MB anon, +3.4 MB of mapped libraries
(libinput, libseat, udev, libei/libeis, xkbcommon already present); 5 threads both. Binary
4.95 → 6.41 MB (+1.46 MB, unstripped size work pending as before). Closure 364.3 → 370.0 MB
(+5.7 MB). Spec §12's fence (anon + binary + the one driver ≤ 60 MB) is untouched: anon 8.2 +
binary 6.4 + RADV 4.8 ≈ 19.4 MB.

## 4. What the host cannot prove (hardware-deferred / not exercised)

- **libinput through libseat on a real seat.** The host session owns the seat; `ZXR_NO_LIBINPUT=1`
  skips the backend. The plan's optional frame-VM proof (a `uinput` device under the unit) was
  not run. The intake path *after* the backend is the one EI exercised (same `InputBackend` →
  `sample_of` → `dispatch`).
- **Trackers.** Every stand-in in §5 (pinch thresholds, blink vs loss windows, held timeout,
  motion tolerance, near band) waits for hands and eyes.
- **Eye gaze.** The simulated HMD offers no `eye_gaze_interaction` (`eye_gaze=false` at attach);
  the gaze quality ladder is unit-tested only.
- **The bridge at 90 Hz.** Its cost while a native app is primary (two hand-locate RPCs per
  sample) is analytic: `hand_joints` calls are counted (`call_hand_joints_n`), 0 on the simulated
  HMD.
- **`XR_EXT_user_presence`.** Monado raises it from `HEAD_DETECT`, which the simulated HMD lacks;
  presence was driven by the injector (`zxr ctl present`), so the event handler is unit-tested
  and the doff/don logic nested-tested, the runtime event not.

## 5. Stand-ins and judgments recorded by the lanes (owner items, rule 4)

Every number below is a stand-in with its source named in the file; none is measured on Mura's
trackers. Listed so the owner can adjudicate or send them to the first-hardware list.

| where | stand-in / judgment | source and reason |
|---|---|---|
| `tier.rs` | controller > hand order when both target; only `Select` pins a commit; stale after 800 ms (= gaze fallback, symmetry) | WiVRn/xrdesktop have no expiry; **flagged** — and research/63 §1's "Transfer" line says hand-over-controller: the design corrects to the code or the code to the design (§6 item 1) |
| `quality.rs` | gaze fallback 800 ms inside the design's 500–1500 band; return hysteresis = fallback; blink 100–400 ms; §9's fallback treated as the same ladder | no comparable states an eyes→head number |
| `held.rs` | controller held for 2 s after last activity; motion 0.005 m | WiVRn never puts a controller down; invented, flagged |
| `loss.rs` | pinch close 0.75 / open 0.5 (WiVRn `trigger_click_thd` + MRTK3 select progress); poke −1 cm down / 0 up | WiVRn `constants.h:42-49` |
| `stabilize.rs` | 50 ms event-time compensation at the commit edge; MRTK3 select threshold | design §4 |
| `hit.rs` | shell/affordance wins within 2 cm of ray depth over content | class-aware hit, design §4 |
| `touch.rs` | contact ids Left 0 / Right 1 / other committer 2; pinch 0.75/0.25 edges; cancel (not up) on loss | smithay `touch/mod.rs:392-403`; no comparable numbers contacts |
| `pointer.rs` | gain 1.0 px/unit; wheel detent = niri's px; mouse motion takes pointer ownership; head claims the pointer before the tier exists | niri `input/mod.rs:3542-3547` |
| `cursor.rs` | the cursor layer's 64 px span subtends 1.5° (the client image at its theme pixel size inside it, ≈ 0.6° for a 24 px cursor); one lift 1 mm; fixed 64 px panel, grow-only; hidden while typing; **one element**: a mouse on a plane suppresses the ray's reticle (§9) | MRTK3 scales without an angle; kwin-vr lifts 15 mm, motorcar 10 mm, neither with a reason; the DRM cursor plane's fixed size; desktops hide on key; the one-element rule is the owner's 2026-09-27 ruling on §7's "as above" |
| `emphasis.rs` | 700 ms ramp (design 500–1000); scale 1 + 0.15·e | HoloLens hover ramp |
| `reserved.rs` | short < 400 ms, long ≥ 800 ms, double gap 300 ms, chord 1 s; reserved *before* a11y (KWin runs a11y first) | research/66 §11; the order is the design's §1a, flagged as a divergence from KWin |
| `a11y.rs` | dwell onset 200 ms, dwell 750 ms, tolerance 2° / 20 px | KWin `dwellclicker.cpp:150-152`; research/42 §5 |
| `activity.rs` | emulated (EI) input counts as activity by default | mutter/KWin spy path; flagged for a headset whose ladder also locks |
| `bridge.rs` | pinch 1.0/1.5 cm, 8 cm (StereoKit); palm cone 35°, hold 300 ms; dominant hand = right | no platform publishes its cone; the Settings1 dominant-hand key does not exist yet |
| `mode.rs` | keyboard samples pass the lock (the PAM conversation); everything else consumed | ADR 0007 I1 |
| `text.rs` | physical-key OSK suppression 5 min (StereoKit); the Seat stage emits keys (`IM_EMITS_KEYS=false`) | `platform.cpp:258` |
| `libinput.rs` | calloop priority above client sources — not set (calloop 0.14 has no priority API on this smithay); EIS socket path unscoped | flagged |
| `theme.rs`, `main.rs` | cursor theme/size from `XCURSOR_*` env until a Settings1 key exists; the cursor as **one** quad layer from **one** fixed swapchain (§9; was two quads and a per-size swapchain in the first pass) rather than a panel re-pass per motion | KWin `cursor.cpp:117-126`; every desktop's cursor plane; Meta's merge-co-located-layers guidance (research/67 §1) |
| `actions.rs` | one extra space locate per tick is folded into the batched call; `MNDX_system_buttons` *exposes* controller home buttons, it does not reserve them (§6 item 3) | Monado |

## 6. Open joins and corrections found while integrating

1. **Design ↔ code order for controller vs hand targeting.** research/63 §1's "Transfer" line and
   spatial-input §3's tier rule (gaze → controller → hand → head) disagree with each other on
   whether a held controller outranks a hand ray; the code follows §3. The owner's call; the
   docs pass corrects research/63 §1 to say so and cites this item.
2. **`Head` is `Class::Pointer` in the code** (the head ray drives the reticle and, before any
   tier exists, the pointer) while §3 lists the head ray under the touch-class "whatever commits"
   rule. Behaviourally the floor is unchanged from the spine; the class label is a docs item.
3. **`MNDX_system_buttons` wording** in spatial-input §1a and spec §8: the extension exposes a
   controller's home/system button as an ordinary input (`XR_MNDX_system_buttons`), it does not
   make the runtime reserve it; zxr's reserved stage does the reserving. Corrected in this pass.
4. **A hand pinch while a controller targets produces no hit** — non-targeting kinds are not hit-
   tested (the tier's one-targeting-source rule, Horizon's shape). Expected under the rule; recorded
   because a wearer will do it.
5. **Monado upstream items** (ADR 0013's list): `xrSyncActions` per-device fan-out (batch);
   `XR_FB_hand_tracking_aim` / a system gesture (bridged); `XR_EXT_user_presence` on the simulated
   HMD (`HEAD_DETECT` only); `XR_MNDX_system_buttons` on the simulated controllers (absent, so the
   reserved path is exercised through the injector's `system` button).
6. **smithay:** `deactivate_input_method` is `pub(crate)`, so OSK suppression is state-only
   (counted, not sent); `Priority` is not exposed on the calloop this smithay pins — libinput's
   source runs at the default priority.

## 7. Determinations

- **D1 — the ruled architecture stands as built:** state-loop intake, per-event dispatch, XR at
  the tick, the closed enum, the static chain. The §9.1 trigger is not met under either storm;
  no input thread (§3.2, with its caveat).
- **D2 — seven runtime calls per frame is the input floor's census**, not 13 + N: batching and
  client-side reads absorb the devices; `xrSyncActions`' time is where devices show (§3.1).
- **D3 — cursors are quads**, never panel re-passes; the theme is the freedesktop mechanism
  until the settings key exists (§5 last rows; owner item). **Amended by §9 (2026-09-27): one
  quad**, not one per element.
- Everything in §5 is a stand-in for the first-hardware list; everything in §6 is an owner or
  upstream item.

## 8. Sources

Comparables as cited per file in §1 (all `references/<clone>/path:line`; Meta/PICO/HoloLens
[external], mechanism only). Harness and scripts: `/tmp/mura-input/harness/` (outside the repo;
removed with the workspace). Journals per trial: `gate.sh`'s `journal-<scenario>-<trial>.txt`.

## 9. Addendum (2026-09-27) — the cursor as one layer in one fixed swapchain

**Question.** The first pass (§1, §5) drew the reticle and the client cursor as two quad layers
from two swapchains, the client's swapchain recreated whenever the image's size changed. Is that
the efficient shape for an embedded target, and if not, what is? Plan:
`one-layer_xr_cursor` (2026-09-27). Design: spatial-input §7 rev 0.3; contract: spec §8 rev 3.6.

### 9.1 The accounting (analytic, from §2.1 of research/65 and §1 of research/67)

A submitted layer is redrawn per view per frame by Monado's squasher: ≈ 0.02 ms per quad on
this host, ~0.1 ms per layer and a 16-layer cap on a Quest 2 [external]. Pointer *motion* costs
a quad layer nothing on the GPU — its pose is a struct `xrEndFrame` already carries — so the
cursor's cost is its layer count per frame plus whatever redraws its panel. Two layers where one
element would do is a permanent ~0.1 ms/frame and two slots of the quad budget (`main.rs`
subtracts the cursor's quads from `quad_budget()` — two windows lost quad status to the cursor).
A swapchain recreated per image size is an `xrDestroySwapchain` + `xrCreateSwapchain` + image
enumeration + Vulkan import on every arrow ↔ I-beam whose images differ in size. The alternative
shape — the cursor drawn into the window's panel (Simula `CanvasBase.hs:162-163,773-796`, wayvr
`overlays/screen/capture.rs:251-281`) — is worse on a bandwidth-bound SoC: OpenXR swapchain
images rotate and cannot be patched, so it is a full-panel re-blit per motion tick (1080p ≈
16.6 MB plus three swapchain RPCs). The cursor-plane shape — one fixed-size plane, one image,
composited by whoever scans out (DRM `cursor` planes; kwin-vr's single node whose texture is
rebuilt only on `currentCursorChanged`, `VrKwinCursor.qml:20-41`, `kwincurrentcursor.cpp:24`;
Meta's guidance to merge co-located layers into one) is the precedent, and it says one layer and
one fixed panel.

### 9.2 The rule (owner's ruling, 2026-09-27)

One cursor element at a time: the client's cursor when the logical pointer is on a plane (a ray
that owns the pointer gets the ring composited around the image, in the same panel); the reticle
at the ray's hit otherwise; nothing under gaze (a mouse under gaze keeps its own cursor — its
position is the mouse's). A mouse on a plane shows no ray reticle beside it: the head ray's look
changes no focus (§6) and only decides where the pointer warps (§8) — the owner's answer to §7's
"the pointer-class cursor as above" was that the mouse takes priority and the head ray is the
degraded-state device. The rule follows from §5's one logical pointer (ruled) rather than adding
to it. The second controller's drawn ray while the first owns the pointer (§5) is a second
element by design and is not built (spatial-input §15).

**Gaze, the second pass (same day).** The first build hid the cursor under gaze with a rule in
`cursor.rs` while the transport kept a head-owned `wl_pointer` hovering the client at its last
point (`enter` with no `leave`) — a §5 seam, not a §7 one. Ruled (owner, option a): when gaze
takes the tier a **ray-owned pointer is released** (`PointerLogic::release_for_gaze`: `leave` +
`frame`, no plane, owner kept; the ray's next sample re-enters when it retakes the tier); a
mouse-owned pointer is not touched. The cursor then needs no gaze rule at all — no plane and no
hit resolve to no layer. Consequences recorded: hover/tooltip state ends correctly on the client;
a gaze flicker costs one `leave`/`enter` pair per drop-and-return, bounded by the two 800 ms
hystereses; a `pointer-constraints` lock on a *controller*-owned pointer is lost when gaze takes
the tier (consistent with amendment 1; mouse locks unaffected). Verified nested (head-only floor
on foot, nominal gaze streamed at 60 Hz for 10 s, then stale): `input_tier_changes` +1 → gaze,
`input_pointer_releases` = 1, **0 cursor layers over 301 frames** under gaze with
`owner=None on_plane=false reticle=false`, the head re-enters when gaze goes stale, and the
client's `WAYLAND_DEBUG` log shows exactly `enter → leave → enter`.

**Two preferences (owner, same day; spatial-input §14):** `input.cursor.ray = both | image |
ring` — what a ray that owns the pointer shows (default `both`, the ruling above; `image` is
kwin-vr's desktop look, `ring` MRTK3's) — and `input.cursor.scale = angle | plane` (default
`angle`; `plane` is kwin-vr's pixels-per-unit). Both are `runtime` per-user keys through
`org.mura.Settings1` when the daemon carries them; until then `zxr ctl cursor ray|scale …`.
Verified live: `image` → `Image`, `ring` → `Ring`, `plane` → 77 mm at 1.5 m (64 px × the plane's
1.2 mm/px) against 39 mm at `angle`. Not a setting, deliberately: the 1 mm lift (a painter's-order
artefact with no wearer-visible meaning — Monado does no depth test between layers) and the
tracker calibrations of §5, which are device-contract numbers, not preferences.

### 9.3 As built (`input/cursor.rs`, `main.rs`, `state.rs`, `journal.rs`, `input/seat.rs`)

`Cursors::layer()` resolves the seat's two inputs (the targeting ray's hit; the logical
pointer's plane point with its owner) to `Option<CursorLayer { pose, m_per_px, content: Ring |
Image | RingAndImage, image }>`. One `cursor_panel` swapchain of `CURSOR_PX`² = 64² (grown only
when a `set_cursor` image needs more room around its hotspot — `panel_side_for` — never shrunk),
created on first use; the ring texture once; a pass into the panel only when `CursorKey {
content, image: Named(icon) | Surface(id, commit) }` changes; the hotspot at the panel's centre,
so the one `QuadLayer` is centred on the point and sized `m_per_px × side`, with 64 px
subtending 1.5° at the point's distance (the client image at its theme pixel size inside that
span, ≈ 0.6° for a 24 px cursor); lifted 1 mm. Journal: `cursor_layers` (+ per frame),
`cursor_passes`, `cursor_swapchains_created`; `zxr ctl list` ends with a `cursor:` line (content,
position, size, panel, and the inputs). 124 unit tests (the precedence table, the fixed panel's
growth rule, the ring, the visual-angle scale).

### 9.4 Measured (host; medians of three trials; Monado simulated HMD at 60 Hz, breeze_cursors at 24 px, `ZXR_NO_LIBINPUT=1`, load 3–5 on 32 cores)

Master `4dc8313` against the branch. `d_` = delta over the measurement window. Master has no
cursor counters: its cursor passes are `panel_passes` minus the windows' (the branch's
`panel_passes` measures the windows alone under the same script), its layers per frame are read
from the code (`main.rs:733-749` at `4dc8313`: reticle + client cursor whenever both have an
image), labelled analytic.

| scenario (window) | frames | cursor layers/frame | cursor passes | cursor swapchains | window panel passes | zxr CPU ms/s | wake-ups/s | missed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| **crossing** — mouse on gtk3-demo, 20 arrow ↔ I-beam crossings in 10 s | 612 | master **2** (analytic) · branch **1.00** (612/612) | master 20 (45 − 25) · branch **20** | 0 · 0 | 25 · 25 | 16.7 · 12.7 | 2 022 · 1 906 | 0 · 0 |
| **stream** — 1 kHz mouse deltas for 10 s (achieved 1000.10 Hz; 10 000 events) | 605 | 2 · **1.00** | 0 · **0** | 0 · 0 | 0 · 0 | 20.8 · 16.9 | 2 988 · 2 864 | 0 · 0 |
| **head only** — the floor on foot, 8 s | 485 | 2 · **1.00** | 0 · 0 | 0 · 0 | 0 · 0 | 13.6 · 8.7 | 1 949 · 1 894 | 0 · 0 |
| **head only, then gaze** — nominal gaze streamed at 60 Hz, 3 s window after the tier took gaze | 180 | master 1 (analytic: the reticle clears, the head-owned client cursor stays) · branch **0** (0/180) | — | — | — | — | — | 0 · 0 |
| **beside xrgears** — zxr overlay, head-owned pointer on foot, 20 s | 1 207 | 2 · **1.00** | 0 · 0 | 0 · 0 | 0 · 0 | 9.4 · 10.4 | 1 799 · 1 799 | 0 · 0 |

Per session: master creates **3** swapchains for one window (the window, the reticle, the
cursor); the branch **2** (the window, the cursor) — `cursor_swapchains_created = 1` in every
trial, including the crossing trials' 20 shape changes. The tier took gaze in every trial
(`input_tier_changes` +1) and the branch submitted no cursor layer for the 180 frames after it.

**Reading.** The one-element rule holds at exactly one layer per frame in every scenario and
zero under gaze; passes track content changes (20 for 20 crossings) and never motion (0 for
10 000 events); the panel is created once. The layer saved is one of the quad budget's slots
and, on Monado, one squasher draw per view per frame — below this host's `fdinfo` noise floor
(research/67 §2.1: 1–4 layers are inside ±5 ms/s), and **not measured here**: this Monado build's
compositor fd exposed no `drm-engine-gfx` line in `fdinfo` during these runs (its VRAM
allocations show; xrgears' and zxr's engine time show), so the Monado column research/67 had
is absent. The Quest-class figure stays analytic: ~0.1 ms/frame and 1/16 of the layer budget
per layer [external]. CPU and wake-up differences (−4 ms/s, −100/s) are inside run-to-run noise
and are not claimed.

**What the host could not show.** (i) Master's per-size swapchain recreation: with one theme at
one nominal size every breeze image is 24×24, and both clients here (foot, GTK 3.24.52) speak
`cursor-shape-v1`, so no size change occurred and master created 0 swapchains in the crossing
window too; the path is exercised by `set_cursor` surfaces of another size (a client with its
own theme or scale) — the fixed panel removes the dependency rather than measuring it. (ii) The
controller-ray crossing: a controller injected at 4 Hz is a lost controller to the tier (800 ms
staleness) and at 60 Hz its crossings landed differently run to run (the ray stabiliser and the
window arc), so the crossing scenario uses the mouse; the controller path was verified by hand
(`RingAndImage` every tick, one pass per crossing, 1.5° at 1.5 m = 39 mm) and is not in the
table.

### 9.5 Determination

**D4 — the cursor is one composition layer from one fixed-size swapchain**, content by the §9.2
precedence; passes on content change only; nothing submitted when there is nothing to show.
Amends D3. Stand-ins added to §5 (`CURSOR_PX` 64, lift 1 mm, the client image at theme pixels
inside the 1.5° span); the second controller's drawn ray is a spatial-input §15 item.

Harness (outside the repo, removed with the workspace): `/tmp/mura-cursor/harness/run.sh`
(scenarios), `pointer-stream` (the 1 kHz mouse and the 60 Hz pose/gaze streams on absolute
`CLOCK_MONOTONIC` deadlines), `summarize.sh` (medians); journals per run under `out/`.
