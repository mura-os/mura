//! The control socket: a line protocol on a Unix socket for the R0 harness and for developers
//! (`niri msg`'s shape). Not a policy seam (ADR 0012): it drives focus, geometry and the journal,
//! nothing a client could not already ask for through the seat.
//!
//!   focus next | list | journal | quit | close | resize W H | move DX DY DZ | spawn CMD
//!   key CODE [press|release]   (evdev keycode, through the seat to the focused surface)
//!   type TEXT                  (ASCII letters/digits/space, US layout, press+release each)

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

#[derive(Debug, PartialEq)]
pub enum Command {
    FocusNext,
    List,
    Journal,
    Quit,
    Close,
    Resize(i32, i32),
    Move(f32, f32, f32),
    Spawn(String),
    /// evdev keycode; `None` = press then release
    Key(u32, Option<bool>),
    Type(String),
    Quiet(bool),
    Unknown(String),
}

/// ASCII → evdev keycode on the US layout (letters, digits, space, enter, minus, dot, slash).
pub fn ascii_keycode(c: char) -> Option<u32> {
    const ROW1: &str = "qwertyuiop";
    const ROW2: &str = "asdfghjkl";
    const ROW3: &str = "zxcvbnm";
    let c = c.to_ascii_lowercase();
    if let Some(i) = ROW1.find(c) {
        return Some(16 + i as u32);
    }
    if let Some(i) = ROW2.find(c) {
        return Some(30 + i as u32);
    }
    if let Some(i) = ROW3.find(c) {
        return Some(44 + i as u32);
    }
    match c {
        '1'..='9' => Some(2 + (c as u32 - '1' as u32)),
        '0' => Some(11),
        ' ' => Some(57),
        '\n' => Some(28),
        '-' => Some(12),
        '.' => Some(52),
        '/' => Some(53),
        _ => None,
    }
}

pub fn parse(line: &str) -> Command {
    let mut it = line.split_whitespace();
    match (it.next(), it.next()) {
        (Some("focus"), Some("next")) => Command::FocusNext,
        (Some("list"), _) => Command::List,
        (Some("journal"), _) => Command::Journal,
        (Some("quit"), _) => Command::Quit,
        (Some("quiet"), Some(v)) => Command::Quiet(v == "on" || v == "1"),
        (Some("close"), _) => Command::Close,
        (Some("key"), Some(code)) => match code.parse() {
            Ok(code) => Command::Key(
                code,
                match it.next() {
                    Some("press") => Some(true),
                    Some("release") => Some(false),
                    _ => None,
                },
            ),
            Err(_) => Command::Unknown(line.to_string()),
        },
        (Some("type"), Some(_)) => Command::Type(line.trim_start_matches("type").trim_start().to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_verb() {
        assert_eq!(parse("focus next"), Command::FocusNext);
        assert_eq!(parse("resize 900 600"), Command::Resize(900, 600));
        assert_eq!(parse("move 0.3 0.1 -0.2"), Command::Move(0.3, 0.1, -0.2));
        assert_eq!(parse("spawn foot -e sh"), Command::Spawn("foot -e sh".into()));
        assert_eq!(parse("key 28"), Command::Key(28, None));
        assert_eq!(parse("key 68 press"), Command::Key(68, Some(true)));
        assert_eq!(parse("type hello world"), Command::Type("hello world".into()));
        assert!(matches!(parse("resize x y"), Command::Unknown(_)));
        assert!(matches!(parse("bogus"), Command::Unknown(_)));
    }

    #[test]
    fn ascii_maps_to_evdev() {
        assert_eq!(ascii_keycode('q'), Some(16));
        assert_eq!(ascii_keycode('a'), Some(30));
        assert_eq!(ascii_keycode('z'), Some(44));
        assert_eq!(ascii_keycode('1'), Some(2));
        assert_eq!(ascii_keycode('0'), Some(11));
        assert_eq!(ascii_keycode(' '), Some(57));
        assert_eq!(ascii_keycode('\n'), Some(28));
        assert_eq!(ascii_keycode('!'), None);
    }
}
