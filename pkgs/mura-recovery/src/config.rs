//! `/etc/mura/recovery.json` (specs/recovery-menu.md §6): the contract's key roles as evdev codes
//! with the keyboard fallbacks appended, the family's slot-switch command, the persist device.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Keys {
    pub next: Vec<u16>,
    pub prev: Vec<u16>,
    pub select: Vec<u16>,
    pub back: Vec<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub keys: Keys,
    #[serde(default)]
    pub switch_slot_command: Option<String>,
    pub persist_device: String,
    pub gadget_addr: String,
    pub docs_url: String,
    #[serde(default = "default_long_press")]
    pub long_press_ms: u64,
}

fn default_long_press() -> u64 {
    750
}

pub const DEFAULT_PATH: &str = "/etc/mura/recovery.json";

impl Config {
    pub fn load(path: &str) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))
    }

    /// A configuration for a system without the file (tests, the shell on a dev box).
    pub fn fallback() -> Config {
        Config {
            keys: Keys { next: vec![114, 108], prev: vec![115, 103], select: vec![353, 28], back: vec![1] },
            switch_slot_command: None,
            persist_device: "/dev/disk/by-partlabel/syspersist".into(),
            gadget_addr: "172.16.42.1".into(),
            docs_url: "https://mura.dev/recovery".into(),
            long_press_ms: 750,
        }
    }
}
