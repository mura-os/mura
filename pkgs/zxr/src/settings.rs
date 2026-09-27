//! The compositor's settings consumer (specs/settings-schema.md rev 4; research/73 §6, option b
//! — ruled 2026-09-27).
//!
//! **What it is.** `org.mura.Settings1`'s resolution, in-process: the daemon's own library
//! (`mura-settingsd`: `artifact`, `store`, `engine`, without `bus`) loads the compiled schema
//! artifact and the wearer's sparse per-schema store files, and resolves every key zxr reads
//! (`per-user > default`; a `locked` or `immutable` key is the artifact's value, spec §7). One
//! inotify fd on the store directory is a calloop source on the state loop: the daemon writes
//! each store with `fsync` + `rename` (`store.rs:64`), so a change is one `IN_MOVED_TO`, and
//! the changed store is forgotten and re-read. No bus connection, no thread, no polling.
//!
//! **Why this shape** (research/73 §6). mutter, KWin and xrdesktop hold a session-bus client
//! (`meta-settings.c:555-563`, `options.cpp:89-90`, `xrd-input-synth.c:369-376`); cosmic-comp
//! watches its config files from its own calloop (`config/mod.rs:176-180`); niri polls from a
//! thread (`utils/watcher.rs:54-90`). Rule 6 picks the file watch: a zbus blocking client costs
//! an executor thread against spec §12's thread fence, and spec §8's "files stay readable
//! without the daemon" (cosmic's rule, research/58) makes the store the contract, not the bus.
//! Spec §6's "the compositor reloads on `Changed`" reads, for this consumer, "on the store's
//! change" — the same event, seen at the file.
//!
//! **Where the values go.** [`Prefs`] is the typed picture of every key zxr owns (the schemas
//! `input.*`, `wm.*`, `system.*`, `games.*`, `session.*`, `ui.*`, and the immutable calibrations
//! `hardware.input.*`, lib/contract/{preferences,input-calibration}.nix). Its defaults are the
//! stand-ins the code carried before the keys existed, so zxr without an artifact (a dev host
//! without `MURA_SETTINGS_SCHEMA`) behaves exactly as before. [`apply`] stores the picture on
//! `Zxr` and bumps `Prefs::generation`; stages that keep their own `*Cfg` re-derive it when the
//! generation they last saw differs — one integer compare per tick, no allocation.
//!
//! Budget: the engine (~94 keys) and the store files are read once at start and once per
//! change; serde_json rides in from the daemon's closure; one fd.

use std::collections::HashMap;
use std::os::fd::{FromRawFd, OwnedFd};

use mura_settingsd::engine::{Engine, Mode};
use serde_json::Value;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode as CMode, PostAction};

use crate::state::Zxr;

/// The input module's tracker and display calibrations (`hardware.input.*`, immutable build
/// facts). Defaults are the code's former constants (research/70 §5).
#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub pinch_close: f32,
    pub pinch_open: f32,
    pub pinch_close_m: f32,
    pub pinch_open_m: f32,
    pub pinch_max_m: f32,
    pub poke_down_m: f32,
    pub poke_up_m: f32,
    pub near_enter_m: f32,
    pub near_leave_m: f32,
    pub gaze_fallback_ms: u64,
    pub gaze_return_ms: u64,
    pub held_timeout_ms: u64,
    pub held_motion_m: f32,
    pub held_axis: f32,
    pub stab_position_half_life_s: f32,
    pub stab_direction_half_life_s: f32,
    pub stab_sticky: f32,
    pub stab_relaxation_ray: f32,
    pub stab_relaxation_gaze: f32,
    pub stab_pinch_closed: f32,
    pub stab_compensation_ms: u64,
    pub hit_class_epsilon_m: f32,
    pub palm_cone_deg: f32,
    pub stick_deadzone: f32,
    /// `hardware.input.comfort.*`: the placement limits (wm §4a, §11) — the compositor's, never delegated
    pub comfort_min_distance_m: f32,
    pub comfort_max_distance_m: f32,
    pub comfort_max_angular_deg: f32,
    /// `hardware.input.hmd.*`: the HMD-body buttons `role=KEY,…` and the select / back / system
    /// roles (device-contract `hmdButtons`, native-openxr-apps §6); empty = none declared
    pub hmd_buttons: String,
    pub hmd_select_role: String,
    pub hmd_back_role: String,
    pub hmd_system_role: String,
}

impl Default for Calibration {
    fn default() -> Self {
        Calibration {
            pinch_close: 0.75,
            pinch_open: 0.5,
            pinch_close_m: 0.010,
            pinch_open_m: 0.015,
            pinch_max_m: 0.08,
            poke_down_m: -0.01,
            poke_up_m: 0.0,
            near_enter_m: 0.18,
            near_leave_m: 0.22,
            gaze_fallback_ms: 800,
            gaze_return_ms: 800,
            held_timeout_ms: 2000,
            held_motion_m: 0.005,
            held_axis: 0.01,
            stab_position_half_life_s: 0.01,
            stab_direction_half_life_s: 0.05,
            stab_sticky: 0.5,
            stab_relaxation_ray: 0.5,
            stab_relaxation_gaze: 0.1,
            stab_pinch_closed: 0.9,
            stab_compensation_ms: 50,
            hit_class_epsilon_m: 0.02,
            palm_cone_deg: 35.0,
            stick_deadzone: 0.0,
            comfort_min_distance_m: 0.4,
            comfort_max_distance_m: 5.0,
            comfort_max_angular_deg: 90.0,
            hmd_buttons: String::new(),
            hmd_select_role: String::new(),
            hmd_back_role: String::new(),
            hmd_system_role: String::new(),
        }
    }
}

