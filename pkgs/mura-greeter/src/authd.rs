//! Backend B — `mura-authd`, the lock path's PAM helper (specs/session-auth.md §2; rev 6: the
//! **program** spawns it and owns the conversation — kscreenlocker's PAM worker, swaylock's PAM
//! child, cosmic-greeter's PAM thread; research/78 §3). One helper per conversation over a
//! `SOCK_SEQPACKET` socketpair (`--fd N`), the 64-bit nonce in `MURA_AUTHD_NONCE` (never argv),
//! JSON records one per packet; `prompt_batch` → one `respond_batch`; `failure.delay_ms` is the
//! fail delay PAM asked for, honoured by the scene before the retry.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::process::{Command as Proc, Stdio};
use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};

use crate::conv::{post, zeroize, Command, Event};

#[derive(Deserialize)]
struct Prompt {
    index: usize,
    style: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum In {
    #[serde(rename = "prompt_batch")]
    PromptBatch { nonce: String, conversation: u32, prompts: Vec<Prompt> },
    #[serde(rename = "success")]
    Success { nonce: String },
    #[serde(rename = "failure")]
    Failure { nonce: String, reason: String, delay_ms: u64 },
}

#[derive(Serialize)]
struct Response {
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Out<'a> {
    #[serde(rename = "respond_batch")]
    RespondBatch { nonce: &'a str, conversation: u32, responses: Vec<Response> },
    #[serde(rename = "cancel")]
    Cancel { nonce: &'a str },
}

const MAX_RECORD: usize = 64 * 1024;

/// The helper binary: baked by the package (`MURA_AUTHD`), else `PATH`.
fn helper() -> String {
    std::env::var("MURA_AUTHD").ok().or_else(|| option_env!("MURA_AUTHD").map(str::to_string)).unwrap_or_else(|| "mura-authd".into())
}

fn nonce() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn seqpacket_pair() -> std::io::Result<(UnixStream, UnixStream)> {
    let mut fds = [0i32; 2];
    // SAFETY: socketpair fills two fds or fails; both are owned from here
    let r = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr()) };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (UnixStream::from(OwnedFd::from_raw_fd(fds[0])), UnixStream::from(OwnedFd::from_raw_fd(fds[1]))) })
}

/// Run one `mura-authd` conversation for `username` on this thread until it ends.
pub fn run(username: String, rx: Receiver<Command>) {
    let (mut ours, theirs) = match seqpacket_pair() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("socketpair: {e}");
            post(Event::Failure { delay_ms: 0 });
            return;
        }
    };
    let nonce = nonce();
    let fd = theirs.as_raw_fd();
    let mut c = Proc::new(helper());
    c.arg("--fd").arg(fd.to_string()).arg("--user").arg(&username).env("MURA_AUTHD_NONCE", &nonce).stdin(Stdio::null());
    // SAFETY: clearing CLOEXEC on the child's end only
    unsafe {
        use std::os::unix::process::CommandExt;
        c.pre_exec(move || {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
            }
            Ok(())
        });
    }
    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(helper = helper(), "spawn: {e}");
            post(Event::Failure { delay_ms: 0 });
            return;
        }
    };
    drop(theirs);
    let mut buf = vec![0u8; MAX_RECORD + 1];
    let outcome = loop {
        let n = match ours.read(&mut buf) {
            Ok(0) => break Event::Failure { delay_ms: 0 },
            Ok(n) => n,
            Err(e) => {
                tracing::error!("authd read: {e}");
                break Event::Failure { delay_ms: 0 };
            }
        };
        let rec: In = match serde_json::from_slice(&buf[..n]) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("authd record: {e}");
                break Event::Failure { delay_ms: 0 };
            }
        };
        match rec {
            In::PromptBatch { nonce: n2, conversation, prompts } => {
                if n2 != nonce {
                    tracing::warn!("authd: stale nonce ignored");
                    continue;
                }
                let mut responses = Vec::with_capacity(prompts.len());
                let mut cancelled = false;
                for p in prompts {
                    match p.style.as_str() {
                        "info" => {
                            post(Event::Info(p.text.unwrap_or_default()));
                            responses.push(Response { index: p.index, response: None });
                        }
                        "error" => {
                            post(Event::Error(p.text.unwrap_or_default()));
                            responses.push(Response { index: p.index, response: None });
                        }
                        "secret" | "visible" => {
                            post(Event::Prompt { text: p.text.unwrap_or_default(), secret: p.style == "secret" });
                            match rx.recv() {
                                Ok(Command::Respond(text)) => responses.push(Response { index: p.index, response: Some(text) }),
                                Ok(Command::Cancel) | Err(_) => {
                                    cancelled = true;
                                    break;
                                }
                            }
                        }
                        other => {
                            tracing::warn!(style = other, "authd: unsupported prompt style; the helper reports failure(unsupported_prompt)");
                            responses.push(Response { index: p.index, response: None });
                        }
                    }
                }
                let out = if cancelled { Out::Cancel { nonce: &nonce } } else { Out::RespondBatch { nonce: &nonce, conversation, responses } };
                let mut bytes = serde_json::to_vec(&out).unwrap_or_default();
                let w = ours.write_all(&bytes);
                for b in bytes.iter_mut() {
                    // SAFETY: plain byte writes to an owned buffer
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
                if let Out::RespondBatch { responses, .. } = out {
                    for mut r in responses {
                        if let Some(s) = r.response.as_mut() {
                            zeroize(s);
                        }
                    }
                }
                if w.is_err() || cancelled {
                    break Event::Failure { delay_ms: 0 };
                }
            }
            In::Success { nonce: n2 } => {
                if n2 == nonce {
                    break Event::Success;
                }
            }
            In::Failure { nonce: n2, reason, delay_ms } => {
                if n2 == nonce {
                    tracing::info!(reason, delay_ms, "authd: failure");
                    break Event::Failure { delay_ms };
                }
            }
        }
    };
    let _ = child.wait();
    post(outcome);
}
