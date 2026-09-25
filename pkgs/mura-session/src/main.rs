//! mura-session — the session wrapper greetd execs (specs/session-bootstrap.md §4).
//!
//! `mura-session start [--target NAME]`
//!   1. wait for the user manager (pam_systemd started it);
//!   2. write $XDG_RUNTIME_DIR/mura/session.env with the login session's identity
//!      (XDG_SESSION_ID / XDG_SEAT / XDG_VTNR) — the compositor unit's EnvironmentFile, which is
//!      how libseat's logind backend finds the session from inside a user unit (the seat
//!      mechanism, review finding F1) — and import an EXPLICIT list of static variables into the
//!      user manager (never a bare import-environment);
//!   3. start mura-session-bindpid@<own pid>.service, so a dead wrapper ends the session;
//!   4. `systemctl --user start --wait mura-session.target` and stay alive for the session's
//!      lifetime; SIGTERM/SIGHUP/SIGINT (greetd or logind tearing the login down) → stop the
//!      target and keep waiting for that stop;
//!   5. after the wait returns: unset what we exported (and the compositor-created variables),
//!      remove session.env, exit 0. The exit status is not the failure signal — the unit result
//!      and the journal are (§4 step 7).
//!
//! `mura-session finalize [VAR…]`
//!   The STAND-IN compositor's readiness hook (sway, until zxr notifies natively at M1): export
//!   WAYLAND_DISPLAY (+ DISPLAY when set) and the named variables to the user manager and the
//!   D-Bus activation environment, then sd_notify(READY=1) on the inherited NOTIFY_SOCKET.
//!
//! This is uwsm's mechanism (verified at D4) in ~400 lines of Rust with libc, over static units:
//! no interpreter on the login path, no unit generation, no daemon-reload.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::{exit, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SYSTEMCTL: &str = match option_env!("MURA_SYSTEMCTL") {
    Some(p) => p,
    None => "systemctl",
};
const DBUS_UPDATE_ENV: &str = match option_env!("MURA_DBUS_UPDATE_ENV") {
    Some(p) => p,
    None => "dbus-update-activation-environment",
};

/// Session-specific class (§3): reaches the compositor UNIT only, never the manager.
const SESSION_VARS: &[&str] = &["XDG_SESSION_ID", "XDG_SEAT", "XDG_VTNR"];
/// Static class imported into the user manager (§3) — an explicit list, uwsm's always_export.
const STATIC_VARS: &[&str] = &["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "XDG_SESSION_CLASS", "XDG_SESSION_TYPE"];
/// Compositor-created class (§3): published by `finalize`, unset at teardown.
const COMPOSITOR_VARS: &[&str] = &["WAYLAND_DISPLAY", "DISPLAY"];

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

fn log(msg: &str) {
    eprintln!("mura-session: {msg}");
}

fn systemctl(args: &[&str]) -> bool {
    match Command::new(SYSTEMCTL).arg("--user").args(args).status() {
        Ok(s) if s.success() => true,
        Ok(s) => {
            log(&format!("systemctl --user {} failed: {s}", args.join(" ")));
            false
        }
        Err(e) => {
            log(&format!("cannot run systemctl: {e}"));
            false
        }
    }
}

fn systemctl_output(args: &[&str]) -> String {
    Command::new(SYSTEMCTL)
        .arg("--user")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn runtime_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        log("XDG_RUNTIME_DIR is not set (pam_systemd did not run?)");
        exit(1)
    })
}

// ---------------------------------------------------------------- start

extern "C" fn on_signal(_sig: libc::c_int) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

