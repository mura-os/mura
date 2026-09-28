//! mura-greeter — the greeter and lock program (specs/session-auth.md rev 6 §5; shell-plane §3.1;
//! research/78).
//!
//! **Greeter mode** (`mura-greeter`): greetd's kiosk child. greetd runs `zxr --greeter`, zxr
//! spawns this program over a socketpair (`WAYLAND_SOCKET`); the scene maps as a layer-shell
//! `overlay`/exclusive surface (cosmic-greeter's, gtkgreet `-l`'s), speaks greetd over
//! `$GREETD_SOCK` itself, and exits 0 after `start_session` — zxr exits with it (cage's rule) and
//! greetd starts the session.
//!
//! **Lock mode** (`mura-greeter --lock`): a resident user unit on the public socket. It waits for
//! logind's `Session.Lock`, locks through `ext-session-lock-v1`, reports `SetLockedHint` after
//! zxr's `locked` (I2), owns its `mura-authd` conversation, and unlocks with `unlock_and_destroy`
//! (swaylock's and cosmic-greeter's shape). `--lock-now` locks at start and exits after the
//! unlock — cosmic's fallback when logind has no session to wait on, and the nested harness's
//! path.
//!
//! Budget: one process; resident only in lock mode, where idle is one thread parked on the bus
//! and a one-second clock timer while a surface is mapped; the scene is redrawn only when Slint
//! asks (research/78 §7).

mod accounts;
mod app;
mod authd;
mod conv;
mod greetd;
mod logind;
mod sessions;

slint::include_modules!();

const USAGE: &str = "mura-greeter [--lock [--lock-now]]";

fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into())).with_writer(std::io::stderr).init();
    let mut lock = false;
    let mut lock_now = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--lock" => lock = true,
            "--lock-now" => {
                lock = true;
                lock_now = true;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("mura-greeter: unknown argument {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    let mode = if lock { app::Mode::Lock { lock_now } } else { app::Mode::Greeter };
    if let Err(e) = run(mode) {
        eprintln!("mura-greeter: {e}");
        std::process::exit(1);
    }
}

fn run(mode: app::Mode) -> Result<(), Box<dyn std::error::Error>> {
    let role = match mode {
        app::Mode::Greeter => mura_slint_platform::Role::Layer {
            layer: mura_slint_platform::Layer::Overlay,
            anchor: mura_slint_platform::Anchor::all(),
            // zone 0, not cosmic-greeter's -1: the OSK is a sibling layer surface here (shell-plane §3.2),
            // and a neutral zone lets the compositor size the scene to the area the OSK leaves —
            // the power and accessibility controls stay reachable while it is up (research/78 §7b)
            exclusive_zone: 0,
            keyboard: mura_slint_platform::KeyboardInteractivity::Exclusive,
            namespace: "mura-greeter".into(),
            size: (0, 0),
        },
        app::Mode::Lock { .. } => mura_slint_platform::Role::SessionLock,
    };
    let handle = mura_slint_platform::init(mura_slint_platform::Config { role, app_id: "mura-greeter".into() })?;
    let ui = Greeter::new()?;
    let _app = app::App::install(ui.clone_strong(), mode, handle);
    ui.show()?;
    slint::run_event_loop()?;
    Ok(())
}

use slint::ComponentHandle;
