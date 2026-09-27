# 73 — User-configurable decisions: what the designs and code fixed, and which of it is the wearer's to change

**Research date:** 2026-09-27. **Question:** every value and behaviour Mura's designs and code have
fixed since R0 — built, designed-not-yet-coded, or merely implied by behaviour — sorted into what is
a per-user **preference** (a settings key the wearer may change), what is a **device calibration**
(the device contract's), what is an **engineering constant** (nobody's knob), and what is
**contested** (the owner's call); each row against the settings a shipping desktop or XR shell
exposes for the same thing, in both directions — Mura's decisions checked for a precedent, and
the comparables' settings checked for a Mura counterpart. Plan: `user-configurable_decisions_audit`
(2026-09-27). **Scope (owner):** every module except OOBE/`mura-setup`, first-run onboarding and
the greeter's PAM flow; ADR 0007's idle and grace timings are in. **Mechanism:**
[specs/settings-schema.md](../../specs/settings-schema.md) rev 3 (D7, landed) — `mkSetting` on a
NixOS option compiles a key into `/etc/mura/settings-schema.json`; `mura-settingsd` serves
`org.mura.Settings1`; mutability `immutable` (Nix owns the value) or `mutable` (Nix owns the
default, the wearer's override survives rebuilds, `Reset` returns to it). **Labels:** every cite is
`references/<clone>/path:line` from the pins (MANIFEST.json) or a repo path; platform settings
without source are [external]. **Budget impact:** a research document. Phase B (declaring keys,
zxr as a consumer, moving calibrations to the contract) begins only after the owner rules §5.

## 0. Summary

- **The mechanism exists and is barely used.** Three keys are compiled today, all in
  `lib/contract/default.nix` (`hardware.ipd.meters` `:126-131`, `xr.passthrough.latencyMode`,
  `xr.passthrough.upperLimbVisibility`) plus the `places.entry` template (`modules/os/settings.nix:79-83`).
  **Thirty-six** dotted keys are named in design docs and compiled nowhere (§4); zxr consumes no
  key at all — every input preference is a "control socket now, Settings1 later" stub
  (`pkgs/zxr/src/input/a11y.rs:214`, `bridge.rs:46`, `cursor.rs:33`, `mod.rs:477-481`).
- **The table (§2): 123 rows** plus the engineering-constant list of §2.12. **48 per-user
  preferences** (4 already compiled; 2 are behaviours every desktop exposes as a toggle and Mura
  fixed silently — hide-cursor-while-typing, the OSK suppression window), **16 device
  calibrations** (all tracker/display numbers now living as `pub const`), **30 engineering
  constants** in rows and a further list in §2.12, **5 contested** rows (11 owner questions in
  §5, since several questions are about mutability, seeds and mechanism rather than a row), 1
  security policy, 1 out of scope. Direction 2 adds **22 finding rows, grouped as 17 findings in
  §3** — knobs every desktop or several XR shells expose for which Mura has no decision at all:
  natural scroll, left-handed buttons, double-click interval, drag threshold, keyboard repeat,
  XKB layout/options, hide-cursor-after-idle, locate/shake-to-find, animation speed / reduced
  motion, UI scale, idle-delay and lock-delay as named keys, click-freeze, stick deadzone,
  secondary-click-by-hold, sticky/slow/bounce keys, OSK enable.
- **Ownership posture (owner, 2026-09-27; the KDE-on-NixOS shape).** Every preference is
  `mutability = mutable` (spec rev 4 §3 — the axis was renamed from `ownership = declarative | runtime` during this audit because `runtime` read as runtime-only; a `mutable` key is Nix default **and** wearer override): Nix owns the default (the wearer may set it in their configuration),
  the wearer may override it at runtime and the override survives rebuilds, `Reset` returns to
  Nix, an administrator enforces with locks (spec §7). `immutable` is for policy and
  security-relevant keys only. The 23 wm/system/games keys that
  [window-workspace-management.md §12](../architecture/window-workspace-management.md) and
  [native-openxr-apps.md §9](../architecture/native-openxr-apps.md) marked "declarative by default" (Nix-only, in rev 3's words)
  are preferences by the tests below and are now `mutable` — **ruled 2026-09-27 (Q1: all 23)**; the two design docs are updated.
- **How zxr should consume settings (§6):** the comparables split between a bus client (mutter
  via GSettings/GDBus, KWin via `KConfigWatcher`, xrdesktop via GSettings) and a file watcher
  (cosmic-comp's `ConfigWatchSource` on calloop; niri's polling thread). `mura-settingsd` is
  already a library whose `Engine` resolves artifact + store in-process, and its zbus pin has
  `blocking-api`. The two real options and their budget are in §6; the owner picks (§5 Q7).

## 1. Method

**Three kinds of decision** enter the table, tagged in the `kind` column: **built** (a named
constant or default in `pkgs/*/src` or `lib/contract`), **designed** (a value or behaviour fixed
in a design doc, ADR or spec but not coded), **implied** (a behaviour the code exhibits with no
named value — focus follows commit, the pointer warps to the looked-at plane). Harvest: every
`const`/`pub const` in `pkgs/{zxr,mura-perception-intake,mura-session,mura-recovery,mura-settingsd}/src`
(zxr 104; the others are protocol ids and paths — §2.12), every "stand-in" in the design corpus,
the three key lists (spatial-input §14, wm §12, native-openxr-apps §9), research/70 §5, the
`lib/contract` session/lock options, and the module docs of `pkgs/zxr/src/input/*.rs`.

**Four buckets, one test each.** *Preference*: a shipping desktop or XR platform exposes the
same knob to its user. *Calibration*: the value depends on the tracker, display or target, not
on the person. *Constant*: no wearer-visible meaning, or changing it breaks an invariant.
*Contested*: the tests disagree, or desktop and XR precedents disagree, or a ruling is required —
an owner question in §5, rule 8 form.

**Two directions of comparables (rule 7).** Direction 1: each Mura row names the precedent key
and *why* the comparable exposes it (its description string, the panel it sits in) or says "no
comparable — Mura's own". Direction 2: every key in the comparables' settings surfaces that
concerns something Mura's compositor, input, session or window model does is checked for a Mura
decision; a knob with none is a **finding** (§3). Surfaces read, all from the pins: mutter
`data/org.gnome.mutter{,.wayland,.experimental}.gschema.xml.in`; KWin `src/kwin.kcfg`,
`src/kcms/*/*.kcfg`, `src/plugins/{dwellclicker,hidecursor,shakecursor}/*.kcfg`; the 31
`gsettings-desktop-schemas/schemas/*.gschema.xml.in`; gnome-shell's schema; Plasma's kcfgs and
`kscreenlocker/settings/kscreenlockersettings.kcfg`; `cosmic-comp/cosmic-comp-config/src/{lib,input}.rs`;
`niri/resources/default-config.kdl` + `niri-config/src/lib.rs`; Hyprland `src/config/ConfigValues.cpp`
(the pin has no `example/hyprland.conf`; `example/hyprland.lua`); XR: `xrdesktop/res/org.xrdesktop.gschema.xml`,
`kwin-vr/src/plugins/vr/kwinvr.kcfg`, `wayvr/wlx-common/src/config.rs` (= wlx-overlay-s), WiVRn
`docs/configuration.md` + `server/driver/configuration.{h,cpp}` + `client/configuration.h`,
`simula/config/config.dhall`, StereoKit `stereokit.h:430-454` (`sk_settings_t`), Stardust
`src/main.rs:108-158` (flags), Monado `config_v0.schema.json`; platforms [external].

**Columns.** `id` · `kind` · `where` (the decision's home) · `now` (value or rule) · `bucket` ·
`comparable` (precedent + its reason, or "none") · `proposed key · type · default · apply`
(preferences only; `mutable`, `per-user`, `preference` unless stated) · notes. Defaults proposed
are the current values; a different default is an owner item.

## 2. The table

### 2.1 Cursor (spatial-input §7; research/70 §9)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| cursor.theme | built | `input/theme.rs:46-50` env `XCURSOR_THEME` | "default" | **preference** | GNOME `org.gnome.desktop.interface cursor-theme` "Cursor theme name" (`interface.gschema.xml.in:180-184`); Plasma `kcminputrc Mouse/cursorTheme` (`cursorthemesettings.kcfg:8-11`); niri `cursor.xcursor-theme` | `input.cursor.theme` · string · "default" · live |
| cursor.size | built | `theme.rs:25-26` `DEFAULT_SIZE = 24`, env `XCURSOR_SIZE` | 24 | **preference** | GNOME `cursor-size` 24 (`:185-189`); Plasma `cursorSize` 24; niri `xcursor-size` | `input.cursor.size` · int [16,96] · 24 · live |
| cursor.ray | designed/built | `cursor.rs` `RayCursor`; §14 | both | **preference** (ruled) | kwin-vr shows the client cursor only (`VrKwinCursor.qml:20-41`); MRTK3 a reticle only — the two positions are the enum | `input.cursor.ray` · enum both\|image\|ring · both · live |
| cursor.scale | designed/built | `cursor.rs` `Scale`; §14 | angle | **preference** (ruled) | visionOS dynamic scale [external] vs kwin-vr `ppu` pixels-per-unit (`kwinvr.kcfg:29`) | `input.cursor.scale` · enum angle\|plane · angle · live |
| cursor.angle | built | `cursor.rs:62-64` `RETICLE_DEG = 1.5` | 1.5° | **preference** | GNOME/Plasma expose size, not angle; HoloLens ≥ 2° target [external]; kwin-vr sizes by ppu — the angle is Mura's own size knob | `input.cursor.angle_deg` · double [0.5,4] · 1.5 · live |
| cursor.hide_typing | implied | `cursor.rs:42-45` always on | on | **preference** (silently fixed) | KWin hidecursor `HideOnTyping` true (`hidecursorconfig.kcfg:11-13`); niri `hide-when-typing`; Hyprland `cursor:hide_on_key_press` false (`ConfigValues.cpp:702`) — every desktop a toggle | `input.cursor.hide_when_typing` · bool · true · live |
| cursor.one_element | designed | §7, research/70 §9.2 | one layer; mouse suppresses ray reticle | **constant** (ruled) | DRM cursor plane; kwin-vr one node | — |
| cursor.lift | built | `cursor.rs:70-71` 1 mm | 1 mm | **constant** | kwin-vr 15 mm, motorcar 10 mm, no reason stated; painter's-order artefact, no depth test between layers (research/65 §2.1) | — (first-hardware list) |
| cursor.panel_px | built | `cursor.rs:65-69` 64 | 64 px | **constant** | DRM `DRM_CAP_CURSOR_WIDTH` fallback 64 (wlroots `backend/drm/drm.c:54`, KWin `drm_gpu.cpp:74`) | — |
| cursor.ring_geometry | implied | `cursor.rs:378-379` 0.47/0.34 | ring radii | **constant** | glyph | — |
| cursor.hide_after_idle | — | none | no such behaviour | **finding → preference** | niri `hide-after-inactive-ms`; Hyprland `cursor:inactive_timeout` 0 (`:687`); COSMIC `cursor_hide_timeout` None "Hide the cursor after this many seconds of pointer inactivity" (`cosmic-comp-config/src/lib.rs:109-110`); KWin hidecursor `InactivityDuration` | `input.cursor.hide_after_ms` · int · 0 (never) · live |
| cursor.locate | — | none | — | **finding → preference** | GNOME `locate-pointer` "pressing a key will highlight the current pointer location" (`interface…:270-276`); KWin shakecursor; COSMIC `cursor_shake_to_find` true (`lib.rs:111-112`) | `input.cursor.shake_to_find` · bool · false · live — a shell item; recorded |

### 2.2 Pointer, scroll, peripherals (spatial-input §5, §8; ADR 0013 am. 6)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| pointer.gain | built | `pointer.rs:55-56` `DEFAULT_GAIN 1.0`; §14 | 1.0 | **preference** | GNOME `mouse speed` [-1,1] "Pointer speed for mice" (`peripherals.gschema.xml.in:177-182`); Hyprland `input:sensitivity`; Simula `_mouseSensitivityScaler`; wayvr `wvr_mouse_speed` | `input.pointer.gain` · double [0.25,4] · 1.0 · live |
| pointer.accel_profile | implied | `libinput.rs:32-35` flat only | flat + gain | **contested** (§5 Q3) | GNOME `accel-profile` default/flat/adaptive (`:188-207`); Hyprland `accel_profile`; COSMIC `acceleration.profile`; wayvr `wvr_mouse_acceleration` true "adaptive vs constant" — every comparable exposes it; research/63 §8 argues flat because there is no screen | `input.pointer.accel_profile` · enum flat\|adaptive · flat · live (if ruled) |
| pointer.warp | designed | §8, §14 | gaze→head warp | **preference** | niri `warp-mouse-to-focus`; COSMIC `cursor_follows_focus`; Hyprland `cursor:warp_on_*` (`:688-694`) | `input.pointer.warp` · enum gaze\|head\|off · gaze · live |
| pointer.natural_scroll | — | none | — | **finding → preference** | GNOME `natural-scroll` false "enable natural (reverse) scrolling" (`:183-187`); Hyprland `natural_scroll`; COSMIC `scroll_config.natural_scroll`; wayvr `invert_scroll_direction_x/y` | `input.scroll.natural` · bool · false · live |
| pointer.left_handed | — | none | — | **finding → preference** | GNOME `left-handed` "Swap left and right mouse buttons" (`:172-176`); Hyprland `left_handed`; wayvr `left_handed_mouse`; COSMIC `left_handed` | `input.pointer.left_handed` · bool · false · live |
| pointer.wheel_px | built | `pointer.rs:53-54`, `libinput.rs:76-78` 15 px/detent | 15 | **preference** | GNOME touchpad `scroll-speed` [0.01,100] 1.0 "Adjusts the scroll delta" (`:13-20`); Hyprland `scroll_factor` 1 (`:378`); wayvr `scroll_speed` 1.0; xrdesktop `scroll-threshold` 0.1 (`org.xrdesktop.gschema.xml:87`) | `input.scroll.factor` · double [0.1,10] · 1.0 · live (15 px × factor) |
| pointer.double_click | — | none (clients decide) | — | **finding → preference** | GNOME `double-click` 400 ms (`:243-247`); the reserved stage's `DOUBLE_NS` 300 ms is the compositor's own double-press, not the client's | `input.pointer.double_click_ms` · int · 400 · live — exported for clients via the seat's `xdg` conventions; recorded |
| pointer.drag_threshold | — | none | — | **finding → preference** | GNOME `drag-threshold` 8 px "Distance before a drag is started" (`:248-252`) | `input.pointer.drag_threshold_px` · int · 8 · live |
| pointer.middle_emulation | — | none | — | **finding → preference** | GNOME `middle-click-emulation` (`:238-242`); Hyprland `middle_button_emulation` | `input.pointer.middle_emulation` · bool · false · live |
| touchpad.* | — | none (libinput defaults) | — | **finding → preference** | GNOME `tap-to-click` true, `two-finger-scrolling-enabled`, `disable-while-typing` true + timeout 500, `tap-and-drag`, `click-method` (`:13-142`); niri, Hyprland, COSMIC the same set | `input.touchpad.{tap,natural_scroll,dwt,dwt_timeout_ms,click_method}` · libinput's types · libinput's defaults · live |
| pointer.click_freeze | — | none | — | **finding → preference** (XR consensus) | xrdesktop `shake-compensation-enabled/threshold/duration-ms` true/2.0/180 "Compensate involuntary shake while pressing" (`org.xrdesktop.gschema.xml:5-28`); kwin-vr `pointerInhibitDelay` 100 ms (`kwinvr.kcfg:209`); wayvr `click_freeze_time_ms` 300 "Helps with double-click precision" (`config.rs:336`) — three XR shells | `input.pointer.click_freeze_ms` · int · 0 · live — the stabiliser's target lock (§4) is the mechanism; whether it needs a wearer knob is §5 Q4 |
| pointer.stick_deadzone | — | none | — | **finding → calibration/preference** | xrdesktop `analog-threshold`; WiVRn `lh-stick-deadzone`; kwin-vr `Thumbstick` scale | contract `mura.hardware.input.stick.deadzone`; a preference only if a comparable exposes it to users (xrdesktop does) — §5 Q4 |
| pointer.one_logical | designed | §5, ADR 0013 am. 4 | one pointer, last commit owns | **constant** (ruled) | kwin-vr `blockOtherPointerMotion` true (`kwinvr.kcfg:215`) — kwin-vr *exposes* it | — |
| pointer.select_map | implied | `pointer.rs:58-67` | Select→BTN_LEFT etc. | **constant** | evdev | — |
| pointer.gaze_scroll | designed | §5, §9 | enter/axis/leave | **constant** (ruled, privacy) | — | — |
| pointer.gaze_release | designed | §5, research/70 §9.2 | ray-owned pointer leaves under gaze | **constant** (ruled) | — | — |
| pointer.head_claims | implied | research/70 §5 | head claims the pointer before the tier exists | **constant** | xrdesktop default primary controller | — |
| ei.activity | built | `activity.rs:19-28` EI counts as activity | on | **contested** (§5 Q5) | mutter/KWin count emulated input as activity; a headset that also locks on the ladder has a security angle (research/70 §5) | `session.idle.count_emulated_input` · bool · true · live |

### 2.3 Keyboard, text entry, OSK (spatial-input §12; §8)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| kbd.layout | designed | plan §4.5 "xkb from settings"; none coded | locale1/xkb defaults | **finding → preference** | GNOME `input-sources sources`, `xkb-options`, `xkb-model` (`input-sources.gschema.xml.in:17-54`); Hyprland `kb_layout/variant/options` (`:353-359`); niri `xkb` "fetch from locale1 if empty" (`default-config.kdl:10-26`); COSMIC `xkb_config` | `input.keyboard.xkb.{layout,variant,options,model}` · string · from `locale1` · live |
| kbd.repeat | designed | plan §4.5; none coded | smithay defaults | **finding → preference** | GNOME `repeat` true, `delay` 500, `repeat-interval` 30 (`peripherals…:145-159`); Hyprland `repeat_rate` 25 / `repeat_delay` 600; COSMIC 600/25 | `input.keyboard.repeat.{enabled,delay_ms,rate_hz}` · bool/int/int · true/600/25 · live |
| kbd.numlock | — | none | — | **finding → preference** | GNOME `remember-numlock-state`; Hyprland `numlock_by_default`; COSMIC `numlock_state` BootOff | `input.keyboard.numlock` · enum off\|on\|remember · remember · relogin |
| osk.suppress | built | `text.rs:70-71` 5 min | 300 s | **preference** (silently fixed) | StereoKit `platform.cpp:258` five minutes is the only comparable with a number; GNOME/KDE hide the OSK while a keyboard is attached rather than by time | `input.osk.suppress_after_key_s` · int · 300 · live |
| osk.enable | — | none | — | **finding → preference** | GNOME `a11y.applications screen-keyboard-enabled` false (`:4-8`); KWin `Wayland/InputMethod` path (`virtualkeyboardsettings.kcfg:8`) | `input.osk.enabled` · bool · true · live; `input.osk.input_method` · string · the shell's · relogin |
| osk.im_emits | built | `text.rs:73-75` `IM_EMITS_KEYS = false` | seat emits | **constant** | integration switch until smithay exposes `deactivate_input_method` | — |
| text.dictation | designed | §12 Look-to-Dictate | shell item | out of scope here | — | — |

### 2.4 Accessibility (spatial-input §13)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| a11y.dwell.enabled | built | `a11y.rs:21` off; §14 | off | **preference** | GNOME `dwell-click-enabled` false "Enable dwell clicks" (`a11y.mouse.gschema.xml.in:44-48`); KWin dwellclicker effect | `input.dwell.enabled` · bool · false · live |
| a11y.dwell.onset | built | `a11y.rs:39-42` 200 ms | 200 | **preference** | KWin `DelayTime` 150 (`dwellclickerconfig.kcfg:8-11`); HoloLens 150–250 [external] | `input.dwell.onset_ms` · int [50,1000] · 200 · live |
| a11y.dwell.complete | built | `a11y.rs:43-47` 750 ms | 750 | **preference** | GNOME `dwell-time` 1.20 s "Time in seconds before a click is triggered" (`:4-8`); KWin `DwellTime` 500 (`:12-15`) | `input.dwell.complete_ms` · int [200,3000] · 750 · live |
| a11y.dwell.tolerance | built | `a11y.rs:48-55` 2° / 20 px | 2°, 20 px | **preference** | GNOME `dwell-threshold` 10 px "Distance in pixels before movement will be recognized" (`:9-13`); KWin `MotionThreshold` 5 (`:16-19`) — both exposed | `input.dwell.tolerance_deg` · double · 2.0 · live |
| a11y.secondary_click | — | none | — | **finding → preference** | GNOME `secondary-click-enabled` / `secondary-click-time` 1.2 s "simulated secondary click" (`:49-58`) | `input.dwell.secondary_hold_ms` · int · 0 (off) · live |
| a11y.sticky_slow_bounce | designed | §13 placement only; none coded | — | **finding → preference** | GNOME `stickykeys-enable`, `slowkeys-delay` 300, `bouncekeys-delay` 300 (`a11y.keyboard.gschema.xml.in:23-97`); KWin `kaccessrc` (`stickykeys.cpp:71-74`) | `input.a11y.{sticky,slow,bounce}.*` · GNOME's shapes · off · live — when the stages land |
| a11y.mouse_keys | — | none | — | **finding → preference** | GNOME `mousekeys-enable` + speed/accel/delay (`:38-57`); KWin mousekeys plugin | `input.a11y.mouse_keys.*` · GNOME's · off · live — when the stage lands |
| a11y.pin_source | designed | §13, §14 `input.targeting.source` | auto | **preference** | HoloLens/Android XR "use head to aim rather than eyes" [external] | `input.targeting.source` · enum auto\|eyes\|hand\|controller\|head · auto · live |
| a11y.magnetism | designed | §4, §14 | off; 0.07 m reference | **preference** | MRTK3 poke magnetism [external via research/63] | `input.magnetism.enabled` · bool · false · live |
| a11y.reduced_motion | designed | spatial-a11y "cap profile" (unnamed) | — | **finding → preference** | GNOME `a11y.interface reduced-motion` (`:18-24`), `interface enable-animations` (`:11-18`); KWin `AnimationDurationFactor` (`animationskdeglobalssettings.kcfg:8-11`); niri `animations.slowdown`; wayvr `ui_animation_speed` | `ui.animation.speed` · double [0,2] · 1.0 · live (0 = none); `ui.reduced_motion` · bool · false |
| a11y.high_contrast | designed | spatial-a11y theme profile (unnamed) | — | **finding → preference** | GNOME `high-contrast` (`a11y.interface…:4-10`) | `ui.high_contrast` · bool · false · live — a shell item |

### 2.5 Targeting, tiers, gestures — the trackers' numbers (spatial-input §3, §4, §10)

| id | kind | where | now | bucket | comparable | proposed home |
|---|---|---|---|---|---|---|
| tier.order | designed | §3, ADR 0013 am. 1 | gaze → controller (held) → hand → head | **constant** (ruled) + **contested** controller-vs-hand (research/70 §6.1) | Horizon "Gaze > Hand Ray > Head ray" [external]; research/63 §1 | §5 Q6 |
| tier.no_mid_gesture | designed | §3 | never mid-gesture | **constant** (ruled) | MRTK3 sticky hover | — |
| tier.near_band | built | `tier.rs:59-62` 0.18 / 0.22 m | WiVRn | **calibration** | WiVRn `constants.h:45-47` | contract `mura.hardware.input.hand.near_band_m` |
| tier.stale | built | `tier.rs:62` 800 ms | = gaze fallback | **calibration** | none states one | contract |
| gaze.fallback / return | built | `quality.rs:41-44` 800 / 800 ms | 500–1500 band | **calibration** | HoloLens order [external]; blink 100–400 ms physiology | contract `mura.hardware.input.gaze.{fallback_ms,return_ms}` |
| held.timeout / motion / axis | built | `held.rs:45-47` 2 s / 5 mm / 0.01 | invented | **calibration** | WiVRn never puts a controller down; Meta "not in hand" [external] | contract |
| stab.half_lives | built | `stabilize.rs:18-19` 0.01 / 0.05 s | MRTK3 | **calibration** | `LOSAngularOffsetHandRayPoseSource.cs:19-23` | contract |
| stab.relaxation | built | `stabilize.rs:21-22` 0.5 rays / 0.1 gaze | MRTK3 | **calibration** | `MRTKRayInteractor.cs:66`, `GazePinchInteractor.cs:97-102` | contract |
| stab.sticky | built | `stabilize.rs:20` 0.5 | MRTK3 | **calibration** | | contract |
| stab.pinch_closed | built | `stabilize.rs:23-26` 0.9 | MRTK3 | **calibration** | | contract |
| stab.compensation | built | `stabilize.rs:27-28` 50 ms | design §4 | **calibration** | xrdesktop shake compensation 180 ms window (`org.xrdesktop.gschema.xml:28`) is the same idea exposed as a *setting* — noted for Q4 | contract |
| pinch.thresholds (three ladders) | built | `touch.rs:45-48` 0.75/0.25; `loss.rs:77-79` 0.7/0.5; `bridge.rs:49-54` 1.0/1.5 cm, 8 cm | three inconsistent ladders (research/70 §5 says 0.75/0.5 for `loss.rs`, code says 0.7/0.5) | **calibration** + a code inconsistency to fix | WiVRn `constants.h:42-49`; StereoKit `input_hand.cpp:395-403`; MRTK3 | contract `mura.hardware.input.hand.pinch.{close,open}` — one ladder; `input.hand.pinch.*` in §14 "exposed because stand-ins" becomes §5 Q2 |
| poke.depths | built | `loss.rs:77-79` −1 cm / 0 | WiVRn | **calibration** | | contract |
| bridge.palm_cone / hold | built | `bridge.rs:55-58` 35° / 300 ms | none publishes | **calibration** (cone) / **preference** (hold — a deliberate-hold duration; wayvr `long_press_duration` 1.0 s exposed, `config.rs:348`) | | contract cone; `system.gesture.hold_ms` · int · 300 · live |
| bridge.dominant | built | `bridge.rs:46-48` Right | flagged | **preference** | every platform's handedness setting [external]; no desktop analogue | `input.hand.dominant` · enum left\|right · right · live |
| bridge.body_model | built | `bridge.rs:60-63` shoulder/head/neck | Monado `ht_ctrl_emu` | **calibration** | | contract (or the runtime's, when the §10 device lands) |
| hit.class_epsilon | built | `hit.rs:42-45` 2 cm | design §4 | **calibration** | | contract |
| emphasis.ramp / scale | built | `emphasis.rs:18-19` 700 ms; `xr.rs:717` 1+0.15e | HoloLens 500–1000 | **preference** | GNOME/KDE expose animation speed, not hover ramps; visionOS hover effect strength is system [external] — a "feedback intensity" knob | `input.emphasis.{ramp_ms,strength}` · int/double · 700/0.15 · live |
| touch.contacts | built | `touch.rs:41-43` ids 0/1/2 | judgment | **constant** | smithay | — |
| touch.cancel_on_loss | designed | §5 | cancel not up | **constant** | | — |
| output.virtual | implied | `state.rs:377-379` XR-1 1920×1080@60 | stand-in | **calibration** | kwin-vr `width/height/refreshrate/scale` 1440×900@60 (`kwinvr.kcfg:13-25`) — kwin-vr *exposes* these | contract per target; the wearer's knob is `wm.density.px_per_cm` (2.7) |

### 2.6 Reserved system input (native-openxr-apps §6, §9; research/66)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| system.press_map | designed | §6 Q-B ruled | short summon, long recenter, double toggle, chord quit | **constant** (actions) | platforms' split [external] | — |
| system.short_max | built | `reserved.rs:47-50` 400 ms | design ~500 | **preference** | Meta > 500 ms [external]; §9 stand-in `longPressMs` 500 — code 400 vs doc 500 is a discrepancy to resolve at B1 | `system.button.long_press_ms` · int [300,1500] · 500 · live (short = below it) |
| system.long | built | `reserved.rs:51-53` 800 ms | between Meta/PICO | **preference** | | folded into the above (long fires at hold ≥ long_press_ms; 800 vs 500 is §5 Q8) |
| system.double | built | `reserved.rs:54-57` 300 ms | desktop double-click order | **preference** | GNOME `double-click` 400 | `system.button.double_tap_ms` · int · 300 · live |
| system.chord | built | `reserved.rs:58-60` 1 s | comparables chord, not hold | **preference** | Steam Deck / Apple force-quit holds [external] | `system.button.chord_hold_ms` · int · 1000 · live |
| system.double_action | designed | §6, §9 | show/hide or passthrough | **preference** | | `system.button.double_press` · enum show_hide_planes\|passthrough\|none · show_hide_planes · live |
| system.quit_timeout | designed | §3, §9 | OpenVR kill timeout | **preference** | OpenVR [external] | `system.quit_timeout_s` · int · (read at B1) · live |
| system.reserved_first | built | `reserved.rs:26-32` | before a11y (KWin runs a11y first) | **constant** (safety) | KWin `input.h:366-393` | — |
| games.keep_planes | designed | §4, §9 | per window | **preference** | HoloLens Follow-me [external] | `games.keep_planes` · bool · false · live |
| games.controller_system | designed | §9 | best-effort | **preference** | | `games.controller_system_button` · bool · true · live |
| games.cutout_default | designed | §4(b) Q-D **open** | — | **contested** (owner, on device) | Meta hands over games [external] | `games.hand_cutout` · enum on\|off\|while_summoned · (open) · live |

### 2.7 Windows and workspace (window-workspace-management §3–§8, §12)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| wm.spawn.distance | designed/built | §12; `scene.rs:21` 1.5 m | 1.5 | **preference** (seeded by contract) | kwin-vr `distance` 100 cm "Distance from user to display windows" (`kwinvr.kcfg:33`); xrdesktop push/pull; Simula push/pull bindings; Horizon/visionOS 1–2 m [external] | `wm.spawn.distance_m` · double [0.5,3] · contract seed · live |
| wm.spawn.elevation | designed | §12 | 0 | **preference** | Android XR 5° below eye line [external] | `wm.spawn.elevation_deg` · double · seed · live |
| wm.spawn.overlap / sibling_offset | designed/built | §12; `scene.rs:347` 0.9 m / 0.35 rad fan | fan | **preference** | kwin-vr `minTransientNormalSpacing` (`:111`); WayVR spread | `wm.spawn.{overlap,sibling_offset}` · double · seed · live |
| wm.density | designed/built | §12; `scene.rs:19-20` 0.0012 m/px (8.3 px/cm) vs §3 stand-in 20 | **two stand-ins disagree** | **preference** (with a calibration seed) | kwin-vr `ppu` 20 "Pixels per cm" (`:29`); GNOME `text-scaling-factor` / `scaling-factor` (`interface…:121-136`); WiVRn `resolution_scale` — the XR shells all expose the scale | `wm.density_px_per_cm` · double [5,40] · seed · live — §5 Q9 picks the seed |
| wm.size.{min,max,maximized} | designed | §12 | contract | **preference** | Horizon/Android XR ranges [external]; Simula `_defaultWindowResolution/_defaultWindowScale` (`config.dhall:17-20`); wayvr `default_overlay_scale` | `wm.size.*` · seeds · live |
| wm.engine.default | designed | §4, §12 Q2 | `free` | **preference** | | `wm.engine` · string · "free" · relogin |
| wm.external_manager | designed | §4, §12 | empty | **preference** | wm managers as executables (Hyprland/river shape) | `wm.external_manager` · string · "" · relogin |
| wm.minimize | designed | §5, §12 Q1 ruled | dock indicator; else close | **preference** | | `wm.minimize` · enum dock\|close · dock · live |
| wm.follow.* | designed | §7, §12 | never by default; opt-in | **preference** | kwin-vr `followEnabled` true, `followFovH/V` 40/20, `followStopFovH/V` 4/4, `followDelay` 0.5 s, `followSpeed` 2.0 (`kwinvr.kcfg:37-61`) — the whole family exposed; Breezy 15°/1 s [external] | `wm.follow.{default,threshold_deg,delay_ms,rate,stop_deg}` · bool/double/int/double/double · false/40/500/2.0/4 · live |
| wm.follow.billboard | designed | §7 | face head while moved | **preference** | kwin-vr `followWorldUpAlignment` (`:65`); wayvr `snap_angle_deg` | `wm.move.billboard` · bool · true · live |
| wm.focus.dim / sibling_alpha | designed | §8, §12 | — | **preference** | Horizon/visionOS recession [external]; wayvr `default_opacity` | `wm.focus.{dim,sibling_alpha}` · double [0,1] · seed · live |
| wm.child_z_gap | designed | §3 0.5 mm | motorcar 0.05 m | **constant** | kwin-vr `zWindowMarginTop` exposes it (`:87`) — noted, not adopted | — |
| wm.curvature | — | none | flat planes | **finding → preference** (XR consensus) | wayvr `default_curvature` 0.15 (`config.rs:557`); Horizon curved panels [external] | recorded: a §15 item for the wm design, not a key yet |
| wm.no_cap / exclusivity / apps_dont_place | designed | §1.4, §9, §1.1 | ruled | **constant** | | — |

### 2.8 Focus and activation (spatial-input §6; ADR 0013 am. 5)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| focus.mode | implied | `focus.rs:1-12` commit only, never hover | click-to-focus | **contested** (§5 Q10) | GNOME `focus-mode` click\|sloppy\|mouse (`wm.preferences…:41-51`); KWin `FocusPolicy` (`kwinoptions_settings.kcfg:92-100`); Hyprland `follow_mouse`; COSMIC `focus_follows_cursor` — every desktop a knob; research/63 §4 gives the reason it must be commit for *gaze* (Jacob 1990) | if ruled: `wm.focus.mode` · enum commit\|hover_pointer · commit — hover only for pointer-class devices, never gaze |
| focus.new_window | implied | `focus.rs:13-19` | take focus unless commit intervened | **preference** | GNOME `focus-new-windows` smart\|strict (`:52-62`); COSMIC `activation_policy` Focus\|Urgent (`lib.rs:248-254`); Hyprland `misc:focus_on_activate` | `wm.focus.new_windows` · enum smart\|strict · smart · live |
| focus.raise_on_commit | implied | §6 | raise | **preference** | GNOME `raise-on-click` true (`:63-78`); KWin `ClickRaise` | `wm.focus.raise_on_commit` · bool · true · live |
| focus.stealing | implied | `focus.rs:20-32` urgency-only | binary | **constant** (ruled) | KWin `FocusStealingPreventionLevel` 0–4 (`:128-132`) is a continuum — recorded, not adopted | — |
| focus.restore | implied | `focus.rs:33-34` MRU committed | | **constant** | Hyprland `focus_on_close` next\|cursor\|mru exposes it | — |
| focus.manager_serial | implied | `focus.rs:35-37` | | **constant** (ruled) | | — |
| activation.token_timeout | built | `state.rs:1275-1276` 10 s | niri | **constant** | niri `XDG_ACTIVATION_TOKEN_TIMEOUT` | — |

### 2.9 Session, idle, presence, lock (ADR 0007; lib/contract)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| lock.enable | built | `lib/contract/default.nix:586-594` | true | **preference/policy** | GNOME `screensaver lock-enabled` true (`screensaver…:21-25`); kscreenlocker `Autolock` true (`kscreenlockersettings.kcfg:8-12`) | `session.lock.enabled` · bool · true · live — `runtime`; an image may lock it |
| lock.triggers | built | `:596-603` | boot, suspend, explicit | **preference/policy** | kscreenlocker `LockOnResume`/`LockOnStart` (`:33-39`) | `session.lock.triggers` · set · as now · live |
| lock.doff_grace | built | `:605-611` 45 s | ADR ~30–60 | **preference** | kscreenlocker `LockGrace` 5 s (`:23-27`); GNOME `lock-delay` 0 (`:26-30`) | `session.lock.doff_grace_s` · int [0,300] · 45 · live |
| idle.delay | designed | ADR 0007 ladder; research/12 §6.2 | unnamed | **finding → preference** | GNOME `session idle-delay` 300 s "Time before session is considered idle" (`session.gschema.xml.in:4-8`); kscreenlocker `Timeout` 5 min (`:13-17`) | `session.idle.delay_s` · int · 300 · live |
| idle.lock_delay | designed | ADR 0007 idle-past-lock | unnamed | **finding → preference** | GNOME `lock-delay` | `session.idle.lock_delay_s` · int · 0 · live |
| idle.blank_deadline | designed | research/12 ≤ 2 s | | **constant** | power | — |
| docked.lock_on_doff / deep_idle | designed | device-contract `lockOnDoffWhileDocked` false, `deepIdleAfter` | doc-only | **preference** | | `session.docked.{lock_on_doff,deep_idle_after_s}` · bool/int · false/null · live |
| presence.never_unlocks | designed | ADR 0007 | | **constant** (security) | | — |
| idle.protocols | designed | ADR 0007 | ext-idle-notify, idle-inhibit | **constant** | standard | — |
| faillock | built | `:626-638` 5 / 300 s | | **policy** (`immutable` stays) | PAM faillock | — |
| readiness/restart timeouts | built | `:614-623`; `session.nix:100-115` | 30 s; 3/60 s; 1 s; 10 s | **constant** | plasmashell precedent | — |
| guest.* | designed | multi-user §4 | off | **preference/policy** (admin) | GNOME/KDE guest sessions [external] | `session.guest.*` — the multi-user design's, listed |

### 2.10 Passthrough, perception, native apps (perception-passthrough-hands; native-openxr-apps §4)

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| passthrough.latency_mode | built (`mkSetting`) | `lib/contract` | low-latency | **preference** (compiled) | wayvr `use_passthrough`; StereoKit `blend_preference`; kwin-vr `blend` true (`kwinvr.kcfg:116`) | exists: `xr.passthrough.latencyMode` |
| passthrough.upper_limb | built (`mkSetting`) | `lib/contract` | automatic | **preference** (compiled) | visionOS upper-limb visibility [external] | exists: `xr.passthrough.upperLimbVisibility` |
| passthrough.enable / depth_backend / gapfill | built/designed | contract | capability | **calibration/build** | | contract |
| ipd | built (`mkSetting`) | `lib/contract:126-131` | 0.063 | **preference** when stored, else calibration | every headset's IPD setting [external] | exists: `hardware.ipd.meters` |
| quiet.zero_layers / cadence | designed | native §2, §4, §10 | | **constant** (measured) | | — |
| overlay.placement | designed | native §2 | stand-in | **constant** until a second overlay | StereoKit `overlay_priority`; Stardust `--overlay`; kwin-vr `overlayPlacement` 20 | — |

### 2.11 Places, multi-user, updates

| id | kind | where | now | bucket | comparable | proposed key |
|---|---|---|---|---|---|---|
| places.entry | built (template) | `settings.nix:79-83`; places §5 | per-place grant | **preference** (compiled) | | exists: `places.entry:<id>.{enabled,launch}`; `summon` named in the spec, not in the template — §4 |
| places.transient / pin / currency | designed | places §4–§5 | ruled | **constant** | GNOME/COSMIC shapes | — |
| updates.channel / auto_apply | designed | images-and-updates | immutable A/B; no auto-apply key named | **contested** (§5 Q11) | AGENTS.md rule 3: software sources are the user's choice; SteamOS/Android auto-update toggles [external] | `system.update.{channel,auto_apply}` — the images design's to name |
| uid_range / boot_tries | built | contract | | **constant/policy** | | — |

### 2.12 Engineering constants (recorded, not knobs)

zxr: `SLOT_COUNT`, `Slot::ALL`, the `Flags` bits, `SourceKind::ALL`, `KIND_COUNT`, `HISTORY_NS/LEN`,
`CONTACT_COUNT`, the evdev `BTN_*`/`KEY_*` codes (`libinput.rs:64-74`), the OpenXR joint indices and
`REQUIRED_JOINTS`, the action-set name and binding tables, `PRIMARY_STEREO`, `MAX_TEXTURES 512`, the
SPIR-V blobs, `FALLBACK_TICKS 60`, `PANEL_SHRINK_TICKS 60`, poll cadences 5/250 ms, image wait
100 ms, `ACTIVATION_TOKEN_TIMEOUT 10 s`, `IM_EMITS_KEYS`, the qwerty rows in `control.rs`, the
`Scene` flags; the chain order, the class-per-kind table, gaze privacy, cancel-on-loss, one logical
pointer, urgency-only activation, focus restore, quads-always, cutout-last, quiet = zero layers.
Other crates: protocol magics, layer ids, datagram sizes, DRM ioctls (`mura-perception-intake`), bus
names and artifact paths (`mura-settingsd`), recovery menu strings and tool paths, `mura-session`'s
30 s user-manager wait and 100 ms poll. Each has a doc comment naming its source; none has a
wearer-visible meaning.

## 3. Findings — direction 2 (knobs comparables expose; Mura has no decision)

Each is a row above marked "finding"; collected here with who exposes it and why it matters:

1. **Natural scroll** — GNOME, Hyprland, COSMIC, niri, wayvr. Trivial to honour (libinput's own option); a wearer with a Mac habit hits it in the first minute.
2. **Left-handed buttons** — GNOME, Hyprland, COSMIC, wayvr. Same.
3. **Double-click interval / drag threshold** — GNOME 400 ms / 8 px. Mura leaves both to clients; desktops publish them so toolkits agree.
4. **Keyboard repeat delay/rate; XKB layout/variant/options; numlock** — every desktop and every Wayland compositor read. The plan's §4.5 designed "xkb from settings" and nothing carries it.
5. **Touchpad family** — tap, DWT, click method, two-finger vs edge. libinput's, exposed everywhere.
6. **Hide cursor after inactivity** — niri, Hyprland, COSMIC, KWin. Mura hides on key only.
7. **Locate / shake-to-find the pointer** — GNOME, KWin, COSMIC. In XR a lost pointer between planes is *more* likely; a shell item.
8. **Animation speed / reduced motion** — GNOME, KWin, niri, Hyprland, wayvr. Mura's a11y notes name a "cap profile" without a key.
9. **UI / text scale** — GNOME `text-scaling-factor`, every compositor's output scale, WiVRn `resolution_scale`, kwin-vr `ppu`. Mura's `wm.density` is the counterpart and is undeclared with two disagreeing stand-ins (8.3 vs 20 px/cm).
10. **Idle delay and lock delay as named keys** — GNOME 300 s / 0 s, kscreenlocker 5 min. ADR 0007 has the ladder and no numbers.
11. **Click freeze / press stabilisation** — xrdesktop, kwin-vr, wayvr: the one knob *three* XR shells agree on. Mura's stabiliser (§4 target lock, 50 ms compensation) is the mechanism; none of the three lets the wearer tune it *because* it is a mechanism — see §5 Q4.
12. **Stick deadzone** — xrdesktop, WiVRn, kwin-vr. Mura has none; a calibration with an xrdesktop precedent for exposing it.
13. **Secondary click by hold** — GNOME a11y. A dwell-class commit for the right button; falls out of the dwell stage.
14. **Sticky / slow / bounce / mouse keys** — GNOME, KWin. Designed as future stages in §13 with no keys.
15. **OSK enable and IM program** — GNOME, KWin. Mura's OSK is a client the shell chooses; the key names which.
16. **Window curvature** — wayvr, Horizon. Mura's planes are flat by design; recorded for the wm design's §15, not a key.
17. **Focus-stealing continuum** — KWin 0–4. Mura is binary (urgency-only). Recorded; the ruling (§6) stands.

Excluded as concepts absent in XR: multi-monitor layout, panels/docks, wallpaper, titlebar
button layout, VT switching, screen edges, virtual-desktop count, tablet area mapping, pointing
sticks/trackballs' wheel emulation, GNOME's gesture-dwell modes and click-type window.

## 4. Declared but undeclared — keys a design names that no `mkSetting` carries

| namespace | keys | doc | stated mutability (rev 3 words) |
|---|---|---|---|
| `input.*` | `targeting.source`, `dwell.enabled`, `dwell.onset_ms`, `dwell.complete_ms`, `pointer.gain`, `pointer.warp`, `magnetism.enabled`, `hand.pinch.{close,open}`, `cursor.ray`, `cursor.scale` (10) | spatial-input §14 | "preferences"; `cursor.*` explicitly `runtime`, `apply = live` |
| `wm.*` | `spawn.{distance,elevation,overlap,sibling_offset}`, `density.px_per_cm`, `size.{min,max,maximized}`, `engine.default`, `minimize`, `follow.{threshold,delay,rate,stop}`, `focus.{dim,sibling_alpha}`, `external_manager` (17) | window-workspace-management §12 | **declarative by default** |
| `system.*` / `games.*` / `quit.*` | `button.longPressMs`, `doubleTapMs`, `doublePress`, `quit.timeout`, `games.keepPlanes`, `games.controllerSystemButton` (6) | native-openxr-apps §9 | **declarative** |
| `places.entry` | `summon` | settings-schema §1.1 | named in prose; the template has `enabled`, `launch` only |
| device-contract | `lockOnDoffWhileDocked`, `deepIdleAfter` | device-contract.md:285-307 | doc-only; not in `lib/contract` |

Compiled today: `hardware.ipd.meters`, `xr.passthrough.latencyMode`, `xr.passthrough.upperLimbVisibility`,
the `places.entry` template. The settingsd test fixture's `xr.passthrough.enable` (`immutable`)
is a fixture, not an option.

## 5. Owner questions (rule 8 form)

Each: what is decided · why it is a decision · the comparables' positions · the options ·
consequences. Options are the comparables' actual positions.

**Q1 — Mutability of the wm/system/games keys — RULED 2026-09-27: all 23 `mutable`.** *Decided:*
whether any of the 23 keys wm §12 and native-openxr-apps §9 marked `declarative` (rev 3's word
for Nix-only) has a policy reason to refuse the wearer's runtime write. *Comparables:* every wm key
has a user-writable precedent (kwin-vr's follow family, GNOME's focus keys); the press map's
*actions* are ruled constants, its *timings* are exposed by every platform as accessibility
settings [external]. *Options were:* (a) all 23 `mutable`; (b) `quit.timeout` and
`games.controllerSystemButton` stay `immutable` as safety policy. *Ruling:* (a) — "they should be
both": Nix seeds the default, the wearer overrides live, `Reset` returns to Nix. The owner also
ruled the vocabulary: the axis is now `mutability = mutable | immutable` (spec rev 4 §3), since
`runtime` implied runtime-only. wm §12 and native-openxr-apps §9 are updated.

**Q2 — `input.hand.pinch.{close,open}` — RULED 2026-09-27: layered.** *Decided:* whether tracker
thresholds are keys or contract fields. *Comparables:* no desktop exposes a gesture threshold;
MRTK3/StereoKit/WiVRn ship them as constants; xrdesktop exposes `grab-window-threshold` 0.25
(`org.xrdesktop.gschema.xml:79`). *Options were:* (a) contract field only; (b) contract field
*and* a per-user key layered on it, the key's Nix default derived from the contract value so
`Reset` returns to the calibration. *Ruling:* (b) — "not a bad idea". One source of truth for the
number (the contract), one override on top; `mkSetting`'s `default` is computed from
`mura.hardware.*` at eval. **Open sub-question (owner):** apply the layering to all 16
calibrations of §2.5, or only to those with an exposure precedent — pinch/grab threshold
(xrdesktop), stick deadzone (xrdesktop, WiVRn), click-freeze (three shells)? My read: the latter,
so half-lives and body-model lengths do not become a settings UI no platform ships.

**Q3 — Pointer acceleration profile — RULED 2026-09-27: a key, default `flat`.** *Why it was a
decision:* every comparable exposes `accel-profile` (GNOME `:188-207`, Hyprland, COSMIC, wayvr's
adaptive toggle); research/63 §8's reason for flat — no screen to accelerate against — is an
argument about the *default*, not about withholding the knob. libinput's own option passed
through; no code beyond the key.

**Q4 — Click freeze / stabilisation as a wearer knob.** *Decided:* whether the stabiliser's target
lock and 50 ms compensation get a user-facing `click_freeze_ms`. *Comparables:* xrdesktop
(180 ms, on), kwin-vr (100 ms), wayvr (300 ms) all expose it — the XR consensus knob; MRTK3 does
not. *Options:* (a) expose one duration (the three shells); (b) keep it a mechanism tuned per
device (MRTK3). *Consequence:* (a) gives the wearer the knob three shells found necessary; (b)
trusts the contract. My read: (a) with the contract value as default — three precedents with the
same reason ("helps with click precision").

**Q5 — Emulated input as activity.** *Decided:* whether EI input keeps the session awake by
default. *Comparables:* mutter/KWin count it (their spies see the seat); no comparable has a
headset that also locks on the ladder. *Options:* (a) key, default true (desktops); (b) key,
default false (a remote client should not keep a headset unlocked). *Consequence:* (b) breaks a
remote-desktop session that idles; (a) lets a remote client hold the device open. Security
angle: the owner's.

**Q6 — Controller versus hand when both target** (carried from research/70 §6.1). *Options:*
§3's order (controller) or research/63 §1's Transfer line (hand). Unchanged; listed for completeness.

**Q7 — How zxr consumes settings** (§6). *Options:* (a) a session-bus client (mutter, KWin,
xrdesktop; `zbus` `blocking-api`, one extra thread) or (b) the in-process `Engine` over the store
files with an inotify calloop source (cosmic-comp's `ConfigWatchSource`; zero threads, no bus).
*Consequence:* (a) follows spec §6's "the compositor reloads on `Changed`" literally and gets
dconf-style coalescing for free; (b) follows spec §8's "files stay readable without the daemon"
and research/58's cosmic rule, costs no bus connection in the compositor, but must re-derive
coalescing and cannot see a `Changed` that did not change the file (none exist today). Rule 6
favours (b); the spec's letter favours (a). My read: (b), with the spec sentence amended to "on
the store's change".

**Q8 — The reserved short/long boundary.** *Decided:* 400 ms (code) vs 500 ms (§9 stand-in and
Meta) and whether long fires at 800 ms or at the same boundary. *Comparables:* Meta > 500 ms,
PICO 1 s [external]; GNOME `double-click` 400. *Options:* (a) one key `long_press_ms` 500, short
is below it; (b) two keys. *Consequence:* (a) is the platforms' shape; the 400/800 dead band was
a judgment (research/66 §11). My read: (a).

**Q9 — `wm.density` seed.** *Decided:* 8.3 px/cm (`scene.rs`, R0) or 20 px/cm (§3, kwin-vr `ppu`).
*Consequence:* the default legibility of every window; measured at M1 either way. Owner's call
on the seed; the key is a preference regardless.

**Q10 — Focus mode.** *Decided:* whether pointer-class devices may have hover focus as a
preference. *Why:* every desktop exposes it; §6's ruling is about *gaze* ("everywhere you look,
another command is activated", research/63 §4) and holds for gaze regardless. *Options:*
(a) `commit` fixed for all kinds (as ruled); (b) `wm.focus.mode = commit | hover_pointer`, hover
only ever for mouse/controller. *Consequence:* (b) is the desktop's `sloppy` for a mouse on a
plane; gaze and hands unchanged. My read: (a) until a wearer asks — the ruling was recent and the
precedent is desktop, not XR.

**Q11 — Update policy keys.** *Decided:* whether channel and auto-apply are user keys. *Why:*
AGENTS.md rule 3 names software sources as the user's choice; images-and-updates names no key.
*Comparables:* SteamOS (channel, auto-update toggle), Android (auto-update) [external]. This is
the images design's item; listed so it is not lost.

## 6. How zxr consumes settings — the comparables and the two options

**What the daemon is.** `pkgs/mura-settingsd` is a library plus two binaries: `artifact.rs` loads
`/etc/mura/settings-schema.json`, `store.rs` reads the sparse per-(schema, instance) JSON under
`$XDG_CONFIG_HOME/mura/settings/`, `engine.rs` resolves `per-user > default` with provenance
(`lib.rs:29-41`); `bus.rs` puts that behind `org.mura.Settings1` with `Changed` coalesced per
event-loop turn. The zbus pin is `default-features = false, features = ["async-io", "blocking-api"]`
(`Cargo.toml:26-29`); zxr has no async runtime and no bus (`pkgs/zxr/Cargo.toml`).

**How the comparables consume theirs.**
- mutter: `g_settings_new("org.gnome.desktop.interface")` + `"changed"` handlers
  (`references/mutter/src/backends/meta-settings.c:555-563`) — GSettings over dconf's GDBus client; changes arrive as bus signals.
- KWin: `KConfigWatcher::create(sharedConfig)` + `configChanged` (`references/kwin/src/options.cpp:89-90`, `main_wayland.cpp:207`) — KConfig files, change notification over D-Bus (`org.kde.kconfig.notify`).
- xrdesktop: per-key `g_signal_connect(settings, "changed::<key>")` callbacks (`references/xrdesktop/src/xrd-input-synth.c:369-376`) — GSettings, the same bus path.
- cosmic-comp: `cosmic_config::calloop::ConfigWatchSource::new(&config)` inserted into the calloop (`references/cosmic-comp/src/config/mod.rs:176-180`) — inotify on the config files, delivered on the compositor's own loop, no bus.
- niri: a filesystem watcher *thread* polling `stat` every `POLLING_INTERVAL` (`references/niri/src/utils/watcher.rs:54-90`) — the shape rule 6 rejects.

**Option (a) — a bus client in zxr.** zbus blocking `Connection::session()`, `Get`/`List` at start,
a `Changed` signal stream. Cost: zbus's blocking API runs its executor on an internal thread (one
more thread against the spec §12 fence, which is already at 5 on the host — research/70 §3.4),
a D-Bus connection held for the session, ~300 KB of code. Gain: `Changed` exactly as spec §6
words it, coalescing done by the daemon, `GenerationChanged` for free, and locks resolved by
the daemon (though §7 says a consumer resolves locked keys from the artifact itself anyway).

**Option (b) — the engine in-process, files watched.** Link `mura-settingsd` as a library, build
an `Engine` on the artifact and the per-user roots, read every key at start, add one inotify fd
on `$XDG_CONFIG_HOME/mura/settings/` as a calloop `Generic` source (the atomic rename the daemon
does on every write is one `IN_MOVED_TO`), re-resolve the changed store's keys. Cost: inotify fd,
zero threads, no bus; the per-key coalescing is one re-read per rename (already one file per
store). Gain: rule 6's answer; identical resolution to the daemon (same code); works when the
daemon is absent ("files stay readable without the daemon", spec §8). Loses: nothing a key needs
today — `Changed` carries provenance zxr does not act on.

**Locked keys either way:** spec §7 — resolved from the authenticated artifact, which option (b)
does by construction and option (a) must do in addition.

## 7. Determinations (what falls out without an owner question)

- **D1 — the buckets hold for 118 of 123 rows;** the 5 contested rows and the 6 questions that
  are about mutability, seeds or mechanism are §5. Preferences are
  `mutable` per the owner's posture; calibrations move to `mura.hardware.*`; constants keep their
  doc comments and get no key.
- **D2 — the harvest found two code inconsistencies to fix at B3, not decisions:** three pinch
  ladders (`touch.rs` 0.75/0.25, `loss.rs` 0.7/0.5, `bridge.rs` cm) where research/70 §5 records
  two; and the reserved short boundary 400 ms in code against 500 ms in native-openxr-apps §9.
- **D3 — the direction-2 findings are real gaps, not XR exotica:** 14 of 17 are libinput/xkb
  options every Wayland compositor passes through; they cost keys, not mechanisms.
- **D4 — the click-freeze family is the one XR-consensus knob Mura has no key for,** and its
  mechanism already exists (the stabiliser); Q4 decides only whether the wearer sees it.

## 8. Sources

Repo: `pkgs/zxr/src/**`, `lib/contract/default.nix`, `modules/os/{settings,session}.nix`,
`pkgs/mura-settingsd/src/*`, spatial-input §3–§14, window-workspace-management §3–§12,
native-openxr-apps §4–§10, ADR 0007, ADR 0013, specs/settings-schema §1–§8, research/58, /63,
/66, /70. Comparables as cited per row (all `references/<clone>/path:line`; visionOS, Horizon,
HoloLens, Android XR, Meta, PICO, SteamOS, Android [external], mechanism only).