/// Every key zxr reads, typed. Enum keys are kept as their artifact string and parsed by the
/// consumer (`cursor::RayCursor::parse`, …) so an unknown value degrades to the default there.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// bumped by every `apply`; stages compare it to re-derive their `*Cfg`
    pub generation: u64,
    /// the artifact's generation string (the closure), `""` without an artifact
    pub artifact_generation: String,
    // input.cursor
    pub cursor_theme: String,
    pub cursor_size: u32,
    pub cursor_ray: String,
    pub cursor_scale: String,
    pub cursor_angle_deg: f32,
    pub cursor_hide_when_typing: bool,
    pub cursor_hide_after_ms: u64,
    // input.pointer / input.scroll / input.touchpad
    pub pointer_gain: f64,
    pub pointer_accel_profile: String,
    pub pointer_left_handed: bool,
    pub pointer_warp: String,
    pub pointer_click_freeze_ms: u64,
    pub pointer_stick_deadzone: f32,
    pub scroll_natural: bool,
    pub scroll_factor: f64,
    pub touchpad_tap: bool,
    pub touchpad_dwt: bool,
    pub touchpad_click_method: String,
    // input.keyboard
    pub xkb_layout: String,
    pub xkb_variant: String,
    pub xkb_options: String,
    pub xkb_model: String,
    pub repeat_delay_ms: u32,
    pub repeat_rate_hz: u32,
    pub numlock: String,
    // input.osk
    pub osk_enabled: bool,
    pub osk_suppress_after_key_s: u64,
    // input.dwell
    pub dwell_enabled: bool,
    pub dwell_onset_ms: u64,
    pub dwell_complete_ms: u64,
    pub dwell_tolerance_deg: f32,
    // input.targeting / input.magnetism / input.hand / input.body / input.emphasis
    pub targeting_source: String,
    pub magnetism_enabled: bool,
    pub hand_dominant: String,
    pub hand_pinch_close: f32,
    pub hand_pinch_open: f32,
    pub body_shoulder_half_m: f32,
    pub body_head_len_m: f32,
    pub body_neck_len_m: f32,
    pub emphasis_ramp_ms: u64,
    pub emphasis_strength: f32,
    // system.* / games.*
    pub system_long_press_ms: u64,
    pub system_double_tap_ms: u64,
    pub system_chord_hold_ms: u64,
    pub system_double_press: String,
    pub system_gesture_hold_ms: u64,
    pub system_quit_timeout_s: u64,
    pub games_keep_planes: bool,
    pub games_controller_system_button: bool,
    // wm.*
    pub wm_spawn_distance_m: f32,
    pub wm_spawn_elevation_deg: f32,
    pub wm_spawn_sibling_offset_m: f32,
    pub wm_spawn_sibling_yaw_rad: f32,
    pub wm_density_px_per_cm: f32,
    pub wm_focus_new_windows: String,
    pub wm_focus_raise_on_commit: bool,
    /// `wm.grab.*`, `wm.move.billboard` (research/76; input/grabs.rs)
    pub wm_grab_depth_rate: f32,
    pub wm_grab_bar_deg: f32,
    pub wm_move_billboard: bool,
    /// `wm.engine`, `wm.minimize`, `wm.follow.*` (the policy module)
    pub wm_engine: String,
    pub wm_minimize: String,
    pub wm_follow_default: bool,
    pub wm_follow_threshold_deg: f32,
    pub wm_follow_delay_ms: u64,
    pub wm_follow_rate: f32,
    pub wm_follow_stop_deg: f32,
    // ui.*
    pub ui_reduced_motion: bool,
    // session.*
    pub session_idle_delay_s: u64,
    pub session_idle_lock_delay_s: u64,
    pub session_idle_count_emulated_input: bool,
    pub session_lock_enabled: bool,
    pub session_lock_doff_grace_s: u64,
    pub session_lock_on_doff: bool,
    pub session_lock_on_idle: bool,
    pub session_lock_on_suspend: bool,
    pub session_docked_lock_on_doff: bool,
    pub session_docked_deep_idle_after_s: u64,
    // hardware.input.*
    pub hardware: Calibration,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            generation: 0,
            artifact_generation: String::new(),
            cursor_theme: "default".into(),
            cursor_size: 24,
            cursor_ray: "both".into(),
            cursor_scale: "angle".into(),
            cursor_angle_deg: 1.5,
            cursor_hide_when_typing: true,
            cursor_hide_after_ms: 0,
            pointer_gain: 1.0,
            pointer_accel_profile: "flat".into(),
            pointer_left_handed: false,
            pointer_warp: "gaze".into(),
            pointer_click_freeze_ms: 50,
            pointer_stick_deadzone: 0.0,
            scroll_natural: false,
            scroll_factor: 1.0,
            touchpad_tap: true,
            touchpad_dwt: true,
            touchpad_click_method: "default".into(),
            xkb_layout: String::new(),
            xkb_variant: String::new(),
            xkb_options: String::new(),
            xkb_model: String::new(),
            repeat_delay_ms: 600,
            repeat_rate_hz: 25,
            numlock: "remember".into(),
            osk_enabled: true,
            osk_suppress_after_key_s: 300,
            dwell_enabled: false,
            dwell_onset_ms: 200,
            dwell_complete_ms: 750,
            dwell_tolerance_deg: 2.0,
            targeting_source: "auto".into(),
            magnetism_enabled: false,
            hand_dominant: "right".into(),
            hand_pinch_close: 0.75,
            hand_pinch_open: 0.5,
            body_shoulder_half_m: (39.0 / 2.0 - 4.0) * 0.01,
            body_head_len_m: 0.10,
            body_neck_len_m: 0.07,
            emphasis_ramp_ms: 700,
            emphasis_strength: 0.15,
            system_long_press_ms: 500,
            system_double_tap_ms: 300,
            system_chord_hold_ms: 1000,
            system_double_press: "show_hide_planes".into(),
            system_gesture_hold_ms: 300,
            system_quit_timeout_s: 5,
            games_keep_planes: false,
            games_controller_system_button: true,
            wm_spawn_distance_m: 1.5,
            wm_spawn_elevation_deg: 0.0,
            wm_spawn_sibling_offset_m: 0.9,
            wm_spawn_sibling_yaw_rad: 0.35,
            wm_density_px_per_cm: 8.3,
            wm_focus_new_windows: "smart".into(),
            wm_focus_raise_on_commit: true,
            wm_grab_depth_rate: 3.0,
            wm_grab_bar_deg: 2.0,
            wm_move_billboard: true,
            wm_engine: "free".into(),
            wm_minimize: "dock".into(),
            wm_follow_default: false,
            wm_follow_threshold_deg: 40.0,
            wm_follow_delay_ms: 500,
            wm_follow_rate: 2.0,
            wm_follow_stop_deg: 4.0,
            ui_reduced_motion: false,
            session_idle_delay_s: 300,
            session_idle_lock_delay_s: 0,
            session_idle_count_emulated_input: true,
            session_lock_enabled: true,
            session_lock_doff_grace_s: 45,
            session_lock_on_doff: false,
            session_lock_on_idle: false,
            session_lock_on_suspend: true,
            session_docked_lock_on_doff: false,
            session_docked_deep_idle_after_s: 0,
            hardware: Calibration::default(),
        }
    }
}

