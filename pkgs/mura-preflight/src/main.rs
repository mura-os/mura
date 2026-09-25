//! mura-preflight — the XR preflight probe of implementation-path §3a-bis (B1b), run as a system
//! unit before greetd (modules/os/health.nix). Exit 0: all pass; 1: a soft check failed (the
//! greeter starts, the result is exposed); 2: a hard check failed (this boot gets no greeter or
//! session; the crash-loop counter is another unit's — the probe never writes persistent state).
//!
//! Usage: `mura-preflight <config.json>` — the module renders the contract into that file:
//! codename, calibrationPaths, displayBackend, trackingSimulated, selectKey, deviceWaitSeconds,
//! monadoRuntime, persistClasses, and the two helper binaries (vulkaninfo, monadoCli).
//! Report: `/run/mura/preflight.json` = {codename, result, checks:[{check, pass, class, detail}]}.
//! Journal: one `PASS ` / `FAIL ` / `WARN ` line per check (the VM tests grep them).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Config {
    codename: String,
    #[serde(rename = "calibrationPaths")]
    calibration_paths: HashMap<String, String>,
    #[serde(rename = "displayBackend")]
    display_backend: String,
    #[serde(rename = "trackingSimulated")]
    tracking_simulated: bool,
    #[serde(rename = "selectKey")]
    select_key: Option<String>,
    #[serde(rename = "deviceWaitSeconds")]
    device_wait_seconds: u64,
    #[serde(rename = "monadoRuntime")]
    monado_runtime: bool,
    #[serde(rename = "persistClasses")]
    persist_classes: Vec<String>,
    vulkaninfo: String,
    #[serde(rename = "monadoCli")]
    monado_cli: String,
}

#[derive(Serialize)]
struct CheckResult {
    check: &'static str,
    pass: bool,
    class: &'static str,
    detail: String,
}

#[derive(Serialize)]
struct Report {
    codename: String,
    result: i32,
    checks: Vec<CheckResult>,
}

struct Results(Vec<CheckResult>);

impl Results {
    fn check(&mut self, check: &'static str, pass: bool, hard: bool, detail: impl Into<String>) {
        self.0.push(CheckResult { check, pass, class: if hard { "hard" } else { "soft" }, detail: detail.into() });
    }
}

