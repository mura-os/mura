//! mura-authd — the lock-path PAM helper of specs/session-auth.md §2.
//!
//! One process per unlock conversation, spawned by the compositor with an inherited
//! SOCK_SEQPACKET socketpair (`--fd N`) and a 64-bit conversation nonce in the environment
//! (`MURA_AUTHD_NONCE=HEX` — never on argv, which is world-readable in /proc; the legacy
//! `--nonce HEX` is accepted with a warning for one release).
//! It runs PAM (`pam_start("mura-lock")` → `pam_authenticate(PAM_DISALLOW_NULL_AUTHTOK)` →
//! `pam_acct_mgmt` → `pam_end`; never setcred/open_session), turning every PAM conversation
//! callback into exactly one `prompt_batch` record and waiting for exactly one `respond_batch`
//! (or `cancel`). Records are UTF-8 JSON, one per seqpacket, no length prefix; anything over
//! 64 KiB, truncated, empty, invalid, or of unknown type ends the conversation as
//! `failure(internal)`. Messages with a stale nonce are ignored (§2.4). The helper reports the
//! maximum PAM fail-delay it was asked for as `delay_ms` and exits 0 only after `success`.
//!
//! No privileges, no D-Bus surface. PAM is reached through a hand-written FFI (Linux-PAM ABI)
//! so the closure carries no PAM binding crate; secrets that pass through this process are
//! zeroed before the buffers are dropped.
//!
//! Process hardening at startup (the kscreenlocker PAM-worker precedent, greeter/worker/prctls.h):
//! not dumpable (no same-uid ptrace attach under Yama, no core), RLIMIT_CORE=0, best-effort
//! mlockall, SIGKILL when the compositor dies (PR_SET_PDEATHSIG), and FD_CLOEXEC on the
//! conversation fd so it never leaks into pam_unix's setuid unix_chkpwd. Deliberately NOT
//! PR_SET_NO_NEW_PRIVS or seccomp: unix_chkpwd is setuid and would break; sandboxing belongs
//! with the compositor's spawn side (specs/session-auth.md §7).

use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::process::exit;

// ---------------------------------------------------------------- Linux-PAM ABI (pam_appl.h)

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}
#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}
#[repr(C)]
struct PamConv {
    conv: Option<
        extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    >,
    appdata_ptr: *mut c_void,
}
type PamHandle = c_void;

const PAM_SUCCESS: c_int = 0;
const PAM_SYSTEM_ERR: c_int = 4;
const PAM_CONV_ERR: c_int = 19;
const PAM_MAXTRIES: c_int = 8;
const PAM_ABORT: c_int = 26;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;
const PAM_FAIL_DELAY: c_int = 10;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_RADIO_TYPE: c_int = 5;
const PAM_BINARY_PROMPT: c_int = 7;

#[link(name = "pam")]
extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        handle: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut PamHandle, status: c_int) -> c_int;
    fn pam_set_item(handle: *mut PamHandle, item_type: c_int, item: *const c_void) -> c_int;
    fn pam_strerror(handle: *mut PamHandle, errnum: c_int) -> *const c_char;
}

// ---------------------------------------------------------------- wire (§2.3)