/// Typed readers over the resolved map; an absent or mistyped key keeps the default (the
/// artifact validated the stored value already — `invalid` provenance resolves to the default
/// on the engine's side, spec §4 item 5).
struct Map<'a>(&'a HashMap<String, Value>);

impl Map<'_> {
    fn b(&self, id: &str, d: bool) -> bool {
        self.0.get(id).and_then(Value::as_bool).unwrap_or(d)
    }
    fn u(&self, id: &str, d: u64) -> u64 {
        self.0.get(id).and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))).unwrap_or(d)
    }
    fn f(&self, id: &str, d: f32) -> f32 {
        self.0.get(id).and_then(Value::as_f64).map(|f| f as f32).unwrap_or(d)
    }
    fn f64(&self, id: &str, d: f64) -> f64 {
        self.0.get(id).and_then(Value::as_f64).unwrap_or(d)
    }
    fn s(&self, id: &str, d: &str) -> String {
        self.0.get(id).and_then(Value::as_str).unwrap_or(d).to_string()
    }
}

impl Prefs {
    /// Build the picture from the engine's resolved keys. Missing keys keep the defaults, so an
    /// older artifact (fewer keys) is read without error.
    pub fn from_map(map: &HashMap<String, Value>, artifact_generation: &str) -> Prefs {
        let m = Map(map);
        let d = Prefs::default();
        let c = &d.hardware;
        Prefs {
            generation: 0,
            artifact_generation: artifact_generation.to_string(),
            cursor_theme: m.s("input.cursor.theme", &d.cursor_theme),
            cursor_size: m.u("input.cursor.size", d.cursor_size as u64) as u32,
            cursor_ray: m.s("input.cursor.ray", &d.cursor_ray),
            cursor_scale: m.s("input.cursor.scale", &d.cursor_scale),
            cursor_angle_deg: m.f("input.cursor.angle_deg", d.cursor_angle_deg),
            cursor_hide_when_typing: m.b("input.cursor.hide_when_typing", d.cursor_hide_when_typing),
            cursor_hide_after_ms: m.u("input.cursor.hide_after_ms", d.cursor_hide_after_ms),
            pointer_gain: m.f64("input.pointer.gain", d.pointer_gain),
            pointer_accel_profile: m.s("input.pointer.accel_profile", &d.pointer_accel_profile),
            pointer_left_handed: m.b("input.pointer.left_handed", d.pointer_left_handed),
            pointer_warp: m.s("input.pointer.warp", &d.pointer_warp),
            pointer_click_freeze_ms: m.u("input.pointer.click_freeze_ms", d.pointer_click_freeze_ms),
            pointer_stick_deadzone: m.f("input.pointer.stick_deadzone", d.pointer_stick_deadzone),
            scroll_natural: m.b("input.scroll.natural", d.scroll_natural),
            scroll_factor: m.f64("input.scroll.factor", d.scroll_factor),
            touchpad_tap: m.b("input.touchpad.tap", d.touchpad_tap),
            touchpad_dwt: m.b("input.touchpad.disable_while_typing", d.touchpad_dwt),
            touchpad_click_method: m.s("input.touchpad.click_method", &d.touchpad_click_method),
            xkb_layout: m.s("input.keyboard.xkb.layout", &d.xkb_layout),
            xkb_variant: m.s("input.keyboard.xkb.variant", &d.xkb_variant),
            xkb_options: m.s("input.keyboard.xkb.options", &d.xkb_options),
            xkb_model: m.s("input.keyboard.xkb.model", &d.xkb_model),
            repeat_delay_ms: m.u("input.keyboard.repeat.delay_ms", d.repeat_delay_ms as u64) as u32,
            repeat_rate_hz: m.u("input.keyboard.repeat.rate_hz", d.repeat_rate_hz as u64) as u32,
            numlock: m.s("input.keyboard.numlock", &d.numlock),
            osk_enabled: m.b("input.osk.enabled", d.osk_enabled),
            osk_suppress_after_key_s: m.u("input.osk.suppress_after_key_s", d.osk_suppress_after_key_s),
            dwell_enabled: m.b("input.dwell.enabled", d.dwell_enabled),
            dwell_onset_ms: m.u("input.dwell.onset_ms", d.dwell_onset_ms),
            dwell_complete_ms: m.u("input.dwell.complete_ms", d.dwell_complete_ms),
            dwell_tolerance_deg: m.f("input.dwell.tolerance_deg", d.dwell_tolerance_deg),
            targeting_source: m.s("input.targeting.source", &d.targeting_source),
            magnetism_enabled: m.b("input.magnetism.enabled", d.magnetism_enabled),
            hand_dominant: m.s("input.hand.dominant", &d.hand_dominant),
            hand_pinch_close: m.f("input.hand.pinch.close", d.hand_pinch_close),
            hand_pinch_open: m.f("input.hand.pinch.open", d.hand_pinch_open),
            body_shoulder_half_m: m.f("input.body.shoulder_half_m", d.body_shoulder_half_m),
            body_head_len_m: m.f("input.body.head_len_m", d.body_head_len_m),
            body_neck_len_m: m.f("input.body.neck_len_m", d.body_neck_len_m),
            emphasis_ramp_ms: m.u("input.emphasis.ramp_ms", d.emphasis_ramp_ms),
            emphasis_strength: m.f("input.emphasis.strength", d.emphasis_strength),
            system_long_press_ms: m.u("system.button.long_press_ms", d.system_long_press_ms),
            system_double_tap_ms: m.u("system.button.double_tap_ms", d.system_double_tap_ms),
            system_chord_hold_ms: m.u("system.button.chord_hold_ms", d.system_chord_hold_ms),
            system_double_press: m.s("system.button.double_press", &d.system_double_press),
            system_gesture_hold_ms: m.u("system.gesture.hold_ms", d.system_gesture_hold_ms),
            system_quit_timeout_s: m.u("system.quit_timeout_s", d.system_quit_timeout_s),
            games_keep_planes: m.b("games.keep_planes", d.games_keep_planes),
            games_controller_system_button: m.b("games.controller_system_button", d.games_controller_system_button),
            wm_spawn_distance_m: m.f("wm.spawn.distance_m", d.wm_spawn_distance_m),
            wm_spawn_elevation_deg: m.f("wm.spawn.elevation_deg", d.wm_spawn_elevation_deg),
            wm_spawn_sibling_offset_m: m.f("wm.spawn.sibling_offset_m", d.wm_spawn_sibling_offset_m),
            wm_spawn_sibling_yaw_rad: m.f("wm.spawn.sibling_yaw_rad", d.wm_spawn_sibling_yaw_rad),
            wm_density_px_per_cm: m.f("wm.density_px_per_cm", d.wm_density_px_per_cm),
            wm_focus_new_windows: m.s("wm.focus.new_windows", &d.wm_focus_new_windows),
            wm_focus_raise_on_commit: m.b("wm.focus.raise_on_commit", d.wm_focus_raise_on_commit),
            wm_grab_depth_rate: m.f("wm.grab.depth_rate", d.wm_grab_depth_rate),
            wm_grab_bar_deg: m.f("wm.grab.bar_deg", d.wm_grab_bar_deg),
            wm_move_billboard: m.b("wm.move.billboard", d.wm_move_billboard),
            wm_engine: m.s("wm.engine", &d.wm_engine),
            wm_minimize: m.s("wm.minimize", &d.wm_minimize),
            wm_follow_default: m.b("wm.follow.default", d.wm_follow_default),
            wm_follow_threshold_deg: m.f("wm.follow.threshold_deg", d.wm_follow_threshold_deg),
            wm_follow_delay_ms: m.u("wm.follow.delay_ms", d.wm_follow_delay_ms),
            wm_follow_rate: m.f("wm.follow.rate", d.wm_follow_rate),
            wm_follow_stop_deg: m.f("wm.follow.stop_deg", d.wm_follow_stop_deg),
            ui_reduced_motion: m.b("ui.reduced_motion", d.ui_reduced_motion),
            session_idle_delay_s: m.u("session.idle.delay_s", d.session_idle_delay_s),
            session_idle_lock_delay_s: m.u("session.idle.lock_delay_s", d.session_idle_lock_delay_s),
            session_idle_count_emulated_input: m.b("session.idle.count_emulated_input", d.session_idle_count_emulated_input),
            session_lock_enabled: m.b("session.lock.enabled", d.session_lock_enabled),
            session_lock_doff_grace_s: m.u("session.lock.doff_grace_s", d.session_lock_doff_grace_s),
            session_lock_on_doff: m.b("session.lock.on_doff", d.session_lock_on_doff),
            session_lock_on_idle: m.b("session.lock.on_idle", d.session_lock_on_idle),
            session_lock_on_suspend: m.b("session.lock.on_suspend", d.session_lock_on_suspend),
            session_docked_lock_on_doff: m.b("session.docked.lock_on_doff", d.session_docked_lock_on_doff),
            session_docked_deep_idle_after_s: m.u("session.docked.deep_idle_after_s", d.session_docked_deep_idle_after_s),
            hardware: Calibration {
                pinch_close: m.f("hardware.input.hand.pinch.close", c.pinch_close),
                pinch_open: m.f("hardware.input.hand.pinch.open", c.pinch_open),
                pinch_close_m: m.f("hardware.input.hand.pinch.close_m", c.pinch_close_m),
                pinch_open_m: m.f("hardware.input.hand.pinch.open_m", c.pinch_open_m),
                pinch_max_m: m.f("hardware.input.hand.pinch.max_m", c.pinch_max_m),
                poke_down_m: m.f("hardware.input.hand.poke.down_m", c.poke_down_m),
                poke_up_m: m.f("hardware.input.hand.poke.up_m", c.poke_up_m),
                near_enter_m: m.f("hardware.input.near_band.enter_m", c.near_enter_m),
                near_leave_m: m.f("hardware.input.near_band.leave_m", c.near_leave_m),
                gaze_fallback_ms: m.u("hardware.input.gaze.fallback_ms", c.gaze_fallback_ms),
                gaze_return_ms: m.u("hardware.input.gaze.return_ms", c.gaze_return_ms),
                held_timeout_ms: m.u("hardware.input.held.timeout_ms", c.held_timeout_ms),
                held_motion_m: m.f("hardware.input.held.motion_m", c.held_motion_m),
                held_axis: m.f("hardware.input.held.axis", c.held_axis),
                stab_position_half_life_s: m.f("hardware.input.stabilize.position_half_life_s", c.stab_position_half_life_s),
                stab_direction_half_life_s: m.f("hardware.input.stabilize.direction_half_life_s", c.stab_direction_half_life_s),
                stab_sticky: m.f("hardware.input.stabilize.sticky", c.stab_sticky),
                stab_relaxation_ray: m.f("hardware.input.stabilize.relaxation_ray", c.stab_relaxation_ray),
                stab_relaxation_gaze: m.f("hardware.input.stabilize.relaxation_gaze", c.stab_relaxation_gaze),
                stab_pinch_closed: m.f("hardware.input.stabilize.pinch_closed", c.stab_pinch_closed),
                stab_compensation_ms: m.u("hardware.input.stabilize.compensation_ms", c.stab_compensation_ms),
                hit_class_epsilon_m: m.f("hardware.input.hit.class_epsilon_m", c.hit_class_epsilon_m),
                palm_cone_deg: m.f("hardware.input.palm.cone_deg", c.palm_cone_deg),
                stick_deadzone: m.f("hardware.input.stick.deadzone", c.stick_deadzone),
                comfort_min_distance_m: m.f("hardware.input.comfort.min_distance_m", c.comfort_min_distance_m),
                comfort_max_distance_m: m.f("hardware.input.comfort.max_distance_m", c.comfort_max_distance_m),
                comfort_max_angular_deg: m.f("hardware.input.comfort.max_angular_deg", c.comfort_max_angular_deg),
                hmd_buttons: m.s("hardware.input.hmd.buttons", &c.hmd_buttons),
                hmd_select_role: m.s("hardware.input.hmd.select_role", &c.hmd_select_role),
                hmd_back_role: m.s("hardware.input.hmd.back_role", &c.hmd_back_role),
                hmd_system_role: m.s("hardware.input.hmd.system_role", &c.hmd_system_role),
            },
        }
    }

