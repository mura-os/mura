//! The OpenXR action set — the XR half of intake (spatial-input §1a "The XR source seam is the
//! OpenXR action set", §2 sources; specs/zxr-core.md §8 "Sources"; research/68 §3.1–3.2, §5.3).
//!
//! **What this file is:** the *vocabulary* — the action names, the per-profile binding tables,
//! the profile → [`SourceKind`] rule, the edge and gaze-quality rules — as data and pure
//! functions, so a new controller is a `PROFILES` entry and the rules are testable without a
//! runtime. The runtime calls themselves live in `xr.rs` (`XrCore::new` creates and attaches;
//! `XrCore::sync_samples` reads), because they need the session.
//!
//! **The flow, per session and per tick:**
//!
//! 1. `XrCore::new` enables `EXT_hand_interaction`, `EXT_eye_gaze_interaction`,
//!    `EXT_hand_tracking` (for the §10 bridge) and `MNDX_system_buttons` when advertised;
//!    creates one action set `mura` with one action per semantic input; suggests bindings for
//!    every profile in [`PROFILES`] whose extension is enabled (a suggestion for an unknown or
//!    disabled profile fails the call — Monado `oxr_api_action.c:278-297`); creates one action
//!    space per pose action and hand (and the gaze space); attaches the set once.
//! 2. [`register_frames`] (called once from `main.rs` after `Zxr::new`) moves the action spaces
//!    into the scene as `Space::Xr` frames. From then on the batched `xrLocateSpacesKHR` the
//!    loop already makes (spec §5a; `ipc_client_space_overseer.c:161-195`) locates them every
//!    tick; `XrCore::locate_spaces` writes each action space's pose *and* location flags back
//!    into a per-tick cache by handle (the scene frame keeps only `valid`, and gaze quality needs
//!    the TRACKED bits).
//! 3. `input::tick` → `XrCore::sync_samples`: one `xrSyncActions` (N_devices `update_inputs`
//!    RPCs on Monado, `oxr_input.c:2045-2050`, `ipc_client_xdev.c:37-70`); then, per hand, the
//!    kind the bound profile maps to ([`kind_for_profile`], refreshed only on
//!    `XrEventDataInteractionProfileChanged`, `input.adoc:478-491`): a controller yields the aim
//!    ray + grasp + stick every tick and one sample per button edge ([`button_edges`]); a hand
//!    on `hand_interaction_ext` yields aim/poke + pinch/aim_activate/grasp + `ready`; a hand
//!    *not* on that profile goes through the joint bridge (`bridge.rs`, §10) when the runtime
//!    tracks joints. Gaze yields one sample per tick with [`gaze_quality`]
//!    (`ext_eye_gaze_interaction.adoc:140-158`).
//!
//! **Budget** (spatial-input §1a): the sync is the one per-tick runtime cost this adds; the
//! action-state reads are client-side on Monado; the bridge adds one `xrLocateHandJointsEXT`
//! per tracked hand per tick while Monado lacks the device. No allocation per tick.

use openxr as xr;

use super::{Button, Quality, Side, SourceKind};
use crate::scene::{FrameKind, Space};
use crate::state::Zxr;
use crate::xr::PoseTag;

pub const SET_NAME: &str = "mura";
/// Monado's preview extension (`bindings.json:82-140`); absent from openxr 0.22's `ExtensionSet`.
pub const MNDX_SYSTEM_BUTTONS: &str = "XR_MNDX_system_buttons";

/// `POSITION_VALID | ORIENTATION_VALID` and `POSITION_TRACKED | ORIENTATION_TRACKED`
/// (`XrSpaceLocationFlags` bits 0–3, `openxr-sys/src/generated.rs:9566-9574`; the crate's
/// `from_raw` is not `const`).
#[inline]
pub fn valid() -> xr::SpaceLocationFlags {
    xr::SpaceLocationFlags::from_raw(0b0011)
}
#[inline]
pub fn tracked() -> xr::SpaceLocationFlags {
    xr::SpaceLocationFlags::from_raw(0b1100)
}

/// One action per semantic input (spatial-input §1a): the closed vocabulary the bindings map onto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    AimPose,
    GripPose,
    PokePose,
    GazePose,
    Select,
    Menu,
    System,
    Secondary,
    Ready,
    Stick,
    Pinch,
    AimActivate,
    Grasp,
}

pub fn name(a: Act) -> &'static str {
    match a {
        Act::AimPose => "aim_pose",
        Act::GripPose => "grip_pose",
        Act::PokePose => "poke_pose",
        Act::GazePose => "gaze_pose",
        Act::Select => "select",
        Act::Menu => "menu",
        Act::System => "system",
        Act::Secondary => "secondary",
        Act::Ready => "ready",
        Act::Stick => "stick",
        Act::Pinch => "pinch",
        Act::AimActivate => "aim_activate",
        Act::Grasp => "grasp",
    }
}