#[derive(Serialize)]
struct Prompt {
    index: usize,
    style: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Out<'a> {
    #[serde(rename = "prompt_batch")]
    PromptBatch { nonce: &'a str, conversation: u32, prompts: Vec<Prompt> },
    #[serde(rename = "success")]
    Success { nonce: &'a str },
    #[serde(rename = "failure")]
    Failure { nonce: &'a str, reason: &'static str, delay_ms: u64 },
}

#[derive(Deserialize)]
struct Response {
    index: usize,
    #[serde(default)]
    response: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum In {
    #[serde(rename = "respond_batch")]
    RespondBatch { nonce: String, conversation: u32, responses: Vec<Response> },
    #[serde(rename = "cancel")]
    Cancel { nonce: String },
}

const MAX_RECORD: usize = 64 * 1024;

// ---------------------------------------------------------------- state shared with the C callbacks

struct State {
    fd: c_int,
    nonce: String,
    conversation: u32,
    max_delay_us: u64,
    aborted: bool,
    internal_error: bool,
    unsupported_prompt: bool,
}

fn zeroize(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
}

fn send(fd: c_int, msg: &Out) -> bool {
    let mut data = serde_json::to_vec(msg).expect("serialise");
    let n = unsafe { libc::send(fd, data.as_ptr() as *const c_void, data.len(), libc::MSG_NOSIGNAL) };
    zeroize(&mut data);
    n == data.len() as isize
}

/// One complete seqpacket record, or None on EOF / oversize / truncation / empty (all internal).
fn recv_record(fd: c_int) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; MAX_RECORD + 1];
    let mut iov = libc::iovec { iov_base: buf.as_mut_ptr() as *mut c_void, iov_len: buf.len() };
    let mut hdr: libc::msghdr = unsafe { std::mem::zeroed() };
    hdr.msg_iov = &mut iov;
    hdr.msg_iovlen = 1;
    let n = unsafe { libc::recvmsg(fd, &mut hdr, 0) };
    if n <= 0 || n as usize > MAX_RECORD || (hdr.msg_flags & libc::MSG_TRUNC) != 0 {
        zeroize(&mut buf);
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

extern "C" fn fail_delay(_retval: c_int, usec: c_uint, appdata: *mut c_void) {
    let st = unsafe { &mut *(appdata as *mut State) };
    st.max_delay_us = st.max_delay_us.max(usec as u64);
    // The compositor enforces the delay from `delay_ms`; the helper does not sleep.
}

extern "C" fn conv(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    let st = unsafe { &mut *(appdata as *mut State) };
    if num_msg <= 0 || msg.is_null() || resp.is_null() {
        st.internal_error = true;
        return PAM_CONV_ERR;
    }
    let n = num_msg as usize;
    let mut prompts = Vec::with_capacity(n);
    for i in 0..n {
        let m = unsafe { &**msg.add(i) };
        let text = if m.msg.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(m.msg) }.to_string_lossy().into_owned())
        };
        let style = match m.msg_style {
            PAM_PROMPT_ECHO_OFF => "secret",
            PAM_PROMPT_ECHO_ON => "visible",
            PAM_ERROR_MSG => "error",
            PAM_TEXT_INFO => "info",
            PAM_RADIO_TYPE => "radio",
            PAM_BINARY_PROMPT => {
                // §2.3: a style this deployment does not render is answered by authd itself
                st.unsupported_prompt = true;
                return PAM_CONV_ERR;
            }
            _ => {
                st.internal_error = true;
                return PAM_CONV_ERR;
            }
        };
        prompts.push(Prompt { index: i, style, text });
    }
    st.conversation += 1;
    let conversation = st.conversation;
    if !send(st.fd, &Out::PromptBatch { nonce: &st.nonce, conversation, prompts }) {
        st.internal_error = true;
        return PAM_CONV_ERR;
    }

    // Wait for exactly one respond_batch for this conversation; ignore stale nonces (§2.4).
    let responses: Vec<Response> = loop {
        let Some(mut rec) = recv_record(st.fd) else {
            st.internal_error = true;
            return PAM_CONV_ERR;
        };
        let parsed: Result<In, _> = serde_json::from_slice(&rec);
        zeroize(&mut rec);
        match parsed {
            Ok(In::RespondBatch { nonce, conversation: c, mut responses }) => {
                if nonce != st.nonce || c != conversation {
                    // stale nonce or conversation: ignored — but its answers were secrets too
                    for r in responses.iter_mut() {
                        if let Some(t) = r.response.as_mut() {
                            zeroize(unsafe { t.as_bytes_mut() });
                        }
                    }
                    continue;
                }
                break responses;
            }
            Ok(In::Cancel { nonce }) => {
                if nonce != st.nonce {
                    continue;
                }
                st.aborted = true;
                return PAM_CONV_ERR;
            }
            Err(_) => {
                st.internal_error = true;
                return PAM_CONV_ERR;
            }
        }
    };