    // -- the per-consumer views -----------------------------------------------------------------

    /// `input.keyboard.xkb.*` as smithay's config; empty strings are xkbcommon's defaults
    /// (`XkbConfig::default()`: rules/model/layout/variant from the environment or `us`).
    pub fn xkb_config(&self) -> smithay::input::keyboard::XkbConfig<'_> {
        smithay::input::keyboard::XkbConfig {
            rules: "",
            model: &self.xkb_model,
            layout: &self.xkb_layout,
            variant: &self.xkb_variant,
            options: if self.xkb_options.is_empty() { None } else { Some(self.xkb_options.clone()) },
        }
    }

    /// Whether the keymap-affecting keys differ between two pictures (a recompile is not free).
    pub fn xkb_differs(&self, other: &Prefs) -> bool {
        self.xkb_layout != other.xkb_layout || self.xkb_variant != other.xkb_variant || self.xkb_options != other.xkb_options || self.xkb_model != other.xkb_model
    }

    /// The joint bridge's configuration (`input.hand.dominant`, `input.body.*`,
    /// `system.gesture.hold_ms`; the calibration's pinch metre ladder and palm cone).
    pub fn bridge_cfg(&self) -> crate::input::bridge::BridgeCfg {
        use crate::input::Side;
        let h = &self.hardware;
        crate::input::bridge::BridgeCfg {
            dominant: if self.hand_dominant == "left" { Side::Left } else { Side::Right },
            pinch_close_m: h.pinch_close_m,
            pinch_open_m: h.pinch_open_m.max(h.pinch_close_m),
            pinch_max_m: h.pinch_max_m.max(h.pinch_open_m + 0.001),
            palm_facing_cos: h.palm_cone_deg.clamp(1.0, 90.0).to_radians().cos(),
            hold_ns: self.system_gesture_hold_ms.saturating_mul(1_000_000),
            shoulder_half_m: self.body_shoulder_half_m,
            head_len_m: self.body_head_len_m,
            neck_len_m: self.body_neck_len_m,
        }
    }

    /// The reserved control's timings and choices (`system.button.*`, `games.controller_system_button`).
    pub fn reserved_cfg(&self) -> crate::input::reserved::ReservedCfg {
        use crate::input::reserved::{DoublePress, ReservedCfg};
        ReservedCfg {
            long_press_ns: self.system_long_press_ms.saturating_mul(1_000_000),
            double_ns: self.system_double_tap_ms.saturating_mul(1_000_000),
            chord_ns: self.system_chord_hold_ms.saturating_mul(1_000_000),
            double_press: DoublePress::parse(&self.system_double_press).unwrap_or_default(),
            controller_system_button: self.games_controller_system_button,
        }
    }

    /// The stabiliser's calibration (`hardware.input.stabilize.*`) with the layered
    /// `input.pointer.click_freeze_ms` as the compensation window.
    pub fn stabilize_cfg(&self) -> crate::input::stabilize::StabilizeCfg {
        let h = &self.hardware;
        crate::input::stabilize::StabilizeCfg {
            position_half_life_s: h.stab_position_half_life_s.max(0.0),
            direction_half_life_s: h.stab_direction_half_life_s.max(0.0),
            sticky: h.stab_sticky.clamp(0.0, 1.0),
            relaxation_ray: h.stab_relaxation_ray.clamp(0.0, 1.0),
            relaxation_gaze: h.stab_relaxation_gaze.clamp(0.0, 1.0),
            pinch_closed: h.stab_pinch_closed.clamp(0.0, 1.0),
            compensation_ns: self.pointer_click_freeze_ms.saturating_mul(1_000_000),
        }
    }

    /// The tier arbiter's calibration (`hardware.input.{gaze,held,near_band,hand.poke}.*`): the
    /// gaze fallback and return windows, the held-controller test, the near band, and the poke
    /// depths of the loss tracker's gesture ladder. The stale window is the gaze fallback (the
    /// symmetry `TierCfg` documents).
    pub fn tier_cfg(&self) -> crate::input::tier::TierCfg {
        use crate::input::{held::HeldCfg, quality::GazeCfg, tier::TierCfg};
        let h = &self.hardware;
        let gaze = GazeCfg { fallback_ns: h.gaze_fallback_ms.saturating_mul(1_000_000).max(1), return_ns: h.gaze_return_ms.saturating_mul(1_000_000).max(1) };
        TierCfg {
            gaze,
            held: HeldCfg { timeout_ns: h.held_timeout_ms.saturating_mul(1_000_000).max(1), motion_m: h.held_motion_m.max(0.0), axis_thd: h.held_axis.clamp(0.0, 1.0) as f64 },
            near_enter_m: h.near_enter_m.max(0.0),
            near_leave_m: h.near_leave_m.max(h.near_enter_m),
            stale_ns: gaze.fallback_ns,
        }
    }

    /// The loss tracker's gesture ladder: the layered pinch (`input.hand.pinch.*`) and the
    /// calibration's poke depths (`hardware.input.hand.poke.*`).
    pub fn gesture_cfg(&self) -> crate::input::loss::GestureCfg {
        let h = &self.hardware;
        crate::input::loss::GestureCfg { pinch_close: self.hand_pinch_close, pinch_open: self.hand_pinch_open.min(self.hand_pinch_close), poke_down_m: h.poke_down_m, poke_up_m: h.poke_up_m.max(h.poke_down_m) }
    }

    /// The grab's configuration (`wm.grab.*`, `wm.move.billboard`, the comfort limits, the pinch
    /// ladder) — window-workspace-management §4a.
    pub fn grab_cfg(&self) -> crate::input::grabs::GrabCfg {
        let h = &self.hardware;
        crate::input::grabs::GrabCfg {
            bar_deg: self.wm_grab_bar_deg.clamp(0.25, 10.0),
            depth_rate: self.wm_grab_depth_rate.max(0.0),
            min_distance_m: h.comfort_min_distance_m.max(0.05),
            max_distance_m: h.comfort_max_distance_m.max(h.comfort_min_distance_m + 0.1),
            max_angular_deg: h.comfort_max_angular_deg.clamp(5.0, 179.0),
            billboard: self.wm_move_billboard,
            pinch_close: self.hand_pinch_close,
            pinch_open: self.hand_pinch_open.min(self.hand_pinch_close),
        }
    }

    /// The scene's scale and placement (`wm.density_px_per_cm`, `wm.spawn.*`).
    pub fn layout(&self) -> crate::scene::Layout {
        crate::scene::Layout {
            m_per_px: crate::scene::Layout::m_per_px_of(self.wm_density_px_per_cm),
            spawn_distance_m: if self.wm_spawn_distance_m > 0.0 { self.wm_spawn_distance_m } else { -crate::scene::PLANE_DISTANCE },
            spawn_elevation_deg: self.wm_spawn_elevation_deg.clamp(-60.0, 60.0),
            sibling_offset_m: self.wm_spawn_sibling_offset_m.max(0.0),
            sibling_yaw_rad: self.wm_spawn_sibling_yaw_rad,
        }
    }

    /// The libinput per-device configuration (`input.pointer.*`, `input.scroll.natural`,
    /// `input.touchpad.*`).
    pub fn device_config(&self) -> crate::input::libinput::DeviceConfig {
        crate::input::libinput::DeviceConfig {
            accel_profile: self.pointer_accel_profile.clone(),
            left_handed: self.pointer_left_handed,
            natural_scroll: self.scroll_natural,
            tap: self.touchpad_tap,
            disable_while_typing: self.touchpad_dwt,
            click_method: self.touchpad_click_method.clone(),
        }
    }
}

