//! mura-settingsd — the daemon (specs/settings-daemon.md §2–§3).
//!
//!   mura-settingsd            session bus, the per-user store (Type=dbus, activated by the bus)
//!   mura-settingsd --system   system bus, root, device keys (reserved until a target declares one)
//!
//! Resident once started: it is the notifier, and an exited daemon cannot observe the generation
//! switch for its subscribers (§3). Stays single-threaded at the interface: one call at a time.

use mura_settingsd::bus::Service;
use mura_settingsd::engine::Mode;
use mura_settingsd::{artifact_path, open, BUS_NAME, OBJECT_PATH};
use zbus::blocking::connection;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        None => Mode::Session,
        Some("--system") => Mode::System,
        Some(_) => {
            eprintln!("usage: mura-settingsd [--system]");
            std::process::exit(2);
        }
    };
    let path = artifact_path();
    let engine = match open(mode, &path) {
        Ok(e) => e,
        Err(e) => {
            // constraint 9: a missing or corrupt artifact is an explicit failure, not a default
            eprintln!("mura-settingsd: {e}");
            std::process::exit(1);
        }
    };
    let (device_keys, served) = {
        let all = engine.artifact.keys.len();
        (engine.artifact.keys.iter().filter(|k| k.stratum == "device").count(), all)
    };
    if mode == Mode::System && device_keys == 0 {
        eprintln!("mura-settingsd --system: the artifact declares no device key; nothing to serve (settings-schema.md §2.1)");
        std::process::exit(1);
    }
    eprintln!("mura-settingsd: {:?} mode, generation {}, {} keys in the artifact ({} device)", mode, engine.generation, served, device_keys);

    let builder = match mode {
        Mode::Session => connection::Builder::session(),
        Mode::System => connection::Builder::system(),
    };
    let _conn = builder
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| b.serve_at(OBJECT_PATH, Service { engine, artifact_path: path }))
        .and_then(|b| b.build())
        .unwrap_or_else(|e| {
            eprintln!("mura-settingsd: cannot serve {BUS_NAME}: {e}");
            std::process::exit(1);
        });
    // the executor runs in zbus's thread; this thread only waits for the manager's SIGTERM
    loop {
        std::thread::park();
    }
}
