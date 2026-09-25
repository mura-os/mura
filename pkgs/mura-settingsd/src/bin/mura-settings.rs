//! mura-settings — the CLI (specs/settings-daemon.md §8): the `gsettings` / `kwriteconfig` shape
//! for scripts, tests and the recovery shell.
//!
//!   mura-settings [--system] get <key>            → "<value>\t<provenance>"
//!   mura-settings [--system] set <key> <value>
//!   mura-settings [--system] reset <key>
//!   mura-settings [--system] list [prefix]
//!   mura-settings [--system] instances <template>
//!   mura-settings [--system] delete-instance <template> <instance>
//!   mura-settings [--system] generation
//!   mura-settings [--system] generation-changed   the NixOS activation hook: Reload on the daemon
//!   mura-settings --direct get|list …             read the artifact and the files, no bus
//!                                                 (the consumer discipline of settings-schema.md §7)
//! Exit: 0; 1 refused (locked/declarative/range/type); 2 unknown key or usage; 3 no bus.

use mura_settingsd::bus::{from_variant, render_json, to_variant};
use mura_settingsd::engine::{Error, Mode};
use mura_settingsd::{artifact_path, open, BUS_NAME, OBJECT_PATH};
use std::process::exit;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

fn usage() -> ! {
    eprintln!("usage: mura-settings [--system|--direct] <get|set|reset|list|instances|delete-instance|generation|generation-changed> …");
    exit(2)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode = Mode::Session;
    let mut direct = false;
    while let Some(a) = args.first() {
        match a.as_str() {
            "--system" => {
                mode = Mode::System;
                args.remove(0);
            }
            "--direct" => {
                direct = true;
                args.remove(0);
            }
            _ => break,
        }
    }
    let Some(cmd) = args.first().cloned() else { usage() };
    let rest = &args[1..];
    if direct {
        direct_cmd(mode, &cmd, rest);
    }
    let conn = match mode {
        Mode::Session => Connection::session(),
        Mode::System => Connection::system(),
    }
    .unwrap_or_else(|e| {
        eprintln!("mura-settings: no bus: {e} (use --direct to read the files)");
        exit(3)
    });
    let proxy = Proxy::new(&conn, BUS_NAME, OBJECT_PATH, BUS_NAME).unwrap_or_else(|e| {
        eprintln!("mura-settings: {e}");
        exit(3)
    });
    let r: Result<(), zbus::Error> = (|| {
        match (cmd.as_str(), rest) {
            ("get", [key]) => {
                let (v, p): (OwnedValue, String) = proxy.call("Get", &(key,))?;
                println!("{}\t{}", render_json(&from_variant(&v)), p);
            }
            ("set", [key, value]) => {
                let v = Value::Str(value.as_str().into()); // the daemon coerces per key type
                proxy.call::<_, _, ()>("Set", &(key, v))?;
            }
            ("reset", [key]) => proxy.call::<_, _, ()>("Reset", &(key,))?,
            ("list", []) | ("list", [_]) => {
                let prefix = rest.first().map(String::as_str).unwrap_or("");
                let rows: Vec<(String, OwnedValue, String)> = proxy.call("List", &(prefix,))?;
                for (id, v, p) in rows {
                    println!("{id}\t{}\t{p}", render_json(&from_variant(&v)));
                }
            }
            ("instances", [t]) => {
                let rows: Vec<String> = proxy.call("ListInstances", &(t,))?;
                for i in rows {
                    println!("{i}");
                }
            }
            ("delete-instance", [t, i]) => proxy.call::<_, _, ()>("DeleteInstance", &(t, i))?,
            ("generation", []) => {
                let g: String = proxy.call("GetGeneration", &())?;
                println!("{g}");
            }
            ("generation-changed", []) => {
                let g: String = proxy.call("Reload", &())?;
                println!("{g}");
            }
            _ => usage(),
        }
        Ok(())
    })();
    if let Err(e) = r {
        let name = match &e {
            zbus::Error::MethodError(n, _, _) => n.as_str().to_string(),
            _ => String::new(),
        };
        eprintln!("mura-settings: {e}");
        exit(match name.rsplit('.').next() {
            Some("UnknownKey") | Some("UnknownInstance") => 2,
            Some("Locked") | Some("Declarative") | Some("Type") | Some("Range") | Some("WrongBus") => 1,
            _ => 3,
        });
    }
}

/// `--direct`: the artifact and the files, no daemon (read-only).
fn direct_cmd(mode: Mode, cmd: &str, rest: &[String]) -> ! {
    let mut engine = open(mode, &artifact_path()).unwrap_or_else(|e| {
        eprintln!("mura-settings: {e}");
        exit(1)
    });
    let code = match (cmd, rest) {
        ("get", [key]) => match engine.get(key) {
            Ok(e) => {
                println!("{}", e.render());
                0
            }
            Err(Error::UnknownKey) | Err(Error::UnknownInstance) => 2,
            Err(_) => 1,
        },
        ("list", []) | ("list", [_]) => {
            for (id, e) in engine.list(rest.first().map(String::as_str).unwrap_or("")) {
                println!("{id}\t{}", e.render());
            }
            0
        }
        ("generation", []) => {
            println!("{}", engine.generation);
            0
        }
        _ => usage(),
    };
    // keep the variant helpers linked for the daemon-shaped output
    let _ = to_variant(&serde_json::Value::Null);
    exit(code)
}