/// The schemas zxr reads: the resolution lists only these prefixes.
const PREFIXES: [&str; 7] = ["input.", "wm.", "system.", "games.", "session.", "ui.", "hardware.input."];

/// The open engine and the watch — on `Zxr` so the calloop callback can re-resolve.
pub struct Settings {
    engine: Engine,
    /// keys whose stored value the engine reported `invalid` at the last resolution
    pub invalid: u64,
    pub keys: u64,
}

impl Settings {
    /// Open the artifact (`/etc/mura/settings-schema.json`, or `MURA_SETTINGS_SCHEMA`) and the
    /// per-user roots. `Err` when there is no artifact — the compositor then runs on
    /// [`Prefs::default`], which is the pre-settings behaviour (a dev host without an image).
    pub fn open() -> Result<Settings, String> {
        let path = mura_settingsd::artifact_path();
        let engine = mura_settingsd::open(Mode::Session, &path)?;
        Ok(Settings { engine, invalid: 0, keys: 0 })
    }

    /// Resolve every key zxr reads into a [`Prefs`].
    pub fn resolve(&mut self) -> Prefs {
        let mut map: HashMap<String, Value> = HashMap::with_capacity(128);
        let mut invalid = 0;
        for prefix in PREFIXES {
            for (id, eff) in self.engine.list(prefix) {
                if eff.provenance == "invalid" {
                    invalid += 1;
                }
                map.insert(id, eff.value);
            }
        }
        self.invalid = invalid;
        self.keys = map.len() as u64;
        Prefs::from_map(&map, &self.engine.generation)
    }