fn start(target: &str) -> ! {
    // 1. the user manager
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let state = systemctl_output(&["is-system-running"]);
        if state == "running" || state == "degraded" || state == "starting" {
            break;
        }
        if Instant::now() > deadline {
            log(&format!("user manager not running (state {state:?})"));
            exit(1);
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // 2. session.env for the compositor unit; static class into the manager
    let dir = format!("{}/mura", runtime_dir());
    if let Err(e) = fs::create_dir_all(&dir) {
        log(&format!("cannot create {dir}: {e}"));
        exit(1);
    }
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let env_path = format!("{dir}/session.env");
    let mut body = String::new();
    for v in SESSION_VARS {
        if let Ok(val) = std::env::var(v) {
            if !val.is_empty() && !val.contains('\n') {
                body.push_str(&format!("{v}={val}\n"));
            }
        }
    }
    match fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&env_path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(body.as_bytes()) {
                log(&format!("cannot write {env_path}: {e}"));
                exit(1);
            }
        }
        Err(e) => {
            log(&format!("cannot open {env_path}: {e}"));
            exit(1);
        }
    }
    // The desktop identity and the session type are Mura's to state (uwsm's -D and its fixed
    // XDG_SESSION_TYPE=wayland): greetd's login is a `tty` session to PAM, and importing that
    // would overwrite environment.d's `wayland` in the manager (found at D4 rev 3). Only the
    // session class is taken from the login environment.
    let mut imports: Vec<String> = vec![
        "XDG_CURRENT_DESKTOP=mura".into(),
        "XDG_SESSION_DESKTOP=mura".into(),
        "XDG_SESSION_TYPE=wayland".into(),
    ];
    if let Ok(val) = std::env::var("XDG_SESSION_CLASS") {
        if !val.is_empty() {
            imports.push(format!("XDG_SESSION_CLASS={val}"));
        }
    }
    let import_refs: Vec<&str> = imports.iter().map(String::as_str).collect();
    systemctl(&[&["set-environment"][..], &import_refs[..]].concat());

    // 3. bind the session to this process
    let bind = format!("mura-session-bindpid@{}.service", std::process::id());
    if !systemctl(&["start", &bind]) {
        log("bindpid unit did not start; continuing without it");
    }

    // 4. the session body; signals stop it and we keep waiting
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_signal as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as libc::sighandler_t);
    }
    log(&format!("starting {target} and waiting while it runs"));
    let mut child = match Command::new(SYSTEMCTL)
        .args(["--user", "start", "--wait", target])
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log(&format!("cannot start {target}: {e}"));
            exit(1);
        }
    };
    let mut stop_sent = false;
    let status = loop {
        if STOP_REQUESTED.load(Ordering::SeqCst) && !stop_sent {
            stop_sent = true;
            log("signal received; stopping the session target");
            let _ = Command::new(SYSTEMCTL).args(["--user", "stop", "--no-block", target]).status();
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                log(&format!("waiting for systemctl failed: {e}"));
                exit(1);
            }
        }
    };
    log(&format!("{target} is down ({status})"));

    // 5. teardown: our exports and the compositor's, then the env file
    let mut unset: Vec<&str> = STATIC_VARS.to_vec();
    unset.extend_from_slice(COMPOSITOR_VARS);
    systemctl(&[&["unset-environment"][..], &unset[..]].concat());
    let _ = fs::remove_file(&env_path);
    exit(0);
}

// ---------------------------------------------------------------- finalize

fn sd_notify_ready() {
    let Ok(sock) = std::env::var("NOTIFY_SOCKET") else {
        log("finalize: NOTIFY_SOCKET is not set; not inside the compositor unit?");
        return;
    };
    let mut path = sock.clone().into_bytes();
    let abstract_ns = path.first() == Some(&b'@');
    if abstract_ns {
        path[0] = 0;
    }
    unsafe {
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            log("finalize: socket() failed");
            return;
        }
        let mut addr: libc::sockaddr_un = std::mem::zeroed();
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let n = path.len().min(addr.sun_path.len() - 1);
        for (i, b) in path[..n].iter().enumerate() {
            addr.sun_path[i] = *b as libc::c_char;
        }
        let len = std::mem::size_of::<libc::sa_family_t>() + n + if abstract_ns { 0 } else { 1 };
        let msg = b"READY=1\n";
        let r = libc::sendto(
            fd,
            msg.as_ptr() as *const libc::c_void,
            msg.len(),
            0,
            &addr as *const libc::sockaddr_un as *const libc::sockaddr,
            len as libc::socklen_t,
        );
        libc::close(fd);
        if r < 0 {
            log("finalize: sd_notify READY=1 failed");
        }
    }
}

fn finalize(extra: &[String]) -> ! {
    let mut names: Vec<String> = COMPOSITOR_VARS.iter().map(|s| s.to_string()).collect();
    names.extend(extra.iter().cloned());
    let mut assignments = Vec::new();
    let mut present = Vec::new();
    for n in &names {
        if let Ok(v) = std::env::var(n) {
            if !v.is_empty() {
                assignments.push(format!("{n}={v}"));
                present.push(n.clone());
            }
        }
    }
    if std::env::var("WAYLAND_DISPLAY").map(|v| v.is_empty()).unwrap_or(true) {
        log("finalize: WAYLAND_DISPLAY is not set; the compositor is not ready");
        exit(1);
    }
    let refs: Vec<&str> = assignments.iter().map(String::as_str).collect();
    systemctl(&[&["set-environment"][..], &refs[..]].concat());
    let present_refs: Vec<&str> = present.iter().map(String::as_str).collect();
    let _ = Command::new(DBUS_UPDATE_ENV).arg("--systemd").args(&present_refs).status();
    sd_notify_ready();
    log(&format!("finalize: published {}", present.join(" ")));
    exit(0);
}

// ---------------------------------------------------------------- main

fn usage() -> ! {
    eprintln!("usage: mura-session start [--target UNIT] | mura-session finalize [VAR...]");
    exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("start") => {
            let mut target = String::from("mura-session.target");
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--target" => target = args.next().unwrap_or_else(|| usage()),
                    "--" => break,
                    _ => usage(),
                }
            }
            start(&target)
        }
        Some("finalize") => finalize(&args.collect::<Vec<_>>()),
        _ => usage(),
    }
}