pub fn localized(a: Act) -> &'static str {
    match a {
        Act::AimPose => "Aim pose",
        Act::GripPose => "Grip pose",
        Act::PokePose => "Poke pose",
        Act::GazePose => "Gaze pose",
        Act::Select => "Select",
        Act::Menu => "Menu",
        Act::System => "System",
        Act::Secondary => "Secondary",
        Act::Ready => "Ready",
        Act::Stick => "Stick",
        Act::Pinch => "Pinch",
        Act::AimActivate => "Aim activate",
        Act::Grasp => "Grasp",
    }
}

/// The boolean actions read for edges, in `HandState::buttons` order.
pub const BUTTONS: [(Act, Button); 4] = [(Act::Select, Button::Select), (Act::Menu, Button::Menu), (Act::System, Button::System), (Act::Secondary, Button::Secondary)];

/// Which top-level user path(s) a binding is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    Both,
    Left,
    Right,
    Eyes,
}

/// The extension a profile needs enabled before it may be suggested.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ext {
    Core,
    HandInteraction,
    EyeGaze,
    MndxSystemButtons,
}

/// What a bound profile makes of the hand: the `SourceKind` rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileKind {
    Controller,
    Hand,
    Gaze,
}

/// Which extensions `XrCore::new` enabled — the filter over `PROFILES`.
#[derive(Clone, Copy, Default, Debug)]
pub struct Enabled {
    pub hand_interaction: bool,
    pub eye_gaze: bool,
    pub hand_tracking: bool,
    pub mndx_system_buttons: bool,
}

pub struct Bind {
    pub act: Act,
    pub on: On,
    /// the component path under the user path, e.g. `input/aim/pose`
    pub sub: &'static str,
}

pub struct Profile {
    pub path: &'static str,
    pub ext: Ext,
    pub kind: ProfileKind,
    pub binds: &'static [Bind],
}

const fn b(act: Act, on: On, sub: &'static str) -> Bind {
    Bind { act, on, sub }
}

