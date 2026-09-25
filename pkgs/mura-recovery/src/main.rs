//! mura-recovery — the recovery environment's one program (specs/recovery-menu.md).
//!
//!   mura-recovery action <status|factory-reset [--confirmed]|switch-slot|reboot|poweroff>
//!       The actions, the only place they live (§2). `factory-reset` refuses without --confirmed.
//!   mura-recovery panel [--config PATH]
//!       The panels + the HMD's buttons: raw evdev (register on release, long press ignored),
//!       the menu state machine, plymouth as the display (§3–§5). Runs for the life of stage 1.
//!   mura-recovery shell [--config PATH]
//!       The same menu as a numbered prompt over ssh or the console; `yes, erase` confirms.
//!
//! One Rust program, three ways in — `mura-setup --recovery` is the third: it execs
//! `mura-recovery action …` from its web page (§7).

mod actions;
mod config;
mod evdev;
mod menu;
mod plymouth;

use actions::Action;
use config::Config;
use menu::{Input, Menu, Screen};
use std::io::{BufRead, Write};
use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: mura-recovery action <status|factory-reset [--confirmed]|switch-slot|reboot|poweroff>\n       mura-recovery panel [--config PATH]\n       mura-recovery shell [--config PATH]");
    exit(2)
}

fn load_config(args: &[String]) -> Config {
    let path = args.windows(2).find(|w| w[0] == "--config").map(|w| w[1].clone()).unwrap_or_else(|| config::DEFAULT_PATH.to_string());
    match Config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mura-recovery: {e}; using built-in defaults");
            Config::fallback()
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("action") => {
            let Some(name) = args.get(1) else { usage() };
            let Some(action) = Action::parse(name) else { usage() };
            let confirmed = args.iter().any(|a| a == "--confirmed");
            let cfg = load_config(&args);
            exit(actions::run(action, confirmed, &cfg));
        }
        Some("panel") => panel(load_config(&args)),
        Some("shell") => shell(load_config(&args)),
        _ => usage(),
    }
}

/// What the panel currently shows, as text — for the shell over ssh and for the VM test, which
/// drives the keys through QEMU and cannot see the pixels.
const PANEL_TEXT: &str = "/run/mura-recovery/panel.txt";
const PANEL_STATUS: &str = "/run/mura-recovery/panel.status";

fn draw(screen: &mut plymouth::Screen, text: &str) {
    screen.draw(text);
    let _ = std::fs::create_dir_all("/run/mura-recovery");
    let _ = std::fs::write(PANEL_TEXT, text);
}

/// The panel frontend (§4–§5): stage-1 daemon.
fn panel(cfg: Config) -> ! {
    let mut screen = plymouth::Screen::new();
    let _ = std::fs::create_dir_all("/run/mura-recovery");
    let _ = std::fs::write(PANEL_STATUS, if screen.available { "plymouth\n" } else { "no-plymouth\n" });
    if !screen.available {
        eprintln!("mura-recovery panel: plymouth is not running; keys are still read (nothing to draw on)");
    }
    // the ways-in lines, drawn once and left standing
    screen.show_static(&actions::status_text(&cfg));
    let mut devices = evdev::Devices::open_all(cfg.long_press_ms);
    eprintln!("mura-recovery panel: {} input device(s)", devices.count());
    let mut menu = Menu::new(cfg.switch_slot_command.is_some());
    let details = actions::status_text(&cfg);
    draw(&mut screen, &menu.render(&details));
    loop {
        let Some(key) = devices.next_key() else {
            eprintln!("mura-recovery panel: no input devices; the shell and web frontends remain");
            std::thread::sleep(std::time::Duration::from_secs(3600));
            continue;
        };
        if key.press == evdev::Press::Long {
            continue; // §4 rule 2: long presses are reserved and ignored
        }
        let k = &cfg.keys;
        let on_confirm = menu.screen == Screen::Confirm;
        // §4: when backRole is volumeDown, the same key is Next on Main and Back on Confirm
        let input = if on_confirm && k.back.contains(&key.code) {
            Input::Back
        } else if k.select.contains(&key.code) {
            Input::Select
        } else if k.next.contains(&key.code) {
            Input::Next
        } else if k.prev.contains(&key.code) {
            Input::Prev
        } else if k.back.contains(&key.code) {
            Input::Back
        } else {
            continue;
        };
        if let Some(action) = menu.on(input) {
            let confirmed = action == Action::FactoryReset; // only Confirm's "Erase everything" yields it
            if action == Action::FactoryReset {
                draw(&mut screen, "Resetting this headset. Do not power off.");
            }
            let rc = actions::run(action, confirmed, &cfg);
            if rc != 0 {
                draw(&mut screen, &format!("The action failed (code {rc}). See the journal or the shell.\n{}", menu.render(&details)));
            } else {
                draw(&mut screen, &menu.render(&details));
            }
            continue;
        }
        draw(&mut screen, &menu.render(&details));
    }
}

/// The shell frontend (§7): the same menu as a numbered prompt.
fn shell(cfg: Config) -> ! {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let menu = Menu::new(cfg.switch_slot_command.is_some());
    loop {
        println!("{}", actions::status_text(&cfg));
        for (i, it) in menu.items.iter().enumerate() {
            println!("  {}) {}", i + 1, it.label());
        }
        print!("> ");
        let _ = out.flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            exit(0);
        }
        let Ok(n) = line.trim().parse::<usize>() else { continue };
        let Some(item) = menu.items.get(n.wrapping_sub(1)) else { continue };
        let action = match item {
            menu::Item::TryAgain => Action::Reboot,
            menu::Item::PowerOff => Action::PowerOff,
            menu::Item::SwitchSlot => Action::SwitchSlot,
            menu::Item::ShowDetails => continue,
            menu::Item::FactoryReset => {
                println!("{}", menu::CONFIRM_HEADER);
                print!("Type exactly: yes, erase\n> ");
                let _ = out.flush();
                let mut answer = String::new();
                let _ = stdin.lock().read_line(&mut answer);
                if answer.trim() != "yes, erase" {
                    println!("Not erased.");
                    continue;
                }
                Action::FactoryReset
            }
        };
        let rc = actions::run(action, action == Action::FactoryReset, &cfg);
        if rc != 0 {
            println!("The action failed (code {rc}).");
        }
    }
}
