//! mura-authd — the lock-path PAM helper of specs/session-auth.md §2.
//!
//! One process per unlock conversation, spawned by the compositor with an inherited
//! SOCK_SEQPACKET socketpair (`--fd N`) and a 64-bit conversation nonce (`--nonce HEX`).
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
            PAM_BINARY_PROMPT => "binary",
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
            Ok(In::RespondBatch { nonce, conversation: c, responses }) => {
                if nonce != st.nonce || c != conversation {
                    continue; // stale nonce or conversation: ignored
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
    for r in responses {
        if r.index >= n {
            continue;
        }
        if let Some(mut text) = r.response {
            let c = CString::new(std::mem::take(&mut text)).unwrap_or_default();
            unsafe { (*arr.add(r.index)).resp = libc::strdup(c.as_ptr()) };
            // zero our copy; PAM owns (and frees) the strdup'd one
            let mut bytes = c.into_bytes();
            zeroize(&mut bytes);
        }
    }
    unsafe { *resp = arr };
    PAM_SUCCESS
}

// ---------------------------------------------------------------- main

fn usage() -> ! {
    eprintln!("usage: mura-authd --fd N --nonce HEX [--user NAME] [--service NAME]");
    exit(2)
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
    let mut nonce: Option<String> = None;
    let mut user: Option<String> = None;
    let mut service = String::from("mura-lock");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--fd" => fd = args.next().and_then(|v| v.parse().ok()),
            "--nonce" => nonce = args.next(),
            "--user" => user = args.next(),
            "--service" => service = args.next().unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }
    let (Some(fd), Some(nonce)) = (fd, nonce) else { usage() };
    if nonce.len() != 16 || !nonce.chars().all(|c| c.is_ascii_hexdigit()) {
        usage();
    }
    let user = user.or_else(current_user).unwrap_or_else(|| usage());

    let mut st = State { fd, nonce, conversation: 0, max_delay_us: 0, aborted: false, internal_error: false };
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
