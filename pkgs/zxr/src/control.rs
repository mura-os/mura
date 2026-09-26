//! The control socket: a line protocol on a Unix socket for the R0 harness and for developers
//! (`niri msg`'s shape). Not a policy seam (ADR 0012): it drives focus, geometry and the journal,
//! nothing a client could not already ask for through the seat.
//!
//!   focus next | list | journal | quit | close | resize W H | move DX DY DZ | spawn CMD

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

pub enum Command {
    FocusNext,
    List,
    Journal,
    Quit,
    Close,
    Resize(i32, i32),
    Move(f32, f32, f32),
    Spawn(String),
    Unknown(String),
}

pub fn parse(line: &str) -> Command {
    let mut it = line.split_whitespace();
    match (it.next(), it.next()) {
        (Some("focus"), Some("next")) => Command::FocusNext,
        (Some("list"), _) => Command::List,
        (Some("journal"), _) => Command::Journal,
        (Some("quit"), _) => Command::Quit,
        (Some("close"), _) => Command::Close,
        (Some("resize"), Some(w)) => match (w.parse(), it.next().and_then(|h| h.parse().ok())) {
            (Ok(w), Some(h)) => Command::Resize(w, h),
            _ => Command::Unknown(line.to_string()),
        },
        (Some("move"), Some(dx)) => {
            let dy = it.next().and_then(|v| v.parse().ok());
            let dz = it.next().and_then(|v| v.parse().ok());
            match (dx.parse(), dy, dz) {
                (Ok(dx), Some(dy), Some(dz)) => Command::Move(dx, dy, dz),
                _ => Command::Unknown(line.to_string()),
            }
        }
        (Some("spawn"), Some(_)) => Command::Spawn(line.trim_start_matches("spawn").trim().to_string()),
        _ => Command::Unknown(line.to_string()),
    }
}

/// `zxr ctl SOCKET COMMAND...` — the client side, so the harness needs no netcat.
pub fn client_main(args: &[String]) -> Result<(), String> {
    let (sock, cmd) = args.split_first().ok_or("usage: zxr ctl SOCKET COMMAND...")?;
    if cmd.is_empty() {
        return Err("usage: zxr ctl SOCKET COMMAND...".into());
    }
    let mut s = UnixStream::connect(sock).map_err(|e| format!("{sock}: {e}"))?;
    s.write_all(cmd.join(" ").as_bytes()).and_then(|_| s.write_all(b"\n")).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut s, &mut reply).map_err(|e| e.to_string())?;
    print!("{reply}");
    Ok(())
}

/// Read one line from an accepted connection and hand the reply back to it.
pub fn serve_line(stream: UnixStream, mut handle: impl FnMut(Command) -> String) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let reply = handle(parse(line.trim()));
    let mut s = stream;
    let _ = s.write_all(reply.as_bytes());
    let _ = s.write_all(b"\n");
}