/// Suggested bindings per interaction profile (spatial-input §1a, §2, §8). Component paths from
/// `references/openxr-docs/specification/sources/chapters/semantic_paths.adoc` (simple
/// `:665-690`, touch `:1137-1181`, index `:1470-1518`),
/// `extensions/ext/ext_hand_interaction.adoc:284-302` and
/// `extensions/ext/ext_eye_gaze_interaction.adoc:118-125`. A boolean `select` bound to a
/// `trigger/value` is the standard's own conversion (threshold + hysteresis, `input.adoc:509-515`).
/// The touch/index semantic choices (trigger = select, squeeze = grasp, thumbstick = stick,
/// `b`/`y` = secondary) are wlx-overlay-s's (`wlx-common/assets/openxr_actions.json5:81-131,
/// 134-185`), the shipping Linux overlay for the same devices; its show/hide on `y`/left-`b` is
/// *not* imported as `menu` (see the lane report — `menu` on the Index profile is unbound).
pub const PROFILES: &[Profile] = &[
    Profile {
        path: "/interaction_profiles/khr/simple_controller",
        ext: Ext::Core,
        kind: ProfileKind::Controller,
        binds: &[b(Act::AimPose, On::Both, "input/aim/pose"), b(Act::GripPose, On::Both, "input/grip/pose"), b(Act::Select, On::Both, "input/select/click"), b(Act::Menu, On::Both, "input/menu/click")],
    },
    Profile {
        path: "/interaction_profiles/ext/hand_interaction_ext",
        ext: Ext::HandInteraction,
        kind: ProfileKind::Hand,
        binds: &[
            b(Act::AimPose, On::Both, "input/aim/pose"),
            b(Act::GripPose, On::Both, "input/grip/pose"),
            b(Act::PokePose, On::Both, "input/poke_ext/pose"),
            b(Act::Pinch, On::Both, "input/pinch_ext/value"),
            b(Act::Ready, On::Both, "input/pinch_ext/ready_ext"),
            b(Act::AimActivate, On::Both, "input/aim_activate_ext/value"),
            b(Act::Ready, On::Both, "input/aim_activate_ext/ready_ext"),
            b(Act::Grasp, On::Both, "input/grasp_ext/value"),
            b(Act::Ready, On::Both, "input/grasp_ext/ready_ext"),
        ],
    },
    Profile { path: "/interaction_profiles/ext/eye_gaze_interaction", ext: Ext::EyeGaze, kind: ProfileKind::Gaze, binds: &[b(Act::GazePose, On::Eyes, "input/gaze_ext/pose")] },
    Profile {
        path: "/interaction_profiles/oculus/touch_controller",
        ext: Ext::Core,
        kind: ProfileKind::Controller,
        binds: &[
            b(Act::AimPose, On::Both, "input/aim/pose"),
            b(Act::GripPose, On::Both, "input/grip/pose"),
            b(Act::Select, On::Both, "input/trigger/value"),
            b(Act::Grasp, On::Both, "input/squeeze/value"),
            b(Act::Stick, On::Both, "input/thumbstick"),
            // `menu/click` is left-only, `system/click` right-only on this profile
            // (`semantic_paths.adoc:1150-1160`; Monado `bindings.json` `side`)
            b(Act::Menu, On::Left, "input/menu/click"),
            b(Act::System, On::Right, "input/system/click"),
            b(Act::Secondary, On::Left, "input/y/click"),
            b(Act::Secondary, On::Right, "input/b/click"),
        ],
    },
    Profile {
        path: "/interaction_profiles/valve/index_controller",
        ext: Ext::Core,
        kind: ProfileKind::Controller,
        binds: &[
            b(Act::AimPose, On::Both, "input/aim/pose"),
            b(Act::GripPose, On::Both, "input/grip/pose"),
            b(Act::Select, On::Both, "input/trigger/value"),
            b(Act::Grasp, On::Both, "input/squeeze/value"),
            b(Act::Stick, On::Both, "input/thumbstick"),
            b(Act::System, On::Both, "input/system/click"),
            b(Act::Secondary, On::Both, "input/b/click"),
        ],
    },
    // Monado's virtual system-button profiles (`bindings.json:82-140`): they add the reserved
    // `system` click on WMR-family controllers whose core profile lacks it
    Profile { path: "/virtual_profiles/mndx/winmr_system_button", ext: Ext::MndxSystemButtons, kind: ProfileKind::Controller, binds: &[b(Act::System, On::Both, "input/system/click")] },
    Profile { path: "/virtual_profiles/mndx/samsung_odyssey_system_button", ext: Ext::MndxSystemButtons, kind: ProfileKind::Controller, binds: &[b(Act::System, On::Both, "input/system/click")] },
    Profile { path: "/virtual_profiles/mndx/hp_reverb_g2_system_button", ext: Ext::MndxSystemButtons, kind: ProfileKind::Controller, binds: &[b(Act::System, On::Both, "input/system/click")] },
];

/// The profile → kind rule (spatial-input §2): a hand bound to `hand_interaction_ext` is a
/// `Hand`, any controller profile is a `Controller`, `XR_NULL_PATH` (nothing bound) is none.
pub fn kind_for_profile(profile: xr::Path, table: &[(xr::Path, ProfileKind)], side: Side) -> Option<SourceKind> {
    if profile == xr::Path::NULL {
        return None;
    }
    match table.iter().find(|(p, _)| *p == profile).map(|(_, k)| *k) {
        Some(ProfileKind::Hand) => Some(SourceKind::Hand(side)),
        Some(ProfileKind::Controller) => Some(SourceKind::Controller(side)),
        Some(ProfileKind::Gaze) | None => None,
    }
}

/// Edge detection over the level states: one `(button, pressed)` per change, none otherwise.
/// `prev` is updated. A fixed-size result — no allocation per tick.
pub fn button_edges(prev: &mut [bool; BUTTONS.len()], cur: [bool; BUTTONS.len()]) -> [Option<(Button, bool)>; BUTTONS.len()] {
    let mut out = [None; BUTTONS.len()];
    for (i, (_, button)) in BUTTONS.iter().enumerate() {
        if cur[i] != prev[i] {
            out[i] = Some((*button, cur[i]));
            prev[i] = cur[i];
        }
    }
    out
}

/// Gaze quality per `ext_eye_gaze_interaction.adoc:140-158`: both TRACKED bits set → nominal
/// (fit for targeting); valid but not tracked → sub-nominal (never used for targeting, §9);
/// otherwise lost. An inactive action (unfocused session, no eyes) is lost too.
pub fn gaze_quality(flags: xr::SpaceLocationFlags, active: bool) -> Quality {
    if !active || !flags.contains(valid()) {
        Quality::Lost
    } else if flags.contains(tracked()) {
        Quality::Nominal
    } else {
        Quality::SubNominal
    }
}

