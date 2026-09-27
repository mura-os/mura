//! libinput intake (spatial-input §1a lines 77, 94-96; §8 lines 323-349; spec §8 line 422
//! "libinput peripherals on the seat (smithay's backend, fd source, no thread)") and the
//! backend-agnostic `InputEvent` → [`Sample`] mapping the EI server (`ei.rs`) shares.
//!
//! **Placement — ruled (owner, 2026-09-26; research/68 §9.1):** the state loop, as a calloop
//! source; no thread. smithay's `LibinputInputBackend` is the `EventSource`
//! (`references/smithay/src/backend/libinput/mod.rs:583` `impl InputBackend`, `:707`
//! `impl EventSource` — one `dispatch` per readiness, events drained in a loop) over a libseat
//! session (`smithay::backend::session::libseat::LibSeatSession`), the shape anvil
//! (`references/smithay/anvil/src/udev.rs:227, 296-300, 307`) and niri
//! (`references/niri/src/backend/tty.rs:420-465`) share: `LibSeatSession::new()` →
//! `Libinput::new_with_udev(LibinputSessionInterface)` → `udev_assign_seat(session.seat())` →
//! `insert_source(backend)`, plus the session notifier pausing/resuming libinput on VT switch
//! (anvil `:328-333`; niri `:446-449` and the "not active at startup → start paused" rule).
//!
//! **Seat.** libinput assigns every device to the physical seat named by udev's `ID_SEAT`,
//! `seat0` by default, and the logical seat `WL_SEAT` / `default`
//! (`references/libinput/doc/user/seats.rst:7-16`; `references/libinput/src/udev-seat.c:34-35`
//! the defaults, `:82-99` the property lookup). zxr asks libseat for its seat name and hands it to
//! `udev_assign_seat`, so the unit's seat (the frame VM's, the headset's) is the one enumerated.
//! Nested on a host there is no seat to open — libseat fails — so intake **logs and continues
//! without it**; `ZXR_NO_LIBINPUT=1` skips the attempt (the harness's host runs).
//!
//! **Priority.** The design asks for a source priority above client sources (§1a line 77).
//! calloop 0.14 has no priority API (`calloop-0.14.4/src/loop_logic.rs`: `insert_source`,
//! `register_dispatcher`, `insert_idle` — no priority parameter; readiness is dispatched in the
//! order the poll returns it). **Documented, not implemented**: the M1 input gate measures
//! libinput-event→`xrEndFrame` under the client storm; if that exceeds a display period the
//! options are a priority-aware dispatcher (a second `Poll` drained first) or the dedicated
//! input thread the ruling holds in reserve — the owner's call, flagged in the lane report.
//!
//! **Mapping** ([`raw_of`] + [`sample_of`]): pointer motion → `SourceKind::Pointer` with `delta`
//! (the device is put on libinput's **flat** acceleration profile at add time — §8 line 326:
//! "there is no screen for adaptive acceleration to refer to"; `Device::config_accel_set_profile`,
//! niri does the same per config `references/niri/src/input/mod.rs:4936`); buttons →
//! `Button::Select/Secondary/Middle/Code` from `BTN_LEFT/RIGHT/MIDDLE`; axis → `axis` +
//! `AxisSource::{Wheel,Finger,Continuous}` (smithay's `PointerScrollAxis`, `libinput/mod.rs:160-232`);
//! keys → `SourceKind::Keyboard` `key: Some((evdev, pressed))` (smithay's `Keycode` is xkb's,
//! evdev + 8 — `libinput/mod.rs:114-117`).
//!
//! **`hmdButtons` roles** (device-contract.md lines 250-271): the HMD-body buttons arrive as
//! evdev keys on the seat (§8 line 346: "read through libinput today"). A key whose code is the
//! contract's `selectRole` / `backRole` / `systemRole` becomes a `SourceKind::Head` sample with
//! `Button::Select/Back/System` instead of a keyboard key, so the reserved stage and the
//! transports see it as the head's commit. The roles come from `ZXR_HMD_BUTTONS`
//! ("select=KEY_VOLUMEUP,back=KEY_VOLUMEDOWN,system=KEY_SELECT") **for now — the contract lands
//! through the settings compiler (flagged)**. The contract's press-duration disambiguation
//! (select sharing a code with power; recenter on long press) is not here: it is a stage's
//! timer, not intake's.
//!
//! **Not mapped yet:** touchpad gestures (`GestureSwipe*/Pinch*/Hold*` → `pointer-gestures`,
//! spec §8 line 454), touch, tablet, switches — reported as future.
//!
//! Budget (invariant 9): one loop dispatch per libinput readiness; per event one `match`, one
//! 128-byte `Sample` pushed into the pre-sized queue — no allocation in steady state.

