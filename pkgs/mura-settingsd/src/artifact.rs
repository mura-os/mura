//! The generated schema artifact (specs/settings-schema.md §1): `/etc/mura/settings-schema.json`,
//! written by lib/settings at build time. The only default channel (constraint 9): nothing here
//! or in a consumer carries a default of its own.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub const DEFAULT_PATH: &str = "/etc/mura/settings-schema.json";

#[derive(Debug, Clone, Deserialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

/// One key of a fixed schema (from an annotated option) or of a template.
#[derive(Debug, Clone, Deserialize)]
pub struct KeyRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub schema: String,
    pub key: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub range: Option<Range>,
    pub default: Value,
    #[serde(default = "default_class")]
    pub class: String,
    #[serde(default = "default_stratum")]
    pub stratum: String,
    #[serde(default = "default_mutability")]
    pub mutability: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_apply")]
    pub apply: String,
    #[serde(default)]
    pub description: String,
}

fn default_class() -> String {
    "preference".into()
}
fn default_stratum() -> String {
    "per-user".into()
}
fn default_mutability() -> String {
    "immutable".into()
}
fn default_apply() -> String {
    "live".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    #[serde(rename = "schemaVersion", default = "one")]
    pub schema_version: u32,
    pub keys: Vec<KeyRecord>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    #[serde(rename = "artifactVersion")]
    pub artifact_version: u32,
    pub keys: Vec<KeyRecord>,
    #[serde(rename = "schemaVersions", default)]
    pub schema_versions: HashMap<String, u32>,
    #[serde(default)]
    pub templates: HashMap<String, Template>,
}

/// A parsed key id: `<schema>.<key>` or `<template>:<instance>.<key>` (§1.1; instances are
/// percent-escaped so they contain no `.` or `:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRef {
    pub schema: String,
    pub instance: Option<String>,
    pub key: String,
}

impl KeyRef {
    pub fn store_name(&self) -> String {
        match &self.instance {
            Some(i) => format!("{}:{}", self.schema, i),
            None => self.schema.clone(),
        }
    }
    pub fn id(&self) -> String {
        match &self.instance {
            Some(i) => format!("{}:{}.{}", self.schema, i, self.key),
            None => format!("{}.{}", self.schema, self.key),
        }
    }
}

impl Artifact {
    pub fn load(path: &Path) -> Result<Artifact, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let a: Artifact = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if a.artifact_version != 1 {
            return Err(format!("{}: artifactVersion {} is not 1", path.display(), a.artifact_version));
        }
        Ok(a)
    }

    /// The artifact's identity: the store path's basename (`<hash>-settings-schema.json`), which
    /// changes with every generation that changes the schema.
    pub fn generation_of(path: &Path) -> String {
        std::fs::canonicalize(path)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "unknown".into())
    }

    pub fn schema_version(&self, schema: &str) -> u32 {
        if let Some(t) = self.templates.get(schema) {
            return t.schema_version;
        }
        *self.schema_versions.get(schema).unwrap_or(&1)
    }

    /// Resolve an id to its record and its (schema, instance, key) parts.
    pub fn lookup(&self, id: &str) -> Option<(KeyRef, &KeyRecord)> {
        if let Some((tmpl, rest)) = id.split_once(':') {
            let t = self.templates.get(tmpl)?;
            let (instance, key) = rest.split_once('.')?;
            if instance.is_empty() || key.is_empty() {
                return None;
            }
            let rec = t.keys.iter().find(|k| k.key == key)?;
            return Some((KeyRef { schema: tmpl.to_string(), instance: Some(instance.to_string()), key: key.to_string() }, rec));
        }
        let rec = self.keys.iter().find(|k| k.id == id)?;
        Some((KeyRef { schema: rec.schema.clone(), instance: None, key: rec.key.clone() }, rec))
    }

    /// Every fixed key whose id starts with `prefix`.
    pub fn keys_with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a KeyRecord> + 'a {
        self.keys.iter().filter(move |k| k.id.starts_with(prefix))
    }
}

