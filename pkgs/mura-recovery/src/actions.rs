//! The recovery actions (specs/recovery-menu.md §2) — the only place they live. Every frontend
//! calls these; none re-implements one.

use crate::config::Config;
use std::fs;
use std::path::Path;
use std::process::Command;

const SYSTEMCTL: &str = match option_env!("MURA_SYSTEMCTL") {
    Some(p) => p,
    None => "systemctl",
};
const REPART: &str = match option_env!("MURA_REPART") {
    Some(p) => p,
    None => "systemd-repart",
};
const UDEVADM: &str = match option_env!("MURA_UDEVADM") {
    Some(p) => p,
    None => "udevadm",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Status,
    FactoryReset,
    SwitchSlot,
    Reboot,
    PowerOff,
}

impl Action {
    pub fn parse(name: &str) -> Option<Action> {
        Some(match name {
            "status" => Action::Status,
            "factory-reset" => Action::FactoryReset,
            "switch-slot" => Action::SwitchSlot,
            "reboot" => Action::Reboot,
            "poweroff" => Action::PowerOff,
            _ => return None,
        })
    }
}

/// `status`: what every surface shows (§2).
pub fn status_text(cfg: &Config) -> String {
    let mut s = String::from("Mura recovery\n");
    match fs::read_to_string("/run/mura/preflight.summary") {
        Ok(t) if !t.trim().is_empty() => {
            s.push_str("Last preflight:\n");
            for l in t.lines() {
                s.push_str("  ");
                s.push_str(l);
                s.push('\n');
            }
        }
        _ => {}
    }
    let fp = fs::read_to_string("/run/mura-recovery/fingerprint").unwrap_or_default();
    let src = fs::read_to_string("/run/mura-recovery/keysource").unwrap_or_default();
    if !fp.trim().is_empty() {
        s.push_str(&format!("Host key {} ({})\n", fp.trim(), if src.trim() == "own" { "the device's own" } else { "generated for this session" }));
    }
    s.push_str(&format!("USB cable: ssh root@{}\n", cfg.gadget_addr));
    if let Ok(env) = fs::read_to_string("/run/mura/hotspot.env") {
        let get = |k: &str| env.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')).map(str::to_string));
        if let (Some(ssid), Some(psk)) = (get("MURA_HOTSPOT_SSID"), get("MURA_HOTSPOT_PSK")) {
            s.push_str(&format!("Wi-Fi: network {ssid}  password {psk}\n"));
        }
    }
    s.push_str(&format!("Help: {}\n", cfg.docs_url));
    s
}

/// The whole disk carrying the persist partition: sysfs makes a partition's parent its `..`.
fn disk_of(persist_dev: &str) -> Option<String> {
    let part = fs::canonicalize(persist_dev).ok()?;
    let name = part.file_name()?.to_string_lossy().into_owned();
    let sys = Path::new("/sys/class/block").join(&name);
    if sys.join("partition").exists() {
        let parent = fs::canonicalize(sys.join(".."))
            .ok()?;
        Some(format!("/dev/{}", parent.file_name()?.to_string_lossy()))
    } else {
        Some(part.to_string_lossy().into_owned())
    }
}

