//! mura-settingsd — `org.mura.Settings1` over the generated schema artifact
//! (specs/settings-schema.md, specs/settings-daemon.md). One crate, two modes: the session bus
//! for per-user keys (`mura-settingsd`), the system bus for device keys (`mura-settingsd --system`,
//! reserved: no target declares a device key yet). `mura-settings` is the CLI.

pub mod artifact;
pub mod bus;
pub mod engine;
pub mod migrations;
pub mod store;

use engine::{Engine, Mode};
use std::path::{Path, PathBuf};

pub const BUS_NAME: &str = "org.mura.Settings1";
pub const OBJECT_PATH: &str = "/org/mura/Settings1";

/// The artifact path: `/etc/mura/settings-schema.json`, or `MURA_SETTINGS_SCHEMA` (tests).
pub fn artifact_path() -> PathBuf {
    std::env::var_os("MURA_SETTINGS_SCHEMA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(artifact::DEFAULT_PATH))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The storage roots per mode (settings-daemon.md §4): XDG for a session, /var/lib for the
/// system mode.
pub fn roots(mode: Mode) -> (PathBuf, PathBuf) {
    match mode {
        Mode::Session => {
            let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
            let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state"));
            (config.join("mura/settings"), state.join("mura/settings"))
        }
        Mode::System => (PathBuf::from("/var/lib/mura/settings/config"), PathBuf::from("/var/lib/mura/settings/state")),
    }
}

/// Load the artifact and build the engine for a mode.
pub fn open(mode: Mode, path: &Path) -> Result<Engine, String> {
    let artifact = artifact::Artifact::load(path)?;
    let generation = artifact::Artifact::generation_of(path);
    let (config, state) = roots(mode);
    let engine = Engine::new(artifact, generation, mode, config, state);
    engine.check_versions()?;
    Ok(engine)
}
