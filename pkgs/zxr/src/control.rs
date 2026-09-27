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
    /// a native app became / stopped being primary (the M1 observer's hook, driven by the harness):
    /// quiet follows unless `games.keep_planes`
    Primary(bool),
    /// research/69: hide / show the focused member (window-workspace-management.md "hidden")
    Hide(bool),
    /// The test-only source injector (spatial-input §1a; plan judgment 1): a synthetic sample
    /// for one source kind, fed into the input chain at the next tick with `Flags::SYNTHETIC`.
    /// Monado's simulated controllers never change a value (`simulated_controller.c:96-115`),
    /// so this is the only way to drive controller/hand/gaze paths on the host.
    Source(SourceCmd),
    /// `present on|off`: user presence (`XR_EXT_user_presence`) from the harness
    Present(bool),
    /// `mode normal|greeter|locked`: the input mode (ADR 0007) from the harness
    Mode(String),
    /// `a11y dwell on|off` / `a11y gain <f>`: the a11y stage's settings (spatial-input §13–§14) from the harness
    A11y(String, String),
    /// `cursor ray both|image|ring` / `cursor scale angle|plane`: the cursor preferences
    /// (spatial-input §14 `input.cursor.*`) until `org.mura.Settings1` carries them
    Cursor(String, String),
    Unknown(String),
}

/// `source <kind> <verb> …` — kinds: head gaze hand-left hand-right controller-left
/// controller-right pointer keyboard.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceCmd {
    /// `pose x y z qx qy qz qw [quality]` — quality: nominal|subnominal|lost (default nominal)
    Pose { kind: String, pos: [f32; 3], quat: [f32; 4], quality: String },
    /// `press|release <button>` — select|secondary|middle|menu|back|system|grip|<evdev code>
    Button { kind: String, button: String, pressed: bool },
    /// `value <name> <f>` — pinch|aim_activate|grasp|poke
    Value { kind: String, name: String, value: f32 },
    /// `delta dx dy` (pointer) or `axis h v [wheel|finger|continuous]`
    Delta { kind: String, dx: f64, dy: f64 },
    Axis { kind: String, h: f64, v: f64, source: String },
    /// `flag <name> on|off` — system_gesture|menu_pressed|dominant
    Flag { kind: String, name: String, on: bool },
    /// `joints <26×7 floats>` — hand joints for the bridge (pos xyz, quat xyzw per joint)
    Joints { kind: String, joints: Vec<f32> },
    /// `off` — the source is gone (tracking lost / device removed)
    Off { kind: String },
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
        (Some("primary"), Some(v)) => Command::Primary(v == "on" || v == "1"),
        (Some("hide"), Some(v)) => Command::Hide(v == "on" || v == "1"),
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
        (Some("present"), Some(v)) => Command::Present(v == "on" || v == "1"),
        (Some("mode"), Some(m)) => Command::Mode(m.to_string()),
        (Some("a11y"), Some(k)) => Command::A11y(k.to_string(), it.next().unwrap_or("").to_string()),
        (Some("cursor"), Some(k)) => Command::Cursor(k.to_string(), it.next().unwrap_or("").to_string()),
        (Some("source"), Some(kind)) => parse_source(kind, it.collect::<Vec<_>>().as_slice()).map(Command::Source).unwrap_or_else(|| Command::Unknown(line.to_string())),
        _ => Command::Unknown(line.to_string()),
    }
}

fn parse_source(kind: &str, rest: &[&str]) -> Option<SourceCmd> {
    let kind = kind.to_string();
    let f = |s: &&str| s.parse::<f32>().ok();
    let d = |s: &&str| s.parse::<f64>().ok();
    match rest {
        ["pose", x, y, z, qx, qy, qz, qw, more @ ..] => {
            let v: Option<Vec<f32>> = [*x, *y, *z, *qx, *qy, *qz, *qw].iter().map(f).collect();
            let v = v?;
            let quality = more.first().map(|s| s.to_string()).unwrap_or_else(|| "nominal".into());
            Some(SourceCmd::Pose { kind, pos: [v[0], v[1], v[2]], quat: [v[3], v[4], v[5], v[6]], quality })
        }
        ["press", b] => Some(SourceCmd::Button { kind, button: b.to_string(), pressed: true }),
        ["release", b] => Some(SourceCmd::Button { kind, button: b.to_string(), pressed: false }),
        ["value", name, v] => Some(SourceCmd::Value { kind, name: name.to_string(), value: f(v)? }),
        ["delta", dx, dy] => Some(SourceCmd::Delta { kind, dx: d(dx)?, dy: d(dy)? }),
        ["axis", h, v, more @ ..] => Some(SourceCmd::Axis { kind, h: d(h)?, v: d(v)?, source: more.first().map(|s| s.to_string()).unwrap_or_else(|| "wheel".into()) }),
        ["flag", name, on] => Some(SourceCmd::Flag { kind, name: name.to_string(), on: *on == "on" || *on == "1" }),
        ["joints", vals @ ..] if vals.len() == 26 * 7 => {
            let v: Option<Vec<f32>> = vals.iter().map(f).collect();
            Some(SourceCmd::Joints { kind, joints: v? })
        }
        ["off"] => Some(SourceCmd::Off { kind }),
        _ => None,
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
    fn parses_the_injector() {
        assert_eq!(
            parse("source hand-left pose 0 1 -1 0 0 0 1"),
            Command::Source(SourceCmd::Pose { kind: "hand-left".into(), pos: [0.0, 1.0, -1.0], quat: [0.0, 0.0, 0.0, 1.0], quality: "nominal".into() })
        );
        assert_eq!(
            parse("source gaze pose 0 0 0 0 0 0 1 subnominal"),
            Command::Source(SourceCmd::Pose { kind: "gaze".into(), pos: [0.0; 3], quat: [0.0, 0.0, 0.0, 1.0], quality: "subnominal".into() })
        );
        assert_eq!(parse("source controller-right press select"), Command::Source(SourceCmd::Button { kind: "controller-right".into(), button: "select".into(), pressed: true }));
        assert_eq!(parse("source hand-right value pinch 0.9"), Command::Source(SourceCmd::Value { kind: "hand-right".into(), name: "pinch".into(), value: 0.9 }));
        assert_eq!(parse("source pointer delta 3 -2"), Command::Source(SourceCmd::Delta { kind: "pointer".into(), dx: 3.0, dy: -2.0 }));
        assert_eq!(parse("source pointer axis 0 15 wheel"), Command::Source(SourceCmd::Axis { kind: "pointer".into(), h: 0.0, v: 15.0, source: "wheel".into() }));
        assert_eq!(parse("source hand-left flag system_gesture on"), Command::Source(SourceCmd::Flag { kind: "hand-left".into(), name: "system_gesture".into(), on: true }));
        assert_eq!(parse("source hand-left off"), Command::Source(SourceCmd::Off { kind: "hand-left".into() }));
        assert_eq!(parse("present off"), Command::Present(false));
        assert_eq!(parse("a11y dwell on"), Command::A11y("dwell".into(), "on".into()));
        assert_eq!(parse("a11y gain 1.5"), Command::A11y("gain".into(), "1.5".into()));
        assert_eq!(parse("cursor ray image"), Command::Cursor("ray".into(), "image".into()));
        assert_eq!(parse("cursor scale plane"), Command::Cursor("scale".into(), "plane".into()));
        assert!(matches!(parse("source hand-left pose 0 1"), Command::Unknown(_)));
        assert!(matches!(parse("source hand-left joints 1 2 3"), Command::Unknown(_)));
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
