# lib/contract/preferences.nix — the wearer's preferences as settings keys.
#
# Every option here is `mkSetting { mutability = "mutable"; }`: Nix owns the *default* (set it in
# the configuration and it is the default), the wearer may override it live through
# `org.mura.Settings1`, the override survives rebuilds, `Reset` returns to Nix, an administrator
# enforces with `mura.settings.locks` (specs/settings-schema.md rev 4 §3, §7 — NixOS
# `users.mutableUsers`, KConfig's `[$i]`). Which values are preferences, and why, is
# docs/research/73-user-configurable-decisions.md (§2, the owner's rulings in §5); each key's
# description names its comparable and its consumer. A key exists exactly once and says who
# reads it: the designed wm keys whose consumer is the not-yet-built policy module are declared
# with that said (window-workspace-management §12 fixes their names); keys whose value the design
# has not given are not declared until it does.
#
# Layered keys (Q2, ruled): a tracker threshold lives in `mura.hardware.input.*` (the contract's
# calibration, lib/contract/input-calibration.nix) and the preference's default *is* that value,
# so `Reset` returns to the calibration and there is one source of truth for the number.
#
# Schema names are the artifact's: `<schema>.<key>` with one store file per schema
# (`input.cursor.json`, `wm.spawn.json`, …).
{ lib, config, ... }:
let
  inherit (lib) mkOption types;
  inherit (import ../settings { inherit lib; }) mkSetting;
  cfg = config.mura;
  cal = cfg.hardware.input;
  # A mutable key: `k schema key { type; default; description; range?; apply? }`.
  k = schema: key: args:
    mkSetting ((removeAttrs args [ "range" "apply" ]) // {
      settings = { inherit schema key; mutability = "mutable"; }
        // lib.optionalAttrs (args ? range) { inherit (args) range; }
        // lib.optionalAttrs (args ? apply) { inherit (args) apply; };
    });
