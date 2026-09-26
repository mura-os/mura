//! Test-harness reporting helpers. Included by the two harness binaries directly; not part of
//! the reusable perception_intake library API.

/// Number of `/proc/self/fd` entries — the harness's fd-leak accounting (§8 item 1).
#[allow(dead_code)] // used by the consumer harness; this module is also included by the producer
pub fn open_fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd").map(|d| d.count()).unwrap_or(0)
}

/// `key=value` report lines (§8 observability): written by both bins to `--report PATH`.
#[derive(Default)]
pub struct Report {
    lines: Vec<(String, String)>,
}

impl Report {
    pub fn set<V: std::fmt::Display>(&mut self, key: &str, value: V) {
        let v = value.to_string();
        if let Some(e) = self.lines.iter_mut().find(|(k, _)| k == key) {
            e.1 = v;
        } else {
            self.lines.push((key.to_string(), v));
        }
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.lines {
            s.push_str(k);
            s.push('=');
            s.push_str(v);
            s.push('\n');
        }
        s
    }

    pub fn write(&self, path: &str) {
        let tmp = format!("{path}.tmp");
        if std::fs::write(&tmp, self.render()).and_then(|_| std::fs::rename(&tmp, path)).is_err() {
            eprintln!("report: cannot write {path}");
        }
    }
}
