//! Schema migrations (specs/settings-schema.md §5; settings-daemon.md §7): numbered Rust steps
//! keyed by (schema, fromVersion) — kconf_update's, snapd's and Android's shape, none of which
//! ship a migration language. Every step is ADDITIVE: it may add keys and bump the header; it may
//! not remove or overwrite an existing key, so the previous generation still reads its keys after
//! a rollback (NixOS's mkStateRevisionOption warning, research/58 §13.2).

use crate::store::StoreFile;

/// A step from `from` to `from + 1` for `schema`.
pub type Step = fn(&mut StoreFile);

pub struct Migration {
    pub schema: &'static str,
    pub from: u32,
    pub step: Step,
}

/// The table. Empty at rev 1: every exported schema is at version 1. A schema bump in Nix
/// without a step here is refused at daemon start (`highest_known` below).
pub const MIGRATIONS: &[Migration] = &[];

/// Run the steps that bring `store` from its header version up to `target`, in order. Returns
/// how many ran. Stops (and reports) if a step is missing — the store keeps the version it
/// reached; the caller resolves with what is there.
pub fn run(table: &[Migration], store: &mut StoreFile, target: u32) -> Result<u32, u32> {
    let mut ran = 0;
    while store.schema_version < target {
        let from = store.schema_version;
        let Some(m) = table.iter().find(|m| m.schema == store.schema && m.from == from) else {
            return Err(from);
        };
        let before: Vec<(String, serde_json::Value)> = store.values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        (m.step)(store);
        // the additive rule, enforced: nothing removed, nothing overwritten
        for (k, v) in before {
            match store.values.get(&k) {
                Some(now) if *now == v => {}
                _ => panic!("migration ({}, {from}) violated the additive rule on key {k}", m.schema),
            }
        }
        store.schema_version = from + 1;
        ran += 1;
    }
    Ok(ran)
}

/// The highest version this binary can bring `schema` to.
pub fn highest_known(table: &[Migration], schema: &str) -> u32 {
    table.iter().filter(|m| m.schema == schema).map(|m| m.from + 1).max().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn rename_latency(store: &mut StoreFile) {
        // rename `latency` → `latencyMode`: write beside, never remove (additive)
        if let Some(v) = store.values.get("latency").cloned() {
            store.values.entry("latencyMode").or_insert(v);
        }
    }
    const TEST_TABLE: &[Migration] = &[Migration { schema: "xr.passthrough", from: 1, step: rename_latency }];

    #[test]
    fn rename_keeps_the_old_key() {
        let mut s = StoreFile::empty("xr.passthrough", None, 1, "g");
        s.values.insert("latency".into(), Value::from("high-quality"));
        assert_eq!(run(TEST_TABLE, &mut s, 2), Ok(1));
        assert_eq!(s.schema_version, 2);
        assert_eq!(s.values["latency"], Value::from("high-quality"));
        assert_eq!(s.values["latencyMode"], Value::from("high-quality"));
    }

    #[test]
    fn missing_step_stops_at_the_reached_version() {
        let mut s = StoreFile::empty("xr.passthrough", None, 1, "g");
        assert_eq!(run(TEST_TABLE, &mut s, 3), Err(2));
        assert_eq!(s.schema_version, 2);
        assert_eq!(highest_known(TEST_TABLE, "xr.passthrough"), 2);
        assert_eq!(highest_known(TEST_TABLE, "other"), 1);
    }

    #[test]
    fn already_current_runs_nothing() {
        let mut s = StoreFile::empty("xr.passthrough", None, 2, "g");
        assert_eq!(run(TEST_TABLE, &mut s, 2), Ok(0));
    }
}