    // Hand PAM an array it frees with free(): calloc + strdup, per the Linux-PAM contract.
    let arr = unsafe { libc::calloc(n, std::mem::size_of::<PamResponse>()) } as *mut PamResponse;
    if arr.is_null() {
        st.internal_error = true;
        return PAM_CONV_ERR;
    }
    let mut seen = vec![false; n];
    let mut bad = false;
    for r in responses {
        // out-of-range or duplicate index: a malformed batch, never a silent skip/overwrite
        if r.index >= n || seen[r.index] {
            bad = true;
        } else {
            seen[r.index] = true;
        }
        if let Some(mut text) = r.response {
            // a NUL inside a response cannot be handed to PAM; it is an error, never an
            // empty answer (which PAM_DISALLOW_NULL_AUTHTOK would then reject for the wrong reason)
            let owned = std::mem::take(&mut text);
            match CString::new(owned) {
                Ok(c) if !bad => {
                    unsafe { (*arr.add(r.index)).resp = libc::strdup(c.as_ptr()) };
                    // zero our copy; PAM owns (and frees) the strdup'd one
                    let mut bytes = c.into_bytes();
                    zeroize(&mut bytes);
                }
                Ok(c) => {
                    let mut bytes = c.into_bytes();
                    zeroize(&mut bytes);
                }
                Err(e) => {
                    let mut bytes = e.into_vec();
                    zeroize(&mut bytes);
                    bad = true;
                }
            }
        }
    }
    if bad {
        // free what we already handed over, as PAM would have
        for i in 0..n {
            let p = unsafe { (*arr.add(i)).resp };
            if !p.is_null() {
                let len = unsafe { libc::strlen(p) };
                zeroize(unsafe { std::slice::from_raw_parts_mut(p as *mut u8, len) });
                unsafe { libc::free(p as *mut c_void) };
            }
        }
        unsafe { libc::free(arr as *mut c_void) };
        st.internal_error = true;
        return PAM_CONV_ERR;
    }
    unsafe { *resp = arr };
    PAM_SUCCESS
}

// ---------------------------------------------------------------- main

fn usage() -> ! {
    eprintln!("usage: MURA_AUTHD_NONCE=HEX mura-authd --fd N [--user NAME] [--service mura-lock[-*]]");
    exit(2)
}

/// Process hardening before anything secret is touched. The split follows the comparables
/// (research/56 §8: kscreenlocker's worker, systemd's fork helper): a *lifecycle* prctl that fails
/// is fatal — a helper that could outlive its compositor holds a half-answered conversation —
/// while *secrecy* hardening (dumpable, RLIMIT_CORE, mlockall) is best-effort: it narrows what a
/// same-uid attacker can do, but refusing every unlock over it would trade availability for a
/// boundary the same uid already crosses (specs/session-auth.md §2.5).
fn harden(fd: c_int) {
    unsafe {
        if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 {
            eprintln!("mura-authd: PR_SET_DUMPABLE failed; continuing (unexpected)");
        }
        let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        libc::setrlimit(libc::RLIMIT_CORE, &none);
        // the compositor is the parent; if it dies, so does every open conversation (§2.4)
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
            eprintln!("mura-authd: PR_SET_PDEATHSIG failed");
            exit(2);
        }
        if libc::getppid() == 1 {
            eprintln!("mura-authd: spawner already gone");
            exit(1);
        }
        // never inherited by pam_unix's setuid unix_chkpwd (or anything else PAM execs)
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            eprintln!("mura-authd: bad conversation fd");
            exit(2);
        }
        // keep the password out of swap where the target allows it
        let _ = libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE);
    }
}

