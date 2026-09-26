//! The frame journal (specs/zxr-core.md §11): per-frame timing and the counters the R0 gates
//! read. Printed as `key=value` lines on SIGUSR1 and at exit; the harness parses them.

use std::fmt::Write as _;

#[derive(Default, Debug, Clone)]
pub struct Journal {
    pub frames: u64,
    pub frames_rendered: u64,
    pub missed_deadlines: u64,
    pub gpu_ns_total: u64,
    pub gpu_ns_max: u64,
    pub wake_to_end_ns_total: u64,
    pub wake_to_end_ns_max: u64,
    pub shm_uploads: u64,
    pub dmabuf_imports: u64,
    pub dmabuf_cpu_copies: u64,
    pub buffers_released: u64,
    pub retention_frames_total: u64,
    pub retention_frames_max: u64,
    pub frame_callbacks: u64,
    pub commits: u64,
    pub toplevels_mapped: u64,
    pub toplevels_unmapped: u64,
    pub popups: u64,
    pub focus_changes: u64,
    pub xwayland_toplevels: u64,
    pub fences_signalled: u64,
    pub fences_outstanding: u64,
    pub stale_texture_draws: u64,
    pub started_at_ns: u64,
}

impl Journal {
    pub fn record_frame(&mut self, rendered: bool, gpu_ns: Option<u64>, wake_to_end_ns: u64, missed: bool) {
        self.frames += 1;
        if rendered {
            self.frames_rendered += 1;
        }
        if let Some(g) = gpu_ns {
            self.gpu_ns_total += g;
            self.gpu_ns_max = self.gpu_ns_max.max(g);
        }
        self.wake_to_end_ns_total += wake_to_end_ns;
        self.wake_to_end_ns_max = self.wake_to_end_ns_max.max(wake_to_end_ns);
        if missed {
            self.missed_deadlines += 1;
        }
    }

    pub fn record_release(&mut self, retained_frames: u64) {
        self.buffers_released += 1;
        self.retention_frames_total += retained_frames;
        self.retention_frames_max = self.retention_frames_max.max(retained_frames);
    }

    pub fn render(&self, now_ns: u64) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "frames={}", self.frames);
        let _ = writeln!(s, "frames_rendered={}", self.frames_rendered);
        let _ = writeln!(s, "missed_deadlines={}", self.missed_deadlines);
        let _ = writeln!(s, "gpu_us_mean={}", if self.frames_rendered > 0 { self.gpu_ns_total / self.frames_rendered / 1000 } else { 0 });
        let _ = writeln!(s, "gpu_us_max={}", self.gpu_ns_max / 1000);
        let _ = writeln!(s, "wake_to_end_us_mean={}", if self.frames > 0 { self.wake_to_end_ns_total / self.frames / 1000 } else { 0 });
        let _ = writeln!(s, "wake_to_end_us_max={}", self.wake_to_end_ns_max / 1000);
        let _ = writeln!(s, "shm_uploads={}", self.shm_uploads);
        let _ = writeln!(s, "dmabuf_imports={}", self.dmabuf_imports);
        let _ = writeln!(s, "dmabuf_cpu_copies={}", self.dmabuf_cpu_copies);
        let _ = writeln!(s, "buffers_released={}", self.buffers_released);
        let _ = writeln!(s, "retention_frames_mean_x100={}", if self.buffers_released > 0 { self.retention_frames_total * 100 / self.buffers_released } else { 0 });
        let _ = writeln!(s, "retention_frames_max={}", self.retention_frames_max);
        let _ = writeln!(s, "frame_callbacks={}", self.frame_callbacks);
        let _ = writeln!(s, "commits={}", self.commits);
        let _ = writeln!(s, "toplevels_mapped={}", self.toplevels_mapped);
        let _ = writeln!(s, "toplevels_unmapped={}", self.toplevels_unmapped);
        let _ = writeln!(s, "popups={}", self.popups);
        let _ = writeln!(s, "focus_changes={}", self.focus_changes);
        let _ = writeln!(s, "xwayland_toplevels={}", self.xwayland_toplevels);
        let _ = writeln!(s, "fences_signalled={}", self.fences_signalled);
        let _ = writeln!(s, "fences_outstanding={}", self.fences_outstanding);
        let _ = writeln!(s, "stale_texture_draws={}", self.stale_texture_draws);
        let _ = writeln!(s, "uptime_ms={}", now_ns.saturating_sub(self.started_at_ns) / 1_000_000);
        s
    }
}
