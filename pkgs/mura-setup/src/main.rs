//! mura-setup (system instance) — D3 STUB of the setup web app (first-run-onboarding.md §5.1).
//!
//! Serves the captive-portal launcher and the phone-OS connectivity-probe redirects on port 80
//! of exactly two addresses — the USB gadget's and the hotspot's — bound with IP_FREEBIND so the
//! hotspot address may not exist yet. Never a wildcard bind: the LAN is out of scope by design
//! (§5.3). `POST /finish` writes the `setup-complete` marker as the last write (§5.2); a watcher
//! exits the process shortly after the marker appears from any source (welcome surface,
//! administrator), so the unit retires within seconds and `ConditionPathExists=!marker` refuses
//! a restart. Hand-rolled HTTP/1.1 for six paths; the real setup program chooses its own stack.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::FromRawFd;
use std::path::Path;
use std::time::Duration;

const MARKER: &str = "/var/lib/mura/state/setup/setup-complete";
const ADDRS: [&str; 2] = ["172.16.42.1", "10.42.0.1"];
const PROBES: [&str; 6] = ["/generate_204", "/hotspot-detect.html", "/connecttest.txt", "/success.txt", "/ncsi.txt", "/canonical.html"];
const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Mura setup</title>\n<h1>Mura</h1><p>Open <a href=\"http://mura.local/\">http://mura.local/</a> in your browser\nto set up this headset.</p><p>(D3 stub: the setup pages arrive with their own rung.)</p>\n<form method=post action=/finish><button>Finish setup</button></form>\n";

fn listen_freebind(addr: &str) -> std::io::Result<TcpListener> {
    let octets: Vec<u8> = addr.split('.').map(|o| o.parse().unwrap()).collect();
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let one: libc::c_int = 1;
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, &one as *const _ as *const libc::c_void, 4);
        // IP_FREEBIND: bind even while the address is not (yet) configured on any interface
        if libc::setsockopt(fd, libc::IPPROTO_IP, libc::IP_FREEBIND, &one as *const _ as *const libc::c_void, 4) < 0 {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        let mut sa: libc::sockaddr_in = std::mem::zeroed();
        sa.sin_family = libc::AF_INET as libc::sa_family_t;
        sa.sin_port = 80u16.to_be();
        sa.sin_addr.s_addr = u32::from_be_bytes([octets[0], octets[1], octets[2], octets[3]]).to_be();
        if libc::bind(fd, &sa as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as u32) < 0
            || libc::listen(fd, 16) < 0
        {
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        Ok(TcpListener::from_raw_fd(fd))
    }
}

fn respond(s: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut out = format!("HTTP/1.1 {status}\r\nServer: mura-setup/stub\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let _ = s.write_all(out.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}

fn handle(mut s: TcpStream) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    // read the head (request line + headers), capped at 8 KiB
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    let head_end = loop {
        match s.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
                if buf.len() > 8192 {
                    respond(&mut s, "400 Bad Request", &[], b"");
                    return;
                }
            }
            Err(_) => return,
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split_whitespace();
    let (method, path) = match (parts.next(), parts.next()) {
        (Some(m), Some(p)) => (m, p),
        _ => {
            respond(&mut s, "400 Bad Request", &[], b"");
            return;
        }
    };
    // drain a POST body (Content-Length) so the peer sees a clean response
    let content_length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut have = buf.len() - head_end;
    while have < content_length.min(65536) {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => have += n,
        }
    }
    let path = path.split('?').next().unwrap_or(path);

    match (method, path) {
        ("GET", p) if PROBES.contains(&p) => respond(&mut s, "302 Found", &[("Location", "http://mura.local/")], b""),
        ("GET", _) | ("HEAD", _) => respond(&mut s, "200 OK", &[("Content-Type", "text/html; charset=utf-8")], PAGE.as_bytes()),
        ("POST", "/finish") => {
            // the marker is the LAST write (first-run §5.2); the stub has no Wi-Fi step
            let tmp = format!("{MARKER}.tmp");
            match std::fs::write(&tmp, "finished via web app\n").and_then(|_| std::fs::rename(&tmp, MARKER)) {
                Ok(()) => respond(&mut s, "200 OK", &[("Content-Type", "text/plain")], b"setup complete\n"),
                Err(e) => respond(&mut s, "500 Internal Server Error", &[], e.to_string().as_bytes()),
            }
        }
        _ => respond(&mut s, "404 Not Found", &[], b""),
    }
}

fn main() {
    // setup finished (here, in the headset, or by an administrator): retire within seconds
    std::thread::spawn(|| {
        while !Path::new(MARKER).exists() {
            std::thread::sleep(Duration::from_secs(2));
        }
        std::thread::sleep(Duration::from_secs(1)); // let an in-flight /finish response go out
        std::process::exit(0);
    });

    let mut handles = Vec::new();
    for addr in ADDRS {
        let listener = match listen_freebind(addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mura-setup: cannot listen on {addr}:80: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("mura-setup: listening on {addr}:80 (stub)");
        handles.push(std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                std::thread::spawn(move || handle(conn));
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
}