/// Minimal glob: `dir/prefix*suffix` one level deep (what the sysfs/IIO checks need).
fn glob1(dir: &str, prefix: &str, suffix: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(prefix) && name.ends_with(suffix) && name.len() >= prefix.len() + suffix.len() {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

fn read_trim(p: &Path) -> String {
    fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn run(cmd: &str, args: &[&str], env: &[(&str, &str)], timeout: Duration) -> (bool, String) {
    let mut c = Command::new(cmd);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => return (false, format!("spawn {cmd}: {e}")),
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child.wait_with_output().unwrap_or_else(|_| std::process::Output {
                    status,
                    stdout: vec![],
                    stderr: vec![],
                });
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                return (status.success(), text);
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return (false, format!("{cmd} timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return (false, format!("wait {cmd}: {e}")),
        }
    }
}

fn keycode(name: &str) -> Option<u32> {
    Some(match name {
        "KEY_POWER" => 116,
        "KEY_VOLUMEUP" => 115,
        "KEY_VOLUMEDOWN" => 114,
        "KEY_SELECT" => 353,
        "KEY_ENTER" => 28,
        _ => return None,
    })
}

/// `B: KEY=...` lines are space-separated hex words, most significant first, each 64 bits.
fn key_bit_set(words: &[&str], bit: u32) -> bool {
    let n = words.len();
    let word_idx_from_lsb = (bit / 64) as usize;
    if word_idx_from_lsb >= n {
        return false;
    }
    let word = words[n - 1 - word_idx_from_lsb];
    u64::from_str_radix(word, 16).map(|w| (w >> (bit % 64)) & 1 == 1).unwrap_or(false)
}

fn main() {
    let cfg_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: mura-preflight <config.json>");
        std::process::exit(2)
    });
    let cfg: Config = serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap_or_default()).unwrap_or_else(|e| {
        eprintln!("mura-preflight: bad config {cfg_path}: {e}");
        std::process::exit(2)
    });
    let wait = Duration::from_secs(cfg.device_wait_seconds);
    let mut r = Results(Vec::new());

    // P1 persist: writable, class dirs present
    {
        let probe = Path::new("/var/lib/mura/.preflight-probe");
        match fs::write(probe, "ok").and_then(|_| fs::remove_file(probe)) {
            Ok(()) => {
                let missing: Vec<&String> = cfg.persist_classes.iter().filter(|c| !Path::new("/var/lib/mura").join(c).is_dir()).collect();
                if missing.is_empty() {
                    r.check("P1 persist", true, true, "writable; classes present");
                } else {
                    r.check("P1 persist", false, true, format!("missing: {missing:?}"));
                }
            }
            Err(e) => r.check("P1 persist", false, true, e.to_string()),
        }
    }

    // P2 factory calibration: every declared path exists and is non-empty
    if cfg.calibration_paths.is_empty() {
        r.check("P2 factory calibration", true, true, "none declared (calibration.paths empty)");
    } else {
        let bad: Vec<String> = cfg
            .calibration_paths
            .iter()
            .filter(|(_, p)| fs::metadata(p).map(|m| m.len() == 0).unwrap_or(true))
            .map(|(k, p)| format!("{k}: {p}"))
            .collect();
        if bad.is_empty() {
            r.check("P2 factory calibration", true, true, format!("{} file(s) present; version check is the runtime's", cfg.calibration_paths.len()));
        } else {
            r.check("P2 factory calibration", false, true, format!("missing/empty: {bad:?}"));
        }
    }

    // P3 display path: a connected connector (vk-display) or any DRM card (window backend)
    {
        let cards: Vec<PathBuf> = glob1("/sys/class/drm", "card", "").into_iter().filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            n.len() > 4 && n[4..].chars().all(|c| c.is_ascii_digit())
        }).collect();
        let connected: Vec<String> = glob1("/sys/class/drm", "card", "")
            .into_iter()
            .filter(|p| p.file_name().unwrap().to_string_lossy().contains('-'))
            .filter(|p| read_trim(&p.join("status")) == "connected")
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        if cfg.display_backend == "window" {
            r.check("P3 display path", !cards.is_empty(), true, format!("{} DRM card(s); window backend", cards.len()));
        } else {
            r.check("P3 display path", !connected.is_empty(), true, format!("connected connectors: {connected:?}"));
        }
    }

    // P4 Vulkan: a physical device the runtime can create (vulkaninfo, a native helper)
    {
        let (ok, out) = run(&cfg.vulkaninfo, &["--summary"], &[], Duration::from_secs(30));
        let devs: Vec<String> = out.lines().filter(|l| l.contains("deviceName")).map(|l| l.trim().to_string()).collect();
        let detail = if devs.is_empty() { out.chars().rev().take(200).collect::<String>().chars().rev().collect() } else { devs.join("; ") };
        r.check("P4 vulkan", ok && !devs.is_empty(), true, detail);
    }

    // P5 tracking nodes: an IIO accel+gyro within the device wait, unless the runtime simulates.
    // SOFT (with P6): everything that waits for a hardware class at boot waits ~10 s and then
    // proceeds degraded — GDM for a primary GPU, postmarketOS for a framebuffer; systemd's
    // guidance is "warn or report failure after a timeout, tailored to the hardware type"
    // (research/56 §5, ruled 2026-09-25). The greeter starts; the report carries the result.
    if cfg.tracking_simulated {
        r.check("P5 tracking nodes", true, false, "simulated tracking (SIMULATED_ENABLE)");
    } else {
        let deadline = Instant::now() + wait;
        let mut found: Vec<String> = Vec::new();
        loop {
            found = glob1("/sys/bus/iio/devices", "iio:device", "")
                .into_iter()
                .filter(|d| {
                    let dir = d.to_string_lossy().into_owned();
                    !glob1(&dir, "in_accel_", "_raw").is_empty() && !glob1(&dir, "in_anglvel_", "_raw").is_empty()
                })
                .map(|d| d.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            if !found.is_empty() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        r.check("P5 tracking nodes", !found.is_empty(), false, format!("iio: {found:?}"));
    }

    // P6 Monado probe: drivers initialise within the wait (first frame is the blessing tier's).
    // monado reads HOME/XDG_* for its config and dies on a NULL env (found at D6): give it a
    // runtime-only home.
    if cfg.monado_runtime {
        let home = "/run/mura/preflight";
        let _ = fs::create_dir_all(home);
        let _ = fs::set_permissions(home, std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let sim = if cfg.tracking_simulated { "true" } else { "false" };
        let env = [
            ("SIMULATED_ENABLE", sim),
            ("XRT_NO_STDIN", "1"),
            ("HOME", home),
            ("XDG_CONFIG_HOME", home),
            ("XDG_CACHE_HOME", home),
            ("XDG_RUNTIME_DIR", home),
        ];
        let (ok, out) = run(&cfg.monado_cli, &["probe"], &env, wait);
        let last = out.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        r.check("P6 monado probe", ok, false, if last.is_empty() { "no output".to_string() } else { last });
    } else {
        r.check("P6 monado probe", true, false, "runtime is not monado");
    }

    // P7 input floor: an evdev device exposing the select key, or a keyboard (soft)
    {
        let want = cfg.select_key.as_deref().and_then(keycode);
        let enter = keycode("KEY_ENTER").unwrap();
        let mut have = false;
        if let Ok(devs) = fs::read_to_string("/proc/bus/input/devices") {
            for line in devs.lines() {
                if let Some(rest) = line.strip_prefix("B: KEY=") {
                    let words: Vec<&str> = rest.split_whitespace().collect();
                    if want.map(|w| key_bit_set(&words, w)).unwrap_or(false) || key_bit_set(&words, enter) {
                        have = true;
                    }
                }
            }
        }
        r.check("P7 input floor", have, false, format!("select={} or a keyboard", cfg.select_key.as_deref().unwrap_or("none")));
    }

    let hard_fail = r.0.iter().any(|c| !c.pass && c.class == "hard");
    let soft_fail = r.0.iter().any(|c| !c.pass && c.class == "soft");
    let rc = if hard_fail { 2 } else if soft_fail { 1 } else { 0 };
    let _ = fs::create_dir_all("/run/mura");
    let report = Report { codename: cfg.codename.clone(), result: rc, checks: r.0 };
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        if fs::write("/run/mura/preflight.json.tmp", json).is_ok() {
            let _ = fs::rename("/run/mura/preflight.json.tmp", "/run/mura/preflight.json");
        }
    }
    for c in &report.checks {
        let tag = if c.pass { "PASS " } else if c.class == "hard" { "FAIL " } else { "WARN " };
        println!("{tag}{}: {}", c.check, c.detail);
    }
    // The failed checks as plain lines for the panel (modules/os/recovery.nix puts them on the
    // plymouth splash on a hard failure — pmOS's shape: what failed, on the first failure); and a
    // marker plymouth-quit is conditioned on, so the splash stays up when there is no greeter.
    let failed: Vec<String> = report
        .checks
        .iter()
        .filter(|c| !c.pass)
        .map(|c| format!("{}{}: {}", if c.class == "hard" { "" } else { "(soft) " }, c.check, c.detail))
        .collect();
    let _ = fs::write("/run/mura/preflight.summary", failed.join("\n") + "\n");
    if hard_fail {
        let _ = fs::write("/run/mura/preflight.failed", "");
    } else {
        let _ = fs::remove_file("/run/mura/preflight.failed");
    }
    std::process::exit(rc);
}