/// Validate a value against a record's type and constraints (§8 errors).
pub fn validate(rec: &KeyRecord, v: &Value) -> Result<(), Invalid> {
    match rec.ty.as_str() {
        "bool" => v.as_bool().map(|_| ()).ok_or(Invalid::Type),
        "int" => {
            let n = v.as_i64().ok_or(Invalid::Type)?;
            in_range(rec, n as f64)
        }
        "double" => {
            let n = v.as_f64().ok_or(Invalid::Type)?;
            in_range(rec, n)
        }
        "string" => v.as_str().map(|_| ()).ok_or(Invalid::Type),
        "enum" => {
            let s = v.as_str().ok_or(Invalid::Type)?;
            if rec.values.iter().any(|x| x == s) {
                Ok(())
            } else {
                Err(Invalid::Range)
            }
        }
        _ => Err(Invalid::Type),
    }
}

fn in_range(rec: &KeyRecord, n: f64) -> Result<(), Invalid> {
    match &rec.range {
        Some(r) if n < r.min || n > r.max => Err(Invalid::Range),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Type,
    Range,
}

#[cfg(test)]
pub fn fixture() -> Artifact {
    serde_json::from_str(FIXTURE).unwrap()
}

#[cfg(test)]
pub const FIXTURE: &str = r#"{
  "artifactVersion": 1,
  "keys": [
    {"id":"xr.passthrough.latencyMode","schema":"xr.passthrough","key":"latencyMode","type":"enum","values":["low-latency","high-quality"],"default":"low-latency","mutability":"mutable"},
    {"id":"xr.passthrough.enable","schema":"xr.passthrough","key":"enable","type":"bool","default":false,"mutability":"immutable"},
    {"id":"hardware.ipd.meters","schema":"hardware.ipd","key":"meters","type":"double","default":0.063,"mutability":"mutable","range":{"min":0.05,"max":0.08}},
    {"id":"shell.locked.thing","schema":"shell.locked","key":"thing","type":"int","default":3,"mutability":"mutable","locked":true}
  ],
  "schemaVersions": {"xr.passthrough": 1, "hardware.ipd": 1, "shell.locked": 1},
  "templates": {
    "places.entry": {"schemaVersion": 1, "keys": [
      {"key":"enabled","type":"bool","default":true,"mutability":"mutable"},
      {"key":"launch","type":"string","default":"","mutability":"mutable"}
    ]}
  }
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_fixed_and_instance_ids() {
        let a = fixture();
        let (r, k) = a.lookup("xr.passthrough.latencyMode").unwrap();
        assert_eq!(r.schema, "xr.passthrough");
        assert_eq!(r.instance, None);
        assert_eq!(k.ty, "enum");
        let (r, k) = a.lookup("places.entry:desk%2Eleft.enabled").unwrap();
        assert_eq!(r.instance.as_deref(), Some("desk%2Eleft"));
        assert_eq!(r.store_name(), "places.entry:desk%2Eleft");
        assert_eq!(k.key, "enabled");
        assert!(a.lookup("places.entry:x.nope").is_none());
        assert!(a.lookup("nope.key").is_none());
        assert!(a.lookup("places.entry:.enabled").is_none());
    }

    #[test]
    fn validation_by_type_and_constraint() {
        let a = fixture();
        let (_, lm) = a.lookup("xr.passthrough.latencyMode").unwrap();
        assert!(validate(lm, &Value::from("high-quality")).is_ok());
        assert_eq!(validate(lm, &Value::from("medium")), Err(Invalid::Range));
        assert_eq!(validate(lm, &Value::from(1)), Err(Invalid::Type));
        let (_, ipd) = a.lookup("hardware.ipd.meters").unwrap();
        assert!(validate(ipd, &Value::from(0.064)).is_ok());
        assert_eq!(validate(ipd, &Value::from(0.09)), Err(Invalid::Range));
        assert_eq!(validate(ipd, &Value::from("x")), Err(Invalid::Type));
    }
}