in
{
  options.mura.xr = {

    ## input.* — spatial-input.md §14 (the compositor's input module reads every key) ----------
    input = {
      cursor = {
        theme = k "input.cursor" "theme" {
          type = types.str;
          default = "default";
          description = "Xcursor theme for cursor-shape-v1 names (spatial-input §7). GNOME `org.gnome.desktop.interface cursor-theme`, Plasma `Mouse/cursorTheme`, niri `xcursor-theme`. Consumer: zxr input/theme.rs; `XCURSOR_THEME` is the fallback when the key is absent.";
        };
        size = k "input.cursor" "size" {
          type = types.ints.between 16 96;
          default = 24;
          description = "Nominal Xcursor size in pixels (theme image selection). GNOME `cursor-size` 24, Plasma `cursorSize` 24, wlroots' default 24. Consumer: zxr input/theme.rs.";
        };
        ray = k "input.cursor" "ray" {
          type = types.enum [ "both" "image" "ring" ];
          default = "both";
          description = "What a ray that owns the pointer (controller, head) shows: the ring composited around the client's cursor image (both), the image alone (kwin-vr's look), or the ring alone (MRTK3's). A mouse always shows the image; a non-owning ray always the ring. Owner ruling 2026-09-27 (research/70 §9.2). Consumer: zxr input/cursor.rs.";
        };
        scale = k "input.cursor" "scale" {
          type = types.enum [ "angle" "plane" ];
          default = "angle";
          description = "Cursor scale: a constant visual angle (spatial-input §7's dynamic-scale rule; visionOS, MRTK3) or the plane's pixel scale (kwin-vr's pixels-per-unit — the cursor shrinks with its window). Consumer: zxr input/cursor.rs.";
        };
        angleDeg = k "input.cursor" "angle_deg" {
          type = types.float;
          default = 1.5;
          range = { min = 0.5; max = 4.0; };
          description = "Visual angle (degrees, diameter) the cursor layer's 64 px span subtends at the point's distance when scale = angle. Stand-in 1.5° (HoloLens' >= 2° is a target size; research/70 §5). Consumer: zxr input/cursor.rs.";
        };
        hideWhenTyping = k "input.cursor" "hide_when_typing" {
          type = types.bool;
          default = true;
          description = "Hide the pointer-class cursor while typing; motion shows it again (a ray owner keeps its ring). KWin hidecursor `HideOnTyping` true, niri `hide-when-typing`, Hyprland `cursor:hide_on_key_press` false — a toggle on every desktop; Mura fixed it silently before research/73. Consumer: zxr input/cursor.rs.";
        };
        hideAfterMs = k "input.cursor" "hide_after_ms" {
          type = types.ints.unsigned;
          default = 0;
          description = "Hide the pointer-class cursor after this many milliseconds without pointer motion; 0 = never. niri `hide-after-inactive-ms`, Hyprland `cursor:inactive_timeout` 0, COSMIC `cursor_hide_timeout`, KWin hidecursor `InactivityDuration` (research/73 §3 finding 6). Consumer: zxr input/cursor.rs.";
        };
      };

      pointer = {
        gain = k "input.pointer" "gain" {
          type = types.float;
          default = 1.0;
          range = { min = 0.25; max = 4.0; };
          description = "Compositor gain on the libinput flat profile: logical px per device unit (spatial-input §8). GNOME `mouse speed`, Hyprland `input:sensitivity`, Simula `_mouseSensitivityScaler`, wayvr `wvr_mouse_speed`. Consumer: zxr input/pointer.rs.";
        };
        accelProfile = k "input.pointer" "accel_profile" {
          type = types.enum [ "flat" "adaptive" ];
          default = "flat";
          description = "libinput pointer acceleration profile. Default flat: an XR pointer has no screen to accelerate against (research/63 §8) — an argument about the default, not the knob (GNOME `accel-profile`, Hyprland, COSMIC, wayvr expose it). Owner ruling 2026-09-27 (research/73 Q3). Consumer: zxr input/libinput.rs device config.";
        };
        leftHanded = k "input.pointer" "left_handed" {
          type = types.bool;
          default = false;
          description = "Swap left and right pointer buttons. GNOME `left-handed`, Hyprland `input:left_handed`, COSMIC, wayvr `left_handed_mouse` (research/73 §3 finding 2). Consumer: zxr input/libinput.rs device config.";
        };
        warp = k "input.pointer" "warp" {
          type = types.enum [ "gaze" "head" "off" ];
          default = "gaze";
          description = "When the pointer moves after the look has changed planes, warp it to the looked-at plane: by gaze (degrading to head), by head only, or never (spatial-input §8; ADR 0013 amendment item 6; niri `warp-mouse-to-focus`, COSMIC `cursor_follows_focus`). Consumer: zxr input/pointer.rs.";
        };
        clickFreezeMs = k "input.pointer" "click_freeze_ms" {
          type = types.ints.unsigned;
          default = cal.stabilize.compensationMs;
          defaultText = lib.literalExpression "config.mura.hardware.input.stabilize.compensationMs";
          description = "Milliseconds the commit target is held at gesture onset (the stabiliser's event-time compensation, spatial-input §4). Layered on the contract's calibration (research/73 Q2/Q4): xrdesktop `shake-compensation-duration-ms` 180, kwin-vr `pointerInhibitDelay` 100, wayvr `click_freeze_time_ms` 300 — the one knob three XR shells agree on. Consumer: zxr input/stabilize.rs.";
        };
        stickDeadzone = k "input.pointer" "stick_deadzone" {
          type = types.float;
          default = cal.stick.deadzone;
          defaultText = lib.literalExpression "config.mura.hardware.input.stick.deadzone";
          range = { min = 0.0; max = 0.5; };
          description = "Thumbstick deadzone (0..1) below which axis input is ignored. Layered on the contract (research/73 Q2): xrdesktop `analog-threshold`, WiVRn `lh-stick-deadzone`. Consumer: zxr input/pointer.rs axis planner.";
        };
      };

      scroll = {
        natural = k "input.scroll" "natural" {
          type = types.bool;
          default = false;
          description = "Natural (reversed) scrolling for mice and touchpads. GNOME `natural-scroll` false, Hyprland `natural_scroll`, COSMIC, niri, wayvr `invert_scroll_direction_*` (research/73 §3 finding 1). Consumer: zxr input/libinput.rs device config.";
        };
        factor = k "input.scroll" "factor" {
          type = types.float;
          default = 1.0;
          range = { min = 0.1; max = 10.0; };
          description = "Multiplier on scroll deltas (wheel detent = 15 px x factor; finger and stick scaled alike). GNOME touchpad `scroll-speed` 1.0, Hyprland `scroll_factor` 1, wayvr `scroll_speed` 1.0, xrdesktop `scroll-threshold`. Consumer: zxr input/pointer.rs, input/libinput.rs.";
        };
      };

      touchpad = {
        tap = k "input.touchpad" "tap" {
          type = types.bool;
          default = true;
          description = "Tap-to-click on touchpads. GNOME `tap-to-click` true, Hyprland `tap-to-click`, COSMIC tap enabled, niri `tap`. Consumer: zxr input/libinput.rs device config.";
        };
        disableWhileTyping = k "input.touchpad" "disable_while_typing" {
          type = types.bool;
          default = true;
          description = "Disable the touchpad while typing (libinput DWT). GNOME `disable-while-typing` true, Hyprland, COSMIC, niri `dwt`. Consumer: zxr input/libinput.rs device config.";
        };
        clickMethod = k "input.touchpad" "click_method" {
          type = types.enum [ "default" "button_areas" "clickfinger" ];
          default = "default";
          description = "How a touchpad click is interpreted: libinput's default for the device, button areas, or clickfinger. GNOME `click-method`, Hyprland `clickfinger_behavior`, COSMIC `click_method`. Consumer: zxr input/libinput.rs device config.";
        };
      };

      keyboard = {
        xkb = {
          layout = k "input.keyboard.xkb" "layout" {
            type = types.str;
            default = "";
            description = "XKB layout(s), comma-separated; empty = the system's (`locale1`). GNOME `input-sources`, Hyprland `kb_layout`, niri `xkb` (fetches from locale1 when empty), COSMIC `xkb_config.layout`. Consumer: zxr state.rs seat keyboard (and the virtual keyboard).";
          };
          variant = k "input.keyboard.xkb" "variant" {
            type = types.str;
            default = "";
            description = "XKB variant(s); empty = none. Consumer: zxr state.rs.";
          };
          options = k "input.keyboard.xkb" "options" {
            type = types.str;
            default = "";
            description = "XKB options, comma-separated (e.g. `caps:escape`). GNOME `xkb-options`, Hyprland `kb_options`. Consumer: zxr state.rs.";
          };
          model = k "input.keyboard.xkb" "model" {
            type = types.str;
            default = "";
            description = "XKB model; empty = pc105. GNOME `xkb-model` pc105+inet. Consumer: zxr state.rs.";
          };
        };
        repeat = {
          delayMs = k "input.keyboard.repeat" "delay_ms" {
            type = types.ints.between 100 2000;
            default = 600;
            description = "Key repeat delay in milliseconds. GNOME `delay` 500, Hyprland `repeat_delay` 600, COSMIC 600; zxr's stand-in was 200 (research/73 §3 finding 4). Consumer: zxr state.rs seat keyboard.";
          };
          rateHz = k "input.keyboard.repeat" "rate_hz" {
            type = types.ints.between 1 100;
            default = 25;
            description = "Key repeat rate in keys per second. GNOME `repeat-interval` 30 ms (33/s), Hyprland `repeat_rate` 25, COSMIC 25. Consumer: zxr state.rs seat keyboard.";
          };
        };
        numlock = k "input.keyboard" "numlock" {
          type = types.enum [ "off" "on" "remember" ];
          default = "remember";
          description = "NumLock at session start. GNOME `remember-numlock-state`, Hyprland `numlock_by_default`, COSMIC `numlock_state`. Consumer: zxr state.rs (state class: the remembered value lives under XDG_STATE_HOME per settings-schema §2).";
          apply = "relogin";
        };
      };

      osk = {
        enabled = k "input.osk" "enabled" {
          type = types.bool;
          default = true;
          description = "Whether text-input activation summons the on-screen keyboard (spatial-input §12). GNOME `screen-keyboard-enabled`, KWin `Wayland/InputMethod`. Consumer: zxr input/text.rs.";
        };
        suppressAfterKeyS = k "input.osk" "suppress_after_key_s" {
          type = types.ints.unsigned;
          default = 300;
          description = "Seconds a physical key press suppresses the on-screen keyboard. StereoKit's five minutes (`platform.cpp:258`) is the only comparable with a number; GNOME/KDE hide by attachment instead. Consumer: zxr input/text.rs.";
        };
      };

      dwell = {
        enabled = k "input.dwell" "enabled" {
          type = types.bool;
          default = false;
          description = "Dwell as a commit method on any targeting tier (spatial-input §13). GNOME `dwell-click-enabled` false, KWin dwellclicker. Consumer: zxr input/a11y.rs.";
        };
        onsetMs = k "input.dwell" "onset_ms" {
          type = types.ints.between 50 1000;
          default = 200;
          description = "Milliseconds the target must be held before the dwell animation starts. KWin `DelayTime` 150; HoloLens 150–250 (research/42 §5). Consumer: zxr input/a11y.rs.";
        };
        completeMs = k "input.dwell" "complete_ms" {
          type = types.ints.between 200 3000;
          default = 750;
          description = "Milliseconds from onset to the dwell commit. GNOME `dwell-time` 1.2 s, KWin `DwellTime` 500; HoloLens 650–850. Consumer: zxr input/a11y.rs.";
        };
        toleranceDeg = k "input.dwell" "tolerance_deg" {
          type = types.float;
          default = 2.0;
          range = { min = 0.5; max = 10.0; };
          description = "Angular motion (degrees) that resets a dwell in progress. GNOME `dwell-threshold` 10 px, KWin `MotionThreshold` 5 (both exposed). Consumer: zxr input/a11y.rs.";
        };
      };

      targeting = {
        source = k "input.targeting" "source" {
          type = types.enum [ "auto" "eyes" "hand" "controller" "head" ];
          default = "auto";
          description = "Pin the targeting source below the hardware's best (spatial-input §13, §14): auto follows the tier rule (§3). HoloLens/Android XR 'use head to aim rather than eyes'. Consumer: zxr input/tier.rs.";
        };
      };

      magnetism = {
        enabled = k "input.magnetism" "enabled" {
          type = types.bool;
          default = false;
          description = "Poke magnetism toward the nearest target (MRTK3's 0.07 m reference; spatial-input §4, off by default). Consumer: zxr input/hit.rs.";
        };
      };

      hand = {
        dominant = k "input.hand" "dominant" {
          type = types.enum [ "left" "right" ];
          default = "right";
          description = "The wearer's dominant hand (the system gesture's DOMINANT flag, spatial-input §10). Every XR platform's handedness setting. Consumer: zxr input/bridge.rs.";
        };
        pinch = {
          close = k "input.hand.pinch" "close" {
            type = types.float;
            default = cal.hand.pinch.close;
            defaultText = lib.literalExpression "config.mura.hardware.input.hand.pinch.close";
            range = { min = 0.0; max = 1.0; };
            description = "pinch_ext value at which a pinch commits (0..1). Layered on the contract's calibration (research/73 Q2, ruled): xrdesktop exposes `grab-window-threshold` 0.25. Consumer: zxr input gesture config (loss.rs, touch.rs, bridge.rs — one ladder).";
          };
          open = k "input.hand.pinch" "open" {
            type = types.float;
            default = cal.hand.pinch.open;
            defaultText = lib.literalExpression "config.mura.hardware.input.hand.pinch.open";
            range = { min = 0.0; max = 1.0; };
            description = "pinch_ext value below which a pinch releases (hysteresis). Layered on the contract's calibration. Consumer: zxr input gesture config.";
          };
        };
      };

      body = {
        shoulderHalfM = k "input.body" "shoulder_half_m" {
          type = types.float;
          default = 0.155;
          range = { min = 0.10; max = 0.30; };
          description = "Half shoulder width in metres for the hand-ray origin (spatial-input §10 bridge). The wearer's body, not the tracker's — reclassified by the owner 2026-09-27 (research/73 Q2); default is Monado ht_ctrl_emu's average ((39/2 - 4) cm). Consumer: zxr input/bridge.rs.";
        };
        headLenM = k "input.body" "head_len_m" {
          type = types.float;
          default = 0.10;
          range = { min = 0.05; max = 0.20; };
          description = "Head length in metres for the shoulder estimate (Monado ht_ctrl_emu average). Consumer: zxr input/bridge.rs.";
        };
        neckLenM = k "input.body" "neck_len_m" {
          type = types.float;
          default = 0.07;
          range = { min = 0.03; max = 0.15; };
          description = "Neck length in metres for the shoulder estimate (Monado ht_ctrl_emu average). Consumer: zxr input/bridge.rs.";
        };
      };

      emphasis = {
        rampMs = k "input.emphasis" "ramp_ms" {
          type = types.ints.between 0 3000;
          default = 700;
          description = "Touch-class hover emphasis ramp in milliseconds (spatial-input §4; HoloLens 500–1000). 0 = instant (also the effect of ui.reduced_motion). Consumer: zxr input/emphasis.rs.";
        };
        strength = k "input.emphasis" "strength" {
          type = types.float;
          default = 0.15;
          range = { min = 0.0; max = 1.0; };
          description = "Colour-scale strength of the emphasised plane (1 + strength x level, XR_KHR_composition_layer_color_scale_bias). visionOS's hover effect is system-strength; a feedback-intensity knob. Consumer: zxr xr.rs quad submission.";
        };
      };
    };

    ## system.* / games.* — native-openxr-apps.md §6, §9 ------------------------------------
    system = {
      button = {
        longPressMs = k "system.button" "long_press_ms" {
          type = types.ints.between 300 1500;
          default = 500;
          description = "Reserved system control: a press shorter than this is short (summon/dismiss the shell), held this long it is long (recenter). Meta > 500 ms, PICO 1 s [external]; native-openxr-apps §9's stand-in 500 — zxr's code used a 400/800 ms dead band (research/73 Q8: retired, say so if wanted back). Consumer: zxr input/reserved.rs.";
        };
        doubleTapMs = k "system.button" "double_tap_ms" {
          type = types.ints.between 100 800;
          default = 300;
          description = "Two short presses within this window are a double press. GNOME `double-click` 400 for pointers; research/66 §11. Consumer: zxr input/reserved.rs.";
        };
        chordHoldMs = k "system.button" "chord_hold_ms" {
          type = types.ints.between 500 3000;
          default = 1000;
          description = "system + select held this long is the force-quit chord (native-openxr-apps §6; Steam Deck / Apple force-quit holds [external]). Consumer: zxr input/reserved.rs.";
        };
        doublePress = k "system.button" "double_press" {
          type = types.enum [ "show_hide_planes" "passthrough" "none" ];
          default = "show_hide_planes";
          description = "What a double press does (native-openxr-apps §6, §9). Consumer: zxr input/reserved.rs.";
        };
      };
      gesture = {
        holdMs = k "system.gesture" "hold_ms" {
          type = types.ints.between 100 2000;
          default = 300;
          description = "How long the posture-gated palm pinch must be held to become the system gesture (native-openxr-apps §6 Q-C; wayvr `long_press_duration` 1.0 s). Consumer: zxr input/bridge.rs.";
        };
      };
      quitTimeoutS = k "system" "quit_timeout_s" {
        type = types.ints.between 1 60;
        default = 5;
        description = "Seconds after the quit request before a native app's scope is killed (native-openxr-apps §3.4; OpenVR's kill timeout shape). Stand-in 5 s — read OpenVR's value at first use. Consumer: zxr main.rs quit path.";
      };
    };

    games = {
      keepPlanes = k "games" "keep_planes" {
        type = types.bool;
        default = false;
        description = "Keep 2D planes visible while a native OpenXR app is primary, by default (native-openxr-apps §4 Q-D; per-window keep is the window's own state; HoloLens Follow-me). Consumer: zxr main.rs quiet/summon logic.";
      };
      controllerSystemButton = k "games" "controller_system_button" {
        type = types.bool;
        default = true;
        description = "Treat a controller's system/home button as the reserved control while a native app is primary (best-effort until the runtime reserves it; XR_MNDX_system_buttons exposes, zxr reserves). Consumer: zxr input/reserved.rs.";
      };
    };

    ## wm.* — window-workspace-management.md §12 ------------------------------------------
    wm = {
      spawn = {
        distanceM = k "wm.spawn" "distance_m" {
          type = types.float;
          default = 1.5;
          range = { min = 0.5; max = 3.0; };
          description = "Distance from the head at which a new window spawns (window-workspace-management §3). kwin-vr `distance` 100 cm, Horizon/visionOS 1–2 m [external]; R0 stand-in 1.5 m. Seed: the device contract's placement defaults when a target declares them. Consumer: zxr scene.rs placement.";
        };
        elevationDeg = k "wm.spawn" "elevation_deg" {
          type = types.float;
          default = 0.0;
          range = { min = -30.0; max = 30.0; };
          description = "Elevation of a new window relative to the eye line (degrees; negative = below). Android XR ~5° below [external]. Consumer: zxr scene.rs placement.";
        };
        siblingOffsetM = k "wm.spawn" "sibling_offset_m" {
          type = types.float;
          default = 0.9;
          range = { min = 0.2; max = 2.0; };
          description = "Lateral offset between sibling windows in the fan (R0 stand-in 0.9 m; kwin-vr `minTransientNormalSpacing`, WayVR spread). Consumer: zxr scene.rs placement.";
        };
        siblingYawRad = k "wm.spawn" "sibling_yaw_rad" {
          type = types.float;
          default = 0.35;
          range = { min = 0.0; max = 1.0; };
          description = "Yaw between sibling windows in the fan (R0 stand-in 0.35 rad). Consumer: zxr scene.rs placement.";
        };
      };
      densityPxPerCm = k "wm" "density_px_per_cm" {
        type = types.float;
        default = 8.3;
        range = { min = 5.0; max = 40.0; };
        description = "Logical pixels per centimetre of plane (window-workspace-management §3; GNOME `text-scaling-factor`, kwin-vr `ppu` 20, WiVRn `resolution_scale`). The code's R0 stand-in is 8.3 (1.2 mm/px); §3 named 20 — research/73 Q9, the owner's seed; a one-number flip. Consumer: zxr scene.rs scale.";
      };
      focus = {
        newWindows = k "wm.focus" "new_windows" {
          type = types.enum [ "smart" "strict" ];
          default = "smart";
          description = "New windows take focus unless a user commit intervened since the request (smart — mutter's intervening-user-event, niri Smart) or never take it (strict). GNOME `focus-new-windows`, COSMIC `activation_policy`. Consumer: zxr input/focus.rs.";
        };
        raiseOnCommit = k "wm.focus" "raise_on_commit" {
          type = types.bool;
          default = true;
          description = "A commit (touch down / button press) on a window raises it within its place (spatial-input §6). GNOME `raise-on-click`, KWin `ClickRaise`. Consumer: zxr input/focus.rs.";
        };
      };

      # The designed keys of window-workspace-management §12 whose consumer is the wm policy
      # module (§4 `free` in-process, external managers; zxr-architecture §6 `policy`) — not yet
      # built. Declared now so the schema and the design's names are fixed; each says so. Keys
      # whose *value* the design has not given (wm.size.*, wm.focus.{dim,sibling_alpha},
      # wm.spawn.overlap: "seed from the contract") are not declared until it does.
      engine = k "wm" "engine" {
        type = types.str;
        default = "free";
        apply = "relogin";
        description = "The default placement engine (window-workspace-management §4, §12 Q2 ruled: the compositor carries `free` only; arc/dock/band ship as external managers). Consumer: the wm policy module when it lands (declared ahead of it).";
      };
      externalManager = k "wm" "external_manager" {
        type = types.str;
        default = "";
        apply = "relogin";
        description = "Executable of an external window manager (window-workspace-management §4, §11; Hyprland/river shape); empty = the in-process default. Consumer: the session's manager launcher when it lands (declared ahead of it).";
      };
      minimize = k "wm" "minimize" {
        type = types.enum [ "dock" "close" ];
        default = "dock";
        description = "What minimize does (window-workspace-management §5, §12 Q1 ruled): park on the dock client with an indicator, degrading to close when no dock runs; or always close. Consumer: the wm policy module when it lands (declared ahead of it).";
      };
      follow = {
        default = k "wm.follow" "default" {
          type = types.bool;
          default = false;
          description = "Whether new windows follow the head by default (window-workspace-management §7, Q6 ruled: never by default, opt-in per window). kwin-vr `followEnabled` true (`kwinvr.kcfg:37-41`) is the dissent, not adopted. Consumer: the wm policy module when it lands (declared ahead of it).";
        };
        thresholdDeg = k "wm.follow" "threshold_deg" {
          type = types.float;
          default = 40.0;
          range = { min = 5.0; max = 90.0; };
          description = "Head yaw from a following window before it starts to move (kwin-vr `followFovH` 40, `kwinvr.kcfg:43-45`; Breezy 15° [external]). Consumer: the wm policy module when it lands (declared ahead of it).";
        };
        delayMs = k "wm.follow" "delay_ms" {
          type = types.ints.unsigned;
          default = 500;
          range = { min = 0; max = 5000; };
          description = "Dwell past the threshold before a following window moves (kwin-vr `followDelay` 0.5 s, `kwinvr.kcfg:55-57`; Breezy 1 s [external]). Consumer: the wm policy module when it lands (declared ahead of it).";
        };
        rate = k "wm.follow" "rate" {
          type = types.float;
          default = 2.0;
          range = { min = 0.1; max = 10.0; };
          description = "Speed of the follow motion (kwin-vr `followSpeed` 2.0, `kwinvr.kcfg:59-61`). Consumer: the wm policy module when it lands (declared ahead of it).";
        };
        stopDeg = k "wm.follow" "stop_deg" {
          type = types.float;
          default = 4.0;
          range = { min = 0.0; max = 45.0; };
          description = "Yaw within which the follow motion stops (kwin-vr `followStopFovH` 4, `kwinvr.kcfg:49-51`). Consumer: the wm policy module when it lands (declared ahead of it).";
        };
      };
      move = {
        billboard = k "wm.move" "billboard" {
          type = types.bool;
          default = true;
          description = "A window being moved faces the head (window-workspace-management §7; kwin-vr `followWorldUpAlignment` is the world-up variant, `kwinvr.kcfg:63-65`; wayvr `snap_angle_deg`). Consumer: the wm policy module when it lands (declared ahead of it).";
        };
      };
    };

    ## ui.* -------------------------------------------------------------------------------
    ui = {
      reducedMotion = k "ui" "reduced_motion" {
        type = types.bool;
        default = false;
        description = "Reduce motion: emphasis ramps become instant and shell animations are disabled where the shell honours it. GNOME `a11y.interface reduced-motion`, KWin `AnimationDurationFactor`, niri `animations.off`. Consumer: zxr input/emphasis.rs (the shell's components read it too).";
      };
    };

    ## session.* — ADR 0007 idle and grace --------------------------------------------------
    # `session.lock.{enabled,doff_grace_s}` are the existing contract options
    # `mura.xr.session.lock.{enable,doffGraceSeconds}` annotated in lib/contract/default.nix; the
    # trigger set is exposed as booleans because the artifact's type vocabulary has no list
    # (settings-schema §1) — GNOME's shape (`lock-enabled`, `lock-on-suspend`).
    session = {
      idle = {
        delayS = k "session.idle" "delay_s" {
          type = types.ints.unsigned;
          default = 300;
          description = "Seconds without activity before the session is idle (the ladder's first step, ADR 0007; research/12 §6.2). GNOME `session idle-delay` 300, kscreenlocker `Timeout` 5 min. 0 = never. Consumer: the compositor's lock machine (zxr input/mode.rs, activity.rs) when built.";
        };
        lockDelayS = k "session.idle" "lock_delay_s" {
          type = types.ints.unsigned;
          default = 0;
          description = "Seconds after idle before the session locks (when `idle` is a lock trigger). GNOME `lock-delay` 0. Consumer: the compositor's lock machine when built.";
        };
        countEmulatedInput = k "session.idle" "count_emulated_input" {
          type = types.bool;
          default = true;
          description = "Whether emulated (EI / remote) input counts as activity for the idle ladder. mutter/KWin count it (research/70 §5); a remote client can then keep a headset unlocked — research/73 Q5, the owner's security call; default follows the desktops. Consumer: zxr input/activity.rs.";
        };
      };
      lock = {
        onDoff = k "session.lock" "on_doff" {
          type = types.bool;
          default = lib.elem "doff" cfg.xr.session.lock.triggers;
          defaultText = lib.literalExpression ''lib.elem "doff" config.mura.xr.session.lock.triggers'';
          description = "Lock when the headset is taken off (after the grace window). Derived from `mura.xr.session.lock.triggers`. Consumer: the compositor's lock machine.";
        };
        onIdle = k "session.lock" "on_idle" {
          type = types.bool;
          default = lib.elem "idle" cfg.xr.session.lock.triggers;
          defaultText = lib.literalExpression ''lib.elem "idle" config.mura.xr.session.lock.triggers'';
          description = "Lock when the session has been idle past `session.idle.lock_delay_s`. Consumer: the compositor's lock machine.";
        };
        onSuspend = k "session.lock" "on_suspend" {
          type = types.bool;
          default = lib.elem "suspend" cfg.xr.session.lock.triggers;
          defaultText = lib.literalExpression ''lib.elem "suspend" config.mura.xr.session.lock.triggers'';
          description = "Lock on suspend/resume (kscreenlocker `LockOnResume`). Consumer: the compositor's lock machine.";
        };
      };
      docked = {
        lockOnDoff = k "session.docked" "lock_on_doff" {
          type = types.bool;
          default = false;
          description = "While docked (ADR 0015), lock on doff instead of entering quiescence only (device-contract.md `lockOnDoffWhileDocked`). Consumer: the compositor's lock machine.";
        };
        deepIdleAfterS = k "session.docked" "deep_idle_after_s" {
          type = types.ints.unsigned;
          default = 0;
          description = "Seconds of docked quiescence before deep idle stops Monado and the perception units; 0 = never (device-contract.md `deepIdleAfter`). Consumer: the session's quiescence ladder.";
        };
      };
    };
  };
}
