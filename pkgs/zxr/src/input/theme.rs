//! The compositor's cursor theme (spatial-input §7: `cursor-shape-v1` names "rendered from the
//! compositor's theme, at one scale").
//!
//! **Mechanism — the freedesktop one every compositor uses.** Xcursor themes found through
//! `XCURSOR_PATH` (wlroots `xcursor/xcursor.c:515-563`: the env, then `XDG_DATA_HOME`, `~/.icons`,
//! the system dirs), named by `XCURSOR_THEME`, sized by `XCURSOR_SIZE`: KWin reads those two first
//! and only then its config (`kwin/src/cursor.cpp:117-126` — "XCURSOR_SIZE might not be set");
//! smithay's anvil reads them (`anvil/src/cursor.rs:18-27`); niri reads its config and *sets*
//! them so clients agree (`niri/src/cursor.rs:189-193`). The `org.mura.Settings1` key for theme
//! and size is spatial-input §14's (none is listed yet — flagged); until it exists the environment
//! is the source, KWin's first step.
//! Theme absent or name missing in the theme → nothing is drawn and the journal counts it
//! (`input_cursor_named_ticks`); the reticle is still there for the ray.
//!
//! **Budget.** One theme lookup per distinct shape name per session (a file read and parse), the
//! decoded image cached; the frame procedure uploads once per name change into the cursor panel.
//! Nothing per tick.

use std::collections::HashMap;

use smithay::input::pointer::CursorIcon;
use xcursor::parser::{parse_xcursor, Image};
use xcursor::CursorTheme;

/// The default nominal size when `XCURSOR_SIZE` is unset (Xcursor's own default, wlroots' 24).
pub const DEFAULT_SIZE: u32 = 24;

/// One decoded cursor image: ARGB8888 bytes (`upload_shm`'s format), size and hotspot.
pub struct Loaded {
    pub width: u32,
    pub height: u32,
    pub hotspot: (i32, i32),
    pub argb: Vec<u8>,
}

pub struct Theme {
    theme: CursorTheme,
    name: String,
    size: u32,
    cache: HashMap<&'static str, Option<std::rc::Rc<Loaded>>>,
    pub misses: u64,
}

impl Theme {
    /// From the environment (the freedesktop mechanism); never fails — a missing theme yields
    /// misses, not an error. The settings' `input.cursor.{theme,size}` override it through
    /// [`Theme::from_prefs`].
    pub fn from_env() -> Theme {
        let (name, size) = env_theme();
        tracing::info!("cursor theme: {name} at {size} px (XCURSOR_THEME/XCURSOR_SIZE)");
        Theme::new(&name, size)
    }

    /// From `input.cursor.{theme,size}` (settings.rs): the key's default `"default"` means the
    /// environment's theme when one is set — the freedesktop mechanism stays the fallback
    /// (cosmic-comp reads only the environment, `backend/render/cursor.rs:669-676`; niri takes
    /// its config key and *exports* it as `XCURSOR_THEME`/`XCURSOR_SIZE` for its clients,
    /// `cursor.rs:189-193` — that export is not done here: `set_var` races the loader's threads,
    /// and the session's environment is the greeter's/systemd's to set).
    pub fn from_prefs(name: &str, size: u32) -> Theme {
        let (env_name, _) = env_theme();
        let name = if name.is_empty() || name == "default" { env_name } else { name.to_string() };
        let size = if size > 0 { size } else { DEFAULT_SIZE };
        tracing::info!("cursor theme: {name} at {size} px (input.cursor.theme/size)");
        Theme::new(&name, size)
    }

    pub fn new(name: &str, size: u32) -> Theme {
        Theme { theme: CursorTheme::load(name), name: name.to_string(), size, cache: HashMap::new(), misses: 0 }
    }

    /// Whether this theme is already `(name, size)` as [`Theme::from_prefs`] would resolve them.
    pub fn matches(&self, name: &str, size: u32) -> bool {
        let (env_name, _) = env_theme();
        let want = if name.is_empty() || name == "default" { env_name.as_str() } else { name };
        self.name == want && self.size == size.max(1)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// The image for a `cursor-shape-v1` name (the icon's name, then its alternates), the frame
    /// nearest the nominal size; animated cursors give their first frame.
    pub fn lookup(&mut self, icon: CursorIcon) -> Option<std::rc::Rc<Loaded>> {
        let key = icon.name();
        if let Some(hit) = self.cache.get(key) {
            return hit.clone();
        }
        let loaded = std::iter::once(key).chain(icon.alt_names().iter().copied()).find_map(|n| self.load(n)).map(std::rc::Rc::new);
        if loaded.is_none() {
            self.misses += 1;
        }
        self.cache.insert(key, loaded.clone());
        loaded
    }

    fn load(&self, name: &str) -> Option<Loaded> {
        let path = self.theme.load_icon(name)?;
        let bytes = std::fs::read(path).ok()?;
        let images = parse_xcursor(&bytes)?;
        let img: &Image = images.iter().min_by_key(|i| (i.size as i64 - self.size as i64).abs())?;
        let (w, h) = (img.width, img.height);
        if w == 0 || h == 0 {
            return None;
        }
        Some(Loaded { width: w, height: h, hotspot: (img.xhot as i32, img.yhot as i32), argb: img.pixels_argb.clone() })
    }
}

/// `XCURSOR_THEME` / `XCURSOR_SIZE`, with Xcursor's own defaults.
fn env_theme() -> (String, u32) {
    let name = std::env::var("XCURSOR_THEME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "default".into());
    let size = std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse::<u32>().ok()).filter(|s| *s > 0).unwrap_or(DEFAULT_SIZE);
    (name, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_override_the_environment_and_default_falls_back_to_it() {
        std::env::set_var("XCURSOR_THEME", "env-theme");
        std::env::set_var("XCURSOR_PATH", "/nonexistent");
        let t = Theme::from_prefs("default", 24);
        assert_eq!(t.name(), "env-theme", "the key's default is the environment's theme");
        assert!(t.matches("default", 24));
        let t = Theme::from_prefs("breeze_cursors", 32);
        assert_eq!((t.name(), t.size()), ("breeze_cursors", 32));
        assert!(!t.matches("default", 32));
        assert!(t.matches("breeze_cursors", 32));
    }

    #[test]
    fn missing_theme_counts_misses_and_caches_them() {
        std::env::set_var("XCURSOR_THEME", "mura-no-such-theme");
        std::env::set_var("XCURSOR_PATH", "/nonexistent");
        let mut t = Theme::from_env();
        assert!(t.lookup(CursorIcon::Default).is_none());
        assert!(t.lookup(CursorIcon::Default).is_none());
        assert_eq!(t.misses, 1, "second lookup is the cache");
        assert_eq!(t.size(), DEFAULT_SIZE);
    }
}