    /// A store file changed on disk: forget it so the next resolution re-reads it.
    pub fn store_changed(&mut self, file_name: &str) {
        if let Some(stem) = file_name.strip_suffix(".json") {
            self.engine.forget_store(stem);
        }
    }

    fn config_root(&self) -> std::path::PathBuf {
        self.engine.config_root().to_path_buf()
    }
}

/// Put a resolved picture on `Zxr`: bump the generation and push the values into the channels
/// the stages already read each tick (`Input.cursor_ray` etc. are the seat stage's intake; the
/// control socket's `zxr ctl cursor|a11y` writes the same fields, so the harness's path and the
/// settings path meet in one place). Stages with their own `*Cfg` re-derive from `st.prefs` when
/// `generation` moves.
pub fn apply(st: &mut Zxr, mut prefs: Prefs) {
    let first = st.prefs.generation == 0;
    prefs.generation = st.prefs.generation + 1;

    // the seat's keyboard: keymap (only when its keys moved — a recompile), repeat, num lock
    if let Some(kb) = st.seat.get_keyboard() {
        let xkb_default = prefs.xkb_layout.is_empty() && prefs.xkb_variant.is_empty() && prefs.xkb_options.is_empty() && prefs.xkb_model.is_empty();
        // the keymap `Zxr::new` compiled is `XkbConfig::default()`: at the first apply only a
        // non-default picture needs a recompile
        if (first && !xkb_default) || (!first && prefs.xkb_differs(&st.prefs)) {
            let cfg = prefs.xkb_config();
            let described = format!("{}/{}/{}/{}", cfg.layout, cfg.variant, cfg.model, cfg.options.clone().unwrap_or_default());
            // `XkbConfig::default()` when every key is empty: xkbcommon's own defaults
            let cfg = if xkb_default { smithay::input::keyboard::XkbConfig::default() } else { cfg };
            match kb.set_xkb_config(st, cfg) {
                Ok(()) => tracing::info!(xkb = %described, "keyboard: keymap from input.keyboard.xkb.*"),
                Err(e) => tracing::warn!(xkb = %described, "keyboard: keymap rejected ({e:?}); keeping the previous one"),
            }
        }
        if first || prefs.repeat_delay_ms != st.prefs.repeat_delay_ms || prefs.repeat_rate_hz != st.prefs.repeat_rate_hz {
            kb.change_repeat_info(prefs.repeat_rate_hz as i32, prefs.repeat_delay_ms as i32);
        }
        if first {
            // `input.keyboard.numlock`: on / off at start (niri `niri.rs:2536-2540`), or the
            // state remembered from the last session (cosmic-comp's `LastBoot`); the seat stage
            // writes the state file on change
            let want = match prefs.numlock.as_str() {
                "on" => Some(true),
                "off" => Some(false),
                _ => crate::input::seat::read_remembered_numlock(),
            };
            if let Some(on) = want {
                let mut mods = kb.modifier_state();
                if mods.num_lock != on {
                    mods.num_lock = on;
                    kb.set_modifier_state(mods);
                }
            }
        }
    }

    // the cursor theme (`input.cursor.{theme,size}`; the environment is the fallback)
    if first || !st.cursor_theme.matches(&prefs.cursor_theme, prefs.cursor_size) {
        st.cursor_theme = crate::input::theme::Theme::from_prefs(&prefs.cursor_theme, prefs.cursor_size);
    }

    // the libinput devices (`input.pointer.*`, `input.scroll.natural`, `input.touchpad.*`)
    crate::input::libinput::reconfigure(st, prefs.device_config());

    // the joint bridge, on the runtime's path and the injector's
    let bridge = prefs.bridge_cfg();
    if let Some(a) = st.xr.actions.as_mut() {
        a.bridge_cfg = bridge;
    }
    st.input.injector.bridge_cfg = bridge;

    // the scene's scale and placement (`wm.density_px_per_cm`, `wm.spawn.*`): a density change
    // re-derives every plane; a spawn change is for the next window
    let layout = prefs.layout();
    if st.scene.layout != layout {
        let rescale = (st.scene.layout.m_per_px - layout.m_per_px).abs() > 1e-9;
        st.scene.layout = layout;
        if rescale {
            st.rescale_planes();
            tracing::info!(density_px_per_cm = prefs.wm_density_px_per_cm, m_per_px = layout.m_per_px, "scene: planes rescaled (wm.density_px_per_cm)");
        }
    }

    // the idle ladder's view of emulated input (`session.idle.count_emulated_input`, Q5 flagged)
    st.input.activity.count_emulated = prefs.session_idle_count_emulated_input;

    // everything else is a stage's: they compare `prefs.generation` at their next tick
    st.journal.settings_generation = prefs.generation;
    st.prefs = prefs;
}

