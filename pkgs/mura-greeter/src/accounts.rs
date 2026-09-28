//! Accounts for the picker (multi-user.md §2; research/41 §1.4): NSS iteration over the
//! login.defs UID window (`UID_MIN`/`UID_MAX`, default 1000–60000 — SDDM's and tuigreet's
//! pattern), display names from the GECOS full-name field, last-user preselection from
//! root-owned state (`state/accounts/last-user`, the SDDM/regreet pattern), and the
//! numeric-credential hint (§3): a per-user file the greeter reads only after checking it is owned
//! by the account it is about to prompt.

use std::ffi::CStr;
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Debug)]
pub struct Account {
    pub name: String,
    pub display: String,
    pub uid: u32,
}

pub const STATE: &str = "/var/lib/mura/state";

/// `UID_MIN`/`UID_MAX` from `/etc/login.defs`, the contract default otherwise.
pub fn uid_window() -> (u32, u32) {
    let (mut lo, mut hi) = (1000u32, 60000u32);
    if let Ok(text) = std::fs::read_to_string("/etc/login.defs") {
        for line in text.lines() {
            let mut it = line.split_whitespace();
            match (it.next(), it.next().and_then(|v| v.parse().ok())) {
                (Some("UID_MIN"), Some(v)) => lo = v,
                (Some("UID_MAX"), Some(v)) => hi = v,
                _ => {}
            }
        }
    }
    (lo, hi)
}

/// Every account in the window with a login shell that is not `nologin`/`false`.
pub fn enumerate() -> Vec<Account> {
    let (lo, hi) = uid_window();
    let mut out = Vec::new();
    // SAFETY: the getpwent family is process-global; this runs once, on one thread, at startup.
    unsafe {
        libc::setpwent();
        loop {
            let pw = libc::getpwent();
            if pw.is_null() {
                break;
            }
            let uid = (*pw).pw_uid;
            if uid < lo || uid > hi {
                continue;
            }
            let shell = CStr::from_ptr((*pw).pw_shell).to_string_lossy();
            if shell.ends_with("nologin") || shell.ends_with("/false") {
                continue;
            }
            let name = CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned();
            let gecos = CStr::from_ptr((*pw).pw_gecos).to_string_lossy();
            let display = gecos.split(',').next().unwrap_or("").trim().to_string();
            out.push(Account { name, display, uid });
        }
        libc::endpwent();
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn last_user() -> Option<String> {
    let s = std::fs::read_to_string(format!("{STATE}/accounts/last-user")).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// The greeter runs as `greeter`; the file is root-owned state written by the session side.
/// Best effort: a read-only greeter simply keeps the previous value.
pub fn remember_last_user(name: &str) {
    let _ = std::fs::create_dir_all(format!("{STATE}/accounts"));
    if let Err(e) = std::fs::write(format!("{STATE}/accounts/last-user"), format!("{name}\n")) {
        tracing::debug!("last-user not written: {e}");
    }
}

/// The digit pad is rendered for this account (multi-user §3): `state/credential-hint/<user>`
/// exists **and is owned by that uid** (the directory is `1777`; anyone may create their own
/// file, so ownership is the check).
pub fn numeric_hint(name: &str, uid: Option<u32>) -> bool {
    let Ok(meta) = std::fs::metadata(format!("{STATE}/credential-hint/{name}")) else { return false };
    match uid {
        Some(uid) => meta.uid() == uid,
        None => lookup_uid(name).map(|u| meta.uid() == u).unwrap_or(false),
    }
}

pub fn lookup_uid(name: &str) -> Option<u32> {
    let c = std::ffi::CString::new(name).ok()?;
    // SAFETY: getpwnam returns a static buffer or null; only pw_uid is read
    let pw = unsafe { libc::getpwnam(c.as_ptr()) };
    (!pw.is_null()).then(|| unsafe { (*pw).pw_uid })
}

pub fn current_user() -> Option<String> {
    // SAFETY: as above
    let pw = unsafe { libc::getpwuid(libc::getuid()) };
    (!pw.is_null()).then(|| unsafe { CStr::from_ptr((*pw).pw_name) }.to_string_lossy().into_owned())
}

pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_default()
}