/// One-time: move the action spaces into the scene as `Space::Xr` frames so the batched locate
/// finds them (spec §5a). Hand poses are `LeftHand`/`RightHand` frames; the gaze space is
/// recorded as a `Head` frame — `FrameKind` has no gaze variant yet (lane report).
pub fn register_frames(st: &mut Zxr) {
    for (tag, space) in st.xr.take_action_spaces() {
        let kind = match tag {
            PoseTag::Aim(Side::Left) | PoseTag::Grip(Side::Left) | PoseTag::Poke(Side::Left) => FrameKind::LeftHand,
            PoseTag::Aim(Side::Right) | PoseTag::Grip(Side::Right) | PoseTag::Poke(Side::Right) => FrameKind::RightHand,
            PoseTag::Gaze => FrameKind::Gaze,
        };
        let id = st.scene.add_frame(Space::Xr(space), kind);
        tracing::debug!(?tag, ?id, "action space registered as a scene frame");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(n: u64) -> xr::Path {
        xr::Path::from_raw(n)
    }

    #[test]
    fn profile_to_kind_table() {
        let table = vec![(path(1), ProfileKind::Controller), (path(2), ProfileKind::Hand), (path(3), ProfileKind::Gaze), (path(4), ProfileKind::Controller)];
        assert_eq!(kind_for_profile(path(1), &table, Side::Left), Some(SourceKind::Controller(Side::Left)));
        assert_eq!(kind_for_profile(path(4), &table, Side::Right), Some(SourceKind::Controller(Side::Right)));
        assert_eq!(kind_for_profile(path(2), &table, Side::Right), Some(SourceKind::Hand(Side::Right)));
        // the gaze profile is never a hand's kind; an unknown profile and XR_NULL_PATH map to nothing
        assert_eq!(kind_for_profile(path(3), &table, Side::Left), None);
        assert_eq!(kind_for_profile(path(99), &table, Side::Left), None);
        assert_eq!(kind_for_profile(xr::Path::NULL, &table, Side::Left), None);
    }

    #[test]
    fn edges_emit_once_per_change() {
        let mut prev = [false; 4];
        // press select: one edge
        let e = button_edges(&mut prev, [true, false, false, false]);
        assert_eq!(e.iter().flatten().count(), 1);
        assert_eq!(e[0], Some((Button::Select, true)));
        // held: no duplicate edge
        let e = button_edges(&mut prev, [true, false, false, false]);
        assert_eq!(e.iter().flatten().count(), 0);
        // release select and press system in the same tick: two edges
        let e = button_edges(&mut prev, [false, false, true, false]);
        assert_eq!(e[0], Some((Button::Select, false)));
        assert_eq!(e[2], Some((Button::System, true)));
        assert_eq!(e.iter().flatten().count(), 2);
        assert_eq!(prev, [false, false, true, false]);
    }

    #[test]
    fn gaze_quality_follows_the_extension_rule() {
        let v = valid();
        let t = xr::SpaceLocationFlags::from_raw(valid().into_raw() | tracked().into_raw());
        assert_eq!(gaze_quality(t, true), Quality::Nominal);
        assert_eq!(gaze_quality(v, true), Quality::SubNominal);
        assert_eq!(gaze_quality(xr::SpaceLocationFlags::EMPTY, true), Quality::Lost);
        assert_eq!(gaze_quality(t, false), Quality::Lost);
        // the bit layout the helpers assume (`generated.rs:9566-9574`)
        assert_eq!(v, xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID);
        assert_eq!(tracked(), xr::SpaceLocationFlags::POSITION_TRACKED | xr::SpaceLocationFlags::ORIENTATION_TRACKED);
    }

    #[test]
    fn binding_tables_are_well_formed() {
        for p in PROFILES {
            assert!(p.path.starts_with("/interaction_profiles/") || p.path.starts_with("/virtual_profiles/"), "{}", p.path);
            assert!(!p.binds.is_empty(), "{}", p.path);
            for bind in p.binds {
                assert!(bind.sub.starts_with("input/"), "{} {}", p.path, bind.sub);
                // pose actions bind only to pose components, gaze only on the eyes path
                match bind.act {
                    Act::AimPose | Act::GripPose | Act::PokePose | Act::GazePose => assert!(bind.sub.ends_with("/pose")),
                    _ => assert!(!bind.sub.ends_with("/pose")),
                }
                assert_eq!(bind.on == On::Eyes, bind.act == Act::GazePose);
            }
        }
        // the guaranteed profile is present and binds the §8 minimum
        let simple = PROFILES.iter().find(|p| p.path.ends_with("khr/simple_controller")).unwrap();
        for act in [Act::AimPose, Act::Select, Act::Menu] {
            assert!(simple.binds.iter().any(|b| b.act == act));
        }
    }
}
