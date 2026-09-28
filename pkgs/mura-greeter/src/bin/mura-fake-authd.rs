//! mura-fake-authd — greetd's `fakegreet` for the lock path: a stand-in for `mura-authd` that
//! speaks specs/session-auth.md §2.3 over the inherited `SOCK_SEQPACKET` fd without PAM, so the
//! lock scene can be driven on a host that has no `mura-lock` stack (the G1 nested gate; the real
//! helper is covered by its own harness and the VM tests). One `secret` prompt `Password:`;
//! `password` succeeds, anything else fails with `auth` and a 2 s delay (fakegreet's numbers).
//! Test-only; never installed on a device image.

use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;

fn main() {
    let mut fd: Option<i32> = None;
    let mut user = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--fd" => fd = args.next().and_then(|v| v.parse().ok()),
            "--user" => user = args.next().unwrap_or_default(),
            _ => {}
        }
    }
    let nonce = std::env::var("MURA_AUTHD_NONCE").unwrap_or_default();
    let (Some(fd), false) = (fd, nonce.is_empty()) else {
        eprintln!("usage: MURA_AUTHD_NONCE=HEX mura-fake-authd --fd N [--user U]");
        std::process::exit(2);
    };
    // SAFETY: the fd was handed to us for this purpose
    let mut s = unsafe { UnixStream::from_raw_fd(fd) };
    let prompt = format!(r#"{{"type":"prompt_batch","nonce":"{nonce}","conversation":1,"prompts":[{{"index":0,"style":"info","text":"fake authd for {user}"}},{{"index":1,"style":"secret","text":"Password:"}}]}}"#);
    if s.write_all(prompt.as_bytes()).is_err() {
        std::process::exit(1);
    }
    let mut buf = vec![0u8; 65536];
    let n = s.read(&mut buf).unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
    let ok = text.contains(&format!(r#""nonce":"{nonce}""#)) && text.contains(r#""response":"password""#);
    let reply = if ok {
        format!(r#"{{"type":"success","nonce":"{nonce}"}}"#)
    } else {
        std::thread::sleep(std::time::Duration::from_millis(2000));
        format!(r#"{{"type":"failure","nonce":"{nonce}","reason":"auth","delay_ms":2000}}"#)
    };
    let _ = s.write_all(reply.as_bytes());
    std::process::exit(if ok { 0 } else { 1 });
}