fn current_user() -> Option<String> {
    let pw = unsafe { libc::getpwuid(libc::getuid()) };
    if pw.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr((*pw).pw_name) }.to_string_lossy().into_owned())
}

fn main() {
    let mut fd: Option<c_int> = None;
    let mut nonce: Option<String> = std::env::var("MURA_AUTHD_NONCE").ok();
    let mut user: Option<String> = None;
    let mut service = String::from("mura-lock");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--fd" => fd = args.next().and_then(|v| v.parse().ok()),
            "--nonce" => {
                // legacy: argv is world-readable in /proc/<pid>/cmdline; accepted for one
                // release so an older compositor build still works, then removed
                eprintln!("mura-authd: --nonce on argv is deprecated; use MURA_AUTHD_NONCE");
                nonce = args.next();
            }
            "--user" => user = args.next(),
            "--service" => service = args.next().unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }
    let (Some(fd), Some(nonce)) = (fd, nonce) else { usage() };
    if nonce.len() != 16 || !nonce.chars().all(|c| c.is_ascii_hexdigit()) {
        usage();
    }
    // the service is ours: `mura-lock`, or a `mura-lock-*` variant (test stacks). Anything else
    // is a caller bug, not a boundary (the caller is the same uid) — refuse before pam_start.
    if service != "mura-lock" && !service.starts_with("mura-lock-") {
        eprintln!("mura-authd: refusing PAM service {service:?}; only mura-lock[-*] is allowed");
        exit(2);
    }
    harden(fd);
    let user = user.or_else(current_user).unwrap_or_else(|| usage());

    let mut st = State { fd, nonce, conversation: 0, max_delay_us: 0, aborted: false, internal_error: false, unsupported_prompt: false };
    let pam_conv = PamConv { conv: Some(conv), appdata_ptr: &mut st as *mut State as *mut c_void };

    let c_service = CString::new(service).unwrap();
    let c_user = CString::new(user).unwrap();
    let mut handle: *mut PamHandle = std::ptr::null_mut();
    let mut rc = unsafe { pam_start(c_service.as_ptr(), c_user.as_ptr(), &pam_conv, &mut handle) };
    if rc == PAM_SUCCESS {
        // fail-delay callback (§2.2): PAM_FAIL_DELAY item is the function pointer itself
        let cb: extern "C" fn(c_int, c_uint, *mut c_void) = fail_delay;
        unsafe { pam_set_item(handle, PAM_FAIL_DELAY, cb as *const c_void) };
        rc = unsafe { pam_authenticate(handle, PAM_DISALLOW_NULL_AUTHTOK) };
        if rc == PAM_SUCCESS {
            rc = unsafe { pam_acct_mgmt(handle, 0) };
        }
        let msg = unsafe { CStr::from_ptr(pam_strerror(handle, rc)) }.to_string_lossy().into_owned();
        unsafe { pam_end(handle, rc) };
        eprintln!("mura-authd: pam result {rc} ({msg})");
    } else {
        eprintln!("mura-authd: pam_start failed: {rc}");
        rc = PAM_SYSTEM_ERR;
    }

    let delay_ms = st.max_delay_us / 1000;
    if rc == PAM_SUCCESS {
        send(st.fd, &Out::Success { nonce: &st.nonce });
        exit(0);
    }
    // Coarse reasons (§2.2): nothing here distinguishes an unknown user from a wrong password.
    let reason = if st.aborted {
        "abort"
    } else if st.unsupported_prompt {
        "unsupported_prompt"
    } else if st.internal_error {
        "internal"
    } else {
        match rc {
            PAM_MAXTRIES => "maxtries",
            PAM_CONV_ERR | PAM_ABORT => "abort",
            PAM_SYSTEM_ERR => "internal",
            _ => "auth",
        }
    };
    send(st.fd, &Out::Failure { nonce: &st.nonce, reason, delay_ms });
    exit(1);
}
