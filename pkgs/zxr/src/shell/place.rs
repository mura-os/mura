//! The wearer's placement table (shell-plane.md §2.6; research/77 §3.3a; owner ruling
//! 2026-09-27): `shell.place:<namespace>` rows, and the seed rows for the namespaces the carried
//! components send. A row's field is `Some` only when the wearer set it (settings.rs drops
//! template defaults on the way in), so a half-filled row leaves the rest to the arrangement.
//!
//! Precedence, per field: the row → the client's anchoring request (frame; the pose for the
//! world frame) → the arranged box's centre in the head fallback. Hyprland's layer rules by
//! namespace are the precedent (`references/hyprland/src/desktop/rule/layerRule/LayerRule.cpp:96-115`).
//!
//! **Seeds** (`seed`): the consumer's defaults for an instance without a stored value — GSettings
//! relocatable schemas have no per-instance defaults, so the component ships them (GNOME's shape).
//! Keyed by protocol namespace strings, never by a program: `osk` is squeekboard's
//! (`references/squeekboard/src/panel.c:63-86`) and the Mura OSK's; `notifications` mako's;
//! `waybar`/`panel` a bar's. Moving the seeds to a Nix option is shell-plane §6's open item.

use super::Frame;

/// One row of the table; every field optional (explicit wearer values only).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaceRow {
    pub frame: Option<Frame>,
    pub azimuth_deg: Option<f32>,
    pub elevation_deg: Option<f32>,
    pub distance_m: Option<f32>,
    pub pitch_deg: Option<f32>,
    pub width_deg: Option<f32>,
}

impl PlaceRow {
    pub fn is_empty(&self) -> bool {
        *self == PlaceRow::default()
    }
}

/// The seed row for a namespace, if one ships.
///
/// - `osk` → body, low-centre, pitched toward the wearer (WayVR's keyboard at (0, −0.65, −0.5) m
///   pitched −10°, `references/wayvr/wayvr/src/overlays/keyboard/mod.rs:109-110`; research/60
///   §10 — the body frame is not built yet, so this resolves to head until it is)
/// - `notifications` → head, upper-right (mako's own anchor; research/36 §4's head-locked toasts)
/// - `waybar` / `panel` / `bar` → body, bottom (research/60 §9's body-frame dock)
pub fn seed(namespace: &str) -> Option<PlaceRow> {
    match namespace {
        "osk" => Some(PlaceRow { frame: Some(Frame::Body), azimuth_deg: Some(0.0), elevation_deg: Some(-35.0), distance_m: Some(0.5), pitch_deg: Some(-10.0), width_deg: None }),
        "notifications" => Some(PlaceRow { frame: Some(Frame::Head), azimuth_deg: None, elevation_deg: None, distance_m: None, pitch_deg: None, width_deg: None }),
        "waybar" | "panel" | "bar" => Some(PlaceRow { frame: Some(Frame::Body), azimuth_deg: Some(0.0), elevation_deg: Some(-25.0), distance_m: None, pitch_deg: Some(-5.0), width_deg: None }),
        _ => None,
    }
}

/// Parse one `shell.place:<namespace>.<key>` id into (namespace, key).
pub fn parse_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix("shell.place:")?;
    let dot = rest.rfind('.')?;
    Some((&rest[..dot], &rest[dot + 1..]))
}

/// Apply one explicit value to a row.
pub fn set_field(row: &mut PlaceRow, key: &str, value: &serde_json::Value) {
    let f = || value.as_f64().map(|v| v as f32);
    match key {
        "frame" => row.frame = value.as_str().and_then(Frame::parse),
        "azimuth_deg" => row.azimuth_deg = f(),
        "elevation_deg" => row.elevation_deg = f(),
        "distance_m" => row.distance_m = f(),
        "pitch_deg" => row.pitch_deg = f(),
        "width_deg" => row.width_deg = f(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_parse_with_dotted_namespaces() {
        assert_eq!(parse_id("shell.place:osk.frame"), Some(("osk", "frame")));
        assert_eq!(parse_id("shell.place:org.example.bar.azimuth_deg"), Some(("org.example.bar", "azimuth_deg")));
        assert_eq!(parse_id("shell.head.distance_m"), None);
    }

    #[test]
    fn a_row_takes_explicit_fields_only() {
        let mut r = PlaceRow::default();
        set_field(&mut r, "frame", &serde_json::json!("world"));
        set_field(&mut r, "elevation_deg", &serde_json::json!(-20.0));
        assert_eq!(r.frame, Some(Frame::World));
        assert_eq!(r.elevation_deg, Some(-20.0));
        assert_eq!(r.azimuth_deg, None);
        assert!(!r.is_empty());
    }

    #[test]
    fn seeds_are_by_namespace_not_program() {
        assert_eq!(seed("osk").and_then(|r| r.frame), Some(Frame::Body));
        assert!(seed("something-else").is_none());
    }
}
