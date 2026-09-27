# lib/contract/input-calibration.nix — the input module's tracker and display numbers, as
# contract facts on the settings artifact.
#
# Every option here is a device calibration (docs/research/73-user-configurable-decisions.md §2.5,
# bucket test: the value depends on the tracker, display or target, not on the person). They are
# declared `mkSetting { mutability = "immutable"; stratum = "build-fact"; }` so that they reach the
# compositor through the same artifact as the wearer's preferences (specs/settings-schema.md §1:
# build facts appear in the artifact when a consumer needs them) — one channel, no environment
# variables — and appear read-only, `locked`, to any settings UI. Where the owner ruled a wearer
# knob on top (research/73 Q2: pinch threshold, stick deadzone, click-freeze), the preference in
# lib/contract/preferences.nix takes its default from the value here.
#
# Values are the stand-ins zxr's code carried until this file (research/70 §5), each with its
# source; every one is on the first-hardware list — measured on Mura's trackers at M1, not here.
{ lib, ... }:
let
  inherit (lib) mkOption types;
  inherit (import ../settings { inherit lib; }) mkSetting;
  # An immutable build-fact key: `c schema key { type; default; description; range? }`.
  c = schema: key: args:
    mkSetting ((removeAttrs args [ "range" ]) // {
      settings = { inherit schema key; mutability = "immutable"; stratum = "build-fact"; }
        // lib.optionalAttrs (args ? range) { inherit (args) range; };
    });