use smithay::backend::input::{Axis, AxisSource as SmAxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent};
use smithay::reexports::calloop::LoopHandle;

use super::{AxisSource, Button, Flags, Sample, SourceKind};
use crate::state::Zxr;

// evdev codes (linux/input-event-codes.h)
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;
pub const KEY_BACK: u32 = 158;
pub const KEY_HOMEPAGE: u32 = 172;
pub const KEY_MENU: u32 = 139;
pub const KEY_POWER: u32 = 116;
pub const KEY_SELECT: u32 = 0x161;
pub const KEY_VOLUMEDOWN: u32 = 114;
pub const KEY_VOLUMEUP: u32 = 115;
pub const KEY_WAKEUP: u32 = 143;

/// libinput's wheel scroll value per detent (`libinput_event_pointer_get_scroll_value` returns 15
/// per click on the wheel source); used only when a backend reports v120 without a value.
const WHEEL_PX_PER_DETENT: f64 = 15.0;

/// The evdev code of a `KEY_*` name the device contract uses, or a bare number.
pub fn key_code_of(name: &str) -> Option<u32> {
    let n = name.trim();
    Some(match n {
        "KEY_BACK" => KEY_BACK,
        "KEY_HOMEPAGE" | "KEY_HOME_PAGE" => KEY_HOMEPAGE,
        "KEY_MENU" => KEY_MENU,
        "KEY_POWER" => KEY_POWER,
        "KEY_SELECT" => KEY_SELECT,
        "KEY_VOLUMEDOWN" => KEY_VOLUMEDOWN,
        "KEY_VOLUMEUP" => KEY_VOLUMEUP,
        "KEY_WAKEUP" => KEY_WAKEUP,
        _ => n.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| n.parse().ok())?,
    })
}

/// The contract's `hmdButtons` roles as evdev codes (device-contract.md lines 250-271).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct HmdRoles {
    pub select: Option<u32>,
    pub back: Option<u32>,
    pub system: Option<u32>,
}

impl HmdRoles {
    /// `"select=KEY_VOLUMEUP,back=KEY_VOLUMEDOWN,system=KEY_SELECT"`; unknown roles and names are
    /// skipped (logged by the caller). Empty → no roles.
    pub fn parse(spec: &str) -> HmdRoles {
        let mut r = HmdRoles::default();
        for item in spec.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            let Some((role, name)) = item.split_once('=') else { continue };
            let code = key_code_of(name);
            match role.trim() {
                "select" | "selectRole" => r.select = code,
                "back" | "backRole" => r.back = code,
                "system" | "systemRole" => r.system = code,
                _ => {}
            }
        }
        r
    }

    /// From the environment (`ZXR_HMD_BUTTONS`) — the harness's override; `None` when unset.
    pub fn from_env() -> Option<HmdRoles> {
        std::env::var("ZXR_HMD_BUTTONS").ok().map(|s| HmdRoles::parse(&s))
    }

    /// From the artifact's build facts (`hardware.input.hmd.*`, settings.rs `Calibration`): the
    /// contract's `hmdButtons` flattened to `role=KEY,…`, and the roles that name its keys
    /// (`selectRole`, `backRole`, `systemRole`). An empty role, or one naming no button, is none.
    pub fn from_calibration(buttons: &str, select_role: &str, back_role: &str, system_role: &str) -> HmdRoles {
        let code_of_role = |role: &str| -> Option<u32> {
            if role.is_empty() {
                return None;
            }
            buttons.split(',').filter_map(|item| item.trim().split_once('=')).find(|(r, _)| r.trim() == role).and_then(|(_, name)| key_code_of(name.trim()))
        };
        HmdRoles { select: code_of_role(select_role), back: code_of_role(back_role), system: code_of_role(system_role) }
    }

    /// Which role, if any, an evdev key code carries. `system` is checked first so a code shared
    /// with `select` (the Steam Frame's Aux, contract line 266) reaches the reserved stage.
    pub fn role_of(&self, code: u32) -> Option<Button> {
        if self.system == Some(code) {
            return Some(Button::System);
        }
        if self.select == Some(code) {
            return Some(Button::Select);
        }
        if self.back == Some(code) {
            return Some(Button::Back);
        }
        None
    }

    pub fn any(&self) -> bool {
        self.select.is_some() || self.back.is_some() || self.system.is_some()
    }
}

