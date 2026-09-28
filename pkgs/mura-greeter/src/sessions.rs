//! Sessions from the module's file, `/etc/greetd/environments` (research/78 §9 det. 4;
//! gtkgreet's and tuigreet's `environments` file): one command per line, `#` comments; the
//! chooser is hidden with one entry. `start_session.env` carries the session type and desktop
//! names — the values are proposed (flagged in research/78 §9): `XDG_SESSION_TYPE=wayland`,
//! `XDG_SESSION_DESKTOP=mura`, `XDG_CURRENT_DESKTOP=Mura`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEntry {
    /// what the chooser shows: the command's first word
    pub name: String,
    pub cmd: Vec<String>,
}

pub const FILE: &str = "/etc/greetd/environments";

pub fn parse(text: &str) -> Vec<SessionEntry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let cmd: Vec<String> = l.split_whitespace().map(str::to_string).collect();
            let name = cmd.first().map(|c| c.rsplit('/').next().unwrap_or(c).to_string()).unwrap_or_default();
            SessionEntry { name, cmd }
        })
        .collect()
}

pub fn load() -> Vec<SessionEntry> {
    match std::fs::read_to_string(FILE) {
        Ok(t) => parse(&t),
        Err(e) => {
            tracing::warn!(FILE, "sessions: {e}; falling back to `mura-session start`");
            vec![SessionEntry { name: "mura-session".into(), cmd: vec!["mura-session".into(), "start".into()] }]
        }
    }
}

pub fn env() -> Vec<String> {
    vec!["XDG_SESSION_TYPE=wayland".into(), "XDG_SESSION_DESKTOP=mura".into(), "XDG_CURRENT_DESKTOP=Mura".into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_modules_file_shape() {
        let s = parse("# sessions\nmura-session start\n\n/usr/bin/sway --unsupported-gpu\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "mura-session");
        assert_eq!(s[0].cmd, vec!["mura-session", "start"]);
        assert_eq!(s[1].name, "sway");
    }
}