/// Unmount every mount of the partition about to be erased, whoever mounted it (Android
/// recovery's `EraseVolume` → `ensure_volume_unmounted` before `format_volume`,
/// install/wipe_data.cpp: a wipe under a live mount is undefined, and the wipe was confirmed).
/// Fails — and the reset with it — if a mount will not go (someone's cwd is in it).
fn unmount_all(persist_dev: &str) -> Result<(), String> {
    let Ok(dev) = fs::canonicalize(persist_dev) else { return Ok(()) };
    let Ok(info) = fs::read_to_string("/proc/self/mountinfo") else { return Ok(()) };
    // mountinfo: id parent major:minor root mountpoint options ... - fstype source superopts
    let mut points: Vec<String> = Vec::new();
    for line in info.lines() {
        let Some((left, right)) = line.split_once(" - ") else { continue };
        let source = right.split_whitespace().nth(1).unwrap_or("");
        if fs::canonicalize(source).ok().as_deref() != Some(dev.as_path()) {
            continue;
        }
        if let Some(mp) = left.split_whitespace().nth(4) {
            points.push(mp.replace("\\040", " "));
        }
    }
    points.sort_by_key(|p| std::cmp::Reverse(p.len())); // deepest first
    for mp in points {
        println!("Unmounting {mp} before the reset");
        let c = std::ffi::CString::new(mp.as_str()).map_err(|e| e.to_string())?;
        if unsafe { libc::umount2(c.as_ptr(), 0) } != 0 {
            return Err(format!("{mp}: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

/// Run an action. Returns the process exit code per §2 (0 done, 1 refused/failed, 2 usage).
pub fn run(action: Action, confirmed: bool, cfg: &Config) -> i32 {
    match action {
        Action::Status => {
            print!("{}", status_text(cfg));
            0
        }
        Action::FactoryReset => {
            if !confirmed {
                eprintln!("mura-recovery: factory-reset needs --confirmed (the frontend confirms; specs/recovery-menu.md §2)");
                return 2;
            }
            let defs = Path::new("/etc/repart.d");
            if !defs.is_dir() || fs::read_dir(defs).map(|mut d| d.next().is_none()).unwrap_or(true) {
                eprintln!("mura-recovery: no repart definitions in /etc/repart.d; nothing to reset");
                return 1;
            }
            let Some(disk) = disk_of(&cfg.persist_device) else {
                eprintln!("mura-recovery: cannot find the disk behind {}", cfg.persist_device);
                return 1;
            };
            if let Err(e) = unmount_all(&cfg.persist_device) {
                eprintln!("mura-recovery: cannot unmount the partition to erase — nothing erased: {e}");
                return 1;
            }
            println!("Resetting: systemd-repart --factory-reset on {disk}");
            // systemd's factory reset from early boot's clean state: every FactoryReset=yes
            // partition is deleted and re-created empty (repart.d(5)); nothing else is touched.
            // Removing a partition from the kernel (BLKPG_DEL_PARTITION) fails with EBUSY while
            // anything holds it open — udev's blkid worker re-probing after a close-for-write
            // (its `watch` rule), most often. repart then leaves the on-disk table without the
            // partition but the kernel still knowing it, and the re-creation fails the same way.
            // So: settle udev first, and on failure re-read the table ourselves (BLKRRPART, which
            // drops the stale kernel entry) and retry; every attempt is idempotent.
            let mut ok = false;
            for attempt in 1..=5 {
                let _ = Command::new(UDEVADM).args(["settle", "--timeout=10"]).status();
                let st = Command::new(REPART)
                    .args(["--dry-run=no", "--factory-reset=yes", "--definitions=/etc/repart.d", &disk])
                    .status();
                match st {
                    Ok(s) if s.success() => {
                        ok = true;
                        break;
                    }
                    Ok(s) => eprintln!("mura-recovery: systemd-repart failed (attempt {attempt}/5): {s}"),
                    Err(e) => {
                        eprintln!("mura-recovery: cannot run systemd-repart: {e}");
                        return 1;
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                reread_partitions(&disk);
            }
            if !ok {
                return 1;
            }
            unsafe { libc::sync() };
            if std::env::var_os("MURA_RECOVERY_NO_REBOOT").is_some() {
                println!("(MURA_RECOVERY_NO_REBOOT set: not rebooting)");
                return 0;
            }
            systemctl(&["reboot"])
        }
        Action::SwitchSlot => match &cfg.switch_slot_command {
            None => {
                eprintln!("mura-recovery: this family has no slot switch");
                2
            }
            Some(cmd) => {
                let st = Command::new("/bin/sh").args(["-c", cmd]).status();
                if !matches!(st, Ok(s) if s.success()) {
                    eprintln!("mura-recovery: slot switch failed");
                    return 1;
                }
                systemctl(&["reboot"])
            }
        },
        Action::Reboot => systemctl(&["reboot"]),
        Action::PowerOff => systemctl(&["poweroff"]),
    }
}

/// BLKRRPART on the whole disk: make the kernel's view match the on-disk table.
fn reread_partitions(disk: &str) {
    const BLKRRPART: libc::c_ulong = 0x125F; // _IO(0x12, 95), linux/fs.h
    if let Ok(f) = fs::File::open(disk) {
        use std::os::unix::io::AsRawFd;
        let r = unsafe { libc::ioctl(f.as_raw_fd(), BLKRRPART as _) };
        if r != 0 {
            eprintln!("mura-recovery: BLKRRPART {disk}: {}", std::io::Error::last_os_error());
        }
    }
}

fn systemctl(args: &[&str]) -> i32 {
    match Command::new(SYSTEMCTL).args(args).status() {
        Ok(s) if s.success() => 0,
        Ok(s) => {
            eprintln!("mura-recovery: systemctl {} failed: {s}", args.join(" "));
            1
        }
        Err(e) => {
            eprintln!("mura-recovery: cannot run systemctl: {e}");
            1
        }
    }
}
