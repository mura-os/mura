//! Drawing through plymouth (specs/recovery-menu.md §5): one message per screen, ≤ 200 bytes
//! (the client protocol asserts at 255); a redraw hides the previous message and displays the
//! new one; the two-step theme stacks messages, so the ways-in lines drawn once stay.

use std::process::Command;

const PLYMOUTH: &str = match option_env!("MURA_PLYMOUTH") {
    Some(p) => p,
    None => "plymouth",
};

pub const MAX_MESSAGE: usize = 200;

pub struct Screen {
    current: Option<String>,
    pub available: bool,
}

impl Screen {
    pub fn new() -> Screen {
        let available = Command::new(PLYMOUTH).arg("--ping").status().map(|s| s.success()).unwrap_or(false);
        Screen { current: None, available }
    }

    fn call(args: &[&str]) -> bool {
        Command::new(PLYMOUTH).args(args).status().map(|s| s.success()).unwrap_or(false)
    }

    /// Show text that stays (the ways-in lines): chunked into whole lines under the cap.
    pub fn show_static(&self, text: &str) {
        if !self.available {
            return;
        }
        for chunk in chunk_lines(text, MAX_MESSAGE) {
            Self::call(&["display-message", &format!("--text={chunk}")]);
        }
    }

    /// Replace the menu's own message with `text` (truncated to the cap if a screen ever grows).
    pub fn draw(&mut self, text: &str) {
        if !self.available {
            return;
        }
        let text = truncate(text, MAX_MESSAGE);
        if let Some(prev) = self.current.take() {
            Self::call(&["hide-message", &format!("--text={prev}")]);
        }
        if Self::call(&["display-message", &format!("--text={text}")]) {
            self.current = Some(text);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub fn chunk_lines(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if !cur.is_empty() && cur.len() + line.len() + 1 > max {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_never_exceed_the_cap() {
        let text = (0..40).map(|i| format!("line number {i} with some words")).collect::<Vec<_>>().join("\n");
        for c in chunk_lines(&text, MAX_MESSAGE) {
            assert!(c.len() <= MAX_MESSAGE);
        }
    }

    #[test]
    fn truncate_keeps_char_boundaries() {
        let s = "é".repeat(150); // 300 bytes
        let t = truncate(&s, MAX_MESSAGE);
        assert!(t.len() <= MAX_MESSAGE && t.is_char_boundary(t.len()));
    }
}