/// The backend-independent content of one device event (what both libinput and EI yield).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Raw {
    Motion { dx: f64, dy: f64 },
    Button { code: u32, pressed: bool },
    Axis { h: Option<f64>, v: Option<f64>, source: SmAxisSource },
    Key { evdev: u32, pressed: bool },
}

/// Pull the [`Raw`] event and its CLOCK_MONOTONIC time (ns) out of a smithay `InputEvent`.
/// Device add/remove, gestures, touch and the rest yield `None` (handled or deferred by the caller).
pub fn raw_of<B: InputBackend>(ev: &InputEvent<B>) -> Option<(Raw, u64)> {
    match ev {
        InputEvent::PointerMotion { event } => {
            let d = event.delta();
            Some((Raw::Motion { dx: d.x, dy: d.y }, event.time().micros() * 1000))
        }
        InputEvent::PointerButton { event } => Some((Raw::Button { code: event.button_code(), pressed: event.state() == ButtonState::Pressed }, event.time().micros() * 1000)),
        InputEvent::PointerAxis { event } => {
            let amount = |axis: Axis| event.amount(axis).or_else(|| event.amount_v120(axis).map(|v| v / 120.0 * WHEEL_PX_PER_DETENT));
            Some((Raw::Axis { h: amount(Axis::Horizontal), v: amount(Axis::Vertical), source: event.source() }, event.time().micros() * 1000))
        }
        InputEvent::Keyboard { event } => Some((Raw::Key { evdev: event.key_code().raw().saturating_sub(8), pressed: event.state() == KeyState::Pressed }, event.time().micros() * 1000)),
        _ => None,
    }
}

/// `BTN_*` → the action vocabulary (spatial-input §2).
pub fn button_of(code: u32) -> Button {
    match code {
        BTN_LEFT => Button::Select,
        BTN_RIGHT => Button::Secondary,
        BTN_MIDDLE => Button::Middle,
        other => Button::Code(other),
    }
}

/// smithay's axis source → the sample's (`WheelTilt` is a wheel for `wl_pointer.axis_source`).
pub fn axis_source_of(s: SmAxisSource) -> AxisSource {
    match s {
        SmAxisSource::Wheel | SmAxisSource::WheelTilt => AxisSource::Wheel,
        SmAxisSource::Finger => AxisSource::Finger,
        SmAxisSource::Continuous => AxisSource::Continuous,
    }
}

