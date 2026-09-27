//! `org.mura.Settings1` (specs/settings-schema.md §8) on top of the engine. Single-threaded: one
//! call at a time, so "coalescing per event-loop turn" is per call — each accepted Set/Reset
//! emits at most one Changed (settings-daemon.md §6).

use crate::artifact::Artifact;
use crate::engine::{Effective, Engine, Error};
use serde_json::Value as Json;
use std::path::PathBuf;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.mura.Settings1.Error")]
pub enum BusError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Locked(String),
    Immutable(String),
    Type(String),
    Range(String),
    UnknownKey(String),
    UnknownInstance(String),
    WrongBus(String),
    Io(String),
}

impl From<Error> for BusError {
    fn from(e: Error) -> BusError {
        let m = e.message();
        match e {
            Error::Locked => BusError::Locked(m),
            Error::Immutable => BusError::Immutable(m),
            Error::Type => BusError::Type(m),
            Error::Range => BusError::Range(m),
            Error::UnknownKey => BusError::UnknownKey(m),
            Error::UnknownInstance => BusError::UnknownInstance(m),
            Error::WrongBus => BusError::WrongBus(m),
            Error::Io(_) => BusError::Io(m),
        }
    }
}

/// JSON (the artifact's and the stores' representation) → a typed D-Bus variant.
pub fn to_variant(v: &Json) -> OwnedValue {
    let val: Value<'static> = match v {
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) if n.is_i64() => Value::I64(n.as_i64().unwrap()),
        Json::Number(n) if n.is_u64() => Value::I64(n.as_u64().unwrap() as i64),
        Json::Number(n) => Value::F64(n.as_f64().unwrap_or(0.0)),
        Json::String(s) => Value::Str(s.clone().into()),
        Json::Null => Value::Str("".into()),
        other => Value::Str(other.to_string().into()),
    };
    OwnedValue::try_from(val).expect("scalar variants are always convertible")
}

/// A D-Bus variant → JSON for the engine (the engine coerces strings per key type).
pub fn from_variant(v: &Value<'_>) -> Json {
    match v {
        Value::Bool(b) => Json::from(*b),
        Value::U8(n) => Json::from(*n),
        Value::I16(n) => Json::from(*n),
        Value::U16(n) => Json::from(*n),
        Value::I32(n) => Json::from(*n),
        Value::U32(n) => Json::from(*n),
        Value::I64(n) => Json::from(*n),
        Value::U64(n) => Json::from(*n),
        Value::F64(n) => Json::from(*n),
        Value::Str(s) => Json::from(s.as_str()),
        Value::Value(inner) => from_variant(inner),
        other => Json::from(other.to_string()),
    }
}

pub struct Service {
    pub engine: Engine,
    pub artifact_path: PathBuf,
}

#[zbus::interface(name = "org.mura.Settings1")]
impl Service {
    /// `Get(s key) → (v value, s provenance)`
    fn get(&mut self, key: &str) -> Result<(OwnedValue, String), BusError> {
        let e = self.engine.get(key)?;
        Ok((to_variant(&e.value), e.provenance.to_string()))
    }

    /// `Set(s key, v value)`: durable before the reply; `Changed` only if the effective value or
    /// its provenance moved.
    async fn set(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>, key: &str, value: Value<'_>) -> Result<(), BusError> {
        let json = from_variant(&value);
        if let Some(e) = self.engine.set(key, json)? {
            Self::changed(&emitter, key, to_variant(&e.value), e.provenance).await?;
        }
        Ok(())
    }

    async fn reset(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>, key: &str) -> Result<(), BusError> {
        if let Some(e) = self.engine.reset(key)? {
            Self::changed(&emitter, key, to_variant(&e.value), e.provenance).await?;
        }
        Ok(())
    }

    /// `List(s prefix) → a(svs)`
    fn list(&mut self, prefix: &str) -> Vec<(String, OwnedValue, String)> {
        self.engine.list(prefix).into_iter().map(|(id, e)| (id, to_variant(&e.value), e.provenance.to_string())).collect()
    }

    fn list_instances(&self, template: &str) -> Result<Vec<String>, BusError> {
        Ok(self.engine.list_instances(template)?)
    }

    async fn delete_instance(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>, template: &str, instance: &str) -> Result<(), BusError> {
        let gone = self.engine.delete_instance(template, instance)?;
        for id in gone {
            if let Ok(e) = self.engine.get(&id) {
                Self::changed(&emitter, &id, to_variant(&e.value), e.provenance).await?;
            }
        }
        Ok(())
    }

    fn get_generation(&self) -> String {
        self.engine.generation.clone()
    }

    /// The generation hook (settings-daemon.md §3): re-read the artifact, signal every moved key,
    /// then `GenerationChanged`. Returns the new generation.
    async fn reload(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> Result<String, BusError> {
        let artifact = Artifact::load(&self.artifact_path).map_err(BusError::Io)?;
        let generation = Artifact::generation_of(&self.artifact_path);
        let changed = self.engine.reload(artifact, generation.clone()).map_err(BusError::Io)?;
        for (id, e) in changed {
            Self::changed(&emitter, &id, to_variant(&e.value), e.provenance).await?;
        }
        Self::generation_changed(&emitter, &generation).await?;
        Ok(generation)
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>, key: &str, value: OwnedValue, provenance: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn generation_changed(emitter: &SignalEmitter<'_>, generation: &str) -> zbus::Result<()>;
}

impl Effective {
    pub fn render(&self) -> String {
        format!("{}\t{}", render_json(&self.value), self.provenance)
    }
}

pub fn render_json(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}