/// Open the settings, resolve once, apply, and watch the store directory from the state loop.
/// Without an artifact: log and run on defaults.
pub fn install(st: &mut Zxr, handle: &LoopHandle<'static, Zxr>) {
    let mut settings = match Settings::open() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("settings: no artifact ({e}); running on the built-in defaults (a device image carries /etc/mura/settings-schema.json)");
            return;
        }
    };
    let prefs = settings.resolve();
    st.journal.settings_keys = settings.keys;
    st.journal.settings_invalid = settings.invalid;
    tracing::info!(keys = settings.keys, invalid = settings.invalid, generation = %prefs.artifact_generation, "settings: resolved from the artifact and the per-user store");
    let root = settings.config_root();
    apply(st, prefs);

    // the watch: the store directory (created if the wearer has never written a preference —
    // the daemon would create it on the first write); one inotify fd, level-triggered
    if let Err(e) = std::fs::create_dir_all(&root) {
        tracing::warn!("settings: cannot create {}: {e}; changes will not be seen until restart", root.display());
        st.settings = Some(settings);
        return;
    }
    // SAFETY: plain libc calls; the fd is owned by the calloop source from here on.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        tracing::warn!("settings: inotify_init1 failed: {}", std::io::Error::last_os_error());
        st.settings = Some(settings);
        return;
    }
    let c_root = match std::ffi::CString::new(root.as_os_str().as_encoded_bytes()) {
        Ok(c) => c,
        Err(_) => {
            st.settings = Some(settings);
            return;
        }
    };
    let wd = unsafe { libc::inotify_add_watch(fd, c_root.as_ptr(), libc::IN_MOVED_TO | libc::IN_CLOSE_WRITE | libc::IN_DELETE) };
    if wd < 0 {
        tracing::warn!("settings: inotify_add_watch({}) failed: {}", root.display(), std::io::Error::last_os_error());
        unsafe { libc::close(fd) };
        st.settings = Some(settings);
        return;
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    st.settings = Some(settings);
    let r = handle.insert_source(Generic::new(owned, Interest::READ, CMode::Level), |_, fd, state| {
        // drain the events; each names a store file
        let mut buf = [0u8; 4096];
        let mut changed: Vec<String> = Vec::new();
        loop {
            // SAFETY: reading into our own buffer from our own fd
            let n = unsafe { libc::read(std::os::fd::AsRawFd::as_raw_fd(&std::os::fd::AsFd::as_fd(fd)), buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            let mut off = 0usize;
            while off + std::mem::size_of::<libc::inotify_event>() <= n as usize {
                // SAFETY: the kernel wrote a complete inotify_event header at `off`
                let ev = unsafe { &*(buf.as_ptr().add(off) as *const libc::inotify_event) };
                let name_len = ev.len as usize;
                let name_start = off + std::mem::size_of::<libc::inotify_event>();
                let name = &buf[name_start..(name_start + name_len).min(n as usize)];
                let name = name.iter().position(|b| *b == 0).map(|z| &name[..z]).unwrap_or(name);
                if let Ok(s) = std::str::from_utf8(name) {
                    if s.ends_with(".json") && !changed.iter().any(|c| c == s) {
                        changed.push(s.to_string());
                    }
                }
                off = name_start + name_len;
            }
        }
        if !changed.is_empty() {
            if let Some(settings) = state.settings.as_mut() {
                for f in &changed {
                    settings.store_changed(f);
                }
                let prefs = settings.resolve();
                state.journal.settings_reloads += 1;
                state.journal.settings_invalid = settings.invalid;
                tracing::info!(stores = ?changed, "settings: store changed, re-resolved");
                apply(state, prefs);
            }
        }
        Ok(PostAction::Continue)
    });
    if let Err(e) = r {
        tracing::warn!("settings: cannot watch the store directory: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_former_constants() {
        let d = Prefs::default();
        assert_eq!(d.cursor_ray, "both");
        assert_eq!(d.pointer_gain, 1.0);
        assert_eq!(d.dwell_onset_ms, 200);
        assert_eq!(d.dwell_complete_ms, 750);
        assert_eq!(d.system_double_tap_ms, 300);
        assert_eq!(d.hardware.pinch_close, 0.75);
        assert_eq!(d.hardware.near_enter_m, 0.18);
        assert!((d.wm_density_px_per_cm - 8.3).abs() < 1e-6 && (crate::scene::Layout::m_per_px_of(d.wm_density_px_per_cm) - crate::scene::M_PER_PX).abs() < 1e-9);
        assert!((d.body_shoulder_half_m - 0.155).abs() < 1e-6);
    }

    #[test]
    fn from_map_reads_typed_values_and_keeps_defaults_for_missing() {
        let mut m = HashMap::new();
        m.insert("input.cursor.ray".to_string(), Value::from("image"));
        m.insert("input.pointer.gain".to_string(), Value::from(2.5));
        m.insert("input.dwell.enabled".to_string(), Value::from(true));
        m.insert("input.keyboard.repeat.delay_ms".to_string(), Value::from(400));
        m.insert("hardware.input.hand.pinch.close".to_string(), Value::from(0.8));
        m.insert("input.hand.pinch.close".to_string(), Value::from(0.8));
        m.insert("session.lock.on_doff".to_string(), Value::from(true));
        let p = Prefs::from_map(&m, "gen-1");
        assert_eq!(p.cursor_ray, "image");
        assert_eq!(p.pointer_gain, 2.5);
        assert!(p.dwell_enabled);
        assert_eq!(p.repeat_delay_ms, 400);
        assert!((p.hardware.pinch_close - 0.8).abs() < 1e-6);
        assert!((p.hand_pinch_close - 0.8).abs() < 1e-6);
        assert!(p.session_lock_on_doff);
        // untouched keys keep the former constants
        assert_eq!(p.cursor_scale, "angle");
        assert_eq!(p.system_long_press_ms, 500);
        assert_eq!(p.artifact_generation, "gen-1");
        // a mistyped value keeps the default rather than failing
        m.insert("input.cursor.size".to_string(), Value::from("big"));
        assert_eq!(Prefs::from_map(&m, "").cursor_size, 24);
    }

    #[test]
    fn resolves_an_artifact_and_a_store_override_in_process() {
        // a minimal artifact with two of zxr's keys and one locked build fact; a store file
        // written the way the daemon writes it; the engine path (artifact load, list by
        // prefix, per-user > default, locked = artifact) exercised without a bus
        let dir = std::env::temp_dir().join(format!("zxr-settings-test-{}", std::process::id()));
        let config = dir.join("config/mura/settings");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let art = dir.join("schema.json");
        std::fs::write(&art, r#"{
          "artifactVersion": 1,
          "keys": [
            {"id":"input.cursor.ray","schema":"input.cursor","key":"ray","type":"enum","values":["both","image","ring"],"default":"both","class":"preference","stratum":"per-user","mutability":"mutable","locked":false,"apply":"live"},
            {"id":"input.pointer.gain","schema":"input.pointer","key":"gain","type":"double","default":1.0,"range":{"min":0.25,"max":4.0},"class":"preference","stratum":"per-user","mutability":"mutable","locked":false,"apply":"live"},
            {"id":"hardware.input.hand.pinch.close","schema":"hardware.input.hand.pinch","key":"close","type":"double","default":0.75,"class":"preference","stratum":"build-fact","mutability":"immutable","locked":true,"apply":"live"}
          ],
          "schemaVersions": {"input.cursor": 1, "input.pointer": 1, "hardware.input.hand.pinch": 1},
          "templates": {}
        }"#).unwrap();
        // the wearer set the ray cursor to image; someone tampered with the locked calibration
        std::fs::write(config.join("input.cursor.json"), r#"{"schema":"input.cursor","schemaVersion":1,"generation":"x","values":{"ray":"image"}}"#).unwrap();
        std::fs::write(config.join("hardware.input.hand.pinch.json"), r#"{"schema":"hardware.input.hand.pinch","schemaVersion":1,"generation":"x","values":{"close":0.1}}"#).unwrap();
        std::env::set_var("MURA_SETTINGS_SCHEMA", &art);
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_STATE_HOME", dir.join("state"));
        let mut s = Settings::open().expect("artifact opens");
        let p = s.resolve();
        assert_eq!(p.cursor_ray, "image", "the store's override wins");
        assert_eq!(p.pointer_gain, 1.0, "no override: the artifact's default");
        assert!((p.hardware.pinch_close - 0.75).abs() < 1e-6, "a locked key is the artifact's, whatever the file says (spec §7)");
        assert_eq!(s.invalid, 0);
        assert_eq!(s.keys, 3);
        // the wearer changes the ray cursor again: forget the store, re-resolve
        std::fs::write(config.join("input.cursor.json"), r#"{"schema":"input.cursor","schemaVersion":1,"generation":"x","values":{"ray":"ring"}}"#).unwrap();
        assert_eq!(s.resolve().cursor_ray, "image", "cached until the watch says so");
        s.store_changed("input.cursor.json");
        assert_eq!(s.resolve().cursor_ray, "ring");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