/// One [`Raw`] event → one [`Sample`]. `flags` is `Flags::EMULATED` for EI, empty for libinput.
/// A key matching an `hmdButtons` role becomes a `Head` button sample.
pub fn sample_of(raw: Raw, time_ns: u64, roles: &HmdRoles, flags: Flags) -> Sample {
    let mut s = match raw {
        Raw::Motion { dx, dy } => {
            let mut s = Sample::new(SourceKind::Pointer, time_ns);
            s.delta = Some((dx, dy));
            s
        }
        Raw::Button { code, pressed } => Sample::new(SourceKind::Pointer, time_ns).with_button(button_of(code), pressed),
        Raw::Axis { h, v, source } => {
            let mut s = Sample::new(SourceKind::Pointer, time_ns);
            s.axis = Some((h.unwrap_or(0.0), v.unwrap_or(0.0)));
            s.axis_source = Some(axis_source_of(source));
            s
        }
        Raw::Key { evdev, pressed } => match roles.role_of(evdev) {
            Some(role) => {
                // the head's own buttons: a commit on the head ray (device-contract 250-271)
                let mut s = Sample::new(SourceKind::Head, time_ns).with_button(role, pressed);
                s.key = Some((evdev, pressed));
                s
            }
            None => {
                let mut s = Sample::new(SourceKind::Keyboard, time_ns);
                s.key = Some((evdev, pressed));
                s
            }
        },
    };
    s.flags = flags;
    s
}

/// The per-device libinput configuration the settings carry (`input.pointer.{accel_profile,
/// left_handed}`, `input.scroll.natural`, `input.touchpad.{tap,disable_while_typing,
/// click_method}`) — applied to a device when it appears and to every known device when a key
/// changes (niri `apply_libinput_settings`, `src/input/mod.rs:4903`, re-run over `niri.devices`
/// on a config reload, `src/niri.rs:1774-1775`; cosmic-comp likewise).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceConfig {
    /// `flat` | `adaptive` — flat by the owner's Q3 ruling (spatial-input §8 line 326)
    pub accel_profile: String,
    pub left_handed: bool,
    pub natural_scroll: bool,
    pub tap: bool,
    pub disable_while_typing: bool,
    /// `default` | `button_areas` | `clickfinger`
    pub click_method: String,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        DeviceConfig { accel_profile: "flat".into(), left_handed: false, natural_scroll: false, tap: true, disable_while_typing: true, click_method: "default".into() }
    }
}

impl DeviceConfig {
    /// Apply to one device: every `config_*` call is best-effort — a device without the
    /// capability rejects it and that is fine (niri ignores the results the same way).
    pub fn apply(&self, device: &mut smithay::reexports::input::Device) {
        use smithay::reexports::input::{AccelProfile, ClickMethod, DeviceCapability};
        if device.has_capability(DeviceCapability::Pointer) {
            if device.config_accel_is_available() {
                let profile = match self.accel_profile.as_str() {
                    "adaptive" => AccelProfile::Adaptive,
                    _ => AccelProfile::Flat,
                };
                let _ = device.config_accel_set_profile(profile);
            }
            if device.config_left_handed_is_available() {
                let _ = device.config_left_handed_set(self.left_handed);
            }
            if device.config_scroll_has_natural_scroll() {
                let _ = device.config_scroll_set_natural_scroll_enabled(self.natural_scroll);
            }
        }
        // a touchpad: libinput's tap finger count is > 0 only for touchpads (niri's test)
        if device.config_tap_finger_count() > 0 {
            let _ = device.config_tap_set_enabled(self.tap);
            let _ = device.config_dwt_set_enabled(self.disable_while_typing);
            match self.click_method.as_str() {
                "button_areas" => {
                    let _ = device.config_click_set_method(ClickMethod::ButtonAreas);
                }
                "clickfinger" => {
                    let _ = device.config_click_set_method(ClickMethod::Clickfinger);
                }
                _ => {
                    if let Some(default) = device.config_click_default_method() {
                        let _ = device.config_click_set_method(default);
                    }
                }
            }
        }
    }
}

