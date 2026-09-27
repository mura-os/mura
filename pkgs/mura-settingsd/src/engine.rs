//! The resolver and writer (specs/settings-daemon.md §5): artifact + stores → effective value with
//! provenance; Set/Reset with the contract's rules (settings-schema.md §3, §4). Bus-agnostic so the
//! CLI's `--direct` and the unit tests exercise exactly what the daemon serves.

use crate::artifact::{validate, Artifact, Invalid, KeyRecord, KeyRef};
use crate::migrations::{self, Migration};
use crate::store::{self, StoreFile};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Session,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Locked,
    Immutable,
    Type,
    Range,
    UnknownKey,
    UnknownInstance,
    WrongBus,
    Io(String),
}

impl Error {
    /// The D-Bus error name (settings-schema.md §8).
    pub fn name(&self) -> &'static str {
        match self {
            Error::Locked => "org.mura.Settings1.Error.Locked",
            Error::Immutable => "org.mura.Settings1.Error.Immutable",
            Error::Type => "org.mura.Settings1.Error.Type",
            Error::Range => "org.mura.Settings1.Error.Range",
            Error::UnknownKey => "org.mura.Settings1.Error.UnknownKey",
            Error::UnknownInstance => "org.mura.Settings1.Error.UnknownInstance",
            Error::WrongBus => "org.mura.Settings1.Error.WrongBus",
            Error::Io(_) => "org.mura.Settings1.Error.Io",
        }
    }
    pub fn message(&self) -> String {
        match self {
            Error::Io(m) => m.clone(),
            e => format!("{e:?}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    pub value: Value,
    pub provenance: &'static str,
}

pub struct Engine {
    pub artifact: Artifact,
    pub generation: String,
    pub mode: Mode,
    config_root: PathBuf,
    state_root: PathBuf,
    stores: HashMap<String, Option<StoreFile>>,
    table: &'static [Migration],
}

impl Engine {
    pub fn new(artifact: Artifact, generation: String, mode: Mode, config_root: PathBuf, state_root: PathBuf) -> Engine {
        Engine { artifact, generation, mode, config_root, state_root, stores: HashMap::new(), table: migrations::MIGRATIONS }
    }

    #[cfg(test)]
    pub fn with_table(mut self, table: &'static [Migration]) -> Engine {
        self.table = table;
        self
    }

    /// A schema bump this binary has no step for is a build error (settings-daemon.md §7).
    pub fn check_versions(&self) -> Result<(), String> {
        let mut bad = Vec::new();
        for (schema, v) in &self.artifact.schema_versions {
            if *v > migrations::highest_known(self.table, schema) {
                bad.push(format!("{schema} v{v}"));
            }
        }
        for (name, t) in &self.artifact.templates {
            if t.schema_version > migrations::highest_known(self.table, name) {
                bad.push(format!("{name} v{}", t.schema_version));
            }
        }
        if bad.is_empty() {
            Ok(())
        } else {
            bad.sort();
            Err(format!("schema versions this daemon has no migration for: {}", bad.join(", ")))
        }
    }

    fn served_here(&self, rec: &KeyRecord) -> bool {
        match (self.mode, rec.stratum.as_str()) {
            (_, "build-fact") => true,
            (Mode::Session, "per-user") => true,
            (Mode::System, "device") => true,
            _ => false,
        }
    }

    fn root_for(&self, rec: &KeyRecord) -> &Path {
        if rec.class == "state" {
            &self.state_root
        } else {
            &self.config_root
        }
    }

    fn lookup(&self, id: &str) -> Result<(KeyRef, KeyRecord), Error> {
        let (r, rec) = self.artifact.lookup(id).ok_or(Error::UnknownKey)?;
        if !self.served_here(rec) {
            return Err(Error::WrongBus);
        }
        Ok((r, rec.clone()))
    }

    /// The store for a key, loaded (and migrated) on first access; `None` when absent.
    fn store(&mut self, r: &KeyRef, rec: &KeyRecord) -> Option<&StoreFile> {
        let name = r.store_name();
        if !self.stores.contains_key(&name) {
            let path = store::path_for(self.root_for(rec), &name);
            let mut loaded = store::load(&path);
            if let Some(s) = loaded.as_mut() {
                let target = self.artifact.schema_version(&r.schema);
                match migrations::run(self.table, s, target) {
                    Ok(0) => {}
                    Ok(_) => {
                        s.generation = self.generation.clone();
                        if let Err(e) = store::save(&path, s) {
                            eprintln!("mura-settingsd: {}: cannot record migration: {e}", path.display());
                        }
                    }
                    Err(at) => eprintln!("mura-settingsd: {}: no migration from v{at} to v{target}; resolving with what is there", path.display()),
                }
            }
            self.stores.insert(name.clone(), loaded);
        }
        self.stores.get(&name).and_then(|s| s.as_ref())
    }

    fn effective_of(&mut self, r: &KeyRef, rec: &KeyRecord) -> Effective {
        if rec.locked {
            return Effective { value: rec.default.clone(), provenance: "locked" };
        }
        if rec.mutability == "immutable" {
            return Effective { value: rec.default.clone(), provenance: "default" };
        }
        let stored = self.store(r, rec).and_then(|s| s.values.get(&r.key).cloned());
        match stored {
            Some(v) => match validate(rec, &v) {
                Ok(()) => Effective { value: v, provenance: if self.mode == Mode::System { "device" } else { "user" } },
                Err(_) => Effective { value: rec.default.clone(), provenance: "invalid" },
            },
            None => Effective { value: rec.default.clone(), provenance: "default" },
        }
    }

    pub fn get(&mut self, id: &str) -> Result<Effective, Error> {
        let (r, rec) = self.lookup(id)?;
        Ok(self.effective_of(&r, &rec))
    }

    fn write_store(&mut self, r: &KeyRef, rec: &KeyRecord, mutate: impl FnOnce(&mut StoreFile)) -> Result<(), Error> {
        let name = r.store_name();
        let _ = self.store(r, rec); // ensure loaded
        let root = self.root_for(rec).to_path_buf();
        let target = self.artifact.schema_version(&r.schema);
        let generation = self.generation.clone();
        let slot = self.stores.entry(name.clone()).or_insert(None);
        let s = slot.get_or_insert_with(|| StoreFile::empty(&r.schema, r.instance.as_deref(), target, &generation));
        mutate(s);
        s.generation = generation;
        store::save(&store::path_for(&root, &name), s).map_err(|e| Error::Io(e.to_string()))
    }

    /// `Set`: refuse locked/immutable/type/range; always write the override; report the new
    /// effective value only if value or provenance changed (§3).
    pub fn set(&mut self, id: &str, value: Value) -> Result<Option<Effective>, Error> {
        let (r, rec) = self.lookup(id)?;
        if rec.locked {
            return Err(Error::Locked);
        }
        if rec.mutability != "mutable" {
            return Err(Error::Immutable);
        }
        let value = coerce(&rec, value)?;
        validate(&rec, &value).map_err(|e| match e {
            Invalid::Type => Error::Type,
            Invalid::Range => Error::Range,
        })?;
        let before = self.effective_of(&r, &rec);
        let key = r.key.clone();
        let v = value.clone();
        self.write_store(&r, &rec, move |s| {
            s.values.insert(key, v);
        })?;
        let after = self.effective_of(&r, &rec);
        Ok(if after != before { Some(after) } else { None })
    }

    /// `Reset`: remove the override; report if the effective value or provenance moved.
    pub fn reset(&mut self, id: &str) -> Result<Option<Effective>, Error> {
        let (r, rec) = self.lookup(id)?;
        if rec.locked {
            return Err(Error::Locked);
        }
        if rec.mutability != "mutable" {
            return Err(Error::Immutable);
        }
        let before = self.effective_of(&r, &rec);
        let had = self.store(&r, &rec).map(|s| s.values.contains_key(&r.key)).unwrap_or(false);
        if had {
            let key = r.key.clone();
            self.write_store(&r, &rec, move |s| {
                s.values.remove(&key);
            })?;
        }
        let after = self.effective_of(&r, &rec);
        Ok(if after != before { Some(after) } else { None })
    }

    /// Fixed keys served here whose id starts with `prefix`, plus the keys of every on-disk
    /// instance of a template whose name starts with it.
    pub fn list(&mut self, prefix: &str) -> Vec<(String, Effective)> {
        let mut out = Vec::new();
        let fixed: Vec<KeyRecord> = self.artifact.keys_with_prefix(prefix).filter(|k| self.served_here(k)).cloned().collect();
        for rec in fixed {
            let r = KeyRef { schema: rec.schema.clone(), instance: None, key: rec.key.clone() };
            let e = self.effective_of(&r, &rec);
            out.push((rec.id.clone(), e));
        }
        let templates: Vec<String> = self.artifact.templates.keys().filter(|t| t.starts_with(prefix) || prefix.starts_with(t.as_str())).cloned().collect();
        for t in templates {
            for inst in self.list_instances(&t).unwrap_or_default() {
                let keys: Vec<KeyRecord> = self.artifact.templates[&t].keys.clone();
                for rec in keys {
                    let r = KeyRef { schema: t.clone(), instance: Some(inst.clone()), key: rec.key.clone() };
                    let id = r.id();
                    if id.starts_with(prefix) {
                        let e = self.effective_of(&r, &rec);
                        out.push((id, e));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn list_instances(&self, template: &str) -> Result<Vec<String>, Error> {
        let t = self.artifact.templates.get(template).ok_or(Error::UnknownKey)?;
        let class_state = t.keys.iter().any(|k| k.class == "state");
        let root = if class_state { &self.state_root } else { &self.config_root };
        Ok(store::list_instances(root, template))
    }

    pub fn delete_instance(&mut self, template: &str, instance: &str) -> Result<Vec<String>, Error> {
        let t = self.artifact.templates.get(template).ok_or(Error::UnknownKey)?.clone();
        let name = format!("{template}:{instance}");
        let root = if t.keys.iter().any(|k| k.class == "state") { self.state_root.clone() } else { self.config_root.clone() };
        let path = store::path_for(&root, &name);
        if !path.exists() {
            return Err(Error::UnknownInstance);
        }
        std::fs::remove_file(&path).map_err(|e| Error::Io(e.to_string()))?;
        self.stores.remove(&name);
        // every key of the instance now resolves to its default
        Ok(t.keys.iter().map(|k| format!("{template}:{instance}.{}", k.key)).collect())
    }

    /// A generation switch (settings-daemon.md §3): swap the artifact, drop the store cache, and
    /// report every key whose effective value or provenance moved.
    pub fn reload(&mut self, artifact: Artifact, generation: String) -> Result<Vec<(String, Effective)>, String> {
        let before = self.snapshot();
        self.artifact = artifact;
        self.generation = generation;
        self.check_versions()?;
        self.stores.clear();
        let after = self.snapshot();
        let mut changed = Vec::new();
        for (id, e) in &after {
            if before.get(id) != Some(e) {
                changed.push((id.clone(), e.clone()));
            }
        }
        for (id, _) in &before {
            if !after.contains_key(id) {
                // a key that left the schema: consumers holding it learn it is gone
                changed.push((id.clone(), Effective { value: Value::Null, provenance: "default" }));
            }
        }
        changed.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(changed)
    }

    fn snapshot(&mut self) -> HashMap<String, Effective> {
        self.list("").into_iter().collect()
    }
}

/// D-Bus variants arrive typed; JSON from the CLI may arrive as strings. Coerce a string to the
/// record's type where unambiguous (the `gsettings set` convenience).
fn coerce(rec: &KeyRecord, v: Value) -> Result<Value, Error> {
    if let Value::String(s) = &v {
        match rec.ty.as_str() {
            "bool" => return s.parse::<bool>().map(Value::from).map_err(|_| Error::Type),
            "int" => return s.parse::<i64>().map(Value::from).map_err(|_| Error::Type),
            "double" => return s.parse::<f64>().map(Value::from).map_err(|_| Error::Type),
            _ => {}
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::fixture;
    use std::fs;

    fn engine(mode: Mode) -> (Engine, PathBuf) {
        let d = std::env::temp_dir().join(format!("mura-engine-{}-{:?}", std::process::id(), std::time::SystemTime::now()));
        fs::create_dir_all(&d).unwrap();
        (Engine::new(fixture(), "g1".into(), mode, d.join("config"), d.join("state")), d)
    }

    #[test]
    fn defaults_then_set_then_reset() {
        let (mut e, d) = engine(Mode::Session);
        let g = e.get("xr.passthrough.latencyMode").unwrap();
        assert_eq!((g.value.as_str().unwrap(), g.provenance), ("low-latency", "default"));
        // Set equal to the default still creates the override: provenance moves, Changed fires
        let ch = e.set("xr.passthrough.latencyMode", Value::from("low-latency")).unwrap().unwrap();
        assert_eq!(ch.provenance, "user");
        // same value, same provenance: nothing to signal
        assert!(e.set("xr.passthrough.latencyMode", Value::from("low-latency")).unwrap().is_none());
        let ch = e.set("xr.passthrough.latencyMode", Value::from("high-quality")).unwrap().unwrap();
        assert_eq!(ch.value, Value::from("high-quality"));
        // the file is sparse and carries the header
        let text = fs::read_to_string(d.join("config/xr.passthrough.json")).unwrap();
        assert!(text.contains("\"latencyMode\": \"high-quality\"") && text.contains("\"schemaVersion\": 1"));
        assert!(!text.contains("enable"));
        let ch = e.reset("xr.passthrough.latencyMode").unwrap().unwrap();
        assert_eq!((ch.value.as_str().unwrap(), ch.provenance), ("low-latency", "default"));
        assert!(e.reset("xr.passthrough.latencyMode").unwrap().is_none());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refusals() {
        let (mut e, d) = engine(Mode::Session);
        assert_eq!(e.set("xr.passthrough.enable", Value::from(true)), Err(Error::Immutable));
        assert_eq!(e.set("shell.locked.thing", Value::from(4)), Err(Error::Locked));
        assert_eq!(e.get("shell.locked.thing").unwrap().provenance, "locked");
        assert_eq!(e.set("xr.passthrough.latencyMode", Value::from("medium")), Err(Error::Range));
        assert_eq!(e.set("hardware.ipd.meters", Value::from(0.2)), Err(Error::Range));
        assert_eq!(e.set("hardware.ipd.meters", Value::from("abc")), Err(Error::Type));
        assert_eq!(e.set("hardware.ipd.meters", Value::from("0.064")).unwrap().unwrap().value, Value::from(0.064));
        assert_eq!(e.get("nope.key"), Err(Error::UnknownKey));
        assert!(!d.join("config/xr.passthrough.json").exists()); // refused writes touch nothing
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn invalid_stored_value_resolves_to_default_and_stays() {
        let (mut e, d) = engine(Mode::Session);
        let p = d.join("config/xr.passthrough.json");
        let mut s = StoreFile::empty("xr.passthrough", None, 1, "old");
        s.values.insert("latencyMode".into(), Value::from("turbo")); // not in the enum any more
        store::save(&p, &s).unwrap();
        let bytes = fs::read(&p).unwrap();
        let g = e.get("xr.passthrough.latencyMode").unwrap();
        assert_eq!((g.value.as_str().unwrap(), g.provenance), ("low-latency", "invalid"));
        assert_eq!(fs::read(&p).unwrap(), bytes); // byte-identical: the user's copy survives
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn instances_create_list_delete() {
        let (mut e, d) = engine(Mode::Session);
        assert_eq!(e.list_instances("places.entry").unwrap(), Vec::<String>::new());
        e.set("places.entry:desk.enabled", Value::from(false)).unwrap();
        e.set("places.entry:sofa.launch", Value::from("firefox")).unwrap();
        assert_eq!(e.list_instances("places.entry").unwrap(), vec!["desk".to_string(), "sofa".to_string()]);
        let g = e.get("places.entry:desk.launch").unwrap();
        assert_eq!((g.value.as_str().unwrap(), g.provenance), ("", "default"));
        let gone = e.delete_instance("places.entry", "desk").unwrap();
        assert_eq!(gone, vec!["places.entry:desk.enabled".to_string(), "places.entry:desk.launch".to_string()]);
        assert_eq!(e.list_instances("places.entry").unwrap(), vec!["sofa".to_string()]);
        assert_eq!(e.delete_instance("places.entry", "desk"), Err(Error::UnknownInstance));
        let listed = e.list("places.entry");
        assert_eq!(listed.len(), 2);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn wrong_bus_and_modes() {
        let (mut e, d) = engine(Mode::System);
        assert_eq!(e.get("xr.passthrough.latencyMode"), Err(Error::WrongBus));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn reload_reports_moved_defaults_and_survivors() {
        let (mut e, d) = engine(Mode::Session);
        e.set("hardware.ipd.meters", Value::from(0.061)).unwrap();
        let mut a2 = fixture();
        for k in a2.keys.iter_mut() {
            if k.id == "xr.passthrough.latencyMode" {
                k.default = Value::from("high-quality"); // the image's default moved
            }
            if k.id == "hardware.ipd.meters" {
                k.default = Value::from(0.065); // moved too, but the user pinned a value
            }
        }
        let changed = e.reload(a2, "g2".into()).unwrap();
        let ids: Vec<&str> = changed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["xr.passthrough.latencyMode"]);
        assert_eq!(e.get("hardware.ipd.meters").unwrap().value, Value::from(0.061));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn version_check_refuses_unknown_bumps() {
        let (mut e, d) = engine(Mode::Session);
        assert!(e.check_versions().is_ok());
        e.artifact.schema_versions.insert("xr.passthrough".into(), 2);
        assert!(e.check_versions().unwrap_err().contains("xr.passthrough v2"));
        fs::remove_dir_all(&d).unwrap();
    }
}
