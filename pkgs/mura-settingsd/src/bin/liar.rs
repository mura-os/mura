//! mura-settingsd-liar — TEST-ONLY (specs/settings-schema.md §9 item 8; §7). Owns
//! `org.mura.Settings1` on the session bus and answers every `Get` with a wrong value, standing in
//! for a compromised daemon. A consumer of a locked key that follows §7 (the artifact, not the
//! bus — `mura-settings --direct`) is unaffected; one that trusts the bus is fooled. Ships in the
//! package for the VM test only, like `mura-authd-harness`.

use zbus::blocking::connection;
use zbus::zvariant::{OwnedValue, Value};

struct Liar;

#[zbus::interface(name = "org.mura.Settings1")]
impl Liar {
    fn get(&self, _key: &str) -> (OwnedValue, String) {
        (OwnedValue::try_from(Value::I64(999)).unwrap(), "user".to_string())
    }
    fn get_generation(&self) -> String {
        "liar".to_string()
    }
}

fn main() {
    let _conn = connection::Builder::session()
        .and_then(|b| b.name(mura_settingsd::BUS_NAME))
        .and_then(|b| b.serve_at(mura_settingsd::OBJECT_PATH, Liar))
        .and_then(|b| b.build())
        .unwrap_or_else(|e| {
            eprintln!("mura-settingsd-liar: {e}");
            std::process::exit(1);
        });
    eprintln!("mura-settingsd-liar: lying on {}", mura_settingsd::BUS_NAME);
    loop {
        std::thread::park();
    }
}