/// The libinput intake's state on `Zxr`.
#[derive(Default)]
pub struct Peripherals {
    pub roles: HmdRoles,
    /// libinput opened a seat and is registered
    pub active: bool,
    pub seat_name: Option<String>,
    pub devices: u32,
    /// the devices present, so a settings change can be applied to them (niri `Niri::devices`)
    pub known: Vec<smithay::reexports::input::Device>,
    /// the configuration in force (settings.rs `apply` replaces it and calls [`reconfigure`])
    pub config: DeviceConfig,
    /// counters (journal patch in the lane report)
    pub events: u64,
    pub events_unmapped: u64,
    pub hmd_buttons: u64,
}

/// The settings changed a device key: apply the new configuration to every known device.
pub fn reconfigure(st: &mut Zxr, config: DeviceConfig) {
    if st.peripherals.config == config {
        return;
    }
    st.peripherals.config = config;
    let mut devices = std::mem::take(&mut st.peripherals.known);
    for d in devices.iter_mut() {
        st.peripherals.config.apply(d);
    }
    st.peripherals.known = devices;
    tracing::info!(devices = st.peripherals.known.len(), ?st.peripherals.config, "libinput devices reconfigured (input.pointer/scroll/touchpad keys)");
}

/// Open the libseat session and register libinput on the loop. `Ok(false)` = no session (nested
/// on a host, or `ZXR_NO_LIBINPUT=1`): logged, intake absent, everything else continues.
pub fn start(st: &mut Zxr, handle: &LoopHandle<'static, Zxr>) -> Result<bool, String> {
    use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
    use smithay::backend::session::libseat::LibSeatSession;
    use smithay::backend::session::{Event as SessionEvent, Session};
    use smithay::reexports::input::{DeviceCapability, Libinput};

    // the HMD-body roles: the artifact's build facts (settings.rs has resolved them by now —
    // `settings::install` runs before this), or the harness's `ZXR_HMD_BUTTONS` override
    st.peripherals.roles = match HmdRoles::from_env() {
        Some(r) => {
            tracing::info!(roles = ?r, "hmdButtons roles (ZXR_HMD_BUTTONS override; device-contract 250-271)");
            r
        }
        None => {
            let h = &st.prefs.hardware;
            let r = HmdRoles::from_calibration(&h.hmd_buttons, &h.hmd_select_role, &h.hmd_back_role, &h.hmd_system_role);
            if r.any() {
                tracing::info!(roles = ?r, buttons = %h.hmd_buttons, "hmdButtons roles (hardware.input.hmd.*; device-contract 250-271)");
            }
            r
        }
    };
    if std::env::var_os("ZXR_NO_LIBINPUT").is_some() {
        tracing::info!("libinput intake skipped (ZXR_NO_LIBINPUT)");
        return Ok(false);
    }
    let (session, notifier) = match LibSeatSession::new() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("libinput intake absent: no libseat session ({e}); nested run or no seat — peripherals arrive over EI only");
            return Ok(false);
        }
    };
    let seat_name = session.seat();
    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput.udev_assign_seat(&seat_name).map_err(|()| format!("libinput: udev_assign_seat({seat_name}) failed"))?;
    if !session.is_active() {
        // niri tty.rs:441-446: start paused so ActivateSession re-enumerates
        libinput.suspend();
    }
    let backend = LibinputInputBackend::new(libinput.clone());
    handle
        .insert_source(backend, move |mut event, _, st: &mut Zxr| {
            match &mut event {
                InputEvent::DeviceAdded { device } => {
                    st.peripherals.devices += 1;
                    // spatial-input §8 line 326: the flat profile by default, the compositor
                    // applies the gain; the rest of the device keys with it (`DeviceConfig`)
                    st.peripherals.config.apply(device);
                    st.peripherals.known.push(device.clone());
                    if device.has_capability(DeviceCapability::Keyboard) {
                        if let Some(leds) = st.seat.get_keyboard().map(|k| k.led_state()) {
                            device.led_update(leds.into());
                        }
                    }
                    tracing::info!(name = %device.name(), pointer = device.has_capability(DeviceCapability::Pointer), keyboard = device.has_capability(DeviceCapability::Keyboard), "libinput device added");
                    return;
                }
                InputEvent::DeviceRemoved { device } => {
                    st.peripherals.devices = st.peripherals.devices.saturating_sub(1);
                    st.peripherals.known.retain(|d| d != device);
                    tracing::info!(name = %device.name(), "libinput device removed");
                    return;
                }
                _ => {}
            }
            st.peripherals.events += 1;
            match raw_of(&event) {
                Some((raw, t)) => {
                    let s = sample_of(raw, t, &st.peripherals.roles, Flags::default());
                    if s.kind == SourceKind::Head {
                        st.peripherals.hmd_buttons += 1;
                    }
                    // per-event dispatch on the state loop (research/68 §9.1, ruled): through the chain now
                    let now = crate::state::now_ns();
                    crate::input::dispatch(st, s, now);
                }
                // gestures, touch, tablet, switches: future (pointer-gestures needs pinch/swipe)
                None => st.peripherals.events_unmapped += 1,
            }
        })
        .map_err(|e| format!("libinput source: {e}"))?;
    handle
        .insert_source(notifier, move |event, _, _st: &mut Zxr| match event {
            SessionEvent::PauseSession => {
                tracing::info!("session paused: libinput suspended");
                libinput.suspend();
            }
            SessionEvent::ActivateSession => {
                tracing::info!("session activated: libinput resumed");
                if libinput.resume().is_err() {
                    tracing::warn!("libinput resume failed");
                }
            }
        })
        .map_err(|e| format!("session notifier: {e}"))?;
    st.peripherals.active = true;
    st.peripherals.seat_name = Some(seat_name.clone());
    tracing::info!(seat = %seat_name, "libinput intake on the state loop (spatial-input §1a; libseat session)");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_codes_and_axis_sources() {
        assert_eq!(button_of(BTN_LEFT), Button::Select);
        assert_eq!(button_of(BTN_RIGHT), Button::Secondary);
        assert_eq!(button_of(BTN_MIDDLE), Button::Middle);
        assert_eq!(button_of(0x113), Button::Code(0x113));
        assert_eq!(axis_source_of(SmAxisSource::Wheel), AxisSource::Wheel);
        assert_eq!(axis_source_of(SmAxisSource::WheelTilt), AxisSource::Wheel);
        assert_eq!(axis_source_of(SmAxisSource::Finger), AxisSource::Finger);
        assert_eq!(axis_source_of(SmAxisSource::Continuous), AxisSource::Continuous);
    }

    #[test]
    fn samples_from_raw_events() {
        let roles = HmdRoles::default();
        let m = sample_of(Raw::Motion { dx: 3.0, dy: -1.5 }, 7, &roles, Flags::default());
        assert_eq!(m.kind, SourceKind::Pointer);
        assert_eq!(m.delta, Some((3.0, -1.5)));
        assert_eq!(m.time_ns, 7);
        assert!(m.is_event() && m.pose.is_none());

        let b = sample_of(Raw::Button { code: BTN_RIGHT, pressed: true }, 8, &roles, Flags::default());
        assert_eq!(b.button, Some((Button::Secondary, true)));
        assert_eq!(b.kind, SourceKind::Pointer);

        let a = sample_of(Raw::Axis { h: None, v: Some(15.0), source: SmAxisSource::Wheel }, 9, &roles, Flags::default());
        assert_eq!(a.axis, Some((0.0, 15.0)));
        assert_eq!(a.axis_source, Some(AxisSource::Wheel));
        let f = sample_of(Raw::Axis { h: Some(2.0), v: Some(4.0), source: SmAxisSource::Finger }, 9, &roles, Flags::default());
        assert_eq!(f.axis_source, Some(AxisSource::Finger));

        let k = sample_of(Raw::Key { evdev: 30, pressed: true }, 10, &roles, Flags::default());
        assert_eq!(k.kind, SourceKind::Keyboard);
        assert_eq!(k.key, Some((30, true)));
        assert!(!k.flags.contains(Flags::EMULATED));

        // the EI path: identical mapping, EMULATED set (libei README 32-71 "distinction")
        let e = sample_of(Raw::Key { evdev: 30, pressed: false }, 11, &roles, Flags::EMULATED);
        assert_eq!(e.key, Some((30, false)));
        assert!(e.flags.contains(Flags::EMULATED));
        let e = sample_of(Raw::Motion { dx: 1.0, dy: 1.0 }, 11, &roles, Flags::EMULATED);
        assert!(e.flags.contains(Flags::EMULATED) && e.kind == SourceKind::Pointer);
    }

    #[test]
    fn hmd_button_roles_from_the_contracts_build_facts() {
        // the Steam Frame's contract: Aux is select and the system control, Vol- is back
        let buttons = "power=KEY_POWER,volumeUp=KEY_VOLUMEUP,volumeDown=KEY_VOLUMEDOWN,select=KEY_SELECT";
        let r = HmdRoles::from_calibration(buttons, "select", "volumeDown", "select");
        assert_eq!(r, HmdRoles { select: Some(KEY_SELECT), back: Some(KEY_VOLUMEDOWN), system: Some(KEY_SELECT) });
        assert_eq!(r.role_of(KEY_SELECT), Some(Button::System), "a shared code reaches the reserved stage");
        // the default contract: Vol+ select, Vol- back, no system control
        let r = HmdRoles::from_calibration("power=KEY_POWER,volumeDown=KEY_VOLUMEDOWN,volumeUp=KEY_VOLUMEUP", "volumeUp", "volumeDown", "");
        assert_eq!(r, HmdRoles { select: Some(KEY_VOLUMEUP), back: Some(KEY_VOLUMEDOWN), system: None });
        // a role naming no button is none
        assert_eq!(HmdRoles::from_calibration("power=KEY_POWER", "select", "", "").select, None);
    }

    #[test]
    fn hmd_button_roles_from_env_string() {
        let r = HmdRoles::parse("select=KEY_VOLUMEUP,back=KEY_VOLUMEDOWN,system=KEY_SELECT");
        assert_eq!(r, HmdRoles { select: Some(KEY_VOLUMEUP), back: Some(KEY_VOLUMEDOWN), system: Some(KEY_SELECT) });
        assert_eq!(r.role_of(KEY_VOLUMEUP), Some(Button::Select));
        assert_eq!(r.role_of(KEY_VOLUMEDOWN), Some(Button::Back));
        assert_eq!(r.role_of(KEY_SELECT), Some(Button::System));
        assert_eq!(r.role_of(30), None);

        // a role key becomes a Head button sample, other keys stay keyboard keys
        let s = sample_of(Raw::Key { evdev: KEY_VOLUMEUP, pressed: true }, 1, &r, Flags::default());
        assert_eq!(s.kind, SourceKind::Head);
        assert_eq!(s.button, Some((Button::Select, true)));
        let s = sample_of(Raw::Key { evdev: KEY_SELECT, pressed: true }, 1, &r, Flags::default());
        assert_eq!((s.kind, s.button), (SourceKind::Head, Some((Button::System, true))));
        let s = sample_of(Raw::Key { evdev: 30, pressed: true }, 1, &r, Flags::default());
        assert_eq!(s.kind, SourceKind::Keyboard);

        // select and system sharing a code (Steam Frame Aux): system wins → reserved stage
        let shared = HmdRoles::parse("select=KEY_SELECT,system=KEY_SELECT");
        assert_eq!(shared.role_of(KEY_SELECT), Some(Button::System));
        // numeric codes, hex, whitespace, unknown roles, empty
        let n = HmdRoles::parse(" select = 115 , back=0x72, bogus=KEY_POWER, nope");
        assert_eq!(n, HmdRoles { select: Some(115), back: Some(114), system: None });
        assert_eq!(HmdRoles::parse(""), HmdRoles::default());
        assert!(!HmdRoles::default().any());
        assert_eq!(key_code_of("KEY_NOPE"), None);
    }
}
