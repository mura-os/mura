//! Per-(schema, instance) store files (specs/settings-daemon.md §4): sparse JSON with a header,
//! written by temp + fsync + rename + directory fsync (snapd's AtomicWriteFile shape). A file that
//! does not parse is treated as absent and never rewritten (cosmic: corrupt keys stick — the
//! user's only copy survives). Unknown entries are preserved verbatim across rewrites (§4;
//! additive migrations, settings-schema.md §5).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoreFile {
    pub schema: String,
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub values: Map<String, Value>,
}

impl StoreFile {
    pub fn empty(schema: &str, instance: Option<&str>, schema_version: u32, generation: &str) -> StoreFile {
        StoreFile {
            schema: schema.to_string(),
            instance: instance.map(str::to_string),
            schema_version,
            generation: generation.to_string(),
            values: Map::new(),
        }
    }
}

/// Where a store lives: `<root>/<schema>[:<instance>].json`.
pub fn path_for(root: &Path, store_name: &str) -> PathBuf {
    root.join(format!("{store_name}.json"))
}

/// Load a store; `Ok(None)` when absent or unparsable (logged), never an error for the caller.
pub fn load(path: &Path) -> Option<StoreFile> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("mura-settingsd: {}: {e}; treating as absent", path.display());
            return None;
        }
    };
    match serde_json::from_str::<StoreFile>(&text) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("mura-settingsd: {}: {e}; treating as absent (file left untouched)", path.display());
            None
        }
    }
}

/// Atomic, durable write: `<file>.tmp` → fsync → rename → fsync(dir).
pub fn save(path: &Path, store: &StoreFile) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("store path has no parent"))?;
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = File::create(&tmp)?;
        let mut text = serde_json::to_string_pretty(store).map_err(std::io::Error::other)?;
        text.push('\n');
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    let d = File::open(dir)?;
    // SAFETY: fsync on an open directory fd is the documented way to make the rename durable.
    if unsafe { libc::fsync(d.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Instance stores of a template present on disk: `<template>:<instance>.json`.
pub fn list_instances(root: &Path, template: &str) -> Vec<String> {
    let prefix = format!("{template}:");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix(&prefix) {
                if let Some(inst) = rest.strip_suffix(".json") {
                    if !inst.is_empty() {
                        out.push(inst.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("mura-settings-test-{}-{}", std::process::id(), rand()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn rand() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
    }

    #[test]
    fn roundtrip_and_unknown_entries_preserved() {
        let d = tmpdir();
        let p = path_for(&d, "xr.passthrough");
        let mut s = StoreFile::empty("xr.passthrough", None, 1, "g1");
        s.values.insert("latencyMode".into(), Value::from("high-quality"));
        s.values.insert("fromTheFuture".into(), Value::from(42)); // a newer generation's key
        save(&p, &s).unwrap();
        let back = load(&p).unwrap();
        assert_eq!(back, s);
        assert!(!p.with_extension("json.tmp").exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn corrupt_file_is_absent_and_untouched() {
        let d = tmpdir();
        let p = path_for(&d, "xr.passthrough");
        fs::write(&p, "{ not json").unwrap();
        assert!(load(&p).is_none());
        assert_eq!(fs::read_to_string(&p).unwrap(), "{ not json");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn instances_listed_by_template() {
        let d = tmpdir();
        save(&path_for(&d, "places.entry:a"), &StoreFile::empty("places.entry", Some("a"), 1, "g")).unwrap();
        save(&path_for(&d, "places.entry:b"), &StoreFile::empty("places.entry", Some("b"), 1, "g")).unwrap();
        save(&path_for(&d, "other:z"), &StoreFile::empty("other", Some("z"), 1, "g")).unwrap();
        assert_eq!(list_instances(&d, "places.entry"), vec!["a".to_string(), "b".to_string()]);
        fs::remove_dir_all(&d).unwrap();
    }
}