in
{
  options.mura.hardware.input = {
    hand = {
      pinch = {
        close = c "hardware.input.hand.pinch" "close" {
          type = types.float;
          default = 0.75;
          description = "pinch_ext value (0..1) at which a pinch commits. MRTK3's select threshold (`MRTKHandsAggregatorConfig.cs`), WiVRn `trigger_click_thd` 0.7 (`constants.h:42-49`). One ladder for loss.rs, touch.rs and the bridge (research/73 D2 — the code carried 0.75/0.25, 0.7/0.5 and centimetres in three files).";
        };
        open = c "hardware.input.hand.pinch" "open" {
          type = types.float;
          default = 0.5;
          description = "pinch_ext value below which a pinch releases (hysteresis). MRTK3 + WiVRn composition (research/70 §5).";
        };
        closeM = c "hardware.input.hand.pinch" "close_m" {
          type = types.float;
          default = 0.010;
          description = "Thumb–index tip distance (m) at which the joint bridge reports a closed pinch (StereoKit `input_hand.cpp:395-403` 1.0 cm). Retired when Monado's EXT_hand_interaction device lands (spatial-input §10).";
        };
        openM = c "hardware.input.hand.pinch" "open_m" {
          type = types.float;
          default = 0.015;
          description = "Tip distance (m) above which the bridge's pinch opens (StereoKit 1.5 cm hysteresis).";
        };
        maxM = c "hardware.input.hand.pinch" "max_m" {
          type = types.float;
          default = 0.08;
          description = "Tip distance (m) mapping to pinch value 0 in the bridge (StereoKit `pinch_max` 8 cm).";
        };
      };
      poke = {
        downM = c "hardware.input.hand.poke" "down_m" {
          type = types.float;
          default = -0.01;
          description = "Poke depth along the plane normal (m, negative = behind the plane) that is a touch down (WiVRn fingertip threshold).";
        };
        upM = c "hardware.input.hand.poke" "up_m" {
          type = types.float;
          default = 0.0;
          description = "Poke depth (m) at which a touch releases (hysteresis stand-in).";
        };
      };
    };
    nearBand = {
      enterM = c "hardware.input.near_band" "enter_m" {
        type = types.float;
        default = 0.18;
        description = "Palm-to-plane distance (m) below which direct touch overrides the ray (spatial-input §3; WiVRn `palm_distance_close_thd_lo`, `constants.h:45-47`).";
      };
      leaveM = c "hardware.input.near_band" "leave_m" {
        type = types.float;
        default = 0.22;
        description = "Palm-to-plane distance (m) above which the ray resumes (WiVRn `palm_distance_close_thd_hi`).";
      };
    };
    gaze = {
      fallbackMs = c "hardware.input.gaze" "fallback_ms" {
        type = types.ints.unsigned;
        default = 800;
        description = "Milliseconds gaze may be sub-nominal or lost before the tier falls to the next source (spatial-input §3: 500–1500 band, HoloLens' order; must exceed a blink, 100–400 ms).";
      };
      returnMs = c "hardware.input.gaze" "return_ms" {
        type = types.ints.unsigned;
        default = 800;
        description = "Milliseconds gaze must be continuously nominal before it retakes the tier (return hysteresis; = fallback by symmetry, no comparable states a number).";
      };
    };
    held = {
      timeoutMs = c "hardware.input.held" "timeout_ms" {
        type = types.ints.unsigned;
        default = 2000;
        description = "Milliseconds after its last activity a tracked controller still counts as held (spatial-input §3 item 2; invented, flagged — WiVRn never puts a controller down).";
      };
      motionM = c "hardware.input.held" "motion_m" {
        type = types.float;
        default = 0.005;
        description = "Controller motion (m) that counts as activity for the held heuristic.";
      };
      axis = c "hardware.input.held" "axis" {
        type = types.float;
        default = 0.01;
        description = "Stick deflection that counts as activity for the held heuristic (WiVRn `scroll_value_thd`).";
      };
    };
    stabilize = {
      positionHalfLifeS = c "hardware.input.stabilize" "position_half_life_s" {
        type = types.float;
        default = 0.01;
        description = "Ray position low-pass half-life (s). MRTK3 `LOSAngularOffsetHandRayPoseSource.cs:19-23` stabilizedPositionHalfLife 0.01.";
      };
      directionHalfLifeS = c "hardware.input.stabilize" "direction_half_life_s" {
        type = types.float;
        default = 0.05;
        description = "Ray direction low-pass half-life (s). MRTK3 stabilizedDirectionHalfLife 0.05.";
      };
      sticky = c "hardware.input.stabilize" "sticky" {
        type = types.float;
        default = 0.5;
        description = "Select progress (0..1) above which the hovered target is locked (MRTK3 sticky hover).";
      };
      relaxationRay = c "hardware.input.stabilize" "relaxation_ray" {
        type = types.float;
        default = 0.5;
        description = "Relaxation threshold before a ray may retarget (MRTK3 `MRTKRayInteractor.cs:66` 0.5).";
      };
      relaxationGaze = c "hardware.input.stabilize" "relaxation_gaze" {
        type = types.float;
        default = 0.1;
        description = "Relaxation threshold before gaze may retarget (MRTK3 `GazePinchInteractor.cs:97-102` 0.1).";
      };
      pinchClosed = c "hardware.input.stabilize" "pinch_closed" {
        type = types.float;
        default = 0.9;
        description = "Select progress treated as fully closed for the target lock (MRTK3 UI select threshold).";
      };
      compensationMs = c "hardware.input.stabilize" "compensation_ms" {
        type = types.ints.unsigned;
        default = 50;
        description = "Event-time compensation: the commit applies to the target held this many milliseconds before the commit edge (spatial-input §4; xrdesktop `xrd-input-synth.c:340-362` shake compensation). The wearer's `input.pointer.click_freeze_ms` layers on this.";
      };
    };
    hit = {
      classEpsilonM = c "hardware.input.hit" "class_epsilon_m" {
        type = types.float;
        default = 0.02;
        description = "Depth (m) within which a shell affordance wins the hit over content behind it (spatial-input §4 class-aware hit).";
      };
    };
    palm = {
      coneDeg = c "hardware.input.palm" "cone_deg" {
        type = types.float;
        default = 35.0;
        description = "Half-angle (degrees) of the palm-toward-face cone that gates the system gesture (spatial-input §10; no platform publishes its angle).";
      };
    };
    stick = {
      deadzone = c "hardware.input.stick" "deadzone" {
        type = types.float;
        default = 0.0;
        description = "Thumbstick deadzone (0..1) applied before axis input (xrdesktop `analog-threshold`, WiVRn `lh-stick-deadzone`). The wearer's `input.pointer.stick_deadzone` layers on this.";
      };
    };
  };
}
